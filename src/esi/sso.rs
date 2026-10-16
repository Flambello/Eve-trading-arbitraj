use crate::error::ErrorCode;
use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ring::digest::{digest, SHA256};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub const DEFAULT_PUBLIC_CLIENT_ID: &str = "5793fc7aa2cb49e385d3cfb5d52061c7";
pub const DEFAULT_CALLBACK_PORT: u16 = 37373;
const SSO_AUTH_URL: &str = "https://login.eveonline.com/v2/oauth/authorize";
const SSO_TOKEN_URL: &str = "https://login.eveonline.com/v2/oauth/token";
const ESI_WAYPOINT_URL: &str = "https://esi.evetech.net/v2/ui/autopilot/waypoint";
const ESI_SCOPES: &str =
    "esi-ui.write_waypoint.v1 esi-location.read_location.v1 esi-location.read_ship_type.v1";
pub const USER_AGENT_VAL: &str =
    "eve-trade-scout/0.1.0 (https://github.com/Flambello/eve-traveling-merchant; contact: Flambello)";

/// Persisted ESI SSO Character Token & Metadata.
#[derive(Clone, Serialize, Deserialize)]
pub struct SsoToken {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_at: i64, // Unix timestamp (seconds)
    pub character_id: u64,
    pub character_name: String,
    #[serde(default)]
    pub client_id: Option<String>,
}

impl std::fmt::Debug for SsoToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SsoToken")
            .field("access_token", &"[REDACTED]")
            .field("refresh_token", &"[REDACTED]")
            .field("token_type", &self.token_type)
            .field("expires_at", &self.expires_at)
            .field("character_id", &self.character_id)
            .field("character_name", &self.character_name)
            .field("client_id", &self.client_id)
            .finish()
    }
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

    /// Returns true if the token needs proactive refresh (within 300 seconds / 5 minutes of expiration).
    pub fn needs_refresh(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        now >= (self.expires_at - 300)
    }

    /// Returns the number of seconds until token expiration (or 0 if expired).
    pub fn seconds_until_expiration(&self) -> i64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        (self.expires_at - now).max(0)
    }
}

/// Returns the path to the stored SSO token file (~/.config/eve-traveling-merchant/sso_token.json).
/// Falls back to legacy ~/.config/eve-trade-scout/sso_token.json if present.
pub fn token_storage_path() -> PathBuf {
    let base = crate::config::config_base_dir().unwrap_or_else(|| PathBuf::from("."));
    let new_path = base.join("eve-traveling-merchant").join("sso_token.json");
    if new_path.exists() {
        return new_path;
    }
    let legacy_path = base.join("eve-trade-scout").join("sso_token.json");
    if legacy_path.exists() {
        return legacy_path;
    }
    let target_dir = base.join("eve-traveling-merchant");
    let _ = std::fs::create_dir_all(&target_dir);
    #[cfg(unix)]
    {
        if let Ok(meta) = std::fs::metadata(&target_dir) {
            let mut perms = meta.permissions();
            perms.set_mode(0o700);
            let _ = std::fs::set_permissions(&target_dir, perms);
        }
    }
    new_path
}

/// Loads the persisted SSO token if available.
pub fn load_saved_token() -> Option<SsoToken> {
    let path = token_storage_path();
    if !path.exists() {
        return None;
    }
    let data = std::fs::read_to_string(path).ok()?;
    let mut token: SsoToken = serde_json::from_str(&data).ok()?;

    // Auto-backfill legacy tokens saved without client_id so future background refreshes succeed
    if token
        .client_id
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        let default_or_dev =
            get_developer_client_id().unwrap_or_else(|| DEFAULT_PUBLIC_CLIENT_ID.to_string());
        token.client_id = Some(default_or_dev);
        let _ = save_token(&token);
    }

    Some(token)
}

