use crate::arbitrage::{
    bundle_route_cargo, filter_deals_by_order_limit, find_deals, ArbitrageConfig, ArbitrageDeal,
    TradeCorridorManifest,
};
use crate::config::{BIG_HAULER, SMALL_HAULER};
use crate::esi::{count_market_orders, fetch_markets_fresh, EsiClient};
use crate::graph::SystemGraph;
use crate::hub_arbitrage::{self, CoreHub};
use crate::route_planner::{plan_route, RouteStop, TradeRoute};
use crate::sde::SdeLoader;
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use egui_extras::{Column, TableBuilder};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(PartialEq, Clone, Copy)]
enum ActiveTab {
    Deals,
    Manifests,
    Route,
    ChainRoute,
    HubArbitrage,
    Settings,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum DealSortBy {
    ProfitPerJump,
    TotalProfit,
    RoiPct,
    ProfitPerM3,
    Jumps,
    Cost,
    ItemName,
    BuySystem,
    BuyPrice,
    SellSystem,
    SellPrice,
    Quantity,
    Volume,
}

impl DealSortBy {
    fn display_name(&self) -> &'static str {
        match self {
            DealSortBy::ProfitPerJump => "Profit / Jump",
            DealSortBy::TotalProfit => "Total Net Profit",
            DealSortBy::RoiPct => "Net ROI %",
            DealSortBy::ProfitPerM3 => "Profit / m³",
            DealSortBy::Jumps => "Fewest Jumps",
            DealSortBy::Cost => "Lowest Cost",
            DealSortBy::ItemName => "Item Name",
            DealSortBy::BuySystem => "Buy System",
            DealSortBy::BuyPrice => "Buy Price / Item (Sell Order)",
            DealSortBy::SellSystem => "Sell System",
            DealSortBy::SellPrice => "Sell Price / Item (Buy Order)",
            DealSortBy::Quantity => "Quantity",
            DealSortBy::Volume => "Volume (m³)",
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            DealSortBy::ProfitPerJump => "profit_per_jump",
            DealSortBy::TotalProfit => "total_profit",
            DealSortBy::RoiPct => "roi_pct",
            DealSortBy::ProfitPerM3 => "profit_per_m3",
            DealSortBy::Jumps => "jumps",
            DealSortBy::Cost => "cost",
            DealSortBy::ItemName => "item_name",
            DealSortBy::BuySystem => "buy_system",
            DealSortBy::BuyPrice => "buy_price",
            DealSortBy::SellSystem => "sell_system",
            DealSortBy::SellPrice => "sell_price",
            DealSortBy::Quantity => "quantity",
            DealSortBy::Volume => "volume",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "total_profit" => DealSortBy::TotalProfit,
            "roi_pct" => DealSortBy::RoiPct,
            "profit_per_m3" => DealSortBy::ProfitPerM3,
            "jumps" => DealSortBy::Jumps,
            "cost" => DealSortBy::Cost,
            "item_name" => DealSortBy::ItemName,
            "buy_system" => DealSortBy::BuySystem,
            "buy_price" => DealSortBy::BuyPrice,
            "sell_system" => DealSortBy::SellSystem,
            "sell_price" => DealSortBy::SellPrice,
            "quantity" => DealSortBy::Quantity,
            "volume" => DealSortBy::Volume,
            _ => DealSortBy::ProfitPerJump,
        }
    }
}

pub enum ScanResult {
    Success {
        deals: Vec<ArbitrageDeal>,
        manifests: Vec<TradeCorridorManifest>,
        route: TradeRoute,
        total_sells: usize,
        total_buys: usize,
    },
    Error(String),
}

#[derive(Clone, Debug)]
pub struct StartupProgress {
    pub stage_index: usize,
    pub total_stages: usize,
    pub stage_name: String,
    pub detail: String,
    pub percent: f32,
    pub completed: bool,
    pub error: Option<String>,
    pub error_code: Option<crate::error::ErrorCode>,
}

impl Default for StartupProgress {
    fn default() -> Self {
        Self {
            stage_index: 1,
            total_stages: 5,
            stage_name: "Universe Initialization".to_string(),
            detail: "Booting New Eden trading intelligence core...".to_string(),
            percent: 0.05,
            completed: false,
            error: None,
            error_code: None,
        }
    }
}

pub enum StartupMsg {
    Progress(StartupProgress),
    Ready {
        sde: Arc<SdeLoader>,
        graph: Arc<SystemGraph>,
        esi: Arc<EsiClient>,
        home_system_name: String,
        scan_regions: Vec<u32>,
        deals: Vec<ArbitrageDeal>,
        manifests: Vec<TradeCorridorManifest>,
        route: Option<TradeRoute>,
    },
    Error {
        code: crate::error::ErrorCode,
        message: String,
    },
}

impl StartupMsg {
    /// Constructs a structured startup error message with an associated ErrorCode.
    pub fn error(code: crate::error::ErrorCode, message: impl Into<String>) -> Self {
        Self::Error {
            code,
            message: message.into(),
        }
    }
}

const TIPS: &[&str] = &[
    "Tip: Minimum 10.0% net ROI is strictly enforced across every arbitrage deal after taxes and broker fees.",
    "Tip: Big Hauler packs up to 60,000 m³ / 500M ISK, while Small Hauler provides an agile 30,000 m³ / 50M ISK profile.",
    "Tip: Safe highsec routing automatically bypasses dangerous solar systems with security status below 0.45.",
    "Tip: Multi-item knapsack manifests bundle multiple high-margin items along identical corridors into single hauler runs.",
    "Tip: Dead-end systems and remote highsec detours with ≥40% net ROI are badged with [HOT] for maximum profit yields.",
    "Tip: You can press F11 anytime to toggle fullscreen, or use Ctrl + / - to scale the interface dynamically.",
    "Tip: Deepest order book liquidity is found across the 5 trade hubs: Jita, Amarr, Dodixie, Rens, and Hek.",
];

/// Helper to ensure clipboard contents are copied both via context and output_mut across all Linux windowing backends
fn copy_to_clipboard(ui: &mut egui::Ui, text: impl Into<String>) {
    let s = text.into();
    ui.ctx().copy_text(s.clone());
    ui.output_mut(|o| o.copied_text = s);
}

/// Export a slice of deals to standard CSV format
fn export_deals_to_csv(deals: &[(usize, &ArbitrageDeal)]) -> String {
    let mut out = String::from("Item,Buy Station,Buy System,Buy Price,Sell Station,Sell System,Sell Price,Quantity,Volume_m3,Capital_Required,Net_Profit,Net_ROI_Pct,Total_Jumps,Profit_Per_Jump,Profit_Per_m3\n");
    for (_, d) in deals {
        out.push_str(&format!(
            "\"{}\",\"{}\",\"{}\",{:.2},\"{}\",\"{}\",{:.2},{},{:.2},{:.2},{:.2},{:.2},{},{:.2},{:.2}\n",
            crate::arbitrage::sanitize_csv_cell(&d.item_name),
            crate::arbitrage::sanitize_csv_cell(&d.source_station_name),
            crate::arbitrage::sanitize_csv_cell(&d.source_system_name),
            d.buy_price,
            crate::arbitrage::sanitize_csv_cell(&d.dest_station_name),
            crate::arbitrage::sanitize_csv_cell(&d.dest_system_name),
            d.sell_price,
            d.quantity,
            d.volume_used,
            d.capital_required,
            d.net_profit,
            d.net_profit_pct,
            d.total_jumps,
            d.profit_per_jump,
            d.profit_per_m3,
        ));
    }
    out
}

/// Export packed manifests and items to CSV format
fn export_manifests_to_csv(manifests: &[TradeCorridorManifest]) -> String {
    let mut out = String::from("Corridor,Source Station,Source System,Dest Station,Dest System,Total Volume m3,Total Capital Required,Total Net Profit,Item,Quantity,Buy Price,Sell Price,Item Net Profit,Item Net ROI\n");
    for (m_idx, m) in manifests.iter().enumerate() {
        for d in &m.deals {
            out.push_str(&format!(
                "{},\"{}\",\"{}\",\"{}\",\"{}\",{:.2},{:.2},{:.2},\"{}\",{},{:.2},{:.2},{:.2},{:.2}\n",
                m_idx + 1,
                crate::arbitrage::sanitize_csv_cell(&m.source_station_name),
                crate::arbitrage::sanitize_csv_cell(&m.source_system_name),
                crate::arbitrage::sanitize_csv_cell(&m.dest_station_name),
                crate::arbitrage::sanitize_csv_cell(&m.dest_system_name),
                m.total_volume_used,
                m.total_capital_required,
                m.total_net_profit,
                crate::arbitrage::sanitize_csv_cell(&d.item_name),
                d.quantity,
                d.buy_price,
                d.sell_price,
                d.net_profit,
                d.net_profit_pct,
            ));
        }
    }
    out
}

/// Resolves a safe user-writable path for CSV exports (e.g. ~/Downloads/filename or ./filename).
fn resolve_csv_export_path(filename: &str) -> std::path::PathBuf {
    if let Some(dl) = dirs::download_dir() {
        if dl.exists() {
            return dl.join(filename);
        }
    }
    if let Some(doc) = dirs::document_dir() {
        if doc.exists() {
            return doc.join(filename);
        }
    }
    std::path::PathBuf::from(filename)
}

/// Safely writes exported CSV content, rejecting symlinks and ensuring valid destination.
fn safe_write_csv(export_path: &std::path::Path, csv_str: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    if let Ok(meta) = std::fs::symlink_metadata(export_path) {
        if meta.file_type().is_symlink() {
            anyhow::bail!("Refusing to export through symlink: {:?}", export_path);
        }
    }
    if let Some(parent) = export_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create export directory: {:?}", parent))?;
        }
    }
    std::fs::write(export_path, csv_str)?;
    Ok(())
}

/// Target corporation for in-game ISK donations.
pub const CORP_DONATION_RECIPIENT: &str = "Expandabliss";

/// Format the official in-game donation instruction note for EVE Trade Scout players.
pub fn format_donation_instructions(amount_millions: Option<u64>) -> String {
    match amount_millions {
        Some(m) => format!(
            "Send {}M ISK donation to corporation: {}",
            m, CORP_DONATION_RECIPIENT
        ),
        None => format!(
            "Send ISK donation to corporation: {}",
            CORP_DONATION_RECIPIENT
        ),
    }
}

/// Embedded application window icon PNG bytes (256x256 RGBA).
pub const APP_ICON_PNG_BYTES: &[u8] =
    include_bytes!("../../assets/icons/io.github.flambello.eve_merchant.png");

/// Embedded Expandabliss corporation crest PNG bytes (124x124 RGBA).
pub const CORP_ICON_PNG_BYTES: &[u8] = include_bytes!("../../assets/icons/expandabliss.png");

fn core_hub_from_str(s: &str) -> Option<crate::hub_arbitrage::CoreHub> {
    match s.trim().to_lowercase().as_str() {
        "jita" => Some(crate::hub_arbitrage::CoreHub::Jita),
        "amarr" => Some(crate::hub_arbitrage::CoreHub::Amarr),
        "dodixie" => Some(crate::hub_arbitrage::CoreHub::Dodixie),
        "rens" => Some(crate::hub_arbitrage::CoreHub::Rens),
        "hek" => Some(crate::hub_arbitrage::CoreHub::Hek),
        _ => None,
    }
}

pub struct DesktopApp {
    sde: Option<Arc<SdeLoader>>,
    graph: Option<Arc<SystemGraph>>,
    esi: Option<Arc<EsiClient>>,

    // Controls
    ship_is_big: bool,
    home_system: String,
    include_adjacent_regions: bool,
    safe_only: bool,
    npc_stations_only: bool,
    min_roi_pct: f64,
    min_profit_isk: f64,
    broker_fee_pct: f64,
    sales_tax_pct: f64,
    max_route_stops: usize,

    // Vessel & Anti-Gank Protection Sizing
    pub custom_cargo_vol: f64,
    pub custom_budget_isk: f64,
    pub use_custom_ship: bool,

    // Auto-Refresh & Audio Alerts (5m cadence matching CCP ESI order book cache)
    pub auto_refresh: bool,
    auto_refresh_secs: u64,
    last_scan_completed: Option<Instant>,
    pub sound_alerts_enabled: bool,
    pub sound_min_roi_pct: f64,
    alerted_deal_keys: std::collections::HashSet<(u32, u64, u64)>,

    // Regions selection
    region_the_forge: bool,
    region_domain: bool,
    region_sinq_laison: bool,
    region_heimatar: bool,
    region_metropolis: bool,

    // Search and Sort in deals tab
    filter_item_name: String,
    sort_by: DealSortBy,
    sort_descending: bool,
    min_profit_filter_m: f64,
    max_jumps_filter: u32,
    pub hide_npc_trades: bool,
    pub max_deals_per_order: usize,

    // Cached deals tab filter/sort output (indices into self.deals)
    cached_deals_filtered: Vec<usize>,
    cached_deals_key: (String, u64, u32, bool, usize, DealSortBy, bool, usize),

    // Cached hub arbitrage tab filter/sort output
    cached_hub_deals: Vec<crate::arbitrage::ArbitrageDeal>,
    cached_hub_key: (
        Option<crate::hub_arbitrage::CoreHub>,
        Option<crate::hub_arbitrage::CoreHub>,
        bool,
        bool,
        u64,
        String,
        DealSortBy,
        bool,
        usize,
    ),

    // Active state
    tab: ActiveTab,
    is_scanning: bool,
    status_text: String,
    deals: Vec<ArbitrageDeal>,
    manifests: Vec<TradeCorridorManifest>,
    route: Option<TradeRoute>,
    selected_deal_idx: Option<usize>,
    toast: Option<(String, Instant)>,

    // Async worker channel
    scan_rx: Option<Receiver<ScanResult>>,

    // ── Chain Route Planner tab ──────────────────────────────────
    chain_max_legs: usize,
    chain_max_dest_loads: usize,
    chain_include_npc: bool,
    chain_detour_enabled: bool,
    chain_max_detour: u32,
    chain_is_running: bool,
    chain_results: Vec<crate::chain_route_planner::ChainRoute>,
    chain_selected_idx: Option<usize>,
    chain_rx: Option<Receiver<Vec<crate::chain_route_planner::ChainRoute>>>,
    chain_flowchart_split: bool,

    // ── Core Hub Arbitrage tab ────────────────────────────────────
    hub_source_filter: Option<crate::hub_arbitrage::CoreHub>,
    hub_dest_filter: Option<crate::hub_arbitrage::CoreHub>,
    hub_include_npc: bool,
    hub_major_stations_only: bool,
    hub_min_profit_m: f64,
    hub_search_query: String,
    hub_sort_by: DealSortBy,
    hub_sort_descending: bool,
    hub_packed_manifest: Option<crate::chain_route_planner::ConstrainedPackResult>,
    hub_show_manifest_card: bool,

    // ── Responsive Layout & UI Scaling ───────────────────────────
    pub ui_scale: f32,
    pub is_fullscreen: bool,
    pub dossier_side_panel: bool,
    pub compact_kpi: bool,
    pub theme: crate::config::AppTheme,
    pub has_applied_initial_viewport: bool,

    // ── Startup & Animated Splash Screen ─────────────────────────
    pub is_startup: bool,
    pub startup_progress: StartupProgress,
    pub startup_rx: Option<Receiver<StartupMsg>>,
    pub startup_tip_index: usize,
    pub startup_last_tip: Instant,

    // ── EVE SSO & In-Game Autopilot Sync ──────────────────────────
    pub sso_client_id: String,
    pub sso_character_name: Option<String>,
    pub sso_character_id: Option<u64>,
    pub sso_is_authenticating: bool,
    pub sso_is_pushing: bool,
    pub sso_auto_sync_location: bool,
    pub sso_is_fetching_location: bool,
    sso_location_scan_after: bool,
    sso_auth_rx: Option<Receiver<Result<crate::esi::sso::SsoToken, String>>>,
    sso_push_rx: Option<Receiver<Result<usize, String>>>,
    sso_location_rx: Option<Receiver<Result<crate::esi::sso::CharacterLocation, String>>>,

    // ── EVE SSO Token Auto-Refresh ────────────────────────────────
    pub sso_token_expires_at: Option<i64>,
    pub sso_last_token_refresh_check: Instant,
    pub sso_is_refreshing_token: bool,
    sso_refresh_rx: Option<Receiver<(Result<crate::esi::sso::SsoToken, String>, bool)>>,

    // ── In-Game ISK Donations & Corp Crest ────────────────────────
    corp_icon_texture: Option<egui::TextureHandle>,
}

impl DesktopApp {
    pub fn new_with_config(
        sde: Arc<SdeLoader>,
        graph: Arc<SystemGraph>,
        esi: Arc<EsiClient>,
        cfg: &crate::config::UserConfig,
        initial_deals: Vec<ArbitrageDeal>,
        initial_route: Option<TradeRoute>,
    ) -> Self {
        let initial_status = if initial_deals.is_empty() {
            "Ready. Configure ship & location, then click SCAN MARKET.".to_string()
        } else {
            format!(
                "Loaded {} profitable arbitrage deals (ROI >= 10.0%).",
                initial_deals.len()
            )
        };

        let initial_ship = if cfg.ship.use_custom_ship {
            crate::config::ShipProfile {
                name: "Custom / Anti-Gank Profile",
                max_volume: cfg.ship.custom_cargo_vol.max(10.0),
                max_budget: cfg.ship.custom_budget_isk.max(100_000.0),
            }
        } else if cfg.ship.profile.to_lowercase() == "small" {
            SMALL_HAULER
        } else {
            BIG_HAULER
        };
        let initial_manifests = bundle_route_cargo(&initial_deals, initial_ship);

        let mut initial_alerted_keys = std::collections::HashSet::new();
        for d in &initial_deals {
            if d.net_profit_pct >= cfg.scan.sound_min_roi_pct.max(10.0) {
                initial_alerted_keys.insert((d.type_id, d.source_station_id, d.dest_station_id));
            }
        }

        let (_startup_tx, startup_rx) = channel();
        let mut app = Self::new_loading(cfg, startup_rx);
        app.sde = Some(sde);
        app.graph = Some(graph);
        app.esi = Some(esi);
        app.status_text = initial_status;
        app.deals = initial_deals;
        app.manifests = initial_manifests;
        app.route = initial_route;
        app.alerted_deal_keys = initial_alerted_keys;
        app.is_startup = false;
        app.startup_progress = StartupProgress {
            stage_index: 5,
            total_stages: 5,
            stage_name: "Ready".to_string(),
            detail: "Startup complete".to_string(),
            percent: 1.0,
            completed: true,
            error: None,
            error_code: None,
        };
        app.startup_rx = None;

        if app.sso_character_name.is_some() && app.sso_auto_sync_location {
            app.fetch_live_character_location(true);
        }

        app
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sde: Arc<SdeLoader>,
        graph: Arc<SystemGraph>,
        esi: Arc<EsiClient>,
        initial_ship_is_big: bool,
        initial_home_system: String,
        initial_safe_only: bool,
        initial_npc_stations_only: bool,
        initial_min_roi: f64,
        initial_min_profit: f64,
        initial_regions: Vec<u32>,
        initial_deals: Vec<ArbitrageDeal>,
        initial_route: Option<TradeRoute>,
        initial_auto_refresh: bool,
        initial_sound_alerts: bool,
        initial_sound_min_roi: f64,
    ) -> Self {
        let mut cfg = crate::config::UserConfig::load().unwrap_or_default();
        cfg.ship.profile = if initial_ship_is_big {
            "big".to_string()
        } else {
            "small".to_string()
        };
        cfg.location.home_system = initial_home_system;
        cfg.scan.safe_route_only = initial_safe_only;
        cfg.scan.npc_stations_only = initial_npc_stations_only;
        cfg.filters.min_net_profit_pct = initial_min_roi.max(10.0);
        cfg.filters.min_profit_isk = initial_min_profit;
        cfg.scan.regions = initial_regions;
        cfg.scan.auto_refresh = initial_auto_refresh;
        cfg.scan.sound_alerts = initial_sound_alerts;
        cfg.scan.sound_min_roi_pct = initial_sound_min_roi;
        Self::new_with_config(sde, graph, esi, &cfg, initial_deals, initial_route)
    }

    pub fn new_loading(cfg: &crate::config::UserConfig, startup_rx: Receiver<StartupMsg>) -> Self {
        let saved_tok = crate::esi::sso::load_saved_token();
        let mut app = Self {
            sde: None,
            graph: None,
            esi: None,
            ship_is_big: cfg.ship.profile.to_lowercase() != "small",
            home_system: cfg.location.home_system.clone(),
            include_adjacent_regions: cfg.scan.scan_adjacent_regions,
            safe_only: cfg.scan.safe_route_only,
            npc_stations_only: cfg.scan.npc_stations_only,
            min_roi_pct: cfg.filters.min_net_profit_pct.max(10.0),
            min_profit_isk: if cfg.filters.min_profit_isk <= 0.0 {
                1_000_000.0
            } else {
                cfg.filters.min_profit_isk
            },
            broker_fee_pct: cfg.fees.broker_fee_pct.clamp(1.0, 5.0),
            sales_tax_pct: cfg.fees.sales_tax_pct.clamp(3.0, 8.0),
            max_route_stops: if cfg.ui.max_route_stops == 0 {
                5
            } else {
                cfg.ui.max_route_stops
            },

            auto_refresh: cfg.scan.auto_refresh,
            auto_refresh_secs: 300,
            last_scan_completed: Some(Instant::now()),
            sound_alerts_enabled: cfg.scan.sound_alerts,
            sound_min_roi_pct: if cfg.scan.sound_min_roi_pct <= 0.0 {
                100.0
            } else {
                cfg.scan.sound_min_roi_pct
            },
            alerted_deal_keys: std::collections::HashSet::new(),

            region_the_forge: cfg.scan.regions.contains(&10000002),
            region_domain: cfg.scan.regions.contains(&10000043),
            region_sinq_laison: cfg.scan.regions.contains(&10000032),
            region_heimatar: cfg.scan.regions.contains(&10000030),
            region_metropolis: cfg.scan.regions.contains(&10000042),

            filter_item_name: String::new(),
            sort_by: DealSortBy::from_str(&cfg.view.deal_sort_by),
            sort_descending: cfg.view.deal_sort_descending,
            min_profit_filter_m: cfg.view.min_profit_filter_m,
            max_jumps_filter: cfg.view.max_jumps_filter,
            hide_npc_trades: cfg.view.hide_npc_trades,
            max_deals_per_order: if cfg.view.max_deals_per_order == 0 {
                2
            } else {
                cfg.view.max_deals_per_order
            },

            cached_deals_filtered: Vec::new(),
            cached_deals_key: (
                String::new(),
                0,
                0,
                false,
                0,
                DealSortBy::ProfitPerJump,
                false,
                usize::MAX,
            ),
            cached_hub_deals: Vec::new(),
            cached_hub_key: (
                None,
                None,
                false,
                false,
                0,
                String::new(),
                DealSortBy::ProfitPerJump,
                false,
                usize::MAX,
            ),

            tab: ActiveTab::Deals,
            is_scanning: false,
            status_text: "Initializing neural core...".to_string(),
            deals: Vec::new(),
            manifests: Vec::new(),
            route: None,
            selected_deal_idx: None,
            toast: None,
            scan_rx: None,

            chain_max_legs: if cfg.view.chain_max_legs == 0 {
                4
            } else {
                cfg.view.chain_max_legs
            },
            chain_max_dest_loads: if cfg.view.chain_max_dest_loads == 0 {
                2
            } else {
                cfg.view.chain_max_dest_loads
            },
            chain_include_npc: cfg.view.chain_include_npc,
            chain_detour_enabled: cfg.view.chain_detour_enabled,
            chain_max_detour: cfg.view.chain_max_detour,
            chain_is_running: false,
            chain_results: Vec::new(),
            chain_selected_idx: None,
            chain_rx: None,
            chain_flowchart_split: true,

            hub_source_filter: cfg
                .view
                .hub_source_hub
                .as_deref()
                .and_then(core_hub_from_str),
            hub_dest_filter: cfg.view.hub_dest_hub.as_deref().and_then(core_hub_from_str),
            hub_include_npc: cfg.view.hub_include_npc,
            hub_major_stations_only: cfg.view.hub_major_stations_only,
            hub_min_profit_m: cfg.view.hub_min_profit_m,
            hub_search_query: String::new(),
            hub_sort_by: DealSortBy::ProfitPerJump,
            hub_sort_descending: true,
            hub_packed_manifest: None,
            hub_show_manifest_card: false,

            custom_cargo_vol: cfg.ship.custom_cargo_vol.max(10.0),
            custom_budget_isk: cfg.ship.custom_budget_isk.max(100_000.0),
            use_custom_ship: cfg.ship.use_custom_ship,

            ui_scale: cfg.ui.ui_scale.clamp(0.75, 1.50),
            is_fullscreen: cfg.ui.is_fullscreen,
            dossier_side_panel: cfg.ui.dossier_side_panel,
            compact_kpi: cfg.ui.compact_kpi,
            theme: cfg.theme,
            has_applied_initial_viewport: false,

            is_startup: true,
            startup_progress: StartupProgress::default(),
            startup_rx: Some(startup_rx),
            startup_tip_index: 0,
            startup_last_tip: Instant::now(),

            sso_client_id: cfg.sso.client_id.clone(),
            sso_character_name: saved_tok.as_ref().map(|t| t.character_name.clone()),
            sso_character_id: saved_tok.as_ref().map(|t| t.character_id),
            sso_token_expires_at: saved_tok.as_ref().map(|t| t.expires_at),
            sso_is_authenticating: false,
            sso_is_pushing: false,
            sso_auto_sync_location: cfg.sso.auto_sync_location,
            sso_is_fetching_location: false,
            sso_location_scan_after: false,
            sso_auth_rx: None,
            sso_push_rx: None,
            sso_location_rx: None,
            sso_last_token_refresh_check: Instant::now() - Duration::from_secs(3600),
            sso_is_refreshing_token: false,
            sso_refresh_rx: None,
            corp_icon_texture: None,
        };

        if app.sso_character_name.is_some() {
            app.check_and_refresh_sso_token(false);
        }

        app
    }

    pub fn apply_user_config(&mut self, cfg: &crate::config::UserConfig) {
        self.ship_is_big = cfg.ship.profile.to_lowercase() != "small";
        self.use_custom_ship = cfg.ship.use_custom_ship;
        self.custom_cargo_vol = cfg.ship.custom_cargo_vol.max(10.0);
        self.custom_budget_isk = cfg.ship.custom_budget_isk.max(100_000.0);

        self.home_system = cfg.location.home_system.clone();

        self.region_the_forge = cfg.scan.regions.contains(&10000002);
        self.region_domain = cfg.scan.regions.contains(&10000043);
        self.region_sinq_laison = cfg.scan.regions.contains(&10000032);
        self.region_heimatar = cfg.scan.regions.contains(&10000030);
        self.region_metropolis = cfg.scan.regions.contains(&10000042);
        self.safe_only = cfg.scan.safe_route_only;
        self.npc_stations_only = cfg.scan.npc_stations_only;
        self.include_adjacent_regions = cfg.scan.scan_adjacent_regions;
        self.auto_refresh = cfg.scan.auto_refresh;
        self.sound_alerts_enabled = cfg.scan.sound_alerts;
        self.sound_min_roi_pct = if cfg.scan.sound_min_roi_pct <= 0.0 {
            100.0
        } else {
            cfg.scan.sound_min_roi_pct
        };

        self.min_profit_isk = if cfg.filters.min_profit_isk <= 0.0 {
            1_000_000.0
        } else {
            cfg.filters.min_profit_isk
        };
        self.min_roi_pct = cfg.filters.min_net_profit_pct.max(10.0);

        self.broker_fee_pct = cfg.fees.broker_fee_pct.clamp(1.0, 5.0);
        self.sales_tax_pct = cfg.fees.sales_tax_pct.clamp(3.0, 8.0);

        if !cfg.sso.client_id.is_empty() {
            self.sso_client_id = cfg.sso.client_id.clone();
        }
        self.sso_auto_sync_location = cfg.sso.auto_sync_location;

        self.theme = cfg.theme;

        self.ui_scale = cfg.ui.ui_scale.clamp(0.75, 1.50);
        self.is_fullscreen = cfg.ui.is_fullscreen;
        self.dossier_side_panel = cfg.ui.dossier_side_panel;
        self.compact_kpi = cfg.ui.compact_kpi;
        self.max_route_stops = if cfg.ui.max_route_stops == 0 {
            5
        } else {
            cfg.ui.max_route_stops
        };

        self.sort_by = DealSortBy::from_str(&cfg.view.deal_sort_by);
        self.sort_descending = cfg.view.deal_sort_descending;
        self.min_profit_filter_m = cfg.view.min_profit_filter_m;
        self.max_jumps_filter = cfg.view.max_jumps_filter;
        self.hide_npc_trades = cfg.view.hide_npc_trades;
        self.max_deals_per_order = if cfg.view.max_deals_per_order == 0 {
            2
        } else {
            cfg.view.max_deals_per_order
        };

        self.chain_max_legs = if cfg.view.chain_max_legs == 0 {
            4
        } else {
            cfg.view.chain_max_legs
        };
        self.chain_max_dest_loads = if cfg.view.chain_max_dest_loads == 0 {
            2
        } else {
            cfg.view.chain_max_dest_loads
        };
        self.chain_include_npc = cfg.view.chain_include_npc;
        self.chain_detour_enabled = cfg.view.chain_detour_enabled;
        self.chain_max_detour = cfg.view.chain_max_detour;

        self.hub_source_filter = cfg
            .view
            .hub_source_hub
            .as_deref()
            .and_then(core_hub_from_str);
        self.hub_dest_filter = cfg.view.hub_dest_hub.as_deref().and_then(core_hub_from_str);
        self.hub_include_npc = cfg.view.hub_include_npc;
        self.hub_major_stations_only = cfg.view.hub_major_stations_only;
        self.hub_min_profit_m = cfg.view.hub_min_profit_m;
    }

    pub fn export_user_config(&self) -> crate::config::UserConfig {
        let mut cfg = crate::config::UserConfig::load().unwrap_or_default();

        cfg.ship.profile = if self.ship_is_big {
            "big".to_string()
        } else {
            "small".to_string()
        };
        cfg.ship.use_custom_ship = self.use_custom_ship;
        cfg.ship.custom_cargo_vol = self.custom_cargo_vol.max(10.0);
        cfg.ship.custom_budget_isk = self.custom_budget_isk.max(100_000.0);

        cfg.location.home_system = self.home_system.clone();

        let mut regions = Vec::new();
        if self.region_the_forge {
            regions.push(10000002);
        }
        if self.region_domain {
            regions.push(10000043);
        }
        if self.region_sinq_laison {
            regions.push(10000032);
        }
        if self.region_heimatar {
            regions.push(10000030);
        }
        if self.region_metropolis {
            regions.push(10000042);
        }
        cfg.scan.regions = regions;
        cfg.scan.safe_route_only = self.safe_only;
        cfg.scan.npc_stations_only = self.npc_stations_only;
        cfg.scan.scan_adjacent_regions = self.include_adjacent_regions;
        cfg.scan.auto_refresh = self.auto_refresh;
        cfg.scan.sound_alerts = self.sound_alerts_enabled;
        cfg.scan.sound_min_roi_pct = self.sound_min_roi_pct.max(10.0);

        cfg.filters.min_profit_isk = self.min_profit_isk;
        cfg.filters.min_net_profit_pct = self.min_roi_pct.max(10.0);

        cfg.fees.broker_fee_pct = self.broker_fee_pct.clamp(1.0, 5.0);
        cfg.fees.sales_tax_pct = self.sales_tax_pct.clamp(3.0, 8.0);

        if !self.sso_client_id.is_empty() {
            cfg.sso.client_id = self.sso_client_id.clone();
        }
        cfg.sso.auto_sync_location = self.sso_auto_sync_location;

        cfg.theme = self.theme;

        cfg.ui.ui_scale = self.ui_scale.clamp(0.75, 1.50);
        cfg.ui.is_fullscreen = self.is_fullscreen;
        cfg.ui.dossier_side_panel = self.dossier_side_panel;
        cfg.ui.compact_kpi = self.compact_kpi;
        cfg.ui.max_route_stops = self.max_route_stops;

        cfg.view.deal_sort_by = self.sort_by.as_str().to_string();
        cfg.view.deal_sort_descending = self.sort_descending;
        cfg.view.min_profit_filter_m = self.min_profit_filter_m;
        cfg.view.max_jumps_filter = self.max_jumps_filter;
        cfg.view.hide_npc_trades = self.hide_npc_trades;
        cfg.view.max_deals_per_order = self.max_deals_per_order;

        cfg.view.chain_max_legs = self.chain_max_legs;
        cfg.view.chain_max_dest_loads = self.chain_max_dest_loads;
        cfg.view.chain_include_npc = self.chain_include_npc;
        cfg.view.chain_detour_enabled = self.chain_detour_enabled;
        cfg.view.chain_max_detour = self.chain_max_detour;

        cfg.view.hub_source_hub = self.hub_source_filter.map(|h| h.name().to_string());
        cfg.view.hub_dest_hub = self.hub_dest_filter.map(|h| h.name().to_string());
        cfg.view.hub_include_npc = self.hub_include_npc;
        cfg.view.hub_major_stations_only = self.hub_major_stations_only;
        cfg.view.hub_min_profit_m = self.hub_min_profit_m;

        cfg
    }

    pub fn save_user_config(&self) -> anyhow::Result<()> {
        let cfg = self.export_user_config();
        match cfg.save() {
            Ok(()) => {
                tracing::info!(target: "eve_trade_scout", "User configuration successfully saved to disk.");
                Ok(())
            }
            Err(e) => {
                tracing::error!(
                    target: "eve_trade_scout",
                    error_code = %crate::error::ErrorCode::ErrConfigSaveFailed,
                    error = %e,
                    "Failed to save user configuration to disk: {e}"
                );
                Err(e)
            }
        }
    }

    pub fn current_ship(&self) -> crate::config::ShipProfile {
        if self.use_custom_ship {
            crate::config::ShipProfile {
                name: "Custom / Anti-Gank Profile",
                max_volume: self.custom_cargo_vol.max(10.0),
                max_budget: self.custom_budget_isk.max(100_000.0),
            }
        } else if self.ship_is_big {
            BIG_HAULER
        } else {
            SMALL_HAULER
        }
    }

