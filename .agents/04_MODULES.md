# Module Specifications — Rust

Detailed function signatures and contracts for every module.

---

## `src/config.rs`

```rust
use serde::{Deserialize, Serialize};

// ── Ship profiles ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct ShipProfile {
    pub name:       &'static str,
    pub max_volume: f64,
    pub max_budget: f64,
}

pub const BIG_HAULER: ShipProfile = ShipProfile {
    name: "Big Hauler", max_volume: 60_000.0, max_budget: 500_000_000.0,
};
pub const SMALL_HAULER: ShipProfile = ShipProfile {
    name: "Small Hauler", max_volume: 30_000.0, max_budget: 50_000_000.0,
};

pub fn profile_from_str(s: &str) -> anyhow::Result<ShipProfile>;
// Returns BIG_HAULER for "big", SMALL_HAULER for "small", else error.

// ── Cockpit Themes ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AppTheme {
    #[default]
    SciFiVoid,
    NeutralBlack,
    BlackRed,
    SolarLight,
}

impl AppTheme {
    pub const ALL: &'static [AppTheme] = &[
        AppTheme::SciFiVoid,
        AppTheme::NeutralBlack,
        AppTheme::BlackRed,
        AppTheme::SolarLight,
    ];

    pub fn display_name(&self) -> &'static str;
    pub fn description(&self) -> &'static str;
    pub fn is_dark(&self) -> bool;
    pub fn as_str(&self) -> &'static str;
    pub fn palette(&self) -> crate::ui::theme::ThemePalette;
}

impl std::fmt::Display for AppTheme { /* prints display_name() */ }
impl std::str::FromStr for AppTheme { /* parses canonical and alias strings */ }

// ── User config ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct UserConfig {
    pub ship: ShipConfig,
    pub location: LocationConfig,
    pub scan: ScanConfig,
    pub filters: FilterConfig,
    pub fees: FeeConfig,
    pub cache: CacheConfig,
    pub sso: SsoConfig,
    pub theme: AppTheme,
}

impl UserConfig {
    pub fn load() -> anyhow::Result<Self>;
    // Reads ~/.config/eve-traveling-merchant/config.toml (or legacy eve-trade-scout).
    // If missing, writes and returns Self::default().

    pub fn save(&self) -> anyhow::Result<()>;
    // Persists to config.toml.

    pub fn ship_profile(&self) -> anyhow::Result<ShipProfile>;
    // Delegates to profile_from_str(&self.ship.profile).
}

impl Default for UserConfig { /* sensible defaults: Big Hauler, Jita, SciFiVoid theme */ }
```

---

## `src/sde/loader.rs`

```rust
pub struct SdeLoader {
    conn: Mutex<rusqlite::Connection>,
    has_inv_volumes: bool,
    has_packaged_col: bool,
}

impl SdeLoader {
    /// Open the SDE database. Downloads if absent or older than max_age_days.
    pub async fn open(db_path: &Path, max_age_days: u32) -> anyhow::Result<Self>;

    /// Create in-memory SdeLoader with standard test fixtures for offline/CI tests.
    pub fn in_memory_with_fixtures() -> anyhow::Result<Self>;

    /// Decompress and validate the downloaded SDE database with safety guards.
    pub fn decompress_and_validate_archive<R: Read>(reader: R, dest_path: &Path) -> anyhow::Result<u64>;

    /// Download and decompress the Fuzzwork SQLite dump.
    /// Shows an indicatif progress bar during download.
    pub async fn download(db_path: &Path) -> anyhow::Result<()>;

    /// Look up item metadata from invTypes.
    /// Returns None if type_id not found or item is unpublished.
    pub fn get_item_info(&self, type_id: u32) -> Option<ItemInfo>;

    /// Autocomplete system search by prefix/infix query with Roman numeral extraction.
    pub fn search_systems(&self, query: &str, limit: usize) -> Vec<SystemSearchResult>;

    /// Case-insensitive system name or station shorthand → system_id.
    pub fn get_system_id(&self, name: &str) -> Option<u32>;

    /// system_id → system name.
    pub fn get_system_name(&self, system_id: u32) -> Option<String>;

    /// system_id → security status (-1.0 to 1.0).
    pub fn get_system_security(&self, system_id: u32) -> Option<f64>;

    /// system_id → region ID.
    pub fn get_system_region(&self, system_id: u32) -> Option<u32>;

    /// station_id → system_id (NPC stations only; returns None for structures).
    pub fn get_station_system(&self, station_id: u64) -> Option<u32>;

    /// station_id → canonical in-game formatted station name.
    pub fn get_station_name(&self, station_id: u64) -> Option<String>;

    /// region_id → region name.
    pub fn get_region_name(&self, region_id: u32) -> Option<String>;

    /// All solar systems in universe for graph building.
    pub fn all_systems(&self) -> anyhow::Result<Vec<SystemNode>>;

    /// All stargate jump links in universe for graph building.
    pub fn all_jumps(&self) -> anyhow::Result<Vec<(u32, u32)>>;
}
```

