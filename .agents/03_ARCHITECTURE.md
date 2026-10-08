# Architecture — Rust

---

## Component Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                             USER INTERFACES                                 │
│  ui/gui.rs ── egui/eframe desktop window (live scan, clipboard, styling)    │
│  ui/tui.rs ── ratatui terminal dashboard (tabs, scrollable tables, hotkeys) │
│  Headless  ── clap formatted console table, CSV exporter                    │
└──────────────────────────────────────┬──────────────────────────────────────┘
                                       │ calls
┌──────────────────────────────────────▼──────────────────────────────────────┐
│                    ORCHESTRATOR & BINARY ENTRY POINT                         │
│  src/main.rs (eve-trade bin) ── parses CLI args, initializes runtime        │
└──────────────────────────────────────┬──────────────────────────────────────┘
                                       │ invokes
┌──────────────────────────────────────▼──────────────────────────────────────┐
│                    SHARED CRATE LIBRARY (eve_trade_scout)                   │
│  src/lib.rs ── exposes modules to bin and to `tests/` integration suites    │
└──────────────┬───────────────────────┬─────────────────────────┬────────────┘
               │                       │                         │
               ▼                       ▼                         ▼
        ┌─────────────┐         ┌─────────────┐           ┌─────────────┐
        │  config.rs  │         │ sde/loader  │           │ esi/client  │
        │ ShipProfile │         │ SQLite dump │           │ HTTP client │
        │ UserConfig  │         │ map/items   │           │ ETag cache  │
        └─────────────┘         └─────────────┘           └──────┬──────┘
                                                                 │
                                                       ┌─────────▼─────────┐
                                                       │   esi/market.rs   │
                                                       │ Region pagination │
                                                       │ Order grouping    │
                                                       └─────────┬─────────┘
                                                                 │
                                                       ┌─────────▼─────────┐
                                                       │   arbitrage.rs    │
                                                       │ Quantity solver   │
                                                       │ 15% ROI invariant │
                                                       │ Highsec/NPC filter│
                                                       │ PPJ ranking       │
                                                       └─────────┬─────────┘
                                                                 │
                         ┌───────────────────────────────────────▼─────────┐
                         │           graph.rs  +  route_planner.rs         │
                         │ graph.rs         — petgraph Dijkstra, LRU cache │
                         │ route_planner.rs — greedy multi-stop builder    │
                         └─────────────────────────────────────────────────┘
```

---

## Module Responsibilities

### `src/lib.rs`
- Root of the `eve_trade_scout` library crate.
- Re-exports `arbitrage`, `config`, `esi`, `graph`, `route_planner`, `sde`, and `ui`.
- Enables complete integration test suites in `tests/` to run against clean crate interfaces without private coupling or SQLite mocking issues.

### `src/main.rs`
- Executable binary (`eve-trade`).
- Parse CLI arguments via `clap` (`--ship`, `--system`, `--regions`, `--safe`, `--unsafe-routes`, `--npc-only`, `--allow-structures`, `--min-roi`, `--gui`, `--tui`, `--no-ui`, `--export`).
- Detects environment (launches Desktop GUI by default when DISPLAY/WAYLAND_DISPLAY is present and stdout is a terminal, falls back to TUI or console).
- Orchestrates SDE check, graph creation, market fetching, deal analysis, and UI launching.

### `src/config.rs`
- `ShipProfile` constants: `BIG_HAULER` (60k m³ / 500M ISK) and `SMALL_HAULER` (30k m³ / 50M ISK).
- `UserConfig` TOML persistence at `~/.config/eve-trade-scout/config.toml`.
- Configures default parameters: 15.0% minimum ROI (`min_net_profit_pct`), safe highsec routing (`safe_route_only = true`), and NPC station filtering (`npc_stations_only = true`).

### `src/sde/loader.rs`
- Automated download + decompression of Fuzzwork SDE SQLite (`eve_sde.sqlite`) via `bzip2`.
- Reads item volumes (`packaged_volume`), solar system details, security ratings, and stargate links.
- Helper queries: `get_item_info`, `get_system_id`, `get_system_name`, `get_station_system`, `all_systems`, `all_jumps`.

### `src/esi/client.rs` & `src/esi/market.rs`
- Bounded async HTTP client (`reqwest`) with disk ETag caching in `cache/etags.json`.
- Concurrent pagination via `tokio::task::JoinSet` (bounded concurrency with rate-limiting backoff).
- Grouping orders into buy vs. sell books and selecting `cheapest_sell` and `best_buy`.

### `src/arbitrage.rs`
- Core arbitrage calculation engine (100% deterministic, memory-pure).
- Quantity constraint solver (constrained by ship cargo volume, capital budget, sell volume remaining, buy volume remaining).
- Strict invariant enforcement:
  - Minimum 15.0% net profitability after taxes and broker fees (`min_net_profit_pct`).
  - Highsec only filtering: source and dest must have security $\ge 0.45$.
  - Station pool toggle: `npc_stations_only` filters out stations with ID $\ge 10^{12}$ (Upwell structures).
  - Profit-per-jump: $\text{net\_profit} / (\text{jumps\_from\_home} + \text{jumps\_source\_dest})$.

### `src/graph.rs`
- Wraps `petgraph::Graph<SystemNode, ()>`.
- In-memory graph constructor `SystemGraph::from_nodes_and_jumps` enabling fast zero-disk testing.
- Dijkstra pathfinding with `LruCache<(u32, u32, bool), u32>` memoization.
- Safe routing mode: lowsec/nullsec gates penalized with weight 1000, unreachable endpoints return 9999.

### `src/route_planner.rs`
- Greedy multi-stop sequential trade chaining.
- Continually re-evaluates remaining capacity and jumps from current stop to the next deal.

### `src/ui/gui.rs` & `src/ui/tui.rs`
- `gui.rs`: Native desktop GUI window powered by `eframe` / `egui`. Includes live scanning thread, item search, clipboard copy buttons, and visual route display.
- `tui.rs`: Interactive terminal interface built with `ratatui` / `crossterm` for SSH or terminal power users.

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
