# EVE Trade Scout — Diagnostic & Troubleshooting Guide

This guide assists operators, traders, and developers in diagnosing and resolving application errors using structured log files (`logs/app.log`) and console output.

---

## Log File Locations
- **Structured Log File**: `logs/app.log.YYYY-MM-DD` (auto-rotated daily)
- **Live Stream**: Standard console output (`stdout` / `stderr`)
- **Configuring Verbosity**: Set the `RUST_LOG` environment variable:
  ```bash
  RUST_LOG=debug cargo run -- --gui
  RUST_LOG=trace cargo run -- --system Jita
  ```

---

## Major Error Categories & Resolution Matrix

### 1. Static Data Export (SDE) SQLite Errors

#### Description & Root Causes
Occurs when `data/eve_sde.sqlite` is missing, incomplete, corrupted, or schema tables (`staStations`, `mapSolarSystems`, `invTypes`) fail integrity checks.

#### Log Representation
```json
{
  "timestamp": "2026-10-13T12:00:10.104Z",
  "level": "ERROR",
  "fields": {
    "error_code": "SDE_CORRUPTION_DETECTED",
    "table": "staStations",
    "error": "database disk image is malformed"
  },
  "target": "eve_trade_scout::sde::loader"
}
```

#### Step-by-Step Resolution
1. Check file size and integrity: `ls -lh data/eve_sde.sqlite` (expected: ~150–250 MB).
2. Delete the damaged database:
   ```bash
   rm -f data/eve_sde.sqlite*
   ```
3. Re-download and compile fresh SDE tables:
   ```bash
   cargo run -- --update-sde
   ```

---

### 2. ESI Network, API & Rate Limit Errors

#### Description & Root Causes
- HTTP 420: ESI Error Limit exceeded (100 errors/minute threshold).
- HTTP 502/503/504: CCP server downtime or Tranquility VIP maintenance.
- Connection timeouts: DNS resolution failures or regional network drops.

#### Log Representation
```json
{
  "timestamp": "2026-10-13T12:05:42.512Z",
  "level": "ERROR",
  "fields": {
    "error_code": "ESI_RATE_LIMIT_EXCEEDED",
    "status": 420,
    "endpoint": "https://esi.evetech.net/v1/markets/10000002/orders/",
    "retry_after": 60
  },
  "target": "eve_trade_scout::esi::client"
}
```

#### Step-by-Step Resolution
1. Verify CCP ESI Server Status at [esi.evetech.net/status](https://esi.evetech.net).
2. Clear cached stale ETags if requests are looping:
   ```bash
   rm -rf cache/orders_*.json* cache/etags.json
   ```
3. Wait 60 seconds for the error limit window to reset before restarting scans.

---

### 3. SSO Authentication & Token Errors

#### Description & Root Causes
- Expired refresh tokens where the client cannot contact `login.eveonline.com`.
- Tampered or inaccessible local credentials file `~/.config/eve-traveling-merchant/sso_token.json` (or `%APPDATA%\eve-traveling-merchant\sso_token.json` on Windows).
- Port conflict binding local loopback listener on `127.0.0.1:37373`.
- Misconfigured optional `client_id` (note: no client secret is needed or supported).

#### Log Representation
```json
{
  "timestamp": "2026-10-13T12:15:20.890Z",
  "level": "WARN",
  "fields": {
    "error_code": "ERR_SSO_TOKEN_REFRESH",
    "character_id": 95123456,
    "error": "invalid_grant: refresh token expired or revoked"
  },
  "target": "eve_trade_scout::esi::sso"
}
```

#### Step-by-Step Resolution
1. Open Native GUI Settings tab (`Alt+S`).
2. Click **Disconnect Pilot** to purge the expired token.
3. Click **Authenticate Pilot via EVE SSO** to re-authorize through your default web browser (works out-of-the-box with embedded public Client ID).
4. If using a custom developer Client ID via `EVE_CLIENT_ID` or config, ensure your CCP Developer Application callback URL is registered as `http://127.0.0.1:37373/callback` (or `http://localhost:37373/callback`).
5. Ensure port `37373` is not occupied by another process (`ss -tulpn | grep 37373`).


---

### 4. Universe Graph & Routing Sentinels

#### Description & Root Causes
- `jump_count` returns `9999`: Target solar system is unreachable, or enters lowsec/nullsec space while `safe_only` is active.
- Unmapped system ID: System added in recent EVE expansion not present in current SDE.

#### Log Representation
```json
{
  "timestamp": "2026-10-13T12:30:15.340Z",
  "level": "WARN",
  "fields": {
    "origin": 30000142,
    "dest": 30003491,
    "safe_mode": true,
    "result": 9999,
    "message": "Safe routing avoided lowsec system"
  },
  "target": "eve_trade_scout::graph"
}
```

#### Step-by-Step Resolution
1. Check if the target station is located in lowsec or nullsec.
2. In GUI: toggle off "Safe Highsec Only" if you are operating a blockade runner or tanked ship.
3. In CLI: pass `--unsafe-routes` to permit travel through lowsec.

---

### 5. Native GUI Window & Wayland/X11 Display Errors

#### Description & Root Causes
- Application crashes on headless server or SSH without X11 forwarding.
- eframe/egui fails to create OpenGL/glow context.

#### Log Representation
```json
{
  "timestamp": "2026-10-13T12:45:00.012Z",
  "level": "ERROR",
  "fields": {
    "error_code": "GUI_DISPLAY_UNAVAILABLE",
    "display": null,
    "wayland_display": null,
    "message": "No display server detected"
  },
  "target": "eve_merchant"
}
```

#### Step-by-Step Resolution
1. Run in CLI/TUI terminal mode on headless environments:
   ```bash
   cargo run -- --no-ui --ship big --system Jita
   ```
2. Verify display server permissions on Linux:
   ```bash
   echo $WAYLAND_DISPLAY $DISPLAY
   ```
