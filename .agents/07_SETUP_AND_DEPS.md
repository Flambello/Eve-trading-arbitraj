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
version = "1.0.0"
edition = "2021"
description = "EVE Online arbitrage route planner"

[[bin]]
name = "eve-trade"
path = "src/main.rs"

[dependencies]
# Async runtime
tokio        = { version = "1",    features = ["full"] }

# HTTP client — async, TLS, gzip, connection pool
reqwest      = { version = "0.12", features = ["json", "gzip", "rustls-tls"], default-features = false }

# Serialization / deserialization
serde        = { version = "1",    features = ["derive"] }
serde_json   = "1"

# TOML config files
toml         = "0.8"

# SQLite (SDE database)
rusqlite     = { version = "0.31", features = ["bundled"] }
# ↑ "bundled" compiles SQLite in — no system libsqlite3 needed

# Graph algorithms (stargate network + Dijkstra)
petgraph     = "0.6"

# LRU cache for jump-count memoization
lru          = "0.12"

# Terminal UI
ratatui      = "0.27"
crossterm    = "0.27"

# CLI argument parsing
clap         = { version = "4",    features = ["derive"] }

# Progress bars (SDE download, market fetch)
indicatif    = "0.17"

# Error handling
anyhow       = "1"
thiserror    = "1"

# Date/time (ESI timestamps)
chrono       = { version = "0.4",  features = ["serde"] }

# bzip2 decompression for SDE download
bzip2        = "0.4"

# Async file I/O
tokio        = { version = "1",    features = ["fs"] }

[dev-dependencies]
tokio-test   = "0.4"

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
target/release/eve-trade
```

### Step 5 — First run

```bash
./target/release/eve-trade
```

On first run it will:
1. Create `~/.config/eve-trade-scout/config.toml` with defaults
2. Download the SDE SQLite database to `data/eve_sde.sqlite` (~115 MB compressed → ~650 MB)
3. Build the stargate graph in memory (~2–3 s)
4. Open the interactive TUI — prompt for ship profile and home system

Subsequent runs skip the SDE download (auto-checks every 30 days).

---

## Optional: Install the binary system-wide

```bash
cargo install --path .

# Then run from anywhere:
eve-trade
```

---

## Config File Location

```
~/.config/eve-trade-scout/config.toml
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

[filters]
min_profit_isk = 1_000_000.0
min_spread_pct = 5.0

[fees]
broker_fee_pct = 3.0
sales_tax_pct  = 8.0

[cache]
directory        = "cache"
sde_max_age_days = 30
```

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
│   ├── debug/eve-trade         ← debug binary
│   └── release/eve-trade       ← release binary (ship this one)
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
~/.config/eve-trade-scout/
```

---

## Running Tests

```bash
cargo test
```

---

## Updating the SDE

```bash
./target/release/eve-trade --update-sde
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
