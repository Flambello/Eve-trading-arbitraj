# EVE Traveling Merchant 🚀

> **Location-Centric Arbitrage Engine & Multi-Item Corridor Route Planner for EVE Online**  
> Written in Rust for maximum speed, type-safety, and reliability on Linux.

---

## Features

- ⚡ **Native Performance**: Pure compiled Rust binary (`eve-merchant`) with async networking powered by `tokio` and `reqwest`.
- 📍 **Starting Location Centric Scans**: Directly scans the player's home solar system plus all **stargate-connected adjacent neighbor regions**, eliminating unnecessary galaxy-wide overhead while catching easy border corridors.
- 💡 **Real-Time System Search & Autocomplete**: Instant search for any solar system in New Eden with security status and region display. 1-click autocomplete chips when typing incomplete system names.
- 📦 **Knapsack Multi-Item Cargo Consolidation**: Bundles multiple profitable trade commodities moving between the same stations or systems into packed corridor manifests. Eliminates flying empty with a single item—fully packs your cargo hold (60,000 m³ for Big Hauler; 30,000 m³ for Small Hauler) and capital budget for 25M–60M+ ISK payouts per run!
- 🔥 **High-ROI Detour Recognition (40%+ Badges)**: Prioritizes and rewards high-margin opportunities and dead ends ($\ge 40\%$ net ROI) with distinctive visual badges and tiebreaking in route planning.
- 🚢 **Ship Profiles**:
  - **Big Hauler**: 60,000 m³ capacity / 500M ISK budget constraint.
  - **Small Hauler**: 30,000 m³ capacity / 50M ISK budget constraint.
- 📈 **Enforced 15.0% Net Profitability**: Filters out low-margin traps. Automatically enforces a minimum 15% net return on capital after all taxes and broker fees (configurable via `--min-roi`).
- 🛡️ **Highsec Only Space**: Excludes all lowsec and nullsec solar systems (security < 0.45). Only safe high security space is considered for arbitrage deals.
- 🏛️ **Station Access Pool Toggle**: Toggle between preferred **Only NPC Stations** (guaranteed 100% public docking) or include **Player-Owned Citadels / Upwell Structures** in the trading pool.
- 🎯 **True Profit-Per-Jump Ranking**: Every candidate deal accounts for both the travel jumps from your current location to the pick-up station **and** the delivery jumps to the destination.
- 🗺️ **Multi-Stop Route Planning**: Greedily chains sequential trade deals together, providing step-by-step autopilot waypoints.
- 📊 **Enriched Table Telemetry**:
  - Security rating pills (`[1.0]`, `[0.9]`, etc.) with security-based color-coding
  - Profit density (`ISK/m³`) column and sorting
  - Ship capacity utilization (% cargo volume / % ISK budget)
  - 1-Click Clipboard Actions: `📋 Item`, `📋 Buy`, `📋 Sell`, and `📋 Shopping List`
- 🖥️ **Native Desktop GUI Window**: Built with `egui` and `eframe` for Linux (Wayland & X11 native). Dark sci-fi aesthetic, interactive deal tables, packed corridor manifests tab, clipboard copy buttons, multi-stop route visualizer, and live "Scan Market" button.
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
make check        # Verify compilation (0 warnings)
make test         # Run unit tests (0 failures)
make release      # Build optimized release binary
make run          # Launch the tool
```

Or using `cargo` directly:
```bash
cargo build --release
./target/release/eve-merchant
```

Optionally install it system-wide:
```bash
cargo install --path .
eve-merchant
```

---

## Usage

### 1. Native Desktop GUI Window (Default on Linux Desktop)
Launch the graphical interface using the startup script or desktop shortcut:
```bash
./start-gui.sh
# or: make run
```
**Desktop GUI Features:**
- 🔄 **Live "Scan Market" Button**: Fetches live orders from CCP's ESI API in a background worker thread without freezing the UI.
- 🚢 **Interactive Ship Selector**: Instantly toggle between Big Hauler (60k m³ / 500M) and Small Hauler (30k m³ / 50M).
- 📍 **Starting Location & Autocomplete**: Type any solar system name with instant autocomplete suggestions.
- 🌐 **+Adjacent Regions Toggle**: Automatically scans stargate-connected neighbor regions of your starting location.
- 📦 **Packed Manifests Tab**: Review knapsack multi-item cargo manifests that pack your ship hold and budget.
- 📋 **Copy to Clipboard**: One-click copy for item names, buy stations, sell stations, shopping lists, and autopilot waypoint trails.
- 🔍 **Real-Time Item Search & Multi-Column Sorting**: Sort by Profit/Jump, Total Profit, Net ROI %, Profit/m³, Jumps, or Cost.

### 2. Terminal Interactive Mode (TUI)
Prefer the terminal or working over SSH? Use `--tui`:
```bash
eve-merchant --tui
```
**Keybindings:**
- `[Tab]` — Switch between Deals Table, Planned Route, and Help
- `[1]`, `[2]`, `[3]` — Direct jump to tabs
- `[↑] / [k]` — Select previous deal
- `[↓] / [j]` — Select next deal
- `[q] / [Esc]` — Quit

### 3. CLI / Headless Scripting Mode
Use `--no-ui` to print formatted tables and manifests straight to the console or pipe into scripts:
```bash
# Scan starting from Jita with Big Hauler (scans The Forge + adjacent neighbor regions)
eve-merchant --ship big --system Jita --no-ui

