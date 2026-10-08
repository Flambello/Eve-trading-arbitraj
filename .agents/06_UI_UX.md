# UI/UX Design — Desktop GUI & Terminal Interfaces

EVE Trade Scout provides **two user interface modes**:
1. **Native Desktop GUI Window** (built with [`egui`](https://github.com/emilk/egui) / [`eframe`](https://docs.rs/eframe)): A sleek, dark-mode sci-fi desktop window running natively on Linux Fedora (Wayland & X11). Features interactive deal tables, clipboard copy buttons for in-game pasting, visual multi-hop route cards, live market re-scanning, and quick hub presets. This launches automatically when a graphical desktop is detected.
2. **Terminal Interactive Interface (TUI)** (built with [`ratatui`](https://ratatui.rs/) and `crossterm`): High-performance keyboard-navigated terminal dashboard for terminal enthusiasts and headless/SSH sessions.
3. **Headless CLI Output**: Scriptable console output (`--no-ui`) with optional CSV export (`--export`).

---

## Startup Flow

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
╭──────────────────────────────────────────────────────────────────────────────────────────╮
│  Top 20 Arbitrage Deals  [Big Hauler · Starting: Jita · Safe Route]                      │
╰──────────────────────────────────────────────────────────────────────────────────────────╯
 #   Item                  Buy @ System    Sell @ System  Qty   Vol(m³) Cost(M)  Profit(M)  ROI%   PPJ
────────────────────────────────────────────────────────────────────────────────────────────────────
 1   Pithum C-Type MedSh…  Jita            Amarr           1    100.0    48.2     12.4    +25.7%  4.13
 2   Tritanium              Jita            Rens        5000k  50 000    25.0      8.1    +32.4%  2.70
 3   PLEX                   Jita            Dodixie        10      0.5   47.5      5.5    +15.8%  1.83
 4   Damage Control II      Perimeter       Amarr          50    750.0   30.1      5.1    +16.9%  1.70
 5   Caldari Navy Raven     Jita            Hek             1  50 000   420.0     20.3    +17.2%  1.69
 ...
────────────────────────────────────────────────────────────────────────────────────────────────────
 Filters: Net ROI ≥ 15% after taxes · Spread ≥ 3% · Net profit ≥ 1 M ISK · Max volume: 60 000 m³
```

**Column definitions:**

| Column | Description |
|---|---|
| `#` | Rank by profit-per-jump |
| `Item` | Item name (truncated to 22 chars) |
| `Buy @ System` | System where you buy (cheapest sell order) |
| `Sell @ System` | System where you sell (highest buy order) |
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
  Regions:         The Forge, Domain
  Min profit:      1 000 000 ISK
  Min spread:      5.0%
  Broker fee:      3.0%
  Sales tax:       8.0%
  Safe route only: Yes
  Config file:     ~/.config/eve-trade-scout/config.toml
```

---

## CLI Arguments

```
Usage: eve-trade [OPTIONS]

Options:
  -s, --ship <SHIP>              Ship profile: 'big' (60k m³, 500M) or 'small' (30k m³, 50M)
      --system <SYSTEM>          Starting solar system name [default: Jita]
      --regions <REGIONS>        Comma-separated region IDs [default: 10000002]
      --safe                     Force highsec safe route only (avoid lowsec/nullsec)
      --unsafe-routes            Allow lowsec and nullsec routes
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