    fn set_toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    /// Lazily decodes and loads the embedded Expandabliss corporation crest PNG into an egui texture.
    fn ensure_corp_icon(&mut self, ctx: &egui::Context) -> Option<&egui::TextureHandle> {
        if self.corp_icon_texture.is_none() {
            if let Ok(img) = image::load_from_memory(CORP_ICON_PNG_BYTES) {
                let rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                    [w as usize, h as usize],
                    rgba.as_raw(),
                );
                self.corp_icon_texture = Some(ctx.load_texture(
                    "expandabliss_corp_icon",
                    color_image,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
        self.corp_icon_texture.as_ref()
    }

    /// Checks if the saved EVE SSO token needs proactive refresh and initiates background renewal.
    pub fn check_and_refresh_sso_token(&mut self, force: bool) {
        if self.sso_is_refreshing_token {
            return;
        }
        let token = match crate::esi::sso::load_saved_token() {
            Some(t) => t,
            None => return,
        };

        if !force && !token.needs_refresh() {
            return;
        }

        let client_id = self.sso_client_id.clone();
        self.sso_is_refreshing_token = true;
        if force {
            self.set_toast("Refreshing EVE SSO access token...");
        }

        let (tx, rx) = channel();
        self.sso_refresh_rx = Some(rx);

        thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send((Err(format!("Runtime build error: {}", e)), force));
                    return;
                }
            };

            let http_client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());
            let res = rt.block_on(crate::esi::sso::refresh_saved_token_opts(
                &http_client,
                &client_id,
                force,
            ));
            match res {
                Ok(Some(token)) => {
                    let _ = tx.send((Ok(token), force));
                }
                Ok(None) => {
                    let _ = tx.send((Err("No token found on disk to refresh".to_string()), force));
                }
                Err(e) => {
                    let _ = tx.send((Err(e.to_string()), force));
                }
            }
        });
    }

    /// Polls the SSO token refresh worker channel and updates token status.
    fn poll_sso_refresh(&mut self, ctx: &egui::Context) {
        if let Some(ref rx) = self.sso_refresh_rx {
            match rx.try_recv() {
                Ok((res, force)) => {
                    self.sso_is_refreshing_token = false;
                    match res {
                        Ok(token) => {
                            self.sso_character_name = Some(token.character_name);
                            self.sso_character_id = Some(token.character_id);
                            self.sso_token_expires_at = Some(token.expires_at);
                            if force {
                                self.set_toast("EVE SSO access token refreshed successfully.");
                            }
                        }
                        Err(err) => {
                            tracing::warn!(
                                error_code = %crate::error::ErrorCode::ErrSsoTokenRefresh,
                                error = %err,
                                "EVE SSO background token refresh encountered error: {}",
                                err
                            );
                            if err.contains("invalid_grant") || err.contains("revoked") {
                                self.sso_character_name = None;
                                self.sso_character_id = None;
                                self.sso_token_expires_at = None;
                                self.set_toast(
                                    "EVE SSO session expired or was revoked. Please connect character.",
                                );
                            } else if force {
                                self.set_toast(format!("SSO token refresh error: {}", err));
                            } else {
                                // Transient error during automatic background refresh - silently retry on next interval
                            }
                        }
                    }
                    self.sso_refresh_rx = None;
                    ctx.request_repaint();
                }
                Err(TryRecvError::Disconnected) => {
                    self.sso_is_refreshing_token = false;
                    self.sso_refresh_rx = None;
                    tracing::warn!(
                        error_code = %crate::error::ErrorCode::ErrUiWorkerChannel,
                        "SSO token refresh worker disconnected unexpectedly"
                    );
                    ctx.request_repaint();
                }
                Err(TryRecvError::Empty) => {}
            }
        }
    }

    /// Polls all active background worker channels (market scan, chain routes, SSO auth, push, location, token refresh)
    /// and resets busy flags if any worker thread disconnected or terminated unexpectedly.
    pub fn poll_active_workers(&mut self, ctx: &egui::Context) {
        self.poll_sso_refresh(ctx);

        // Poll market scan worker thread
        if let Some(ref rx) = self.scan_rx {
            match rx.try_recv() {
                Ok(res) => {
                    self.is_scanning = false;
                    self.last_scan_completed = Some(Instant::now());
                    match res {
                        ScanResult::Success {
                            deals,
                            manifests,
                            route,
                            total_sells,
                            total_buys,
                        } => {
                            let count = deals.len();
                            let manifest_count = manifests.len();
                            self.status_text = format!(
                                "[OK] Scan Complete! Parsed {} sell orders & {} buy orders. Discovered {} deals (ROI >= {:.1}%) across {} packed manifests.",
                                total_sells, total_buys, count, self.min_roi_pct, manifest_count
                            );

                            // Check for newly discovered high-profit trades in auto-refresh mode
                            let mut new_high_profit_count = 0;
                            if self.auto_refresh && self.sound_alerts_enabled {
                                for d in &deals {
                                    if d.net_profit_pct >= self.sound_min_roi_pct {
                                        let key = (d.type_id, d.source_station_id, d.dest_station_id);
                                        if !self.alerted_deal_keys.contains(&key) {
                                            self.alerted_deal_keys.insert(key);
                                            new_high_profit_count += 1;
                                        }
                                    }
                                }
                            }

                            // Prune old alerted keys that no longer exist in current deals pool
                            let current_high_profit_keys: std::collections::HashSet<_> = deals
                                .iter()
                                .filter(|d| d.net_profit_pct >= self.sound_min_roi_pct)
                                .map(|d| (d.type_id, d.source_station_id, d.dest_station_id))
                                .collect();
                            self.alerted_deal_keys
                                .retain(|k| current_high_profit_keys.contains(k));

                            if new_high_profit_count > 0 {
                                crate::audio::play_high_profit_alert();
                                self.set_toast(format!(
                                    "[ALERT] HIGH PROFIT: Found {} new trade(s) with ≥ {:.0}% Net ROI!",
                                    new_high_profit_count, self.sound_min_roi_pct
                                ));
                            } else {
                                // Completely silent on normal refresh
                                self.set_toast(format!(
                                    "Discovered {} profitable deals ({} manifests)!",
                                    count, manifest_count
                                ));
                            }

                            self.deals = deals;
                            self.manifests = manifests;
                            self.route = Some(route);
                            self.selected_deal_idx = None;
                        }
                        ScanResult::Error(err) => {
                            tracing::error!(
                                error_code = %crate::error::ErrorCode::ErrEsiNetworkFailed,
                                error = %err,
                                "Market scan failed with error: {}",
                                err
                            );
                            self.status_text = format!("[!] {}", err);
                            self.set_toast(format!("Error: {}", err));
                        }
                    }
                    self.scan_rx = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.is_scanning = false;
                    self.scan_rx = None;
                    tracing::error!(
                        error_code = %crate::error::ErrorCode::ErrUiWorkerChannel,
                        "Market scan worker thread disconnected unexpectedly"
                    );
                    self.status_text = "[!] Market scan worker terminated unexpectedly.".to_string();
                    self.set_toast("Market scan worker failed unexpectedly. You may retry.");
                }
                Err(TryRecvError::Empty) => {}
            }
        }

        // Poll chain route worker thread
        if let Some(ref rx) = self.chain_rx {
            match rx.try_recv() {
                Ok(results) => {
                    self.chain_is_running = false;
                    let count = results.len();
                    self.chain_results = results;
                    self.chain_selected_idx = if self.chain_results.is_empty() {
                        None
                    } else {
                        Some(0)
                    };
                    if count > 0 {
                        self.set_toast(format!("Found {} optimized chained trade route(s)!", count));
                    } else {
                        self.set_toast("No chain routes found with current parameters.");
                    }
                    self.chain_rx = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.chain_is_running = false;
                    self.chain_rx = None;
                    tracing::error!(
                        error_code = %crate::error::ErrorCode::ErrUiWorkerChannel,
                        "Chain route planner worker thread disconnected unexpectedly"
                    );
                    self.set_toast("Chain route calculation failed unexpectedly. You may retry.");
                }
                Err(TryRecvError::Empty) => {}
            }
        }

        if self.chain_is_running {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        // Poll SSO auth worker
        if let Some(ref rx) = self.sso_auth_rx {
            match rx.try_recv() {
                Ok(res) => {
                    self.sso_is_authenticating = false;
                    match res {
                        Ok(tok) => {
                            self.sso_character_name = Some(tok.character_name.clone());
                            self.sso_character_id = Some(tok.character_id);
                            self.sso_token_expires_at = Some(tok.expires_at);
                            self.set_toast(format!(
                                "Connected to EVE character: {}",
                                tok.character_name
                            ));
                            crate::audio::play_high_profit_alert();
                        }
                        Err(err) => {
                            tracing::error!(
                                target: "eve_trade_scout",
                                error_code = %crate::error::ErrorCode::ErrSsoTokenExchange,
                                error = %err,
                                "SSO authentication worker reported failure: {err}"
                            );
                            self.set_toast(format!("SSO Authentication Error: {}", err));
                        }
                    }
                    self.sso_auth_rx = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.sso_is_authenticating = false;
                    self.sso_auth_rx = None;
                    tracing::error!(
                        target: "eve_trade_scout",
                        error_code = %crate::error::ErrorCode::ErrUiWorkerChannel,
                        "SSO authentication worker disconnected unexpectedly"
                    );
                    self.set_toast("SSO Authentication worker failed unexpectedly.");
                }
                Err(TryRecvError::Empty) => {}
            }
        }

        // Poll SSO autopilot push worker
        if let Some(ref rx) = self.sso_push_rx {
            match rx.try_recv() {
                Ok(res) => {
                    self.sso_is_pushing = false;
                    match res {
                        Ok(stops) => {
                            self.sync_sso_token_from_disk();
                            self.set_toast(format!(
                                "Injected {} waypoint(s) into live EVE client HUD!",
                                stops
                            ));
                            crate::audio::play_high_profit_alert();
                        }
                        Err(err) => {
                            tracing::error!(
                                target: "eve_trade_scout",
                                error_code = %crate::error::ErrorCode::ErrSsoAutopilotFailed,
                                error = %err,
                                "SSO waypoint injection worker reported failure: {err}"
                            );
                            if err.contains("invalid_grant") || err.contains("revoked") {
                                self.sso_character_name = None;
                                self.sso_character_id = None;
                                self.sso_token_expires_at = None;
                                self.set_toast(
                                    "EVE SSO session expired or was revoked. Please connect character in Settings.",
                                );
                            } else {
                                self.set_toast(format!("Autopilot injection failed: {}", err));
                            }
                        }
                    }
                    self.sso_push_rx = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.sso_is_pushing = false;
                    self.sso_push_rx = None;
                    tracing::error!(
                        target: "eve_trade_scout",
                        error_code = %crate::error::ErrorCode::ErrUiWorkerChannel,
                        "SSO waypoint injection worker disconnected unexpectedly"
                    );
                    self.set_toast("Autopilot injection worker failed unexpectedly.");
                }
                Err(TryRecvError::Empty) => {}
            }
        }

        // Poll SSO character location worker
        if let Some(ref rx) = self.sso_location_rx {
            match rx.try_recv() {
                Ok(res) => {
                    self.sso_is_fetching_location = false;
                    let scan_after = self.sso_location_scan_after;
                    self.sso_location_scan_after = false;
                    match res {
                        Ok(loc) => {
                            self.sync_sso_token_from_disk();
                            let sys_name_opt = self
                                .sde
                                .as_ref()
                                .and_then(|s| s.get_system_name(loc.solar_system_id));
                            if let Some(sys_name) = sys_name_opt {
                                let old_system = self.home_system.clone();
                                self.home_system = sys_name.clone();
                                self.set_toast(format!(
                                    "Detected in-game system: {} (ID: {})",
                                    sys_name, loc.solar_system_id
                                ));
                                crate::audio::play_high_profit_alert();
                                if scan_after || old_system != sys_name {
                                    self.start_scan();
                                }
                            } else {
                                tracing::warn!(
                                    target: "eve_trade_scout",
                                    error_code = %crate::error::ErrorCode::ErrSdeSystemNotFound,
                                    system_id = loc.solar_system_id,
                                    "In-game solar system ID {} detected, but not found in SDE",
                                    loc.solar_system_id
                                );
                                self.set_toast(format!(
                                    "In-game solar system ID {} detected, but not found in SDE.",
                                    loc.solar_system_id
                                ));
                            }
                        }
                        Err(err) => {
                            tracing::error!(
                                target: "eve_trade_scout",
                                error_code = %crate::error::ErrorCode::ErrSsoLocationFailed,
                                error = %err,
                                "SSO character location sync worker reported failure: {err}"
                            );
                            if err.contains("invalid_grant") || err.contains("revoked") {
                                self.sso_character_name = None;
                                self.sso_character_id = None;
                                self.sso_token_expires_at = None;
                                self.set_toast(
                                    "EVE SSO session expired or was revoked. Please connect character in Settings.",
                                );
                            } else {
                                self.set_toast(format!("Location sync error: {}", err));
                            }
                        }
                    }
                    self.sso_location_rx = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.sso_is_fetching_location = false;
                    self.sso_location_scan_after = false;
                    self.sso_location_rx = None;
                    tracing::error!(
                        target: "eve_trade_scout",
                        error_code = %crate::error::ErrorCode::ErrUiWorkerChannel,
                        "SSO character location worker disconnected unexpectedly"
                    );
                    self.set_toast("Character location sync failed unexpectedly.");
                }
                Err(TryRecvError::Empty) => {}
            }
        }
    }


    #[doc(hidden)]
    pub fn is_chain_running(&self) -> bool {
        self.chain_is_running
    }

    #[doc(hidden)]
    pub fn is_sso_refreshing(&self) -> bool {
        self.sso_is_refreshing_token
    }

    #[doc(hidden)]
    pub fn is_sso_authenticating(&self) -> bool {
        self.sso_is_authenticating
    }

    #[doc(hidden)]
    pub fn is_sso_pushing(&self) -> bool {
        self.sso_is_pushing
    }

    #[doc(hidden)]
    pub fn is_sso_fetching_location(&self) -> bool {
        self.sso_is_fetching_location
    }

    #[doc(hidden)]
    pub fn toast_text(&self) -> Option<&str> {
        self.toast.as_ref().map(|(msg, _)| msg.as_str())
    }

    #[doc(hidden)]
    pub fn set_scan_rx_for_test(&mut self, rx: Receiver<ScanResult>) {
        self.is_scanning = true;
        self.scan_rx = Some(rx);
    }

    #[doc(hidden)]
    pub fn set_chain_rx_for_test(
        &mut self,
        rx: Receiver<Vec<crate::chain_route_planner::ChainRoute>>,
    ) {
        self.chain_is_running = true;
        self.chain_rx = Some(rx);
    }

    #[doc(hidden)]
    pub fn set_sso_refresh_rx_for_test(
        &mut self,
        rx: Receiver<(Result<crate::esi::sso::SsoToken, String>, bool)>,
    ) {
        self.sso_is_refreshing_token = true;
        self.sso_refresh_rx = Some(rx);
    }

    #[doc(hidden)]
    pub fn set_sso_auth_rx_for_test(
        &mut self,
        rx: Receiver<Result<crate::esi::sso::SsoToken, String>>,
    ) {
        self.sso_is_authenticating = true;
        self.sso_auth_rx = Some(rx);
    }

    #[doc(hidden)]
    pub fn set_sso_push_rx_for_test(&mut self, rx: Receiver<Result<usize, String>>) {
        self.sso_is_pushing = true;
        self.sso_push_rx = Some(rx);
    }

    #[doc(hidden)]
    pub fn set_sso_location_rx_for_test(
        &mut self,
        rx: Receiver<Result<crate::esi::sso::CharacterLocation, String>>,
    ) {
        self.sso_is_fetching_location = true;
        self.sso_location_rx = Some(rx);
    }

    /// Syncs in-memory SSO token state (character name, ID, and expiration) with the stored token on disk.
    pub fn sync_sso_token_from_disk(&mut self) {
        if let Some(tok) = crate::esi::sso::load_saved_token() {
            self.sso_character_name = Some(tok.character_name);
            self.sso_character_id = Some(tok.character_id);
            self.sso_token_expires_at = Some(tok.expires_at);
        }
    }

    pub fn start_sso_authentication(&mut self) {
        if self.sso_is_authenticating {
            return;
        }
        let client_id = match crate::esi::sso::resolve_client_id(&self.sso_client_id) {
            Ok(id) => id,
            Err(e) => {
                self.set_toast(format!("SSO unavailable: {}", e));
                return;
            }
        };

        self.sso_is_authenticating = true;
        self.set_toast("Opening browser for EVE SSO authentication...");

        let (tx, rx) = channel();
        self.sso_auth_rx = Some(rx);

        let callback_port = crate::config::UserConfig::load()
            .map(|c| c.sso.validated_callback_port())
            .unwrap_or(crate::esi::sso::DEFAULT_CALLBACK_PORT);
        thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(Err(format!("Runtime build error: {}", e)));
                    return;
                }
            };

            let http_client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());
            let res = rt.block_on(crate::esi::sso::authenticate_character_via_pkce(
                &http_client,
                &client_id,
                callback_port,
            ));
            let _ = tx.send(res.map_err(|e| e.to_string()));
        });
    }

    pub fn disconnect_sso(&mut self) {
        let _ = crate::esi::sso::delete_saved_token();
        self.sso_character_name = None;
        self.sso_character_id = None;
        self.sso_token_expires_at = None;
        self.set_toast("Disconnected EVE character and deleted saved token.");
    }

    pub fn push_to_eve_autopilot(&mut self, destination_ids: Vec<u64>, label: &str) {
        if self.sso_is_pushing {
            self.set_toast("Autopilot sync already in progress...");
            return;
        }
        if destination_ids.is_empty() {
            self.set_toast("No waypoints found to push to EVE client.");
            return;
        }
        if self.sso_character_name.is_none() {
            self.set_toast("Please connect your EVE character in Settings first!");
            self.tab = ActiveTab::Settings;
            return;
        }

        let client_id = match crate::esi::sso::resolve_client_id(&self.sso_client_id) {
            Ok(id) => id,
            Err(e) => {
                self.set_toast(format!("Autopilot sync error: {}", e));
                return;
            }
        };
        self.sso_is_pushing = true;
        self.set_toast(format!(
            "Injecting {} waypoints into live EVE HUD for {}...",
            destination_ids.len(),
            label
        ));

        let (tx, rx) = channel();
        self.sso_push_rx = Some(rx);

        thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(Err(format!("Runtime build error: {}", e)));
                    return;
                }
            };

            let http_client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());
            let res = rt.block_on(crate::esi::sso::push_waypoints_with_token(
                &http_client,
                &client_id,
                &destination_ids,
            ));
            let _ = tx.send(res.map_err(|e| e.to_string()));
        });
    }

    pub fn fetch_live_character_location(&mut self, scan_after: bool) {
        if self.sso_is_fetching_location {
            return;
        }
        if self.sso_character_name.is_none() {
            self.set_toast("Please connect your EVE character in Settings first!");
            self.tab = ActiveTab::Settings;
            return;
        }

        let client_id = match crate::esi::sso::resolve_client_id(&self.sso_client_id) {
            Ok(id) => id,
            Err(e) => {
                self.set_toast(format!("Location sync error: {}", e));
                return;
            }
        };
        self.sso_is_fetching_location = true;
        self.sso_location_scan_after = scan_after;
        self.set_toast("Querying live character location from CCP ESI...");

        let (tx, rx) = channel();
        self.sso_location_rx = Some(rx);

        thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(Err(format!("Runtime build error: {}", e)));
                    return;
                }
            };

            let http_client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());
            let res = rt.block_on(crate::esi::sso::get_character_location_with_token(
                &http_client,
                &client_id,
            ));
            let _ = tx.send(res.map_err(|e| e.to_string()));
        });
    }

    fn selected_regions(&self) -> Vec<u32> {
        let mut set = std::collections::HashSet::new();

        if let (Some(sde), Some(graph)) = (&self.sde, &self.graph) {
            if let Some(home_id) = sde.get_system_id(&self.home_system) {
                if let Some(home_reg) = sde.get_system_region(home_id) {
                    set.insert(home_reg);
                    if self.include_adjacent_regions {
                        for adj in graph.adjacent_regions(home_reg, self.safe_only) {
                            set.insert(adj);
                        }
                    }
                }
            }
        }

        if self.region_the_forge {
            set.insert(10000002);
        }
        if self.region_domain {
            set.insert(10000043);
        }
        if self.region_sinq_laison {
            set.insert(10000032);
        }
        if self.region_heimatar {
            set.insert(10000030);
        }
        if self.region_metropolis {
            set.insert(10000042);
        }

        if set.is_empty() {
            set.insert(10000002); // Default to The Forge (Jita)
        }

        let mut list: Vec<u32> = set.into_iter().collect();
        list.sort_unstable();
        list
    }

    /// Returns true if any essential universe dependency (SDE, Stargate graph, ESI client) is uninitialized.
    pub fn is_offline(&self) -> bool {
        self.sde.is_none() || self.graph.is_none() || self.esi.is_none()
    }

    /// Returns the current application telemetry status text.
    pub fn status_text(&self) -> &str {
        &self.status_text
    }

    /// Returns true if an asynchronous market scan is actively running.
    pub fn is_scanning(&self) -> bool {
        self.is_scanning
    }

    /// Initiates a live asynchronous market scan across active regions.
    pub fn start_scan(&mut self) {
        if self.is_scanning {
            return;
        }

        let _ = self.save_user_config();

        let (Some(sde), Some(graph), Some(esi)) = (&self.sde, &self.graph, &self.esi) else {
            self.status_text =
                "[WARN] Cannot scan live market: SDE universe database or ESI client is offline."
                    .to_string();
            return;
        };

        self.is_scanning = true;
        self.status_text = "Connecting to CCP ESI API & streaming market orders...".to_string();

        let sde = Arc::clone(sde);
        let graph = Arc::clone(graph);
        let esi = Arc::clone(esi);
        let (tx, rx) = channel();
        self.scan_rx = Some(rx);

        let ship = self.current_ship();

        let home_name = self.home_system.clone();
        let safe_only = self.safe_only;
        let npc_stations_only = self.npc_stations_only;
        let min_roi = self.min_roi_pct.max(10.0); // Strict 10% invariant
        let min_profit = self.min_profit_isk;
        let broker_fee = self.broker_fee_pct;
        let sales_tax = self.sales_tax_pct;
        let max_stops = self.max_route_stops;
        let regions = self.selected_regions();

        thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(
                        error_code = %crate::error::ErrorCode::ErrUiRuntimeInit,
                        error = %e,
                        "Failed to start Tokio runtime for market scanner"
                    );
                    let _ = tx.send(ScanResult::Error(format!(
                        "Failed to start Tokio runtime: {}",
                        e
                    )));
                    return;
                }
            };

            rt.block_on(async move {
                let home_id = match sde.get_system_id(&home_name) {
                    Some(id) => id,
                    None => {
                        tracing::warn!(
                            error_code = %crate::error::ErrorCode::ErrSdeSystemNotFound,
                            system = %home_name,
                            "Starting solar system not found in universe database"
                        );
                        let _ = tx.send(ScanResult::Error(format!(
                            "Solar system '{}' not found in EVE universe database.",
                            home_name
                        )));
                        return;
                    }
                };

                let markets = match fetch_markets_fresh(&esi, &regions).await {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::error!(
                            error_code = %crate::error::ErrorCode::ErrEsiNetworkFailed,
                            error = %e,
                            regions = ?regions,
                            "Market order stream failed across regions: {}",
                            e
                        );
                        let _ = tx.send(ScanResult::Error(format!("ESI API Error: {}", e)));
                        return;
                    }
                };

                let (total_sells, total_buys) = count_market_orders(&markets);

                let arb_cfg = ArbitrageConfig {
                    ship,
                    home_system: home_id,
                    min_profit,
                    min_spread: 3.0,
                    min_net_profit_pct: min_roi,
                    broker_fee,
                    sales_tax,
                    safe_only,
                    npc_stations_only,
                };

                let deals = find_deals(&markets, &sde, &graph, &arb_cfg);
                let manifests = bundle_route_cargo(&deals, ship);
                let route = plan_route(&deals, ship, home_id, &graph, safe_only, max_stops);

                // Save updated ESI cache to disk
                let _ = esi.save_cache().await;

                let _ = tx.send(ScanResult::Success {
                    deals,
                    manifests,
                    route,
                    total_sells,
                    total_buys,
                });
            });
        });
    }

    /// Polls startup messages from the background worker channel and updates progress / errors.
    pub fn poll_startup(&mut self, ctx: &egui::Context) {
        let mut startup_complete = None;
        if let Some(ref rx) = self.startup_rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    StartupMsg::Progress(prog) => {
                        self.startup_progress = prog;
                        ctx.request_repaint();
                    }
                    StartupMsg::Ready {
                        sde,
                        graph,
                        esi,
                        home_system_name,
                        scan_regions,
                        deals,
                        manifests,
                        route,
                    } => {
                        startup_complete = Some((
                            sde,
                            graph,
                            esi,
                            home_system_name,
                            scan_regions,
                            deals,
                            manifests,
                            route,
                        ));
                        break;
                    }
                    StartupMsg::Error { code, message } => {
                        tracing::error!(
                            target: "eve_trade_scout",
                            error_code = %code,
                            error = %message,
                            "Application startup sequence failed at stage '{}': {}",
                            self.startup_progress.stage_name,
                            message
                        );
                        self.startup_progress.error = Some(message);
                        self.startup_progress.error_code = Some(code);
                        ctx.request_repaint();
                    }
                }
            }

            // Catch-all: If startup is still pending and no error was explicitly set, check if the worker channel disconnected unexpectedly
            if startup_complete.is_none() && self.startup_progress.error.is_none() {
                if let Err(std::sync::mpsc::TryRecvError::Disconnected) = rx.try_recv() {
                    let err_code = crate::error::ErrorCode::ErrUiWorkerChannel;
                    let err_msg = "Startup background worker channel disconnected unexpectedly before completion. Worker may have terminated prematurely.".to_string();
                    tracing::error!(
                        target: "eve_trade_scout",
                        error_code = %err_code,
                        "{}",
                        err_msg
                    );
                    self.startup_progress.error = Some(err_msg);
                    self.startup_progress.error_code = Some(err_code);
                    ctx.request_repaint();
                }
            }
        }

        if let Some((sde, graph, esi, home_system_name, _scan_regions, deals, manifests, route)) =
            startup_complete
        {
            self.sde = Some(sde);
            self.graph = Some(graph);
            self.esi = Some(esi);
            if self.is_startup || self.home_system == "Jita" || self.home_system.is_empty() {
                self.home_system = home_system_name;
            }
            self.deals = deals;
            self.manifests = manifests;
            self.route = route;
            self.is_startup = false;
            self.startup_rx = None;
            self.status_text = format!(
                "[OK] Ready! Loaded {} profitable trade deals across {} packed manifests.",
                self.deals.len(),
                self.manifests.len()
            );
            if self.sso_character_name.is_some() && self.sso_auto_sync_location {
                self.fetch_live_character_location(true);
            }
            ctx.request_repaint();
        }
    }
}

impl eframe::App for DesktopApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.has_applied_initial_viewport {
            self.has_applied_initial_viewport = true;
            ctx.set_zoom_factor(self.ui_scale);
            if self.is_fullscreen {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
            }
        }

        // Startup splash handling
        if self.is_startup {
            self.poll_startup(ctx);

            // Poll SSO token refresh concurrently during startup splash screen
            self.poll_sso_refresh(ctx);

            if self.is_startup {
                ctx.request_repaint_after(Duration::from_millis(16));
                self.render_splash_screen(ctx);
                return;
            }
        }

        // Poll all background worker channels
        self.poll_active_workers(ctx);

        if self.chain_is_running
            || self.sso_is_authenticating
            || self.sso_is_pushing
            || self.sso_is_fetching_location
            || self.sso_is_refreshing_token
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else if self.sso_character_name.is_some() {
            // Keep background token heartbeat and countdown timer alive even when window is idle/unfocused
            ctx.request_repaint_after(Duration::from_secs(5));
        }

        // Auto-refresh timer check
        if self.auto_refresh && !self.is_scanning {
            let elapsed = self
                .last_scan_completed
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(self.auto_refresh_secs);
            if elapsed >= self.auto_refresh_secs {
                self.start_scan();
            } else {
                ctx.request_repaint_after(Duration::from_secs(1));
            }
        }

        // ── Global Shortcuts: Fullscreen (F11) & Dynamic UI Zoom (Ctrl +/-/0) ──
        if ctx.input(|i| i.key_pressed(egui::Key::F11)) {
            self.is_fullscreen = !self.is_fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.is_fullscreen));
            self.set_toast(if self.is_fullscreen {
                "Entered Fullscreen mode (F11)"
            } else {
                "Exited Fullscreen mode (F11)"
            });
        }

        let ctrl = ctx.input(|i| i.modifiers.command || i.modifiers.ctrl);
        if ctrl {
            if ctx.input(|i| i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals)) {
                self.ui_scale = (self.ui_scale + 0.05).min(1.50);
                self.set_toast(format!("UI Zoom: {:.0}%", self.ui_scale * 100.0));
            } else if ctx.input(|i| i.key_pressed(egui::Key::Minus)) {
                self.ui_scale = (self.ui_scale - 0.05).max(0.75);
                self.set_toast(format!("UI Zoom: {:.0}%", self.ui_scale * 100.0));
            } else if ctx.input(|i| i.key_pressed(egui::Key::Num0)) {
                self.ui_scale = 1.0;
                self.set_toast("UI Zoom reset to 100%");
            }
        }
        ctx.set_zoom_factor(self.ui_scale);

        // Apply Active Cockpit Visual Theme
        let palette = self.theme.palette();
        palette.apply_to_ctx(ctx);

        // Top Control and Navigation Panel
        egui::TopBottomPanel::top("top_panel")
            .frame(palette.top_panel_frame())
            .show(ctx, |ui| {
                self.render_header_and_controls(ui, ctx);
            });

        // Bottom Status Bar & Toast Notifications
        egui::TopBottomPanel::bottom("bottom_panel")
            .frame(palette.bottom_panel_frame())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (indicator_color, indicator_sym) = if self.is_scanning {
                        (Color32::from_rgb(255, 200, 0), "● SCANNING")
                    } else if self.status_text.starts_with("[!") {
                        (Color32::from_rgb(255, 80, 80), "● ERROR")
                    } else {
                        (Color32::from_rgb(0, 240, 160), "● READY")
                    };

                    ui.label(
                        RichText::new(indicator_sym)
                            .size(11.0)
                            .color(indicator_color)
                            .strong(),
                    );
                    if self.auto_refresh {
                        let audio_tag = if self.sound_alerts_enabled {
                            " • SOUND ON"
                        } else {
                            " • MUTED"
                        };
                        ui.label(
                            RichText::new(format!("• AUTO-REFRESH ON{}", audio_tag))
                                .size(11.0)
                                .color(Color32::from_rgb(0, 200, 255))
                                .strong(),
                        );
                    }
                    ui.separator();
                    ui.label(
                        RichText::new(&self.status_text)
                            .size(12.0)
                            .color(palette.text_secondary),
                    );

                    // Toast notification — fading glowing card
                    if let Some((ref msg, time)) = self.toast {
                        let elapsed = time.elapsed().as_secs_f32();
                        if elapsed < 4.0 {
                            let alpha = ((4.0 - elapsed) / 4.0 * 220.0) as u8;
                            let toast_col = Color32::from_rgba_unmultiplied(
                                palette.accent_primary.r(),
                                palette.accent_primary.g(),
                                palette.accent_primary.b(),
                                alpha,
                            );
                            let frame_fill =
                                Color32::from_rgba_unmultiplied(palette.card_bg.r(), palette.card_bg.g(), palette.card_bg.b(), (alpha / 6).max(10));
                            let frame_stroke =
                                Color32::from_rgba_unmultiplied(palette.accent_primary.r(), palette.accent_primary.g(), palette.accent_primary.b(), (alpha / 2).max(18));
                            ui.separator();
                            egui::Frame::none()
                                .fill(frame_fill)
                                .stroke(Stroke::new(1.0_f32, frame_stroke))
                                .rounding(5.0)
                                .inner_margin(6.0)
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(msg.as_str())
                                            .size(12.0)
                                            .color(toast_col)
                                            .strong(),
                                    );
                                });
                            ctx.request_repaint();
                        }
                    }

                    let corp_tex = self.ensure_corp_icon(ctx).cloned();
                    let mut copy_corp_clicked = false;

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("EVE Traveling Merchant • Linux Fedora Native")
                                .size(11.0)
                                .color(palette.text_muted),
                        );

                        ui.separator();

                        // In-Game ISK Donation Banner
                        egui::Frame::none()
                            .fill(palette.card_sub_bg)
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(218, 165, 32)))
                            .rounding(4.0)
                            .inner_margin(egui::vec2(6.0, 2.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if let Some(ref tex) = corp_tex {
                                        ui.image((tex.id(), egui::vec2(16.0, 16.0)));
                                    }
                                    ui.label(
                                        RichText::new("Support: ISK to")
                                            .size(11.0)
                                            .color(palette.text_secondary),
                                    );
                                    ui.label(
                                        RichText::new(CORP_DONATION_RECIPIENT)
                                            .size(11.0)
                                            .color(Color32::from_rgb(255, 215, 0))
                                            .strong(),
                                    );
                                    if ui
                                        .button(
                                            RichText::new("Copy Corp")
                                                .size(10.5)
                                                .color(Color32::from_rgb(255, 230, 120))
                                                .strong(),
                                        )
                                        .on_hover_ui(|ui| {
                                            ui.heading("In-Game ISK Donations");
                                            ui.label("EVE Traveling Merchant is 100% free & open-source.");
                                            ui.label("Send voluntary ISK donations in-game to support development:");
                                            ui.label(
                                                RichText::new("Corporation: Expandabliss")
                                                    .color(Color32::from_rgb(255, 215, 0))
                                                    .strong(),
                                            );
                                            ui.add_space(4.0);
                                            ui.label("1. Press Alt+W in EVE to open Wallet");
                                            ui.label("2. Click 'Transfer ISK'");
                                            ui.label("3. Search recipient: Expandabliss (Corporation)");
                                            ui.label("Fly safe o7!");
                                        })
                                        .clicked()
                                    {
                                        copy_corp_clicked = true;
                                    }
                                });
                            });
                    });

                    if copy_corp_clicked {
                        ui.output_mut(|o| o.copied_text = CORP_DONATION_RECIPIENT.to_string());
                        self.set_toast(
                            "Copied 'Expandabliss' to clipboard! Send ISK in-game via Wallet > Transfer.",
                        );
                    }
                });
            });

        // Central Panel
        egui::CentralPanel::default()
            .frame(palette.central_panel_frame())
            .show(ctx, |ui| match self.tab {
                ActiveTab::Deals => self.render_deals_tab(ui),
                ActiveTab::Manifests => self.render_manifests_tab(ui),
                ActiveTab::Route => self.render_route_tab(ui),
                ActiveTab::ChainRoute => self.render_chain_route_tab(ui),
                ActiveTab::HubArbitrage => self.render_hub_arbitrage_tab(ui),
                ActiveTab::Settings => self.render_settings_tab(ui),
            });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Err(e) = self.save_user_config() {
            tracing::error!(
                target: "eve_trade_scout",
                error_code = %crate::error::ErrorCode::ErrConfigSaveFailed,
                error = %e,
                "Failed to auto-save user configuration on application exit"
            );
        } else {
            tracing::info!(target: "eve_trade_scout", "User configuration saved successfully on application exit.");
        }
    }
}

fn security_color(sec: f64) -> Color32 {
    if sec >= 0.9 {
        Color32::from_rgb(46, 204, 113) // Bright green
    } else if sec >= 0.7 {
        Color32::from_rgb(0, 210, 160) // Teal/Green
    } else if sec >= 0.5 {
        Color32::from_rgb(0, 200, 255) // Cyan
    } else if sec >= 0.3 {
        Color32::from_rgb(241, 196, 15) // Yellow
    } else if sec > 0.0 {
        Color32::from_rgb(230, 126, 34) // Orange
    } else {
        Color32::from_rgb(231, 76, 60) // Red
    }
}

fn format_rate_isk(val: f64) -> String {
    if val >= 1_000_000.0 {
        format!("{:.1}M", val / 1_000_000.0)
    } else if val >= 1_000.0 {
        format!("{:.1}k", val / 1_000.0)
    } else {
        format!("{:.0}", val)
    }
}

use super::format_unit_price as format_unit_price_isk;

