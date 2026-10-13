use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
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
pub const MAX_PAGES_LIMIT: u32 = 500;
pub const MAX_NETWORK_RETRIES: u32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEtag {
    pub etag: String,
    pub expires: Option<chrono::DateTime<chrono::Utc>>,
    pub body_json: String,
}

impl StoredEtag {
    /// Checks whether this cached entry is still fresh (unexpired).
    /// Entries with no explicit expiration or whose expiration time is in the past are considered stale.
    pub fn is_fresh(&self) -> bool {
        match self.expires {
            Some(exp) => chrono::Utc::now() < exp,
            None => false,
        }
    }
}

/// Parses the HTTP `Expires` header string (RFC 2822 format), or falls back to `default_ttl_secs` from now.
pub fn parse_http_expires_str(
    header_val: Option<&str>,
    default_ttl_secs: i64,
) -> chrono::DateTime<chrono::Utc> {
    if let Some(val) = header_val {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(val) {
            return dt.with_timezone(&chrono::Utc);
        }
    }
    chrono::Utc::now() + chrono::Duration::seconds(default_ttl_secs)
}

/// Parses the HTTP `Expires` header (RFC 2822 format), or falls back to `default_ttl_secs` from now.
pub fn parse_http_expires(resp: &Response, default_ttl_secs: i64) -> chrono::DateTime<chrono::Utc> {
    let header_val = resp.headers().get("expires").and_then(|v| v.to_str().ok());
    parse_http_expires_str(header_val, default_ttl_secs)
}