---

## `src/esi/client.rs`

```rust
pub struct EsiClient {
    client:          reqwest::Client,
    cache_dir:       PathBuf,
    etags:           Arc<Mutex<HashMap<String, StoredEtag>>>,
    concurrency_sem: Arc<Semaphore>,
}

impl EsiClient {
    pub fn new(cache_dir: impl AsRef<Path>) -> anyhow::Result<Self>;

    /// GET a single ESI JSON endpoint with ETag caching, decompression, and error bounding.
    pub async fn get_json<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        extra_query: &[(&str, String)],
        access_token: Option<&str>,
    ) -> anyhow::Result<T>;

    /// GET all pages of a paginated ESI endpoint.
    /// Reads X-Pages header from page 1, then fetches remaining pages
    /// concurrently (bounded JoinSet with semaphore limit).
    pub async fn get_all_pages<T: Serialize + DeserializeOwned + Send + 'static>(
        &self,
        endpoint: &str,
        extra_query: &[(&str, String)],
    ) -> anyhow::Result<Vec<T>>;

    /// Force-fetches all pages bypassing local ETag cache.
    pub async fn get_all_pages_fresh<T: Serialize + DeserializeOwned + Send + 'static>(
        &self,
        endpoint: &str,
        extra_query: &[(&str, String)],
    ) -> anyhow::Result<Vec<T>>;

    /// Flush the ETag cache to disk.
    pub async fn save_cache(&self) -> anyhow::Result<()>;
}
```

---

## `src/esi/market.rs`

```rust
/// All orders for a single region, split into sell and buy maps.
pub struct RegionMarket {
    pub region_id:   u32,
    pub fetched_at:  DateTime<Utc>,
    /// type_id → list of sell orders (is_buy_order == false)
    pub sell_orders: HashMap<u32, Vec<MarketOrder>>,
    /// type_id → list of buy orders (is_buy_order == true)
    pub buy_orders:  HashMap<u32, Vec<MarketOrder>>,
}

/// Fetch and parse a single region's complete order book.
pub async fn fetch_region_market_opts(
    client:        &EsiClient,
    region_id:     u32,
    force_refresh: bool,
) -> anyhow::Result<RegionMarket>;

/// Fetch multiple regions concurrently via tokio JoinSet.
/// Returns HashMap<region_id, RegionMarket>.
pub async fn fetch_markets(
    client:     &EsiClient,
    region_ids: &[u32],
) -> anyhow::Result<HashMap<u32, RegionMarket>>;

/// Force-fetch multiple regions fresh bypassing ETag cache.
pub async fn fetch_markets_fresh(
    client:     &EsiClient,
    region_ids: &[u32],
) -> anyhow::Result<HashMap<u32, RegionMarket>>;

/// From a list of orders, return the one with the lowest price.
pub fn cheapest_sell(orders: &[MarketOrder]) -> Option<&MarketOrder>;

/// From a list of orders, return the one with the highest price.
pub fn best_buy(orders: &[MarketOrder]) -> Option<&MarketOrder>;
```

