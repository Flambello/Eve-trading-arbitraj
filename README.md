# EVE Trade Scout 🚀

> **High-Performance Arbitrage Engine & Multi-Stop Route Planner for EVE Online**  
> Written in Rust for maximum speed, type-safety, and reliability on Linux.

---

## Features

- ⚡ **Native Performance**: Pure compiled Rust binary with async networking powered by `tokio` and `reqwest`.
- 🚢 **Ship Profiles**:
  - **Big Hauler**: 60,000 m³ capacity / 500M ISK budget constraint.
  - **Small Hauler**: 30,000 m³ capacity / 50M ISK budget constraint.
- 📈 **Enforced 15% Net Profitability**: Filters out low-margin traps. Automatically enforces a minimum 15% net return on capital after all taxes and broker fees (configurable via `--min-roi`).
- 🛡️ **Highsec Only Space**: Excludes all lowsec and nullsec solar systems (security < 0.45). Only safe high security space is considered for arbitrage deals.
- 🏛️ **Station Access Pool Toggle**: Toggle between preferred **Only NPC Stations** (guaranteed 100% public docking) or include **Player-Owned Citadels / Upwell Structures** in the trading pool.
- 🎯 **Profit-Per-Jump Ranking**: Every candidate deal accounts for both the travel jumps from your current location to the pick-up station **and** the delivery jumps to the destination.
- 🗺️ **Multi-Stop Route Planning**: Greedily chains the best sequential trade deals together, providing step-by-step autopilot waypoints.
- 🛡️ **Safe Route Option**: Highsec-only pathfinding using `petgraph` Dijkstra over the complete New Eden stargate network (avoids lowsec/nullsec camps).
- 🖥️ **Native Desktop GUI Window**: Built with `egui` and `eframe` for Linux (Wayland & X11 native). Dark sci-fi aesthetic, interactive deal tables, clipboard copy buttons for in-game pasting, multi-stop route visualizer, and live "Scan Market" button.
- 💻 **Interactive Terminal UI (TUI)**: Built with `ratatui` with table navigation, tab switching, and route views for terminal users and headless/SSH sessions.
- 📊 **CSV Export & Scripting**: Export all discovered trade deals to CSV or run in pure CLI mode for scripts and bots.

---

## Installation

