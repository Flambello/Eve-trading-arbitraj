# EVE Traveling Merchant 🚀

[![CI](https://github.com/Flambello/eve-traveling-merchant/actions/workflows/ci.yml/badge.svg)](https://github.com/Flambello/eve-traveling-merchant/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Flambello/eve-traveling-merchant?color=orange&logo=github)](https://github.com/Flambello/eve-traveling-merchant/releases)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Rust](https://img.shields.io/badge/Rust-1.78%2B-blue.svg?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Linux%20%7C%20Windows-green.svg)](https://github.com/Flambello/eve-traveling-merchant/releases)

> **Location-Centric Cross-Station Arbitrage Engine & Multi-Item Corridor Route Planner for EVE Online**  
> Pure compiled Rust binary (`eve-merchant`) with zero runtime dependencies, maximum performance, and a native desktop graphical interface.

---

## What is EVE Traveling Merchant?

Traditional trade finders often force you to haul single items, leave your cargo hold mostly empty, or plan routes based on global galaxy spreads that require 40+ jumps through dangerous lowsec.

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
- 🖥️ **Native Desktop GUI**: Built with `egui` and `eframe` for Windows and Linux (native Wayland and X11), featuring a dark sci-fi aesthetic, sortable dossier tables, and 1-click clipboard buttons for in-game pasting.
- 📊 **Built-in CSV & Flight Sheet Export**: Export filtered deals, packed corridor manifests, and multi-leg chain routes directly to CSV files and clipboard via dedicated GUI buttons.

---

## Installation & Getting Started

EVE Traveling Merchant runs **completely out-of-the-box with zero initial configuration**. Both pre-compiled binaries and source builds use an embedded public Client ID (`5793fc7aa2cb49e385d3cfb5d52061c7`) registered with CCP. **Zero developer portal registration or API secret setup is required.**

### Option 1: Standalone Linux Release Binary (.tar.gz)

Download the latest pre-compiled release tarball for Linux x86_64:

```bash
# Download latest Linux release archive
wget https://github.com/Flambello/eve-traveling-merchant/releases/latest/download/eve-traveling-merchant-linux-x86_64.tar.gz

# Extract archive
tar -xzf eve-traveling-merchant-linux-x86_64.tar.gz
cd eve-traveling-merchant

# Launch the desktop GUI interface directly (launches GUI by default)
./eve-merchant      # or via launcher: ./start-gui.sh
```

### Option 2: Portable Windows Release Bundle (.zip)

Download the latest standalone `.zip` bundle for Windows x86_64 from [GitHub Releases](https://github.com/Flambello/eve-traveling-merchant/releases/latest):

1. Download `eve-traveling-merchant-windows-x86_64.zip`.
2. Extract the archive into a folder of your choice.
3. Double-click `eve-merchant.exe` (or `start-gui.bat`) to launch the native desktop GUI. No command-line arguments or console windows are required.

### Option 3: Install via Cargo

```bash
cargo install eve-trade-scout
eve-merchant
```

### Option 4: Build from Source

**Prerequisites:** Rust 1.78+ (via [rustup](https://rustup.rs/)) and standard C build tools:

```bash
# Fedora / RHEL Linux:
sudo dnf install gcc pkg-config git -y

# Ubuntu / Debian Linux:
sudo apt install build-essential pkg-config git -y

# Arch Linux:
sudo pacman -S base-devel git
```

**Build & Run:**

```bash
git clone https://github.com/Flambello/eve-traveling-merchant.git
cd eve-traveling-merchant

# Run test suites
make test

# Build optimized release binary
make release

# Run desktop GUI
./target/release/eve-merchant
```

---

## 🔐 EVE Online SSO & Autopilot (Zero-Registration)

EVE Traveling Merchant features direct in-game integration with the official CCP EVE Single Sign-On (SSO) v2 API:
- 🛰️ **Live Pilot Location Sync**: Automatically queries your active character's solar system on startup and deal calculation.
- 🎯 **1-Click Autopilot Waypoint Injection**: Transmits planned trade stops and multi-hop routes directly to your active in-game autopilot.

### Works Out-of-the-Box (No Developer Registration Required)
You **do not** need to register as a third-party developer on `developers.eveonline.com`. The application comes pre-configured with a public Client ID using the **OAuth 2.0 PKCE flow (RFC 7636)**:
1. Open the **Settings** tab in the desktop GUI (`Alt+S`).
2. Click **Authenticate Pilot via EVE SSO**.
3. Your default web browser will open CCP's official login page (`login.eveonline.com`).
4. Select your character and authorize access.
5. The local application loopback listener (`http://127.0.0.1:37373/callback`) securely receives the authorization code, exchanges it for character tokens, and your pilot profile connects instantly.

### Optional Developer Override (`EVE_CLIENT_ID`)
Advanced users who prefer running their own registered CCP application can override the default public Client ID without modifying source code:
- **Environment Variable**: `export EVE_CLIENT_ID="your_custom_client_id"` (see [.env.example](.env.example))
- **Settings Tab**: Enter your custom Client ID directly into the GUI Settings panel.
- **Local Config File**: Save your ID to `~/.config/eve-traveling-merchant/developer_client_id`.

*(Note: In accordance with RFC 7636 for public desktop clients, **no client secret is required, supported, or used**).*

---

## 🛡️ Security, Privacy & Token Storage

Your character credentials and New Eden assets are protected by design:
- **Zero Secrets in Binaries**: No private API secrets, developer passwords, or client secrets are compiled into the binary or stored in git.
- **Secure Local Storage**:
  - **Linux**: OAuth tokens are saved strictly on your local machine at `~/.config/eve-traveling-merchant/sso_token.json` protected by restricted file permissions (`chmod 0600` on the file and `chmod 0700` on the directory).
  - **Windows**: Tokens are saved at `%APPDATA%\eve-traveling-merchant\sso_token.json` within your protected Windows user profile.
- **Ephemeral PKCE Derivation**: Cryptographic code verifiers and SHA-256 challenges are generated in-memory per session using system CSPRNG (`ring`) and never logged.
- **In-Memory Credential Redaction**: Access tokens and refresh tokens are masked as `[REDACTED]` in internal debugging representations and structured application logs.
- **Minimal Read-Only Scopes**: The application requests only the minimum permissions necessary:
  - `esi-location.read_location.v1` (reads current system)
  - `esi-location.read_ship_type.v1` (reads current active ship)
  - `esi-ui.write_waypoint.v1` (injects autopilot destination)
- The application **never** requests or accesses wallet balances, asset registries, character skills, private EveMail, or market placement scopes.

---

## Usage Guide

### Launching the Desktop GUI

Launch via your desktop application launcher, application menu, or terminal:

```bash
./start-gui.sh
# or simply:
./eve-merchant
```

- **Top Bar Controls**: Select Ship profile (Big / Small), type starting system with real-time autocomplete, toggle adjacent regions, and click **⚡ SCAN MARKET NOW**.
- **Interactive Dossier**: Click any deal row to expand exact buy/sell stations, volume, capital required, and 1-click copy buttons (`📋 Item`, `📋 Buy Station`, `📋 Sell Station`).
- **📦 Packed Manifests Tab**: View knapsack cargo lists grouping multiple items into unified origin-destination flights. Use `📋 Shopping List` to copy item names and quantities for in-game multi-buy.
- **🔗 Chain Route Plotter**: Configure maximum legs (1–8) and detour tolerance (0–5 jumps) to plot multi-hop trading expeditions. Click `📋 Autopilot Trail` to paste the waypoint sequence directly into EVE chat.
- **📊 CSV Export**: Dedicated 1-click buttons to export filtered deals and manifests to CSV files with clipboard integration.

---

## Configuration Files & Templates

The application creates its configuration file automatically upon first launch:
- **Linux**: `~/.config/eve-traveling-merchant/config.toml` (with legacy fallback to `~/.config/eve-trade-scout/config.toml`)
- **Windows**: `%APPDATA%\eve-traveling-merchant\config.toml`

Reference templates are included in the repository:
- **[.env.example](.env.example)**: Environment variable template documenting optional overrides such as `EVE_CLIENT_ID` and `RUST_LOG`.
- **[config.example.toml](config.example.toml)**: Fully annotated TOML configuration template detailing ship profiles, scan settings, tax rates, and SSO callback options.

---

## 🛠️ Build & Distribution Guide

### Linux Compilation & Bundling

```bash
# Prerequisites (Fedora / RHEL):
sudo dnf install -y gcc pkg-config git rust cargo desktop-file-utils tar gzip

# Prerequisites (Ubuntu / Debian):
sudo apt install -y build-essential pkg-config git desktop-file-utils tar gzip

# Build optimized release binary
cargo build --release --bin eve-merchant

# Assemble standalone distributable Linux tarball (.tar.gz) and SHA256 checksums
make dist

# Inspect created archive
tar -ztvf eve-traveling-merchant-linux-x86_64.tar.gz
```

### Windows Compilation & Bundling

#### Native Windows Build (PowerShell)
```powershell
# Build optimized release binary (compiled with windows_subsystem = "windows" to prevent console popup)
cargo build --release --bin eve-merchant

# Assemble portable standalone zip archive
$Dist = "eve-traveling-merchant-windows-x86_64"
New-Item -ItemType Directory -Force -Path $Dist
Copy-Item "target\release\eve-merchant.exe", "README.md", "LICENSE", "CHANGELOG.md", "start-gui.bat" -Destination $Dist
Compress-Archive -Path "$Dist\*" -DestinationPath "$Dist.zip" -Force
```

#### Cross-Compiling for Windows from Linux (MinGW-w64)
```bash
# Install MinGW toolchain (Fedora)
sudo dnf install mingw64-gcc -y

# Add Windows Rust target
rustup target add x86_64-pc-windows-gnu

# Compile Windows binary with embedded SQLite
cargo build --release --target x86_64-pc-windows-gnu --bin eve-merchant
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
