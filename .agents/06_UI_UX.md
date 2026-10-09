# UI/UX Design — Desktop GUI & Terminal Interfaces

EVE Traveling Merchant provides **two user interface modes**:
1. **Native Desktop GUI Window** (built with [`egui`](https://github.com/emilk/egui) / [`eframe`](https://docs.rs/eframe)): A sleek, dark-mode sci-fi desktop window running natively on Linux Fedora (Wayland & X11). Features interactive deal tables, real-time starting system autocomplete, packed corridor manifests, clipboard copy buttons for in-game pasting, visual multi-hop route cards, live market re-scanning, and quick hub presets. This launches automatically when a graphical desktop is detected.
2. **Terminal Interactive Interface (TUI)** (built with [`ratatui`](https://ratatui.rs/) and `crossterm`): High-performance keyboard-navigated terminal dashboard for terminal enthusiasts and headless/SSH sessions.
3. **Headless CLI Output**: Scriptable console output (`--no-ui`) with optional CSV export (`--export`).

---

## 1. Native Desktop GUI Window (`egui` / `eframe`)

The graphical desktop application provides an immersive, high-contrast dark sci-fi workstation tuned for active EVE Online trading:

### A. Persistent Controls & Executive Telemetry Header
- **⚡ "SCAN MARKET NOW" Action Button**: Launches live asynchronous order book fetch from CCP's ESI API in a background thread. Keeps the UI at 60 FPS without hanging or freezing.
- **Ship Profile Segmented Switcher**: Instant 1-click toggle between:
  - 🚢 **Big Hauler**: `60,000 m³` volume cap · `500,000,000 ISK` capital budget.
  - 🚀 **Small Hauler**: `30,000 m³` volume cap · `50,000,000 ISK` capital budget.
- **Starting System Input with Real-Time Autocomplete**:
  - Live system name resolution from SQLite SDE database as you type.
  - Active suggestion pills for instant 1-click selection of matching systems.
  - Real-time security rating pill (`[1.0]`) and parent region indicator (`The Forge`).
  - Quick-select buttons for major trade hubs: `Jita`, `Amarr`, `Dodixie`, `Rens`.
- **🌐 Stargate-Connected Adjacent Regions Toggle**:
  - Automatically identifies all stargate-connected highsec neighbor regions adjacent to your starting region (e.g. Lonetrek, Citadel, Black Rise from The Forge).
  - Active region counter tag (`(X regions)`).
- **Safety Switch**: `🛡️ Safe Highsec Only` toggle enforces Dijkstra avoidance of systems with security $<0.45$. Lowsec and Nullsec space is strictly excluded.
- **Station Access Pool Switch**: Instant 1-click toggle between:
  - 🏛️ **NPC Stations Only** (Preferred & Default): Strictly targets NPC stations with guaranteed 100% public docking.
  - 🏰 **+ Citadels Included**: Adds player-owned Citadels / Upwell Structures to the arbitrage search pool.
- **Min ROI % Drag Slider**: Live adjustments starting at the enforced minimum of $15.0\%$.
- **Max Jumps Filter**: Slider to restrict maximum travel jumps (or set to 0 for unlimited).
- **Executive KPI Strip**: Real-time badges displaying Top Deal Profit (`+XX.XX M ISK`) and Top Profit Per Jump (`XX.XX M / jump`).

### B. Deals Table & Interactive Dossier
- **Search & Multi-Key Sorting**:
  - Live substring filter by item name with an instant `✖ Clear` button.
  - Sorting criteria: `Profit / Jump`, `Profit / m³`, `Total Profit`, `Net ROI %`, `Fewest Jumps`, `Lowest Cost`.
  - Direction toggle: `▼ Descending` / `▲ Ascending`.
  - Minimum Net Profit threshold slider (in Millions ISK).
- **Deals Grid**:
  - Gold (`#1`), Silver (`#2`), Bronze (`#3`) visual rank highlights.
  - Security rating badges on system names (`[1.0] Jita`, `[0.9] Amarr`).
  - Glowing Emerald Net Profit and Net ROI % tags.
  - High-yield badges (`🔥 40%+ ROI`) highlighting profitable regional dead ends and remote pockets.
  - **PROFIT/m³** density column for maximizing cargo hold value.
  - **SHIP UTIL** telemetry showing percentage of ship hold (`% v`) and wallet budget (`% isk`) utilized.
  - 1-click `📋 Item`, `📋 Buy`, and `📋 Sell` clipboard copy buttons.
- **Expandable Deal Dossier**:
  - Clicking any row expands a detailed 3-column trade breakdown:
    - **Purchase Column**: Exact station name, system, unit price, quantity, capital required, and 1-click copy station button.
    - **Sale Column**: Destination station name, system, unit price, gross revenue, spread %, and copy station button.
    - **Financials Column**: Net profit, ROI %, profit per jump, profit per m³, cargo volume used, and travel telemetry.

### C. 📦 Packed Manifests (Knapsack Multi-Item Trade Corridors)
- Consolidates multiple high-margin items flying along identical origin-to-destination system corridors into single packed hauler trips.
- Respects the 60,000 m³ / 500M ISK (Big Hauler) or 30,000 m³ / 50M ISK (Small Hauler) knapsack constraints.
- Multi-item shopping list view with item quantities, volumes, capital, and individual profits.
- 1-Click `📋 Shopping List` clipboard copy button formatted for easy in-game multi-buy pasting.

### D. Visual Multi-Stop Route Flight Plan
- **Overall Route KPI Card**: Total jumps, total net profit, average profit per jump, peak cargo space filled, and capital required.
- **1-Click Autopilot Trail Copy**: Formats the entire route chain (`Jita -> Perimeter -> Amarr`) into the clipboard for immediate pasting into in-game chat or route planner.
- **Step-by-Step Flight Cards**: Visual leg timeline displaying pickup action, flight jumps, and delivery action.

### E. 🔗 Chain Route Plotter (Multi-Leg Optimizer)
- **Interactive Controls**:
  - `Max Legs` slider: Configurable 1 to 8 legs (default 4).
  - `En-Route Pickups` toggle: Enable/disable searching for intermediate pickups.
  - `Detour Tolerance` slider: 0 to 5 extra jumps allowed for pickups off the direct path (disabled when strict mode is active).
  - `🔗 FIND CHAIN ROUTES` button: Launches async background thread beam search.
- **Top Chains Strip**: Horizontal selector showing discovered chain routes ranked by net profit and profit per jump.
- **Visual Dossier**:
  - Executive telemetry: Total Jumps, Net Profit, Net ROI %, Profit/Jump, Capital Invested, Peak Cargo hold utilized.
  - Step-by-step visual leg timeline with distinct badges for primary pickups, en-route pickups (`+N detour jumps`), and sell deliveries.
  - Detailed knapsack cargo tracking: item quantities, volumes, capital, destinations, and freed cargo on sell.
  - 1-Click `📋 Copy Autopilot Route` button for immediate in-game trail navigation.

### F. Settings & Tax Skill Customizer
- **Skill Sliders**:
  - Broker Fee % (default 3.0%, adjustable down to 1.0% for pilots with *Broker Relations*).
  - Sales Tax % (default 8.0%, adjustable down to 3.6% for pilots with *Accounting*).
- **Active Scan Regions**: Checkbox toggles for the 5 major empire regions:
  - The Forge (Jita IV-4) — Region 10000002
  - Domain (Amarr VIII) — Region 10000043
  - Sinq Laison (Dodixie IX-20) — Region 10000032
  - Heimatar (Rens VI-8) — Region 10000030
  - Metropolis (Hek VIII-12) — Region 10000042
- **Feedback Toasts**: Temporary cyan status banners providing tactile confirmation when items/systems/manifests/chains are copied.

---

## 2. Terminal Interactive Interface (TUI)

```
╔══════════════════════════════════════════════╗
║        EVE Trade Scout  v1.0                 ║
║     Arbitrage Route Planner for EVE Online   ║
╚══════════════════════════════════════════════╝

? Select ship profile:
  ❯  1. Big Hauler   (60 000 m³ / 500 M ISK)
     2. Small Hauler (30 000 m³ /  50 M ISK)

? Starting system: Jita
  ✔ Resolved: Jita (30000142) — The Forge — Security: 0.95

? Regions to scan:
  [x] The Forge
  [x] Domain
  [ ] Heimatar
  [ ] Sinq Laison
  [ ] Metropolis

? Safe route only (avoid lowsec/nullsec)? [Y/n]: Y

⠸ Fetching market data for 2 regions...
⠸ Processing 1 243 892 orders...
✔ Found 847 profitable deals in 4.3 s
```

---

## Main Results Table

```
╭────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│  Top 20 Arbitrage Deals  [Big Hauler · Starting: Jita · Safe Route]                                    │
╰────────────────────────────────────────────────────────────────────────────────────────────────────────╯
 #   Item                  Buy System      Buy Price  Sell System     Sell Price  Qty   Vol(m³) Cost(M)  Profit(M)  ROI%   PPJ
────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
 1   Pithum C-Type MedSh…  Jita               48.2M   Amarr              60.6M      1    100.0    48.2     12.4    +25.7%  4.13
 2   Tritanium              Jita                5.00   Rens                6.62  5000k  50 000    25.0      8.1    +32.4%  2.70
 3   PLEX                   Jita               4.75M   Dodixie            5.30M     10      0.5   47.5      5.5    +15.8%  1.83
 4   Damage Control II      Perimeter        602.0k   Amarr             704.0k     50    750.0   30.1      5.1    +16.9%  1.70
 5   Caldari Navy Raven     Jita             420.0M   Hek               440.3M      1  50 000   420.0     20.3    +17.2%  1.69
 ...
────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
 Filters: Net ROI ≥ 15% after taxes · Spread ≥ 3% · Net profit ≥ 1 M ISK · Max volume: 60 000 m³
```

**Column definitions:**

| Column | Description |
|---|---|
| `#` | Rank by profit-per-jump |
| `Item` | Item name (truncated to 22 chars) |
| `Buy System` | System where you buy (cheapest sell order) |
| `Buy Price` | Unit purchase price at source order |
| `Sell System` | System where you sell (highest buy order) |
| `Sell Price` | Unit sale price at destination order |
| `Qty` | Units you can buy (respecting volume + budget) |
| `Vol(m³)` | Total cargo volume for that quantity |
| `Cost(M)` | Capital required in millions ISK |
| `Profit(M)` | Net profit in millions ISK (after taxes) |
| `ROI%` | **Net Profitability %** after taxes & fees (`net_profit / capital_required * 100`) |
| `PPJ` | **Profit Per Jump** in millions ISK |

---

## Route Plan Display

After viewing the deal list, user can press `R` to build a route:

```
╭─────────────────────────────────────────────────────────────╮
│  Planned Trade Route  (3 stops · 8 jumps · 25.0 M ISK net)  │
╰─────────────────────────────────────────────────────────────╯

 [START]  Jita  (home)
    │
    │  2 jumps
    ▼
 [STOP 1]  Perimeter  →  buy 50x Damage Control II  (30.1 M ISK)
    │        sell at: Amarr
    │  4 jumps
    ▼
 [STOP 2]  Amarr  →  sell 50x Damage Control II  (+5.1 M ISK)
    │        then buy 1x PLEX  (47.5 M ISK)
    │  sell at: Dodixie
    │  2 jumps
    ▼
 [STOP 3]  Dodixie  →  sell 1x PLEX  (+5.5 M ISK)
    │
    ▼
 [END]

 Total jumps:   8
 Total profit:  25.0 M ISK
 Profit/jump:   3.1 M ISK/jump
 Volume used:   750.5 / 60 000 m³
 Capital used:  77.6 / 500.0 M ISK
```

---

## Interactive Menu

After results are shown:

```
[D] Show all deals list
[R] Build route from top deals
[F] Change filters (min profit, min spread)
[S] Change ship profile
[L] Change starting location
[E] Export deals to CSV
[Q] Quit
```

---

## Progress Indicators

All long-running operations show a Rich spinner:

```
⠸ Downloading SDE database (115 MB)... [=====>    ] 62%
⠸ Fetching The Forge market orders (page 3 / 12)...
⠸ Computing jump distances for 847 deals...
✔ Done in 4.3 s
```

---

## Error Display

```
⚠ Warning: Could not resolve system "Jitaa" — did you mean "Jita"?
✘ Error: ESI rate limit hit (HTTP 420). Waiting 60 s before retry...
⚠ Deal #4: source system "Stacmon" is lowsec — excluded (safe mode on)
```

---

## Config Display (`--show-config`)

```
Current configuration:
  Ship profile:    Big Hauler (60 000 m³ / 500 M ISK)
  Home system:     Jita
  Regions:         The Forge (and adjacent regions)
  Min profit:      1 000 000 ISK
  Min spread:      5.0%
  Broker fee:      3.0%
  Sales tax:       8.0%
  Safe route only: Yes
  Config file:     ~/.config/eve-traveling-merchant/config.toml
```

---

## CLI Arguments

```
Usage: eve-merchant [OPTIONS]

Options:
  -s, --ship <SHIP>              Ship profile: 'big' (60k m³, 500M) or 'small' (30k m³, 50M)
      --system <SYSTEM>          Starting solar system name [default: Jita]
      --regions <REGIONS>        Comma-separated region IDs [default: 10000002]
      --no-adjacent              Disable automatic scanning of stargate-adjacent neighbor regions
      --max-jumps <MAX_JUMPS>    Maximum travel jumps allowed from starting system
      --safe                     Force highsec safe route only (avoid lowsec/nullsec)
      --unsafe-routes            Allow lowsec and nullsec routes
      --npc-only                 Restrict search strictly to NPC stations (guaranteed public docking)
      --allow-structures         Allow player-owned structures (Upwell Citadels) in search pool
      --min-profit <MIN_PROFIT>  Minimum net profit in ISK [default: 1000000]
      --min-spread <MIN_SPREAD>  Minimum price spread percentage [default: 3.0]
      --min-roi <MIN_ROI>        Minimum net ROI % after taxes [default: 15.0]
  -t, --top <TOP>                Number of top deals to display in CLI mode [default: 25]
      --max-stops <MAX_STOPS>    Maximum stops in route [default: 5]
      --route                    Print planned multi-stop route
      --gui                      Force desktop GUI window mode (egui native window)
      --tui                      Force terminal TUI mode (ratatui)
      --no-ui                    Disable interactive UI (GUI & TUI) and output directly to console
      --no-tui                   Disable interactive TUI and output directly to console
      --export <EXPORT>          Export found deals to CSV file
      --update-sde               Force update / re-download of SDE database
  -h, --help                     Print help
```
