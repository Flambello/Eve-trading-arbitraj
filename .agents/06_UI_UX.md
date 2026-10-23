# UI/UX Design — Native Desktop GUI

EVE Traveling Merchant provides a **GUI-First Desktop Architecture**:
Built with [`egui`](https://github.com/emilk/egui) and [`eframe`](https://docs.rs/eframe), the native desktop application provides a sleek, high-contrast dark-mode sci-fi workstation running on Linux (Wayland & X11) and Windows 10/11. It features interactive sortable deal tables, real-time starting system autocomplete from SQLite SDE, multi-item knapsack corridor manifests, multi-leg beam route plotting, inter-hub trading matrices, live market re-scanning, and 1-click clipboard flight sheet export. The desktop GUI launches automatically on all platforms without requiring CLI flags.

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
- **Min ROI % Drag Slider**: Live adjustments starting at the enforced minimum of $10.0\%$.
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
- **Cockpit Theme Switcher**:
  - Interactive selection cards with live preview color chips (surface fill, primary accent, secondary accent).
  - Displays theme name, descriptive caption, and active status tag (`[ACTIVE]`).
  - Supports quick 1-click selection via interactive card clicks or radio buttons.
  - Automatically persists the selected theme to `config.toml` (`theme = "..."`) upon selection.
- **Skill Sliders**:
  - Broker Fee % (default 3.0%, adjustable down to 1.0% for pilots with *Broker Relations*).
  - Sales Tax % (default 8.0%, adjustable down to 3.6% for pilots with *Accounting*).
- **Active Scan Regions**: Checkbox toggles for the 5 major empire regions:
  - The Forge (Jita IV-4) — Region 10000002
  - Domain (Amarr VIII) — Region 10000043
  - Sinq Laison (Dodixie IX-20) — Region 10000032
  - Heimatar (Rens VI-8) — Region 10000030
  - Metropolis (Hek VIII-12) — Region 10000042
- **Feedback Toasts**: Temporary status banners providing tactile confirmation when items/systems/manifests/chains are copied.

### G. Visual Theme System & Cockpit Color Schemes
The desktop GUI architecture features a centralized, tokenized design system ([`src/ui/theme.rs`](file:///mnt/Projects/Eve%20trading%20arbitraj/src/ui/theme.rs)) offering four finely tuned cockpit color schemes:

1. **Sci-Fi Void (Default)** (`AppTheme::SciFiVoid` / `"sci_fi_void"`):
   - **Chassis & Surfaces**: Deep obsidian space navy (`#050712` app background, `#0a0e1e` cards, `#070b18` sidebars).
   - **Accents**: Neon telemetry cyan (`#00d2ff` primary) and Caldari/EVE gold (`#ffcd00` secondary).
   - **Aesthetic**: Immersive dark cockpit optimized for long trading sessions in low-light environments.
2. **Neutral Black** (`AppTheme::NeutralBlack` / `"neutral_black"`):
   - **Chassis & Surfaces**: True OLED pitch black (`#080808` app background, `#141414` cards, `#1c1c1c` strokes).
   - **Accents**: Crisp monochrome slate (`#e0e0e0` primary, `#a0a0a0` secondary).
   - **Aesthetic**: Minimalist, distraction-free stealth interface maximizing screen contrast on modern OLED displays.
3. **Black & Red** (`AppTheme::BlackRed` / `"black_red"`):
   - **Chassis & Surfaces**: Triglavian/pirate obsidian surfaces (`#050505` app background, `#101010` cards, `#2a0c10` strokes).
   - **Accents**: High-saturation combat crimson red (`#ff1e37` primary, `#ff4455` secondary).
   - **Aesthetic**: Aggressive tactical combat styling tailored for pirate faction and nullsec/lowsec haulers.
4. **Solar Light** (`AppTheme::SolarLight` / `"solar_light"`):
   - **Chassis & Surfaces**: Clean daylight workstation (`#f1f5f9` app background, `#ffffff` pure white cards, `#e2e8f0` borders).
   - **Accents**: Electric enterprise cobalt blue (`#0284c7` primary, `#38bdf8` secondary).
   - **Aesthetic & Typography**: High-clarity ergonomics featuring pure black-on-white text (`#000000` primary text on `#ffffff` cards) with crisp contrast for high-ambient-light trading.

#### Design System Architecture & Theme Tokens
All UI rendering dynamically references the active `ThemePalette`:
- **Dynamic Per-Frame Application (`apply_to_ctx`)**: Injects widget frame colors, selection fills, panel backgrounds, rounding, and text colors directly into `egui::Context` at the beginning of each frame.
- **Card & Panel Frame Helpers**: `palette.card_frame()`, `palette.sub_card_frame()`, `palette.top_panel_frame()`, `palette.central_panel_frame()`.
- **Button Helpers**: `palette.primary_button(text)` and `palette.secondary_button(text)` enforce theme-appropriate contrast, padding, and text colors without hardcoding.
- **Adaptive Semantic Badges**:
  - `palette.rank_silver()`: Adaptive silver badge ensuring $\ge 0.35$ contrast against both dark and pure white card backgrounds.
  - `palette.hub_badge_color(hub)`: Tailored trade hub indicator tags (Jita blue, Amarr gold, Dodixie green, Rens orange, Hek purple).
  - Semantic indicators: `success_green`, `warning_yellow`, `error_red`, `info_blue` maintain $\ge 0.30$ contrast across all 4 themes.

---

## 2. Desktop GUI Workstation Tabs

The application organizes New Eden trading operations into dedicated specialized tabs:

### A. Deals Tab (Cross-Station Arbitrage)
- **Filters**: Real-time substring filter on item names, origin/destination systems, or stations.
- **Sorting**: Multi-key sortable columns: `Profit / Jump`, `Profit / m³`, `Total Net Profit`, `ROI %`, `Jumps`, `Capital Cost`, `Buy System`, `Sell System`.
- **Dossier Drawer / Side Panel**: Configurable widescreen docked side panel (`dossier_side_panel = true`) or bottom drawer displaying detailed purchase, sale, and financial metrics.
- **Action Buttons**: 1-click clipboard copy for `📋 Item`, `📋 Buy Station`, `📋 Sell Station`, and full flight sheet.

### B. 📦 Packed Manifests Tab (Knapsack Corridor Consolidation)
- Groups all separate profitable items flowing along the identical source-destination station corridor.
- Knapsack cargo optimizer packs items up to ship hold volume and wallet capital limits.
- Displays individual commodities with required capital, volume share, and net profit.
- `📋 Shopping List`: 1-click copy formatted for EVE Online Multi-Buy window.

### C. 🗺️ Multi-Stop Route Tab (Greedy Sequential Itinerary)
- Chains sequential trades from the starting system.
- Shows total jumps, total profit, average profit per jump, capital required, and max volume used.
- `📋 Copy Autopilot Trail`: Copies the waypoint path (`Jita -> Perimeter -> Amarr`) to paste directly into in-game chat or notes.

### D. 🔗 Chain Route Plotter (Multi-Leg Optimizer)
- Beam-search optimizer supporting 1 to 8 legs, intermediate en-route pickups, and 0–5 detour jumps.
- Recycles capital and frees cargo hold volume upon arriving at destination stations.
- Step-by-step visual leg cards showing pickup stops, en-route bonus loads, and sell deliveries.

### E. 🏛️ Hub Arbitrage Matrix
- Matrix price comparison and trade corridor planner across the 5 primary empire trading hubs:
  - Jita IV-4 (The Forge)
  - Amarr VIII (Domain)
  - Dodixie IX-20 (Sinq Laison)
  - Rens VI-8 (Heimatar)
  - Hek VIII-12 (Metropolis)
- Filters for intra-hub trades, NPC seeded trade goods, and primary 4-4 stations only.

### F. ⚙️ Settings Tab (`Alt+S`)
- **Theme Switcher**: Live preview cards for 4 cockpit themes with automatic persistence to `config.toml`.
- **Pilot EVE SSO Authentication**: 1-click browser login via CCP OAuth 2.0 PKCE flow.
- **Tax & Fee Skills**: Broker fee % and sales tax % sliders reflecting *Broker Relations* and *Accounting* skills.
- **Regions & Audio**: Active region toggles and audio notification volume test.

---

## 3. In-Game Flight Sheet Integration (`Alt+N`)

The desktop application formats trade dossiers and routes directly into clean text blocks optimized for EVE Online's in-game Notepad (`Alt+N`):

```
═══════════════════════════════════════════════════════════
🚀 EVE TRADE FLIGHT SHEET: Pithum C-Type Medium Shield Booster
Net Profit: +12.40M ISK | Net ROI: +25.7% | Distance: 3 Jumps
═══════════════════════════════════════════════════════════

▶ 1. BUY AT: Jita IV - Moon 4 - Caldari Navy Assembly Plant (Jita)
     Quantity: 1 units | Buy Price: 48.20M ISK | Capital: 48.20M ISK | Vol: 100.0 m³

▶ 2. SELL AT: Amarr VIII (Oris) - Emperor Family Academy (Amarr)
     Sell Price: 60.60M ISK | Revenue: 60.60M ISK | Net Return: +12.40M ISK
═══════════════════════════════════════════════════════════
```

Pilots can paste these manifests into EVE Notepad or fleet chat, enabling instant 1-click station docking navigation using in-game link autolinking.