---

## `src/graph.rs`

```rust
pub struct SystemGraph {
    graph:      petgraph::graph::UnGraph<SystemNode, f64>,
    id_to_idx:  HashMap<u32, NodeIndex>,  // system_id → graph NodeIndex
    jump_cache: Mutex<LruCache<(u32, u32, bool), u32>>,  // thread-safe LRU (capacity 20,000)
}

impl SystemGraph {
    /// In-memory graph builder from raw nodes and jump pairs (ideal for tests and SDE).
    pub fn from_nodes_and_jumps(systems: Vec<SystemNode>, jumps: Vec<(u32, u32)>) -> Self;

    /// Build from SdeLoader data.
    pub fn from_sde(sde: &SdeLoader) -> anyhow::Result<Self>;

    /// Security status of a system (-1.0 to 1.0).
    pub fn security(&self, system_id: u32) -> Option<f64>;

    /// System name lookup from graph nodes.
    pub fn system_name(&self, system_id: u32) -> Option<&str>;

    /// Find adjacent region IDs connected by stargate jumps.
    /// If safe_only is true, only highsec jumps are considered.
    pub fn adjacent_regions(&self, region_id: u32, safe_only: bool) -> Vec<u32>;

    /// True if security >= 0.45 (highsec).
    pub fn is_highsec(&self, system_id: u32) -> bool;

    /// Return the number of stargate jumps between two systems.
    /// safe_only: systems with security < 0.45 have edge weight = 1000
    ///            (effectively routes around lowsec without making path impossible).
    /// Returns 9999 as a sentinel if no path exists or if safe_only requires traversing lowsec.
    pub fn jump_count(&self, origin: u32, dest: u32, safe_only: bool) -> u32;

    /// Return the ordered list of system IDs on the shortest path using A*.
    /// Returns empty Vec as a sentinel if no path exists or exceeds safety cost.
    pub fn shortest_path(&self, origin: u32, dest: u32, safe_only: bool) -> Vec<u32>;
}
```

---

## `src/arbitrage.rs`

```rust
pub struct ArbitrageConfig {
    pub ship:               ShipProfile,
    pub home_system:        u32,
    pub min_profit:         f64,
    pub min_spread:         f64,
    pub min_net_profit_pct: f64,   // Enforced minimum net profit % after taxes (default: 10.0%)
    pub broker_fee:         f64,
    pub sales_tax:          f64,
    pub safe_only:          bool,
    pub npc_stations_only:  bool,
}

pub struct TradeCorridorManifest {
    pub source_system_id:       u32,
    pub source_system_name:     String,
    pub source_station_name:    String,
    pub source_security:        f64,
    pub dest_system_id:         u32,
    pub dest_system_name:       String,
    pub dest_station_name:      String,
    pub dest_security:          f64,
    pub deals:                  Vec<ArbitrageDeal>,
    pub total_volume_used:      f64,
    pub total_capital_required: f64,
    pub total_net_profit:       f64,
    pub overall_roi_pct:        f64,
    pub jumps:                  u32,
    pub profit_per_jump:        f64,
}

/// Identifies whether a location is an NPC station (ID < 1_000_000_000_000).
pub fn is_npc_station(location_id: u64) -> bool;

/// Main arbitrage engine. Pure computation, no I/O.
///
/// For each type_id with both sell and buy orders:
///   1. Get best sell price per station (lowest sell order)
///   2. Get best buy price per station (highest buy order)
///   3. For each (source_station, dest_station) pair where buy > sell:
///      a. Calculate quantity (bounded by volume, budget, supply, demand)
///      b. Calculate net profit after fees
///      c. Enforce minimum net profitability (ROI >= 10.0% after taxes)
///      d. Calculate jump counts via SystemGraph
///      e. Score as profit_per_jump
///   4. Filter by min_profit, min_spread, and min_net_profit_pct
///
/// Returns Vec<ArbitrageDeal> sorted by profit_per_jump DESC, then net_profit DESC.
pub fn find_deals(
    markets: &HashMap<u32, RegionMarket>,
    sde:     &SdeLoader,
    graph:   &SystemGraph,
    cfg:     &ArbitrageConfig,
) -> Vec<ArbitrageDeal>;

/// Calculate a single deal. Returns None if unprofitable, doesn't fit, or violates safety/station limits.
pub fn calculate_deal(
    type_id:       u32,
    item:          &ItemInfo,
    src_order:     &MarketOrder,
    dst_order:     &MarketOrder,
    cfg:           &ArbitrageConfig,
    sde:           &SdeLoader,
    graph:         &SystemGraph,
) -> Option<ArbitrageDeal>;

/// Bundle multiple compatible deals between the same station corridor into a packed cargo manifest.
pub fn bundle_route_cargo(
    deals: &[ArbitrageDeal],
    ship:  ShipProfile,
) -> Vec<TradeCorridorManifest>;
```

