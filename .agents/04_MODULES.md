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
    pub ship:          ShipProfile,
    pub home_system:   u32,
    pub min_profit:    f64,
    pub min_spread:    f64,
    pub broker_fee:    f64,
    pub sales_tax:     f64,
    pub safe_only:     bool,
}

/// Main arbitrage engine. Pure computation, no I/O.
///
/// For each type_id with both sell and buy orders:
///   1. Get best sell price per station (lowest sell order)
///   2. Get best buy price per station (highest buy order)
///   3. For each (source_station, dest_station) pair where buy > sell:
///      a. Calculate quantity (bounded by volume, budget, supply, demand)
///      b. Calculate net profit after fees
///      c. Calculate jump counts via SystemGraph
///      d. Score as profit_per_jump
///   4. Filter by min_profit and min_spread
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

## `src/ui/tui.rs`

```rust
pub struct Tui {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl Tui {
    pub fn new() -> anyhow::Result<Self>;

    /// Restore terminal to normal mode on drop.
    pub fn restore() -> anyhow::Result<()>;
}

/// Top-level interactive UI entry point.
pub async fn run(
    config: &mut UserConfig,
    sde:    &SdeLoader,
    graph:  &SystemGraph,
    client: &EsiClient,
) -> anyhow::Result<()>;

/// Render the deal table (scrollable, colour-coded by profit tier).
pub fn render_deal_table(frame: &mut Frame, deals: &[ArbitrageDeal], scroll: usize);

/// Render the planned route panel.
pub fn render_route(frame: &mut Frame, route: &TradeRoute, sde: &SdeLoader);

/// Render the settings panel.
pub fn render_settings(frame: &mut Frame, config: &UserConfig);

/// Render a fetch progress overlay.
pub fn render_progress(frame: &mut Frame, message: &str, progress: f64);
```

---

## `src/main.rs`

```rust
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "eve-trade-scout", about = "EVE Online arbitrage route planner")]
struct Args {
    #[arg(long)] ship:        Option<String>,
    #[arg(long)] system:      Option<String>,
    #[arg(long)] regions:     Option<String>,   // comma-separated
    #[arg(long, default_value = "20")] top: usize,
    #[arg(long, default_value = "1000000.0")] min_profit: f64,
    #[arg(long, default_value = "5.0")]        min_spread: f64,
    #[arg(long)] safe:        bool,
    #[arg(long)] route:       bool,
    #[arg(long)] export:      Option<PathBuf>,
    #[arg(long)] no_cache:    bool,
    #[arg(long)] show_config: bool,
    #[arg(long)] update_sde:  bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()>;
```
