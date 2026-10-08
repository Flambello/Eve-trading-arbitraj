#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, IF_NONE_MATCH, USER_AGENT};
use reqwest::{Client, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Semaphore};

const ESI_BASE: &str = "https://esi.evetech.net/latest";
const USER_AGENT_VAL: &str = "eve-trade-scout/0.1.0 (EVE Online Arbitrage Scout)";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredEtag {
    etag: String,
    expires: Option<chrono::DateTime<chrono::Utc>>,
    body_json: String,
}

pub struct EsiClient {
    client: Client,
    cache_dir: PathBuf,
    etags: Arc<Mutex<HashMap<String, StoredEtag>>>,
    concurrency_sem: Arc<Semaphore>,
}

impl EsiClient {
    pub fn new(cache_dir: impl AsRef<Path>) -> Result<Self> {
        let cache_dir = cache_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&cache_dir)?;

        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VAL));

        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .gzip(true)
            .build()?;

        let etags_file = cache_dir.join("etags.json");
        let etags = if etags_file.exists() {
            let data = std::fs::read_to_string(&etags_file).unwrap_or_default();
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            HashMap::new()
        };

        Ok(Self {
            client,
            cache_dir,
            etags: Arc::new(Mutex::new(etags)),
            concurrency_sem: Arc::new(Semaphore::new(10)),
        })
    }

    pub async fn save_cache(&self) -> Result<()> {
        let etags_file = self.cache_dir.join("etags.json");
        let map = self.etags.lock().await;
        let serialized = serde_json::to_string(&*map)?;
        tokio::fs::write(etags_file, serialized).await?;
        Ok(())
    }

    pub async fn get_json<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        query: &[(&str, String)],
    ) -> Result<T> {
        let url = if endpoint.starts_with("http") {
            endpoint.to_string()
        } else {
            format!("{}{}", ESI_BASE, endpoint)
        };

        // Cache key includes URL and query string
        let cache_key = if query.is_empty() {
            url.clone()
        } else {
            let q_str = query
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("&");
            format!("{}?{}", url, q_str)
        };

        let cached_etag = {
            let map = self.etags.lock().await;
            map.get(&cache_key).cloned()
        };

        let _permit = self.concurrency_sem.acquire().await?;

        let mut req = self.client.get(&url);
        for (k, v) in query {
            req = req.query(&[(*k, v)]);
        }

        if let Some(ref entry) = cached_etag {
            req = req.header(IF_NONE_MATCH, entry.etag.as_str());
        }

        let mut backoff = Duration::from_millis(500);
        let resp: Response = loop {
            let req_clone = req.try_clone().context("Failed to clone request")?;
            match req_clone.send().await {
                Ok(r) => {
                    let status = r.status();
                    if status == StatusCode::TOO_MANY_REQUESTS || status.as_u16() == 420 {
                        eprintln!(
                            " [ESI] Rate limit hit ({}). Backing off for {:?}...",
                            status, backoff
                        );
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(60));
                        continue;
                    } else if status.is_server_error() {
                        eprintln!(
                            " [ESI] Server error ({}). Retrying in {:?}...",
                            status, backoff
                        );
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(10));
                        continue;
                    }
                    break r;
                }
                Err(e) => {
                    eprintln!(" [ESI] Network error: {}. Retrying in {:?}...", e, backoff);
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(10));
                }
            }
        };

        if resp.status() == StatusCode::NOT_MODIFIED {
            if let Some(entry) = cached_etag {
                let parsed: T = serde_json::from_str(&entry.body_json)
                    .context("Failed to deserialize cached ESI response")?;
                return Ok(parsed);
            }
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("ESI error {}: {}", status, body);
        }

        let etag_hdr = resp
            .headers()
            .get("etag")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        let text = resp.text().await?;
        let parsed: T = serde_json::from_str(&text)
            .with_context(|| format!("Failed to parse ESI JSON from {}", url))?;

        if let Some(etag) = etag_hdr {
            let mut map = self.etags.lock().await;
            map.insert(
                cache_key,
                StoredEtag {
                    etag,
                    expires: None,
                    body_json: text,
                },
            );
        }

        Ok(parsed)
    }

    /// Fetches all paginated items from an ESI endpoint.
    /// Checks `x-pages` header on page 1 and fetches subsequent pages concurrently.
    pub async fn get_all_pages<T: DeserializeOwned + Send + 'static>(
        &self,
        endpoint: &str,
        extra_query: &[(&str, String)],
    ) -> Result<Vec<T>> {
        let url = if endpoint.starts_with("http") {
            endpoint.to_string()
        } else {
            format!("{}{}", ESI_BASE, endpoint)
        };

        // Fetch page 1
        let mut p1_query = extra_query.to_vec();
        p1_query.push(("page", "1".to_string()));

        let _permit = self.concurrency_sem.acquire().await?;
        let mut req = self.client.get(&url);
        for (k, v) in &p1_query {
            req = req.query(&[(*k, v)]);
        }

        let resp = req.send().await?;
        drop(_permit);

        if !resp.status().is_success() {
            bail!("Failed to fetch page 1: status {}", resp.status());
        }

        let total_pages = resp
            .headers()
            .get("x-pages")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(1);

        let body_text = resp.text().await?;
        let mut all_items: Vec<T> = serde_json::from_str(&body_text)
            .with_context(|| format!("Failed to parse JSON array from {}", url))?;

        if total_pages <= 1 {
            return Ok(all_items);
        }

        // Fetch pages 2..=total_pages
        let mut tasks = Vec::new();
        let owned_query: Vec<(String, String)> = extra_query
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();

        for page in 2..=total_pages {
            let client = self.client.clone();
            let sem = Arc::clone(&self.concurrency_sem);
            let u = url.clone();
            let mut q = owned_query.clone();
            q.push(("page".to_string(), page.to_string()));

            tasks.push(tokio::spawn(async move {
                let _p = sem.acquire().await.map_err(|e| anyhow::anyhow!("{}", e))?;
                let mut req = client.get(&u);
                for (k, v) in &q {
                    req = req.query(&[(k.as_str(), v.as_str())]);
                }
                let r = req.send().await?;
                if !r.status().is_success() {
                    bail!("Page {} failed with status {}", page, r.status());
                }
                let text = r.text().await?;
                let page_items: Vec<T> = serde_json::from_str(&text)?;
                Ok::<Vec<T>, anyhow::Error>(page_items)
            }));
        }

        for task in tasks {
            let page_items = task.await??;
            all_items.extend(page_items);
        }

        Ok(all_items)
    }
}