---

## `src/route_planner.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteStop {
    pub step:             usize,
    pub deal:             ArbitrageDeal,
    pub jumps_from_prev:  u32,
    pub prev_system_id:   u32,
    pub prev_system_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRoute {
    pub stops:                  Vec<RouteStop>,
    pub waypoints:              Vec<u32>,
    pub waypoint_names:         Vec<String>,
    pub total_jumps:            u32,
    pub total_profit:           f64,
    pub total_capital_required: f64,
    pub max_volume_used:        f64,
    pub avg_profit_per_jump:    f64,
}

impl TradeRoute {
    /// Returns the ordered sequence of station IDs for ESI in-game autopilot.
    pub fn ordered_destination_ids(&self) -> Vec<u64>;

    /// Formats a complete in-game flight sheet suitable for EVE Notepad (Alt+N).
    pub fn format_eve_flight_sheet(&self) -> String;
}

/// Build a greedy multi-stop trade route from a ranked deal list.
///
/// Deals have already been volume/budget bounded for the ship profile during arbitrage calculation.
/// The `_ship` profile is passed for interface uniformity.
pub fn plan_route(
    deals:       &[ArbitrageDeal],
    _ship:       ShipProfile,
    home_system: u32,
    graph:       &SystemGraph,
    safe_only:   bool,
    max_stops:   usize,
) -> TradeRoute;
```

---

## `src/chain_route_planner.rs`

```rust
pub struct CargoSlot {
    pub deal:        ArbitrageDeal,
    pub sell_system: u32,
    pub sell_name:   String,
    pub volume:      f64,
    pub capital:     f64,
    pub net_profit:  f64,
}

pub struct PickupStop {
    pub system_id:       u32,
    pub system_name:     String,
    pub jumps_from_prev: u32,
    pub detour_jumps:    u32,
    pub cargo_loaded:    Vec<CargoSlot>,
    pub volume_loaded:   f64,
    pub capital_spent:   f64,
}

pub struct SellStop {
    pub system_id:       u32,
    pub system_name:     String,
    pub jumps_from_prev: u32,
    pub cargo_sold:      Vec<CargoSlot>,
    pub total_profit:    f64,
    pub volume_freed:    f64,
}

pub struct ChainLeg {
    pub pickups:          Vec<PickupStop>,
    pub sell:             SellStop,
    pub leg_total_jumps:  u32,
    pub leg_total_profit: f64,
}

pub struct ChainRoute {
    pub legs:            Vec<ChainLeg>,
    pub total_jumps:     u32,
    pub total_profit:    f64,
    pub total_capital:   f64,
    pub peak_volume_m3:  f64,
    pub profit_per_jump: f64,
    pub overall_roi_pct: f64,
    pub waypoints:       Vec<u32>,
    pub waypoint_names:  Vec<String>,
}

impl ChainRoute {
    pub fn ordered_destination_ids(&self) -> Vec<u64>;
    pub fn format_eve_flight_sheet(&self, route_idx: usize) -> String;
}

