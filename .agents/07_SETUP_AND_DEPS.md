# Setup and Dependencies — Rust on Fedora Linux

---

## System Requirements

| Requirement | Minimum | Notes |
|---|---|---|
| OS | Fedora 38+ | Any modern Linux works |
| Rust | 1.78+ stable | Installed via `rustup` |
| Disk space | ~800 MB | SDE SQLite + Rust build artefacts |
| RAM | 512 MB | petgraph holds ~8 000 systems in memory |
| Internet | Required at first run | ESI API + SDE download |

---

## `Cargo.toml` — Full Dependency List

```toml
[package]
name    = "eve-trade-scout"
version = "0.1.0"
edition = "2021"
description = "EVE Online location-centric arbitrage route planner & multi-item cargo knapsack optimizer"

[[bin]]
name = "eve-merchant"
path = "src/main.rs"

[dependencies]
# Async runtime
tokio        = { version = "1.38", features = ["full"] }

# HTTP client — async, TLS, gzip, stream
reqwest      = { version = "0.12", features = ["json", "gzip", "rustls-tls", "stream"], default-features = false }

# Serialization / deserialization
serde        = { version = "1.0",  features = ["derive"] }
serde_json   = "1.0"

# TOML config files
toml         = "0.8"

# SQLite (SDE database)
rusqlite     = { version = "0.31", features = ["bundled"] }
# ↑ "bundled" compiles SQLite in — no system libsqlite3 needed

# Graph algorithms (stargate network + Dijkstra)
petgraph     = "0.6"

# LRU cache for jump-count memoization
lru          = "0.12"

# Desktop GUI (egui / eframe)
eframe       = { version = "0.28", default-features = false, features = ["default_fonts", "glow", "wayland", "x11"] }
egui_extras  = { version = "0.28", features = ["default"] }

# Structured logging & telemetry
tracing            = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
tracing-appender   = "0.2"

# Error handling
anyhow       = "1.0"

# Date/time (ESI timestamps)
chrono       = { version = "0.4",  features = ["serde"] }

# gzip decompression for SDE download
flate2       = "1.0"

# System directories & crypto
dirs         = "5.0"
ring         = "0.17"
base64       = "0.22"
image        = { version = "0.25", default-features = false, features = ["png"] }

[profile.release]
opt-level     = 3
lto           = true     # Link-time optimisation — smaller, faster binary
codegen-units = 1
strip         = true     # Strip debug symbols — minimise binary size
```

---

## Installation on Fedora

### Step 1 — Install Rust via rustup (recommended)

```bash
# Install rustup (manages Rust toolchains)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Follow the on-screen prompts (choose option 1 — default install)
# Then reload your shell:
source "$HOME/.cargo/env"

# Verify
rustc --version   # → rustc 1.78.0 (...)
cargo --version   # → cargo 1.78.0 (...)
```

### Step 2 — Install system build dependencies

```bash
# Required for linking (already present on most Fedora installs)
sudo dnf install gcc pkg-config openssl-devel -y
```

> **Note:** Because `rusqlite` uses the `bundled` feature, SQLite is compiled *into* the binary — no system `libsqlite3` required.

### Step 3 — Clone / navigate to the project

```bash
cd "/mnt/Projects/Eve trading arbitraj"
```

### Step 4 — Build

```bash
# Development build (fast compile, debug info)
cargo build

# Release build (optimised, stripped binary — use for daily runs)
cargo build --release
```

The release binary will be at:
```
target/release/eve-merchant
```

### Step 5 — First run

```bash
./target/release/eve-merchant
```

On first run it will:
1. Create `~/.config/eve-traveling-merchant/config.toml` with defaults
2. Download the SDE SQLite database to `data/eve_sde.sqlite` (~115 MB compressed → ~650 MB)
3. Build the stargate graph in memory (~2–3 s)
4. Open the native desktop GUI window (`egui`/`eframe`)

Subsequent runs skip the SDE download (auto-checks every 30 days).

---

## Optional: Install the binary system-wide

```bash
cargo install --path .

# Then run from anywhere:
eve-merchant
```

