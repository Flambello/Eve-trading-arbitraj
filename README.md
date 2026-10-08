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
- 💾 **Smart Caching**: Full ETag caching respecting ESI API limits and local SQLite integration for the EVE Static Data Export (SDE).
- 🖥️ **Interactive Terminal UI**: Built with `ratatui` with table navigation, tab switching, and route views.
- 📊 **CSV Export**: Export all discovered trade deals to CSV with a single flag.

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

### Interactive TUI Mode
Run without arguments to launch the interactive terminal interface:
```bash
eve-trade
```

**Keybindings:**
- `[Tab]` — Switch between Deals Table, Planned Route, and Help
- `[1]`, `[2]`, `[3]` — Direct jump to tabs
- `[↑] / [k]` — Select previous deal
- `[↓] / [j]` — Select next deal
- `[q] / [Esc]` — Quit

### CLI / Headless Mode
```bash
# Quick scan with Big Hauler starting in Jita (The Forge)
eve-trade --ship big --system Jita --no-tui

# Safe highsec routes only with Small Hauler from Amarr
eve-trade --ship small --system Amarr --safe --no-tui

# Scan multiple regions (e.g., The Forge 10000002 and Domain 10000043)
eve-trade --regions 10000002,10000043 --top 30 --no-tui

# Export deals to CSV for spreadsheets
eve-trade --ship big --system Jita --export deals.csv --no-tui
```

---

## Command-Line Arguments

| Flag | Default | Description |
|---|---|---|
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
| `--no-tui` | `false` | Disable TUI and print to stdout |
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
- `src/ui/tui.rs`: `ratatui` UI layout and terminal state handling.

---

## Legal & EULA Compliance

This software is an external market analysis tool that interacts solely with CCP Games' public **EVE Swagger Interface (ESI)**. It does not automate any in-game actions, client inputs, or game memory, fully complying with the **EVE Online EULA** and Developer License Agreement.

---

## License

MIT License. See [LICENSE](LICENSE) for details.
