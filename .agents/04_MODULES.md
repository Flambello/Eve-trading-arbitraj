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

// ── User config ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct UserConfig { /* see 02_DATA_MODEL.md */ }

impl UserConfig {
    pub fn load() -> anyhow::Result<Self>;
    // Reads ~/.config/eve-trade-scout/config.toml.
    // If missing, writes and returns Self::default().

    pub fn save(&self) -> anyhow::Result<()>;
    // Persists to config.toml.

    pub fn ship_profile(&self) -> anyhow::Result<ShipProfile>;
    // Delegates to profile_from_str(&self.ship.profile).
}

impl Default for UserConfig { /* sensible defaults */ }
```

---

## `src/sde/loader.rs`

```rust
pub struct SdeLoader {
    conn: rusqlite::Connection,
}

impl SdeLoader {
    /// Open the SDE database. Downloads if absent or older than max_age_days.
    pub async fn open(db_path: &Path, max_age_days: u32) -> anyhow::Result<Self>;

    /// Download and decompress the Fuzzwork SQLite dump.
    /// Shows an indicatif progress bar during download.
    pub async fn download(db_path: &Path) -> anyhow::Result<()>;

    /// Look up item metadata from invTypes.
    /// Returns None if type_id not found or item is unpublished.
    pub fn get_item_info(&self, type_id: u32) -> Option<ItemInfo>;

    /// Case-insensitive system name → system_id.
    pub fn get_system_id(&self, name: &str) -> Option<u32>;

    /// system_id → system name.
    pub fn get_system_name(&self, system_id: u32) -> Option<String>;

    /// system_id → security status (-1.0 to 1.0).
    pub fn get_system_security(&self, system_id: u32) -> Option<f64>;

    /// station_id → system_id (NPC stations only; returns None for structures).
    pub fn get_station_system(&self, station_id: u64) -> Option<u32>;

    /// Build a petgraph::Graph from mapSolarSystemJumps.
    /// Nodes: SystemNode  |  Edges: () (unit, unweighted)
    /// Edge weights for Dijkstra are applied at query time.
    pub fn build_system_graph(&self) -> anyhow::Result<SystemGraph>;

    /// Return (region_id, region_name) for all regions.
    pub fn all_regions(&self) -> anyhow::Result<Vec<(u32, String)>>;
}
```

---

## `src/esi/client.rs`

```rust
pub struct EsiClient {
    inner:     reqwest::Client,
    cache_dir: PathBuf,
    etags:     HashMap<String, CacheEntry>,   // loaded from disk at startup
}

impl EsiClient {
    pub fn new(cache_dir: &Path) -> anyhow::Result<Self>;

    /// GET a single ESI path. Handles ETag caching, gzip, retries.
    /// Returns deserialized T.
    pub async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> anyhow::Result<T>;

    /// GET all pages of a paginated ESI endpoint.
    /// Reads X-Pages header from page 1, then fetches remaining pages
    /// concurrently (bounded JoinSet, max 10 parallel).
    pub async fn get_all_pages<T: DeserializeOwned + Send + 'static>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> anyhow::Result<Vec<T>>;

    /// Flush the ETag cache to disk.
    pub async fn save_cache(&self) -> anyhow::Result<()>;
}

// ESI-specific error types
#[derive(Debug, thiserror::Error)]
pub enum EsiError {
    #[error("rate limited (HTTP 420), retry after {retry_after}s")]
    RateLimited { retry_after: u64 },
    #[error("not found (HTTP 404): {path}")]
    NotFound { path: String },
    #[error("server error ({status}): {body}")]
    ServerError { status: u16, body: String },
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
pub async fn fetch_region_market(
    client:    &EsiClient,
    sde:       &SdeLoader,
    region_id: u32,
) -> anyhow::Result<RegionMarket>;

/// Fetch multiple regions concurrently via tokio JoinSet.
/// Returns HashMap<region_id, RegionMarket>.
pub async fn fetch_markets(
    client:     &EsiClient,
    sde:        &SdeLoader,
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
    graph:      petgraph::Graph<SystemNode, ()>,
    id_to_idx:  HashMap<u32, NodeIndex>,  // system_id → graph NodeIndex
    jump_cache: Mutex<LruCache<(u32, u32, bool), u32>>,  // thread-safe LRU
}

impl SystemGraph {
    /// In-memory graph builder from raw nodes and jump pairs (ideal for tests and SDE).
    pub fn from_nodes_and_jumps(systems: Vec<SystemNode>, jumps: Vec<(u32, u32)>) -> Self;

    /// Build from SdeLoader data.
    pub fn from_sde(sde: &SdeLoader) -> anyhow::Result<Self>;