pub struct ChainConfig {
    pub home_system:      u32,
    pub max_legs:         usize,
    pub beam_width:       usize,
    pub detour_enabled:   bool,
    pub max_detour_jumps: u32,
    pub max_dest_loads:   usize, // Max loads delivered to any destination station (default: 2)
    pub ship_max_volume:  f64,
    pub ship_max_budget:  f64,
    pub safe_only:        bool,
    pub include_npc:      bool,
}

/// Optimizes multi-leg chained trade routes with knapsack cargo packing and detour tolerance.
pub fn find_chain_routes(
    deals:  &[ArbitrageDeal],
    graph:  &SystemGraph,
    config: &ChainConfig,
) -> Vec<ChainRoute>;

pub fn knapsack_pack(
    deals:         &[&ArbitrageDeal],
    remaining_vol: f64,
    remaining_isk: f64,
    used_keys:     &HashSet<(u32, u64, u64)>,
) -> (Vec<CargoSlot>, f64, f64, f64);

pub fn knapsack_pack_constrained(
    deals:                   &[&ArbitrageDeal],
    remaining_vol:           f64,
    remaining_isk:           f64,
    used_keys:               &HashSet<(u32, u64, u64)>,
    prior_consumed_sources:  &HashMap<u64, u32>,
    prior_consumed_dests:    &HashMap<u64, u32>,
    prior_exhausted_sources: &HashSet<(u32, u64)>,
    prior_exhausted_dests:   &HashSet<(u32, u64)>,
) -> ConstrainedPackResult;

pub fn detour_cost(from: u32, waypoint: u32, to: u32, graph: &SystemGraph, safe: bool) -> u32;
```

---

## `src/ui/mod.rs`, `src/ui/gui.rs` & `src/ui/tui.rs`

```rust
// UI Entry Points
pub fn run_desktop_gui(app: DesktopApp) -> Result<(), eframe::Error>;

pub fn run_tui(
    deals:             &[ArbitrageDeal],
    route:             &TradeRoute,
    ship:              ShipProfile,
    home_system:       &str,
    safe_only:         bool,
    npc_stations_only: bool,
) -> anyhow::Result<()>;

pub fn format_unit_price(val: f64) -> String;
```

### Desktop GUI Features (`src/ui/gui.rs`)
- **Theme**: 4 selectable cockpit themes (Sci-Fi Void, Neutral Black, Black & Red, Solar Light) driven by `ThemePalette`.
- **Background Worker**: Multi-threaded non-blocking startup progress and market scanning using `mpsc::channel`.
- **Tabs**:
  - `Deals`: Live filterable arbitrage deals table with item search, sorting, and clipboard copying.
  - `Manifests`: Station-to-station cargo bundling manifests.
  - `Route`: Greedy multi-stop route planner with EVE flight sheet generation.
  - `ChainRoute`: Multi-leg knapsack chain planner.
  - `HubArbitrage`: Inter-hub trading matrix across Jita, Amarr, Dodixie, Rens, and Hek.
  - `Settings`: Live configuration editor, interactive Cockpit Theme Switcher, and EVE Online SSO v2 PKCE connection.

---

## `src/ui/theme.rs`

```rust
use eframe::egui::{Color32, Context, Frame, Stroke};
use crate::config::AppTheme;
use crate::hub_arbitrage::CoreHub;

pub struct ThemePalette {
    pub dark_mode: bool,
    pub bg_app: Color32,
    pub bg_panel: Color32,
    pub bg_panel_stroke: Stroke,
    pub bg_central: Color32,
    pub card_bg: Color32,
    pub card_stroke: Stroke,
    pub card_sub_bg: Color32,
    pub card_sub_stroke: Stroke,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub accent_primary: Color32,
    pub accent_primary_text: Color32,
    pub accent_secondary: Color32,
    pub accent_secondary_text: Color32,
    pub success_green: Color32,
    pub warning_yellow: Color32,
    pub error_red: Color32,
    pub info_blue: Color32,
    pub kpi_card_bg: Color32,
}

