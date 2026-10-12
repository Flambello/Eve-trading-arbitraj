use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ring::digest::{digest, SHA256};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub const DEFAULT_CALLBACK_PORT: u16 = 37373;
const SSO_AUTH_URL: &str = "https://login.eveonline.com/v2/oauth/authorize";
const SSO_TOKEN_URL: &str = "https://login.eveonline.com/v2/oauth/token";
const ESI_WAYPOINT_URL: &str = "https://esi.evetech.net/v2/ui/autopilot/waypoint";
const ESI_SCOPES: &str = "esi-ui.write_waypoint.v1 esi-location.read_location.v1 esi-location.read_ship_type.v1";
const USER_AGENT_VAL: &str = "eve-trade-scout/0.1.0 (EVE Online Arbitrage Scout)";

/// Persisted ESI SSO Character Token & Metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsoToken {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_at: i64, // Unix timestamp (seconds)
    pub character_id: u64,
    pub character_name: String,
}

impl SsoToken {
    /// Returns true if the token is expired or within 60 seconds of expiration.
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        now >= (self.expires_at - 60)
    }
}

/// Returns the path to the stored SSO token file (~/.config/eve-trade-scout/sso_token.json).
pub fn token_storage_path() -> PathBuf {
    let base = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("eve-trade-scout");
    let _ = std::fs::create_dir_all(&base);
    base.join("sso_token.json")
}

