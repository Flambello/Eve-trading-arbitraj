# EVE Traveling Merchant 🚀

[![CI](https://github.com/Flambello/eve-traveling-merchant/actions/workflows/ci.yml/badge.svg)](https://github.com/Flambello/eve-traveling-merchant/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Flambello/eve-traveling-merchant?color=orange&logo=github)](https://github.com/Flambello/eve-traveling-merchant/releases)
[![Flathub](https://img.shields.io/badge/Flathub-io.github.flambello.eve__merchant-blue?logo=flathub)](https://flathub.org/apps/io.github.flambello.eve_merchant)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Rust](https://img.shields.io/badge/Rust-1.78%2B-blue.svg?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Linux%20%28Wayland%20%2F%20X11%29-green.svg?logo=linux)](https://kernel.org/)

> **Location-Centric Cross-Station Arbitrage Engine & Multi-Item Corridor Route Planner for EVE Online**  
> Pure compiled Rust binary (`eve-merchant`) with zero runtime dependencies, maximum performance, and native desktop GUI / TUI interfaces.

---

## What is EVE Traveling Merchant?

Traditional trade finders often force you to haul single items, leave your hold mostly empty, or plan routes based on global galaxy spreads that require 40+ jumps through dangerous lowsec.

**EVE Traveling Merchant** solves this by:
1. **Scanning from where you actually are**: Analyzes your home solar system plus all **stargate-connected adjacent neighbor regions** via CCP's live EVE Swagger Interface (ESI) API.
2. **Packing your hold via Knapsack Optimization**: Groups multiple high-margin commodities along identical station-to-station corridors to fully utilize your ship volume (60,000 m³ / 30,000 m³) and ISK wallet budget.
3. **Calculating true Profit-Per-Jump (PPJ)**: Accounts for both the approach flight from your location to the buy station *and* the transit flight to the sell destination via Dijkstra pathfinding.
4. **Guaranteed Safety & Invariants**: Strictly enforces safe highsec space (security $\ge 0.45$) by default and filters out low-margin traps with an enforced minimum 10.0% net return on capital after all broker fees and sales taxes.

---

## Features

- ⚡ **Blazing Fast Native Rust**: Single compiled binary with asynchronous multi-region pagination and caching via `tokio` and `reqwest`.
- 📦 **Knapsack Multi-Item Cargo Manifests**: Consolidates separate profitable deals moving along the same corridor into packed hauler runs netting 25M–60M+ ISK per flight.
- 🔗 **Multi-Leg Chained Route Optimizer**: Beam-search route planner that chains consecutive pickup-delivery stops, supports en-route detour pickups, and recycles capital upon delivery.
- 🔥 **High-ROI Dead-End Recognition ($\ge 40\%$)**: Automatically flags and prioritizes lucrative regional dead ends and high-margin opportunities.
- 🚢 **Ship Profiles**:
  - **Big Hauler**: 60,000 m³ cargo capacity · 500,000,000 ISK budget.
  - **Small Hauler**: 30,000 m³ cargo capacity · 50,000,000 ISK budget.
- 🛡️ **Safe Highsec Routing**: Dijkstra pathfinding penalizes dangerous space, completely avoiding lowsec and nullsec gate networks.
- 🏛️ **Station Docking Pool Toggle**: Choose between **NPC Stations Only** (guaranteed 100% public docking access) or include **Player Citadels / Upwell Structures**.
- 💡 **Real-Time SDE Autocomplete**: Type any solar system name for instant prefix/substring search, security rating pills, and parent region tags from the embedded universe SQLite export.
- 🔔 **Audible Sound Alerts**: Freedesktop audio alerts and desktop notifications when deals yielding $\ge 100\%$ ROI are detected.
- 🖥️ **Native Desktop GUI**: Built with `egui` and `eframe` for Linux (native Wayland and X11), featuring a dark sci-fi aesthetic, sortable dossier tables, and 1-click clipboard buttons for in-game pasting.
- 💻 **Terminal Dashboard (TUI)**: Fully keyboard-driven `ratatui` terminal interface for SSH sessions and terminal enthusiasts.
- 📊 **Headless Scripting & CSV Export**: Stream formatted tables to stdout or export deal rosters directly to CSV via `--no-ui` and `--export`.

---

## Installation

### Option 1: Install from Flathub (Recommended for Linux Desktops)

```bash
flatpak install flathub io.github.flambello.eve_merchant
flatpak run io.github.flambello.eve_merchant
```

### Option 2: Pre-compiled GitHub Release Binary

Download the latest release tarball for Linux x86_64:

```bash
# Download latest release archive
wget https://github.com/Flambello/eve-traveling-merchant/releases/latest/download/eve-traveling-merchant-linux-x86_64.tar.gz

# Extract and run
tar -xzf eve-traveling-merchant-linux-x86_64.tar.gz
cd eve-traveling-merchant
./eve-merchant --gui
```

### Option 3: Install via Cargo

```bash
cargo install eve-trade-scout
eve-merchant
```

### Option 4: Build from Source

**Prerequisites:** Rust 1.78+ (via [rustup](https://rustup.rs/)) and GCC/pkg-config headers:

```bash
# Fedora Linux:
sudo dnf install gcc pkg-config git -y

# Ubuntu / Debian:
sudo apt install build-essential pkg-config git -y

# Arch Linux:
sudo pacman -S base-devel git
```

**Build & Run:**

```bash
git clone https://github.com/Flambello/eve-traveling-merchant.git
cd eve-traveling-merchant

# Verify tests pass
make test

# Build optimized release binary
make release

# Run desktop GUI
./target/release/eve-merchant --gui
```

---

## Usage Guide

### 1. Native Desktop GUI (Default)

Launch via desktop application launcher or terminal:

```bash
./start-gui.sh
# or: eve-merchant --gui
```

- **Top Bar Controls**: Select Ship profile (Big / Small), type starting system with real-time autocomplete, toggle adjacent regions, and click **⚡ SCAN MARKET NOW**.
- **Interactive Dossier**: Click any deal row to expand exact buy/sell stations, volume, capital required, and 1-click copy buttons (`📋 Item`, `📋 Buy Station`, `📋 Sell Station`).
- **📦 Packed Manifests Tab**: View knapsack cargo lists grouping multiple items into unified origin-destination flights. Use `📋 Shopping List` to copy item names and quantities for in-game multi-buy.
- **🔗 Chain Route Plotter**: Configure maximum legs (1–8) and detour tolerance (0–5 jumps) to plot multi-hop trading expeditions. Click `📋 Autopilot Trail` to paste the waypoint sequence directly into EVE chat.

### 2. Terminal Interactive Mode (TUI)

```bash
eve-merchant --tui
```

**Keybindings:**
- `[Tab]` — Switch between Deals Table, Planned Route, and Settings
- `[↑] / [k]` — Select previous deal
- `[↓] / [j]` — Select next deal
- `[q] / [Esc]` — Quit

### 3. Headless Scripting & Automation

```bash
# Scan starting from Jita with Big Hauler (outputs formatted table)
eve-merchant --ship big --system Jita --no-ui

# Scan safe highsec routes with Small Hauler starting from Amarr
eve-merchant --ship small --system Amarr --safe --no-ui

# Scan specific regions (The Forge + Domain) and show top 50 deals
eve-merchant --regions 10000002,10000043 --top 50 --no-ui

# Export ranked deals to CSV file
eve-merchant --ship big --system Jita --export deals.csv
```

---

## CLI Reference

| Flag | Default | Description |
|---|---|---|
| `--gui` | `auto` | Force desktop GUI window mode (`egui` native window; default when display server is active) |
| `--tui` | `false` | Force terminal TUI mode (`ratatui`) |
| `--no-ui` | `false` | Disable interactive interfaces (GUI & TUI) and output directly to console |
| `--no-tui` | `false` | Disable interactive TUI and output directly to console |
| `--auto-refresh` | `false` | Automatically refresh market data every 5 minutes in GUI mode |
| `-s, --ship <SHIP>` | `big` | Ship profile (`big`: 60k m³/500M or `small`: 30k m³/50M) |
| `--system <SYSTEM>` | `Jita` | Starting solar system name |
| `--adjacent-regions` | `true` | Automatically scan stargate-connected neighbor regions |
| `--regions <IDS>` | `auto` | Comma-separated region IDs (overrides adjacent region scan) |
| `--safe` | `true` | Avoid lowsec/nullsec gates (Highsec only) |
| `--unsafe-routes` | `false` | Allow lowsec and nullsec routes |
| `--npc-only` | `true` | Restrict search strictly to NPC stations (preferred) |
| `--allow-structures` | `false` | Include player-owned Citadels / Upwell structures |
| `--min-profit <ISK>` | `1,000,000` | Minimum net profit in ISK |
| `--min-spread <PCT>` | `3.0` | Minimum gross price spread (%) |
| `--min-roi <PCT>` | `10.0` | Enforced minimum net ROI % after taxes |
| `-t, --top <N>` | `25` | Number of deals in CLI summary |
| `--max-stops <N>` | `5` | Maximum stops in planned route |
| `--route` | `false` | Print planned multi-stop route in CLI mode |
| `--export <PATH>` | — | Export discovered deals to CSV file |
| `--update-sde` | — | Force re-download / update of SDE SQLite database |

---

## Packaging & Distribution

### Building the Flatpak Locally

```bash
# Install flatpak-builder
sudo dnf install flatpak-builder -y  # on Fedora

# Build and install the Flatpak package locally
flatpak-builder --user --install --force-clean build-dir packaging/flatpak/io.github.flambello.eve_merchant.yml
```

### Validating Desktop & Metadata Files

```bash
# Validate desktop entry
desktop-file-validate packaging/flatpak/io.github.flambello.eve_merchant.desktop

# Validate AppStream metainfo XML
appstreamcli validate --no-net packaging/flatpak/io.github.flambello.eve_merchant.metainfo.xml
```

---

## Contributing

We welcome contributions from pilots, developers, and open-source enthusiasts! Please check out [CONTRIBUTING.md](CONTRIBUTING.md) for architecture guidelines, setup steps, and the testing protocol.

---

## Legal & EULA Compliance

This software is an external market analysis aid that interacts strictly with CCP Games' public **EVE Swagger Interface (ESI)**. It does not automate any in-game actions, client inputs, or game memory, fully complying with the **EVE Online EULA** and CCP Developer License Agreement.

*EVE Online and the EVE logo are the registered trademarks of CCP hf. All rights reserved.*

---

## License

Distributed under the **GNU General Public License v3.0** (GPL-3.0). See [LICENSE](LICENSE) for full text.