impl ThemePalette {
    pub fn preview_swatches(&self) -> (Color32, Color32, Color32);
    pub fn card_frame(&self) -> Frame;
    pub fn sub_card_frame(&self) -> Frame;
    pub fn top_panel_frame(&self) -> Frame;
    pub fn bottom_panel_frame(&self) -> Frame;
    pub fn central_panel_frame(&self) -> Frame;
    pub fn rank_silver(&self) -> Color32;
    pub fn hub_badge_color(&self, hub: Option<CoreHub>) -> Color32;
    pub fn primary_button<'a>(&self, text: impl Into<egui::WidgetText>) -> egui::Button<'a>;
    pub fn secondary_button<'a>(&self, text: impl Into<egui::WidgetText>) -> egui::Button<'a>;
    pub fn apply_to_ctx(&self, ctx: &Context);
}

pub fn theme_palette(theme: AppTheme) -> ThemePalette;
```

---

## `src/error.rs`

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    // SDE (1001..1008)
    ErrSdeNotFound,
    ErrSdeDownloadFailed,
    ErrSdeDecompressionFailed,
    ErrSdeDecompressionBomb,
    ErrSdeSchemaInvalid,
    ErrSdeQueryFailed,
    ErrSdeSystemNotFound,
    ErrSdeStationNotFound,

    // ESI (2001..2007)
    ErrEsiClientInit,
    ErrEsiNetworkFailed,
    ErrEsiRateLimited,
    ErrEsiServerError,
    ErrEsiClientError,
    ErrEsiParseFailed,
    ErrEsiEtagCache,

    // SSO (3001..3007)
    ErrSsoConfigInvalid,
    ErrSsoAuthServerFailed,
    ErrSsoTokenExchange,
    ErrSsoTokenRefresh,
    ErrSsoTokenSave,
    ErrSsoLocationFailed,
    ErrSsoAutopilotFailed,

    // Graph (4001..4002)
    ErrGraphBuildFailed,
    ErrGraphRouteUnreachable,

    // Arbitrage (5001..5002)
    ErrArbConfigInvalid,
    ErrArbZeroCapacity,

    // Config (6001..6002)
    ErrConfigLoadFailed,
    ErrConfigSaveFailed,

    // UI (7001..7004)
    ErrUiRuntimeInit,
    ErrUiWorkerChannel,
    ErrUiAudioPlayback,
    ErrUiCsvExport,

    // System (9001)
    ErrSysPanic,
}

impl ErrorCode {
    pub const fn as_str(&self) -> &'static str;
    pub const fn code_num(&self) -> u16;
    pub const fn description(&self) -> &'static str;
}
```

---

## `src/logging.rs`

```rust
use anyhow::Result;
use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;

pub fn init_logging(log_dir: Option<&Path>, default_level: &str) -> Result<WorkerGuard>;
// Initializes dual-sink tracing subscriber:
// 1. Stdout ANSI terminal layer with EnvFilter.
// 2. Daily rolling JSONL file appender in logs/app.log.YYYY-MM-DD.
// 3. Global panic hook capturing backtraces with ErrorCode::ErrSysPanic.
```

---

## `src/main.rs`