    /// Return the number of stargate jumps between two systems.
    /// safe_only: systems with security < 0.45 have edge weight = 1000
    ///            (effectively routes around lowsec without making path impossible).
    /// Returns 9999 if no path exists.
    pub fn jump_count(&self, origin: u32, dest: u32, safe_only: bool) -> u32;

    /// Return the ordered list of system IDs on the shortest path.
    /// Returns empty Vec if no path.
    pub fn shortest_path(&self, origin: u32, dest: u32, safe_only: bool) -> Vec<u32>;

    /// Security status of a system (-1.0 to 1.0).
    pub fn security(&self, system_id: u32) -> Option<f64>;

    /// True if security >= 0.45 (highsec).
    pub fn is_highsec(&self, system_id: u32) -> bool;
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
    pub min_net_profit_pct: f64,   // Enforced minimum net profit % after taxes (default: 15.0%)
    pub broker_fee:         f64,
    pub sales_tax:          f64,
    pub safe_only:          bool,
    pub npc_stations_only:  bool,
}

/// Main arbitrage engine. Pure computation, no I/O.
///
/// For each type_id with both sell and buy orders:
///   1. Get best sell price per station (lowest sell order)
///   2. Get best buy price per station (highest buy order)
///   3. For each (source_station, dest_station) pair where buy > sell:
///      a. Calculate quantity (bounded by volume, budget, supply, demand)
///      b. Calculate net profit after fees
///      c. Enforce minimum net profitability (ROI >= 15.0% after taxes)
///      d. Calculate jump counts via SystemGraph
///      e. Score as profit_per_jump
///   4. Filter by min_profit, min_spread, and min_net_profit_pct
///
/// Returns Vec<ArbitrageDeal> sorted by profit_per_jump DESC.
pub fn find_deals(
    markets: &HashMap<u32, RegionMarket>,
    sde:     &SdeLoader,
    graph:   &SystemGraph,
    cfg:     &ArbitrageConfig,
) -> Vec<ArbitrageDeal>;

/// Calculate a single deal. Returns None if unprofitable or doesn't fit.
pub fn calculate_deal(
    type_id:       u32,
    item:          &ItemInfo,
    source_order:  &MarketOrder,
    dest_order:    &MarketOrder,
    cfg:           &ArbitrageConfig,
    graph:         &SystemGraph,
) -> Option<ArbitrageDeal>;

/// Fee calculation helper.
/// net = gross - broker_fee_on_buy - sales_tax_on_sell
pub fn net_profit(
    buy_price:  f64,
    sell_price: f64,
    quantity:   u32,
    broker_fee: f64,
    sales_tax:  f64,
) -> f64;
```

---

## `src/route_planner.rs`

```rust
/// Build a greedy multi-stop trade route from a ranked deal list.
///
/// Algorithm:
///   current_pos = home_system
///   remaining_vol = ship.max_volume
///   remaining_isk = ship.max_budget
///
///   loop (up to max_stops):
///     Re-score all unused deals from current_pos
///     Pick deal with highest profit_per_jump that fits vol + isk
///     If none: break
///     Append to route
///     Update remaining_vol, remaining_isk, current_pos
///
/// Returns the complete TradeRoute.
pub fn plan_route(
    deals:       &[ArbitrageDeal],
    ship:        ShipProfile,
    home_system: u32,
    graph:       &SystemGraph,
    max_stops:   usize,
) -> TradeRoute;

/// Re-score a deal's profit_per_jump relative to a new origin.
pub fn rescore(deal: &ArbitrageDeal, from_system: u32, graph: &SystemGraph) -> f64;

/// Build the ordered waypoint list from a sequence of deals.
pub fn build_waypoints(deals: &[ArbitrageDeal]) -> Vec<u32>;
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

pub struct ChainConfig {
    pub home_system:      u32,
    pub max_legs:         usize,
    pub beam_width:       usize,
    pub detour_enabled:   bool,
    pub max_detour_jumps: u32,
    pub ship_max_volume:  f64,
    pub ship_max_budget:  f64,
    pub safe_only:        bool,
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

pub fn detour_cost(from: u32, waypoint: u32, to: u32, graph: &SystemGraph, safe: bool) -> u32;
```

---

## `src/ui/mod.rs` & `src/ui/gui.rs`

```rust
// UI Mode Dispatcher
pub enum UiMode {
    Gui,     // Native eframe/egui desktop window (default when DISPLAY is present)
    Tui,     // Terminal ratatui interface
    NoUi,    // Direct stdout/headless CLI mode
}

pub fn select_ui_mode(args: &Args) -> UiMode;

// Native Desktop GUI
pub fn run_gui(
    config: UserConfig,
    sde: Arc<SdeLoader>,
    graph: Arc<SystemGraph>,
    client: Arc<EsiClient>,
    initial_deals: Vec<ArbitrageDeal>,
    initial_route: Option<TradeRoute>,
) -> anyhow::Result<()>;
```

### Key GUI Components (`src/ui/gui.rs`)
- **Theme**: Dark space sci-fi theme with custom palettes (dark slate panels, gold accents, neon cyan highlights, green profit tags).
- **Background Scan Worker**: Runs async market fetches and calculations on background threads, updating deal state via thread channels without freezing frame rendering.
- **Header Controls**: Ship profile toggle, starting system input + quick presets (Jita, Amarr, Dodixie, Rens), max stops slider, and "Scan Market" trigger.
- **Deals Table**: Real-time item search query, sortable columns, copy-to-clipboard buttons for buy/sell systems.
- **Visual Route Planner**: Waypoint sequence cards detailing action, item, quantity, investment, profit, and jumps.

---

## `src/ui/tui.rs`

```rust
pub struct Tui {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl Tui {
    pub fn new() -> anyhow::Result<Self>;
    pub fn restore() -> anyhow::Result<()>;
}

/// Top-level interactive TUI entry point.
pub async fn run(
    config: &mut UserConfig,
    sde:    &SdeLoader,
    graph:  &SystemGraph,
    client: &EsiClient,
) -> anyhow::Result<()>;

pub fn render_deal_table(frame: &mut Frame, deals: &[ArbitrageDeal], scroll: usize);
pub fn render_route(frame: &mut Frame, route: &TradeRoute, sde: &SdeLoader);
pub fn render_settings(frame: &mut Frame, config: &UserConfig);
pub fn render_progress(frame: &mut Frame, message: &str, progress: f64);
```

---

## `src/main.rs`

```rust
use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "eve-trade",
    version = "0.1.0",
    about = "Finds the closest and most profitable cross-station arbitrage deals and plans trade routes in EVE Online"
)]
pub struct Args {
    #[arg(short, long)]                      pub ship: Option<String>,
    #[arg(long)]                             pub system: Option<String>,
    #[arg(long)]                             pub regions: Option<String>,
    #[arg(long)]                             pub safe: bool,
    #[arg(long)]                             pub unsafe_routes: bool,
    #[arg(long)]                             pub npc_only: bool,
    #[arg(long, aliases = ["allow-citadels"])] pub allow_structures: bool,
    #[arg(long)]                             pub min_profit: Option<f64>,
    #[arg(long)]                             pub min_spread: Option<f64>,
    #[arg(long)]                             pub min_roi: Option<f64>,     // Default: 15.0% enforced
    #[arg(short, long, default_value_t = 25)] pub top: usize,
    #[arg(long, default_value_t = 5)]        pub max_stops: usize,
    #[arg(long)]                             pub route: bool,
    #[arg(long)]                             pub gui: bool,
    #[arg(long)]                             pub tui: bool,
    #[arg(long)]                             pub no_ui: bool,
    #[arg(long)]                             pub no_tui: bool,
    #[arg(long)]                             pub export: Option<PathBuf>,
    #[arg(long)]                             pub update_sde: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()>;
```

---

## Automated Test Suites (`tests/`)

The repository contains 5 dedicated integration test suites providing 100% test coverage over critical domain logic and invariants:

| Suite | File | Tests Covered |
|---|---|---|
| **Arbitrage Engine** | `tests/test_arbitrage.rs` | Ship profile volume/budget limits, profit-per-jump (PPJ) formula, enforced 15.0% net profitability rejection/acceptance, NPC station ID boundary ($< 10^{12}$ vs $\ge 10^{12}$), station toggle filter, highsec security filtering ($\ge 0.45$), 4-way quantity solver, deal sorting (PPJ DESC, net DESC). |
| **Config & Profiles** | `tests/test_config.rs` | Ship profile string parsing (case-insensitivity, aliases, invalid string rejection), `UserConfig` defaults and invariant checks, TOML roundtrip serialization & deserialization. |
| **Graph & Pathfinding** | `tests/test_graph.rs` | Solar system security classification (`is_highsec`), Dijkstra jump counting, safe routing bypass around lowsec systems, lowsec destination rejection in safe mode. |
| **Route Planner** | `tests/test_route_planner.rs` | Empty deal handling, greedy sequential multi-stop deal chaining, cargo volume and budget accumulation, maximum stop constraint enforcement. |
| **ESI Models & Deserialization** | `tests/test_esi_models.rs` | JSON order array deserialization, buy vs sell discrimination, `cheapest_sell` and `best_buy` price matching across order books. |