/// Two stacked mini progress bars: cyan for cargo volume, gold for ISK budget.
fn render_ship_util_bars(ui: &mut egui::Ui, vol_frac: f32, budget_frac: f32) -> egui::Response {
    let bar_w = 80.0_f32;
    let bar_h = 5.0_f32;
    let gap = 3.0_f32;
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(bar_w, bar_h * 2.0 + gap), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();

        // Cargo volume bar (cyan, top)
        let vol_track = egui::Rect::from_min_size(rect.min, Vec2::new(bar_w, bar_h));
        painter.rect_filled(
            vol_track,
            egui::Rounding::same(2.0),
            Color32::from_rgb(16, 32, 58),
        );
        let vol_w = (bar_w * vol_frac.clamp(0.0, 1.0)).max(0.0);
        if vol_w > 0.0 {
            let vol_col = if vol_frac >= 0.9 {
                Color32::from_rgb(255, 90, 50)
            } else {
                Color32::from_rgb(0, 200, 255)
            };
            painter.rect_filled(
                egui::Rect::from_min_size(rect.min, Vec2::new(vol_w, bar_h)),
                egui::Rounding::same(2.0),
                vol_col,
            );
        }

        // ISK budget bar (gold, bottom)
        let isk_min = egui::pos2(rect.min.x, rect.min.y + bar_h + gap);
        let isk_track = egui::Rect::from_min_size(isk_min, Vec2::new(bar_w, bar_h));
        painter.rect_filled(
            isk_track,
            egui::Rounding::same(2.0),
            Color32::from_rgb(38, 28, 10),
        );
        let isk_w = (bar_w * budget_frac.clamp(0.0, 1.0)).max(0.0);
        if isk_w > 0.0 {
            let isk_col = if budget_frac >= 0.9 {
                Color32::from_rgb(255, 70, 70)
            } else {
                Color32::from_rgb(255, 200, 50)
            };
            painter.rect_filled(
                egui::Rect::from_min_size(isk_min, Vec2::new(isk_w, bar_h)),
                egui::Rounding::same(2.0),
                isk_col,
            );
        }
    }
    resp
}

#[derive(Default)]
struct DealsGridAction {
    pub toggle_select: Option<usize>,
    pub action_toast: Option<String>,
    pub change_sort: Option<(DealSortBy, bool)>,
    pub direct_push: Option<Vec<u64>>,
}

impl DesktopApp {
    fn render_header_and_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let is_widescreen = ui.available_width() >= 1150.0;

        let palette = self.theme.palette();

        // Header primary action buttons (Scan, Auto-Refresh, Audio, Zoom, Fullscreen)
        let render_primary_actions = |app: &mut Self, ui: &mut egui::Ui, ctx: &egui::Context| {
            let palette = app.theme.palette();
            // Fullscreen toggle button
            let fs_label = if app.is_fullscreen {
                "Windowed"
            } else {
                "Fullscreen"
            };
            let fs_btn = egui::Button::new(
                RichText::new(fs_label)
                    .size(12.0)
                    .color(palette.accent_primary)
                    .strong(),
            )
            .fill(palette.card_sub_bg);
            if ui
                .add(fs_btn)
                .on_hover_text("Toggle borderless fullscreen mode (F11)")
                .clicked()
            {
                app.is_fullscreen = !app.is_fullscreen;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(app.is_fullscreen));
                app.set_toast(if app.is_fullscreen {
                    "Entered Fullscreen mode (F11)"
                } else {
                    "Exited Fullscreen mode (F11)"
                });
            }

            ui.add_space(4.0);