#[derive(Clone)]
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
            let mut map: HashMap<String, StoredEtag> =
                serde_json::from_str(&data).unwrap_or_default();
            // Prune expired or stale entries upon loading to avoid serving dead market orders
            map.retain(|_, entry| entry.is_fresh());
            map
        } else {
            HashMap::new()
        };

        Ok(Self {
            client,
            cache_dir,
            etags: Arc::new(Mutex::new(etags)),
            concurrency_sem: Arc::new(Semaphore::new(6)),
        })
    }

    pub async fn save_cache(&self) -> Result<()> {
        let etags_file = self.cache_dir.join("etags.json");
        let mut map = self.etags.lock().await;
        // Prune stale or expired entries before persisting to disk to prevent disk bloat
        map.retain(|_, entry| entry.is_fresh());
        let serialized = serde_json::to_string(&*map)?;
        tokio::fs::write(etags_file, serialized).await?;
        Ok(())
    }


    /// Fetches all paginated items from an ESI endpoint.
    /// Checks `x-pages` header on page 1 and fetches subsequent pages concurrently.
    /// Respects the ESI `Expires` header (300s TTL). When fresh, returns cached data.
    pub async fn get_all_pages<T: Serialize + DeserializeOwned + Send + 'static>(
        &self,
        endpoint: &str,
        extra_query: &[(&str, String)],
    ) -> Result<Vec<T>> {
        self.get_all_pages_opts(endpoint, extra_query, false).await
    }

    /// Force-fetches all paginated items fresh, bypassing any cached response.
    pub async fn get_all_pages_fresh<T: Serialize + DeserializeOwned + Send + 'static>(
        &self,
        endpoint: &str,
        extra_query: &[(&str, String)],
    ) -> Result<Vec<T>> {
        self.get_all_pages_opts(endpoint, extra_query, true).await
    }

    /// Fetches all paginated items from an ESI endpoint with configurable force_refresh.
    pub async fn get_all_pages_opts<T: Serialize + DeserializeOwned + Send + 'static>(
        &self,
        endpoint: &str,
        extra_query: &[(&str, String)],
        force_refresh: bool,
    ) -> Result<Vec<T>> {
        let url = if endpoint.starts_with("http") {
            endpoint.to_string()
        } else {
            format!("{}{}", ESI_BASE, endpoint)
        };

        let cache_key = if extra_query.is_empty() {
            format!("{}:all", url)
        } else {
            let q_str = extra_query
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("&");
            format!("{}:all?{}", url, q_str)
        };

        if !force_refresh {
            let mut map = self.etags.lock().await;
            if let Some(entry) = map.get(&cache_key) {
                if entry.is_fresh() {
                    let parsed: Vec<T> = serde_json::from_str(&entry.body_json)
                        .context("Failed to deserialize cached paginated ESI response")?;
                    return Ok(parsed);
                } else {
                    map.remove(&cache_key);
                }
            }
        } else {
            let mut map = self.etags.lock().await;
            map.remove(&cache_key);
        }

        // Fetch page 1 fresh (no IF_NONE_MATCH to avoid false 304s from Page 1 alone)
        let mut p1_query = extra_query.to_vec();
        p1_query.push(("page", "1".to_string()));

        let mut backoff = Duration::from_millis(500);
        let mut attempts = 0u32;
        let resp = loop {
            attempts += 1;
            let _permit = self.concurrency_sem.acquire().await?;
            let mut req = self.client.get(&url);
            for (k, v) in &p1_query {
                req = req.query(&[(*k, v)]);
            }

            match req.send().await {
                Ok(r) => {
                    let status = r.status();
                    if status == StatusCode::TOO_MANY_REQUESTS || status.as_u16() == 420 {
                        if attempts > MAX_NETWORK_RETRIES {
                            bail!(
                                "ESI page 1 rate limited; aborted after {} attempts",
                                attempts
                            );
                        }
                        eprintln!(
                            " [ESI] Page 1 rate limited ({}). Backing off for {:?}...",
                            status, backoff
                        );
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(10));
                        continue;
                    } else if status.is_server_error() {
                        if attempts > MAX_NETWORK_RETRIES {
                            bail!(
                                "ESI page 1 server error ({}); aborted after {} attempts",
                                status,
                                attempts
                            );
                        }
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(10));
                        continue;
                    } else if !status.is_success() {
                        bail!("Failed to fetch page 1: status {}", status);
                    }
                    break r;
                }
                Err(e) => {
                    if attempts > MAX_NETWORK_RETRIES {
                        bail!(
                            "ESI page 1 request failed after {} retries: {}",
                            attempts,
                            e
                        );
                    }
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(10));
                }
            }
        };

        // Proactive error limit throttling if CCP error limit is dwindling
        if let Some(remain) = resp
            .headers()
            .get("x-esi-error-limit-remain")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok())
        {
            if remain < 20 {
                tokio::time::sleep(Duration::from_millis(1000)).await;
            }
        }

        let expires_at = parse_http_expires(&resp, 300);
        let etag_hdr = resp
            .headers()
            .get("etag")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_default();

        let raw_pages = resp
            .headers()
            .get("x-pages")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(1);

        if raw_pages > MAX_PAGES_LIMIT {
            bail!(
                "ESI response exceeded maximum safe pagination limit ({} pages; limit is {})",
                raw_pages,
                MAX_PAGES_LIMIT
            );
        }
        let total_pages = raw_pages;

        let body_text = resp.text().await?;
        let mut all_items: Vec<T> = serde_json::from_str(&body_text)
            .with_context(|| format!("Failed to parse JSON array from {}", url))?;

        if total_pages > 1 {
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
                    let mut page_backoff = Duration::from_millis(500);
                    let mut attempts = 0u32;
                    let text = loop {
                        attempts += 1;
                        let _p = sem.acquire().await.map_err(|e| anyhow::anyhow!("{}", e))?;
                        let mut req = client.get(&u);
                        for (k, v) in &q {
                            req = req.query(&[(k.as_str(), v.as_str())]);
                        }
                        match req.send().await {
                            Ok(r) => {
                                let status = r.status();
                                if status == StatusCode::TOO_MANY_REQUESTS || status.as_u16() == 420
                                {
                                    if attempts > MAX_NETWORK_RETRIES {
                                        bail!(
                                            "Page {} rate limited; aborted after {} attempts",
                                            page,
                                            attempts
                                        );
                                    }
                                    eprintln!(
                                        " [ESI] Page {} rate limited ({}). Retrying in {:?}...",
                                        page, status, page_backoff
                                    );
                                    tokio::time::sleep(page_backoff).await;
                                    page_backoff = (page_backoff * 2).min(Duration::from_secs(10));
                                    continue;
                                } else if status.is_server_error() {
                                    if attempts > MAX_NETWORK_RETRIES {
                                        bail!(
                                            "Page {} server error ({}); aborted after {} attempts",
                                            page,
                                            status,
                                            attempts
                                        );
                                    }
                                    tokio::time::sleep(page_backoff).await;
                                    page_backoff = (page_backoff * 2).min(Duration::from_secs(10));
                                    continue;
                                } else if !status.is_success() {
                                    bail!("Page {} failed with status {}", page, status);
                                }
                                break r.text().await?;
                            }
                            Err(e) => {
                                if attempts > MAX_NETWORK_RETRIES {
                                    bail!(
                                        "Page {} request failed after {} retries: {}",
                                        page,
                                        attempts,
                                        e
                                    );
                                }
                                tokio::time::sleep(page_backoff).await;
                                page_backoff = (page_backoff * 2).min(Duration::from_secs(10));
                            }
                        }
                    };
                    let page_items: Vec<T> = serde_json::from_str(&text)?;
                    Ok::<Vec<T>, anyhow::Error>(page_items)
                }));
            }

            for task in tasks {
                let page_items = task.await??;
                all_items.extend(page_items);
            }
        }

        // Cache total order list under explicit expires timestamp
        if let Ok(serialized) = serde_json::to_string(&all_items) {
            let mut map = self.etags.lock().await;
            map.insert(
                cache_key,
                StoredEtag {
                    etag: etag_hdr,
                    expires: Some(expires_at),
                    body_json: serialized,
                },
            );
        }

        Ok(all_items)
    }
}