# Safe highsec routes only with Small Hauler from Amarr
eve-merchant --ship small --system Amarr --safe --no-ui

# Specific custom regions (e.g. The Forge 10000002 and Domain 10000043)
eve-merchant --regions 10000002,10000043 --top 30 --no-ui

# Export deals to CSV for spreadsheets
eve-merchant --ship big --system Jita --export deals.csv
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
| `--adjacent-regions` | `true` | Automatically scan stargate-connected neighbor regions |
| `--regions` | `auto` | Comma-separated region IDs (overrides adjacent region scan) |
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
- `src/config.rs`: Ship limits, user settings, adjacent region settings, TOML loading.
- `src/sde/loader.rs`: SQLite loader, automated Fuzzwork SDE sync, and `search_systems` autocomplete.
- `src/graph.rs`: Stargate topology, adjacent region discovery, and Dijkstra routing with LRU cache.
- `src/esi/`: Async REST client, ETag store, market order streaming and pagination.
- `src/arbitrage.rs`: Quantity constraint solver, 15% ROI invariant, PPJ ranker, `is_high_roi()`, and `bundle_route_cargo()` knapsack consolidation.
- `src/route_planner.rs`: Greedy sequential route builder with high-ROI priority.
- `src/ui/gui.rs`: `egui`/`eframe` native desktop GUI window with dark sci-fi aesthetic, autocomplete suggestions, deals grid with telemetry pills, packed manifests tab, and dossier inspector.
- `src/ui/tui.rs`: `ratatui` interactive terminal UI dashboard with enriched security ratings.

---

## Automated Test Suites & Quality Assurance

EVE Traveling Merchant enforces 100% passing test suites across all core features and domain invariants:

```bash
# Run all tests with verbose output
make test
# or
cargo test --verbose
```

### Test Suites Overview

| Test Suite | Purpose | Covered Invariants & Logic |
|---|---|---|
| **`tests/test_arbitrage.rs`** | Arbitrage & Knapsack Packing | • Enforced 15.0% net ROI threshold after taxes & broker fees<br>• Station pool toggle (NPC stations only vs Citadels)<br>• Highsec space security restriction ($\ge 0.45$)<br>• Quantity solver (volume, budget, supply, demand limits)<br>• Profit-per-jump (PPJ) formula with starting system jumps<br>• Knapsack multi-item cargo consolidation (`bundle_route_cargo`)<br>• High-ROI threshold detection (`is_high_roi`)<br>• Big Hauler (60k m³ / 500M ISK) & Small Hauler (30k m³ / 50M ISK) |
| **`tests/test_config.rs`** | Configuration & Profiles | • Profile parsing from CLI arguments & config strings<br>• Case-insensitivity & aliases ("big", "b", "small", "s")<br>• Default configuration settings and invariant preservation<br>• Full TOML serialization & round-trip verification |
| **`tests/test_graph.rs`** | Topology & Neighbors | • Solar system security lookup & classification<br>• Adjacent region discovery via stargates (`adjacent_regions`)<br>• Dijkstra jump distance calculation<br>• Safe routing highsec detour around lowsec systems |
| **`tests/test_route_planner.rs`** | Route Optimization | • Empty opportunity handling<br>• Greedy multi-stop route construction<br>• Cargo volume and budget replenishment/deduction<br>• Maximum stop constraint cutoff |
| **`tests/test_sde_search.rs`** | System Search & Autocomplete | • Real-time prefix/substring search across New Eden systems<br>• System ID, security rating, and region resolution |
| **`tests/test_esi_models.rs`** | ESI Models & Deserialization | • Real CCP ESI market order JSON deserialization<br>• Buy order vs sell order discrimination<br>• Cheapest sell and best buy order discovery |

---

## Legal & EULA Compliance

This software is an external market analysis tool that interacts solely with CCP Games' public **EVE Swagger Interface (ESI)**. It does not automate any in-game actions, client inputs, or game memory, fully complying with the **EVE Online EULA** and Developer License Agreement.

---

## License

MIT License. See [LICENSE](LICENSE) for details.
