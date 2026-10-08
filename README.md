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
| `--safe` | `true` | Avoid lowsec/nullsec gates |
| `--unsafe-routes` | `false` | Allow lowsec/nullsec gates |
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

- `src/config.rs`: Ship limits, user settings, TOML loading.
- `src/sde/loader.rs`: SQLite loader and automated Fuzzwork SDE sync.
- `src/graph.rs`: Stargate topology and Dijkstra routing with LRU cache.
- `src/esi/`: Async REST client, ETag store, market order pagination.
- `src/arbitrage.rs`: Quantity constraint solver and profit-per-jump ranker.
- `src/route_planner.rs`: Greedy sequential route builder.
- `src/ui/gui.rs`: `egui`/`eframe` native desktop GUI window with dark sci-fi design.
- `src/ui/tui.rs`: `ratatui` interactive terminal UI dashboard.

---

## Legal & EULA Compliance

This software is an external market analysis tool that interacts solely with CCP Games' public **EVE Swagger Interface (ESI)**. It does not automate any in-game actions, client inputs, or game memory, fully complying with the **EVE Online EULA** and Developer License Agreement.

---

## License

MIT License. See [LICENSE](LICENSE) for details.