/// Loads the persisted SSO token if available.
pub fn load_saved_token() -> Option<SsoToken> {
    let path = token_storage_path();
    if !path.exists() {
        return None;
    }
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

/// Persists the SSO token to disk.
pub fn save_token(token: &SsoToken) -> Result<()> {
    let path = token_storage_path();
    let data = serde_json::to_string_pretty(token)?;
    std::fs::write(path, data)?;
    Ok(())
}

/// Deletes the persisted SSO token from disk.
pub fn delete_saved_token() -> Result<()> {
    let path = token_storage_path();
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Generates a PKCE code_verifier (64 random chars) and its S256 code_challenge.
pub fn generate_pkce_pair() -> Result<(String, String)> {
    let rng = SystemRandom::new();
    let mut random_bytes = [0u8; 48];
    rng.fill(&mut random_bytes)
        .map_err(|_| anyhow::anyhow!("Failed to generate secure random bytes for PKCE"))?;

    let code_verifier = URL_SAFE_NO_PAD.encode(random_bytes);
    let hash = digest(&SHA256, code_verifier.as_bytes());
    let code_challenge = URL_SAFE_NO_PAD.encode(hash.as_ref());

    Ok((code_verifier, code_challenge))
}

/// Retrieves the developer Client ID from private local sources (env var, compile-time env, or private config).
/// NEVER stored in git to protect the developer from third-party misuse.
pub fn get_developer_client_id() -> Option<String> {
    // 1. Runtime environment variable
    if let Ok(val) = std::env::var("EVE_CLIENT_ID") {
        let trimmed = val.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    // 2. Compile-time environment variable (for release builds)
    if let Some(val) = option_env!("EVE_CLIENT_ID") {
        let trimmed = val.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    // 3. Private local config file (~/.config/eve-trade-scout/developer_client_id)
    let private_path = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("eve-trade-scout")
        .join("developer_client_id");
    if let Ok(content) = std::fs::read_to_string(private_path) {
        let trimmed = content.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    None
}

/// Resolves the effective Client ID: if `custom` is non-empty, use it; otherwise check private local sources.
pub fn resolve_client_id(custom: &str) -> Result<String> {
    let trimmed = custom.trim();
    if !trimmed.is_empty() {
        return Ok(trimmed.to_string());
    }
    if let Some(id) = get_developer_client_id() {
        return Ok(id);
    }
    bail!(
        "No EVE Application Client ID configured for this build. Maintainer credentials are kept private to protect the developer account. Set the EVE_CLIENT_ID environment variable or create ~/.config/eve-trade-scout/developer_client_id to enable SSO."
    )
}

/// Builds the browser authorization URL for CCP SSO.
pub fn build_authorize_url(
    client_id: &str,
    callback_port: u16,
    state: &str,
    code_challenge: &str,
) -> String {
    let redirect_uri = format!("http://localhost:{}/callback", callback_port);
    let encoded_redirect = urlencoding_simple(&redirect_uri);
    let encoded_scopes = urlencoding_simple(ESI_SCOPES);
    let encoded_state = urlencoding_simple(state);
    let encoded_challenge = urlencoding_simple(code_challenge);

    format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        SSO_AUTH_URL, client_id, encoded_redirect, encoded_scopes, encoded_state, encoded_challenge
    )
}

/// Basic URL encoder helper for alphanumeric + safe characters.
fn urlencoding_simple(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Listens on 127.0.0.1:<port> for the OAuth callback from the browser.
/// Returns the authorization code once received.
pub async fn listen_for_auth_code(
    port: u16,
    expected_state: &str,
    timeout_duration: std::time::Duration,
) -> Result<String> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .with_context(|| format!("Failed to bind local loopback listener on port {}", port))?;

    let listen_fut = async {
        loop {
            let (mut socket, _) = listener.accept().await?;
            let mut buf = [0u8; 4096];
            let n = socket.read(&mut buf).await?;
            if n == 0 {
                continue;
            }

            let request_str = String::from_utf8_lossy(&buf[..n]);
            let first_line = request_str.lines().next().unwrap_or_default();

            // Expected format: GET /callback?code=XYZ&state=ABC HTTP/1.1
            if let Some(query_start) = first_line.find("/callback?") {
                let query = &first_line[query_start + 10..];
                let query_end = query.find(' ').unwrap_or(query.len());
                let query_str = &query[..query_end];

                let mut code = None;
                let mut state = None;

                for pair in query_str.split('&') {
                    let mut parts = pair.split('=');
                    if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                        if k == "code" {
                            code = Some(v.to_string());
                        } else if k == "state" {
                            state = Some(v.to_string());
                        }
                    }
                }

                if let (Some(c), Some(s)) = (code, state) {
                    if s == expected_state {
                        // Send success HTML
                        let html = r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><title>EVE Trade Scout - Connected</title><style>
  body { background: #0c1424; color: #e0e8f5; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0; }
  .card { background: #141f36; border: 1.5px solid #00e6aa; border-radius: 12px; padding: 40px; text-align: center; max-width: 480px; box-shadow: 0 10px 40px rgba(0,0,0,0.6); }
  h1 { color: #00e6aa; margin-top: 0; font-size: 24px; }
  p { color: #8ca5cb; line-height: 1.6; font-size: 15px; }
</style></head>
<body>
  <div class="card">
    <h1>✅ Character Connected!</h1>
    <p>EVE Trade Scout has successfully received your authorization.<br><br>You can close this tab and return to the application.</p>
  </div>
</body>
</html>"#;
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            html.len(),
                            html
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                        let _ = socket.flush().await;

                        return Ok(c);
                    }
                }
            }

            // Fallback 400 response for invalid requests
            let err_body = "Invalid authorization callback or state mismatch.";
            let response = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                err_body.len(),
                err_body
            );
            let _ = socket.write_all(response.as_bytes()).await;
        }
    };

    tokio::time::timeout(timeout_duration, listen_fut)
        .await
        .map_err(|_| anyhow::anyhow!("SSO authorization timed out waiting for browser callback."))?
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: i64,
    refresh_token: String,
}

/// Exchanges the PKCE authorization code for an access token and refresh token.
pub async fn exchange_code_for_token(
    client: &reqwest::Client,
    client_id: &str,
    code: &str,
    code_verifier: &str,
    callback_port: u16,
) -> Result<SsoToken> {
    let redirect_uri = format!("http://localhost:{}/callback", callback_port);

    let params = [
        ("grant_type", "authorization_code"),
        ("client_id", client_id),
        ("code", code),
        ("code_verifier", code_verifier),
        ("redirect_uri", &redirect_uri),
    ];

    let resp = client
        .post(SSO_TOKEN_URL)
        .header("User-Agent", USER_AGENT_VAL)
        .form(&params)
        .send()
        .await
        .context("Failed to send token exchange request to CCP SSO")?;

    if !resp.status().is_success() {
        let err_text = resp.text().await.unwrap_or_default();
        bail!("CCP SSO token exchange failed: {}", err_text);
    }

    let token_data: TokenResponse = resp
        .json()
        .await
        .context("Failed to parse SSO token response")?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let expires_at = now + token_data.expires_in;

    // Decode character name and character ID from JWT access token payload
    let (character_id, character_name) = parse_jwt_character_claims(&token_data.access_token)
        .unwrap_or((0, "EVE Pilot".to_string()));

    let sso_token = SsoToken {
        access_token: token_data.access_token,
        refresh_token: token_data.refresh_token,
        token_type: token_data.token_type,
        expires_at,
        character_id,
        character_name,
    };

    save_token(&sso_token)?;
    Ok(sso_token)
}

/// Refreshes the access token using the refresh token if expired.
pub async fn ensure_fresh_token(
    client: &reqwest::Client,
    client_id: &str,
    token: &mut SsoToken,
) -> Result<()> {
    if !token.is_expired() {
        return Ok(());
    }

    let params = [
        ("grant_type", "refresh_token"),
        ("refresh_token", &token.refresh_token),
        ("client_id", client_id),
    ];

    let resp = client
        .post(SSO_TOKEN_URL)
        .header("User-Agent", USER_AGENT_VAL)
        .form(&params)
        .send()
        .await
        .context("Failed to refresh SSO access token")?;

    if !resp.status().is_success() {
        let err_text = resp.text().await.unwrap_or_default();
        bail!("CCP SSO token refresh failed: {}", err_text);
    }

    let token_data: TokenResponse = resp
        .json()
        .await
        .context("Failed to parse refreshed SSO token response")?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    token.access_token = token_data.access_token;
    token.refresh_token = token_data.refresh_token;
    token.expires_at = now + token_data.expires_in;

    let _ = save_token(token);
    Ok(())
}

/// Decodes the payload segment of a JWT token to extract `name` and `sub` (CHARACTER:EVE:<id>).
pub fn parse_jwt_character_claims(jwt: &str) -> Option<(u64, String)> {
    let mut parts = jwt.split('.');
    let _header = parts.next()?;
    let payload_b64 = parts.next()?;

    let decoded_bytes = URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&decoded_bytes).ok()?;

    let name = v
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("EVE Pilot")
        .to_string();

    let character_id = v
        .get("sub")
        .and_then(|s| s.as_str())
        .and_then(|sub_str| {
            // sub format: "CHARACTER:EVE:12345678"
            sub_str.split(':').next_back()?.parse::<u64>().ok()
        })
        .unwrap_or(0);

    Some((character_id, name))
}

/// Injects an ordered sequence of waypoints (station IDs and/or system IDs) directly into the running EVE client.
///
/// For the first waypoint: `clear_other_waypoints = true`
/// For subsequent waypoints: `clear_other_waypoints = false, add_to_beginning = false`
pub async fn push_waypoints_to_client(
    client: &reqwest::Client,
    access_token: &str,
    destination_ids: &[u64],
) -> Result<usize> {
    if destination_ids.is_empty() {
        bail!("No waypoints to inject into EVE client");
    }

    let mut injected_count = 0;

    for (idx, &dest_id) in destination_ids.iter().enumerate() {
        let is_first = idx == 0;
        let url = format!(
            "{}?destination_id={}&add_to_beginning=false&clear_other_waypoints={}",
            ESI_WAYPOINT_URL, dest_id, is_first
        );

        let resp = client
            .post(&url)
            .bearer_auth(access_token)
            .header("User-Agent", USER_AGENT_VAL)
            .send()
            .await
            .with_context(|| format!("Failed to send waypoint for destination_id {}", dest_id))?;

        if !resp.status().is_success() && resp.status().as_u16() != 204 {
            let err = resp.text().await.unwrap_or_default();
            bail!(
                "ESI rejected waypoint {} (destination {}): {}",
                idx + 1,
                dest_id,
                err
            );
        }

        injected_count += 1;

        // Small pause between ESI waypoint calls to prevent burst rate-limiting
        if idx + 1 < destination_ids.len() {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        }
    }

    Ok(injected_count)
}

/// Orchestrates full PKCE login flow: generates keys, launches browser, listens on localhost, exchanges code.
pub async fn authenticate_character_via_pkce(
    client: &reqwest::Client,
    client_id: &str,
    callback_port: u16,
) -> Result<SsoToken> {
    let effective_client_id = resolve_client_id(client_id)?;
    let (code_verifier, code_challenge) = generate_pkce_pair()?;
    let rng = SystemRandom::new();
    let mut state_bytes = [0u8; 16];
    rng.fill(&mut state_bytes)
        .map_err(|_| anyhow::anyhow!("Failed to generate random state for SSO"))?;
    let state = URL_SAFE_NO_PAD.encode(state_bytes);

    let auth_url = build_authorize_url(&effective_client_id, callback_port, &state, &code_challenge);

    // Attempt to open the default system browser via xdg-open (Linux/desktop)
    let _ = std::process::Command::new("xdg-open")
        .arg(&auth_url)
        .spawn();

    let code = listen_for_auth_code(
        callback_port,
        &state,
        std::time::Duration::from_secs(180),
    )
    .await?;

    let token = exchange_code_for_token(
        client,
        &effective_client_id,
        &code,
        &code_verifier,
        callback_port,
    )
    .await?;

    Ok(token)
}

/// Loads the saved token, refreshes it if needed, and pushes waypoints to the live EVE client HUD.
pub async fn push_waypoints_with_token(
    client: &reqwest::Client,
    client_id: &str,
    destination_ids: &[u64],
) -> Result<usize> {
    let effective_client_id = resolve_client_id(client_id)?;
    let mut token = load_saved_token()
        .context("No EVE SSO character authorization found. Please connect your character in Settings first.")?;

    ensure_fresh_token(client, &effective_client_id, &mut token).await?;
    push_waypoints_to_client(client, &token.access_token, destination_ids).await
}

/// Live character location returned by ESI GET /v1/characters/{character_id}/location/
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterLocation {
    pub solar_system_id: u32,
    #[serde(default)]
    pub station_id: Option<u64>,
    #[serde(default)]
    pub structure_id: Option<u64>,
}

/// Queries the live character location from ESI for an authenticated character.
pub async fn fetch_character_location(
    client: &reqwest::Client,
    token: &SsoToken,
) -> Result<CharacterLocation> {
    let url = format!(
        "https://esi.evetech.net/v1/characters/{}/location/",
        token.character_id
    );

    let resp = client
        .get(&url)
        .header("User-Agent", USER_AGENT_VAL)
        .header("Authorization", format!("Bearer {}", token.access_token))
        .send()
        .await
        .context("Failed to query live character location from ESI")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if status.as_u16() == 403 {
            bail!("Missing required scope 'esi-location.read_location.v1'. Please re-connect character in Settings.");
        }
        bail!("ESI character location request failed ({}): {}", status, body);
    }

    let loc: CharacterLocation = resp
        .json()
        .await
        .context("Failed to parse ESI character location JSON")?;

    Ok(loc)
}

/// Loads the saved token, refreshes it if needed, and fetches the character's live location.
pub async fn get_character_location_with_token(
    client: &reqwest::Client,
    client_id: &str,
) -> Result<CharacterLocation> {
    let effective_client_id = resolve_client_id(client_id)?;
    let mut token = load_saved_token()
        .context("No EVE SSO character authorization found. Please connect your character in Settings first.")?;

    ensure_fresh_token(client, &effective_client_id, &mut token).await?;
    fetch_character_location(client, &token).await
}