### Prerequisites (Fedora Linux / Generic Linux)
- Rust 1.78+ (via [rustup](https://rustup.rs/))
- GCC and development headers:
  ```bash
  sudo dnf install gcc pkg-config -y
  ```

### Build & Run

You can use standard `cargo` commands or the provided `Makefile`:

```bash
# Using Makefile on Fedora
make check        # Verify compilation
make test         # Run unit tests
make release      # Build optimized release binary
make run          # Launch the tool
```

Or using `cargo` directly:
```bash
cargo build --release
./target/release/eve-trade
```

Optionally install it system-wide:
```bash
cargo install --path .
eve-trade
```

---

## Usage

### 1. Native Desktop GUI Window (Default on Linux Desktop)
Simply run without arguments on your Fedora desktop (Wayland or X11) to open the native graphical application:
```bash
./target/release/eve-trade
# or: make run
```
**Desktop GUI Features:**
- 🔄 **Live "Scan Market" Button**: Fetches live orders from CCP's ESI API in a background worker thread without freezing the UI.
- 🚢 **Interactive Ship Selector**: Instantly toggle between Big Hauler (60k m³ / 500M) and Small Hauler (30k m³ / 50M).
- 📍 **Starting Location & Presets**: Type any solar system or click quick presets (`Jita`, `Amarr`, `Dodixie`, `Rens`).
- 📋 **Copy to Clipboard**: One-click copy for buy system, sell system, and the entire autopilot waypoints trail for instant in-game pasting.
- 🔍 **Real-Time Item Search**: Filter deals by item name on the fly.
- 🗺️ **Visual Route Plan**: Step-by-step waypoint cards showing what to buy, quantity, cost, warp jumps, and net profit.

### 2. Terminal Interactive Mode (TUI)
Prefer the terminal or working over SSH? Use `--tui`:
```bash
eve-trade --tui
```
**Keybindings:**
- `[Tab]` — Switch between Deals Table, Planned Route, and Help
- `[1]`, `[2]`, `[3]` — Direct jump to tabs
- `[↑] / [k]` — Select previous deal
- `[↓] / [j]` — Select next deal
- `[q] / [Esc]` — Quit

### 3. CLI / Headless Scripting Mode
Use `--no-ui` to print the formatted table straight to the console or pipe into scripts:
```bash
# Quick scan with Big Hauler starting in Jita (The Forge)
eve-trade --ship big --system Jita --no-ui

# Safe highsec routes only with Small Hauler from Amarr
eve-trade --ship small --system Amarr --safe --no-ui

# Scan multiple regions (e.g., The Forge 10000002 and Domain 10000043)
eve-trade --regions 10000002,10000043 --top 30 --no-ui

# Export deals to CSV for spreadsheets
eve-trade --ship big --system Jita --export deals.csv
```

---

## Command-Line Arguments

| Flag | Default | Description |
|---|---|---|
| `--gui` | `auto` | Force desktop GUI window mode (`egui` native window) |
| `--tui` | `false` | Force terminal TUI mode (`ratatui`) |
| `--no-ui` | `false` | Disable interactive UI (GUI & TUI) and output directly to console |
| `-s, --ship` | `big` | Ship profile (`big` or `small`) |
| `--system` | `Jita` | Starting solar system name |
| `--regions` | `10000002` | Comma-separated region IDs |
| `--safe` | `true` | Avoid lowsec/nullsec gates (Highsec only) |
| `--unsafe-routes` | `false` | Allow lowsec/nullsec gates |
| `--npc-only` | `true` | Restrict search strictly to NPC stations (preferred) |
| `--allow-structures` | `false` | Include player-owned Citadels / Upwell structures |
| `--min-profit` | `1,000,000` | Minimum net profit in ISK |
| `--min-spread` | `3.0` | Minimum gross price spread (%) |
| `--min-roi` | `15.0` | **Enforced minimum net profitability (ROI %) after taxes** |
| `-t, --top` | `25` | Number of deals in CLI summary |
| `--max-stops` | `5` | Maximum stops in planned route |
| `--route` | `false` | Print planned route in CLI mode |
| `--export <path>` | — | Export deals to CSV file |
| `--update-sde` | — | Force re-download of SDE database |

---

## Architecture & Codebase

- `src/lib.rs`: Shared crate root (`eve_trade_scout`) exposing clean APIs for integration testing and binaries.
- `src/config.rs`: Ship limits, user settings, TOML loading.
- `src/sde/loader.rs`: SQLite loader and automated Fuzzwork SDE sync.
- `src/graph.rs`: Stargate topology and Dijkstra routing with LRU cache.
- `src/esi/`: Async REST client, ETag store, market order pagination.
- `src/arbitrage.rs`: Quantity constraint solver, 15% ROI invariant, highsec/station filters, and PPJ ranker.
- `src/route_planner.rs`: Greedy sequential route builder.
- `src/ui/gui.rs`: `egui`/`eframe` native desktop GUI window with dark sci-fi design.
- `src/ui/tui.rs`: `ratatui` interactive terminal UI dashboard.

---

## Automated Test Suites & Quality Assurance

EVE Trade Scout enforces 100% passing test suites across all core features and domain invariants:

```bash
# Run all 19 tests with verbose output
make test
# or
cargo test --verbose
```

### Test Suites Overview

| Test Suite | Purpose | Covered Invariants & Logic |
|---|---|---|
| **`tests/test_arbitrage.rs`** | Arbitrage Engine | • Enforced 15.0% net ROI threshold after taxes & broker fees<br>• Station pool toggle (NPC stations only vs Citadels)<br>• Highsec space security restriction ($\ge 0.45$)<br>• Quantity solver (volume, budget, supply, demand limits)<br>• Profit-per-jump (PPJ) formula with starting system jumps<br>• Big Hauler (60k m³ / 500M ISK) & Small Hauler (30k m³ / 50M ISK)<br>• Deal ranking by PPJ DESC, then net profit DESC |
| **`tests/test_config.rs`** | Configuration & Profiles | • Profile parsing from CLI arguments & config strings<br>• Case-insensitivity & aliases ("big", "b", "small", "s")<br>• Rejection of invalid profile names<br>• Default configuration settings and invariant preservation<br>• Full TOML serialization & round-trip verification |
| **`tests/test_graph.rs`** | Topology & Pathfinding | • Solar system security lookup & classification<br>• Dijkstra jump distance calculation<br>• Safe routing highsec detour around lowsec systems<br>• Lowsec destination rejection in safe mode (9999 jumps) |
| **`tests/test_route_planner.rs`** | Route Optimization | • Empty opportunity handling<br>• Greedy multi-stop route construction<br>• Cargo volume and budget replenishment/deduction<br>• Maximum stop constraint cutoff |
| **`tests/test_esi_models.rs`** | ESI Models & Deserialization | • Real CCP ESI market order JSON deserialization<br>• Buy order vs sell order discrimination<br>• Cheapest sell and best buy order discovery |

---

## Legal & EULA Compliance

This software is an external market analysis tool that interacts solely with CCP Games' public **EVE Swagger Interface (ESI)**. It does not automate any in-game actions, client inputs, or game memory, fully complying with the **EVE Online EULA** and Developer License Agreement.

---

## License

MIT License. See [LICENSE](LICENSE) for details.
