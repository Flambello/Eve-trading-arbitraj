# Architecture — Rust

---

## Component Diagram

```
┌──────────────────────────────────────────────────────────────┐
│                        USER INTERFACE                         │
│  ui/tui.rs  ──  ratatui terminal (tables, spinners, input)   │
└─────────────────────────┬────────────────────────────────────┘
                           │ calls
┌─────────────────────────▼────────────────────────────────────┐
│                       ORCHESTRATOR                            │
│  main.rs  ──  clap CLI args → async tokio::main entry point  │
└────┬────────────┬──────────────────┬─────────────────────────┘
     │            │                  │
     ▼            ▼                  ▼
┌─────────┐ ┌──────────┐  ┌────────────────────┐
│config.rs│ │sde/      │  │esi/client.rs        │
│         │ │loader.rs │  │                     │
│ Loads   │ │          │  │ reqwest async client│
│ config  │ │ SQLite   │  │ ETag cache on disk  │
│ .toml   │ │ reader   │  │ rate-limit backoff  │
└─────────┘ └──────────┘  └──────────┬──────────┘
                                       │ raw JSON
                           ┌───────────▼──────────────┐
                           │  esi/market.rs             │
                           │                           │
                           │  - Paginates all pages    │
                           │  - Groups sell/buy orders │
                           │  - Enriches with system   │
                           └───────────┬───────────────┘
                                       │ HashMap<TypeId, orders>
                           ┌───────────▼───────────────┐
                           │     arbitrage.rs            │
                           │                            │
                           │  - Pairs sell→buy          │
                           │  - Calculates profit       │
                           │  - Applies constraints     │
                           │  - Sorts by PPJ            │
                           └───────────┬────────────────┘
                                       │ Vec<ArbitrageDeal>
          ┌────────────────────────────▼─────────────────────────┐
          │         graph.rs  +  route_planner.rs                 │
          │                                                       │
          │  graph.rs         — petgraph Dijkstra, LRU cache      │
          │  route_planner.rs — greedy multi-stop route builder   │
          └───────────────────────────────────────────────────────┘
```

---

## Module Responsibilities

### `main.rs`
- Parse CLI args via `clap` derive macros
- Load `UserConfig` from TOML
- Ensure SDE is present (download if needed)
- Spawn `tokio::main` async runtime
- Coordinate the full pipeline
- Hand results to the TUI

### `config.rs`
- `ShipProfile` constants (`BIG_HAULER`, `SMALL_HAULER`)
- `UserConfig` struct with `serde` derives for `toml` read/write
- Default config generation
- Config file path: `~/.config/eve-trade-scout/config.toml`

### `sde/loader.rs`
- Download + decompress Fuzzwork SDE SQLite (`bzip2` crate) if absent or stale
- Open `rusqlite::Connection` to local `data/eve_sde.sqlite`
- Exposes:
  - `get_item_info(type_id: u32) -> Option<ItemInfo>`
  - `get_system_id(name: &str) -> Option<u32>`
  - `get_system_name(system_id: u32) -> Option<String>`
  - `get_station_system(station_id: u64) -> Option<u32>`
  - `build_system_graph() -> petgraph::Graph<SystemNode, ()>`

### `esi/client.rs`
- `reqwest::Client` configured with:
  - `User-Agent` header
  - gzip decoding enabled
  - connection pool (max 10 connections)
- ETag cache: `HashMap<String, CacheEntry>` persisted to `cache/etags.json`
- `async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T>`
- `async fn get_all_pages<T>(&self, path: &str) -> Result<Vec<T>>`
  - Reads `X-Pages` header, fetches pages concurrently via `tokio::spawn` + `JoinSet`
- Exponential backoff on 5xx and 420 errors (`tokio::time::sleep`)

### `esi/market.rs`
- `async fn fetch_region_market(client, sde, region_id) -> Result<RegionMarket>`
- `async fn fetch_markets(client, sde, region_ids) -> Result<HashMap<u32, RegionMarket>>`
  - Concurrently fetches all regions via `tokio::task::JoinSet`

### `arbitrage.rs`
- Pure sync computation (no I/O) — fast enough to run on the main thread
- `fn find_deals(markets, sde, graph, ship, home_system, config) -> Vec<ArbitrageDeal>`
- `fn calculate_deal(...) -> Option<ArbitrageDeal>`

### `graph.rs`
- Wraps `petgraph::Graph<SystemNode, ()>`
- `fn jump_count(&self, origin: u32, dest: u32, safe_only: bool) -> u32`
  - Uses `petgraph::algo::dijkstra`
  - Memoized with `lru::LruCache<(u32, u32, bool), u32>`

### `route_planner.rs`
- `fn plan_route(deals, ship, home_system, max_stops) -> TradeRoute`
- Greedy selection loop (details in `05_ALGORITHM.md`)

### `ui/tui.rs`
- Full `ratatui` terminal UI
- `crossterm` backend for raw mode input
- Renders:
  - Deal table with scrolling
  - Route panel
  - Settings panel
  - Progress overlay during fetch

---

## Async Model

```
tokio::main (multi-thread scheduler)
    │
    ├── market fetch tasks (JoinSet) ─── up to N region fetches in parallel
    │       └── per-region page tasks (JoinSet, bounded 10) ─── pages in parallel
    │
    └── everything else is sync (arbitrage, graph, UI)
```

- No `std::thread` needed — `tokio` handles all concurrency
- `reqwest` reuses a single connection pool across all fetches
- Disk I/O for cache files done with `tokio::fs` to avoid blocking

---

## Caching Strategy

| Data | Storage | TTL |
|---|---|---|
| Market orders (per region) | Disk JSON + ETag (`cache/`) | 300 s (ESI) |
| Jump counts | In-process `LruCache` (10 000 entries) | Until exit |
| Item info | `rusqlite` query | SDE file lifetime |
| System graph | `petgraph::Graph` in heap | Until exit |
| SDE database | `data/eve_sde.sqlite` | 30 days or manual update |

---

## Error Handling

All errors use `anyhow::Error` with context:

```rust
let orders = client
    .get_all_pages::<MarketOrder>(&url)
    .await
    .context("fetching market orders for The Forge")?;
```

Custom error variants for ESI-specific cases:

```rust
#[derive(Debug, thiserror::Error)]
pub enum EsiError {
    #[error("ESI rate limit hit (HTTP 420), retry after {retry_after}s")]
    RateLimited { retry_after: u64 },
    #[error("ESI not found (HTTP 404): {path}")]
    NotFound { path: String },
    #[error("ESI server error (HTTP {status}): {body}")]
    ServerError { status: u16, body: String },
}
```
