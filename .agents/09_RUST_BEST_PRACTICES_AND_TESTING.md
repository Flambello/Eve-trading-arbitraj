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

## 3. Testing Necessities

### Where Tests Belong
1. **Unit Tests**: Located in `tests/test_arbitrage.rs` and module-level `#[cfg(test)]` blocks.
2. **Key Behaviors to Always Test**:
   - Ship profile volume and budget boundaries.
   - Profit-per-jump (PPJ) calculation formula.
   - Enforced 15% minimum net profitability (verifying sub-15% after-tax trades are rejected and $\ge 15\%$ are accepted).
   - Highsec vs. Lowsec pathfinding security constraints.

### Adding New Features
Whenever adding a new trading filter, formula, or routing mode:
1. Write the corresponding test in `tests/` FIRST or alongside the feature.
2. Verify with `cargo test`.
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