---

## Config File Location

```
~/.config/eve-traveling-merchant/config.toml
```

### Default `config.toml`

```toml
[ship]
profile = "big"

[location]
home_system = "Jita"

[scan]
regions = [10000002]     # The Forge; add more region IDs
safe_route_only = true
npc_stations_only = true
scan_adjacent_regions = true
auto_refresh = false
sound_alerts = true
sound_min_roi_pct = 100.0

[filters]
min_profit_isk     = 1_000_000.0
min_spread_pct     = 3.0
min_net_profit_pct = 10.0         # Enforced minimum 10% overall ROI after taxes

[fees]
broker_fee_pct = 3.0
sales_tax_pct  = 8.0

[cache]
directory        = "cache"
sde_max_age_days = 30

[sso]
client_id          = ""           # Optional custom Client ID override (empty = built-in public ID)
callback_port      = 37373        # Loopback callback port
auto_sync_location = false        # Automatically query character location on startup
```

### Out-of-the-Box EVE SSO Configuration (Zero Registration)

EVE Traveling Merchant uses the official CCP OAuth 2.0 PKCE flow (RFC 7636).
- **Public Client ID**: Built-in `5793fc7aa2cb49e385d3cfb5d52061c7` requires zero developer registration for pilots.
- **Zero Secrets**: No `CLIENT_SECRET` is used, accepted, or needed.
- **Optional Override**: Advanced users can override the Client ID via `EVE_CLIENT_ID` environment variable (see `.env.example`) or `developer_client_id` file.
- **Token Storage**: Saved to `~/.config/eve-traveling-merchant/sso_token.json` with POSIX `0600`/`0700` restricted permissions.


---

## Project Directory After Build

```
Eve trading arbitraj/
├── .agents/
├── src/
├── data/
│   └── eve_sde.sqlite          ← downloaded SDE (~650 MB, git-ignored)
├── cache/
│   └── etags.json              ← ESI ETag cache (git-ignored)
├── target/
│   ├── debug/eve-merchant      ← debug binary
│   └── release/eve-merchant    ← release binary (ship this one)
├── Cargo.toml
├── Cargo.lock
└── README.md
```

---

## `.gitignore`

```
/target
/data/eve_sde.sqlite
/cache/
~/.config/eve-traveling-merchant/
```

---

## Running Tests

```bash
cargo test
```

---

## Updating the SDE

The application automatically verifies SDE freshness on startup. If the local database is missing or older than `sde_max_age_days` (default 30 days in `config.toml`), a fresh dump is downloaded automatically. To force an immediate re-download:

```bash
rm -f data/eve_sde.sqlite
# Next application launch will re-download the latest Fuzzwork dump
```

---

## Performance Profile (expected)

| Operation | Time |
|---|---|
| Binary startup | < 50 ms |
| SDE graph build | ~2–3 s |
| Fetch 1 region (async) | ~3–6 s |
| Fetch 5 regions (concurrent) | ~6–12 s |
| Arbitrage computation (1 M orders) | < 100 ms |
| Route planning (1000 deals) | < 5 ms |
| Total first run | ~15–20 s |
| Total cached run | ~3–6 s |

---

## EULA Compliance Note

> This tool is a **read-only market analysis tool**. It:
> - Never performs any in-game action
> - Never automates trades
> - Only reads publicly available ESI API data
>
> Fully permitted under the [EVE Online EULA](https://www.eveonline.com/legal/eula) and ESI Developer Licence.

---

## Troubleshooting

| Problem | Solution |
|---|---|
| `rustc: command not found` | Run `source "$HOME/.cargo/env"` or reopen terminal |
| `linker cc not found` | `sudo dnf install gcc -y` |
| SDE download hangs | Check internet; Fuzzwork may be temporarily down |
| HTTP 420 from ESI | Tool backs off automatically; wait ~60 s |
| System name not found | Check spelling — use exact in-game name (case-insensitive) |
| `error: failed to run custom build command for rusqlite` | `sudo dnf install gcc pkg-config -y` |
