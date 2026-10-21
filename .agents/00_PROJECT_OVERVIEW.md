# EVE Traveling Merchant — Project Overview

> **Purpose:** A compiled location-centric trading engine and multi-item corridor route planner for EVE Online that finds the most profitable cross-station arbitrage opportunities from the player's current system, scans adjacent neighbor regions, consolidates cargo via knapsack optimization, and plans high-ROI trade routes.
>
> **Language: Rust** — single compiled binary (`eve-merchant`), zero runtime dependencies, maximum performance.

---

## What the tool does

1. You tell it:
   - **Ship mode** — Big (60 000 m³ / 500 M ISK budget) or Small (30 000 m³ / 50 M ISK budget)
   - **Starting solar system** (your current location, with real-time autocomplete)
2. It fetches live market data for your **Home Region PLUS stargate-connected adjacent neighbor regions** via CCP's **EVE ESI REST API**
3. It finds items that can be bought cheaply at a station near you and sold for more at another station, enforcing $\ge 10.0\%$ net ROI
4. It groups multiple profitable deals on identical origin-to-destination corridors using **multi-item cargo packing (knapsack)** to fully utilize ship hold and budget
5. It prioritizes high-ROI routes and dead ends ($\ge 40\%$) and ranks every opportunity by **profit per jump** (accounting for travel to the source *and* from source to destination)
6. It presents a **sorted deal list**, **packed cargo corridor manifests**, and an optimized **multi-stop trade route**

---

## High-level user flow

```
[Start]
   │
   ├─ Choose ship profile (Big / Small)
   ├─ Enter starting system name
   │
   ▼
[Fetch market data for the region(s)]
   │
   ▼
[For each (source-station, dest-station) pair with a spread > 0]
   │
   ├─ Calculate total jumps  (home→source + source→dest)
   ├─ Calculate quantity fitting within volume & ISK limits
   ├─ Calculate net profit and profit-per-jump
   │
   ▼
[Sort deals by profit-per-jump DESC]
   │
   ▼
[Display ranked results in terminal table]
   │
   ├─ Optional: select top deals to build a multi-stop route
   │
   ▼
[Print planned route with waypoints + expected total profit]
```

---

## Repository layout

