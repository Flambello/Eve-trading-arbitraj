# 10. Centralized Error Codes & Diagnostic Logging

This document defines the standardized error classification taxonomy, structured logging architecture, and diagnostic troubleshooting procedures for EVE Trade Scout.

---

## 1. Logging Architecture Overview

EVE Trade Scout employs a production-grade dual-sink telemetry system powered by [`tracing`](https://crates.io/crates/tracing) and [`tracing-subscriber`](https://crates.io/crates/tracing-subscriber):

1. **Terminal Console Sink (`stdout`)**:
   - Compact, human-readable ANSI-colored log formatting.
   - Respects user terminal capabilities and TUI raw mode guards.
   - Configurable via `RUST_LOG` environment variable (default: `info`).

2. **Persistent Structured File Sink (`logs/app.log.YYYY-MM-DD`)**:
   - Daily rolling file appender managed via [`tracing-appender`](https://crates.io/crates/tracing-appender).
   - Strict newline-delimited JSON format (`.jsonl`).
   - Every event includes ISO 8601 UTC timestamp, log level, span target, source code file and line origin, thread ID, message, and structured key-value diagnostic metadata.
   - File retention is automatically pruned according to retention policy (7 days default).
   - Local isolation: Directory `logs/` and `*.log` files are strictly excluded from Git version control via root `.gitignore`.

3. **Global Crash & Panic Telemetry**:
   - Custom `std::panic::set_hook` intercepts unhandled panics and fatal runtime terminations.
   - Emits structured error events with `error_code = "ERR_SYS_PANIC"`, backtrace strings, and panic payload before terminating.

---

## 2. Structured Log Schema

Log files stored under `logs/app.log.YYYY-MM-DD` adhere to the following schema:

```json
{
  "timestamp": "2026-10-13T18:25:31.402819Z",
  "level": "ERROR",
  "target": "eve_trade_scout",
  "fields": {
    "error_code": "ERR_ESI_NETWORK_FAILED",
    "region_id": 10000002,
    "error": "connection closed before message completed",
    "message": "Network connection dropped while querying ESI orders for region 10000002"
  },
  "file": "src/esi/client.rs",
  "line": 154
}
```

### Standard Fields

| Field | Type | Description |
|---|---|---|
| `timestamp` | String | ISO 8601 UTC timestamp with microsecond precision |
| `level` | String | Event severity: `TRACE`, `DEBUG`, `INFO`, `WARN`, `ERROR` |
| `target` | String | Tracing target module name (usually `eve_trade_scout`) |
| `error_code` | String | Standardized uppercase snake-case identifier (e.g. `ERR_ESI_RATE_LIMITED`) |
| `message` | String | Human-readable event description |
| Context keys | Various | Subsystem context (e.g. `region_id`, `system_id`, `status_code`, `path`) |

---

## 3. Error Code Catalog

Every non-trivial failure condition in the application emits a standardized `ErrorCode` from `src/error.rs`:

### 1000–1999: SDE & Universe Database

| Code ID | Constant Identifier | HTTP/Sys Equivalent | Description | Recommended Resolution |
|---|---|---|---|---|
| `1001` | `ERR_SDE_NOT_FOUND` | `ENOENT` | SDE SQLite database file not found on disk or at configured path. | Verify `data/eve_sde.sqlite` exists or launch with `--update-sde` to download. |
| `1002` | `ERR_SDE_DOWNLOAD_FAILED` | HTTP 5xx / Net | Remote download from CCP/Fuzzwork failed. | Check internet connection or CCP server status; inspect DNS settings. |
| `1003` | `ERR_SDE_DECOMPRESSION_FAILED` | Corrupt | Gzip decompression failed or SQLite magic header invalid. | Delete corrupted `data/eve_sde.sqlite.bz2` or `.sqlite` and re-download. |
| `1004` | `ERR_SDE_DECOMPRESSION_BOMB` | Security | Decompressed payload exceeded safety ceiling (1.5 GB). | Reject archive; verify source URL points to trusted CCP / Fuzzwork repository. |
| `1005` | `ERR_SDE_SCHEMA_INVALID` | Schema mismatch | SQLite schema missing expected tables (`mapSolarSystems`, `mapSolarSystemJumps`). | Rebuild SDE database using supported conversion tools or fresh download. |
| `1006` | `ERR_SDE_QUERY_FAILED` | SQL error | Rusqlite database query execution failed. | Check disk read permissions and database integrity (`PRAGMA integrity_check`). |
| `1007` | `ERR_SDE_SYSTEM_NOT_FOUND` | 404 | Solar system name or ID could not be resolved. | Check spelling in configuration or verify solar system exists in New Eden. |
| `1008` | `ERR_SDE_STATION_NOT_FOUND` | 404 | Station or citadel record could not be resolved. | Verify station ID against live universe data. |

### 2000–2999: CCP ESI REST API & Market

| Code ID | Constant Identifier | HTTP/Sys Equivalent | Description | Recommended Resolution |
|---|---|---|---|---|
| `2001` | `ERR_ESI_CLIENT_INIT` | Config / FS | Failed to initialize HTTP client or cache directory. | Verify write permissions on cache directory (`~/.cache/eve-traveling-merchant/`). |
| `2002` | `ERR_ESI_NETWORK_FAILED` | Timeout / DNS | Network connection dropped, timed out, or host unreachable. | Inspect internet connection; verify `esi.evetech.net` reachability. |
| `2003` | `ERR_ESI_RATE_LIMITED` | HTTP 420 / 429 | ESI rate limit encountered after exponential backoff budget exhausted. | Increase request throttle delay or pause scans until rate limit reset window clears. |
| `2004` | `ERR_ESI_SERVER_ERROR` | HTTP 500 / 502 / 503 / 504 | Upstream CCP ESI cluster server failure. | CCP server maintenance or VIP mode; retry after EVE server status clears. |
| `2005` | `ERR_ESI_CLIENT_ERROR` | HTTP 400 / 403 / 404 | Request rejected by ESI due to invalid client parameter. | Validate region IDs, type IDs, or query parameters sent to API endpoint. |
| `2006` | `ERR_ESI_PARSE_FAILED` | Serde JSON | JSON parsing failed or returned schema mismatch. | Check for breaking ESI API changes or payload truncation. |
| `2007` | `ERR_ESI_ETAG_CACHE` | I/O | Failed to serialize or write ETag cache map to disk. | Check disk space and write permissions for cache file `etag_cache.json`. |

### 3000–3999: EVE Single Sign-On (SSO) & Autopilot

| Code ID | Constant Identifier | HTTP/Sys Equivalent | Description | Recommended Resolution |
|---|---|---|---|---|
| `3001` | `ERR_SSO_CONFIG_INVALID` | 400 | Custom client ID or SSO configuration invalid. | Verify custom client ID contains only alphanumeric characters or remove to use default public ID. |
| `3002` | `ERR_SSO_AUTH_SERVER_FAILED` | `EADDRINUSE` | Loopback TCP listener could not bind for OAuth callback. | Free port `127.0.0.1:8080` or `127.0.0.1:37428` from conflicting processes. |
| `3003` | `ERR_SSO_TOKEN_EXCHANGE` | HTTP 400 / 401 | OAuth authorization code exchange with `login.eveonline.com` failed. | Ensure authorization code hasn't expired and PKCE code verifier is valid. |
| `3004` | `ERR_SSO_TOKEN_REFRESH` | HTTP 400 / 401 | Refresh token rejected or revoked by CCP SSO. | Re-authenticate character via desktop GUI SSO login button. |
| `3005` | `ERR_SSO_TOKEN_SAVE` | I/O / Perms | Failed to save token file or set secure `0600` permissions. | Verify directory permissions on token path (`~/.config/eve-trade-scout/sso_token.json`). |
| `3006` | `ERR_SSO_LOCATION_FAILED` | HTTP 401 / 403 / 404 | Character live location query failed. | Check character online status and ensure `esi-location.read_location.v1` scope is granted. |
| `3007` | `ERR_SSO_AUTOPILOT_FAILED` | HTTP 401 / 403 / 500 | Waypoint injection failed. | Ensure EVE client is running, character is logged in, and `esi-ui.open_window.v1` scope granted. |

### 4000–4999: Stargate Topology & Pathfinding

| Code ID | Constant Identifier | Description | Recommended Resolution |
|---|---|---|---|
| `4001` | `ERR_GRAPH_BUILD_FAILED` | Failed to compile New Eden stargate graph from SDE. | Verify SDE jump tables are populated and undamaged. |
| `4002` | `ERR_GRAPH_ROUTE_UNREACHABLE` | Solar system unreachable under routing constraints (e.g. isolated island). | Disable safe highsec filter or pick an accessible home system. |

### 5000–5999: Arbitrage Engine & Knapsack

| Code ID | Constant Identifier | Description | Recommended Resolution |
|---|---|---|---|
| `5001` | `ERR_ARB_CONFIG_INVALID` | Arbitrage optimizer filter or configuration constraint invalid (<10% ROI). | Ensure minimum ROI $\ge 10.0\%$, positive prices, and valid hauler settings. |
| `5002` | `ERR_ARB_ZERO_CAPACITY` | Ship volume or budget evaluates to zero. | Select valid hauler profile (Big Hauler: 60k m³ / 500M ISK, Small Hauler: 30k m³ / 50M ISK). |

### 6000–6999: Configuration & Persistence

| Code ID | Constant Identifier | Description | Recommended Resolution |
|---|---|---|---|
| `6001` | `ERR_CONFIG_LOAD_FAILED` | Failed to load or deserialize `config.toml`. | Check syntax in `config.toml` or delete file to restore defaults. |
| `6002` | `ERR_CONFIG_SAVE_FAILED` | Failed to serialize or write user `config.toml`. | Verify write permissions and available disk space. |

### 7000–7999: User Interface & Audio

| Code ID | Constant Identifier | Description | Recommended Resolution |
|---|---|---|---|
| `7001` | `ERR_UI_RUNTIME_INIT` | Failed to initialize async runtime, desktop GUI, or TUI. | Verify graphics drivers, display server (`DISPLAY` / `WAYLAND_DISPLAY`), or terminal. |
| `7002` | `ERR_UI_WORKER_CHANNEL` | Background worker thread channel disconnected or thread panicked. | Check logs for preceding panic or worker thread termination. |
| `7003` | `ERR_UI_AUDIO_PLAYBACK` | Sound alert audio output failed (ALSA / PulseAudio / PipeWire). | Verify sound device configuration or disable sound alerts in settings. |
| `7004` | `ERR_UI_CSV_EXPORT` | Failed to write CSV export to disk (deals or manifests). | Ensure destination directory exists, is writable, and is not a symlink. |
| `7005` | `ERR_UI_STARTUP_FAILED` | Application startup initialization sequence failed across one or more subsystems. | Inspect error code badge on splash screen, check logs for root cause, or click 'Enter Offline Mode'. |

### 9000–9999: System & Panic Handling

| Code ID | Constant Identifier | Description | Recommended Resolution |
|---|---|---|---|
| `9001` | `ERR_SYS_PANIC` | Unhandled application panic caught by global panic hook. | Inspect logged backtrace; report issue with `logs/app.log.YYYY-MM-DD`. |

---

## 4. Diagnostics & Troubleshooting Guide

### Startup Failure Recovery & Splash Telemetry

If application startup encounters any fatal error (missing SDE database, offline ESI network, invalid configuration, or unexpected worker panic):
1. **Machine-Readable Badge**: The loading splash screen displays a prominent red `ErrorCode` badge (e.g. `[ERR_SDE_NOT_FOUND]`, `[ERR_UI_WORKER_CHANNEL]`, `[ERR_UI_STARTUP_FAILED]`).
2. **Offline Mode Bypass**: Pilots can click **"Enter Offline Mode"** to enter the workstation, inspect settings, and review cached telemetry without restarting the process.
3. **Supervisor Task Interception**: Background startup worker panics are caught by a dedicated supervisor task and logged with `ERR_SYS_PANIC` before relaying an actionable error card to the GUI.

### Inspecting Live Logs

To tail the current day's structured log file:
```bash
tail -f logs/app.log.$(date +%F)
```

To pretty-print structured JSON log entries using `jq`:
```bash
tail -n 50 logs/app.log.$(date +%F) | jq '.'
```

To filter for error events only:
```bash
grep '"level":"ERROR"' logs/app.log.$(date +%F) | jq '.'
```

To search for specific error codes (e.g. network drops):
```bash
grep 'ERR_ESI_NETWORK_FAILED' logs/app.log.$(date +%F) | jq '.'
```

### Reproducing & Verifying Log Capture

Run the test suite to verify log format and error capture:
```bash
cargo test test_logging -- --nocapture
cargo test test_error_codes -- --nocapture
```