```rust
use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "eve-merchant",
    author = "EVE Traveling Merchant",
    version = "0.1.0",
    about = "EVE Traveling Merchant — High-profit cross-station arbitrage engine & multi-stop route planner"
)]
pub struct Args {
    #[arg(short, long)]                                               pub ship: Option<String>,
    #[arg(long)]                                                      pub system: Option<String>,
    #[arg(long)]                                                      pub regions: Option<String>,
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)] pub adjacent_regions: bool,
    #[arg(long)]                                                      pub safe: bool,
    #[arg(long)]                                                      pub unsafe_routes: bool,
    #[arg(long)]                                                      pub npc_only: bool,
    #[arg(long, aliases = ["allow-citadels", "include-structures"])]  pub allow_structures: bool,
    #[arg(long)]                                                      pub min_profit: Option<f64>,
    #[arg(long)]                                                      pub min_spread: Option<f64>,
    #[arg(long)]                                                      pub min_roi: Option<f64>,     // Default: 10.0% enforced
    #[arg(short, long, default_value_t = 25)]                         pub top: usize,
    #[arg(long, default_value_t = 5)]                                 pub max_stops: usize,
    #[arg(long)]                                                      pub route: bool,
    #[arg(long)]                                                      pub gui: bool,
    #[arg(long)]                                                      pub auto_refresh: bool,
    #[arg(long)]                                                      pub tui: bool,
    #[arg(long)]                                                      pub no_ui: bool,
    #[arg(long)]                                                      pub no_tui: bool,
    #[arg(long)]                                                      pub export: Option<PathBuf>,
    #[arg(long)]                                                      pub update_sde: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()>;
```

---

## Automated Test Suites (`tests/`)

The repository contains 13 dedicated integration test suites providing rigorous test coverage over critical domain logic, invariants, security, SSO, and diagnostics:

| Suite | File | Tests Covered |
|---|---|---|
| **Arbitrage Engine** | `tests/test_arbitrage.rs` | Ship profile volume/budget limits, profit-per-jump (PPJ) formula, enforced 10.0% net profitability rejection/acceptance, NPC station ID boundary ($< 10^{12}$ vs $\ge 10^{12}$), station toggle filter, highsec security filtering ($\ge 0.45$), 4-way quantity solver, deal sorting (PPJ DESC, net DESC). |
| **Chain Route Planner** | `tests/test_chain_route_planner.rs` | Multi-leg chained trade routes, knapsack cargo packing, detour tolerance, beam search candidate exploration, market load limits per destination station. |
| **Config & Profiles** | `tests/test_config.rs` | Ship profile string parsing (case-insensitivity, aliases, invalid string rejection), `UserConfig` defaults and invariant checks, TOML roundtrip serialization & deserialization. |
| **Donation Branding** | `tests/test_donation_branding.rs` | In-game ISK donation branding, developer character ID and name verification for tip jar. |
| **Error Codes Taxonomy** | `tests/test_error_codes.rs` | Taxonomy verification: uniqueness of integer code numbers, valid code ranges (1000..9999), string identifier formatting, JSON serialization/deserialization. |
| **ESI Models & Deserialization** | `tests/test_esi_models.rs` | JSON order array deserialization, buy vs sell discrimination, `cheapest_sell` and `best_buy` price matching across order books. |
| **Graph & Pathfinding** | `tests/test_graph.rs` | Solar system security classification (`is_highsec`), Dijkstra jump counting, safe routing bypass around lowsec systems, lowsec destination rejection in safe mode, adjacent region discovery. |
| **Hub Arbitrage** | `tests/test_hub_arbitrage.rs` | Multi-hub arbitrage matrix across 5 major trade hubs (Jita, Amarr, Dodixie, Rens, Hek), capital allocation, ROI filtering. |
| **Logging & Diagnostics** | `tests/test_logging.rs` | Initialization of dual-sink logging, file output generation, panic hook interceptor, JSON line schema validation, log retention. |
| **Route Planner** | `tests/test_route_planner.rs` | Empty deal handling, greedy sequential multi-stop deal chaining, cargo volume and budget accumulation, maximum stop constraint enforcement. |
| **SDE Search** | `tests/test_sde_search.rs` | Solar system substring and fuzzy search, station name resolution, item type lookup from SQLite. |
| **Security Audit** | `tests/test_security_audit.rs` | Untrusted input validation, path traversal prevention, CSV injection sanitization, numeric edge cases (NaN/Inf pricing). |
| **SSO Integration** | `tests/test_sso.rs` | EVE Online SSO v2 PKCE OAuth authorization flow, state verification, token exchange and refresh, authenticated client headers. |