```
Eve trading arbitraj/
├── .agents/                        ← planning & architecture docs
│   ├── 00_PROJECT_OVERVIEW.md
│   ├── 01_API_REFERENCE.md
│   ├── 02_DATA_MODEL.md
│   ├── 03_ARCHITECTURE.md
│   ├── 04_MODULES.md
│   ├── 05_ALGORITHM.md
│   ├── 06_UI_UX.md
│   ├── 07_SETUP_AND_DEPS.md
│   ├── 08_PACKAGING_AND_PUBLISHING.md
│   ├── 09_RUST_BEST_PRACTICES_AND_TESTING.md
│   ├── 10_ERROR_CODES_AND_LOGGING.md
│   └── rules/session_start.md
│
├── src/
│   ├── lib.rs                      ← library root (eve_trade_scout crate)
│   ├── main.rs                     ← binary entry point (eve-merchant executable)
│   ├── error.rs                    ← centralized ErrorCode enum & error taxonomy
│   ├── logging.rs                  ← tracing subscriber, file rotation & panic hook
│   ├── config.rs                   ← ship profiles, user settings (TOML)
│   ├── esi/
│   │   ├── mod.rs
│   │   ├── client.rs               ← async HTTP layer (reqwest + ETag caching)
│   │   ├── market.rs               ← market order fetching & parsing
│   │   ├── models.rs               ← ESI response types (serde structs)
│   │   └── sso.rs                  ← ESI SSO PKCE OAuth2 authentication
│   ├── sde/
│   │   ├── mod.rs
│   │   └── loader.rs               ← SDE SQLite reader (item volumes, system graph)
│   ├── graph.rs                    ← stargate graph + Dijkstra pathfinding (petgraph)
│   ├── arbitrage.rs                ← core profit calculation engine & quantity solver
│   ├── route_planner.rs            ← multi-stop greedy route builder
│   ├── chain_route_planner.rs      ← multi-leg chained routes with knapsack & detours
│   ├── hub_arbitrage.rs            ← core trade hub arbitrage & corridor runs
│   ├── audio.rs                    ← freedesktop notifications & sound alerts
│   └── ui/
│       ├── mod.rs                  ← UI dispatcher (GUI vs TUI vs CLI)
│       ├── gui.rs                  ← native desktop GUI (egui / eframe, universal default)
│       ├── theme.rs                ← 4 cockpit themes, tokenized ThemePalette & contrast system
│       └── tui.rs                  ← terminal interface (ratatui / crossterm, deprecated)
│
├── data/                           ← cached SDE SQLite database
│   └── .gitkeep
│
├── tests/                          ← comprehensive automated test suites (14 suites)
│   ├── test_arbitrage.rs           ← 10% ROI, station filter, highsec filter, PPJ, hauler limits
│   ├── test_config.rs              ← ship profile parser, user config defaults & TOML roundtrip
│   ├── test_graph.rs               ← highsec security tags, safe route bypass, Dijkstra jumps
│   ├── test_route_planner.rs       ← greedy trade chaining, multi-stop constraints
│   ├── test_esi_models.rs          ← JSON order deserialization, best buy/sell matching
│   ├── test_chain_route_planner.rs ← multi-leg chain beam search, detour cost, knapsack packing
│   ├── test_hub_arbitrage.rs       ← core hub corridors, filters, manifests, and summaries
│   ├── test_sde_search.rs          ← Roman numerals, autocomplete, SQL escaping resilience
│   ├── test_security_audit.rs      ← decompression bomb guard, SSO validation, CSV escaping
│   ├── test_sso.rs                 ← PKCE derivation, token lifecycle, claims validation
│   ├── test_donation_branding.rs   ← in-game donation recipient & branding formatting
│   ├── test_error_codes.rs         ← centralized error code taxonomy & formatting
│   ├── test_logging.rs             ← structured logging, file rotation & redacting
│   └── test_theme.rs               ← 4 cockpit themes, contrast tokens, frames & indicators
│
├── Cargo.toml
├── Cargo.lock
├── Makefile                        ← Fedora build, check, test, and release targets
└── README.md
```

---

## Tech stack

| Layer | Crate / Tool | Reason |
|---|---|---|
| Language | **Rust 1.78+** (stable) | Single binary, zero runtime, memory-safe |
| Library & Binary | `eve_trade_scout` / `eve-merchant` | Modular crate exposes lib for integration testing |
| Async runtime | `tokio` | Industry standard async executor |
| HTTP client | `reqwest` | Async, TLS, gzip, ETag-friendly |
| JSON parsing | `serde` + `serde_json` | Zero-copy deserialization |
| SQLite (SDE) | `rusqlite` (bundled) | Zero dynamic dependencies, embedded C SQLite |
| Graph / pathfinding | `petgraph` | Dijkstra, efficient adjacency list |
| Desktop GUI | `eframe` + `egui` | Native Linux window (Wayland & X11), 4 selectable cockpit themes |
| Terminal UI | `ratatui` + `crossterm` | Rich interactive TUI, tables, spinners |
| CLI argument parsing | `clap` | Derive-macro based, flags and headless mode |
| Config files | `toml` crate | Read/write `config.toml` |
| Progress bars | `indicatif` | Download and processing progress |
| Error handling | `anyhow` + `thiserror` | Contextual and typed error propagation |
| Date/time | `chrono` | Timestamp parsing from ESI |
| LRU cache | `lru` crate | Jump-count memoization |

---

## Non-goals (explicit out-of-scope)

- No in-game automation (EULA compliance)
- No capital-ship jump-drive routing (only gate jumps)
- No Upwell structure market access (requires per-structure auth tokens)
- No real-time order streaming (poll every 5 min max, ESI cache limit)

---

## Key advantages of Rust for this project

| Feature | Benefit |
|---|---|
| Compiled binary | Ship one file — no Python venv, no pip, no runtime |
| `async`/`await` + tokio | Fetch 5 regions concurrently with zero threads overhead |
| `serde` | ESI JSON deserialized at native speed |
| `petgraph` Dijkstra | Pathfinding 10× faster than NetworkX |
| Type system | Impossible to mix up ISK and m³ if you use newtypes |
| `anyhow` errors | Propagate context-rich errors from API calls gracefully |