            // Dynamic UI Zoom Controls
            egui::Frame::none()
                .fill(palette.card_sub_bg)
                .stroke(palette.card_sub_stroke)
                .rounding(4.0)
                .inner_margin(egui::vec2(6.0, 3.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .small_button("-")
                            .on_hover_text("Zoom Out (Ctrl -)")
                            .clicked()
                        {
                            app.ui_scale = (app.ui_scale - 0.05).max(0.75);
                            app.set_toast(format!("UI Zoom: {:.0}%", app.ui_scale * 100.0));
                        }
                        let zoom_pct = format!("{:.0}%", app.ui_scale * 100.0);
                        if ui
                            .button(
                                RichText::new(zoom_pct)
                                    .size(11.5)
                                    .strong()
                                    .color(palette.accent_primary),
                            )
                            .on_hover_text("Click to reset zoom to 100% (Ctrl 0)")
                            .clicked()
                        {
                            app.ui_scale = 1.0;
                            app.set_toast("UI Zoom reset to 100%");
                        }
                        if ui
                            .small_button("+")
                            .on_hover_text("Zoom In (Ctrl +)")
                            .clicked()
                        {
                            app.ui_scale = (app.ui_scale + 0.05).min(1.50);
                            app.set_toast(format!("UI Zoom: {:.0}%", app.ui_scale * 100.0));
                        }
                    });
                });

            ui.add_space(4.0);

            // Audio alert toggle button
            let (sound_icon, sound_color) = if app.sound_alerts_enabled {
                (
                    format!("Sound Alert (≥{:.0}%)", app.sound_min_roi_pct),
                    palette.accent_primary,
                )
            } else {
                ("Muted".to_string(), palette.text_muted)
            };
            let sound_btn = egui::Button::new(
                RichText::new(sound_icon)
                    .size(12.0)
                    .color(sound_color)
                    .strong(),
            )
            .fill(palette.card_sub_bg);
            if ui
                .add(sound_btn)
                .on_hover_text("Audio chime alert for high-profit trades discovered during Auto-Refresh. Click to toggle.")
                .clicked()
            {
                app.sound_alerts_enabled = !app.sound_alerts_enabled;
                if app.sound_alerts_enabled {
                    crate::audio::play_test_sound();
                    app.set_toast(format!("Audio alert enabled (≥{:.0}% Net ROI)", app.sound_min_roi_pct));
                } else {
                    app.set_toast("Audio alert muted");
                }
            }

            ui.add_space(4.0);

            // Auto-refresh checkbox toggle
            let auto_color = if app.auto_refresh {
                palette.accent_primary
            } else {
                palette.text_muted
            };
            let toggle_resp = ui.checkbox(
                &mut app.auto_refresh,
                RichText::new("Auto-Refresh (5m)")
                    .size(13.0)
                    .color(auto_color)
                    .strong(),
            );
            if toggle_resp.changed() {
                if app.auto_refresh {
                    app.last_scan_completed = Some(Instant::now());
                    app.set_toast("Auto-Refresh enabled (5-minute interval)");
                } else {
                    app.set_toast("Auto-Refresh disabled");
                }
            }

            // Auto-refresh countdown badge
            if app.auto_refresh {
                ui.add_space(2.0);
                let remaining_secs = if app.is_scanning {
                    app.auto_refresh_secs
                } else {
                    let elapsed = app
                        .last_scan_completed
                        .map(|t| t.elapsed().as_secs())
                        .unwrap_or(0);
                    app.auto_refresh_secs.saturating_sub(elapsed)
                };
                let mm = remaining_secs / 60;
                let ss = remaining_secs % 60;
                let badge_text = format!("⏱ {:02}:{:02}", mm, ss);

                egui::Frame::none()
                    .fill(palette.card_sub_bg)
                    .stroke(Stroke::new(1.0_f32, palette.accent_primary))
                    .rounding(4.0)
                    .inner_margin(egui::vec2(8.0, 5.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(badge_text)
                                .size(12.0)
                                .color(palette.accent_primary)
                                .monospace()
                                .strong(),
                        );
                    });
            }

            ui.add_space(4.0);

            // Primary Scan Button
            if app.is_scanning {
                let t = ctx.input(|i| i.time) as f32;
                let pulse = (t * 2.5).sin() * 0.5 + 0.5;
                let pulse_r = (210.0 + pulse * 45.0) as u8;
                let pulse_g = (155.0 + pulse * 50.0) as u8;
                ui.spinner();
                ui.label(
                    RichText::new("FETCHING ESI ORDERS...")
                        .size(14.0)
                        .color(Color32::from_rgb(pulse_r, pulse_g, 0))
                        .strong(),
                );
                ctx.request_repaint();
            } else {
                let scan_btn = egui::Button::new(
                    RichText::new("▶ SCAN MARKET NOW")
                        .size(14.5)
                        .color(Color32::BLACK)
                        .strong(),
                )
                .fill(Color32::from_rgb(0, 235, 170))
                .min_size(Vec2::new(170.0, 34.0));

                let btn_resp = ui.add(scan_btn);
                ui.painter().rect_stroke(
                    btn_resp.rect.expand(3.0),
                    egui::Rounding::same(7.0),
                    Stroke::new(2.0_f32, Color32::from_rgba_unmultiplied(0, 235, 160, 70)),
                );
                if btn_resp.clicked() {
                    app.last_scan_completed = Some(Instant::now());
                    app.start_scan();
                }
            }
        };

        if is_widescreen {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("// EVE TRAVELING MERCHANT")
                        .size(23.0)
                        .color(palette.accent_primary)
                        .strong(),
                );
                ui.label(
                    RichText::new("│ Arbitrage Engine & Multi-Stop Route Planner")
                        .size(13.0)
                        .color(palette.text_muted),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    render_primary_actions(self, ui, ctx);
                });
            });
        } else {
            // Compact / narrow window: title row first, then wrap-safe actions row
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("// EVE TRAVELING MERCHANT")
                        .size(20.0)
                        .color(palette.accent_primary)
                        .strong(),
                );
                ui.label(
                    RichText::new("│ Arbitrage Engine")
                        .size(12.0)
                        .color(palette.text_muted),
                );
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                render_primary_actions(self, ui, ctx);
            });
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        // Control parameters row
        ui.horizontal_wrapped(|ui| {
            // Ship Selector
            ui.label(RichText::new("SHIP PROFILE:").size(12.0).color(palette.text_secondary).strong());
            if self.use_custom_ship {
                let custom_label = format!("Custom ({:.0}k m³ · {:.0}M)", self.custom_cargo_vol / 1000.0, self.custom_budget_isk / 1_000_000.0);
                if ui.selectable_label(true, RichText::new(custom_label).color(palette.accent_primary).strong()).clicked() {
                    self.tab = ActiveTab::Settings;
                }
                if ui.small_button("Edit").on_hover_text("Configure ship cargo hold & anti-gank ISK limit in Settings").clicked() {
                    self.tab = ActiveTab::Settings;
                }
                if ui.small_button("↺ Presets").on_hover_text("Switch back to Standard Hauler presets").clicked() {
                    self.use_custom_ship = false;
                }
            } else {
                if ui.selectable_label(self.ship_is_big, RichText::new("Big Hauler (60k m³ · 500M)").strong()).clicked() {
                    self.ship_is_big = true;
                }
                if ui.selectable_label(!self.ship_is_big, RichText::new("Small Hauler (30k m³ · 50M)").strong()).clicked() {
                    self.ship_is_big = false;
                }
                if ui.small_button("Custom").on_hover_text("Configure custom cargo hold & anti-gank ISK limit in Settings").clicked() {
                    self.use_custom_ship = true;
                    self.tab = ActiveTab::Settings;
                }
            }

            ui.separator();

            // Home System Input
            ui.label(RichText::new("ORIGIN SYSTEM:").size(12.0).color(palette.text_secondary).strong());
            let input_width = if ui.available_width() < 900.0 { 110.0 } else { 135.0 };
            let text_edit_resp = ui.add(egui::TextEdit::singleline(&mut self.home_system).desired_width(input_width))
                .on_hover_text("Enter solar system (e.g. 'Amarr'), station name (e.g. 'Jita IV - Moon 4...'), or hub shorthand (e.g. 'Jita 4-4')\nPress Enter to immediately search this location.");
            if text_edit_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.start_scan();
            }

            // Quick hub chips — immediate 1-click switch & scan
            if ui.small_button("Jita").on_hover_text("Set origin to Jita (The Forge) & scan").clicked() {
                self.home_system = "Jita".to_string();
                self.start_scan();
            }
            if ui.small_button("Amarr").on_hover_text("Set origin to Amarr (Domain) & scan").clicked() {
                self.home_system = "Amarr".to_string();
                self.start_scan();
            }
            if ui.small_button("Dodixie").on_hover_text("Set origin to Dodixie (Sinq Laison) & scan").clicked() {
                self.home_system = "Dodixie".to_string();
                self.start_scan();
            }
            if ui.small_button("Rens").on_hover_text("Set origin to Rens (Heimatar) & scan").clicked() {
                self.home_system = "Rens".to_string();
                self.start_scan();
            }

            // Single dedicated "Check Location" button
            let loc_btn_label = if self.sso_is_fetching_location {
                "Locating..."
            } else {
                "Locate Me"
            };
            let loc_btn = egui::Button::new(
                RichText::new(loc_btn_label)
                    .size(11.5)
                    .color(if self.sso_character_name.is_some() {
                        Color32::from_rgb(0, 220, 255)
                    } else {
                        Color32::LIGHT_GRAY
                    })
                    .strong(),
            );
            if ui
                .add_enabled(!self.sso_is_fetching_location, loc_btn)
                .on_hover_text(if self.sso_character_name.is_some() {
                    "Query CCP ESI right now for your pilot's live in-game solar system, update Deals origin, and scan"
                } else {
                    "Connect your EVE character in Settings to enable 1-click in-game location detection"
                })
                .clicked()
            {
                if self.sso_character_name.is_none() {
                    self.set_toast("Please connect your character via EVE SSO in Settings first!");
                    self.tab = ActiveTab::Settings;
                } else {
                    self.fetch_live_character_location(true);
                }
            }

            // System detected status pill
            let home_id_opt = self.sde.as_ref().and_then(|s| s.get_system_id(&self.home_system));
            if let Some(hid) = home_id_opt {
                let sec = self.graph.as_ref().and_then(|g| g.security(hid)).unwrap_or(1.0);
                let sec_col = security_color(sec);
                let reg_name = self.sde.as_ref().and_then(|s| s.get_system_region(hid))
                    .and_then(|r| self.sde.as_ref().and_then(|s| s.get_region_name(r)))
                    .unwrap_or_else(|| "Unknown".to_string());
                let sys_name = self.sde.as_ref().and_then(|s| s.get_system_name(hid)).unwrap_or_default();
                let sync_tag = "";
                ui.label(
                    RichText::new(format!("[{:.1}] {} · {}{}", sec, sys_name, reg_name, sync_tag))
                        .size(11.0)
                        .color(sec_col)
                        .strong(),
                )
                .on_hover_text(format!("System: {} (ID: {})\nSecurity: {:.2}\nRegion: {}\nResolved successfully from: '{}'", sys_name, hid, sec, reg_name, self.home_system));

                if !sys_name.is_empty()
                    && self.home_system.trim().to_lowercase() != sys_name.to_lowercase()
                    && ui
                        .small_button(format!("Use {}", sys_name))
                        .on_hover_text(format!("Clean input to exact solar system name '{}' and scan", sys_name))
                        .clicked()
                {
                    self.home_system = sys_name;
                    self.start_scan();
                }
            } else {
                ui.label(
                    RichText::new("[?] Unknown System")
                        .size(11.0)
                        .color(Color32::from_rgb(255, 100, 100))
                        .strong(),
                );
            }

            ui.separator();

            // Adjacent Regions Toggle
            ui.checkbox(&mut self.include_adjacent_regions, RichText::new("+Adjacent Regions").strong())
                .on_hover_text("Automatically scans all stargate-connected neighbor regions of your starting location");
            let total_regions = self.selected_regions().len();
            ui.label(
                RichText::new(format!("({} reg)", total_regions))
                    .size(11.0)
                    .color(Color32::from_rgb(0, 210, 255)),
            );

            ui.separator();

            // Safe Highsec
            let safe_color = if self.safe_only { Color32::from_rgb(0, 230, 150) } else { Color32::from_rgb(255, 120, 120) };
            ui.checkbox(&mut self.safe_only, RichText::new("Safe Highsec Only").color(safe_color).strong());

            ui.separator();

            // Station Pool Toggle (NPC Stations Only vs Include Player Citadels)
            let (station_icon, station_text, station_color) = if self.npc_stations_only {
                ("[NPC]", "NPC Stations Only", Color32::from_rgb(0, 220, 255))
            } else {
                ("[CITADEL]", "+ Citadels Included", Color32::from_rgb(255, 185, 60))
            };

            let station_tip = if self.npc_stations_only {
                "Pool: Strictly NPC Stations only (public docking guaranteed).\nClick to switch: Include player-owned Citadels / Upwell structures."
            } else {
                "Pool: Includes player-owned Citadels & Upwell structures.\nClick to switch: Restrict strictly to NPC Stations only."
            };

            if ui.button(RichText::new(format!("{} {}", station_icon, station_text)).color(station_color).strong())
                .on_hover_text(station_tip)
                .clicked()
            {
                self.npc_stations_only = !self.npc_stations_only;
                let toast_msg = if self.npc_stations_only {
                    "Station Pool: NPC Stations Only"
                } else {
                    "Station Pool: Player Citadels Included"
                };
                self.set_toast(toast_msg);
            }

            ui.separator();

            // Min ROI %
            ui.label(RichText::new("MIN NET ROI:").size(12.0).color(palette.text_secondary).strong());
            ui.add(
                egui::DragValue::new(&mut self.min_roi_pct)
                    .speed(0.5)
                    .range(10.0..=100.0)
                    .suffix("%"),
            );
            ui.label(RichText::new("(≥10% Enforced)").size(11.0).color(Color32::GRAY));

            // Right-aligned status anchor on widescreen to prevent visual emptiness
            if ui.available_width() > 220.0 {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new("● Tranquility Live")
                            .size(11.0)
                            .color(Color32::from_rgb(0, 220, 150))
                            .strong(),
                    );
                    ui.label(
                        RichText::new("ESI v1")
                            .size(10.5)
                            .color(Color32::from_rgb(120, 150, 190)),
                    );
                });
            }
        });

        // Autocomplete suggestion strip when typing an incomplete system name
        let home_id_opt = self
            .sde
            .as_ref()
            .and_then(|s| s.get_system_id(&self.home_system));
        let trimmed_query = self.home_system.trim();
        if home_id_opt.is_none() && trimmed_query.len() >= 2 {
            let suggestions = self
                .sde
                .as_ref()
                .map(|s| s.search_systems(trimmed_query, 6))
                .unwrap_or_default();
            if !suggestions.is_empty() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Autocomplete Matches:")
                            .size(11.0)
                            .color(Color32::from_rgb(255, 210, 0))
                            .strong(),
                    );
                    for s in suggestions {
                        let sec_col = security_color(s.security_status);
                        let chip_text = format!(
                            "{} [{:.1}] ({})",
                            s.system_name, s.security_status, s.region_name
                        );
                        if ui
                            .button(RichText::new(chip_text).size(11.0).color(sec_col))
                            .on_hover_text("Click to set as starting system & scan")
                            .clicked()
                        {
                            self.home_system = s.system_name;
                            self.start_scan();
                        }
                    }
                });
            }
        }

        ui.add_space(8.0);
        ui.separator();

        // Executive KPI Badge Bar — adaptive compact pills or three glowing metric cards
        self.render_kpi_bar(ui);
        ui.add_space(8.0);

        // Navigation Tab Bar with glowing underline indicator
        self.render_tab_bar(ui);
    }

    fn render_kpi_bar(&mut self, ui: &mut egui::Ui) {
        let palette = self.theme.palette();
        let top_profit = self.deals.first().map(|d| d.net_profit).unwrap_or(0.0);
        let top_ppj = self.deals.first().map(|d| d.profit_per_jump).unwrap_or(0.0);
        let deal_count = self.deals.len();
        let top_roi = self
            .deals
            .iter()
            .map(|d| d.net_profit_pct)
            .fold(0.0_f64, f64::max);
        let top_density = self
            .deals
            .iter()
            .map(|d| d.profit_per_m3)
            .fold(0.0_f64, f64::max);
        let current_ship = self.current_ship();
        let ship_name = if self.use_custom_ship {
            format!("Custom ({:.0}k m³)", current_ship.max_volume / 1000.0)
        } else if self.ship_is_big {
            "Big Hauler (60k m³)".to_string()
        } else {
            "Small Hauler (30k m³)".to_string()
        };

        let avail_w = ui.available_width();
        let is_compact = self.compact_kpi || avail_w < 950.0;
        let is_widescreen = avail_w >= 1350.0;

        if is_compact {
            ui.horizontal_wrapped(|ui| {
                // Pill 1: Best Profit
                egui::Frame::none()
                    .fill(palette.kpi_card_bg)
                    .stroke(Stroke::new(1.0_f32, palette.success_green))
                    .rounding(4.0)
                    .inner_margin(egui::vec2(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("TOP PROFIT:")
                                .size(10.5)
                                .color(palette.text_muted)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(format!("+{:.2}M ISK", top_profit / 1_000_000.0))
                                .size(12.5)
                                .color(palette.success_green)
                                .strong(),
                        );
                    });

                // Pill 2: Top Profit/Jump
                egui::Frame::none()
                    .fill(palette.kpi_card_bg)
                    .stroke(Stroke::new(1.0_f32, palette.accent_secondary))
                    .rounding(4.0)
                    .inner_margin(egui::vec2(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("PROFIT / JUMP:")
                                .size(10.5)
                                .color(palette.text_muted)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(format!("{:.2}M / jump", top_ppj / 1_000_000.0))
                                .size(12.5)
                                .color(palette.accent_secondary)
                                .strong(),
                        );
                    });

                // Pill 3: Max ROI
                egui::Frame::none()
                    .fill(palette.kpi_card_bg)
                    .stroke(Stroke::new(1.0_f32, palette.warning_yellow))
                    .rounding(4.0)
                    .inner_margin(egui::vec2(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("MAX ROI:")
                                .size(10.5)
                                .color(palette.text_muted)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(format!("+{:.1}%", top_roi))
                                .size(12.5)
                                .color(palette.warning_yellow)
                                .strong(),
                        );
                    });

                // Pill 4: Density
                egui::Frame::none()
                    .fill(palette.kpi_card_bg)
                    .stroke(Stroke::new(1.0_f32, palette.info_blue))
                    .rounding(4.0)
                    .inner_margin(egui::vec2(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("DENSITY:")
                                .size(10.5)
                                .color(palette.text_muted)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(format!("{} ISK/m³", format_rate_isk(top_density)))
                                .size(12.5)
                                .color(palette.info_blue)
                                .strong(),
                        );
                    });

                // Pill 5: Deals Count
                egui::Frame::none()
                    .fill(palette.kpi_card_bg)
                    .stroke(Stroke::new(1.0_f32, palette.accent_primary))
                    .rounding(4.0)
                    .inner_margin(egui::vec2(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("DEALS:")
                                .size(10.5)
                                .color(palette.text_muted)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(format!("{} profitable", deal_count))
                                .size(12.5)
                                .color(palette.accent_primary)
                                .strong(),
                        );
                    });

                // Pill 6: Fleet
                egui::Frame::none()
                    .fill(palette.kpi_card_bg)
                    .stroke(palette.card_sub_stroke)
                    .rounding(4.0)
                    .inner_margin(egui::vec2(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("FLEET:")
                                .size(10.5)
                                .color(palette.text_muted)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(ship_name)
                                .size(12.5)
                                .color(palette.text_primary)
                                .strong(),
                        );
                    });

                let toggle_label = if self.compact_kpi {
                    "▼ Expand KPIs"
                } else {
                    "▲ Compact KPIs"
                };
                if ui
                    .small_button(toggle_label)
                    .on_hover_text("Toggle between compact bar and full KPI cards")
                    .clicked()
                {
                    self.compact_kpi = !self.compact_kpi;
                }
            });
        } else if is_widescreen {
            // 6-column fluid metric grid for widescreen / fullscreen displays
            ui.columns(6, |cols| {
                // Card 1: Best Profit
                egui::Frame::none()
                    .fill(palette.kpi_card_bg)
                    .stroke(Stroke::new(1.0_f32, palette.success_green))
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(&mut cols[0], |ui| {
                        ui.label(
                            RichText::new("TOP PROFIT")
                                .size(10.0)
                                .color(palette.text_muted),
                        );
                        ui.label(
                            RichText::new(format!("+{:.2}M ISK", top_profit / 1_000_000.0))
                                .size(18.0)
                                .color(palette.success_green)
                                .strong(),
                        );
                    });

                // Card 2: Top Profit/Jump
                egui::Frame::none()
                    .fill(palette.kpi_card_bg)
                    .stroke(Stroke::new(1.0_f32, palette.accent_secondary))
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(&mut cols[1], |ui| {
                        ui.label(
                            RichText::new("TOP PROFIT / JUMP")
                                .size(10.0)
                                .color(palette.text_muted),
                        );
                        ui.label(
                            RichText::new(format!("{:.2}M / jump", top_ppj / 1_000_000.0))
                                .size(18.0)
                                .color(palette.accent_secondary)
                                .strong(),
                        );
                    });

                // Card 3: Max Net ROI
                egui::Frame::none()
                    .fill(palette.kpi_card_bg)
                    .stroke(Stroke::new(1.0_f32, palette.warning_yellow))
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(&mut cols[2], |ui| {
                        ui.label(
                            RichText::new("PEAK NET ROI")
                                .size(10.0)
                                .color(palette.text_muted),
                        );
                        ui.label(
                            RichText::new(format!("+{:.1}%", top_roi))
                                .size(18.0)
                                .color(palette.warning_yellow)
                                .strong(),
                        );
                    });

                // Card 4: Profit Density
                egui::Frame::none()
                    .fill(palette.kpi_card_bg)
                    .stroke(Stroke::new(1.0_f32, palette.info_blue))
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(&mut cols[3], |ui| {
                        ui.label(
                            RichText::new("PROFIT DENSITY")
                                .size(10.0)
                                .color(palette.text_muted),
                        );
                        ui.label(
                            RichText::new(format!("{} ISK/m³", format_rate_isk(top_density)))
                                .size(18.0)
                                .color(palette.info_blue)
                                .strong(),
                        );
                    });

                // Card 5: Deals Discovered
                egui::Frame::none()
                    .fill(palette.kpi_card_bg)
                    .stroke(Stroke::new(1.0_f32, palette.accent_primary))
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(&mut cols[4], |ui| {
                        ui.label(
                            RichText::new("ACTIVE DEALS")
                                .size(10.0)
                                .color(palette.text_muted),
                        );
                        ui.label(
                            RichText::new(format!("{} profitable", deal_count))
                                .size(18.0)
                                .color(palette.accent_primary)
                                .strong(),
                        );
                    });

                // Card 6: Ship & Safety with collapse button
                egui::Frame::none()
                    .fill(palette.kpi_card_bg)
                    .stroke(palette.card_sub_stroke)
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(&mut cols[5], |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("FLEET PROFILE")
                                    .size(10.0)
                                    .color(palette.text_muted),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .small_button("▲")
                                        .on_hover_text("Collapse to compact KPI strip")
                                        .clicked()
                                    {
                                        self.compact_kpi = true;
                                    }
                                },
                            );
                        });
                        ui.label(
                            RichText::new(ship_name)
                                .size(15.0)
                                .color(palette.text_primary)
                                .strong(),
                        );
                    });
            });
        } else {
            ui.columns(3, |cols| {
                // Badge 1: Best Profit (emerald green)
                egui::Frame::none()
                    .fill(palette.kpi_card_bg)
                    .stroke(Stroke::new(1.0_f32, palette.success_green))
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(&mut cols[0], |ui| {
                        ui.label(
                            RichText::new("TOP PROFIT")
                                .size(10.0)
                                .color(palette.text_muted),
                        );
                        ui.label(
                            RichText::new(format!("+{:.2}M ISK", top_profit / 1_000_000.0))
                                .size(20.0)
                                .color(palette.success_green)
                                .strong(),
                        );
                    });

                // Badge 2: Top Profit/Jump (secondary accent)
                egui::Frame::none()
                    .fill(palette.kpi_card_bg)
                    .stroke(Stroke::new(1.0_f32, palette.accent_secondary))
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(&mut cols[1], |ui| {
                        ui.label(
                            RichText::new("TOP PROFIT / JUMP")
                                .size(10.0)
                                .color(palette.text_muted),
                        );
                        ui.label(
                            RichText::new(format!("{:.2}M / jump", top_ppj / 1_000_000.0))
                                .size(20.0)
                                .color(palette.accent_secondary)
                                .strong(),
                        );
                    });

                // Badge 3: Deals found (primary accent) with collapse button
                egui::Frame::none()
                    .fill(palette.kpi_card_bg)
                    .stroke(Stroke::new(1.0_f32, palette.accent_primary))
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(&mut cols[2], |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("DEALS DISCOVERED")
                                    .size(10.0)
                                    .color(palette.text_muted),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .small_button("▲")
                                        .on_hover_text("Collapse to compact KPI strip")
                                        .clicked()
                                    {
                                        self.compact_kpi = true;
                                    }
                                },
                            );
                        });
                        ui.label(
                            RichText::new(format!("{} profitable", deal_count))
                                .size(20.0)
                                .color(palette.accent_primary)
                                .strong(),
                        );
                    });
            });
        }
    }

    fn render_tab_bar(&mut self, ui: &mut egui::Ui) {
        let hub_count = crate::hub_arbitrage::filter_hub_deals(
            &self.deals,
            &crate::hub_arbitrage::HubFilterOptions {
                source_hub: self.hub_source_filter,
                dest_hub: self.hub_dest_filter,
                include_npc: self.hub_include_npc,
                major_stations_only: self.hub_major_stations_only,
                min_profit: self.hub_min_profit_m * 1_000_000.0,
                search_query: self.hub_search_query.clone(),
            },
        )
        .len();

        let palette = self.theme.palette();

        ui.horizontal(|ui| {
            let tabs: &[(ActiveTab, String)] = &[
                (ActiveTab::Deals, format!("Deals ({})", self.deals.len())),
                (
                    ActiveTab::Manifests,
                    format!("Manifests ({})", self.manifests.len()),
                ),
                (
                    ActiveTab::Route,
                    format!(
                        "Route ({})",
                        self.route.as_ref().map(|r| r.stops.len()).unwrap_or(0)
                    ),
                ),
                (
                    ActiveTab::ChainRoute,
                    format!("Chain Routes ({})", self.chain_results.len()),
                ),
                (
                    ActiveTab::HubArbitrage,
                    format!("[NPC] Hub Arbitrage ({})", hub_count),
                ),
                (ActiveTab::Settings, "Settings & Fees".to_string()),
            ];

            let mut new_tab: Option<ActiveTab> = None;

            for (tab, label) in tabs {
                let is_active = self.tab == *tab;
                let text_col = if is_active {
                    palette.tab_active_text
                } else {
                    palette.tab_inactive_text
                };
                let fill = if is_active {
                    palette.tab_active_fill
                } else {
                    Color32::TRANSPARENT
                };

                let resp = ui.add(
                    egui::Button::new(
                        RichText::new(label.as_str())
                            .size(13.5)
                            .color(text_col)
                            .strong(),
                    )
                    .fill(fill)
                    .min_size(Vec2::new(0.0, 30.0)),
                );

                // Glowing underline beneath active tab
                if is_active {
                    let painter = ui.painter();
                    let y = resp.rect.bottom() - 1.0;
                    // Outer glow
                    painter.line_segment(
                        [
                            egui::pos2(resp.rect.left() - 1.0, y),
                            egui::pos2(resp.rect.right() + 1.0, y),
                        ],
                        Stroke::new(
                            4.0_f32,
                            Color32::from_rgba_unmultiplied(
                                palette.accent_primary.r(),
                                palette.accent_primary.g(),
                                palette.accent_primary.b(),
                                45,
                            ),
                        ),
                    );
                    // Bright core
                    painter.line_segment(
                        [
                            egui::pos2(resp.rect.left(), y),
                            egui::pos2(resp.rect.right(), y),
                        ],
                        Stroke::new(2.0_f32, palette.accent_primary),
                    );
                }

                if resp.clicked() {
                    new_tab = Some(*tab);
                }
            }

            if let Some(t) = new_tab {
                self.tab = t;
            }
        });
    }

    fn render_deals_tab(&mut self, ui: &mut egui::Ui) {
        let palette = self.theme.palette();
        if self.deals.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.heading(
                    RichText::new("No Deals Available")
                        .size(22.0)
                        .color(palette.text_secondary),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Click the '▶ SCAN MARKET NOW' button above to fetch live orders from CCP's ESI API.")
                        .size(14.0)
                        .color(palette.text_muted),
                );
                ui.add_space(16.0);
                let btn = egui::Button::new(
                    RichText::new("▶ Start First Market Scan")
                        .size(16.0)
                        .color(Color32::BLACK)
                        .strong(),
                )
                .fill(Color32::from_rgb(0, 220, 170))
                .min_size(Vec2::new(240.0, 42.0));

                if ui.add(btn).clicked() {
                    self.start_scan();
                }
            });
            return;
        }

        // Search & Filter Toolbar
        palette.card_frame().inner_margin(8.0).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Search:").strong());
                    let search_w = if ui.available_width() < 900.0 { 130.0 } else { 180.0 };
                    ui.add(egui::TextEdit::singleline(&mut self.filter_item_name).desired_width(search_w))
                        .on_hover_text("Search by item name, station, or system\nSupports Roman numerals ('Jita IV') and Arabic numbers ('Jita 4')");
                    if !self.filter_item_name.is_empty() && ui.button("✕ Clear").clicked() {
                        self.filter_item_name.clear();
                    }

                    ui.separator();

                    ui.label(RichText::new("Sort:").strong());
                    egui::ComboBox::from_id_source("deal_sort_combo")
                        .selected_text(self.sort_by.display_name())
                        .width(135.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.sort_by, DealSortBy::ProfitPerJump, "Profit / Jump");
                            ui.selectable_value(&mut self.sort_by, DealSortBy::TotalProfit, "Total Net Profit");
                            ui.selectable_value(&mut self.sort_by, DealSortBy::RoiPct, "Net ROI %");
                            ui.selectable_value(&mut self.sort_by, DealSortBy::ProfitPerM3, "Profit / m³");
                            ui.selectable_value(&mut self.sort_by, DealSortBy::ItemName, "Item Name (A-Z)");
                            ui.selectable_value(&mut self.sort_by, DealSortBy::BuySystem, "Buy System");
                            ui.selectable_value(&mut self.sort_by, DealSortBy::BuyPrice, "Buy Price");
                            ui.selectable_value(&mut self.sort_by, DealSortBy::SellSystem, "Sell System");
                            ui.selectable_value(&mut self.sort_by, DealSortBy::SellPrice, "Sell Price");
                            ui.selectable_value(&mut self.sort_by, DealSortBy::Quantity, "Quantity");
                            ui.selectable_value(&mut self.sort_by, DealSortBy::Volume, "Volume (m³)");
                            ui.selectable_value(&mut self.sort_by, DealSortBy::Cost, "Lowest Cost");
                            ui.selectable_value(&mut self.sort_by, DealSortBy::Jumps, "Fewest Jumps");
                        });

                    let sort_dir_btn = if self.sort_descending { "▼ Desc" } else { "▲ Asc" };
                    if ui.button(sort_dir_btn).on_hover_text("Toggle sort order (Ascending / Descending)").clicked() {
                        self.sort_descending = !self.sort_descending;
                    }

                    ui.separator();

                    // Min Profit Filter
                    ui.label(RichText::new("Min Profit (M):").strong());
                    ui.add(
                        egui::DragValue::new(&mut self.min_profit_filter_m)
                            .speed(0.5)
                            .range(0.0..=100.0)
                            .suffix("M"),
                    );

                    ui.separator();

                    // Max Jumps Filter
                    ui.label(RichText::new("Max Jumps:").strong());
                    ui.add(
                        egui::DragValue::new(&mut self.max_jumps_filter)
                            .speed(1)
                            .range(0..=60),
                    );

                    ui.separator();

                    // NPC Trade Deals Toggle Filter Button
                    let (npc_icon, npc_label, npc_fill, npc_stroke, npc_text_col) = if self.hide_npc_trades {
                        ("", "Hide NPC Trades: ON", Color32::from_rgb(180, 50, 40), Color32::from_rgb(255, 90, 80), Color32::WHITE)
                    } else {
                        ("[NPC]", "NPC Trades: Visible", Color32::from_rgba_unmultiplied(20, 36, 60, 200), Color32::from_rgb(60, 110, 170), Color32::from_rgb(160, 210, 255))
                    };

                    let npc_btn = egui::Button::new(
                        RichText::new(format!("{} {}", npc_icon, npc_label))
                            .size(12.0)
                            .color(npc_text_col)
                            .strong(),
                    )
                    .fill(npc_fill)
                    .stroke(Stroke::new(1.0_f32, npc_stroke));

                    if ui.add(npc_btn)
                        .on_hover_text("Toggle to filter out NPC trade deals (seeded commodities like Spirits, Dairy Products, Wheat, Livestock, Antibiotics, and 365-day NPC orders).\n• Active (ON): Hides NPC trade commodity deals\n• Inactive (OFF): Shows all profitable deals")
                        .clicked()
                    {
                        self.hide_npc_trades = !self.hide_npc_trades;
                        let toast_msg = if self.hide_npc_trades {
                            "Filter: Hiding NPC trade deals"
                        } else {
                            "Filter: Showing all trade deals (including NPC)"
                        };
                        self.set_toast(toast_msg);
                    }

                    ui.separator();

                    // Max Deals Per Order Toggle (Order Diversity / Filler Pruning)
                    let (cap_icon, cap_label, cap_fill, cap_stroke, cap_text_col) =
                        if self.max_deals_per_order > 0 {
                            (
                                "",
                                format!("Top {}/Order: ON", self.max_deals_per_order),
                                Color32::from_rgba_unmultiplied(16, 44, 60, 220),
                                Color32::from_rgb(0, 200, 255),
                                Color32::from_rgb(0, 220, 255),
                            )
                        } else {
                            (
                                "",
                                "Max/Order: ALL".to_string(),
                                Color32::from_rgba_unmultiplied(25, 30, 45, 180),
                                Color32::from_rgb(50, 70, 95),
                                Color32::from_rgb(140, 160, 190),
                            )
                        };

                    let cap_btn = egui::Button::new(
                        RichText::new(format!("{} {}", cap_icon, cap_label))
                            .size(12.0)
                            .color(cap_text_col)
                            .strong(),
                    )
                    .fill(cap_fill)
                    .stroke(Stroke::new(1.0_f32, cap_stroke));

                    if ui
                        .add(cap_btn)
                        .on_hover_text(
                            "Toggle order diversity filter to prune redundant filler deals:\n• ON: Limits each sell order to top N destinations & each buy order to top N sources\n• Prioritizes fastest, closest, and highest profit-per-jump routes from starting location\n• ALL: Displays all raw order permutations",
                        )
                        .clicked()
                    {
                        if self.max_deals_per_order > 0 {
                            self.max_deals_per_order = 0;
                            self.set_toast("Order Diversity: Showing all order pairs (unlimited)");
                        } else {
                            self.max_deals_per_order = 2;
                            self.set_toast("Order Diversity: Limiting to top 2 routes per order");
                        }
                    }

                    ui.add(
                        egui::DragValue::new(&mut self.max_deals_per_order)
                            .speed(0.1)
                            .range(0..=10)
                            .suffix("/ord"),
                    )
                    .on_hover_text("Max destinations per sell order and sources per buy order (0 = unlimited / all pairs)");

                    ui.separator();
                    let export_btn = egui::Button::new(
                        RichText::new("Export Deals to CSV")
                            .color(Color32::from_rgb(0, 220, 255))
                            .strong(),
                    );
                    if ui.add(export_btn).on_hover_text("Export all currently filtered deals to ./deals_export.csv and copy to clipboard").clicked() {
                        let filter_inner = self.filter_item_name.trim().to_lowercase();
                        let filter_sde_inner = crate::sde::station_name_to_sde(&filter_inner).to_lowercase();
                        let min_prof = self.min_profit_filter_m * 1_000_000.0;
                        let max_j = self.max_jumps_filter;
                        let hide_npc = self.hide_npc_trades;

                        let export_filtered: Vec<(usize, &ArbitrageDeal)> = self
                            .deals
                            .iter()
                            .enumerate()
                            .filter(|(_, d)| {
                                if hide_npc && d.is_npc_trade {
                                    return false;
                                }
                                (filter_inner.is_empty()
                                    || d.item_name.to_lowercase().contains(&filter_inner)
                                    || d.source_system_name.to_lowercase().contains(&filter_inner)
                                    || d.dest_system_name.to_lowercase().contains(&filter_inner)
                                    || d.source_station_name.to_lowercase().contains(&filter_inner)
                                    || d.source_station_name.to_lowercase().contains(&filter_sde_inner)
                                    || d.dest_station_name.to_lowercase().contains(&filter_inner)
                                    || d.dest_station_name.to_lowercase().contains(&filter_sde_inner))
                                    && d.net_profit >= min_prof
                                    && (max_j == 0 || d.total_jumps <= max_j)
                            })
                            .collect();

                        let export_filtered = if self.max_deals_per_order > 0 {
                            filter_deals_by_order_limit(
                                export_filtered,
                                self.max_deals_per_order,
                                self.max_deals_per_order,
                                |(_, d)| d,
                            )
                        } else {
                            export_filtered
                        };

                        let csv_str = export_deals_to_csv(&export_filtered);
                        copy_to_clipboard(ui, csv_str.clone());
                        let export_path = resolve_csv_export_path("deals_export.csv");
                        match safe_write_csv(&export_path, &csv_str) {
                            Ok(()) => {
                                self.set_toast(format!(
                                    "Exported {} deals to {} & copied to clipboard!",
                                    export_filtered.len(),
                                    export_path.display()
                                ));
                            }
                            Err(e) => {
                                tracing::error!(
                                    target: "eve_trade_scout",
                                    error_code = %crate::error::ErrorCode::ErrUiCsvExport,
                                    path = %export_path.display(),
                                    error = %e,
                                    "Failed to write CSV deals export to disk: {e}"
                                );
                                self.set_toast(format!("Failed to export CSV: {e}"));
                            }
                        }
                    }
                });
            });

        ui.add_space(8.0);

        // Filter and sort candidate deals with memoization across 60 FPS renders
        let current_key = (
            self.filter_item_name.clone(),
            self.min_profit_filter_m.to_bits(),
            self.max_jumps_filter,
            self.hide_npc_trades,
            self.max_deals_per_order,
            self.sort_by,
            self.sort_descending,
            self.deals.len(),
        );

        if self.cached_deals_key != current_key {
            let filter = self.filter_item_name.trim().to_lowercase();
            let filter_sde = crate::sde::station_name_to_sde(&filter).to_lowercase();
            let min_profit_threshold = self.min_profit_filter_m * 1_000_000.0;
            let max_jumps = self.max_jumps_filter;
            let hide_npc = self.hide_npc_trades;

            let mut filtered: Vec<(usize, &ArbitrageDeal)> = self
                .deals
                .iter()
                .enumerate()
                .filter(|(_, d)| {
                    if hide_npc && d.is_npc_trade {
                        return false;
                    }
                    (filter.is_empty()
                        || d.item_name.to_lowercase().contains(&filter)
                        || d.source_system_name.to_lowercase().contains(&filter)
                        || d.dest_system_name.to_lowercase().contains(&filter)
                        || d.source_station_name.to_lowercase().contains(&filter)
                        || d.source_station_name.to_lowercase().contains(&filter_sde)
                        || d.dest_station_name.to_lowercase().contains(&filter)
                        || d.dest_station_name.to_lowercase().contains(&filter_sde))
                        && d.net_profit >= min_profit_threshold
                        && (max_jumps == 0 || d.total_jumps <= max_jumps)
                })
                .collect();

            if self.max_deals_per_order > 0 {
                filtered = filter_deals_by_order_limit(
                    filtered,
                    self.max_deals_per_order,
                    self.max_deals_per_order,
                    |(_, d)| d,
                );
            }

            match self.sort_by {
                DealSortBy::ProfitPerJump => {
                    filtered.sort_by(|(_, a), (_, b)| {
                        if self.sort_descending {
                            b.profit_per_jump
                                .partial_cmp(&a.profit_per_jump)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        } else {
                            a.profit_per_jump
                                .partial_cmp(&b.profit_per_jump)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        }
                    });
                }
                DealSortBy::TotalProfit => {
                    filtered.sort_by(|(_, a), (_, b)| {
                        if self.sort_descending {
                            b.net_profit
                                .partial_cmp(&a.net_profit)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        } else {
                            a.net_profit
                                .partial_cmp(&b.net_profit)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        }
                    });
                }
                DealSortBy::RoiPct => {
                    filtered.sort_by(|(_, a), (_, b)| {
                        if self.sort_descending {
                            b.net_profit_pct
                                .partial_cmp(&a.net_profit_pct)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        } else {
                            a.net_profit_pct
                                .partial_cmp(&b.net_profit_pct)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        }
                    });
                }
                DealSortBy::ProfitPerM3 => {
                    filtered.sort_by(|(_, a), (_, b)| {
                        if self.sort_descending {
                            b.profit_per_m3
                                .partial_cmp(&a.profit_per_m3)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        } else {
                            a.profit_per_m3
                                .partial_cmp(&b.profit_per_m3)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        }
                    });
                }
                DealSortBy::Jumps => {
                    filtered.sort_by(|(_, a), (_, b)| {
                        if self.sort_descending {
                            b.total_jumps.cmp(&a.total_jumps)
                        } else {
                            a.total_jumps.cmp(&b.total_jumps)
                        }
                    });
                }
                DealSortBy::Cost => {
                    filtered.sort_by(|(_, a), (_, b)| {
                        if self.sort_descending {
                            b.capital_required
                                .partial_cmp(&a.capital_required)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        } else {
                            a.capital_required
                                .partial_cmp(&b.capital_required)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        }
                    });
                }
                DealSortBy::ItemName => {
                    filtered.sort_by(|(_, a), (_, b)| {
                        if self.sort_descending {
                            b.item_name.to_lowercase().cmp(&a.item_name.to_lowercase())
                        } else {
                            a.item_name.to_lowercase().cmp(&b.item_name.to_lowercase())
                        }
                    });
                }
                DealSortBy::BuySystem => {
                    filtered.sort_by(|(_, a), (_, b)| {
                        if self.sort_descending {
                            b.source_system_name
                                .to_lowercase()
                                .cmp(&a.source_system_name.to_lowercase())
                        } else {
                            a.source_system_name
                                .to_lowercase()
                                .cmp(&b.source_system_name.to_lowercase())
                        }
                    });
                }
                DealSortBy::BuyPrice => {
                    filtered.sort_by(|(_, a), (_, b)| {
                        if self.sort_descending {
                            b.buy_price
                                .partial_cmp(&a.buy_price)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        } else {
                            a.buy_price
                                .partial_cmp(&b.buy_price)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        }
                    });
                }
                DealSortBy::SellSystem => {
                    filtered.sort_by(|(_, a), (_, b)| {
                        if self.sort_descending {
                            b.dest_system_name
                                .to_lowercase()
                                .cmp(&a.dest_system_name.to_lowercase())
                        } else {
                            a.dest_system_name
                                .to_lowercase()
                                .cmp(&b.dest_system_name.to_lowercase())
                        }
                    });
                }
                DealSortBy::SellPrice => {
                    filtered.sort_by(|(_, a), (_, b)| {
                        if self.sort_descending {
                            b.sell_price
                                .partial_cmp(&a.sell_price)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        } else {
                            a.sell_price
                                .partial_cmp(&b.sell_price)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        }
                    });
                }
                DealSortBy::Quantity => {
                    filtered.sort_by(|(_, a), (_, b)| {
                        if self.sort_descending {
                            b.quantity.cmp(&a.quantity)
                        } else {
                            a.quantity.cmp(&b.quantity)
                        }
                    });
                }
                DealSortBy::Volume => {
                    filtered.sort_by(|(_, a), (_, b)| {
                        if self.sort_descending {
                            b.volume_used
                                .partial_cmp(&a.volume_used)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        } else {
                            a.volume_used
                                .partial_cmp(&b.volume_used)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        }
                    });
                }
            }

            self.cached_deals_filtered = filtered.into_iter().map(|(idx, _)| idx).collect();
            self.cached_deals_key = current_key;
        }

        let filtered: Vec<(usize, &ArbitrageDeal)> = self
            .cached_deals_filtered
            .iter()
            .map(|&idx| (idx, &self.deals[idx]))
            .collect();

        let mut top_bar_push: Option<(Vec<u64>, String)> = None;
        let mut top_bar_toast: Option<String> = None;
        let mut top_bar_switch_settings = false;

        // EVE Autopilot Quick Action Bar
        palette.card_frame().inner_margin(8.0).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if let Some(ref char_name) = self.sso_character_name {
                        ui.label(
                            RichText::new(format!("● Connected: {}", char_name))
                                .size(12.5)
                                .color(Color32::from_rgb(0, 255, 140))
                                .strong(),
                        );
                    } else {
                        ui.label(
                            RichText::new("○ Pilot: Disconnected")
                                .size(12.5)
                                .color(Color32::GRAY),
                        );
                        if ui.small_button("Connect in Settings").clicked() {
                            top_bar_switch_settings = true;
                        }
                    }
                    ui.separator();

                    let target_deal = self.selected_deal_idx
                        .and_then(|idx| self.deals.get(idx))
                        .or_else(|| filtered.first().map(|(_, d)| *d));

                    if let Some(deal) = target_deal {
                        let btn_label = if self.selected_deal_idx.is_some() {
                            format!("▶ Push Selected Deal to Autopilot ({})", deal.item_name)
                        } else {
                            format!("▶ Push Top Deal to Autopilot (#1 {})", deal.item_name)
                        };
                        let push_btn = egui::Button::new(
                            RichText::new(btn_label)
                                .size(12.0)
                                .color(Color32::BLACK)
                                .strong(),
                        )
                        .fill(Color32::from_rgb(0, 220, 255));

                        if ui
                            .add(push_btn)
                            .on_hover_text(format!(
                                "Inject this deal's buy & sell waypoints straight into your running EVE client HUD:\n1. Buy at: {}\n2. Sell at: {}",
                                deal.source_station_name, deal.dest_station_name
                            ))
                            .clicked()
                        {
                            top_bar_push = Some((deal.ordered_destination_ids(), format!("Deal: {}", deal.item_name)));
                        }

                        let sheet_btn = egui::Button::new(
                            RichText::new("Copy Flight Sheet (Alt+N)")
                                .size(12.0)
                                .color(Color32::BLACK)
                                .strong(),
                        )
                        .fill(Color32::from_rgb(255, 205, 0));

                        if ui
                            .add(sheet_btn)
                            .on_hover_text("Copy formatted trade sheet for in-game Notepad (Alt+N)")
                            .clicked()
                        {
                            let sheet = deal.format_eve_flight_sheet();
                            copy_to_clipboard(ui, sheet);
                            top_bar_toast = Some(format!("Copied flight sheet for '{}' to clipboard!", deal.item_name));
                        }
                    }

                    if let Some(ref route) = self.route {
                        if !route.stops.is_empty() {
                            ui.separator();
                            let route_btn = egui::Button::new(
                                RichText::new(format!("▶ Push Planned Route ({} Stops) to EVE", route.stops.len()))
                                    .size(12.0)
                                    .color(Color32::BLACK)
                                    .strong()
                            )
                            .fill(Color32::from_rgb(0, 255, 180));

                            if ui
                                .add(route_btn)
                                .on_hover_text("Directly inject all planned multi-hop route waypoints into your live EVE client Autopilot HUD")
                                .clicked()
                            {
                                top_bar_push = Some((route.ordered_destination_ids(), "Planned Route".to_string()));
                            }
                        }
                    }
                });
            });

        ui.add_space(6.0);

        // Responsive Layout: Choose between Side Panel (on wide screens) and Bottom Drawer (compact screens)
        let avail_w = ui.available_width();
        let avail_h = ui.available_height();
        let has_selection = self.selected_deal_idx.is_some();
        let use_side_panel = has_selection && self.dossier_side_panel && avail_w >= 1150.0;

        if use_side_panel {
            let side_w = (avail_w * 0.36).clamp(380.0, 680.0);
            let table_w = (avail_w - side_w - 10.0).max(300.0);

            let mut grid_res = DealsGridAction::default();
            let mut dossier_res = (false, false, None, None);

            ui.horizontal(|ui| {
                ui.allocate_ui(Vec2::new(table_w, avail_h), |ui| {
                    grid_res = self.render_deals_table_grid(ui, &filtered);
                });
                ui.allocate_ui(Vec2::new(side_w, avail_h), |ui| {
                    if let Some(idx) = self.selected_deal_idx {
                        if let Some(deal) = self.deals.get(idx) {
                            dossier_res = self.render_deal_dossier(ui, deal, true);
                        }
                    }
                });
            });

            let (close_req, toggle_dock, d_toast, push_dest_ids) = dossier_res;
            if close_req {
                self.selected_deal_idx = None;
            }
            if toggle_dock {
                self.dossier_side_panel = !self.dossier_side_panel;
                if self.dossier_side_panel {
                    self.set_toast("Dossier docked to right side panel");
                } else {
                    self.set_toast("Dossier docked to bottom drawer");
                }
            }
            if let Some(msg) = d_toast {
                self.set_toast(msg);
            }
            if let Some(dest_ids) = push_dest_ids {
                self.push_to_eve_autopilot(dest_ids, "Deal Route");
            }

            if let Some(dest_ids) = grid_res.direct_push {
                self.push_to_eve_autopilot(dest_ids, "Deal Route");
            }
            if let Some(idx) = grid_res.toggle_select {
                if self.selected_deal_idx == Some(idx) {
                    self.selected_deal_idx = None;
                } else {
                    self.selected_deal_idx = Some(idx);
                }
            }
            if let Some(msg) = grid_res.action_toast {
                self.set_toast(msg);
            }
            if let Some((new_sort, new_desc)) = grid_res.change_sort {
                self.sort_by = new_sort;
                self.sort_descending = new_desc;
            }
        } else {
            let table_height = if has_selection {
                (avail_h * 0.58).max(180.0)
            } else {
                avail_h
            };

            let mut grid_res = DealsGridAction::default();
            ui.allocate_ui(Vec2::new(avail_w, table_height), |ui| {
                grid_res = self.render_deals_table_grid(ui, &filtered);
            });

            if let Some(dest_ids) = grid_res.direct_push {
                self.push_to_eve_autopilot(dest_ids, "Deal Route");
            }
            if let Some(idx) = grid_res.toggle_select {
                if self.selected_deal_idx == Some(idx) {
                    self.selected_deal_idx = None;
                } else {
                    self.selected_deal_idx = Some(idx);
                }
            }
            if let Some(msg) = grid_res.action_toast {
                self.set_toast(msg);
            }
            if let Some((new_sort, new_desc)) = grid_res.change_sort {
                self.sort_by = new_sort;
                self.sort_descending = new_desc;
            }

            // Bottom Selected Deal Dossier (bounded and vertically scrollable)
            if let Some(idx) = self.selected_deal_idx {
                if let Some(deal) = self.deals.get(idx) {
                    ui.add_space(6.0);
                    let dossier_h = ui.available_height();
                    let mut dossier_res = (false, false, None, None);
                    ui.allocate_ui(Vec2::new(avail_w, dossier_h), |ui| {
                        dossier_res = self.render_deal_dossier(ui, deal, false);
                    });

                    let (close_req, toggle_dock, d_toast, push_dest_ids) = dossier_res;
                    if close_req {
                        self.selected_deal_idx = None;
                    }
                    if toggle_dock {
                        self.dossier_side_panel = !self.dossier_side_panel;
                        if self.dossier_side_panel {
                            self.set_toast("Dossier docked to right side panel");
                        } else {
                            self.set_toast("Dossier docked to bottom drawer");
                        }
                    }
                    if let Some(msg) = d_toast {
                        self.set_toast(msg);
                    }
                    if let Some(dest_ids) = push_dest_ids {
                        self.push_to_eve_autopilot(dest_ids, "Deal Route");
                    }
                }
            }
        }

        if top_bar_switch_settings {
            self.tab = ActiveTab::Settings;
        }
        if let Some(msg) = top_bar_toast {
            self.set_toast(msg);
        }
        if let Some((dests, label)) = top_bar_push {
            self.push_to_eve_autopilot(dests, &label);
        }
    }

    fn render_deals_table_grid(
        &self,
        ui: &mut egui::Ui,
        filtered: &[(usize, &ArbitrageDeal)],
    ) -> DealsGridAction {
        let mut toggle_select: Option<usize> = None;
        let mut action_toast: Option<String> = None;
        let palette = self.theme.palette();
        let mut change_sort: Option<(DealSortBy, bool)> = None;
        let mut direct_push: Option<Vec<u64>> = None;
        let current_selected_idx = self.selected_deal_idx;
        let current_sort = self.sort_by;
        let is_desc = self.sort_descending;
        let is_big = self.ship_is_big;

        let avail_w = ui.available_width();
        let is_compact_tier = avail_w < 1000.0;

        let mut render_header_btn = |ui: &mut egui::Ui, title: &str, sort_target: DealSortBy| {
            let is_active = current_sort == sort_target;
            let arrow = if is_active {
                if is_desc {
                    " ▼"
                } else {
                    " ▲"
                }
            } else {
                ""
            };
            let label_text = format!("{}{}", title, arrow);
            let text_color = if is_active {
                palette.accent_primary
            } else {
                palette.text_secondary
            };
            let resp = ui
                .add(
                    egui::Button::new(RichText::new(label_text).color(text_color).strong())
                        .frame(false),
                )
                .on_hover_text(format!(
                    "Click to sort by {} ({})",
                    title,
                    if is_active && is_desc {
                        "switch to Ascending"
                    } else {
                        "switch to Descending"
                    }
                ));
            if resp.clicked() {
                ui.ctx().request_repaint();
                if is_active {
                    change_sort = Some((sort_target, !is_desc));
                } else {
                    let default_desc = !matches!(
                        sort_target,
                        DealSortBy::ItemName
                            | DealSortBy::BuySystem
                            | DealSortBy::SellSystem
                            | DealSortBy::Jumps
                            | DealSortBy::Cost
                    );
                    change_sort = Some((sort_target, default_desc));
                }
            }
        };

        // Responsive fluid column width distribution to fill 100% of window width
        let actions_w = (avail_w * 0.14).clamp(210.0, 260.0);
        let rank_w = 38.0;
        let pool = (avail_w - rank_w - actions_w - 24.0).max(550.0);

        let w_item = 15.0_f32;
        let w_buy_sys = 9.5_f32;
        let w_buy_price = 7.5_f32;
        let w_sell_sys = 9.5_f32;
        let w_sell_price = 7.5_f32;
        let w_qty = 5.5_f32;
        let w_vol = 6.0_f32;
        let w_profit_m3 = if is_compact_tier { 0.0 } else { 7.0 };
        let w_cost = 7.0_f32;
        let w_ship_util = if is_compact_tier { 0.0 } else { 7.0 };
        let w_net_profit = 9.5_f32;
        let w_net_roi = 6.0_f32;
        let w_jumps = 5.0_f32;
        let w_profit_jump = 9.0_f32;

        let total_w = w_item
            + w_buy_sys
            + w_buy_price
            + w_sell_sys
            + w_sell_price
            + w_qty
            + w_vol
            + w_profit_m3
            + w_cost
            + w_ship_util
            + w_net_profit
            + w_net_roi
            + w_jumps
            + w_profit_jump;

        let col_item = (pool * (w_item / total_w)).max(120.0);
        let col_buy_sys = (pool * (w_buy_sys / total_w)).max(75.0);
        let col_buy_price = (pool * (w_buy_price / total_w)).max(65.0);
        let col_sell_sys = (pool * (w_sell_sys / total_w)).max(75.0);
        let col_sell_price = (pool * (w_sell_price / total_w)).max(65.0);
        let col_qty = (pool * (w_qty / total_w)).max(42.0);
        let col_vol = (pool * (w_vol / total_w)).max(48.0);
        let col_profit_m3 = (pool * (w_profit_m3 / total_w)).max(60.0);
        let col_cost = (pool * (w_cost / total_w)).max(60.0);
        let col_ship_util = (pool * (w_ship_util / total_w)).max(65.0);
        let col_net_profit = (pool * (w_net_profit / total_w)).max(80.0);
        let col_net_roi = (pool * (w_net_roi / total_w)).max(48.0);
        let col_jumps = (pool * (w_jumps / total_w)).max(38.0);
        let col_profit_jump = (pool * (w_profit_jump / total_w)).max(80.0);

        let mut builder = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .min_scrolled_height(0.0)
            .column(Column::exact(rank_w)) // 1: #
            .column(Column::remainder().at_least(col_item).resizable(true)) // 2: ITEM (flexible expanding remainder)
            .column(Column::initial(col_buy_sys).at_least(70.0).resizable(true)) // 3: BUY SYSTEM
            .column(
                Column::initial(col_buy_price)
                    .at_least(60.0)
                    .resizable(true),
            ) // 4: BUY / ITEM
            .column(Column::initial(col_sell_sys).at_least(70.0).resizable(true)) // 5: SELL SYSTEM
            .column(
                Column::initial(col_sell_price)
                    .at_least(60.0)
                    .resizable(true),
            ) // 6: SELL / ITEM
            .column(Column::initial(col_qty).at_least(40.0).resizable(true)) // 7: QTY
            .column(Column::initial(col_vol).at_least(45.0).resizable(true)); // 8: VOL (m³)

        if !is_compact_tier {
            builder = builder.column(
                Column::initial(col_profit_m3)
                    .at_least(55.0)
                    .resizable(true),
            ); // 9: PROFIT/m³
        }

        builder = builder.column(Column::initial(col_cost).at_least(55.0).resizable(true)); // 10: COST (M)

        if !is_compact_tier {
            builder = builder.column(
                Column::initial(col_ship_util)
                    .at_least(60.0)
                    .resizable(true),
            ); // 11: SHIP UTIL
        }

        builder = builder
            .column(
                Column::initial(col_net_profit)
                    .at_least(75.0)
                    .resizable(true),
            ) // 12: NET PROFIT
            .column(Column::initial(col_net_roi).at_least(45.0).resizable(true)) // 13: NET ROI
            .column(Column::initial(col_jumps).at_least(35.0).resizable(true)) // 14: JUMPS
            .column(
                Column::initial(col_profit_jump)
                    .at_least(75.0)
                    .resizable(true),
            ) // 15: PROFIT/JUMP
            .column(Column::initial(actions_w).at_least(115.0).resizable(true)); // 16: QUICK ACTIONS

        builder
            .header(24.0, |mut header| {
                header.col(|ui| { ui.label(RichText::new("#").color(palette.text_secondary).strong()); });
                header.col(|ui| { render_header_btn(ui, "ITEM", DealSortBy::ItemName); });
                header.col(|ui| { render_header_btn(ui, "BUY SYSTEM", DealSortBy::BuySystem); });
                header.col(|ui| { render_header_btn(ui, "BUY / ITEM", DealSortBy::BuyPrice); });
                header.col(|ui| { render_header_btn(ui, "SELL SYSTEM", DealSortBy::SellSystem); });
                header.col(|ui| { render_header_btn(ui, "SELL / ITEM", DealSortBy::SellPrice); });
                header.col(|ui| { render_header_btn(ui, "QTY", DealSortBy::Quantity); });
                header.col(|ui| { render_header_btn(ui, "VOL (m³)", DealSortBy::Volume); });
                if !is_compact_tier {
                    header.col(|ui| { render_header_btn(ui, "PROFIT/m³", DealSortBy::ProfitPerM3); });
                }
                header.col(|ui| { render_header_btn(ui, "COST (M)", DealSortBy::Cost); });
                if !is_compact_tier {
                    header.col(|ui| { ui.label(RichText::new("SHIP UTIL").color(palette.text_secondary).strong()); });
                }
                header.col(|ui| { render_header_btn(ui, "NET PROFIT", DealSortBy::TotalProfit); });
                header.col(|ui| { render_header_btn(ui, "NET ROI", DealSortBy::RoiPct); });
                header.col(|ui| { render_header_btn(ui, "JUMPS", DealSortBy::Jumps); });
                header.col(|ui| { render_header_btn(ui, "PROFIT/JUMP", DealSortBy::ProfitPerJump); });
                header.col(|ui| { ui.label(RichText::new("QUICK ACTIONS").color(palette.text_secondary).strong()); });
            })
            .body(|mut body| {
                for (display_rank, &(orig_idx, deal)) in filtered.iter().enumerate() {
                    let is_selected = current_selected_idx == Some(orig_idx);
                    body.row(28.0, |mut row| {
                        row.set_selected(is_selected);

                        // 1: Rank badge
                        row.col(|ui| {
                            let rank_color = match display_rank {
                                0 => Color32::from_rgb(255, 215, 0),   // Gold
                                1 => palette.rank_silver(), // Silver
                                2 => Color32::from_rgb(205, 127, 50),  // Bronze
                                _ => palette.text_muted,
                            };
                            let rank_resp = egui::Frame::none()
                                .fill(Color32::from_rgba_unmultiplied(
                                    rank_color.r(), rank_color.g(), rank_color.b(), 28,
                                ))
                                .stroke(Stroke::new(1.0_f32, rank_color))
                                .rounding(3.0)
                                .inner_margin(4.0)
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(format!("#{}", display_rank + 1))
                                            .color(rank_color)
                                            .strong()
                                            .size(12.0),
                                    );
                                });
                            if rank_resp.response.interact(egui::Sense::click()).clicked() {
                                toggle_select = Some(orig_idx);
                            }
                        });

                        // 2: Item Name (selectable)
                        row.col(|ui| {
                            let item_color = if is_selected {
                                palette.accent_primary
                            } else {
                                palette.text_primary
                            };
                            ui.horizontal(|ui| {
                                if ui.selectable_label(is_selected, RichText::new(&deal.item_name).color(item_color).strong()).clicked() {
                                    toggle_select = Some(orig_idx);
                                }
                                if deal.is_high_roi() {
                                    ui.label(RichText::new("[+40% HOT]").size(10.0).color(Color32::from_rgb(255, 120, 0)).strong())
                                        .on_hover_text("High-ROI deal: Net return >= 40%");
                                }
                                if deal.is_npc_trade {
                                    ui.label(RichText::new("[NPC] NPC").size(10.0).color(Color32::from_rgb(110, 190, 255)).strong())
                                        .on_hover_text("NPC trade deal: Seeded commodity or 365-day NPC order");
                                }
                                if !crate::arbitrage::is_npc_station(deal.source_station_id) || !crate::arbitrage::is_npc_station(deal.dest_station_id) {
                                    ui.label(RichText::new("[CITADEL]").color(Color32::from_rgb(255, 185, 60)).strong())
                                        .on_hover_text("Includes player-owned Upwell Citadel / Structure");
                                }
                            });
                        });

                        // 3: Buy System
                        row.col(|ui| {
                            let src_sec_col = security_color(deal.source_security);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("[{:.1}]", deal.source_security)).size(11.0).color(src_sec_col).strong());
                                ui.label(RichText::new(&deal.source_system_name).color(Color32::from_rgb(120, 200, 255)));
                            });
                        });

                        // 4: Buy Price
                        row.col(|ui| {
                            ui.label(
                                RichText::new(format_unit_price_isk(deal.buy_price))
                                    .color(Color32::from_rgb(140, 210, 255)),
                            )
                            .on_hover_text(format!("Buy Price: {:.2} ISK / unit (from Sell Order)", deal.buy_price));
                        });

                        // 5: Sell System
                        row.col(|ui| {
                            let dst_sec_col = security_color(deal.dest_security);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("[{:.1}]", deal.dest_security)).size(11.0).color(dst_sec_col).strong());
                                ui.label(RichText::new(&deal.dest_system_name).color(Color32::from_rgb(255, 140, 240)));
                            });
                        });

                        // 6: Sell Price
                        row.col(|ui| {
                            ui.label(
                                RichText::new(format_unit_price_isk(deal.sell_price))
                                    .color(Color32::from_rgb(255, 160, 240)),
                            )
                            .on_hover_text(format!("Sell Price: {:.2} ISK / unit (into Buy Order)", deal.sell_price));
                        });

                        // 7: Quantity
                        row.col(|ui| {
                            ui.label(format!("{}", deal.quantity));
                        });

                        // 8: Volume
                        row.col(|ui| {
                            ui.label(format!("{:.1}", deal.volume_used));
                        });

                        // 9: Profit / m3 (if not compact)
                        if !is_compact_tier {
                            row.col(|ui| {
                                ui.label(RichText::new(format_rate_isk(deal.profit_per_m3)).color(palette.accent_secondary).strong());
                            });
                        }

                        // 10: Cost
                        row.col(|ui| {
                            ui.label(format!("{:.2}M", deal.capital_required / 1_000_000.0));
                        });

                        // 11: Ship Util (if not compact)
                        if !is_compact_tier {
                            row.col(|ui| {
                                let max_vol = if is_big { 60_000.0_f64 } else { 30_000.0_f64 };
                                let max_budget = if is_big { 500_000_000.0_f64 } else { 50_000_000.0_f64 };
                                let vol_frac = (deal.volume_used / max_vol) as f32;
                                let budget_frac = (deal.capital_required / max_budget) as f32;
                                render_ship_util_bars(ui, vol_frac, budget_frac)
                                    .on_hover_text(format!(
                                        "Cargo:  {:.1} / {:.0} m³  ({:.0}%)\nBudget: {:.2}M / {:.0}M ISK  ({:.0}%)",
                                        deal.volume_used, max_vol, vol_frac * 100.0,
                                        deal.capital_required / 1_000_000.0,
                                        max_budget / 1_000_000.0,
                                        budget_frac * 100.0,
                                    ));
                            });
                        }

                        // 12: Net Profit
                        row.col(|ui| {
                            ui.label(
                                RichText::new(format!("+{:.2}M ISK", deal.net_profit / 1_000_000.0))
                                    .color(palette.success_green)
                                    .strong(),
                            );
                        });

                        // 13: Net ROI
                        row.col(|ui| {
                            let roi_col = if deal.is_high_roi() {
                                palette.warning_yellow
                            } else {
                                palette.success_green
                            };
                            ui.label(
                                RichText::new(format!("+{:.1}%", deal.net_profit_pct))
                                    .color(roi_col)
                                    .strong(),
                            );
                        });

                        // 14: Jumps
                        row.col(|ui| {
                            let jumps_tag = format!("{}+{}={}", deal.jumps_from_home, deal.jumps_source_dest, deal.total_jumps);
                            ui.label(RichText::new(jumps_tag).color(palette.text_muted));
                        });

                        // 15: Profit / Jump
                        row.col(|ui| {
                            ui.label(
                                RichText::new(format!("{:.2}M", deal.profit_per_jump / 1_000_000.0))
                                    .color(palette.accent_secondary)
                                    .strong(),
                            );
                        });

                        // 16: Quick Actions
                        row.col(|ui| {
                            ui.horizontal(|ui| {
                                let auto_btn = palette.primary_button(
                                    RichText::new("▶ Autopilot")
                                        .size(11.0)
                                        .color(palette.accent_primary_text)
                                        .strong(),
                                );
                                if ui
                                    .add(auto_btn)
                                    .on_hover_text(format!(
                                        "Inject 2-stop trade route into live EVE Autopilot HUD:\n1. Buy at: {}\n2. Sell at: {}",
                                        deal.source_station_name, deal.dest_station_name
                                    ))
                                    .clicked()
                                {
                                    direct_push = Some(deal.ordered_destination_ids());
                                    action_toast = Some(format!(
                                        "Injecting route for '{}' into EVE Autopilot HUD...",
                                        deal.item_name
                                    ));
                                }

                                if ui.small_button("Item").on_hover_text("Copy item name to clipboard").clicked() {
                                    copy_to_clipboard(ui, deal.item_name.clone());
                                    action_toast = Some(format!("Copied item '{}' to clipboard!", deal.item_name));
                                }
                                if ui.small_button("Buy").on_hover_text(format!("Copy buy station '{}' to clipboard", deal.source_station_name)).clicked() {
                                    copy_to_clipboard(ui, deal.source_station_name.clone());
                                    action_toast = Some(format!("Copied buy station '{}' to clipboard!", deal.source_station_name));
                                }
                                if ui.small_button("Sell").on_hover_text(format!("Copy sell station '{}' to clipboard", deal.dest_station_name)).clicked() {
                                    copy_to_clipboard(ui, deal.dest_station_name.clone());
                                    action_toast = Some(format!("Copied sell station '{}' to clipboard!", deal.dest_station_name));
                                }
                            });
                        });
                    });
                }
            });

        DealsGridAction {
            toggle_select,
            action_toast,
            change_sort,
            direct_push,
        }
    }

    fn render_deal_dossier(
        &self,
        ui: &mut egui::Ui,
        deal: &ArbitrageDeal,
        is_side_panel: bool,
    ) -> (bool, bool, Option<String>, Option<Vec<u64>>) {
        let mut close_requested = false;
        let mut toggle_dock_requested = false;
        let mut toast = None;
        let mut push_dest_ids = None;

        let palette = self.theme.palette();

        palette.card_frame().inner_margin(10.0).show(ui, |ui| {
                // Header row
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("TRADE DOSSIER: {}", deal.item_name))
                            .size(15.0)
                            .color(palette.accent_primary)
                            .strong(),
                    );
                    if ui.small_button("Copy Name").clicked() {
                        copy_to_clipboard(ui, deal.item_name.clone());
                        toast = Some(format!("Copied '{}'", deal.item_name));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✕ Close").clicked() {
                            close_requested = true;
                        }
                        let dock_text = if is_side_panel {
                            "▬ Dock Bottom"
                        } else {
                            "◫ Dock Side"
                        };
                        let can_dock_side = ui.available_width() >= 1150.0 || is_side_panel;
                        let dock_btn = egui::Button::new(RichText::new(dock_text).size(12.0));
                        let resp = ui.add_enabled(can_dock_side, dock_btn);
                        if resp.clicked() {
                            toggle_dock_requested = true;
                        }
                        if !can_dock_side && !is_side_panel {
                            resp.on_hover_text("Side docking requires >= 1150px window width");
                        }

                        let push_btn = palette.primary_button(
                            RichText::new("▶ Push Deal to EVE Autopilot")
                                .size(11.5)
                                .color(palette.accent_primary_text)
                                .strong(),
                        );
                        if ui
                            .add(push_btn)
                            .on_hover_text("Inject this deal's buy & sell stations straight into your running EVE client Autopilot HUD")
                            .clicked()
                        {
                            push_dest_ids = Some(deal.ordered_destination_ids());
                        }

                        let sheet_btn = palette.secondary_button(
                            RichText::new("Copy Flight Sheet (Alt+N)")
                                .size(11.5)
                                .color(palette.accent_secondary_text)
                                .strong(),
                        );
                        if ui
                            .add(sheet_btn)
                            .on_hover_text("Copy formatted trade sheet for in-game Notepad (Alt+N)")
                            .clicked()
                        {
                            let sheet = deal.format_eve_flight_sheet();
                            copy_to_clipboard(ui, sheet);
                            toast = Some("Copied deal flight sheet to clipboard! Paste in EVE Notepad (Alt+N).".to_string());
                        }
                    });
                });

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                // Content inside vertical scroll area so nothing ever clips off-screen
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let render_purchase = |ui: &mut egui::Ui, toast: &mut Option<String>| {
                            palette.sub_card_frame().show(ui, |ui| {
                                    ui.heading(
                                        RichText::new("PURCHASE ORDER")
                                            .size(13.0)
                                            .color(palette.info_blue),
                                    );
                                    let src_is_npc =
                                        crate::arbitrage::is_npc_station(deal.source_station_id);
                                    let (src_badge, src_badge_color) = if src_is_npc {
                                        ("[NPC Station]", palette.info_blue)
                                    } else {
                                        ("[Upwell Citadel]", palette.warning_yellow)
                                    };
                                    ui.label(
                                        RichText::new(&deal.source_station_name)
                                            .size(12.0)
                                            .strong(),
                                    );
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(src_badge)
                                                .size(11.0)
                                                .color(src_badge_color)
                                                .strong(),
                                        );
                                        if deal.is_npc_trade {
                                            ui.label(
                                                RichText::new("[NPC Trade]")
                                                    .size(11.0)
                                                    .color(palette.info_blue)
                                                    .strong(),
                                            )
                                            .on_hover_text("NPC trade: Seeded commodity trade good or 365-day NPC order");
                                        }
                                    });
                                    let src_sec_col = security_color(deal.source_security);
                                    ui.label(
                                        RichText::new(format!(
                                            "System: {} [{:.1}] ({} jumps from home)",
                                            deal.source_system_name,
                                            deal.source_security,
                                            deal.jumps_from_home
                                        ))
                                        .color(src_sec_col),
                                    );
                                    ui.label(format!(
                                        "Buy Price / Item: {:.2} ISK (Sell Order)",
                                        deal.buy_price
                                    ));
                                    ui.label(format!("Quantity: {} units", deal.quantity));
                                    ui.label(format!(
                                        "Capital Cost: {:.2}M ISK",
                                        deal.capital_required / 1_000_000.0
                                    ));
                                    if ui.small_button("Copy Buy Station").clicked() {
                                        copy_to_clipboard(ui, deal.source_station_name.clone());
                                        *toast = Some(format!(
                                            "Copied buy station '{}'",
                                            deal.source_station_name
                                        ));
                                    }
                                });
                        };

                        let render_sale = |ui: &mut egui::Ui, toast: &mut Option<String>| {
                            palette.sub_card_frame().show(ui, |ui| {
                                    ui.heading(
                                        RichText::new("SALE TARGET")
                                            .size(13.0)
                                            .color(palette.accent_secondary),
                                    );
                                    let dst_is_npc =
                                        crate::arbitrage::is_npc_station(deal.dest_station_id);
                                    let (dst_badge, dst_badge_color) = if dst_is_npc {
                                        ("[NPC Station]", palette.info_blue)
                                    } else {
                                        ("[Upwell Citadel]", palette.warning_yellow)
                                    };
                                    ui.label(
                                        RichText::new(&deal.dest_station_name).size(12.0).strong(),
                                    );
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(dst_badge)
                                                .size(11.0)
                                                .color(dst_badge_color)
                                                .strong(),
                                        );
                                        if deal.is_npc_trade {
                                            ui.label(
                                                RichText::new("[NPC Trade]")
                                                    .size(11.0)
                                                    .color(palette.info_blue)
                                                    .strong(),
                                            )
                                            .on_hover_text("NPC trade: Seeded commodity trade good or 365-day NPC order");
                                        }
                                    });
                                    let dst_sec_col = security_color(deal.dest_security);
                                    ui.label(
                                        RichText::new(format!(
                                            "System: {} [{:.1}] ({} jumps delivery)",
                                            deal.dest_system_name,
                                            deal.dest_security,
                                            deal.jumps_source_dest
                                        ))
                                        .color(dst_sec_col),
                                    );
                                    ui.label(format!(
                                        "Sell Price / Item: {:.2} ISK (Buy Order)",
                                        deal.sell_price
                                    ));
                                    ui.label(format!(
                                        "Gross Revenue: {:.2}M ISK",
                                        (deal.sell_price * deal.quantity as f64) / 1_000_000.0
                                    ));
                                    ui.label(format!("Price Spread: +{:.1}%", deal.spread_pct));
                                    if ui.small_button("Copy Sell Station").clicked() {
                                        copy_to_clipboard(ui, deal.dest_station_name.clone());
                                        *toast = Some(format!(
                                            "Copied sell station '{}'",
                                            deal.dest_station_name
                                        ));
                                    }
                                });
                        };

                        let ship = self.current_ship();
                        let max_vol = ship.max_volume;
                        let max_budget = ship.max_budget;
                        let vol_pct = (deal.volume_used / max_vol) * 100.0;
                        let budget_pct = (deal.capital_required / max_budget) * 100.0;

                        let render_financial = |ui: &mut egui::Ui| {
                            palette.sub_card_frame().show(ui, |ui| {
                                    ui.heading(
                                        RichText::new("FINANCIAL SUMMARY")
                                            .size(13.0)
                                            .color(palette.success_green),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "NET PROFIT: +{:.2}M ISK",
                                            deal.net_profit / 1_000_000.0
                                        ))
                                        .color(palette.success_green)
                                        .strong(),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "NET ROI: +{:.1}%",
                                            deal.net_profit_pct
                                        ))
                                        .color(palette.success_green)
                                        .strong(),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "PROFIT / JUMP: {:.2}M ISK",
                                            deal.profit_per_jump / 1_000_000.0
                                        ))
                                        .color(palette.accent_secondary)
                                        .strong(),
                                    );
                                    ui.label(format!(
                                        "Profit Density: {} ISK/m³",
                                        format_rate_isk(deal.profit_per_m3)
                                    ));
                                    ui.label(format!(
                                        "Cargo: {:.1} m³ ({:.1}%)",
                                        deal.volume_used, vol_pct
                                    ));
                                    ui.label(format!(
                                        "Budget: {:.2}M ({:.1}%)",
                                        deal.capital_required / 1_000_000.0,
                                        budget_pct
                                    ));
                                    ui.label(format!("Travel: {} total jumps", deal.total_jumps));
                                });
                        };

                        let use_cols = (!is_side_panel && ui.available_width() >= 850.0)
                            || (is_side_panel && ui.available_width() >= 480.0);
                        if use_cols {
                            if is_side_panel {
                                ui.columns(2, |cols| {
                                    render_purchase(&mut cols[0], &mut toast);
                                    cols[0].add_space(6.0);
                                    render_sale(&mut cols[0], &mut toast);
                                    render_financial(&mut cols[1]);
                                });
                            } else {
                                ui.columns(3, |cols| {
                                    render_purchase(&mut cols[0], &mut toast);
                                    render_sale(&mut cols[1], &mut toast);
                                    render_financial(&mut cols[2]);
                                });
                            }
                        } else {
                            render_purchase(ui, &mut toast);
                            ui.add_space(6.0);
                            render_sale(ui, &mut toast);
                            ui.add_space(6.0);
                            render_financial(ui);
                        }
                    });
            });

        (close_requested, toggle_dock_requested, toast, push_dest_ids)
    }

    fn render_manifest_card(
        &self,
        ui: &mut egui::Ui,
        idx: usize,
        m: &TradeCorridorManifest,
        max_vol: f64,
        max_budget: f64,
    ) -> (Option<String>, Option<Vec<u64>>) {
        let palette = self.theme.palette();
        let mut toast = None;
        let mut push_dest_ids = None;
        let vol_pct = (m.total_volume_used / max_vol) * 100.0;
        let budget_pct = (m.total_capital_required / max_budget) * 100.0;
        let src_sec_col = security_color(m.source_security);
        let dst_sec_col = security_color(m.dest_security);

        palette.card_frame().inner_margin(12.0).show(ui, |ui| {
                // Header row with route & profit badge
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("MANIFEST #{}", idx + 1))
                            .color(palette.accent_primary)
                            .size(14.0)
                            .strong(),
                    );
                    ui.separator();
                    ui.label(RichText::new(format!("[{:.1}]", m.source_security)).color(src_sec_col).strong());
                    ui.label(RichText::new(&m.source_system_name).color(Color32::from_rgb(0, 210, 255)).strong());
                    ui.label(RichText::new("→").color(palette.text_secondary).strong());
                    ui.label(RichText::new(format!("[{:.1}]", m.dest_security)).color(dst_sec_col).strong());
                    ui.label(RichText::new(&m.dest_system_name).color(Color32::from_rgb(255, 140, 240)).strong());
                    ui.label(format!("({} jumps)", m.jumps));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("+{:.2}M ISK Net Profit (+{:.1}% ROI)", m.total_net_profit / 1_000_000.0, m.overall_roi_pct))
                                .color(palette.success_green)
                                .size(13.5)
                                .strong(),
                        );
                    });
                });

                ui.add_space(6.0);

                // Metrics bar
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("Profit/Jump: {:.2}M/jump", m.profit_per_jump / 1_000_000.0))
                            .color(palette.accent_secondary)
                            .strong(),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!("Cargo Hold: {:.1} / {:.0} m³ ({:.1}%)", m.total_volume_used, max_vol, vol_pct))
                            .color(palette.text_secondary),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!("Budget Used: {:.2}M / {:.0}M ({:.1}%)", m.total_capital_required / 1_000_000.0, max_budget / 1_000_000.0, budget_pct))
                            .color(palette.text_secondary),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!("Items: {} distinct deals packed", m.deals.len()))
                            .color(palette.text_muted),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let push_btn = palette.primary_button(
                            RichText::new("▶ Push Autopilot")
                                .size(11.0)
                                .color(palette.accent_primary_text)
                                .strong(),
                        );
                        if ui
                            .add(push_btn)
                            .on_hover_text("Inject corridor stations into live EVE Autopilot HUD")
                            .clicked()
                        {
                            push_dest_ids = Some(m.ordered_destination_ids());
                        }

                        let sheet_btn = palette.secondary_button(
                            RichText::new("Copy Flight Sheet")
                                .size(11.0)
                                .color(palette.accent_secondary_text)
                                .strong(),
                        );
                        if ui
                            .add(sheet_btn)
                            .on_hover_text("Copy formatted corridor flight sheet for EVE Notepad (Alt+N)")
                            .clicked()
                        {
                            let sheet = m.format_eve_flight_sheet(idx + 1);
                            copy_to_clipboard(ui, sheet);
                            toast = Some("Copied manifest flight sheet! Paste into in-game Notepad (Alt+N).".to_string());
                        }

                        if ui.small_button("Multi-Buy").on_hover_text("Copy in EVE Online Multi-Buy format: '<Item Name>\\t<Quantity>\\n' for in-game paste").clicked() {
                            let mb = m.deals.iter().map(|d| format!("{}\t{}", d.item_name, d.quantity)).collect::<Vec<_>>().join("\n");
                            copy_to_clipboard(ui, mb);
                            toast = Some(format!("Copied {} items in Multi-Buy format!", m.deals.len()));
                        }
                        if ui.small_button("Copy Shopping List").on_hover_text("Copy item names and quantities to clipboard").clicked() {
                            let list = m.deals.iter().map(|d| format!("{}x {}", d.quantity, d.item_name)).collect::<Vec<_>>().join("\n");
                            copy_to_clipboard(ui, list);
                            toast = Some(format!("Copied {} manifest items to clipboard!", m.deals.len()));
                        }
                        if ui.small_button("Copy Buy Station").clicked() {
                            copy_to_clipboard(ui, m.source_station_name.clone());
                            toast = Some(format!("Copied '{}'", m.source_station_name));
                        }
                        if ui.small_button("Copy Sell Station").clicked() {
                            copy_to_clipboard(ui, m.dest_station_name.clone());
                            toast = Some(format!("Copied '{}'", m.dest_station_name));
                        }
                    });
                });

                ui.add_space(6.0);

                // Table of packed items in this manifest
                egui::Grid::new(format!("manifest_grid_{}", idx))
                    .striped(true)
                    .spacing(Vec2::new(12.0, 4.0))
                    .min_col_width(50.0)
                    .show(ui, |ui| {
                        ui.label(RichText::new("PACKED ITEM").color(palette.text_secondary).strong());
                        ui.label(RichText::new("QTY").color(palette.text_secondary).strong());
                        ui.label(RichText::new("VOL (m³)").color(palette.text_secondary).strong());
                        ui.label(RichText::new("BUY / UNIT").color(palette.text_secondary).strong());
                        ui.label(RichText::new("SELL / UNIT").color(palette.text_secondary).strong());
                        ui.label(RichText::new("COST (M)").color(palette.text_secondary).strong());
                        ui.label(RichText::new("PROFIT (M)").color(palette.text_secondary).strong());
                        ui.label(RichText::new("ROI %").color(palette.text_secondary).strong());
                        ui.label(RichText::new("PROFIT/m³").color(palette.text_secondary).strong());
                        ui.end_row();

                        for d in &m.deals {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&d.item_name).strong());
                                if d.is_high_roi() {
                                    ui.label(RichText::new("[+40% HOT]").size(10.0).color(palette.warning_yellow).strong());
                                }
                            });
                            ui.label(format!("{}", d.quantity));
                            ui.label(format!("{:.1}", d.volume_used));
                            ui.label(format!("{:.2}", d.buy_price));
                            ui.label(format!("{:.2}", d.sell_price));
                            ui.label(format!("{:.2}M", d.capital_required / 1_000_000.0));
                            ui.label(RichText::new(format!("+{:.2}M", d.net_profit / 1_000_000.0)).color(palette.success_green).strong());
                            ui.label(RichText::new(format!("+{:.1}%", d.net_profit_pct)).color(palette.success_green).strong());
                            ui.label(RichText::new(format_rate_isk(d.profit_per_m3)).color(palette.accent_secondary));
                            ui.end_row();
                        }
                    });
            });

        (toast, push_dest_ids)
    }

    fn render_manifests_tab(&mut self, ui: &mut egui::Ui) {
        let palette = self.theme.palette();
        if self.manifests.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.heading(
                    RichText::new("No Packed Corridor Manifests Available")
                        .size(22.0)
                        .color(palette.text_secondary),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "Manifests group multiple profitable items along the exact same flight path to pack your cargo hold and capital budget.\n\
                         Run a market scan (or enable +Adjacent Regions) to discover multi-item corridor opportunities.",
                    )
                    .size(14.0)
                    .color(palette.text_muted),
                );
                ui.add_space(16.0);
                let btn = palette.primary_button(
                    RichText::new("▶ Scan Market Now")
                        .size(16.0)
                        .color(palette.accent_primary_text)
                        .strong(),
                )
                .min_size(Vec2::new(240.0, 42.0));

                if ui.add(btn).clicked() {
                    self.start_scan();
                }
            });
            return;
        }

        // Header info card
        palette.card_frame().inner_margin(12.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        RichText::new(format!("[MANIFEST] {} Packed Trade Corridor Manifests", self.manifests.len()))
                            .color(palette.accent_primary)
                            .strong(),
                    );
                    ui.label(
                        RichText::new("│ Multi-Item Cargo Bundles Packed for Maximum Profit Density")
                            .size(12.0)
                            .color(palette.text_muted),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let export_btn = egui::Button::new(
                            RichText::new("Export Manifests to CSV")
                                .color(palette.accent_primary)
                                .strong(),
                        )
                        .fill(palette.card_sub_bg);
                        if ui.add(export_btn).on_hover_text("Export all packed manifests to ./manifests_export.csv and clipboard").clicked() {
                            let csv_str = export_manifests_to_csv(&self.manifests);
                            copy_to_clipboard(ui, csv_str.clone());
                            let export_path = resolve_csv_export_path("manifests_export.csv");
                            match safe_write_csv(&export_path, &csv_str) {
                                Ok(()) => {
                                    self.set_toast(format!(
                                        "Exported {} manifests to {} & copied to clipboard!",
                                        self.manifests.len(),
                                        export_path.display()
                                    ));
                                }
                                Err(e) => {
                                    tracing::error!(
                                        target: "eve_trade_scout",
                                        error_code = %crate::error::ErrorCode::ErrUiCsvExport,
                                        path = %export_path.display(),
                                        error = %e,
                                        "Failed to write CSV manifests export to disk: {e}"
                                    );
                                    self.set_toast(format!("Failed to export CSV: {e}"));
                                }
                            }
                        }
                    });
                });
                ui.add_space(4.0);
                ui.label(
                    "Each manifest consolidates multiple profitable arbitrage items moving along the same origin-to-destination corridor, maximizing your hauler's volume and capital budget per run.",
                );
            });

        ui.add_space(10.0);

        let ship = self.current_ship();
        let max_vol = ship.max_volume;
        let max_budget = ship.max_budget;

        let mut manifest_toast: Option<String> = None;
        let avail_w = ui.available_width();
        let manifest_indices: Vec<usize> = (0..self.manifests.len()).collect();

        egui::ScrollArea::vertical().show(ui, |ui| {
            if avail_w >= 1400.0 {
                // Browser-grade 2-column responsive layout for wide displays
                let mut push_manifest_dests: Option<Vec<u64>> = None;
                for chunk in manifest_indices.chunks(2) {
                    if chunk.len() == 2 {
                        let idx0 = chunk[0];
                        let idx1 = chunk[1];
                        let m0 = &self.manifests[idx0];
                        let m1 = &self.manifests[idx1];
                        ui.columns(2, |cols| {
                            let (msg0, dest0) = self.render_manifest_card(
                                &mut cols[0],
                                idx0,
                                m0,
                                max_vol,
                                max_budget,
                            );
                            if let Some(msg) = msg0 {
                                manifest_toast = Some(msg);
                            }
                            if let Some(d) = dest0 {
                                push_manifest_dests = Some(d);
                            }

                            let (msg1, dest1) = self.render_manifest_card(
                                &mut cols[1],
                                idx1,
                                m1,
                                max_vol,
                                max_budget,
                            );
                            if let Some(msg) = msg1 {
                                manifest_toast = Some(msg);
                            }
                            if let Some(d) = dest1 {
                                push_manifest_dests = Some(d);
                            }
                        });
                    } else {
                        let idx0 = chunk[0];
                        let m0 = &self.manifests[idx0];
                        ui.columns(2, |cols| {
                            let (msg0, dest0) = self.render_manifest_card(
                                &mut cols[0],
                                idx0,
                                m0,
                                max_vol,
                                max_budget,
                            );
                            if let Some(msg) = msg0 {
                                manifest_toast = Some(msg);
                            }
                            if let Some(d) = dest0 {
                                push_manifest_dests = Some(d);
                            }
                        });
                    }
                    ui.add_space(8.0);
                }
                if let Some(dests) = push_manifest_dests {
                    self.push_to_eve_autopilot(dests, "Corridor Manifest");
                }
            } else {
                let mut push_manifest_dests: Option<Vec<u64>> = None;
                // Fluid single-column layout for standard/compact displays
                for (idx, m) in self.manifests.iter().enumerate() {
                    let (msg, dest) = self.render_manifest_card(ui, idx, m, max_vol, max_budget);
                    if let Some(m_msg) = msg {
                        manifest_toast = Some(m_msg);
                    }
                    if let Some(d) = dest {
                        push_manifest_dests = Some(d);
                    }
                    ui.add_space(8.0);
                }
                if let Some(dests) = push_manifest_dests {
                    self.push_to_eve_autopilot(dests, "Corridor Manifest");
                }
            }
        });

        if let Some(msg) = manifest_toast {
            self.set_toast(msg);
        }
    }

    fn render_route_stop_card(&self, ui: &mut egui::Ui, stop: &RouteStop) -> Option<String> {
        let palette = self.theme.palette();
        let mut toast = None;
        palette.card_frame().inner_margin(12.0).show(ui, |ui| {
            // Stop header
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("[STOP #{}]", stop.step))
                        .color(palette.accent_primary)
                        .size(14.0)
                        .strong(),
                );
                ui.label(format!(
                    "Travel {} jumps to pickup at",
                    stop.jumps_from_prev
                ));
                ui.label(
                    RichText::new(&stop.deal.source_system_name)
                        .color(palette.info_blue)
                        .strong(),
                );
                if ui.small_button("Copy System").clicked() {
                    copy_to_clipboard(ui, stop.deal.source_system_name.clone());
                    toast = Some(format!("Copied '{}'", stop.deal.source_system_name));
                }
            });

            ui.add_space(6.0);

            // Actions breakdown
            ui.columns(2, |cols| {
                // Buy side
                cols[0].label(
                    RichText::new("1. BUY AT SOURCE:")
                        .color(palette.success_green)
                        .strong(),
                );
                cols[0].label(
                    RichText::new(format!("{}x {}", stop.deal.quantity, stop.deal.item_name))
                        .strong(),
                );
                cols[0].label(format!(
                    "Unit: {:.2} ISK (Total: {:.2}M ISK)",
                    stop.deal.buy_price,
                    stop.deal.capital_required / 1_000_000.0
                ));
                cols[0].label(
                    RichText::new(&stop.deal.source_station_name)
                        .color(palette.text_muted)
                        .size(11.0),
                );
                if cols[0].small_button("Copy Buy Station").clicked() {
                    copy_to_clipboard(&mut cols[0], stop.deal.source_station_name.clone());
                    toast = Some(format!(
                        "Copied buy station '{}'",
                        stop.deal.source_station_name
                    ));
                }

                // Sell side
                cols[1].label(
                    RichText::new(format!(
                        "2. WARP {} JUMPS & SELL:",
                        stop.deal.jumps_source_dest
                    ))
                    .color(palette.warning_yellow)
                    .strong(),
                );
                cols[1].label(
                    RichText::new(format!("Destination: {}", stop.deal.dest_system_name))
                        .color(palette.accent_secondary)
                        .strong(),
                );
                cols[1].label(
                    RichText::new(format!(
                        "Net Profit: +{:.2}M ISK (+{:.1}% ROI)",
                        stop.deal.net_profit / 1_000_000.0,
                        stop.deal.net_profit_pct
                    ))
                    .color(palette.success_green)
                    .strong(),
                );
                cols[1].label(
                    RichText::new(&stop.deal.dest_station_name)
                        .color(palette.text_muted)
                        .size(11.0),
                );
                if cols[1].small_button("Copy Sell Station").clicked() {
                    copy_to_clipboard(&mut cols[1], stop.deal.dest_station_name.clone());
                    toast = Some(format!(
                        "Copied sell station '{}'",
                        stop.deal.dest_station_name
                    ));
                }
            });
        });
        toast
    }

    fn render_route_tab(&mut self, ui: &mut egui::Ui) {
        let palette = self.theme.palette();
        let route = match self.route.clone() {
            Some(r) if !r.stops.is_empty() => r,
            _ => {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    ui.heading(
                        RichText::new("No Multi-Stop Route Planned Yet")
                            .size(22.0)
                            .color(palette.text_secondary),
                    );
                    ui.label(
                        RichText::new("Run a market scan to automatically plan an optimized multi-hop trade route chaining deals together.")
                            .size(14.0)
                            .color(palette.text_muted),
                    );
                });
                return;
            }
        };

        // Summary Card
        palette.card_frame().inner_margin(12.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        RichText::new("Planned Multi-Stop Route Summary")
                            .color(palette.accent_primary)
                            .strong(),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let push_btn = palette.primary_button(
                            RichText::new(if self.sso_is_pushing {
                                "⏳ Pushing to EVE..."
                            } else {
                                "▶ Push Route to EVE Autopilot"
                            })
                            .color(palette.accent_primary_text)
                            .strong(),
                        );

                        if ui
                            .add_enabled(!self.sso_is_pushing, push_btn)
                            .on_hover_text("Directly inject all planned waypoint stops into your live running EVE Online Autopilot HUD")
                            .clicked()
                        {
                            let dest_ids = route.ordered_destination_ids();
                            self.push_to_eve_autopilot(dest_ids, "Planned Route");
                        }

                        let sheet_btn = palette.secondary_button(
                            RichText::new("Copy Flight Sheet (Alt+N)")
                                .color(palette.accent_secondary_text)
                                .strong(),
                        );

                        if ui
                            .add(sheet_btn)
                            .on_hover_text("Copy formatted Flight Sheet for EVE Notepad (Alt+N) with full buy/sell stops")
                            .clicked()
                        {
                            let sheet = route.format_eve_flight_sheet();
                            copy_to_clipboard(ui, sheet);
                            self.set_toast("Copied EVE Flight Sheet! Paste into in-game Notepad (Alt+N).");
                        }

                        let copy_btn = egui::Button::new(
                            RichText::new("Copy Waypoints")
                                .color(palette.text_primary)
                                .strong(),
                        )
                        .fill(palette.card_sub_bg);

                        if ui.add(copy_btn).clicked() {
                            let trail = route.waypoint_names.join(" -> ");
                            copy_to_clipboard(ui, trail);
                            self.set_toast("Copied entire in-game waypoint trail to clipboard!");
                        }
                    });
                });

                ui.add_space(8.0);

                let route_roi = if route.total_capital_required > 0.0 {
                    (route.total_profit / route.total_capital_required) * 100.0
                } else {
                    0.0
                };

                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("Total Jumps: {}", route.total_jumps))
                            .strong()
                            .color(palette.text_secondary),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "Total Profit: +{:.2}M ISK",
                            route.total_profit / 1_000_000.0
                        ))
                        .strong()
                        .color(palette.success_green),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!("Overall ROI: +{:.1}%", route_roi))
                            .strong()
                            .color(palette.success_green),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "Avg Profit/Jump: {:.2}M ISK",
                            route.avg_profit_per_jump / 1_000_000.0
                        ))
                        .strong()
                        .color(palette.accent_secondary),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "Capital Needed: {:.2}M ISK",
                            route.total_capital_required / 1_000_000.0
                        ))
                        .color(palette.info_blue),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!("Peak Cargo: {:.1} m³", route.max_volume_used))
                            .color(palette.accent_primary),
                    );
                });
            });

        ui.add_space(10.0);
        ui.heading("Sequential Flight Legs & Trade Actions:");
        ui.add_space(6.0);

        let avail_w = ui.available_width();
        let mut route_toast: Option<String> = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            if avail_w >= 1400.0 {
                // Browser-grade 2-column responsive layout for wide displays
                for chunk in route.stops.chunks(2) {
                    if chunk.len() == 2 {
                        ui.columns(2, |cols| {
                            if let Some(msg) = self.render_route_stop_card(&mut cols[0], &chunk[0])
                            {
                                route_toast = Some(msg);
                            }
                            if let Some(msg) = self.render_route_stop_card(&mut cols[1], &chunk[1])
                            {
                                route_toast = Some(msg);
                            }
                        });
                    } else {
                        ui.columns(2, |cols| {
                            if let Some(msg) = self.render_route_stop_card(&mut cols[0], &chunk[0])
                            {
                                route_toast = Some(msg);
                            }
                        });
                    }
                    ui.add_space(6.0);
                }
            } else {
                for stop in &route.stops {
                    if let Some(msg) = self.render_route_stop_card(ui, stop) {
                        route_toast = Some(msg);
                    }
                    ui.add_space(6.0);
                }
            }

            ui.add_space(10.0);
            ui.label(RichText::new("Complete In-Game Autopilot Waypoints Trail:").strong());
            ui.label(
                RichText::new(route.waypoint_names.join(" → "))
                    .size(13.0)
                    .color(Color32::from_rgb(180, 215, 255)),
            );
        });

        if let Some(msg) = route_toast {
            self.set_toast(msg);
        }
    }

    fn start_chain_search(&mut self) {
        if self.chain_is_running || self.deals.is_empty() {
            return;
        }
        let (Some(sde), Some(graph)) = (&self.sde, &self.graph) else {
            return;
        };
        self.chain_is_running = true;
        let deals = self.deals.clone();
        let graph = Arc::clone(graph);
        let (tx, rx) = channel();
        self.chain_rx = Some(rx);

        let home_id = sde.get_system_id(&self.home_system).unwrap_or(30000142);

        let cfg = crate::chain_route_planner::ChainConfig {
            home_system: home_id,
            max_legs: self.chain_max_legs,
            beam_width: 15,
            detour_enabled: self.chain_detour_enabled,
            max_detour_jumps: self.chain_max_detour,
            max_dest_loads: self.chain_max_dest_loads,
            ship_max_volume: {
                let ship = self.current_ship();
                ship.max_volume
            },
            ship_max_budget: {
                let ship = self.current_ship();
                ship.max_budget
            },
            safe_only: self.safe_only,
            include_npc: self.chain_include_npc,
        };

        thread::spawn(move || {
            let results = crate::chain_route_planner::find_chain_routes(&deals, &graph, &cfg);
            let _ = tx.send(results);
        });
    }

    fn render_chain_route_tab(&mut self, ui: &mut egui::Ui) {
        let palette = self.theme.palette();
        // 1. Controls Header Card
        palette.card_frame().inner_margin(12.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        RichText::new("Chain Routes Plotter — Multi-Leg Hauling Optimizer")
                            .color(palette.accent_primary)
                            .strong(),
                    );
                    ui.label(
                        RichText::new("Knapsack cargo packing & en-route bonus trade chaining")
                            .color(palette.text_muted)
                            .size(12.0),
                    );
                });

                ui.add_space(8.0);

                ui.horizontal_wrapped(|ui| {
                    ui.scope(|ui| {
                        // Distinct, visible rail styling with theme styling
                        ui.spacing_mut().slider_rail_height = 6.0;
                        let mut visuals = ui.visuals_mut().clone();
                        visuals.widgets.inactive.bg_fill = palette.card_sub_bg;
                        visuals.widgets.inactive.bg_stroke = palette.widget_inactive_stroke;
                        visuals.widgets.hovered.bg_fill = palette.widget_hovered_bg;
                        visuals.widgets.hovered.bg_stroke = palette.widget_hovered_stroke;
                        visuals.widgets.active.bg_fill = palette.widget_active_bg;
                        visuals.widgets.active.bg_stroke = Stroke::new(2.0_f32, palette.accent_primary);
                        visuals.selection.bg_fill = palette.accent_primary;
                        visuals.slider_trailing_fill = true;
                        *ui.visuals_mut() = visuals;

                        // Max Chain Legs
                        ui.label(RichText::new("Max Legs:").strong());
                        ui.add(
                            egui::Slider::new(&mut self.chain_max_legs, 1..=8)
                                .text("legs")
                                .clamp_to_range(true),
                        );

                        ui.separator();

                        // Max Destination Loads slider (NPC supply & demand cap)
                        ui.label(RichText::new("Max Dest Loads:").strong());
                        ui.add(
                            egui::Slider::new(&mut self.chain_max_dest_loads, 1..=3)
                                .text("loads/station")
                                .clamp_to_range(true),
                        )
                        .on_hover_text("Max deliveries of the same commodity to a single destination station (EVE NPC dynamic pricing drops after 2 loads)");

                        ui.separator();

                        // NPC commodities toggle
                        ui.checkbox(&mut self.chain_include_npc, RichText::new("NPC Commodities").strong())
                            .on_hover_text("Include seeded NPC trade commodities (e.g. Spirits, Wheat, Toxic Waste). When unchecked, only player market orders are considered.");

                        ui.separator();

                        // En-route detour toggle
                        ui.checkbox(&mut self.chain_detour_enabled, RichText::new("En-Route Pickups").strong());

                        // Detour Tolerance slider
                        ui.add_enabled(
                            self.chain_detour_enabled,
                            egui::Slider::new(&mut self.chain_max_detour, 0..=5)
                                .text("max detour jumps")
                                .clamp_to_range(true),
                        );
                    });

                    if !self.chain_detour_enabled {
                        ui.label(RichText::new("(Strict on-route systems only)").color(Color32::GRAY));
                    }

                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "[x] Finite Stock & Supply Caps (≤{} loads/dest)",
                            self.chain_max_dest_loads
                        ))
                        .color(Color32::from_rgb(0, 255, 140))
                        .size(11.5)
                        .strong(),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.chain_is_running {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(
                                    RichText::new("Optimizing chains...")
                                        .color(Color32::from_rgb(0, 220, 255))
                                        .strong(),
                                );
                            });
                        } else {
                            let can_search = !self.deals.is_empty();
                            let btn = egui::Button::new(
                                RichText::new("▶ FIND CHAIN ROUTES")
                                    .color(if palette.dark_mode { Color32::BLACK } else { Color32::WHITE })
                                    .strong(),
                            )
                            .fill(if can_search {
                                palette.accent_primary
                            } else {
                                palette.card_sub_bg
                            });

                            let resp = ui.add_enabled(can_search, btn);
                            if resp.clicked() {
                                self.start_chain_search();
                            }
                            if !can_search {
                                resp.on_hover_text("Load market data first by clicking 'SCAN MARKET NOW' in the header");
                            }
                        }
                    });
                });
            });

        ui.add_space(10.0);

        // 2. Empty state when no deals
        if self.deals.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.heading(
                    RichText::new("No Market Deals Loaded")
                        .size(22.0)
                        .color(palette.text_secondary),
                );
                ui.label(
                    RichText::new("Run a market scan first by clicking 'SCAN MARKET NOW' so the optimizer has trade deals to chain.")
                        .size(14.0)
                        .color(palette.text_muted),
                );
            });
            return;
        }

        // 3. Results Section
        if self.chain_results.is_empty() && !self.chain_is_running {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.heading(
                    RichText::new("No Chain Routes Generated Yet")
                        .size(22.0)
                        .color(palette.text_secondary),
                );
                ui.label(
                    RichText::new("Configure your max legs and detour tolerance above, then click 'FIND CHAIN ROUTES'.")
                        .size(14.0)
                        .color(palette.text_muted),
                );
            });
            return;
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut new_selected_idx: Option<usize> = None;

            // Discovered Chains Selector Strip
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("Top Ranked Chains ({} found):", self.chain_results.len()))
                        .strong()
                        .color(palette.text_secondary),
                );
                ui.separator();

                ui.label(RichText::new("Active Route:").strong());
                let current_idx = self.chain_selected_idx.unwrap_or(0);
                let selected_label = if let Some(r) = self.chain_results.get(current_idx) {
                    format!("#{} (+{:.2}M ISK, {}j, {} legs)", current_idx + 1, r.total_profit / 1_000_000.0, r.total_jumps, r.legs.len())
                } else {
                    "None".to_string()
                };

                egui::ComboBox::from_id_source("chain_route_dropdown_selector")
                    .selected_text(selected_label)
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for (idx, r) in self.chain_results.iter().enumerate() {
                            let label = format!("#{} (+{:.2}M ISK, {}j, {} legs)", idx + 1, r.total_profit / 1_000_000.0, r.total_jumps, r.legs.len());
                            let mut sel = current_idx;
                            if ui.selectable_value(&mut sel, idx, label).clicked() {
                                new_selected_idx = Some(idx);
                            }
                        }
                    });
            });

            ui.add_space(6.0);

            // Horizontal card strip for selecting routes
            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (idx, route) in self.chain_results.iter().enumerate() {
                        let is_selected = self.chain_selected_idx == Some(idx);

                        ui.push_id(idx, |ui| {
                            let bg_color = if is_selected {
                                palette.card_sub_bg
                            } else {
                                palette.card_bg
                            };
                            let border_color = if is_selected {
                                palette.accent_primary
                            } else {
                                palette.card_stroke.color
                            };

                            let frame = egui::Frame::none()
                                .fill(bg_color)
                                .stroke(Stroke::new(if is_selected { 1.5_f32 } else { 1.0_f32 }, border_color))
                                .rounding(6.0)
                                .inner_margin(egui::Margin::symmetric(10.0, 8.0));

                            let frame_resp = frame.show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        let rank_text = format!("#{}", idx + 1);
                                        let rank_color = match idx {
                                            0 => Color32::from_rgb(255, 215, 0),
                                            1 => Color32::from_rgb(200, 205, 215),
                                            2 => Color32::from_rgb(205, 127, 50),
                                            _ => Color32::from_rgb(140, 160, 190),
                                        };
                                        ui.label(RichText::new(rank_text).strong().color(rank_color));

                                        ui.label(
                                            RichText::new(format!("+{:.2}M ISK", route.total_profit / 1_000_000.0))
                                                .strong()
                                                .color(Color32::from_rgb(0, 255, 140)),
                                        );
                                    });

                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(format!("{} jumps", route.total_jumps)).color(Color32::from_rgb(255, 205, 0)).size(11.0));
                                        ui.label(RichText::new(format!("{:.2}M/j", route.profit_per_jump / 1_000_000.0)).color(Color32::from_rgb(0, 220, 255)).size(11.0));
                                        ui.label(RichText::new(format!("{} legs", route.legs.len())).color(palette.text_muted).size(11.0));
                                    });
                                });
                            });

                            let card_rect = frame_resp.response.rect;
                            let card_interact = ui.interact(card_rect, ui.id(), egui::Sense::click());
                            if card_interact.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                if !is_selected {
                                    ui.painter().rect_stroke(
                                        card_rect,
                                        6.0,
                                        Stroke::new(1.0_f32, palette.accent_hover),
                                    );
                                }
                            }
                            if card_interact.clicked() {
                                new_selected_idx = Some(idx);
                            }
                        });
                    }
                });
            });

            if let Some(idx) = new_selected_idx {
                self.chain_selected_idx = Some(idx);
            }

            ui.add_space(14.0);

            // Detailed Breakdown of Selected Route
            if let Some(selected_idx) = self.chain_selected_idx {
                if let Some(route) = self.chain_results.get(selected_idx).cloned() {
                    let can_split = ui.available_width() >= 1150.0;
                    // Top header strip for selected route with Split / Stacked view switcher
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("Chain Routes #{}: Visual Stops Flowchart & Route Dossier", selected_idx + 1))
                                .size(16.0)
                                .color(palette.accent_primary)
                                .strong(),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let toggle_label = if self.chain_flowchart_split && can_split {
                                "◫ View: Split (Flowchart Left)"
                            } else {
                                "▬ View: Stacked (Full Width)"
                            };
                            let toggle_btn = egui::Button::new(
                                RichText::new(toggle_label)
                                    .size(11.5)
                                    .color(palette.accent_primary)
                                    .strong(),
                            )
                            .fill(palette.card_sub_bg);

                            let resp = ui.add_enabled(can_split, toggle_btn);
                            if resp.clicked() {
                                self.chain_flowchart_split = !self.chain_flowchart_split;
                                if self.chain_flowchart_split {
                                    self.set_toast("View: Split (Flowchart on Left, Dossier on Right)");
                                } else {
                                    self.set_toast("View: Stacked (Full Width Flowchart & Dossier)");
                                }
                            }
                            if !can_split {
                                resp.on_hover_text("Split view requires at least 1150px window width; stacked mode is active");
                            }
                        });
                    });

                    ui.add_space(8.0);

                    if self.chain_flowchart_split && can_split {
                        ui.columns(2, |cols| {
                            self.render_chain_flowchart(&mut cols[0], &route);
                            self.render_chain_dossier_as_is(&mut cols[1], &route, selected_idx);
                        });
                    } else {
                        self.render_chain_flowchart(ui, &route);
                        ui.add_space(14.0);
                        self.render_chain_dossier_as_is(ui, &route, selected_idx);
                    }
                }
            }
        });
    }

    /// Interactive visual flowchart of stops along the chained trade route.
    fn render_chain_flowchart(
        &mut self,
        ui: &mut egui::Ui,
        route: &crate::chain_route_planner::ChainRoute,
    ) {
        let palette = self.theme.palette();
        // Flowchart Header Frame
        palette.card_frame().inner_margin(10.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Visual Stops Flowchart")
                        .size(14.0)
                        .color(palette.accent_primary)
                        .strong(),
                );
                ui.label(
                    RichText::new("Flight nodes with in-game quick-copy buttons")
                        .size(11.0)
                        .color(palette.text_muted),
                );
            });
        });

        ui.add_space(8.0);

        let mut stop_idx = 1;

        // ── 1. Start / Departure Node ──
        let home_id = self
            .sde
            .as_ref()
            .and_then(|s| s.get_system_id(&self.home_system));
        let home_sec = home_id
            .and_then(|id| self.sde.as_ref().and_then(|s| s.get_system_security(id)))
            .unwrap_or(1.0);
        let home_sec_col = security_color(home_sec);

        egui::Frame::none()
            .fill(palette.card_sub_bg)
            .stroke(Stroke::new(1.2_f32, palette.accent_primary))
            .rounding(6.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("[START]")
                            .size(13.0)
                            .color(palette.accent_primary)
                            .strong(),
                    );
                    ui.label(
                        RichText::new("DEPARTURE POINT")
                            .size(13.0)
                            .strong()
                            .color(palette.text_primary),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let home_copy = self.home_system.clone();
                        let sys_btn = egui::Button::new(
                            RichText::new("System")
                                .size(11.0)
                                .color(if palette.dark_mode {
                                    Color32::BLACK
                                } else {
                                    Color32::WHITE
                                })
                                .strong(),
                        )
                        .fill(palette.accent_primary);
                        if ui.add(sys_btn).clicked() {
                            copy_to_clipboard(ui, home_copy.clone());
                            self.set_toast(format!("Copied start system '{}'", home_copy));
                        }
                    });
                });

                ui.add_space(3.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("[{:.1}]", home_sec))
                            .size(12.0)
                            .color(home_sec_col)
                            .strong(),
                    );
                    ui.label(
                        RichText::new(&self.home_system)
                            .size(13.0)
                            .color(palette.text_secondary)
                            .strong(),
                    );
                    ui.label(
                        RichText::new("• Starting system location")
                            .size(11.0)
                            .color(palette.text_muted),
                    );
                });
            });

        // ── Helper closure to draw flowchart connector ──
        let render_connector = |ui: &mut egui::Ui, jumps: u32, is_empty_reposition: bool| {
            ui.vertical_centered(|ui| {
                ui.add_space(3.0);
                let line_color = if is_empty_reposition && jumps > 0 {
                    Color32::from_rgb(255, 170, 0)
                } else {
                    palette.accent_primary
                };
                ui.label(RichText::new("│").size(13.0).color(line_color));
                let (jump_text, stroke_color, text_color) = if is_empty_reposition {
                    if jumps == 0 {
                        (
                            "Seamless Station Trade (0 empty jumps)".to_string(),
                            palette.success_green,
                            palette.success_green,
                        )
                    } else {
                        (
                            format!("{} Empty Reposition Jump(s) • Deadhead", jumps),
                            palette.warning_yellow,
                            palette.warning_yellow,
                        )
                    }
                } else if jumps == 0 {
                    (
                        "Intra-System Delivery (0 jumps)".to_string(),
                        palette.accent_primary,
                        palette.accent_primary,
                    )
                } else {
                    (
                        format!("{} Haul Jump(s) • Cargo On Board", jumps),
                        palette.accent_primary,
                        palette.text_secondary,
                    )
                };
                egui::Frame::none()
                    .fill(palette.card_sub_bg)
                    .stroke(Stroke::new(1.0_f32, stroke_color))
                    .rounding(10.0)
                    .inner_margin(egui::Margin::symmetric(10.0, 3.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(jump_text)
                                .size(11.0)
                                .color(text_color)
                                .strong(),
                        );
                    });
                ui.label(RichText::new("▼").size(13.0).color(line_color));
                ui.add_space(3.0);
            });
        };

        // Initial connector from start to first pickup stop
        let first_jumps = route
            .legs
            .first()
            .and_then(|l| l.pickups.first())
            .map(|p| p.jumps_from_prev)
            .unwrap_or(0);
        render_connector(ui, first_jumps, true);

        // ── 2. Sequential Stops Across Legs ──
        for (leg_idx, leg) in route.legs.iter().enumerate() {
            // Pickup Stops
            for (pickup_idx, pickup) in leg.pickups.iter().enumerate() {
                let is_detour = pickup.detour_jumps > 0;
                let stroke_col = if is_detour {
                    Color32::from_rgb(241, 196, 15) // Gold
                } else {
                    Color32::from_rgb(0, 220, 140) // Emerald green
                };

                let sec = pickup
                    .cargo_loaded
                    .first()
                    .map(|s| s.deal.source_security)
                    .or_else(|| {
                        self.sde
                            .as_ref()
                            .and_then(|s| s.get_system_security(pickup.system_id))
                    })
                    .unwrap_or(1.0);
                let sec_col = security_color(sec);

                let primary_station = pickup
                    .cargo_loaded
                    .first()
                    .map(|s| s.deal.source_station_name.clone())
                    .unwrap_or_default();

                egui::Frame::none()
                    .fill(palette.card_bg)
                    .stroke(Stroke::new(1.3_f32, stroke_col))
                    .rounding(6.0)
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        // Header line
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("[STOP #{}]", stop_idx))
                                    .size(13.0)
                                    .color(palette.accent_primary)
                                    .strong(),
                            );
                            let badge_text = if is_detour {
                                format!("BUY STOP (En-Route +{}j)", pickup.detour_jumps)
                            } else {
                                "BUY STOP".to_string()
                            };
                            ui.label(
                                RichText::new(badge_text)
                                    .size(13.0)
                                    .color(stroke_col)
                                    .strong(),
                            );

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if !primary_station.is_empty() {
                                        let st_copy = primary_station.clone();
                                        let st_btn = egui::Button::new(
                                            RichText::new("Station")
                                                .size(11.0)
                                                .color(palette.text_primary)
                                                .strong(),
                                        )
                                        .fill(palette.card_sub_bg);
                                        if ui.add(st_btn).clicked() {
                                            copy_to_clipboard(ui, st_copy.clone());
                                            self.set_toast(format!("Copied station '{}'", st_copy));
                                        }
                                    }
                                    let sys_copy = pickup.system_name.clone();
                                    let sys_btn = egui::Button::new(
                                        RichText::new("System")
                                            .size(11.0)
                                            .color(palette.accent_primary_text)
                                            .strong(),
                                    )
                                    .fill(palette.accent_primary);
                                    if ui.add(sys_btn).clicked() {
                                        copy_to_clipboard(ui, sys_copy.clone());
                                        self.set_toast(format!("Copied system '{}'", sys_copy));
                                    }
                                },
                            );
                        });

                        ui.add_space(3.0);

                        // Location info
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(format!("[{:.1}]", sec))
                                    .size(12.0)
                                    .color(sec_col)
                                    .strong(),
                            );
                            ui.label(
                                RichText::new(&pickup.system_name)
                                    .size(13.0)
                                    .color(palette.accent_primary)
                                    .strong(),
                            );
                            if !primary_station.is_empty() {
                                ui.label(
                                    RichText::new(format!("• {}", primary_station))
                                        .size(11.0)
                                        .color(palette.text_muted),
                                );
                            }
                        });

                        ui.add_space(5.0);
                        ui.separator();
                        ui.add_space(3.0);

                        // Cargo items to BUY
                        for slot in &pickup.cargo_loaded {
                            ui.horizontal(|ui| {
                                // Green BUY Pill
                                egui::Frame::none()
                                    .fill(Color32::from_rgba_unmultiplied(
                                        palette.success_green.r(),
                                        palette.success_green.g(),
                                        palette.success_green.b(),
                                        if palette.dark_mode { 36 } else { 24 },
                                    ))
                                    .stroke(Stroke::new(1.0_f32, palette.success_green))
                                    .rounding(3.0)
                                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new("BUY")
                                                .size(11.0)
                                                .color(palette.success_green)
                                                .strong(),
                                        );
                                    });

                                ui.label(
                                    RichText::new(format!(
                                        "{}x {}",
                                        slot.deal.quantity, slot.deal.item_name
                                    ))
                                    .size(12.5)
                                    .color(palette.text_primary)
                                    .strong(),
                                );

                                let item_copy = slot.deal.item_name.clone();
                                let item_btn = egui::Button::new(
                                    RichText::new("Item")
                                        .size(11.0)
                                        .color(palette.accent_secondary_text)
                                        .strong(),
                                )
                                .fill(palette.accent_secondary);
                                if ui.add(item_btn).clicked() {
                                    copy_to_clipboard(ui, item_copy.clone());
                                    self.set_toast(format!("Copied item '{}'", item_copy));
                                }
                            });

                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "Cost: {:.2}M ISK",
                                        slot.capital / 1_000_000.0
                                    ))
                                    .size(11.0)
                                    .color(palette.text_secondary),
                                );
                                ui.label(
                                    RichText::new(format!("• Vol: {:.1} m³", slot.volume))
                                        .size(11.0)
                                        .color(palette.accent_primary),
                                );
                                ui.label(
                                    RichText::new(format!("• → Deliver to {}", slot.sell_name))
                                        .size(11.0)
                                        .color(palette.accent_primary),
                                );
                                if slot.deal.source_order_volume_remain > 0 {
                                    ui.label(
                                        RichText::new(format!(
                                            "• Stock: {}/{} avail",
                                            slot.deal.quantity,
                                            slot.deal.source_order_volume_remain
                                        ))
                                        .size(11.0)
                                        .color(palette.text_muted),
                                    );
                                }
                                ui.label(
                                    RichText::new(format!(
                                        "(+{:.2}M ISK net)",
                                        slot.net_profit / 1_000_000.0
                                    ))
                                    .size(11.0)
                                    .color(palette.success_green),
                                );
                            });

                            // If item has a specific station different from primary
                            if !slot.deal.source_station_name.is_empty()
                                && slot.deal.source_station_name != primary_station
                            {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "Station: {}",
                                            slot.deal.source_station_name
                                        ))
                                        .size(10.5)
                                        .color(Color32::GRAY),
                                    );
                                    let st_copy = slot.deal.source_station_name.clone();
                                    if ui.small_button("Station").clicked() {
                                        copy_to_clipboard(ui, st_copy.clone());
                                        self.set_toast(format!("Copied station '{}'", st_copy));
                                    }
                                });
                            }

                            ui.add_space(4.0);
                        }
                    });

                stop_idx += 1;

                // Connector to next stop: next pickup or leg's sell stop
                let next_jumps = if pickup_idx + 1 < leg.pickups.len() {
                    leg.pickups[pickup_idx + 1].jumps_from_prev
                } else {
                    leg.sell.jumps_from_prev
                };
                render_connector(ui, next_jumps, false);
            }

            // Sell Stop
            let sec = leg
                .sell
                .cargo_sold
                .first()
                .map(|s| s.deal.dest_security)
                .or_else(|| {
                    self.sde
                        .as_ref()
                        .and_then(|s| s.get_system_security(leg.sell.system_id))
                })
                .unwrap_or(1.0);
            let sec_col = security_color(sec);

            let sell_station = leg
                .sell
                .cargo_sold
                .first()
                .map(|s| s.deal.dest_station_name.clone())
                .unwrap_or_default();

            egui::Frame::none()
                .fill(palette.card_bg)
                .stroke(Stroke::new(1.3_f32, palette.accent_primary))
                .rounding(6.0)
                .inner_margin(10.0)
                .show(ui, |ui| {
                    // Header line
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("[STOP #{}]", stop_idx))
                                .size(13.0)
                                .color(palette.accent_primary)
                                .strong(),
                        );
                        ui.label(
                            RichText::new("SELL STOP")
                                .size(13.0)
                                .color(palette.accent_primary)
                                .strong(),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !sell_station.is_empty() {
                                let st_copy = sell_station.clone();
                                let st_btn = egui::Button::new(
                                    RichText::new("Station")
                                        .size(11.0)
                                        .color(palette.text_primary)
                                        .strong(),
                                )
                                .fill(palette.card_sub_bg);
                                if ui.add(st_btn).clicked() {
                                    copy_to_clipboard(ui, st_copy.clone());
                                    self.set_toast(format!("Copied station '{}'", st_copy));
                                }
                            }
                            let sys_copy = leg.sell.system_name.clone();
                            let sys_btn = egui::Button::new(
                                RichText::new("System")
                                    .size(11.0)
                                    .color(palette.accent_primary_text)
                                    .strong(),
                            )
                            .fill(palette.accent_primary);
                            if ui.add(sys_btn).clicked() {
                                copy_to_clipboard(ui, sys_copy.clone());
                                self.set_toast(format!("Copied system '{}'", sys_copy));
                            }
                        });
                    });

                    ui.add_space(3.0);

                    // Location info
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("[{:.1}]", sec))
                                .size(12.0)
                                .color(sec_col)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(&leg.sell.system_name)
                                .size(13.0)
                                .color(palette.success_green)
                                .strong(),
                        );
                        if !sell_station.is_empty() {
                            ui.label(
                                RichText::new(format!("• {}", sell_station))
                                    .size(11.0)
                                    .color(palette.text_muted),
                            );
                        }
                    });

                    ui.add_space(5.0);
                    ui.separator();
                    ui.add_space(3.0);

                    // Cargo items SOLD
                    for slot in &leg.sell.cargo_sold {
                        ui.horizontal(|ui| {
                            // Themed SELL Pill
                            egui::Frame::none()
                                .fill(Color32::from_rgba_unmultiplied(
                                    palette.accent_primary.r(),
                                    palette.accent_primary.g(),
                                    palette.accent_primary.b(),
                                    if palette.dark_mode { 36 } else { 24 },
                                ))
                                .stroke(Stroke::new(1.0_f32, palette.accent_primary))
                                .rounding(3.0)
                                .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new("SELL")
                                            .size(11.0)
                                            .color(palette.accent_primary)
                                            .strong(),
                                    );
                                });

                            ui.label(
                                RichText::new(format!(
                                    "{}x {}",
                                    slot.deal.quantity, slot.deal.item_name
                                ))
                                .size(12.5)
                                .color(palette.text_primary)
                                .strong(),
                            );

                            let item_copy = slot.deal.item_name.clone();
                            let item_btn = egui::Button::new(
                                RichText::new("Item")
                                    .size(11.0)
                                    .color(palette.accent_secondary_text)
                                    .strong(),
                            )
                            .fill(palette.accent_secondary);
                            if ui.add(item_btn).clicked() {
                                copy_to_clipboard(ui, item_copy.clone());
                                self.set_toast(format!("Copied item '{}'", item_copy));
                            }
                        });

                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "Net Profit: +{:.2}M ISK (+{:.1}% ROI)",
                                    slot.net_profit / 1_000_000.0,
                                    slot.deal.net_profit_pct
                                ))
                                .size(11.0)
                                .color(palette.success_green)
                                .strong(),
                            );
                            ui.label(
                                RichText::new(format!("• Freed: {:.1} m³", slot.volume))
                                    .size(11.0)
                                    .color(palette.text_muted),
                            );
                        });

                        ui.add_space(4.0);
                    }
                });

            stop_idx += 1;

            // Connector to next leg's first pickup (if there is a next leg)
            if leg_idx + 1 < route.legs.len() {
                let next_leg_jumps = route.legs[leg_idx + 1]
                    .pickups
                    .first()
                    .map(|p| p.jumps_from_prev)
                    .unwrap_or(0);
                render_connector(ui, next_leg_jumps, true);
            }
        }

        // ── 3. Route Finish Node ──
        ui.vertical_centered(|ui| {
            ui.add_space(3.0);
            ui.label(RichText::new("│").size(13.0).color(palette.success_green));
            ui.label(RichText::new("▼").size(13.0).color(palette.success_green));
            ui.add_space(3.0);
        });

        egui::Frame::none()
            .fill(palette.card_sub_bg)
            .stroke(Stroke::new(1.2_f32, palette.success_green))
            .rounding(6.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("[FINISH]")
                            .size(13.0)
                            .color(palette.success_green)
                            .strong(),
                    );
                    ui.label(
                        RichText::new("CHAIN ROUTE COMPLETED")
                            .size(13.0)
                            .color(palette.text_primary)
                            .strong(),
                    );
                });
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "Total Profit: +{:.2}M ISK",
                            route.total_profit / 1_000_000.0
                        ))
                        .strong()
                        .color(Color32::from_rgb(0, 255, 140)),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!("Total Jumps: {}", route.total_jumps))
                            .strong()
                            .color(Color32::from_rgb(255, 205, 0)),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!("Overall Net ROI: +{:.1}%", route.overall_roi_pct))
                            .strong()
                            .color(Color32::from_rgb(0, 240, 180)),
                    );
                });
            });
    }

    /// Renders the existing comprehensive route dossier, leg timeline & cargo tracking as-is.
    fn render_chain_dossier_as_is(
        &mut self,
        ui: &mut egui::Ui,
        route: &crate::chain_route_planner::ChainRoute,
        selected_idx: usize,
    ) {
        let palette = self.theme.palette();
        palette.card_frame().inner_margin(12.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        RichText::new(format!(
                            "Chain Routes #{}: Complete Visual Flight Dossier",
                            selected_idx + 1
                        ))
                        .color(palette.accent_primary)
                        .strong(),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let push_btn = palette.primary_button(
                            RichText::new(if self.sso_is_pushing {
                                "⏳ Pushing to EVE..."
                            } else {
                                "▶ Push Route to EVE Autopilot"
                            })
                            .color(palette.accent_primary_text)
                            .strong(),
                        );

                        if ui
                            .add_enabled(!self.sso_is_pushing, push_btn)
                            .on_hover_text("Directly inject all station stops into your live running EVE Online Autopilot HUD")
                            .clicked()
                        {
                            let dest_ids = route.ordered_destination_ids();
                            self.push_to_eve_autopilot(dest_ids, "Chain Route");
                        }

                        let sheet_btn = palette.secondary_button(
                            RichText::new("Copy EVE Flight Sheet (Alt+N)")
                                .color(palette.accent_secondary_text)
                                .strong(),
                        );

                        if ui
                            .add(sheet_btn)
                            .on_hover_text("Copy formatted Flight Sheet with clickable in-game station links for EVE Notepad (Alt+N)")
                            .clicked()
                        {
                            let sheet = route.format_eve_flight_sheet(selected_idx + 1);
                            copy_to_clipboard(ui, sheet);
                            self.set_toast("Copied EVE Flight Sheet! Paste into in-game Notepad (Alt+N).");
                        }

                        let copy_btn = egui::Button::new(
                            RichText::new("Copy Waypoints")
                                .color(palette.text_primary)
                                .strong(),
                        )
                        .fill(palette.card_sub_bg);

                        if ui.add(copy_btn).clicked() {
                            let trail = route.waypoint_names.join(" -> ");
                            copy_to_clipboard(ui, trail);
                            self.set_toast("Copied chained autopilot waypoints to clipboard!");
                        }
                    });
                });

                ui.add_space(8.0);

                // Route Telemetry Badges
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("Total Jumps: {}", route.total_jumps))
                            .strong()
                            .color(palette.text_secondary),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "Total Net Profit: +{:.2}M ISK",
                            route.total_profit / 1_000_000.0
                        ))
                        .strong()
                        .color(palette.success_green),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!("Overall Net ROI: +{:.1}%", route.overall_roi_pct))
                            .strong()
                            .color(palette.success_green),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "Profit / Jump: {:.2}M ISK",
                            route.profit_per_jump / 1_000_000.0
                        ))
                        .strong()
                        .color(palette.accent_secondary),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "Capital Invested: {:.2}M ISK",
                            route.total_capital / 1_000_000.0
                        ))
                        .color(palette.info_blue),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!("Peak Cargo Hold: {:.1} m³", route.peak_volume_m3))
                            .color(palette.accent_primary),
                    );
                });
            });

        ui.add_space(10.0);
        ui.label(
            RichText::new("Step-by-Step Leg Timeline & Knapsack Cargo Tracking:")
                .strong()
                .size(14.0),
        );
        ui.add_space(6.0);

        // Render Each Leg
        for (leg_idx, leg) in route.legs.iter().enumerate() {
            palette.sub_card_frame().inner_margin(12.0).show(ui, |ui| {
                let leg_empty_jumps = leg.pickups.first().map(|p| p.jumps_from_prev).unwrap_or(0);
                let leg_haul_jumps = leg.leg_total_jumps.saturating_sub(leg_empty_jumps);
                let leg_ppj = leg.leg_total_profit / (leg.leg_total_jumps.max(1) as f64);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("LEG #{}", leg_idx + 1))
                            .size(15.0)
                            .strong()
                            .color(palette.accent_primary),
                    );
                    ui.separator();
                    let jumps_str = if leg_empty_jumps > 0 {
                        format!(
                            "{} jumps ({} empty + {} haul)",
                            leg.leg_total_jumps, leg_empty_jumps, leg_haul_jumps
                        )
                    } else {
                        format!("{} jumps (direct haul)", leg.leg_total_jumps)
                    };
                    ui.label(RichText::new(jumps_str).color(Color32::from_rgb(255, 205, 0)));
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "Leg Profit: +{:.2}M ISK",
                            leg.leg_total_profit / 1_000_000.0
                        ))
                        .strong()
                        .color(Color32::from_rgb(0, 255, 140)),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!("PPJ: {:.2}M/j", leg_ppj / 1_000_000.0))
                            .strong()
                            .color(Color32::from_rgb(0, 220, 255)),
                    );
                });

                ui.add_space(8.0);

                // Pickup Stops in this leg
                for pickup in &leg.pickups {
                    let is_detour = pickup.detour_jumps > 0;
                    let badge_col = if is_detour {
                        Color32::from_rgb(241, 196, 15) // Gold
                    } else {
                        palette.accent_primary
                    };

                    let header_text = if is_detour {
                        format!(
                            "EN-ROUTE PICKUP — {} (+{} detour jumps)",
                            pickup.system_name, pickup.detour_jumps
                        )
                    } else {
                        format!("PICKUP STOP — {}", pickup.system_name)
                    };

                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(header_text).strong().color(badge_col));
                            if pickup.jumps_from_prev > 0 {
                                ui.label(
                                    RichText::new(format!(
                                        "({} jumps from prev)",
                                        pickup.jumps_from_prev
                                    ))
                                    .color(palette.text_muted),
                                );
                            }
                        });

                        ui.add_space(4.0);

                        // Item Slots
                        for slot in &pickup.cargo_loaded {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "+ {}x {}",
                                        slot.deal.quantity, slot.deal.item_name
                                    ))
                                    .strong()
                                    .color(palette.text_primary),
                                );
                                ui.label(
                                    RichText::new(format!("{:.1} m³", slot.volume))
                                        .color(Color32::from_rgb(255, 140, 240)),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "{:.2}M ISK",
                                        slot.capital / 1_000_000.0
                                    ))
                                    .color(palette.text_secondary),
                                );
                                ui.label(
                                    RichText::new(format!("→ Sell at {}", slot.sell_name))
                                        .color(palette.accent_primary),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "(+{:.2}M ISK net)",
                                        slot.net_profit / 1_000_000.0
                                    ))
                                    .size(11.0)
                                    .color(Color32::from_rgb(0, 255, 140)),
                                );
                            });
                        }

                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(format!(
                                "Stop Totals: Loaded {:.1} m³ · Spent {:.2}M ISK",
                                pickup.volume_loaded,
                                pickup.capital_spent / 1_000_000.0
                            ))
                            .size(11.0)
                            .color(palette.text_muted),
                        );
                    });

                    ui.add_space(4.0);
                }

                // Flight Arrow
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "▼ Fly {} jump(s) to destination",
                            leg.sell.jumps_from_prev
                        ))
                        .color(Color32::from_rgb(255, 205, 0))
                        .strong(),
                    );
                });

                ui.add_space(4.0);

                // Sell Stop
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("SELL DESTINATION — {}", leg.sell.system_name))
                                .strong()
                                .color(palette.success_green),
                        );
                    });

                    ui.add_space(4.0);

                    for slot in &leg.sell.cargo_sold {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "Sold {}x {}",
                                    slot.deal.quantity, slot.deal.item_name
                                ))
                                .color(palette.text_primary),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "+{:.2}M ISK net profit",
                                    slot.net_profit / 1_000_000.0
                                ))
                                .color(Color32::from_rgb(0, 255, 140))
                                .strong(),
                            );
                            ui.label(
                                RichText::new(format!("(freed {:.1} m³)", slot.volume))
                                    .color(palette.text_muted),
                            );
                        });
                    }

                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(format!(
                            "Sell Totals: Realized +{:.2}M ISK · Cargo Freed: {:.1} m³",
                            leg.sell.total_profit / 1_000_000.0,
                            leg.sell.volume_freed
                        ))
                        .size(11.0)
                        .color(Color32::from_rgb(0, 230, 170))
                        .strong(),
                    );
                });
            });

            ui.add_space(8.0);
        }

        // Autopilot Waypoint List
        ui.add_space(8.0);
        ui.label(RichText::new("Chained Autopilot Waypoints Sequence:").strong());
        ui.label(
            RichText::new(route.waypoint_names.join(" → "))
                .size(13.0)
                .color(palette.accent_primary),
        );
    }

    fn render_hub_arbitrage_tab(&mut self, ui: &mut egui::Ui) {
        let palette = self.theme.palette();
        if self.deals.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.heading(
                    RichText::new("No Market Deals Loaded")
                        .size(22.0)
                        .color(palette.text_secondary),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Scan the 5 empire trade hubs to discover inter-hub arbitrage opportunities.")
                        .size(14.0)
                        .color(palette.text_muted),
                );
                ui.add_space(16.0);
                let btn = palette.primary_button(
                    RichText::new("▶ Scan All 5 Core Hubs")
                        .size(16.0)
                        .color(palette.accent_primary_text)
                        .strong(),
                )
                .min_size(Vec2::new(260.0, 42.0));

                if ui.add(btn).clicked() {
                    self.region_the_forge = true;
                    self.region_domain = true;
                    self.region_sinq_laison = true;
                    self.region_heimatar = true;
                    self.region_metropolis = true;
                    self.start_scan();
                }
            });
            return;
        }

        let mut do_scan_all = false;
        let mut do_pack_cargo = false;
        let mut do_export_csv = false;
        let mut action_toast: Option<String> = None;
        let mut toggle_select: Option<usize> = None;
        let mut change_sort: Option<(DealSortBy, bool)> = None;
        let mut push_dest_ids: Option<(Vec<u64>, &'static str)> = None;

        // 1. Controls Header Card
        palette.card_frame().inner_margin(12.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        RichText::new("Core Hub Arbitrage — Inter-Hub Direct Trade Scout")
                            .color(palette.accent_primary)
                            .strong(),
                    );
                    ui.label(
                        RichText::new("Trade directly between EVE's 5 major empire trade hubs")
                            .color(palette.text_muted)
                            .size(12.0),
                    );
                });

                ui.add_space(8.0);

                // Row 1: Hub Corridor Selectors & Scan/Pack/Export buttons
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Origin Hub:").strong());
                    let src_name = self.hub_source_filter.map(|h| h.name()).unwrap_or("Any Hub (Wildcard)");
                    egui::ComboBox::from_id_source("hub_source_combo")
                        .selected_text(src_name)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.hub_source_filter, None, "Any Hub (Wildcard)");
                            for &hub in &CoreHub::ALL {
                                ui.selectable_value(
                                    &mut self.hub_source_filter,
                                    Some(hub),
                                    format!("{} ({})", hub.name(), hub.region_name()),
                                );
                            }
                        });

                    if ui.button(RichText::new("⇄ Swap").strong()).on_hover_text("Swap origin and destination hubs").clicked() {
                        std::mem::swap(&mut self.hub_source_filter, &mut self.hub_dest_filter);
                    }

                    ui.label(RichText::new("Destination Hub:").strong());
                    let dst_name = self.hub_dest_filter.map(|h| h.name()).unwrap_or("Any Hub (Wildcard)");
                    egui::ComboBox::from_id_source("hub_dest_combo")
                        .selected_text(dst_name)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.hub_dest_filter, None, "Any Hub (Wildcard)");
                            for &hub in &CoreHub::ALL {
                                ui.selectable_value(
                                    &mut self.hub_dest_filter,
                                    Some(hub),
                                    format!("{} ({})", hub.name(), hub.region_name()),
                                );
                            }
                        });

                    ui.separator();

                    let scan_btn = palette.primary_button(
                        RichText::new("▶ Scan All 5 Hubs")
                            .color(palette.accent_primary_text)
                            .strong(),
                    );
                    if ui.add(scan_btn)
                        .on_hover_text("Select all 5 trade hub regions (Forge, Domain, Sinq Laison, Heimatar, Metropolis) and fetch fresh market orders")
                        .clicked()
                    {
                        do_scan_all = true;
                    }

                    let pack_btn = palette.secondary_button(
                        RichText::new("Pack Ship Cargo")
                            .color(palette.accent_secondary_text)
                            .strong(),
                    );
                    if ui.add(pack_btn)
                        .on_hover_text("Use knapsack optimization to pack your active ship cargo with the most profitable items in this corridor")
                        .clicked()
                    {
                        do_pack_cargo = true;
                    }

                    let export_btn = egui::Button::new(
                        RichText::new("Export CSV")
                            .color(palette.text_secondary)
                            .strong(),
                    )
                    .fill(palette.card_sub_bg);
                    if ui.add(export_btn)
                        .on_hover_text("Export current filtered hub deals to ./hub_arbitrage_export.csv and clipboard")
                        .clicked()
                    {
                        do_export_csv = true;
                    }
                });

                ui.add_space(6.0);

                // Row 2: Secondary Filters & Search
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut self.hub_include_npc, RichText::new("NPC Commodities").strong())
                        .on_hover_text("Include seeded NPC trade commodities (e.g. Spirits, Wheat). When unchecked, only player market orders are considered.");

                    ui.separator();

                    ui.checkbox(&mut self.hub_major_stations_only, RichText::new("Major Stations Only").strong())
                        .on_hover_text("Restrict trades strictly to the primary trade stations (Jita 4-4, Amarr VIII, Dodixie IX, Rens VI, Hek VIII). Excludes secondary stations in these systems.");

                    ui.separator();

                    ui.label(RichText::new("Min Profit:").strong());
                    ui.add(
                        egui::Slider::new(&mut self.hub_min_profit_m, 0.1..=50.0)
                            .suffix("M ISK")
                            .step_by(0.5)
                            .clamp_to_range(true),
                    );

                    ui.separator();

                    ui.label(RichText::new("Search:").strong());
                    ui.add(egui::TextEdit::singleline(&mut self.hub_search_query).desired_width(160.0))
                        .on_hover_text("Search by item name or station name");
                    if !self.hub_search_query.is_empty() && ui.button("✕ Clear").clicked() {
                        self.hub_search_query.clear();
                    }
                });
            });

        ui.add_space(6.0);

        let opts = hub_arbitrage::HubFilterOptions {
            source_hub: self.hub_source_filter,
            dest_hub: self.hub_dest_filter,
            include_npc: self.hub_include_npc,
            major_stations_only: self.hub_major_stations_only,
            min_profit: self.hub_min_profit_m * 1_000_000.0,
            search_query: self.hub_search_query.clone(),
        };

        let current_hub_key = (
            self.hub_source_filter,
            self.hub_dest_filter,
            self.hub_include_npc,
            self.hub_major_stations_only,
            self.hub_min_profit_m.to_bits(),
            self.hub_search_query.clone(),
            self.hub_sort_by,
            self.hub_sort_descending,
            self.deals.len(),
        );

        if self.cached_hub_key != current_hub_key {
            let mut filtered: Vec<crate::arbitrage::ArbitrageDeal> =
                hub_arbitrage::filter_hub_deals(&self.deals, &opts)
                    .into_iter()
                    .cloned()
                    .collect();

            // Sorting
            match self.hub_sort_by {
                DealSortBy::ProfitPerJump => filtered.sort_by(|a, b| {
                    if self.hub_sort_descending {
                        b.profit_per_jump
                            .partial_cmp(&a.profit_per_jump)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        a.profit_per_jump
                            .partial_cmp(&b.profit_per_jump)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }
                }),
                DealSortBy::TotalProfit => filtered.sort_by(|a, b| {
                    if self.hub_sort_descending {
                        b.net_profit
                            .partial_cmp(&a.net_profit)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        a.net_profit
                            .partial_cmp(&b.net_profit)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }
                }),
                DealSortBy::RoiPct => filtered.sort_by(|a, b| {
                    if self.hub_sort_descending {
                        b.net_profit_pct
                            .partial_cmp(&a.net_profit_pct)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        a.net_profit_pct
                            .partial_cmp(&b.net_profit_pct)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }
                }),
                DealSortBy::ProfitPerM3 => filtered.sort_by(|a, b| {
                    if self.hub_sort_descending {
                        b.profit_per_m3
                            .partial_cmp(&a.profit_per_m3)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        a.profit_per_m3
                            .partial_cmp(&b.profit_per_m3)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }
                }),
                DealSortBy::Jumps => filtered.sort_by(|a, b| {
                    if self.hub_sort_descending {
                        b.total_jumps.cmp(&a.total_jumps)
                    } else {
                        a.total_jumps.cmp(&b.total_jumps)
                    }
                }),
                DealSortBy::Cost => filtered.sort_by(|a, b| {
                    if self.hub_sort_descending {
                        b.capital_required
                            .partial_cmp(&a.capital_required)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        a.capital_required
                            .partial_cmp(&b.capital_required)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }
                }),
                DealSortBy::ItemName => filtered.sort_by(|a, b| {
                    if self.hub_sort_descending {
                        b.item_name.to_lowercase().cmp(&a.item_name.to_lowercase())
                    } else {
                        a.item_name.to_lowercase().cmp(&b.item_name.to_lowercase())
                    }
                }),
                DealSortBy::BuySystem => filtered.sort_by(|a, b| {
                    if self.hub_sort_descending {
                        b.source_system_name
                            .to_lowercase()
                            .cmp(&a.source_system_name.to_lowercase())
                    } else {
                        a.source_system_name
                            .to_lowercase()
                            .cmp(&b.source_system_name.to_lowercase())
                    }
                }),
                DealSortBy::BuyPrice => filtered.sort_by(|a, b| {
                    if self.hub_sort_descending {
                        b.buy_price
                            .partial_cmp(&a.buy_price)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        a.buy_price
                            .partial_cmp(&b.buy_price)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }
                }),
                DealSortBy::SellSystem => filtered.sort_by(|a, b| {
                    if self.hub_sort_descending {
                        b.dest_system_name
                            .to_lowercase()
                            .cmp(&a.dest_system_name.to_lowercase())
                    } else {
                        a.dest_system_name
                            .to_lowercase()
                            .cmp(&b.dest_system_name.to_lowercase())
                    }
                }),
                DealSortBy::SellPrice => filtered.sort_by(|a, b| {
                    if self.hub_sort_descending {
                        b.sell_price
                            .partial_cmp(&a.sell_price)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        a.sell_price
                            .partial_cmp(&b.sell_price)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }
                }),
                DealSortBy::Quantity => filtered.sort_by(|a, b| {
                    if self.hub_sort_descending {
                        b.quantity.cmp(&a.quantity)
                    } else {
                        a.quantity.cmp(&b.quantity)
                    }
                }),
                DealSortBy::Volume => filtered.sort_by(|a, b| {
                    if self.hub_sort_descending {
                        b.volume_used
                            .partial_cmp(&a.volume_used)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        a.volume_used
                            .partial_cmp(&b.volume_used)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }
                }),
            }

            self.cached_hub_deals = filtered;
            self.cached_hub_key = current_hub_key;
        }

        let filtered_deals = &self.cached_hub_deals;

        if do_pack_cargo {
            let ship = self.current_ship();
            let deal_refs: Vec<&ArbitrageDeal> = filtered_deals.iter().collect();
            let packed = hub_arbitrage::pack_hub_corridor_manifest(
                &deal_refs,
                ship.max_volume,
                ship.max_budget,
            );
            let count = packed.packed.len();
            self.hub_packed_manifest = Some(packed);
            self.hub_show_manifest_card = true;
            action_toast = Some(format!("Packed {} items into ship cargo manifest!", count));
        }

        if do_export_csv {
            let deals_for_csv: Vec<(usize, &ArbitrageDeal)> =
                filtered_deals.iter().enumerate().collect();
            let csv_str = export_deals_to_csv(&deals_for_csv);
            let export_path = resolve_csv_export_path("hub_arbitrage_export.csv");
            match safe_write_csv(&export_path, &csv_str) {
                Ok(()) => {
                    action_toast = Some(format!(
                        "Exported {} hub deals to {} & copied to clipboard!",
                        filtered_deals.len(),
                        export_path.display()
                    ));
                }
                Err(e) => {
                    tracing::error!(
                        target: "eve_trade_scout",
                        error_code = %crate::error::ErrorCode::ErrUiCsvExport,
                        path = %export_path.display(),
                        error = %e,
                        "Failed to write hub CSV export to disk: {e}"
                    );
                    action_toast = Some(format!("Failed to export CSV: {e}"));
                }
            }
            copy_to_clipboard(ui, csv_str);
        }

        // 2. Corridor Telemetry & KPI Banner
        let total_pot_profit: f64 = filtered_deals.iter().map(|d| d.net_profit).sum();
        let total_pot_capital: f64 = filtered_deals.iter().map(|d| d.capital_required).sum();
        let total_pot_vol: f64 = filtered_deals.iter().map(|d| d.volume_used).sum();
        let jumps_str = if let (Some(s), Some(d), Some(g)) =
            (self.hub_source_filter, self.hub_dest_filter, &self.graph)
        {
            let j = g.jump_count(s.system_id(), d.system_id(), self.safe_only);
            format!("{} jumps (highsec safe)", j)
        } else {
            "Variable jumps".to_string()
        };

        palette.sub_card_frame().inner_margin(8.0).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let corridor_badge = format!(
                        "Corridor: {} → {}",
                        self.hub_source_filter.map(|h| h.name()).unwrap_or("Any Hub"),
                        self.hub_dest_filter.map(|h| h.name()).unwrap_or("Any Hub"),
                    );
                    ui.label(RichText::new(corridor_badge).color(palette.accent_primary).strong());
                    ui.separator();
                    ui.label(RichText::new(format!("Distance: {}", jumps_str)).color(palette.text_muted));
                    ui.separator();
                    ui.label(RichText::new(format!("Deals: {}", filtered_deals.len())).color(palette.text_primary).strong());
                    ui.separator();
                    ui.label(RichText::new(format!("Total Potential Profit: +{:.2}M ISK", total_pot_profit / 1_000_000.0)).color(palette.success_green).strong());
                    ui.separator();
                    ui.label(RichText::new(format!("Capital: {:.2}M ISK", total_pot_capital / 1_000_000.0)).color(palette.text_secondary));
                    ui.separator();
                    ui.label(RichText::new(format!("Volume: {:.1} m³", total_pot_vol)).color(palette.text_secondary));

                    if let (Some(s), Some(d)) = (self.hub_source_filter, self.hub_dest_filter) {
                        ui.separator();
                        let push_corridor_btn = palette.primary_button(
                            RichText::new("▶ Push Corridor to EVE Autopilot")
                                .color(palette.accent_primary_text)
                                .strong(),
                        );
                        if ui.add_enabled(!self.sso_is_pushing, push_corridor_btn)
                            .on_hover_text("Send origin and destination hub trade stations directly to your in-game Autopilot")
                            .clicked()
                        {
                            let dests = hub_arbitrage::hub_corridor_destination_ids(s, d, &self.hub_packed_manifest);
                            push_dest_ids = Some((dests, "Hub Corridor"));
                        }

                        if let Some(manifest) = &self.hub_packed_manifest {
                            let sheet_btn = palette.secondary_button(
                                RichText::new("Copy Corridor for EVE Notepad")
                                    .color(palette.accent_secondary_text)
                                    .strong(),
                            );
                            if ui.add(sheet_btn)
                                .on_hover_text("Copy formatted trade run manifest for in-game Notepad (Alt+N)")
                                .clicked()
                            {
                                let jumps = self.graph.as_ref().map(|g| g.jump_count(s.system_id(), d.system_id(), self.safe_only)).unwrap_or(0);
                                let sheet = hub_arbitrage::format_hub_flight_sheet(s, d, jumps, manifest);
                                copy_to_clipboard(ui, sheet);
                                action_toast = Some("Copied Hub Corridor Flight Sheet for in-game Notepad (Alt+N)!".to_string());
                            }
                        }
                    }
                });
            });

        ui.add_space(6.0);

        // 3. Packed Cargo Manifest Card (if active)
        let mut close_manifest = false;
        if self.hub_show_manifest_card {
            if let Some(manifest) = &self.hub_packed_manifest {
                let ship = self.current_ship();
                let ship_is_big = self.ship_is_big;
                palette.card_frame().inner_margin(12.0).show(ui, |ui| {
                        let vol_pct = (manifest.total_volume / ship.max_volume * 100.0).min(100.0);
                        let cap_pct = (manifest.total_capital / ship.max_budget * 100.0).min(100.0);

                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Optimized Ship Cargo Manifest")
                                    .size(15.0)
                                    .color(palette.accent_primary)
                                    .strong(),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "({}: {:.0} m³ / {:.0}M ISK)",
                                    if ship_is_big { "Big Hauler" } else { "Small Hauler" },
                                    ship.max_volume,
                                    ship.max_budget / 1_000_000.0
                                ))
                                .color(palette.text_muted)
                                .size(12.0),
                            );

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button(RichText::new("✕ Close Manifest").color(palette.text_muted)).clicked() {
                                    close_manifest = true;
                                }

                                if let (Some(s), Some(d)) = (self.hub_source_filter, self.hub_dest_filter) {
                                    let sheet_btn = palette.secondary_button(
                                        RichText::new("Copy Manifest for EVE Notepad (Alt+N)")
                                            .color(palette.accent_secondary_text)
                                            .strong(),
                                    );
                                    if ui.add(sheet_btn)
                                        .on_hover_text("Copy complete manifest shopping list and sell orders for in-game Notepad (Alt+N)")
                                        .clicked()
                                    {
                                        let jumps = self.graph.as_ref().map(|g| g.jump_count(s.system_id(), d.system_id(), self.safe_only)).unwrap_or(0);
                                        let sheet = hub_arbitrage::format_hub_flight_sheet(s, d, jumps, manifest);
                                        copy_to_clipboard(ui, sheet);
                                        action_toast = Some("Copied Hub Manifest Flight Sheet for in-game Notepad (Alt+N)!".to_string());
                                    }

                                    let push_btn = palette.primary_button(
                                        RichText::new("▶ Push Manifest to EVE Autopilot")
                                            .color(palette.accent_primary_text)
                                            .strong(),
                                    );
                                    if ui.add_enabled(!self.sso_is_pushing, push_btn)
                                        .on_hover_text("Send origin and destination hub trade stations directly to in-game Autopilot")
                                        .clicked()
                                    {
                                        let dests = hub_arbitrage::hub_corridor_destination_ids(s, d, &Some(manifest.clone()));
                                        push_dest_ids = Some((dests, "Hub Manifest"));
                                    }
                                } else if let Some(first) = manifest.packed.first() {
                                    let push_btn = palette.primary_button(
                                        RichText::new("▶ Push Manifest to EVE Autopilot")
                                            .color(palette.accent_primary_text)
                                            .strong(),
                                    );
                                    if ui.add_enabled(!self.sso_is_pushing, push_btn)
                                        .on_hover_text("Send origin and destination trade stations directly to in-game Autopilot")
                                        .clicked()
                                    {
                                        let dests = if first.deal.source_station_id == first.deal.dest_station_id {
                                            vec![first.deal.source_station_id]
                                        } else {
                                            vec![first.deal.source_station_id, first.deal.dest_station_id]
                                        };
                                        push_dest_ids = Some((dests, "Hub Manifest"));
                                    }
                                }
                            });
                        });

                        ui.add_space(6.0);

                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(format!("Items: {}", manifest.packed.len())).strong());
                            ui.separator();
                            ui.label(RichText::new(format!("Volume Used: {:.1} / {:.0} m³ ({:.1}%)", manifest.total_volume, ship.max_volume, vol_pct)).strong());
                            ui.separator();
                            ui.label(RichText::new(format!("Capital Spent: {:.2} / {:.0}M ISK ({:.1}%)", manifest.total_capital / 1_000_000.0, ship.max_budget / 1_000_000.0, cap_pct)).strong());
                            ui.separator();
                            ui.label(
                                RichText::new(format!("Realized Net Profit: +{:.2}M ISK", manifest.total_profit / 1_000_000.0))
                                    .color(palette.success_green)
                                    .size(13.5)
                                    .strong(),
                            );
                        });

                        ui.add_space(8.0);

                        egui::ScrollArea::vertical()
                            .max_height(140.0)
                            .id_source("hub_manifest_scroll")
                            .show(ui, |ui| {
                                for (slot_idx, slot) in manifest.packed.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(format!("{}.", slot_idx + 1)).color(palette.text_muted));
                                        ui.label(RichText::new(&slot.deal.item_name).color(palette.text_primary).strong());
                                        ui.label(format!("Qty: {} ({:.1} m³)", slot.deal.quantity, slot.volume));
                                        ui.label(format!("Buy: {:.2}M", slot.capital / 1_000_000.0));
                                        ui.label(
                                            RichText::new(format!("+{:.2}M ISK ({:.1}%)", slot.net_profit / 1_000_000.0, slot.deal.net_profit_pct))
                                                .color(palette.success_green)
                                                .strong(),
                                        );
                                        ui.label(RichText::new(format!("→ {}", slot.sell_name)).color(palette.text_muted));

                                        if ui.small_button("Item").clicked() {
                                            copy_to_clipboard(ui, slot.deal.item_name.clone());
                                            action_toast = Some(format!("Copied '{}' to clipboard", slot.deal.item_name));
                                        }
                                        if ui.small_button("Buy").clicked() {
                                            copy_to_clipboard(ui, slot.deal.source_station_name.clone());
                                            action_toast = Some(format!("Copied '{}' to clipboard", slot.deal.source_station_name));
                                        }
                                        if ui.small_button("Sell").clicked() {
                                            copy_to_clipboard(ui, slot.deal.dest_station_name.clone());
                                            action_toast = Some(format!("Copied '{}' to clipboard", slot.deal.dest_station_name));
                                        }
                                    });
                                }
                            });
                    });
                ui.add_space(6.0);
            }
        }
        if close_manifest {
            self.hub_show_manifest_card = false;
        }

        // 4. Deals Table Grid
        if filtered_deals.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(
                    RichText::new("No hub deals match the selected criteria.")
                        .size(16.0)
                        .color(palette.text_secondary),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Try selecting 'Any Hub (Wildcard)', enabling NPC Commodities, lowering Min Profit, or clearing the search query.")
                        .color(palette.text_muted),
                );
            });
        } else {
            let current_sort = self.hub_sort_by;
            let is_desc = self.hub_sort_descending;
            let current_selected_idx = self.selected_deal_idx;

            let mut render_header_btn =
                |ui: &mut egui::Ui, title: &str, sort_target: DealSortBy| {
                    let is_active = current_sort == sort_target;
                    let arrow = if is_active {
                        if is_desc {
                            " ▼"
                        } else {
                            " ▲"
                        }
                    } else {
                        ""
                    };
                    let label_text = format!("{}{}", title, arrow);
                    let text_color = if is_active {
                        palette.accent_primary
                    } else {
                        palette.text_secondary
                    };
                    let resp = ui.add(
                        egui::Button::new(RichText::new(label_text).color(text_color).strong())
                            .frame(false),
                    );
                    if resp.clicked() {
                        ui.ctx().request_repaint();
                        if is_active {
                            change_sort = Some((sort_target, !is_desc));
                        } else {
                            let default_desc = !matches!(
                                sort_target,
                                DealSortBy::ItemName
                                    | DealSortBy::BuySystem
                                    | DealSortBy::SellSystem
                                    | DealSortBy::Jumps
                                    | DealSortBy::Cost
                            );
                            change_sort = Some((sort_target, default_desc));
                        }
                    }
                };

            let actions_w = 120.0;
            let rank_w = 38.0;

            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .min_scrolled_height(0.0)
                .column(Column::exact(rank_w)) // #
                .column(Column::remainder().at_least(140.0).resizable(true)) // ITEM
                .column(Column::initial(150.0).at_least(110.0).resizable(true)) // BUY HUB & STATION
                .column(Column::initial(80.0).at_least(65.0).resizable(true)) // BUY / ITEM
                .column(Column::initial(150.0).at_least(110.0).resizable(true)) // SELL HUB & STATION
                .column(Column::initial(80.0).at_least(65.0).resizable(true)) // SELL / ITEM
                .column(Column::initial(50.0).at_least(40.0).resizable(true)) // QTY
                .column(Column::initial(60.0).at_least(45.0).resizable(true)) // VOL
                .column(Column::initial(70.0).at_least(55.0).resizable(true)) // COST
                .column(Column::initial(85.0).at_least(75.0).resizable(true)) // NET PROFIT
                .column(Column::initial(55.0).at_least(45.0).resizable(true)) // NET ROI
                .column(Column::initial(45.0).at_least(35.0).resizable(true)) // JUMPS
                .column(Column::initial(80.0).at_least(70.0).resizable(true)) // PROFIT/JUMP
                .column(Column::exact(actions_w)) // ACTIONS
                .header(24.0, |mut header| {
                    header.col(|ui| {
                        ui.label(RichText::new("#").color(palette.text_secondary).strong());
                    });
                    header.col(|ui| {
                        render_header_btn(ui, "ITEM", DealSortBy::ItemName);
                    });
                    header.col(|ui| {
                        render_header_btn(ui, "BUY HUB / STATION", DealSortBy::BuySystem);
                    });
                    header.col(|ui| {
                        render_header_btn(ui, "BUY / ITEM", DealSortBy::BuyPrice);
                    });
                    header.col(|ui| {
                        render_header_btn(ui, "SELL HUB / STATION", DealSortBy::SellSystem);
                    });
                    header.col(|ui| {
                        render_header_btn(ui, "SELL / ITEM", DealSortBy::SellPrice);
                    });
                    header.col(|ui| {
                        render_header_btn(ui, "QTY", DealSortBy::Quantity);
                    });
                    header.col(|ui| {
                        render_header_btn(ui, "VOL (m³)", DealSortBy::Volume);
                    });
                    header.col(|ui| {
                        render_header_btn(ui, "COST (M)", DealSortBy::Cost);
                    });
                    header.col(|ui| {
                        render_header_btn(ui, "NET PROFIT", DealSortBy::TotalProfit);
                    });
                    header.col(|ui| {
                        render_header_btn(ui, "NET ROI", DealSortBy::RoiPct);
                    });
                    header.col(|ui| {
                        render_header_btn(ui, "JUMPS", DealSortBy::Jumps);
                    });
                    header.col(|ui| {
                        render_header_btn(ui, "PROFIT/JUMP", DealSortBy::ProfitPerJump);
                    });
                    header.col(|ui| {
                        ui.label(
                            RichText::new("ACTIONS")
                                .color(palette.text_secondary)
                                .strong(),
                        );
                    });
                })
                .body(|mut body| {
                    for (display_rank, deal) in filtered_deals.iter().enumerate() {
                        let orig_idx = self.deals.iter().position(|d| {
                            d.type_id == deal.type_id
                                && d.source_order_id == deal.source_order_id
                                && d.dest_order_id == deal.dest_order_id
                        });
                        let is_selected = orig_idx.is_some() && orig_idx == current_selected_idx;

                        body.row(28.0, |mut row| {
                            row.set_selected(is_selected);

                            // 1: Rank badge
                            row.col(|ui| {
                                let rank_color = match display_rank {
                                    0 => Color32::from_rgb(255, 215, 0),
                                    1 => palette.rank_silver(),
                                    2 => Color32::from_rgb(205, 127, 50),
                                    _ => palette.text_muted,
                                };
                                let rank_resp = egui::Frame::none()
                                    .fill(Color32::from_rgba_unmultiplied(
                                        rank_color.r(),
                                        rank_color.g(),
                                        rank_color.b(),
                                        28,
                                    ))
                                    .stroke(Stroke::new(1.0_f32, rank_color))
                                    .rounding(3.0)
                                    .inner_margin(4.0)
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new(format!("#{}", display_rank + 1))
                                                .color(rank_color)
                                                .strong()
                                                .size(12.0),
                                        );
                                    });
                                if rank_resp.response.interact(egui::Sense::click()).clicked() {
                                    if let Some(idx) = orig_idx {
                                        toggle_select = Some(idx);
                                    }
                                }
                            });

                            // 2: Item Name
                            row.col(|ui| {
                                let item_color = if is_selected {
                                    palette.accent_primary
                                } else {
                                    palette.text_primary
                                };
                                ui.horizontal(|ui| {
                                    if ui
                                        .selectable_label(
                                            is_selected,
                                            RichText::new(&deal.item_name)
                                                .color(item_color)
                                                .strong(),
                                        )
                                        .clicked()
                                    {
                                        if let Some(idx) = orig_idx {
                                            toggle_select = Some(idx);
                                        }
                                    }
                                    if deal.is_npc_trade {
                                        egui::Frame::none()
                                            .fill(Color32::from_rgba_unmultiplied(
                                                palette.warning_yellow.r(),
                                                palette.warning_yellow.g(),
                                                palette.warning_yellow.b(),
                                                if palette.dark_mode { 35 } else { 45 },
                                            ))
                                            .stroke(Stroke::new(1.0_f32, palette.warning_yellow))
                                            .rounding(3.0)
                                            .inner_margin(egui::Margin::symmetric(4.0, 1.0))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    RichText::new("NPC")
                                                        .color(palette.warning_yellow)
                                                        .size(10.0)
                                                        .strong(),
                                                );
                                            });
                                    }
                                });
                            });

                            // 3: Buy Hub & Station
                            row.col(|ui| {
                                let src_hub = CoreHub::from_system_id(deal.source_system_id);
                                let col = palette.hub_badge_color(src_hub);
                                ui.horizontal(|ui| {
                                    if let Some(h) = src_hub {
                                        ui.label(
                                            RichText::new(format!("[{}]", h.name()))
                                                .color(col)
                                                .strong(),
                                        );
                                    }
                                    let st_label = if deal.source_station_name.len() > 22 {
                                        format!("{}…", &deal.source_station_name[..20])
                                    } else {
                                        deal.source_station_name.clone()
                                    };
                                    ui.label(RichText::new(st_label).color(palette.text_muted))
                                        .on_hover_text(&deal.source_station_name);
                                });
                            });

                            // 4: Buy / Item
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(format!("{:.2}", deal.buy_price))
                                        .color(palette.text_secondary),
                                );
                            });

                            // 5: Sell Hub & Station
                            row.col(|ui| {
                                let dst_hub = CoreHub::from_system_id(deal.dest_system_id);
                                let col = palette.hub_badge_color(dst_hub);
                                ui.horizontal(|ui| {
                                    if let Some(h) = dst_hub {
                                        ui.label(
                                            RichText::new(format!("[{}]", h.name()))
                                                .color(col)
                                                .strong(),
                                        );
                                    }
                                    let st_label = if deal.dest_station_name.len() > 22 {
                                        format!("{}…", &deal.dest_station_name[..20])
                                    } else {
                                        deal.dest_station_name.clone()
                                    };
                                    ui.label(RichText::new(st_label).color(palette.text_muted))
                                        .on_hover_text(&deal.dest_station_name);
                                });
                            });

                            // 6: Sell / Item
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(format!("{:.2}", deal.sell_price))
                                        .color(palette.text_secondary),
                                );
                            });

                            // 7: QTY
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(deal.quantity.to_string())
                                        .color(palette.text_secondary),
                                );
                            });

                            // 8: VOL
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(format!("{:.1}", deal.volume_used))
                                        .color(palette.text_secondary),
                                );
                            });

                            // 9: COST (M)
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "{:.2}M",
                                        deal.capital_required / 1_000_000.0
                                    ))
                                    .color(palette.text_secondary),
                                );
                            });

                            // 10: NET PROFIT
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "+{:.2}M ISK",
                                        deal.net_profit / 1_000_000.0
                                    ))
                                    .color(palette.success_green)
                                    .strong(),
                                );
                            });

                            // 11: NET ROI
                            row.col(|ui| {
                                let roi_col = if deal.is_high_roi() {
                                    palette.warning_yellow
                                } else {
                                    palette.success_green
                                };
                                ui.label(
                                    RichText::new(format!("+{:.1}%", deal.net_profit_pct))
                                        .color(roi_col)
                                        .strong(),
                                );
                            });

                            // 12: JUMPS
                            row.col(|ui| {
                                let jumps_tag = format!(
                                    "{}+{}={}",
                                    deal.jumps_from_home, deal.jumps_source_dest, deal.total_jumps
                                );
                                ui.label(RichText::new(jumps_tag).color(palette.text_muted));
                            });

                            // 13: PROFIT/JUMP
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "{:.2}M",
                                        deal.profit_per_jump / 1_000_000.0
                                    ))
                                    .color(palette.accent_secondary)
                                    .strong(),
                                );
                            });

                            // 14: ACTIONS
                            row.col(|ui| {
                                ui.horizontal(|ui| {
                                    if ui
                                        .small_button("Item")
                                        .on_hover_text("Copy item name to clipboard")
                                        .clicked()
                                    {
                                        copy_to_clipboard(ui, deal.item_name.clone());
                                        action_toast = Some(format!(
                                            "Copied item '{}' to clipboard!",
                                            deal.item_name
                                        ));
                                    }
                                    if ui
                                        .small_button("Buy")
                                        .on_hover_text(format!(
                                            "Copy buy station '{}' to clipboard",
                                            deal.source_station_name
                                        ))
                                        .clicked()
                                    {
                                        copy_to_clipboard(ui, deal.source_station_name.clone());
                                        action_toast = Some(format!(
                                            "Copied buy station '{}' to clipboard!",
                                            deal.source_station_name
                                        ));
                                    }
                                    if ui
                                        .small_button("Sell")
                                        .on_hover_text(format!(
                                            "Copy sell station '{}' to clipboard",
                                            deal.dest_station_name
                                        ))
                                        .clicked()
                                    {
                                        copy_to_clipboard(ui, deal.dest_station_name.clone());
                                        action_toast = Some(format!(
                                            "Copied sell station '{}' to clipboard!",
                                            deal.dest_station_name
                                        ));
                                    }
                                });
                            });
                        });
                    }
                });
        }

        // Apply any deferred UI updates
        if let Some((new_sort, new_desc)) = change_sort {
            self.hub_sort_by = new_sort;
            self.hub_sort_descending = new_desc;
        }

        if let Some(toggled) = toggle_select {
            if self.selected_deal_idx == Some(toggled) {
                self.selected_deal_idx = None;
            } else {
                self.selected_deal_idx = Some(toggled);
                self.dossier_side_panel = true; // Open dossier panel automatically on selection
            }
        }

        if let Some((dests, label)) = push_dest_ids {
            self.push_to_eve_autopilot(dests, label);
        }

        if let Some(msg) = action_toast {
            self.set_toast(msg);
        }
    }

    fn render_settings_tab(&mut self, ui: &mut egui::Ui) {
        let palette = self.theme.palette();
        ui.horizontal(|ui| {
            ui.heading(
                RichText::new("Configuration & System Settings").color(palette.text_primary),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(
                        RichText::new("💾 Save Settings to Disk")
                            .strong()
                            .color(palette.accent_primary),
                    )
                    .clicked()
                {
                    match self.save_user_config() {
                        Ok(()) => self.set_toast("Configuration saved to disk!"),
                        Err(e) => self.set_toast(format!("Failed to save config: {e}")),
                    }
                }
                if ui
                    .button(
                        RichText::new("↺ Reset Defaults").color(Color32::from_rgb(220, 100, 100)),
                    )
                    .on_hover_text("Reset all settings to default factory values")
                    .clicked()
                {
                    let default_cfg = crate::config::UserConfig::default();
                    self.apply_user_config(&default_cfg);
                    let _ = self.save_user_config();
                    self.set_toast("Reset all settings to default factory values");
                }
            });
        });
        ui.add_space(10.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            // Card: Visual Theme & Appearance
            palette.card_frame().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("🎨 Cockpit Visual Theme & Appearance")
                            .size(15.0)
                            .color(palette.accent_primary)
                            .strong(),
                    );
                    egui::Frame::none()
                        .fill(Color32::from_rgba_unmultiplied(
                            palette.accent_primary.r(),
                            palette.accent_primary.g(),
                            palette.accent_primary.b(),
                            32,
                        ))
                        .stroke(Stroke::new(1.0_f32, palette.accent_primary))
                        .rounding(4.0)
                        .inner_margin(egui::vec2(6.0, 2.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("ACTIVE: {}", self.theme.display_name().to_uppercase()))
                                    .size(10.5)
                                    .color(palette.accent_primary)
                                    .strong(),
                            );
                        });
                });
                ui.label(
                    RichText::new("Select your active workstation theme. All panels, cards, buttons, and telemetry adapt dynamically and persist across application runs:")
                        .color(palette.text_secondary),
                );
                ui.add_space(10.0);

                let mut theme_switched: Option<crate::config::AppTheme> = None;

                // 4 Interactive Theme Selector Cards in a responsive horizontal grid
                ui.horizontal_wrapped(|ui| {
                    for &t in crate::config::AppTheme::ALL {
                        let is_active = self.theme == t;
                        let t_palette = t.palette();
                        let (bg_swatch, acc_swatch, sec_swatch) = t_palette.preview_swatches();

                        let card_stroke = if is_active {
                            Stroke::new(2.0_f32, t_palette.accent_primary)
                        } else {
                            Stroke::new(1.0_f32, palette.card_sub_stroke.color)
                        };

                        let card_fill = if is_active {
                            if t_palette.dark_mode {
                                Color32::from_rgb(
                                    (t_palette.card_bg.r() as u16 + 12).min(255) as u8,
                                    (t_palette.card_bg.g() as u16 + 12).min(255) as u8,
                                    (t_palette.card_bg.b() as u16 + 12).min(255) as u8,
                                )
                            } else {
                                Color32::from_rgb(250, 252, 255)
                            }
                        } else {
                            palette.card_sub_bg
                        };

                        let tile_resp = egui::Frame::none()
                            .fill(card_fill)
                            .stroke(card_stroke)
                            .rounding(6.0)
                            .inner_margin(10.0)
                            .show(ui, |ui| {
                                ui.set_min_width(200.0);
                                ui.set_max_width(235.0);
                                ui.vertical(|ui| {
                                    // Header: Theme Name + Active Tag
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(t.display_name())
                                                .size(13.5)
                                                .color(if is_active { t_palette.accent_primary } else { palette.text_primary })
                                                .strong(),
                                        );
                                        if is_active {
                                            ui.label(
                                                RichText::new("✔")
                                                    .size(11.0)
                                                    .color(t_palette.accent_primary)
                                                    .strong(),
                                            );
                                        }
                                    });

                                    ui.add_space(4.0);

                                    // Swatch row: 3 preview color chips
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("Colors:").size(10.5).color(palette.text_muted));
                                        // Base color swatch
                                        let (r1, _) = ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::hover());
                                        ui.painter().rect_filled(r1, egui::Rounding::same(3.0), bg_swatch);
                                        ui.painter().rect_stroke(r1, egui::Rounding::same(3.0), Stroke::new(1.0_f32, Color32::GRAY));

                                        // Primary accent swatch
                                        let (r2, _) = ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::hover());
                                        ui.painter().rect_filled(r2, egui::Rounding::same(3.0), acc_swatch);
                                        ui.painter().rect_stroke(r2, egui::Rounding::same(3.0), Stroke::new(1.0_f32, Color32::GRAY));

                                        // Secondary accent swatch
                                        let (r3, _) = ui.allocate_exact_size(Vec2::splat(14.0), egui::Sense::hover());
                                        ui.painter().rect_filled(r3, egui::Rounding::same(3.0), sec_swatch);
                                        ui.painter().rect_stroke(r3, egui::Rounding::same(3.0), Stroke::new(1.0_f32, Color32::GRAY));
                                    });

                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(t.description())
                                            .size(11.0)
                                            .color(palette.text_muted),
                                    );

                                    ui.add_space(6.0);
                                    let btn_label = if is_active { "Active Theme" } else { "Select Theme" };
                                    if ui.button(RichText::new(btn_label).size(11.5).strong()).clicked() {
                                        theme_switched = Some(t);
                                    }
                                });
                            });

                        if tile_resp.response.interact(egui::Sense::click()).clicked() {
                            theme_switched = Some(t);
                        }
                    }
                });

                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Quick Select:").size(12.0).strong().color(palette.text_secondary));
                    for &t in crate::config::AppTheme::ALL {
                        if ui.radio_value(&mut self.theme, t, t.display_name()).clicked() {
                            theme_switched = Some(t);
                        }
                    }
                });

                if let Some(new_t) = theme_switched {
                    if self.theme != new_t {
                        self.theme = new_t;
                        let _ = self.save_user_config();
                        self.set_toast(format!("Theme switched to {}", new_t.display_name()));
                        ui.ctx().request_repaint();
                    }
                }
            });

            ui.add_space(12.0);

            // Card 0: Display, UI Scaling & Layout
            palette.card_frame().show(ui, |ui| {
                ui.label(RichText::new("Display, UI Scaling & Layout").size(15.0).color(palette.accent_primary).strong());
                ui.label(RichText::new("Configure desktop scaling factors, responsive docking, and fullscreen display modes:").color(palette.text_secondary));
                ui.add_space(8.0);

                    ui.horizontal_wrapped(|ui| {
                        ui.label("UI Scale / Zoom Factor:");
                        let mut pct = (self.ui_scale * 100.0).round() as i32;
                        if ui.add(egui::Slider::new(&mut pct, 75..=150).suffix("%").step_by(5.0)).changed() {
                            self.ui_scale = (pct as f32 / 100.0).clamp(0.75, 1.50);
                            ui.ctx().set_zoom_factor(self.ui_scale);
                            let _ = self.save_user_config();
                        }
                        if ui.button("↺ Reset 100%").clicked() {
                            self.ui_scale = 1.0;
                            ui.ctx().set_zoom_factor(1.0);
                            let _ = self.save_user_config();
                            self.set_toast("UI Scale reset to 100%");
                        }
                        ui.label(RichText::new("(Shortcuts: Ctrl + / Ctrl - / Ctrl 0)").color(Color32::GRAY).size(11.0));
                    });

                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Window Mode:");
                        let fs_label = if self.is_fullscreen { "Exit Fullscreen (F11)" } else { "Enter Fullscreen (F11)" };
                        if ui.button(fs_label).clicked() {
                            self.is_fullscreen = !self.is_fullscreen;
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.is_fullscreen));
                            let _ = self.save_user_config();
                            if self.is_fullscreen {
                                self.set_toast("Entered Fullscreen mode (Press F11 to exit)");
                            } else {
                                self.set_toast("Exited Fullscreen mode");
                            }
                        }

                        ui.separator();

                        ui.label("Deal Dossier Docking:");
                        if ui.radio_value(&mut self.dossier_side_panel, true, "◫ Side Panel (widescreen)").clicked() {
                            let _ = self.save_user_config();
                            self.set_toast("Dossier docking set to Side Panel");
                        }
                        if ui.radio_value(&mut self.dossier_side_panel, false, "▬ Bottom Drawer").clicked() {
                            let _ = self.save_user_config();
                            self.set_toast("Dossier docking set to Bottom Drawer");
                        }

                        ui.separator();

                        ui.label("Executive KPI Bar:");
                        if ui.radio_value(&mut self.compact_kpi, false, "Full Cards").clicked() {
                            let _ = self.save_user_config();
                            self.set_toast("KPI bar set to Full Cards");
                        }
                        if ui.radio_value(&mut self.compact_kpi, true, "Compact Pills").clicked() {
                            let _ = self.save_user_config();
                            self.set_toast("KPI bar set to Compact Pills");
                        }
                    });
                });

            ui.add_space(12.0);

            // Card: EVE SSO & In-Game Autopilot Sync
            palette.card_frame().show(ui, |ui| {
                ui.label(
                    RichText::new("EVE SSO & Live In-Game Autopilot Sync")
                        .size(15.0)
                        .color(palette.accent_primary)
                        .strong(),
                );
                    ui.label(
                        "Connect your EVE Online pilot via official CCP SSO (OAuth2 + PKCE) to push multi-stop trade routes and waypoints straight into your running EVE client Autopilot HUD (`Alt+A` / `POST /v2/ui/autopilot/waypoint/`).",
                    );
                    ui.add_space(8.0);

                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new("Official CCP Single Sign-On (OAuth2 PKCE)")
                                .color(Color32::from_rgb(0, 220, 255))
                                .size(12.0)
                                .strong(),
                        );
                        ui.separator();
                        ui.label(
                            RichText::new("Scopes: Waypoints (`Alt+A`) & Live Pilot Location")
                                .color(Color32::from_rgb(0, 255, 140))
                                .size(11.5),
                        );
                        ui.separator();
                        ui.label(
                            RichText::new("Safe client: No passwords or private keys stored.")
                                .color(Color32::GRAY)
                                .size(11.5),
                        );
                    });

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);

                    ui.horizontal_wrapped(|ui| {
                        if let Some(ref char_name) = self.sso_character_name {
                            let char_id_str = self
                                .sso_character_id
                                .map(|id| format!(" [ID: {}]", id))
                                .unwrap_or_default();
                            ui.label(
                                RichText::new(format!(
                                    "Connected Character: {}{}",
                                    char_name, char_id_str
                                ))
                                .size(13.5)
                                .color(Color32::from_rgb(0, 255, 140))
                                .strong(),
                            );

                            let token_status_label = if let Some(expires_at) = self.sso_token_expires_at {
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_secs() as i64)
                                    .unwrap_or(0);
                                let secs = (expires_at - now).max(0);
                                let mins = secs / 60;
                                if self.sso_is_refreshing_token {
                                    "⏳ Refreshing token...".to_string()
                                } else if secs == 0 {
                                    "[ACTIVE] Connected • Auto-refreshes on demand".to_string()
                                } else if secs < 300 {
                                    format!("⏳ Rotating soon ({}s) • Auto-refreshes", secs)
                                } else {
                                    format!("[ACTIVE] Token Active (Expires in {}m) • Auto-Refreshes", mins)
                                }
                            } else if crate::esi::sso::token_storage_path().exists() {
                                if self.sso_is_refreshing_token {
                                    "⏳ Refreshing token...".to_string()
                                } else {
                                    "[ACTIVE] Connected • Auto-Refreshes".to_string()
                                }
                            } else {
                                "No token".to_string()
                            };

                            ui.separator();
                            ui.label(
                                RichText::new(token_status_label)
                                    .size(11.5)
                                    .color(Color32::from_rgb(0, 220, 255)),
                            );

                            ui.add_space(8.0);
                            let ref_btn = egui::Button::new(
                                RichText::new(if self.sso_is_refreshing_token {
                                    "⏳ Refreshing"
                                } else {
                                    "Refresh Token"
                                })
                                .size(12.0)
                                .color(Color32::from_rgb(255, 230, 120))
                                .strong(),
                            );
                            if ui
                                .add_enabled(!self.sso_is_refreshing_token, ref_btn)
                                .on_hover_text(
                                    "Manually renew the EVE SSO access token right now using your saved refresh token",
                                )
                                .clicked()
                            {
                                self.check_and_refresh_sso_token(true);
                            }

                            ui.add_space(6.0);
                            let disc_btn = egui::Button::new(
                                RichText::new("Disconnect")
                                    .size(12.0)
                                    .color(Color32::from_rgb(255, 100, 100)),
                            );
                            if ui.add(disc_btn).clicked() {
                                self.disconnect_sso();
                            }
                        } else if self.sso_is_authenticating {
                            ui.spinner();
                            ui.label(
                                RichText::new(
                                    "⏳ Waiting for CCP SSO Authorization in web browser...",
                                )
                                .size(13.0)
                                .color(Color32::from_rgb(255, 205, 0))
                                .strong(),
                            );
                        } else {
                            let conn_btn = egui::Button::new(
                                RichText::new("▶ Connect EVE Character via SSO")
                                    .size(13.0)
                                    .color(Color32::BLACK)
                                    .strong(),
                            )
                            .fill(Color32::from_rgb(0, 230, 170));

                            if ui.add(conn_btn).clicked() {
                                self.start_sso_authentication();
                            }
                            ui.label(
                                RichText::new("(Opens your default browser to login with CCP)")
                                    .color(Color32::GRAY)
                                    .size(11.5),
                            );
                        }
                    });

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);

                    if self.sso_character_name.is_some() {
                        ui.horizontal(|ui| {
                            let loc_btn = egui::Button::new(
                                RichText::new(if self.sso_is_fetching_location {
                                    "⏳ Querying Live Location..."
                                } else {
                                    "Locate Character"
                                })
                                .size(12.5)
                                .color(Color32::BLACK)
                                .strong(),
                            )
                            .fill(Color32::from_rgb(0, 220, 255));

                            if ui
                                .add_enabled(!self.sso_is_fetching_location, loc_btn)
                                .on_hover_text(
                                    "Query ESI right now for your pilot's live in-game solar system and set it as Deals starting system",
                                )
                                .clicked()
                            {
                                self.fetch_live_character_location(true);
                            }

                            ui.label(
                                RichText::new(
                                    "Queries CCP ESI for your pilot's active solar system and automatically updates the Deals starting location.",
                                )
                                .size(11.5)
                                .color(Color32::GRAY),
                            );

                            if self.sso_is_fetching_location {
                                ui.spinner();
                            }
                        });

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if ui
                                .checkbox(
                                    &mut self.sso_auto_sync_location,
                                    "Auto-sync character location on launch",
                                )
                                .on_hover_text(
                                    "Automatically query CCP ESI on application launch to detect your active solar system and set it as the starting hub.",
                                )
                                .changed()
                            {
                                let _ = self.save_user_config();
                            }
                        });
                    }
                });

            ui.add_space(12.0);

            // Card 1: Trading Taxes & Character Skills
            palette.card_frame().show(ui, |ui| {
                ui.label(RichText::new("Market Taxes & Character Skills").size(15.0).color(palette.accent_primary).strong());
                    ui.label("Customize fee percentages according to your pilot's Accounting and Broker Relations skills:");
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.label("Broker Fee % (on buy order):");
                        if ui.add(egui::DragValue::new(&mut self.broker_fee_pct).speed(0.1).range(1.0..=5.0).suffix("%")).changed() {
                            let _ = self.save_user_config();
                        }
                        ui.label(RichText::new("(Default 3.0%, reduced by Broker Relations skill)").color(Color32::GRAY));
                    });

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Sales Tax % (on sell order):");
                        if ui.add(egui::DragValue::new(&mut self.sales_tax_pct).speed(0.1).range(3.375..=8.0).suffix("%")).changed() {
                            let _ = self.save_user_config();
                        }
                        ui.label(RichText::new("(Default 8.0%, reduced by Accounting skill to 3.375%)").color(Color32::GRAY));
                    });

                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Skill Presets:").strong());
                        if ui.small_button("Max Skills (Accounting V / Broker V)").on_hover_text("Accounting V: 3.375% sales tax\nBroker Relations V: 1.0% broker fee").clicked() {
                            self.sales_tax_pct = 3.375;
                            self.broker_fee_pct = 1.0;
                            let _ = self.save_user_config();
                            self.set_toast("Applied Max Skills: 3.375% sales tax, 1.0% broker fee");
                        }
                        if ui.small_button("Level IV Skills").on_hover_text("Accounting IV: 4.3% sales tax\nBroker Relations IV: 1.4% broker fee").clicked() {
                            self.sales_tax_pct = 4.3;
                            self.broker_fee_pct = 1.4;
                            let _ = self.save_user_config();
                            self.set_toast("Applied Level IV Skills: 4.3% sales tax, 1.4% broker fee");
                        }
                        if ui.small_button("Base / Untrained").on_hover_text("Accounting 0: 8.0% sales tax\nBroker Relations 0: 3.0% broker fee").clicked() {
                            self.sales_tax_pct = 8.0;
                            self.broker_fee_pct = 3.0;
                            let _ = self.save_user_config();
                            self.set_toast("Applied Untrained: 8.0% sales tax, 3.0% broker fee");
                        }
                    });
                });

            ui.add_space(12.0);

            // Card: Vessel Cargo Capacity & Anti-Gank Collateral Budget
            palette.card_frame().show(ui, |ui| {
                ui.label(RichText::new("Vessel Cargo Capacity & Collateral Budget").size(15.0).color(palette.accent_primary).strong());
                    ui.label("Configure cargo hold volume and ISK collateral caps to match your active ship hull and prevent suicide ganking:");
                    ui.add_space(8.0);

                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Profile Mode:").strong());
                        if ui.radio_value(&mut self.use_custom_ship, false, "Standard Hauler Presets").clicked() {
                            let _ = self.save_user_config();
                            self.set_toast(if self.ship_is_big { "Active: Big Hauler (60k m³ · 500M)" } else { "Active: Small Hauler (30k m³ · 50M)" });
                        }
                        if ui.radio_value(&mut self.use_custom_ship, true, "Custom Ship & Anti-Gank Sizing").clicked() {
                            let _ = self.save_user_config();
                            self.set_toast("Active: Custom Vessel & Anti-Gank Protection");
                        }
                    });

                    if !self.use_custom_ship {
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Active Preset:");
                            if ui.selectable_label(self.ship_is_big, "Big Hauler (Deep Space Transport: 60,000 m³ · 500M ISK)").clicked() {
                                self.ship_is_big = true;
                                let _ = self.save_user_config();
                                self.set_toast("Selected Big Hauler preset (60k m³ · 500M ISK)");
                            }
                            if ui.selectable_label(!self.ship_is_big, "Small Hauler (T1 Industrial: 30,000 m³ · 50M ISK)").clicked() {
                                self.ship_is_big = false;
                                let _ = self.save_user_config();
                                self.set_toast("Selected Small Hauler preset (30k m³ · 50M ISK)");
                            }
                        });
                    } else {
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("Hull Quick Presets:").strong());
                            if ui.small_button("T1 Hauler (30k m³ · 50M ISK)").on_hover_text("Iteron Mark V, Bestower, Tayra, Mammoth\nAnti-Gank Cap: 50M ISK (safe against 1 Catalyst)").clicked() {
                                self.custom_cargo_vol = 30_000.0;
                                self.custom_budget_isk = 50_000_000.0;
                                let _ = self.save_user_config();
                                self.set_toast("Loaded T1 Hauler preset (30k m³ · 50M ISK cap)");
                            }
                            if ui.small_button("Deep Space Transport (60k m³ · 500M ISK)").on_hover_text("Occator, Bustard, Mastodon, Impel (Overheated tank)\nAnti-Gank Cap: 500M ISK").clicked() {
                                self.custom_cargo_vol = 60_000.0;
                                self.custom_budget_isk = 500_000_000.0;
                                let _ = self.save_user_config();
                                self.set_toast("Loaded DST preset (60k m³ · 500M ISK cap)");
                            }
                            if ui.small_button("Blockade Runner (12k m³ · 300M ISK)").on_hover_text("Prowler, Prorator, Crane, Viator (Cloaky / Immune to Cargo Scanners)\nAnti-Gank Cap: 300M ISK").clicked() {
                                self.custom_cargo_vol = 12_000.0;
                                self.custom_budget_isk = 300_000_000.0;
                                let _ = self.save_user_config();
                                self.set_toast("Loaded Blockade Runner preset (12k m³ · 300M ISK cap)");
                            }
                            if ui.small_button("Freighter (900k m³ · 2.0B ISK)").on_hover_text("Charon, Obelisk, Fenrir, Providence\nAnti-Gank Cap: 2.0B ISK (Tornado gank threshold)").clicked() {
                                self.custom_cargo_vol = 900_000.0;
                                self.custom_budget_isk = 2_000_000_000.0;
                                let _ = self.save_user_config();
                                self.set_toast("Loaded Freighter preset (900k m³ · 2.0B ISK cap)");
                            }
                        });

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label("Cargo Hold Capacity (m³):");
                            if ui.add(egui::DragValue::new(&mut self.custom_cargo_vol).speed(250.0).range(100.0..=1_500_000.0).suffix(" m³")).changed() {
                                let _ = self.save_user_config();
                            }
                            ui.label(RichText::new(format!("({:.1}k m³)", self.custom_cargo_vol / 1000.0)).color(Color32::from_rgb(0, 220, 255)));
                        });

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label("Anti-Gank Collateral Budget (ISK):");
                            if ui.add(egui::DragValue::new(&mut self.custom_budget_isk).speed(5_000_000.0).range(1_000_000.0..=100_000_000_000.0)).changed() {
                                let _ = self.save_user_config();
                            }
                            let isk_badge = if self.custom_budget_isk >= 1_000_000_000.0 {
                                format!("{:.2}B ISK", self.custom_budget_isk / 1_000_000_000.0)
                            } else {
                                format!("{:.1}M ISK", self.custom_budget_isk / 1_000_000.0)
                            };
                            ui.label(RichText::new(isk_badge).color(Color32::from_rgb(255, 215, 0)).strong());
                        });

                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "Tip: Anti-Gank Rule: In Highsec (0.5 - 1.0), carrying more than 100M ISK in a T1 industrial or 1B - 2B ISK in a standard freighter makes you a prime target for suicide gankers (Catalysts / Tornadoes). Capping your trip budget prevents dangerous cargo exposure."
                            )
                            .size(11.0)
                            .color(Color32::from_rgb(180, 200, 225)),
                        );
                    }
                });

            ui.add_space(12.0);

            // Card 2: Scan Regions
            egui::Frame::none()
                .fill(Color32::from_rgb(18, 25, 40))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 45, 72)))
                .rounding(6.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.label(RichText::new("Active Scan Regions").size(15.0).color(Color32::from_rgb(0, 210, 255)).strong());
                    ui.label("Select major trade hub regions to include in the arbitrage scanner:");
                    ui.add_space(8.0);

                    if ui.checkbox(&mut self.region_the_forge, "The Forge (Jita IV-4) — Region 10000002").changed() {
                        let _ = self.save_user_config();
                    }
                    if ui.checkbox(&mut self.region_domain, "Domain (Amarr VIII) — Region 10000043").changed() {
                        let _ = self.save_user_config();
                    }
                    if ui.checkbox(&mut self.region_sinq_laison, "Sinq Laison (Dodixie IX-20) — Region 10000032").changed() {
                        let _ = self.save_user_config();
                    }
                    if ui.checkbox(&mut self.region_heimatar, "Heimatar (Rens VI-8) — Region 10000030").changed() {
                        let _ = self.save_user_config();
                    }
                    if ui.checkbox(&mut self.region_metropolis, "Metropolis (Hek VIII-12) — Region 10000042").changed() {
                        let _ = self.save_user_config();
                    }
                });

            ui.add_space(12.0);

            // Card 3: Auto-Refresh Settings
            egui::Frame::none()
                .fill(Color32::from_rgb(18, 25, 40))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 45, 72)))
                .rounding(6.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.label(RichText::new("Automated Market Refresh").size(15.0).color(Color32::from_rgb(0, 210, 255)).strong());
                    ui.label("Automatically fetch fresh regional market data every 5 minutes (matching CCP ESI cache expiration):");
                    ui.add_space(8.0);

                    if ui.checkbox(&mut self.auto_refresh, "Enable 5-Minute Market Auto-Refresh").changed() {
                        let _ = self.save_user_config();
                        if self.auto_refresh {
                            self.last_scan_completed = Some(Instant::now());
                            self.set_toast("Auto-Refresh enabled (5-minute interval)");
                        } else {
                            self.set_toast("Auto-Refresh disabled");
                        }
                    }
                    ui.add_space(4.0);
                    ui.label(RichText::new("Tip: When enabled, a live countdown badge is displayed in the executive header next to the scan button.").color(Color32::GRAY));

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.label(RichText::new("High-Profit Sound Notification:").size(13.0).color(Color32::from_rgb(0, 210, 255)).strong());
                    ui.label("Plays an audible alert chime whenever a trade exceeding the ROI threshold is discovered during auto-refresh:");
                    ui.add_space(6.0);

                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut self.sound_alerts_enabled, "Play audio chime on high-profit trades").changed() {
                            let _ = self.save_user_config();
                            if self.sound_alerts_enabled {
                                crate::audio::play_test_sound();
                                self.set_toast("Sound alerts enabled");
                            } else {
                                self.set_toast("Sound alerts muted");
                            }
                        }
                    });

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Alert Minimum Net ROI %:");
                        if ui.add(egui::DragValue::new(&mut self.sound_min_roi_pct).speed(1.0).range(10.0..=1000.0).suffix("%")).changed() {
                            let _ = self.save_user_config();
                        }
                        ui.label(RichText::new("(Default: 100.0% Net ROI)").color(Color32::GRAY));

                        ui.add_space(10.0);
                        if ui.button("▶ Test Sound Chime").clicked() {
                            crate::audio::play_test_sound();
                            self.set_toast("Playing test sound chime...");
                        }
                    });
                });

            ui.add_space(12.0);

            // Card 4: Station & Structure Access Pool
            palette.card_frame().show(ui, |ui| {
                ui.label(RichText::new("Station & Structure Access Pool").size(15.0).color(palette.accent_primary).strong());
                ui.label("Configure whether arbitrage deals should strictly target NPC stations or allow player-owned Citadels:");
                ui.add_space(8.0);

                if ui.radio_value(&mut self.npc_stations_only, true, "Preferred: NPC Stations Only (Guaranteed public docking, no access lockouts)").changed() {
                    let _ = self.save_user_config();
                    self.set_toast("Station Pool: Set to NPC Stations Only");
                }
                ui.add_space(4.0);
                if ui.radio_value(&mut self.npc_stations_only, false, "Include Player Citadels / Upwell Structures in Pool (May require docking access ACL)").changed() {
                    let _ = self.save_user_config();
                    self.set_toast("Station Pool: Set to Include Player Citadels");
                }
            });

            ui.add_space(12.0);

            // Card 4: Enforced Invariants
            palette.card_frame().show(ui, |ui| {
                ui.label(RichText::new("Domain Rules & Invariants").size(15.0).color(palette.accent_primary).strong());
                    ui.add_space(6.0);
                    ui.label("• Strict 10.0% Net Profitability: Deals producing under 10% net return after all taxes are rejected.");
                    ui.label("• Big Hauler Constraints: 60,000 m³ volume capacity / 500,000,000 ISK max budget.");
                    ui.label("• Small Hauler Constraints: 30,000 m³ volume capacity / 50,000,000 ISK max budget.");
                    ui.label("• True Profit-Per-Jump: Jumps required to fly to the pickup station are counted in total flight distance.");
                    ui.label("• Safe Stargate Navigation: Routes avoid lowsec (<0.45) and nullsec camps by default.");
                    ui.label("• Highsec Space Only: All trade routes and station orders are restricted to high security space.");
                });

            ui.add_space(14.0);

            // Card 5: In-Game ISK Donations & Support
            let corp_tex = self.ensure_corp_icon(ui.ctx()).cloned();
            let mut copy_corp = false;
            egui::Frame::none()
                .fill(Color32::from_rgb(22, 28, 44))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(218, 165, 32)))
                .rounding(8.0)
                .inner_margin(16.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if let Some(ref tex) = corp_tex {
                            ui.image((tex.id(), egui::vec2(52.0, 52.0)));
                            ui.add_space(12.0);
                        }

                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Support EVE Traveling Merchant — In-Game ISK Donations")
                                        .size(16.0)
                                        .color(Color32::from_rgb(255, 215, 0))
                                        .strong(),
                                );
                                egui::Frame::none()
                                    .fill(Color32::from_rgba_unmultiplied(255, 215, 0, 30))
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(255, 215, 0)))
                                    .rounding(4.0)
                                    .inner_margin(egui::vec2(8.0, 2.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new("COMMUNITY")
                                                .size(10.0)
                                                .color(Color32::from_rgb(255, 215, 0))
                                                .strong(),
                                        );
                                    });
                            });

                            ui.add_space(4.0);
                            ui.label(
                                "EVE Traveling Merchant is 100% free, open-source, and community-driven. If the arbitrage scout and knapsack corridor route engine help you haul billions of ISK, consider sending a voluntary in-game ISK donation to support ongoing development and server infrastructure!",
                            );

                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Recipient Corporation:").strong());
                                ui.label(
                                    RichText::new(CORP_DONATION_RECIPIENT)
                                        .size(14.0)
                                        .color(Color32::from_rgb(255, 225, 80))
                                        .strong(),
                                );

                                if ui.button(RichText::new("Copy 'Expandabliss'").strong()).clicked() {
                                    copy_corp = true;
                                }
                            });

                            ui.add_space(6.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("How to donate in-game:").size(11.5).color(Color32::GRAY));
                                ui.label(
                                    RichText::new("1. Open Wallet (Alt+W)  •  2. Click 'Transfer ISK'  •  3. Search 'Expandabliss' (Corporation)  •  4. Fly Safe o7!")
                                        .size(11.5)
                                        .color(Color32::from_rgb(140, 190, 240)),
                                );
                            });
                        });
                    });
                });

            if copy_corp {
                ui.output_mut(|o| o.copied_text = CORP_DONATION_RECIPIENT.to_string());
                self.set_toast("Copied 'Expandabliss' to clipboard! Send ISK in-game via Wallet > Transfer.");
            }
        });
    }

    fn render_splash_screen(&mut self, ctx: &egui::Context) {
        let palette = self.theme.palette();
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(palette.bg_app).inner_margin(24.0))
            .show(ctx, |ui| {
                let total_rect = ui.max_rect();
                let center = total_rect.center();
                let time = ctx.input(|i| i.time) as f32;

                // Rotating tips timer
                if self.startup_last_tip.elapsed() >= Duration::from_secs(4) {
                    self.startup_tip_index = (self.startup_tip_index + 1) % TIPS.len();
                    self.startup_last_tip = Instant::now();
                }

                // Holographic radar / scanner background (scoped borrow)
                {
                    let painter = ui.painter();
                    let radar_center = egui::pos2(center.x, center.y - 70.0);
                    let base_radius = 120.0_f32;

                    // Range rings
                    for (idx, r_factor) in
                        [0.35_f32, 0.65_f32, 1.0_f32, 1.35_f32].iter().enumerate()
                    {
                        let r = base_radius * r_factor;
                        let alpha = if idx == 2 { 60 } else { 25 };
                        painter.circle_stroke(
                            radar_center,
                            r,
                            Stroke::new(
                                1.0_f32,
                                Color32::from_rgba_unmultiplied(0, 229, 255, alpha),
                            ),
                        );
                    }

                    // Radar sweep line
                    let sweep_angle = time * 2.2_f32;
                    let sweep_end = egui::pos2(
                        radar_center.x + (base_radius * 1.35_f32) * sweep_angle.cos(),
                        radar_center.y + (base_radius * 1.35_f32) * sweep_angle.sin(),
                    );
                    painter.line_segment(
                        [radar_center, sweep_end],
                        Stroke::new(1.5_f32, Color32::from_rgba_unmultiplied(0, 255, 220, 180)),
                    );

                    // Pulsing target reticle
                    let pulse = (time * 4.0_f32).sin().abs();
                    let reticle_radius = 4.0_f32 + pulse * 3.0_f32;
                    painter.circle_filled(
                        radar_center,
                        reticle_radius,
                        Color32::from_rgba_unmultiplied(0, 255, 200, 220),
                    );

                    // Crosshair tick marks
                    for angle in [
                        0.0_f32,
                        std::f32::consts::FRAC_PI_2,
                        std::f32::consts::PI,
                        3.0_f32 * std::f32::consts::FRAC_PI_2,
                    ] {
                        let p1 = egui::pos2(
                            radar_center.x + (base_radius * 0.9_f32) * angle.cos(),
                            radar_center.y + (base_radius * 0.9_f32) * angle.sin(),
                        );
                        let p2 = egui::pos2(
                            radar_center.x + (base_radius * 1.4_f32) * angle.cos(),
                            radar_center.y + (base_radius * 1.4_f32) * angle.sin(),
                        );
                        painter.line_segment(
                            [p1, p2],
                            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0, 180, 240, 45)),
                        );
                    }
                }

                // Centered UI Content
                ui.vertical_centered(|ui| {
                    ui.add_space((total_rect.height() * 0.08_f32).max(10.0));

                    // Glowing Branding Header
                    ui.label(
                        RichText::new("EVE TRAVELING MERCHANT")
                            .size(30.0)
                            .color(Color32::from_rgb(0, 229, 255))
                            .strong(),
                    );
                    ui.label(
                        RichText::new("AUTONOMOUS MULTI-ITEM ARBITRAGE & CORRIDOR ROUTE ENGINE")
                            .size(11.0)
                            .color(Color32::from_rgb(140, 175, 215))
                            .extra_letter_spacing(2.0_f32),
                    );

                    ui.add_space(150.0); // Radar height offset

                    // Active Stage Pill
                    let stage_text = format!(
                        "STAGE {} OF {} • {}",
                        self.startup_progress.stage_index,
                        self.startup_progress.total_stages,
                        self.startup_progress.stage_name.to_uppercase()
                    );
                    egui::Frame::none()
                        .fill(Color32::from_rgba_unmultiplied(16, 26, 48, 220))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0, 180, 240)))
                        .rounding(4.0)
                        .inner_margin(egui::vec2(14.0, 5.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(stage_text)
                                    .size(12.0)
                                    .color(Color32::from_rgb(255, 215, 0))
                                    .strong(),
                            );
                        });

                    ui.add_space(12.0);

                    // Glowing Progress Bar
                    let progress = self.startup_progress.percent.clamp(0.0, 1.0);
                    let bar_width = 540.0_f32.min(total_rect.width() - 60.0);
                    let bar_height = 20.0;

                    let (rect, _) = ui.allocate_exact_size(
                        Vec2::new(bar_width, bar_height),
                        egui::Sense::hover(),
                    );
                    let bar_painter = ui.painter();

                    bar_painter.rect_filled(rect, 4.0, Color32::from_rgb(12, 17, 30));
                    bar_painter.rect_stroke(
                        rect,
                        4.0,
                        Stroke::new(1.0_f32, Color32::from_rgb(30, 48, 78)),
                    );

                    if progress > 0.001 {
                        let fill_rect = egui::Rect::from_min_size(
                            rect.min,
                            Vec2::new(bar_width * progress, bar_height),
                        );
                        bar_painter.rect_filled(fill_rect, 4.0, Color32::from_rgb(0, 210, 255));
                    }

                    let pct_label = format!("{:.0}%", progress * 100.0);
                    bar_painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        pct_label,
                        egui::FontId::proportional(12.0),
                        Color32::from_rgb(255, 255, 255),
                    );

                    ui.add_space(12.0);

                    // Operation Detail Subtitle
                    ui.label(
                        RichText::new(&self.startup_progress.detail)
                            .size(13.0)
                            .color(Color32::from_rgb(215, 230, 255)),
                    );

                    // Error Alert Box (if any)
                    if let Some(ref err) = self.startup_progress.error {
                        ui.add_space(14.0);
                        let err_code = self
                            .startup_progress
                            .error_code
                            .unwrap_or(crate::error::ErrorCode::ErrUiStartupFailed);

                        egui::Frame::none()
                            .fill(Color32::from_rgba_unmultiplied(48, 12, 16, 235))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(231, 76, 60)))
                            .rounding(6.0)
                            .inner_margin(12.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // High-visibility error code badge
                                    egui::Frame::none()
                                        .fill(Color32::from_rgb(180, 25, 42))
                                        .rounding(3.0)
                                        .inner_margin(egui::vec2(6.0, 2.0))
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new(err_code.as_str())
                                                    .size(11.0)
                                                    .color(Color32::WHITE)
                                                    .strong(),
                                            );
                                        });

                                    ui.label(
                                        RichText::new(format!(
                                            "Stage: {}",
                                            self.startup_progress.stage_name
                                        ))
                                        .size(11.0)
                                        .color(Color32::from_rgb(200, 215, 235)),
                                    );
                                });

                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(err)
                                        .size(12.0)
                                        .color(Color32::from_rgb(255, 170, 170)),
                                );
                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    if ui
                                        .button(
                                            RichText::new("Enter Offline Mode")
                                                .color(Color32::from_rgb(0, 229, 255)),
                                        )
                                        .clicked()
                                    {
                                        self.is_startup = false;
                                    }
                                });
                            });
                    }

                    ui.add_space(20.0);

                    // Rotating Tip Box
                    let tip = TIPS[self.startup_tip_index];
                    egui::Frame::none()
                        .fill(Color32::from_rgba_unmultiplied(10, 15, 26, 200))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(24, 34, 52)))
                        .rounding(5.0)
                        .inner_margin(egui::vec2(16.0, 10.0))
                        .show(ui, |ui| {
                            ui.set_max_width(560.0);
                            ui.label(
                                RichText::new(tip)
                                    .size(12.0)
                                    .color(Color32::from_rgb(160, 190, 225))
                                    .italics(),
                            );
                        });

                    ui.add_space(16.0);

                    // Skip Button
                    if ui
                        .button(
                            RichText::new("⏩ Skip Pre-Fetch & Open Dashboard")
                                .size(11.0)
                                .color(Color32::from_rgb(110, 135, 170)),
                        )
                        .clicked()
                    {
                        self.is_startup = false;
                    }
                });
            });
    }
}

pub fn run_desktop_gui(app: DesktopApp) -> Result<(), eframe::Error> {
    let icon_data = eframe::icon_data::from_png_bytes(APP_ICON_PNG_BYTES).ok();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("EVE Traveling Merchant — Arbitrage & Multi-Stop Route Planner")
        .with_inner_size([1280.0, 768.0])
        .with_min_inner_size([800.0, 480.0]);

    if let Some(icon) = icon_data {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "EVE Traveling Merchant",
        options,
        Box::new(|_cc| Ok(Box::new(app))),
    )
}
