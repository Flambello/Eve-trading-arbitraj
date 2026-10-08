# Rust Best Practices & Testing Protocol

> **Mandatory Guidelines for EVE Trade Scout**  
> Every contributor and AI agent working on this codebase must strictly adhere to these practices.

---

## 1. The Iron Rule: Post-Edit Verification

**NEVER consider a code edit complete without running compile and test checks.**

Whenever ANY file in `src/`, `tests/`, or `Cargo.toml` is modified, the following cycle MUST be executed:

```bash
# 1. Fast compile & type check (zero errors, zero warnings required)
cargo check

# 2. Automated test suite (all unit and integration tests must pass)
cargo test --verbose

# 3. Optional clippy lint check
cargo clippy -- -D warnings
```

Or using the project's native Fedora `Makefile`:
```bash
make check
make test
```

If `cargo check` or `cargo test` produces ANY failure or compiler warning:
- The issue must be investigated and resolved immediately.
- Never commit code that breaks tests or leaves unaddressed warnings.

---

## 2. Rust Coding & Architectural Best Practices

### A. Robust Error Handling (No Panics)
- **Zero `unwrap()` / `expect()` in Runtime Code**: Never use `.unwrap()` or `.expect()` on `Option` or `Result` in production logic where input, network, or data files might be invalid.
- **Context-Rich Errors**: Use `anyhow::Context` to attach clear error descriptions explaining *what* was being attempted:
  ```rust
  let resp = client.get(&url).send().await
      .with_context(|| format!("Failed to fetch market orders from {}", url))?;
  ```
- **Domain Errors with `thiserror`**: When matching against specific operational failures (e.g. rate limits, 404s), define typed enums using `thiserror`.

### B. Type Safety & Domain Invariants
- **Ship Limits**: Never hardcode ship cargo capacity (60,000 m³ / 30,000 m³) or budget limits (500M / 50M ISK) as raw literals across multiple files. Always reference `ShipProfile` (`BIG_HAULER`, `SMALL_HAULER`).
- **Enforced 15% Net ROI**: Every arbitrage deal must strictly enforce $\ge 15.0\%$ net profitability after sales tax and broker fees before being emitted:
  $$\text{net\_profit\_pct} = \frac{\text{net\_profit}}{\text{capital\_required}} \times 100 \ge 15.0$$

### C. Concurrency & Async Architecture
- **Tokio Runtime**: All async networking runs under `#[tokio::main]`.
- **Bounded Concurrency**: Never spawn unbounded tasks against CCP's ESI API. Always use `tokio::sync::Semaphore` (configured to max 10 concurrent requests) to prevent HTTP 420 error limiting.
- **ETag Caching**: Never bypass ETag checks. Always inspect `If-None-Match` and handle `304 Not Modified` gracefully to respect cache timers and save bandwidth.

### D. Memory & Graph Performance
- **Petgraph Stargate Routing**: Pathfinding across ~8,000 solar systems must use `petgraph::algo::dijkstra`.
- **LRU Cache**: All jump distance calculations must be memoized using `lru::LruCache<(u32, u32, bool), u32>` protected by a `std::sync::Mutex`.
- **Bundled SQLite**: Rusqlite must use `features = ["bundled"]` so SQLite compiles directly into the binary with zero dynamic library dependencies.

---

## 3. Testing Necessities & Test Suites Catalog

The codebase enforces full automated test suites across all domain components and invariants. All tests are located in `tests/` and run via `cargo test --verbose` or `make test`.

### Test Suite Structure

1. **`tests/test_arbitrage.rs` (Arbitrage Engine & Invariants)**
   - `test_ship_profile_values`: Validates exact max volume and budget for `BIG_HAULER` (60k m³, 500M ISK) and `SMALL_HAULER` (30k m³, 50M ISK).
   - `test_profit_per_jump_formula`: Confirms jumps from starting system are included in the denominator (`total_jumps = jumps_from_home + jumps_source_dest`).
   - `test_minimum_15_percent_net_profitability_enforced`: Verifies 15.0% after-tax ROI threshold strictly rejects sub-15% trades and accepts profitable ones.
   - `test_is_npc_station_id_threshold`: Tests station ID threshold separating NPC stations ($< 10^{12}$) from player Upwell structures ($\ge 10^{12}$).
   - `test_npc_stations_only_toggle_filter`: Validates that enabling the toggle excludes player structures and disabling it includes them.
   - `test_highsec_only_filtering_excludes_lowsec_and_nullsec`: Confirms systems with security $< 0.45$ are excluded.
   - `test_quantity_solver_constraints`: Tests the 4-way quantity solver against volume caps, ISK budget caps, order supply, and order demand limits.
   - `test_deal_sorting_ppj_descending`: Verifies sorting strictly orders deals by PPJ descending, breaking ties by net profit descending.

2. **`tests/test_config.rs` (Config & Profiles)**
   - `test_ship_profile_parsing_valid`: Tests parsing of valid inputs ("big", "small", aliases, case insensitivity, whitespace).
   - `test_ship_profile_parsing_invalid`: Tests graceful error reporting on invalid profile names.
   - `test_user_config_defaults_and_invariants`: Verifies default configuration preserves 15% ROI, safe routing, and NPC station filtering.
   - `test_user_config_toml_roundtrip`: Verifies complete serializability to and from TOML format.

3. **`tests/test_graph.rs` (Graph & Stargate Pathfinding)**
   - `test_system_security_and_is_highsec`: Validates security lookup and highsec classification threshold ($\ge 0.45$).
   - `test_jump_counts_normal_and_safe_mode`: Validates Dijkstra jump counts and safe mode lowsec endpoint rejection (9999).
   - `test_shortest_path_safe_highsec_only`: Confirms shortest path routes safely around lowsec systems when in safe mode.

4. **`tests/test_route_planner.rs` (Sequential Route Planning)**
   - `test_route_planner_empty_deals`: Validates graceful handling of empty opportunity sets.
   - `test_route_planner_greedy_chaining_and_max_stops`: Verifies multi-stop route construction, volume/budget deduction, and max stop cutoff.

5. **`tests/test_esi_models.rs` (ESI Models & JSON Parsing)**
   - `test_market_order_json_deserialization`: Verifies correct deserialization of live CCP ESI JSON responses.
   - `test_cheapest_sell_and_best_buy_selection`: Validates picking lowest sell price and highest buy price from order arrays.

### Adding New Features
Whenever adding a new trading filter, formula, or routing mode:
1. Write the corresponding test in `tests/` FIRST or alongside the feature.
2. Verify with `cargo test --verbose` (0 failures required).
3. Verify release optimization with `cargo build --release`.

---

## 4. Release Build Standard

Production binaries must be built using the release profile:
```bash
cargo build --release
# or
make release
```

Configured in `Cargo.toml`:
- `opt-level = 3` (maximum speed optimization)
- `lto = true` (link-time optimization across all crates)
- `codegen-units = 1` (maximum inter-procedural optimization)
- `strip = true` (removes debug symbols, producing a lean ~6 MB binary)