/// Persists the SSO token to disk with restricted permissions (0600 on Unix) to prevent local reading of OAuth tokens.
pub fn save_token(token: &SsoToken) -> Result<()> {
    let res = (|| -> Result<()> {
        let path = token_storage_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
            #[cfg(unix)]
            {
                if let Ok(meta) = std::fs::metadata(parent) {
                    let mut perms = meta.permissions();
                    perms.set_mode(0o700);
                    let _ = std::fs::set_permissions(parent, perms);
                }
            }
        }
        let data = serde_json::to_string_pretty(token)?;
        #[cfg(unix)]
        {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)?;
            let mut perms = file.metadata()?.permissions();
            perms.set_mode(0o600);
            let _ = file.set_permissions(perms);
            file.write_all(data.as_bytes())?;
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&path, data)?;
        }
        Ok(())
    })();
    if let Err(ref e) = res {
        tracing::error!(
            error_code = %ErrorCode::ErrSsoTokenSave,
            error = %e,
            "Failed to securely save SSO token file"
        );
    }
    res
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
    // 3. Private local config file (~/.config/eve-traveling-merchant/developer_client_id or legacy ~/.config/eve-trade-scout/developer_client_id)
    let base = crate::config::config_base_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let new_path = base
        .join("eve-traveling-merchant")
        .join("developer_client_id");
    if let Ok(content) = std::fs::read_to_string(&new_path) {
        let trimmed = content.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    let legacy_path = base.join("eve-trade-scout").join("developer_client_id");
    if let Ok(content) = std::fs::read_to_string(&legacy_path) {
        let trimmed = content.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    None
}

/// Validates that an EVE Application Client ID matches safe alphanumeric and hyphen/underscore formatting.
pub fn is_valid_client_id(client_id: &str) -> bool {
    let trimmed = client_id.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 128
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Sanitizes an upstream error response from the SSO token endpoint by stripping tokens and credentials.
pub fn sanitize_sso_error(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("access_token")
        || lower.contains("refresh_token")
        || lower.contains("code_verifier")
        || lower.contains("bearer")
    {
        "OAuth token endpoint error (details withheld for security)".to_string()
    } else {
        let truncated: String = raw.chars().take(256).collect();
        truncated.replace(['\r', '\n'], " ")
    }
}

/// Sanitizes an authorization URL for safe logging by masking the sensitive CSRF state parameter.
pub fn sanitize_auth_url(url: &str) -> String {
    if let Some(state_idx) = url.find("state=") {
        let rest = &url[state_idx + 6..];
        let end_idx = rest.find('&').unwrap_or(rest.len());
        format!("{}state=[REDACTED]{}", &url[..state_idx], &rest[end_idx..])
    } else {
        url.to_string()
    }
}

/// Constant-time byte slice equality comparison to mitigate timing side-channel attacks.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Resolves the effective Client ID: if `custom` is non-empty, use it; otherwise check
/// private local sources (env var, config file), falling back to the built-in public Client ID.
pub fn resolve_client_id(custom: &str) -> Result<String> {
    let trimmed = custom.trim();
    if !trimmed.is_empty() {
        if !is_valid_client_id(trimmed) {
            bail!("Invalid EVE Application Client ID format. Must contain only alphanumeric characters, dashes, or underscores.");
        }
        return Ok(trimmed.to_string());
    }
    if let Some(id) = get_developer_client_id() {
        if !is_valid_client_id(&id) {
            bail!("Invalid developer Client ID format in local configuration.");
        }
        return Ok(id);
    }
    Ok(DEFAULT_PUBLIC_CLIENT_ID.to_string())
}

/// Opens the specified URL in the system's default web browser across platforms.
pub fn open_browser(url: &str) -> Result<()> {
    tracing::info!(
        url = %sanitize_auth_url(url),
        "Opening browser for EVE SSO authentication (or navigate manually to this URL)"
    );

    #[cfg(target_os = "windows")]
    {
        // On Windows, use rundll32 url.dll,FileProtocolHandler to directly invoke ShellExecuteEx
        // without cmd.exe shell interpretation stripping query parameters (&).
        std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn()
            .context("Failed to spawn Windows browser command (rundll32 url.dll,FileProtocolHandler)")?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .context("Failed to spawn macOS browser command (open)")?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .context("Failed to spawn Linux browser command (xdg-open)")?;
    }

    Ok(())
}

/// Builds the browser authorization URL for CCP SSO.
pub fn build_authorize_url(
    client_id: &str,
    callback_port: u16,
    state: &str,
    code_challenge: &str,
) -> String {
    let redirect_uri = format!("http://localhost:{}/callback", callback_port);
    let encoded_client = urlencoding_simple(client_id);
    let encoded_redirect = urlencoding_simple(&redirect_uri);
    let encoded_scopes = urlencoding_simple(ESI_SCOPES);
    let encoded_state = urlencoding_simple(state);
    let encoded_challenge = urlencoding_simple(code_challenge);

    format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        SSO_AUTH_URL, encoded_client, encoded_redirect, encoded_scopes, encoded_state, encoded_challenge
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

/// Basic URL decoding helper for percent-encoded query parameters.
pub fn urldecode_simple(s: &str) -> String {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(val) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or_default(),
                16,
            ) {
                out.push(val);
                i += 3;
                continue;
            }
        } else if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Listens on 127.0.0.1:<port> for the OAuth callback from the browser.
/// Returns the authorization code once received.
pub async fn listen_for_auth_code(
    port: u16,
    expected_state: &str,
    timeout_duration: std::time::Duration,
) -> Result<String> {
    let listener = match TcpListener::bind(("127.0.0.1", port)).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(
                error_code = %ErrorCode::ErrSsoAuthServerFailed,
                port = port,
                error = %e,
                "Failed to bind local loopback listener for SSO OAuth callback"
            );
            if e.kind() == std::io::ErrorKind::AddrInUse {
                bail!(
                    "Local port {} is already in use by another application (e.g. another instance of EVE Trade Scout). Please close conflicting applications or configure an alternate callback port.",
                    port
                );
            }
            return Err(e).with_context(|| {
                format!("Failed to bind local loopback listener on port {}", port)
            });
        }
    };

    let listen_fut = async {
        loop {
            let (mut socket, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    continue;
                }
            };

            let mut request_buf = Vec::with_capacity(2048);
            const MAX_REQUEST_SIZE: usize = 8192;

            // Bounded per-connection read timeout to prevent slowloris connection starvation
            let read_fut = async {
                let mut temp_buf = [0u8; 1024];
                loop {
                    let n = match socket.read(&mut temp_buf).await {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(_) => break,
                    };
                    request_buf.extend_from_slice(&temp_buf[..n]);
                    if request_buf.windows(4).any(|w| w == b"\r\n\r\n")
                        || request_buf.len() >= MAX_REQUEST_SIZE
                    {
                        break;
                    }
                }
                Some(())
            };

            if tokio::time::timeout(std::time::Duration::from_secs(5), read_fut)
                .await
                .is_err()
                || request_buf.is_empty()
            {
                continue;
            }

            let request_str = String::from_utf8_lossy(&request_buf);
            let first_line = request_str.lines().next().unwrap_or_default();

            // RFC 8252: Ensure request starts strictly with GET /callback?
            if !first_line.starts_with("GET /callback?") {
                let err_body = "Invalid callback path or HTTP method.";
                let response = format!(
                    "HTTP/1.1 400 Bad Request\r\n\
                     Content-Type: text/plain; charset=utf-8\r\n\
                     Content-Length: {}\r\n\
                     X-Frame-Options: DENY\r\n\
                     Content-Security-Policy: default-src 'none'; frame-ancestors 'none'\r\n\
                     X-Content-Type-Options: nosniff\r\n\
                     Cache-Control: no-store, no-cache, must-revalidate\r\n\
                     Connection: close\r\n\r\n{}",
                    err_body.len(),
                    err_body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                continue;
            }

            // RFC 8252 Section 8.8: Validate Host header targets localhost or 127.0.0.1 to mitigate DNS rebinding
            let has_valid_host = request_str.lines().any(|l| {
                let lower = l.to_ascii_lowercase();
                lower.starts_with("host: localhost") || lower.starts_with("host: 127.0.0.1")
            });
            if !has_valid_host {
                let err_body = "Invalid Host header.";
                let response = format!(
                    "HTTP/1.1 400 Bad Request\r\n\
                     Content-Type: text/plain; charset=utf-8\r\n\
                     Content-Length: {}\r\n\
                     X-Frame-Options: DENY\r\n\
                     Content-Security-Policy: default-src 'none'; frame-ancestors 'none'\r\n\
                     X-Content-Type-Options: nosniff\r\n\
                     Cache-Control: no-store, no-cache, must-revalidate\r\n\
                     Connection: close\r\n\r\n{}",
                    err_body.len(),
                    err_body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                continue;
            }

            let query_start = first_line.find("/callback?").unwrap();
            let query = &first_line[query_start + 10..];
            let query_end = query.find(' ').unwrap_or(query.len());
            let query_str = &query[..query_end];

            let mut code = None;
            let mut state = None;
            let mut oauth_error = None;
            let mut oauth_error_desc = None;

            for pair in query_str.split('&') {
                let mut parts = pair.split('=');
                if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                    if k == "code" {
                        code = Some(urldecode_simple(v));
                    } else if k == "state" {
                        state = Some(urldecode_simple(v));
                    } else if k == "error" {
                        oauth_error = Some(urldecode_simple(v));
                    } else if k == "error_description" {
                        oauth_error_desc = Some(urldecode_simple(v));
                    }
                }
            }

            // Immediately handle user cancellation or denial without hanging for 3 minutes
            if let Some(err) = oauth_error {
                let desc = oauth_error_desc
                    .unwrap_or_else(|| "Authorization was cancelled or denied by user".to_string());
                let cancel_html = format!(
                    r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><title>EVE Trade Scout - Authorization Cancelled</title><style>
  body {{ background: #0c1424; color: #e0e8f5; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0; }}
  .card {{ background: #141f36; border: 1.5px solid #ff5555; border-radius: 12px; padding: 40px; text-align: center; max-width: 480px; box-shadow: 0 10px 40px rgba(0,0,0,0.6); }}
  h1 {{ color: #ff5555; margin-top: 0; font-size: 24px; }}
  p {{ color: #8ca5cb; line-height: 1.6; font-size: 15px; }}
</style></head>
<body>
  <div class="card">
    <h1>❌ Authorization Cancelled</h1>
    <p>EVE Online SSO authorization was cancelled or denied.<br><br>{}<br><br>You can close this tab and return to the application.</p>
  </div>
</body>
</html>"#,
                    desc
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\n\
                     Content-Type: text/html; charset=utf-8\r\n\
                     Content-Length: {}\r\n\
                     X-Frame-Options: DENY\r\n\
                     Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; frame-ancestors 'none'\r\n\
                     X-Content-Type-Options: nosniff\r\n\
                     Cache-Control: no-store, no-cache, must-revalidate\r\n\
                     Connection: close\r\n\r\n{}",
                    cancel_html.len(),
                    cancel_html
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.flush().await;
                let _ = socket.shutdown().await;

                bail!("EVE SSO authorization rejected by user: {} ({})", err, desc);
            }

            if let (Some(c), Some(s)) = (code, state) {
                // Constant-time state comparison to mitigate timing side channels
                if constant_time_eq(s.as_bytes(), expected_state.as_bytes()) {
                    // Send success HTML with defensive security headers
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
                        "HTTP/1.1 200 OK\r\n\
                         Content-Type: text/html; charset=utf-8\r\n\
                         Content-Length: {}\r\n\
                         X-Frame-Options: DENY\r\n\
                         Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; frame-ancestors 'none'\r\n\
                         X-Content-Type-Options: nosniff\r\n\
                         Cache-Control: no-store, no-cache, must-revalidate\r\n\
                         Connection: close\r\n\r\n{}",
                        html.len(),
                        html
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.flush().await;
                    let _ = socket.shutdown().await;

                    return Ok(c);
                }
            }

            // Fallback 400 response for invalid requests or state mismatch with defensive security headers
            let err_body = "Invalid authorization callback or state mismatch.";
            let response = format!(
                "HTTP/1.1 400 Bad Request\r\n\
                 Content-Type: text/plain; charset=utf-8\r\n\
                 Content-Length: {}\r\n\
                 X-Frame-Options: DENY\r\n\
                 Content-Security-Policy: default-src 'none'; frame-ancestors 'none'\r\n\
                 X-Content-Type-Options: nosniff\r\n\
                 Cache-Control: no-store, no-cache, must-revalidate\r\n\
                 Connection: close\r\n\r\n{}",
                err_body.len(),
                err_body
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.flush().await;
            let _ = socket.shutdown().await;
        }
    };

    tokio::time::timeout(timeout_duration, listen_fut)
        .await
        .map_err(|_| anyhow::anyhow!("SSO authorization timed out waiting for browser callback."))?
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

/// Exchanges the PKCE authorization code for an access token and refresh token.
pub async fn exchange_code_for_token(
    client: &reqwest::Client,
    client_id: &str,
    code: &str,
    code_verifier: &str,
    callback_port: u16,
) -> Result<SsoToken> {
    exchange_code_for_token_at_url(
        client,
        SSO_TOKEN_URL,
        client_id,
        code,
        code_verifier,
        callback_port,
    )
    .await
}

/// Exchanges the PKCE authorization code for an access token and refresh token against the specified token endpoint URL.
pub async fn exchange_code_for_token_at_url(
    client: &reqwest::Client,
    token_url: &str,
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
        .post(token_url)
        .header("User-Agent", USER_AGENT_VAL)
        .form(&params)
        .send()
        .await
        .context("Failed to send token exchange request to CCP SSO")?;

    if !resp.status().is_success() {
        let err_text = resp.text().await.unwrap_or_default();
        let sanitized = sanitize_sso_error(&err_text);
        tracing::error!(
            error_code = %ErrorCode::ErrSsoTokenExchange,
            error = %sanitized,
            "CCP SSO token exchange failed"
        );
        bail!("CCP SSO token exchange failed: {}", sanitized);
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
        .context("Failed to extract valid character claims from SSO access token")?;

    let refresh_token = token_data
        .refresh_token
        .filter(|s| !s.trim().is_empty())
        .context("Missing refresh token in authorization response from CCP SSO")?;

    let sso_token = SsoToken {
        access_token: token_data.access_token,
        refresh_token,
        token_type: token_data.token_type,
        expires_at,
        character_id,
        character_name,
        client_id: Some(client_id.to_string()),
    };

    save_token(&sso_token)?;
    Ok(sso_token)
}

/// Refreshes the access token using the refresh token if expired or nearing expiration.
pub async fn ensure_fresh_token(
    client: &reqwest::Client,
    client_id: &str,
    token: &mut SsoToken,
) -> Result<()> {
    ensure_fresh_token_at_url(client, SSO_TOKEN_URL, client_id, token).await
}

/// Refreshes the access token using the refresh token against the specified token endpoint URL.
pub async fn ensure_fresh_token_at_url(
    client: &reqwest::Client,
    token_url: &str,
    client_id: &str,
    token: &mut SsoToken,
) -> Result<()> {
    if !token.needs_refresh() {
        return Ok(());
    }

    let effective_client_id: String = if !client_id.trim().is_empty() {
        resolve_client_id(client_id)?
    } else if let Some(cid) = token.client_id.as_deref().filter(|s| !s.trim().is_empty()) {
        resolve_client_id(cid)?
    } else {
        resolve_client_id("")?
    };

    let params = [
        ("grant_type", "refresh_token"),
        ("refresh_token", &token.refresh_token),
        ("client_id", &effective_client_id),
    ];

    let resp = client
        .post(token_url)
        .header("User-Agent", USER_AGENT_VAL)
        .form(&params)
        .send()
        .await
        .context("Failed to refresh SSO access token")?;

    if !resp.status().is_success() {
        let err_text = resp.text().await.unwrap_or_default();
        if err_text.contains("invalid_grant") {
            let _ = delete_saved_token();
            tracing::error!(
                error_code = %ErrorCode::ErrSsoTokenRefresh,
                "EVE SSO session has expired or was revoked"
            );
            bail!("EVE SSO session has expired or was revoked. Please log in again.");
        }
        let sanitized = sanitize_sso_error(&err_text);
        tracing::error!(
            error_code = %ErrorCode::ErrSsoTokenRefresh,
            error = %sanitized,
            "CCP SSO token refresh failed"
        );
        bail!("CCP SSO token refresh failed: {}", sanitized);
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
    token.token_type = token_data.token_type;
    // Under RFC 6749 and CCP SSO v2, refresh_token is omitted on refresh unless rotation is active.
    // If a new refresh token is provided, adopt it; otherwise preserve the existing valid refresh token.
    if let Some(new_rt) = token_data.refresh_token.filter(|s| !s.trim().is_empty()) {
        token.refresh_token = new_rt;
    }
    token.expires_at = now + token_data.expires_in;
    token.client_id = Some(effective_client_id);

    save_token(token)?;
    Ok(())
}

/// Proactively refreshes the saved token if it needs refresh (within 5 minutes of expiration or expired).
pub async fn refresh_saved_token_if_needed(
    client: &reqwest::Client,
    client_id: &str,
) -> Result<Option<SsoToken>> {
    let mut token = match load_saved_token() {
        Some(t) => t,
        None => return Ok(None),
    };

    if !token.needs_refresh() {
        return Ok(Some(token));
    }

    ensure_fresh_token(client, client_id, &mut token).await?;
    Ok(Some(token))
}

/// Decodes the payload segment of a JWT token to extract `name` and `sub` (CHARACTER:EVE:<id>).
pub fn parse_jwt_character_claims(jwt: &str) -> Option<(u64, String)> {
    let mut parts = jwt.split('.');
    let _header = parts.next()?;
    let payload_b64 = parts.next()?;

    // Support both unpadded and padded base64url payloads
    let decoded_bytes = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .or_else(|_| {
            let pad_len = (4 - (payload_b64.len() % 4)) % 4;
            let padded = format!("{}{}", payload_b64, "=".repeat(pad_len));
            base64::engine::general_purpose::URL_SAFE.decode(&padded)
        })
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&decoded_bytes).ok()?;

    let name = v
        .get("name")
        .and_then(|n| n.as_str())
        .filter(|s| !s.trim().is_empty())?
        .to_string();

    let character_id = v
        .get("sub")
        .and_then(|s| s.as_str())
        .and_then(|sub_str| {
            // sub format: "CHARACTER:EVE:12345678"
            sub_str.split(':').next_back()?.parse::<u64>().ok()
        })
        .filter(|&id| id > 0)?;

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

        let mut attempts = 0u32;
        let mut backoff = std::time::Duration::from_millis(500);
        let resp = loop {
            attempts += 1;
            let resp_res = client
                .post(&url)
                .bearer_auth(access_token)
                .header("User-Agent", USER_AGENT_VAL)
                .send()
                .await;

            match resp_res {
                Ok(r) => {
                    let status = r.status();
                    if (status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.as_u16() == 420)
                        && attempts <= 3
                    {
                        let wait_dur = r
                            .headers()
                            .get("x-esi-error-limit-reset")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|s| s.parse::<u64>().ok())
                            .map(|s| std::time::Duration::from_secs(s.max(1) + 1))
                            .unwrap_or(backoff);
                        tracing::warn!(
                            error_code = %ErrorCode::ErrEsiRateLimited,
                            destination_id = dest_id,
                            wait = ?wait_dur,
                            "Waypoint injection rate-limited (HTTP {}). Backing off...",
                            status
                        );
                        tokio::time::sleep(wait_dur).await;
                        backoff = (backoff * 2).min(std::time::Duration::from_secs(10));
                        continue;
                    }
                    break r;
                }
                Err(e) => {
                    if attempts <= 3 {
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(std::time::Duration::from_secs(10));
                        continue;
                    }
                    return Err(e).with_context(|| {
                        format!("Failed to send waypoint for destination_id {}", dest_id)
                    });
                }
            }
        };

        if !resp.status().is_success() && resp.status().as_u16() != 204 {
            let status = resp.status();
            let err = resp.text().await.unwrap_or_default();
            let sanitized = sanitize_sso_error(&err);
            tracing::error!(
                error_code = %ErrorCode::ErrSsoAutopilotFailed,
                destination_id = dest_id,
                error = %sanitized,
                "ESI rejected waypoint injection into client HUD"
            );
            if status.as_u16() == 401 {
                bail!("EVE character authorization session has expired or was revoked (HTTP 401). Please re-connect your character in Settings.");
            }
            bail!(
                "ESI rejected waypoint {} (destination {}): {}",
                idx + 1,
                dest_id,
                sanitized
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

    let auth_url =
        build_authorize_url(&effective_client_id, callback_port, &state, &code_challenge);

    // Attempt to open the default system browser across platforms
    if let Err(e) = open_browser(&auth_url) {
        tracing::warn!(
            error = %e,
            url = %sanitize_auth_url(&auth_url),
            "Could not open system browser automatically; navigate to the URL manually in your browser"
        );
    }

    let code =
        listen_for_auth_code(callback_port, &state, std::time::Duration::from_secs(180)).await?;

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
    let mut token = load_saved_token()
        .context("No EVE SSO character authorization found. Please connect your character in Settings first.")?;

    ensure_fresh_token(client, client_id, &mut token).await?;
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
        let sanitized = sanitize_sso_error(&body);
        tracing::error!(
            error_code = %ErrorCode::ErrSsoLocationFailed,
            status = %status,
            character_id = token.character_id,
            error = %sanitized,
            "Failed to query live character location from ESI"
        );
        if status.as_u16() == 401 {
            bail!("EVE character authorization session has expired or was revoked (HTTP 401). Please re-connect your character in Settings.");
        }
        if status.as_u16() == 403 {
            bail!("Missing required scope 'esi-location.read_location.v1'. Please re-connect character in Settings.");
        }
        bail!(
            "ESI character location request failed ({}): {}",
            status,
            sanitized
        );
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
    let mut token = load_saved_token()
        .context("No EVE SSO character authorization found. Please connect your character in Settings first.")?;

    ensure_fresh_token(client, client_id, &mut token).await?;
    fetch_character_location(client, &token).await
}
