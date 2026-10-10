use crate::arbitrage::{bundle_route_cargo, find_deals, ArbitrageConfig, ArbitrageDeal, TradeCorridorManifest};
use crate::config::{BIG_HAULER, SMALL_HAULER};
use crate::esi::{fetch_markets, EsiClient};
use crate::graph::SystemGraph;
use crate::route_planner::{plan_route, RouteStop, TradeRoute};
use crate::sde::SdeLoader;
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use egui_extras::{Column, TableBuilder};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum BreakpointTier {
    Compact,    // < 1000px: laptops / compact window
    Standard,   // 1000px - 1440px: normal 1080p windowed
    Widescreen, // 1440px - 2100px: 1080p fullscreen, 1440p
    Ultrawide,  // >= 2100px: 1440p fullscreen, 4K, ultrawides
}

impl BreakpointTier {
    pub fn from_width(w: f32) -> Self {
        if w < 1000.0 {
            BreakpointTier::Compact
        } else if w < 1440.0 {
            BreakpointTier::Standard
        } else if w < 2100.0 {
            BreakpointTier::Widescreen
        } else {
            BreakpointTier::Ultrawide
        }
    }

    pub fn is_wide(self) -> bool {
        matches!(self, BreakpointTier::Widescreen | BreakpointTier::Ultrawide)
    }
}

#[derive(PartialEq, Clone, Copy)]
enum ActiveTab {
    Deals,
    Manifests,
    Route,
    ChainRoute,
    Settings,
}

#[derive(PartialEq, Clone, Copy)]
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
}

enum ScanResult {
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
    Error(String),
}

const TIPS: &[&str] = &[
    "💡 Minimum 10.0% net ROI is strictly enforced across every arbitrage deal after taxes and broker fees.",
    "💡 Big Hauler packs up to 60,000 m³ / 500M ISK, while Small Hauler provides an agile 30,000 m³ / 50M ISK profile.",
    "💡 Safe highsec routing automatically bypasses dangerous solar systems with security status below 0.45.",
    "💡 Multi-item knapsack manifests bundle multiple high-margin items along identical corridors into single hauler runs.",
    "💡 Dead-end systems and remote highsec detours with ≥40% net ROI are badged with 🔥 for maximum profit yields.",
    "💡 You can press F11 anytime to toggle fullscreen, or use Ctrl + / - to scale the interface dynamically.",
    "💡 Deepest order book liquidity is found across the 5 trade hubs: Jita, Amarr, Dodixie, Rens, and Hek.",
];

#[allow(dead_code)]
enum ChainSearchResult {
    Success(Vec<crate::chain_route_planner::ChainRoute>),
    Error(String),
}


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
            d.item_name.replace('"', "\"\""),
            d.source_station_name.replace('"', "\"\""),
            d.source_system_name.replace('"', "\"\""),
            d.buy_price,
            d.dest_station_name.replace('"', "\"\""),
            d.dest_system_name.replace('"', "\"\""),
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
                m.source_station_name.replace('"', "\"\""),
                m.source_system_name.replace('"', "\"\""),
                m.dest_station_name.replace('"', "\"\""),
                m.dest_system_name.replace('"', "\"\""),
                m.total_volume_used,
                m.total_capital_required,
                m.total_net_profit,
                d.item_name.replace('"', "\"\""),
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
    rx: Receiver<ScanResult>,
    tx: Sender<ScanResult>,

    // ── Chain Route Planner tab ──────────────────────────────────
    chain_max_legs: usize,
    chain_detour_enabled: bool,
    chain_max_detour: u32,
    chain_is_running: bool,
    chain_results: Vec<crate::chain_route_planner::ChainRoute>,
    chain_selected_idx: Option<usize>,
    chain_rx: Receiver<ChainSearchResult>,
    chain_tx: Sender<ChainSearchResult>,
    chain_flowchart_split: bool,

    // ── Responsive Layout & UI Scaling ───────────────────────────
    pub ui_scale: f32,
    pub is_fullscreen: bool,
    pub dossier_side_panel: bool,
    pub compact_kpi: bool,

    // ── Startup & Animated Splash Screen ─────────────────────────
    pub is_startup: bool,
    pub startup_progress: StartupProgress,
    pub startup_rx: Option<Receiver<StartupMsg>>,
    pub startup_tip_index: usize,
    pub startup_last_tip: Instant,
}

impl DesktopApp {
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
        _initial_regions: Vec<u32>,
        initial_deals: Vec<ArbitrageDeal>,
        initial_route: Option<TradeRoute>,
        initial_auto_refresh: bool,
        initial_sound_alerts: bool,
        initial_sound_min_roi: f64,
    ) -> Self {
        let (tx, rx) = channel();
        let (chain_tx, chain_rx) = channel();

        let initial_status = if initial_deals.is_empty() {
            "Ready. Configure ship & location, then click SCAN MARKET.".to_string()
        } else {
            format!("Loaded {} profitable arbitrage deals (ROI >= 10.0%).", initial_deals.len())
        };

        let has_forge = false;
        let has_domain = false;
        let has_sinq = false;
        let has_heim = false;
        let has_metr = false;

        let initial_ship = if initial_ship_is_big { BIG_HAULER } else { SMALL_HAULER };
        let initial_manifests = bundle_route_cargo(&initial_deals, initial_ship);

        let mut initial_alerted_keys = std::collections::HashSet::new();
        for d in &initial_deals {
            if d.net_profit_pct >= initial_sound_min_roi.max(10.0) {
                initial_alerted_keys.insert((d.type_id, d.source_station_id, d.dest_station_id));
            }
        }

        Self {
            sde: Some(sde),
            graph: Some(graph),
            esi: Some(esi),
            ship_is_big: initial_ship_is_big,
            home_system: initial_home_system,
            include_adjacent_regions: true,
            safe_only: initial_safe_only,
            npc_stations_only: initial_npc_stations_only,
            min_roi_pct: initial_min_roi.max(10.0),
            min_profit_isk: if initial_min_profit <= 0.0 { 1_000_000.0 } else { initial_min_profit },
            broker_fee_pct: 3.0,
            sales_tax_pct: 8.0,
            max_route_stops: 5,

            auto_refresh: initial_auto_refresh,
            auto_refresh_secs: 300,
            last_scan_completed: Some(Instant::now()),
            sound_alerts_enabled: initial_sound_alerts,
            sound_min_roi_pct: if initial_sound_min_roi <= 0.0 { 100.0 } else { initial_sound_min_roi },
            alerted_deal_keys: initial_alerted_keys,

            region_the_forge: has_forge,
            region_domain: has_domain,
            region_sinq_laison: has_sinq,
            region_heimatar: has_heim,
            region_metropolis: has_metr,

            filter_item_name: String::new(),
            sort_by: DealSortBy::ProfitPerJump,
            sort_descending: true,
            min_profit_filter_m: 0.0,
            max_jumps_filter: 0,
            hide_npc_trades: false,

            tab: ActiveTab::Deals,
            is_scanning: false,
            status_text: initial_status,
            deals: initial_deals,
            manifests: initial_manifests,
            route: initial_route,
            selected_deal_idx: None,
            toast: None,
            rx,
            tx,

            chain_max_legs: 4,
            chain_detour_enabled: true,
            chain_max_detour: 2,
            chain_is_running: false,
            chain_results: Vec::new(),
            chain_selected_idx: None,
            chain_rx,
            chain_tx,
            chain_flowchart_split: true,

            custom_cargo_vol: 60_000.0,
            custom_budget_isk: 500_000_000.0,
            use_custom_ship: false,

            ui_scale: 1.0,
            is_fullscreen: false,
            dossier_side_panel: true,
            compact_kpi: false,

            is_startup: false,
            startup_progress: StartupProgress {
                stage_index: 5,
                total_stages: 5,
                stage_name: "Ready".to_string(),
                detail: "Startup complete".to_string(),
                percent: 1.0,
                completed: true,
                error: None,
            },
            startup_rx: None,
            startup_tip_index: 0,
            startup_last_tip: Instant::now(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_loading(
        initial_ship_is_big: bool,
        initial_home_system: String,
        initial_safe_only: bool,
        initial_npc_stations_only: bool,
        initial_min_roi: f64,
        initial_min_profit: f64,
        initial_auto_refresh: bool,
        initial_sound_alerts: bool,
        initial_sound_min_roi: f64,
        startup_rx: Receiver<StartupMsg>,
    ) -> Self {
        let (tx, rx) = channel();
        let (chain_tx, chain_rx) = channel();

        Self {
            sde: None,
            graph: None,
            esi: None,
            ship_is_big: initial_ship_is_big,
            home_system: initial_home_system,
            include_adjacent_regions: true,
            safe_only: initial_safe_only,
            npc_stations_only: initial_npc_stations_only,
            min_roi_pct: initial_min_roi.max(10.0),
            min_profit_isk: if initial_min_profit <= 0.0 { 1_000_000.0 } else { initial_min_profit },
            broker_fee_pct: 3.0,
            sales_tax_pct: 8.0,
            max_route_stops: 5,

            auto_refresh: initial_auto_refresh,
            auto_refresh_secs: 300,
            last_scan_completed: Some(Instant::now()),
            sound_alerts_enabled: initial_sound_alerts,
            sound_min_roi_pct: if initial_sound_min_roi <= 0.0 { 100.0 } else { initial_sound_min_roi },
            alerted_deal_keys: std::collections::HashSet::new(),

            region_the_forge: false,
            region_domain: false,
            region_sinq_laison: false,
            region_heimatar: false,
            region_metropolis: false,

            filter_item_name: String::new(),
            sort_by: DealSortBy::ProfitPerJump,
            sort_descending: true,
            min_profit_filter_m: 0.0,
            max_jumps_filter: 0,
            hide_npc_trades: false,

            tab: ActiveTab::Deals,
            is_scanning: false,
            status_text: "Initializing neural core...".to_string(),
            deals: Vec::new(),
            manifests: Vec::new(),
            route: None,
            selected_deal_idx: None,
            toast: None,
            rx,
            tx,

            chain_max_legs: 4,
            chain_detour_enabled: true,
            chain_max_detour: 2,
            chain_is_running: false,
            chain_results: Vec::new(),
            chain_selected_idx: None,
            chain_rx,
            chain_tx,
            chain_flowchart_split: true,

            custom_cargo_vol: 60_000.0,
            custom_budget_isk: 500_000_000.0,
            use_custom_ship: false,

            ui_scale: 1.0,
            is_fullscreen: false,
            dossier_side_panel: true,
            compact_kpi: false,

            is_startup: true,
            startup_progress: StartupProgress::default(),
            startup_rx: Some(startup_rx),
            startup_tip_index: 0,
            startup_last_tip: Instant::now(),
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

        if self.region_the_forge { set.insert(10000002); }
        if self.region_domain { set.insert(10000043); }
        if self.region_sinq_laison { set.insert(10000032); }
        if self.region_heimatar { set.insert(10000030); }
        if self.region_metropolis { set.insert(10000042); }

        if set.is_empty() {
            set.insert(10000002); // Default to The Forge (Jita)
        }

        let mut list: Vec<u32> = set.into_iter().collect();
        list.sort_unstable();
        list
    }

    fn start_scan(&mut self) {
        if self.is_scanning {
            return;
        }

        let (Some(sde), Some(graph), Some(esi)) = (&self.sde, &self.graph, &self.esi) else {
            return;
        };

        self.is_scanning = true;
        self.status_text = "Connecting to CCP ESI API & streaming market orders...".to_string();

        let sde = Arc::clone(sde);
        let graph = Arc::clone(graph);
        let esi = Arc::clone(esi);
        let tx = self.tx.clone();

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
                    let _ = tx.send(ScanResult::Error(format!("Failed to start Tokio runtime: {}", e)));
                    return;
                }
            };

            rt.block_on(async move {
                let home_id = match sde.get_system_id(&home_name) {
                    Some(id) => id,
                    None => {
                        let _ = tx.send(ScanResult::Error(format!(
                            "Solar system '{}' not found in EVE universe database.",
                            home_name
                        )));
                        return;
                    }
                };

                let markets = match fetch_markets(&esi, &regions).await {
                    Ok(m) => m,
                    Err(e) => {
                        let _ = tx.send(ScanResult::Error(format!("ESI API Error: {}", e)));
                        return;
                    }
                };

                let total_sells = markets
                    .values()
                    .map(|m| m.sell_orders.values().map(|v| v.len()).sum::<usize>())
                    .sum();
                let total_buys = markets
                    .values()
                    .map(|m| m.buy_orders.values().map(|v| v.len()).sum::<usize>())
                    .sum();

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
}

impl eframe::App for DesktopApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Startup splash handling
        if self.is_startup {
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
                            startup_complete = Some((sde, graph, esi, home_system_name, scan_regions, deals, manifests, route));
                            break;
                        }
                        StartupMsg::Error(err) => {
                            self.startup_progress.error = Some(err);
                            ctx.request_repaint();
                        }
                    }
                }
            }

            if let Some((sde, graph, esi, home_system_name, _scan_regions, deals, manifests, route)) = startup_complete {
                self.sde = Some(sde);
                self.graph = Some(graph);
                self.esi = Some(esi);
                self.home_system = home_system_name;
                self.deals = deals;
                self.manifests = manifests;
                self.route = route;
                self.is_startup = false;
                self.startup_rx = None;
                self.status_text = format!(
                    "✔ Ready! Loaded {} profitable trade deals across {} packed manifests.",
                    self.deals.len(),
                    self.manifests.len()
                );
                ctx.request_repaint();
            }

            if self.is_startup {
                ctx.request_repaint_after(Duration::from_millis(16));
                self.render_splash_screen(ctx);
                return;
            }
        }

        // Poll worker thread
        if let Ok(res) = self.rx.try_recv() {
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
                        "✔ Scan Complete! Parsed {} sell orders & {} buy orders. Discovered {} deals (ROI >= {:.1}%) across {} packed manifests.",
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
                    self.alerted_deal_keys.retain(|k| current_high_profit_keys.contains(k));

                    if new_high_profit_count > 0 {
                        crate::audio::play_high_profit_alert();
                        self.set_toast(format!(
                            "🚨 HIGH PROFIT ALERT: Found {} new trade(s) with ≥ {:.0}% Net ROI!",
                            new_high_profit_count, self.sound_min_roi_pct
                        ));
                    } else {
                        // Completely silent on normal refresh
                        self.set_toast(format!("Discovered {} profitable deals ({} manifests)!", count, manifest_count));
                    }

                    self.deals = deals;
                    self.manifests = manifests;
                    self.route = Some(route);
                    self.selected_deal_idx = None;
                }
                ScanResult::Error(err) => {
                    self.status_text = format!("✘ {}", err);
                    self.set_toast(format!("Error: {}", err));
                }
            }
        }

        // Poll chain route worker thread
        if let Ok(res) = self.chain_rx.try_recv() {
            self.chain_is_running = false;
            match res {
                ChainSearchResult::Success(results) => {
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
                }
                ChainSearchResult::Error(err) => {
                    self.set_toast(format!("Chain search error: {}", err));
                }
            }
        }

        if self.chain_is_running {
            ctx.request_repaint_after(Duration::from_millis(100));
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

        // Apply Premium EVE Sci-Fi Theme — deeper void palette
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(7, 9, 26);
        visuals.window_fill = Color32::from_rgb(11, 15, 32);
        visuals.extreme_bg_color = Color32::from_rgb(5, 7, 18);
        visuals.override_text_color = Some(Color32::from_rgb(230, 240, 255));
        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(14, 19, 38);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(28, 40, 65));
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(18, 26, 48);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(0, 180, 240));
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(28, 40, 72);
        visuals.widgets.active.bg_fill = Color32::from_rgb(0, 160, 220);
        visuals.slider_trailing_fill = true;
        ctx.set_visuals(visuals);
        // Global rounded corners + tighter spacing for premium feel
        let mut style = (*ctx.style()).clone();
        style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(4.0);
        style.visuals.widgets.inactive.rounding = egui::Rounding::same(4.0);
        style.visuals.widgets.hovered.rounding = egui::Rounding::same(4.0);
        style.visuals.widgets.active.rounding = egui::Rounding::same(4.0);
        style.spacing.item_spacing = Vec2::new(8.0, 5.0);
        style.spacing.slider_rail_height = 5.0;
        ctx.set_style(style);

        // Top Control and Navigation Panel
        egui::TopBottomPanel::top("top_panel")
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(13, 18, 30))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(30, 42, 68)))
                    .inner_margin(12.0),
            )
            .show(ctx, |ui| {
                self.render_header_and_controls(ui, ctx);
            });

        // Bottom Status Bar & Toast Notifications
        egui::TopBottomPanel::bottom("bottom_panel")
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(10, 14, 22))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(26, 36, 58)))
                    .inner_margin(8.0),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (indicator_color, indicator_sym) = if self.is_scanning {
                        (Color32::from_rgb(255, 200, 0), "🟡 SCANNING")
                    } else if self.status_text.starts_with('✘') {
                        (Color32::from_rgb(255, 80, 80), "🔴 ERROR")
                    } else {
                        (Color32::from_rgb(0, 240, 160), "🟢 READY")
                    };

                    ui.label(RichText::new(indicator_sym).size(11.0).color(indicator_color).strong());
                    if self.auto_refresh {
                        let audio_tag = if self.sound_alerts_enabled { " • 🔔 SOUND ON" } else { " • 🔕 MUTED" };
                        ui.label(
                            RichText::new(format!("• 🔄 AUTO-REFRESH ON{}", audio_tag))
                                .size(11.0)
                                .color(Color32::from_rgb(0, 200, 255))
                                .strong(),
                        );
                    }
                    ui.separator();
                    ui.label(RichText::new(&self.status_text).size(12.0).color(Color32::from_rgb(200, 220, 245)));

                    // Toast notification — fading glowing card
                    if let Some((ref msg, time)) = self.toast {
                        let elapsed = time.elapsed().as_secs_f32();
                        if elapsed < 4.0 {
                            let alpha = ((4.0 - elapsed) / 4.0 * 220.0) as u8;
                            let toast_col = Color32::from_rgba_unmultiplied(0, 220, 255, alpha);
                            let frame_fill = Color32::from_rgba_unmultiplied(0, 15, 35, (alpha / 6).max(10));
                            let frame_stroke = Color32::from_rgba_unmultiplied(0, 190, 255, (alpha / 2).max(18));
                            ui.separator();
                            egui::Frame::none()
                                .fill(frame_fill)
                                .stroke(Stroke::new(1.0_f32, frame_stroke))
                                .rounding(5.0)
                                .inner_margin(6.0)
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(format!("💬 {}", msg))
                                            .size(12.0)
                                            .color(toast_col)
                                            .strong(),
                                    );
                                });
                            ctx.request_repaint();
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("EVE Traveling Merchant • Linux Fedora Native")
                                .size(11.0)
                                .color(Color32::from_rgb(120, 140, 170)),
                        );
                    });
                });
            });

        // Central Panel
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(15, 20, 32))
                    .inner_margin(12.0),
            )
            .show(ctx, |ui| match self.tab {
                ActiveTab::Deals => self.render_deals_tab(ui),
                ActiveTab::Manifests => self.render_manifests_tab(ui),
                ActiveTab::Route => self.render_route_tab(ui),
                ActiveTab::ChainRoute => self.render_chain_route_tab(ui),
                ActiveTab::Settings => self.render_settings_tab(ui),
            });
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

fn format_unit_price_isk(val: f64) -> String {
    if val >= 1_000_000_000.0 {
        format!("{:.2}B", val / 1_000_000_000.0)
    } else if val >= 1_000_000.0 {
        format!("{:.2}M", val / 1_000_000.0)
    } else if val >= 1_000.0 {
        format!("{:.2}k", val / 1_000.0)
    } else {
        format!("{:.2}", val)
    }
}

/// Two stacked mini progress bars: cyan for cargo volume, gold for ISK budget.
fn render_ship_util_bars(ui: &mut egui::Ui, vol_frac: f32, budget_frac: f32) -> egui::Response {
    let bar_w = 80.0_f32;
    let bar_h = 5.0_f32;
    let gap = 3.0_f32;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(bar_w, bar_h * 2.0 + gap), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();

        // Cargo volume bar (cyan, top)
        let vol_track = egui::Rect::from_min_size(rect.min, Vec2::new(bar_w, bar_h));
        painter.rect_filled(vol_track, egui::Rounding::same(2.0), Color32::from_rgb(16, 32, 58));
        let vol_w = (bar_w * vol_frac.clamp(0.0, 1.0)).max(0.0);
        if vol_w > 0.0 {
            let vol_col = if vol_frac >= 0.9 { Color32::from_rgb(255, 90, 50) } else { Color32::from_rgb(0, 200, 255) };
            painter.rect_filled(
                egui::Rect::from_min_size(rect.min, Vec2::new(vol_w, bar_h)),
                egui::Rounding::same(2.0),
                vol_col,
            );
        }

        // ISK budget bar (gold, bottom)
        let isk_min = egui::pos2(rect.min.x, rect.min.y + bar_h + gap);
        let isk_track = egui::Rect::from_min_size(isk_min, Vec2::new(bar_w, bar_h));
        painter.rect_filled(isk_track, egui::Rounding::same(2.0), Color32::from_rgb(38, 28, 10));
        let isk_w = (bar_w * budget_frac.clamp(0.0, 1.0)).max(0.0);
        if isk_w > 0.0 {
            let isk_col = if budget_frac >= 0.9 { Color32::from_rgb(255, 70, 70) } else { Color32::from_rgb(255, 200, 50) };
            painter.rect_filled(
                egui::Rect::from_min_size(isk_min, Vec2::new(isk_w, bar_h)),
                egui::Rounding::same(2.0),
                isk_col,
            );
        }
    }
    resp
}

impl DesktopApp {
    fn render_header_and_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let is_widescreen = ui.available_width() >= 1150.0;

        // Header primary action buttons (Scan, Auto-Refresh, Audio, Zoom, Fullscreen)
        let render_primary_actions = |app: &mut Self, ui: &mut egui::Ui, ctx: &egui::Context| {
            // Fullscreen toggle button
            let fs_label = if app.is_fullscreen { "🗗 Windowed" } else { "⛶ Fullscreen" };
            let fs_btn = egui::Button::new(
                RichText::new(fs_label).size(12.0).color(Color32::from_rgb(0, 220, 255)).strong(),
            )
            .fill(Color32::from_rgb(16, 26, 46));
            if ui.add(fs_btn).on_hover_text("Toggle borderless fullscreen mode (F11)").clicked() {
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
                .fill(Color32::from_rgb(16, 26, 46))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 48, 76)))
                .rounding(4.0)
                .inner_margin(egui::vec2(6.0, 3.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.small_button("➖").on_hover_text("Zoom Out (Ctrl -)").clicked() {
                            app.ui_scale = (app.ui_scale - 0.05).max(0.75);
                            app.set_toast(format!("UI Zoom: {:.0}%", app.ui_scale * 100.0));
                        }
                        let zoom_pct = format!("{:.0}%", app.ui_scale * 100.0);
                        if ui.button(RichText::new(zoom_pct).size(11.5).strong().color(Color32::from_rgb(0, 220, 255)))
                            .on_hover_text("Click to reset zoom to 100% (Ctrl 0)")
                            .clicked()
                        {
                            app.ui_scale = 1.0;
                            app.set_toast("UI Zoom reset to 100%");
                        }
                        if ui.small_button("➕").on_hover_text("Zoom In (Ctrl +)").clicked() {
                            app.ui_scale = (app.ui_scale + 0.05).min(1.50);
                            app.set_toast(format!("UI Zoom: {:.0}%", app.ui_scale * 100.0));
                        }
                    });
                });

            ui.add_space(4.0);

            // Audio alert toggle button
            let (sound_icon, sound_color) = if app.sound_alerts_enabled {
                (
                    format!("🔔 Alert (≥{:.0}%)", app.sound_min_roi_pct),
                    Color32::from_rgb(0, 230, 255),
                )
            } else {
                (
                    "🔕 Muted".to_string(),
                    Color32::from_rgb(130, 145, 170),
                )
            };
            let sound_btn = egui::Button::new(
                RichText::new(sound_icon).size(12.0).color(sound_color).strong(),
            )
            .fill(Color32::from_rgb(16, 24, 42));
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
                Color32::from_rgb(0, 230, 255)
            } else {
                Color32::from_rgb(150, 170, 200)
            };
            let toggle_resp = ui.checkbox(
                &mut app.auto_refresh,
                RichText::new("🔄 Auto-Refresh (5m)")
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
                    .fill(Color32::from_rgb(16, 26, 48))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0, 180, 240)))
                    .rounding(4.0)
                    .inner_margin(egui::vec2(8.0, 5.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(badge_text)
                                .size(12.0)
                                .color(Color32::from_rgb(0, 230, 255))
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
                    RichText::new("⚡ SCAN MARKET NOW")
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
                    RichText::new("🚀 EVE TRAVELING MERCHANT")
                        .size(23.0)
                        .color(Color32::from_rgb(0, 225, 255))
                        .strong(),
                );
                ui.label(
                    RichText::new("│ Arbitrage Engine & Multi-Stop Route Planner")
                        .size(13.0)
                        .color(Color32::from_rgb(140, 160, 190)),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    render_primary_actions(self, ui, ctx);
                });
            });
        } else {
            // Compact / narrow window: title row first, then wrap-safe actions row
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("🚀 EVE TRAVELING MERCHANT")
                        .size(20.0)
                        .color(Color32::from_rgb(0, 225, 255))
                        .strong(),
                );
                ui.label(
                    RichText::new("│ Arbitrage Engine")
                        .size(12.0)
                        .color(Color32::from_rgb(140, 160, 190)),
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
            ui.label(RichText::new("SHIP PROFILE:").size(12.0).color(Color32::from_rgb(150, 180, 220)).strong());
            if self.use_custom_ship {
                let custom_label = format!("✔ Custom ({:.0}k m³ · {:.0}M)", self.custom_cargo_vol / 1000.0, self.custom_budget_isk / 1_000_000.0);
                if ui.selectable_label(true, RichText::new(custom_label).color(Color32::from_rgb(0, 220, 255)).strong()).clicked() {
                    self.tab = ActiveTab::Settings;
                }
                if ui.small_button("⚙ Edit").on_hover_text("Configure ship cargo hold & anti-gank ISK limit in Settings").clicked() {
                    self.tab = ActiveTab::Settings;
                }
                if ui.small_button("↺ Presets").on_hover_text("Switch back to Standard Hauler presets").clicked() {
                    self.use_custom_ship = false;
                }
            } else {
                let big_label = if self.ship_is_big { "✔ Big Hauler (60k m³ · 500M)" } else { "Big Hauler (60k m³ · 500M)" };
                if ui.selectable_label(self.ship_is_big, RichText::new(big_label).strong()).clicked() {
                    self.ship_is_big = true;
                }
                let small_label = if !self.ship_is_big { "✔ Small Hauler (30k m³ · 50M)" } else { "Small Hauler (30k m³ · 50M)" };
                if ui.selectable_label(!self.ship_is_big, RichText::new(small_label).strong()).clicked() {
                    self.ship_is_big = false;
                }
                if ui.small_button("⚙ Custom").on_hover_text("Configure custom cargo hold & anti-gank ISK limit in Settings").clicked() {
                    self.use_custom_ship = true;
                    self.tab = ActiveTab::Settings;
                }
            }

            ui.separator();

            // Home System Input
            ui.label(RichText::new("📍 STARTING SYSTEM:").size(12.0).color(Color32::from_rgb(150, 180, 220)).strong());
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

            // System detected status pill
            let home_id_opt = self.sde.as_ref().and_then(|s| s.get_system_id(&self.home_system));
            if let Some(hid) = home_id_opt {
                let sec = self.graph.as_ref().and_then(|g| g.security(hid)).unwrap_or(1.0);
                let sec_col = security_color(sec);
                let reg_name = self.sde.as_ref().and_then(|s| s.get_system_region(hid))
                    .and_then(|r| self.sde.as_ref().and_then(|s| s.get_region_name(r)))
                    .unwrap_or_else(|| "Unknown".to_string());
                let sys_name = self.sde.as_ref().and_then(|s| s.get_system_name(hid)).unwrap_or_default();
                ui.label(
                    RichText::new(format!("[{:.1}] {} · {}", sec, sys_name, reg_name))
                        .size(11.0)
                        .color(sec_col)
                        .strong(),
                )
                .on_hover_text(format!("System: {} (ID: {})\nSecurity: {:.2}\nRegion: {}\nResolved successfully from: '{}'", sys_name, hid, sec, reg_name, self.home_system));

                if !sys_name.is_empty()
                    && self.home_system.trim().to_lowercase() != sys_name.to_lowercase()
                    && ui
                        .small_button(format!("📍 Use {}", sys_name))
                        .on_hover_text(format!("Clean input to exact solar system name '{}' and scan", sys_name))
                        .clicked()
                {
                    self.home_system = sys_name;
                    self.start_scan();
                }
            } else {
                ui.label(
                    RichText::new("⚠ Unknown System")
                        .size(11.0)
                        .color(Color32::from_rgb(255, 100, 100))
                        .strong(),
                );
            }

            ui.separator();

            // Adjacent Regions Toggle
            ui.checkbox(&mut self.include_adjacent_regions, RichText::new("🌐 +Adjacent Regions").strong())
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
            ui.checkbox(&mut self.safe_only, RichText::new("🛡️ Safe Highsec Only").color(safe_color).strong());

            ui.separator();

            // Station Pool Toggle (NPC Stations Only vs Include Player Citadels)
            let (station_icon, station_text, station_color) = if self.npc_stations_only {
                ("🏛️", "NPC Stations Only", Color32::from_rgb(0, 220, 255))
            } else {
                ("🏰", "+ Citadels Included", Color32::from_rgb(255, 185, 60))
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
            ui.label(RichText::new("MIN NET ROI:").size(12.0).color(Color32::from_rgb(150, 180, 220)).strong());
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
        let home_id_opt = self.sde.as_ref().and_then(|s| s.get_system_id(&self.home_system));
        let trimmed_query = self.home_system.trim();
        if home_id_opt.is_none() && trimmed_query.len() >= 2 {
            let suggestions = self.sde.as_ref().map(|s| s.search_systems(trimmed_query, 6)).unwrap_or_default();
            if !suggestions.is_empty() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("💡 Autocomplete Matches:").size(11.0).color(Color32::from_rgb(255, 210, 0)).strong());
                    for s in suggestions {
                        let sec_col = security_color(s.security_status);
                        let chip_text = format!("{} [{:.1}] ({})", s.system_name, s.security_status, s.region_name);
                        if ui.button(RichText::new(chip_text).size(11.0).color(sec_col)).on_hover_text("Click to set as starting system & scan").clicked() {
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
        let top_profit = self.deals.first().map(|d| d.net_profit).unwrap_or(0.0);
        let top_ppj = self.deals.first().map(|d| d.profit_per_jump).unwrap_or(0.0);
        let deal_count = self.deals.len();
        let top_roi = self.deals.iter().map(|d| d.net_profit_pct).fold(0.0_f64, f64::max);
        let top_density = self.deals.iter().map(|d| d.profit_per_m3).fold(0.0_f64, f64::max);
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
                    .fill(Color32::from_rgba_unmultiplied(0, 190, 110, 24))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0, 155, 85)))
                    .rounding(4.0)
                    .inner_margin(egui::vec2(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new("💎 BEST PROFIT:").size(10.5).color(Color32::from_rgb(0, 190, 110)).strong());
                        ui.label(RichText::new(format!("+{:.2}M ISK", top_profit / 1_000_000.0)).size(12.5).color(Color32::from_rgb(0, 255, 150)).strong());
                    });

                // Pill 2: Top Profit/Jump
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(195, 155, 0, 24))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(165, 125, 0)))
                    .rounding(4.0)
                    .inner_margin(egui::vec2(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new("⚡ TOP PROFIT/JUMP:").size(10.5).color(Color32::from_rgb(195, 155, 0)).strong());
                        ui.label(RichText::new(format!("{:.2}M / jump", top_ppj / 1_000_000.0)).size(12.5).color(Color32::from_rgb(255, 215, 0)).strong());
                    });

                // Pill 3: Max ROI
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(255, 120, 0, 24))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(210, 95, 0)))
                    .rounding(4.0)
                    .inner_margin(egui::vec2(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new("🔥 MAX ROI:").size(10.5).color(Color32::from_rgb(255, 140, 0)).strong());
                        ui.label(RichText::new(format!("+{:.1}%", top_roi)).size(12.5).color(Color32::from_rgb(255, 185, 60)).strong());
                    });

                // Pill 4: Density
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(0, 200, 200, 24))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0, 160, 160)))
                    .rounding(4.0)
                    .inner_margin(egui::vec2(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new("📦 DENSITY:").size(10.5).color(Color32::from_rgb(0, 200, 200)).strong());
                        ui.label(RichText::new(format!("{} ISK/m³", format_rate_isk(top_density))).size(12.5).color(Color32::from_rgb(0, 245, 245)).strong());
                    });

                // Pill 5: Deals Count
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(0, 145, 205, 24))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0, 125, 185)))
                    .rounding(4.0)
                    .inner_margin(egui::vec2(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new("📊 DEALS:").size(10.5).color(Color32::from_rgb(0, 180, 240)).strong());
                        ui.label(RichText::new(format!("{} profitable", deal_count)).size(12.5).color(Color32::from_rgb(0, 220, 255)).strong());
                    });

                // Pill 6: Fleet
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(120, 140, 180, 24))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(90, 110, 150)))
                    .rounding(4.0)
                    .inner_margin(egui::vec2(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new("🛡️ FLEET:").size(10.5).color(Color32::from_rgb(150, 175, 210)).strong());
                        ui.label(RichText::new(ship_name).size(12.5).color(Color32::from_rgb(200, 220, 255)).strong());
                    });

                let toggle_label = if self.compact_kpi { "▼ Expand KPIs" } else { "▲ Compact KPIs" };
                if ui.small_button(toggle_label).on_hover_text("Toggle between compact bar and full KPI cards").clicked() {
                    self.compact_kpi = !self.compact_kpi;
                }
            });
        } else if is_widescreen {
            // 6-column fluid metric grid for widescreen / fullscreen displays
            ui.columns(6, |cols| {
                // Card 1: Best Profit
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(0, 190, 110, 18))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0, 155, 85)))
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(&mut cols[0], |ui| {
                        ui.label(RichText::new("💎  BEST PROFIT").size(10.0).color(Color32::from_rgb(0, 140, 75)));
                        ui.label(RichText::new(format!("+{:.2}M ISK", top_profit / 1_000_000.0)).size(18.0).color(Color32::from_rgb(0, 255, 150)).strong());
                    });

                // Card 2: Top Profit/Jump
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(195, 155, 0, 18))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(165, 125, 0)))
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(&mut cols[1], |ui| {
                        ui.label(RichText::new("⚡  TOP PROFIT / JUMP").size(10.0).color(Color32::from_rgb(145, 108, 0)));
                        ui.label(RichText::new(format!("{:.2}M / jump", top_ppj / 1_000_000.0)).size(18.0).color(Color32::from_rgb(255, 215, 0)).strong());
                    });

                // Card 3: Max Net ROI
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(255, 120, 0, 18))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(210, 95, 0)))
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(&mut cols[2], |ui| {
                        ui.label(RichText::new("🔥  MAX NET ROI").size(10.0).color(Color32::from_rgb(200, 90, 0)));
                        ui.label(RichText::new(format!("+{:.1}%", top_roi)).size(18.0).color(Color32::from_rgb(255, 170, 50)).strong());
                    });

                // Card 4: Profit Density
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(0, 200, 200, 18))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0, 160, 160)))
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(&mut cols[3], |ui| {
                        ui.label(RichText::new("📦  PROFIT DENSITY").size(10.0).color(Color32::from_rgb(0, 150, 150)));
                        ui.label(RichText::new(format!("{} ISK/m³", format_rate_isk(top_density))).size(18.0).color(Color32::from_rgb(0, 240, 240)).strong());
                    });

                // Card 5: Deals Discovered
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(0, 145, 205, 18))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0, 125, 185)))
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(&mut cols[4], |ui| {
                        ui.label(RichText::new("📊  ACTIVE DEALS").size(10.0).color(Color32::from_rgb(0, 115, 172)));
                        ui.label(RichText::new(format!("{} profitable", deal_count)).size(18.0).color(Color32::from_rgb(0, 210, 255)).strong());
                    });

                // Card 6: Ship & Safety with collapse button
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(120, 140, 180, 18))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(90, 110, 150)))
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(&mut cols[5], |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("🛡️  FLEET PROFILE").size(10.0).color(Color32::from_rgb(120, 145, 180)));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("▲").on_hover_text("Collapse to compact KPI strip").clicked() {
                                    self.compact_kpi = true;
                                }
                            });
                        });
                        ui.label(RichText::new(ship_name).size(15.0).color(Color32::from_rgb(200, 220, 255)).strong());
                    });
            });
        } else {
            ui.columns(3, |cols| {
                // Badge 1: Best Profit (emerald green)
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(0, 190, 110, 18))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0, 155, 85)))
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(&mut cols[0], |ui| {
                        ui.label(
                            RichText::new("💎  BEST PROFIT")
                                .size(10.0)
                                .color(Color32::from_rgb(0, 140, 75)),
                        );
                        ui.label(
                            RichText::new(format!("+{:.2}M ISK", top_profit / 1_000_000.0))
                                .size(20.0)
                                .color(Color32::from_rgb(0, 255, 150))
                                .strong(),
                        );
                    });

                // Badge 2: Top Profit/Jump (gold)
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(195, 155, 0, 18))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(165, 125, 0)))
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(&mut cols[1], |ui| {
                        ui.label(
                            RichText::new("⚡  TOP PROFIT / JUMP")
                                .size(10.0)
                                .color(Color32::from_rgb(145, 108, 0)),
                        );
                        ui.label(
                            RichText::new(format!("{:.2}M / jump", top_ppj / 1_000_000.0))
                                .size(20.0)
                                .color(Color32::from_rgb(255, 215, 0))
                                .strong(),
                        );
                    });

                // Badge 3: Deals found (cyan) with collapse button
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(0, 145, 205, 18))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0, 125, 185)))
                    .rounding(6.0)
                    .inner_margin(8.0)
                    .show(&mut cols[2], |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("📊  DEALS DISCOVERED")
                                    .size(10.0)
                                    .color(Color32::from_rgb(0, 115, 172)),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("▲").on_hover_text("Collapse to compact KPI strip").clicked() {
                                    self.compact_kpi = true;
                                }
                            });
                        });
                        ui.label(
                            RichText::new(format!("{} profitable", deal_count))
                                .size(20.0)
                                .color(Color32::from_rgb(0, 210, 255))
                                .strong(),
                        );
                    });
            });
        }
    }

    fn render_tab_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let tabs: &[(ActiveTab, String)] = &[
                (ActiveTab::Deals,     format!("📊 Deals ({})",     self.deals.len())),
                (ActiveTab::Manifests, format!("📦 Manifests ({})", self.manifests.len())),
                (ActiveTab::Route,     format!("🗺️ Route ({})",      self.route.as_ref().map(|r| r.stops.len()).unwrap_or(0))),
                (ActiveTab::ChainRoute, format!("🔗 Chain Route ({})", self.chain_results.len())),
                (ActiveTab::Settings,  "⚙️ Settings & Fees".to_string()),
            ];

            let mut new_tab: Option<ActiveTab> = None;

            for (tab, label) in tabs {
                let is_active = self.tab == *tab;
                let text_col = if is_active {
                    Color32::from_rgb(0, 220, 255)
                } else {
                    Color32::from_rgb(130, 165, 205)
                };
                let fill = if is_active {
                    Color32::from_rgba_unmultiplied(0, 180, 255, 18)
                } else {
                    Color32::TRANSPARENT
                };

                let resp = ui.add(
                    egui::Button::new(
                        RichText::new(label.as_str()).size(13.5).color(text_col).strong(),
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
                        [egui::pos2(resp.rect.left() - 1.0, y), egui::pos2(resp.rect.right() + 1.0, y)],
                        Stroke::new(4.0_f32, Color32::from_rgba_unmultiplied(0, 180, 255, 45)),
                    );
                    // Bright core
                    painter.line_segment(
                        [egui::pos2(resp.rect.left(), y), egui::pos2(resp.rect.right(), y)],
                        Stroke::new(2.0_f32, Color32::from_rgb(0, 220, 255)),
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
        if self.deals.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.heading(
                    RichText::new("No Deals Available")
                        .size(22.0)
                        .color(Color32::from_rgb(150, 170, 200)),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Click the '⚡ SCAN MARKET NOW' button above to fetch live orders from CCP's ESI API.")
                        .size(14.0)
                        .color(Color32::GRAY),
                );
                ui.add_space(16.0);
                let btn = egui::Button::new(
                    RichText::new("🚀 Start First Market Scan")
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
        egui::Frame::none()
            .fill(Color32::from_rgb(18, 25, 40))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 45, 70)))
            .rounding(6.0)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("🔍 Search:").strong());
                    let search_w = if ui.available_width() < 900.0 { 130.0 } else { 180.0 };
                    ui.add(egui::TextEdit::singleline(&mut self.filter_item_name).desired_width(search_w))
                        .on_hover_text("Search by item name, station, or system\nSupports Roman numerals ('Jita IV') and Arabic numbers ('Jita 4')");
                    if !self.filter_item_name.is_empty() && ui.button("✖ Clear").clicked() {
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
                        ("🚫", "Hide NPC Trades: ON", Color32::from_rgb(180, 50, 40), Color32::from_rgb(255, 90, 80), Color32::WHITE)
                    } else {
                        ("🏛️", "NPC Trades: Visible", Color32::from_rgba_unmultiplied(20, 36, 60, 200), Color32::from_rgb(60, 110, 170), Color32::from_rgb(160, 210, 255))
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
                    let export_btn = egui::Button::new(
                        RichText::new("📥 Export Deals to CSV")
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

                        let csv_str = export_deals_to_csv(&export_filtered);
                        copy_to_clipboard(ui, csv_str.clone());
                        let _ = std::fs::write("deals_export.csv", &csv_str);
                        self.set_toast(format!("Exported {} deals to ./deals_export.csv & copied to clipboard!", export_filtered.len()));
                    }
                });
            });

        ui.add_space(8.0);

        // Filter and sort candidate deals
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

        match self.sort_by {
            DealSortBy::ProfitPerJump => {
                filtered.sort_by(|(_, a), (_, b)| {
                    if self.sort_descending {
                        b.profit_per_jump.partial_cmp(&a.profit_per_jump).unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        a.profit_per_jump.partial_cmp(&b.profit_per_jump).unwrap_or(std::cmp::Ordering::Equal)
                    }
                });
            }
            DealSortBy::TotalProfit => {
                filtered.sort_by(|(_, a), (_, b)| {
                    if self.sort_descending {
                        b.net_profit.partial_cmp(&a.net_profit).unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        a.net_profit.partial_cmp(&b.net_profit).unwrap_or(std::cmp::Ordering::Equal)
                    }
                });
            }
            DealSortBy::RoiPct => {
                filtered.sort_by(|(_, a), (_, b)| {
                    if self.sort_descending {
                        b.net_profit_pct.partial_cmp(&a.net_profit_pct).unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        a.net_profit_pct.partial_cmp(&b.net_profit_pct).unwrap_or(std::cmp::Ordering::Equal)
                    }
                });
            }
            DealSortBy::ProfitPerM3 => {
                filtered.sort_by(|(_, a), (_, b)| {
                    if self.sort_descending {
                        b.profit_per_m3.partial_cmp(&a.profit_per_m3).unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        a.profit_per_m3.partial_cmp(&b.profit_per_m3).unwrap_or(std::cmp::Ordering::Equal)
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
                        b.capital_required.partial_cmp(&a.capital_required).unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        a.capital_required.partial_cmp(&b.capital_required).unwrap_or(std::cmp::Ordering::Equal)
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
                        b.source_system_name.to_lowercase().cmp(&a.source_system_name.to_lowercase())
                    } else {
                        a.source_system_name.to_lowercase().cmp(&b.source_system_name.to_lowercase())
                    }
                });
            }
            DealSortBy::BuyPrice => {
                filtered.sort_by(|(_, a), (_, b)| {
                    if self.sort_descending {
                        b.buy_price.partial_cmp(&a.buy_price).unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        a.buy_price.partial_cmp(&b.buy_price).unwrap_or(std::cmp::Ordering::Equal)
                    }
                });
            }
            DealSortBy::SellSystem => {
                filtered.sort_by(|(_, a), (_, b)| {
                    if self.sort_descending {
                        b.dest_system_name.to_lowercase().cmp(&a.dest_system_name.to_lowercase())
                    } else {
                        a.dest_system_name.to_lowercase().cmp(&b.dest_system_name.to_lowercase())
                    }
                });
            }
            DealSortBy::SellPrice => {
                filtered.sort_by(|(_, a), (_, b)| {
                    if self.sort_descending {
                        b.sell_price.partial_cmp(&a.sell_price).unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        a.sell_price.partial_cmp(&b.sell_price).unwrap_or(std::cmp::Ordering::Equal)
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
                        b.volume_used.partial_cmp(&a.volume_used).unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        a.volume_used.partial_cmp(&b.volume_used).unwrap_or(std::cmp::Ordering::Equal)
                    }
                });
            }
        }

        // Responsive Layout: Choose between Side Panel (on wide screens) and Bottom Drawer (compact screens)
        let avail_w = ui.available_width();
        let avail_h = ui.available_height();
        let has_selection = self.selected_deal_idx.is_some();
        let use_side_panel = has_selection && self.dossier_side_panel && avail_w >= 1150.0;

        if use_side_panel {
            let side_w = (avail_w * 0.36).clamp(380.0, 680.0);
            let table_w = (avail_w - side_w - 10.0).max(300.0);

            let mut grid_res = (None, None, None);
            let mut dossier_res = (false, false, None);

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

            let (close_req, toggle_dock, d_toast) = dossier_res;
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

            let (toggle_select, action_toast, change_sort) = grid_res;
            if let Some(idx) = toggle_select {
                if self.selected_deal_idx == Some(idx) {
                    self.selected_deal_idx = None;
                } else {
                    self.selected_deal_idx = Some(idx);
                }
            }
            if let Some(msg) = action_toast {
                self.set_toast(msg);
            }
            if let Some((new_sort, new_desc)) = change_sort {
                self.sort_by = new_sort;
                self.sort_descending = new_desc;
            }
        } else {
            let table_height = if has_selection {
                (avail_h * 0.58).max(180.0)
            } else {
                avail_h
            };

            let mut grid_res = (None, None, None);
            ui.allocate_ui(Vec2::new(avail_w, table_height), |ui| {
                grid_res = self.render_deals_table_grid(ui, &filtered);
            });

            let (toggle_select, action_toast, change_sort) = grid_res;
            if let Some(idx) = toggle_select {
                if self.selected_deal_idx == Some(idx) {
                    self.selected_deal_idx = None;
                } else {
                    self.selected_deal_idx = Some(idx);
                }
            }
            if let Some(msg) = action_toast {
                self.set_toast(msg);
            }
            if let Some((new_sort, new_desc)) = change_sort {
                self.sort_by = new_sort;
                self.sort_descending = new_desc;
            }

            // Bottom Selected Deal Dossier (bounded and vertically scrollable)
            if let Some(idx) = self.selected_deal_idx {
                if let Some(deal) = self.deals.get(idx) {
                    ui.add_space(6.0);
                    let dossier_h = ui.available_height();
                    let mut dossier_res = (false, false, None);
                    ui.allocate_ui(Vec2::new(avail_w, dossier_h), |ui| {
                        dossier_res = self.render_deal_dossier(ui, deal, false);
                    });

                    let (close_req, toggle_dock, d_toast) = dossier_res;
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
                }
            }
        }
    }

    fn render_deals_table_grid(
        &self,
        ui: &mut egui::Ui,
        filtered: &[(usize, &ArbitrageDeal)],
    ) -> (Option<usize>, Option<String>, Option<(DealSortBy, bool)>) {
        let mut toggle_select: Option<usize> = None;
        let mut action_toast: Option<String> = None;
        let mut change_sort: Option<(DealSortBy, bool)> = None;
        let current_selected_idx = self.selected_deal_idx;
        let current_sort = self.sort_by;
        let is_desc = self.sort_descending;
        let is_big = self.ship_is_big;

        let avail_w = ui.available_width();
        let is_compact_tier = avail_w < 1000.0;

        let mut render_header_btn = |ui: &mut egui::Ui, title: &str, sort_target: DealSortBy| {
            let is_active = current_sort == sort_target;
            let arrow = if is_active {
                if is_desc { " ▼" } else { " ▲" }
            } else {
                ""
            };
            let label_text = format!("{}{}", title, arrow);
            let text_color = if is_active {
                Color32::from_rgb(255, 215, 0)
            } else {
                Color32::from_rgb(255, 205, 0)
            };
            let resp = ui.add(
                egui::Button::new(RichText::new(label_text).color(text_color).strong())
                    .frame(false),
            )
            .on_hover_text(format!("Click to sort by {} ({})", title, if is_active && is_desc { "switch to Ascending" } else { "switch to Descending" }));
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
        let actions_w = (avail_w * 0.09).clamp(125.0, 160.0);
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

        let total_w = w_item + w_buy_sys + w_buy_price + w_sell_sys + w_sell_price + w_qty + w_vol + w_profit_m3 + w_cost + w_ship_util + w_net_profit + w_net_roi + w_jumps + w_profit_jump;

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
            .column(Column::initial(col_buy_price).at_least(60.0).resizable(true))  // 4: BUY / ITEM
            .column(Column::initial(col_sell_sys).at_least(70.0).resizable(true)) // 5: SELL SYSTEM
            .column(Column::initial(col_sell_price).at_least(60.0).resizable(true))  // 6: SELL / ITEM
            .column(Column::initial(col_qty).at_least(40.0).resizable(true))  // 7: QTY
            .column(Column::initial(col_vol).at_least(45.0).resizable(true)); // 8: VOL (m³)

        if !is_compact_tier {
            builder = builder.column(Column::initial(col_profit_m3).at_least(55.0).resizable(true)); // 9: PROFIT/m³
        }

        builder = builder.column(Column::initial(col_cost).at_least(55.0).resizable(true)); // 10: COST (M)

        if !is_compact_tier {
            builder = builder.column(Column::initial(col_ship_util).at_least(60.0).resizable(true)); // 11: SHIP UTIL
        }

        builder = builder
            .column(Column::initial(col_net_profit).at_least(75.0).resizable(true)) // 12: NET PROFIT
            .column(Column::initial(col_net_roi).at_least(45.0).resizable(true))  // 13: NET ROI
            .column(Column::initial(col_jumps).at_least(35.0).resizable(true))  // 14: JUMPS
            .column(Column::initial(col_profit_jump).at_least(75.0).resizable(true)) // 15: PROFIT/JUMP
            .column(Column::initial(actions_w).at_least(115.0).resizable(true)); // 16: QUICK ACTIONS

        builder
            .header(24.0, |mut header| {
                header.col(|ui| { ui.label(RichText::new("#").color(Color32::from_rgb(255, 205, 0)).strong()); });
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
                    header.col(|ui| { ui.label(RichText::new("SHIP UTIL").color(Color32::from_rgb(255, 205, 0)).strong()); });
                }
                header.col(|ui| { render_header_btn(ui, "NET PROFIT", DealSortBy::TotalProfit); });
                header.col(|ui| { render_header_btn(ui, "NET ROI", DealSortBy::RoiPct); });
                header.col(|ui| { render_header_btn(ui, "JUMPS", DealSortBy::Jumps); });
                header.col(|ui| { render_header_btn(ui, "PROFIT/JUMP", DealSortBy::ProfitPerJump); });
                header.col(|ui| { ui.label(RichText::new("QUICK ACTIONS").color(Color32::from_rgb(255, 205, 0)).strong()); });
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
                                1 => Color32::from_rgb(200, 215, 230), // Silver
                                2 => Color32::from_rgb(205, 127, 50),  // Bronze
                                _ => Color32::GRAY,
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
                                Color32::from_rgb(0, 230, 255)
                            } else {
                                Color32::WHITE
                            };
                            ui.horizontal(|ui| {
                                if ui.selectable_label(is_selected, RichText::new(&deal.item_name).color(item_color).strong()).clicked() {
                                    toggle_select = Some(orig_idx);
                                }
                                if deal.is_high_roi() {
                                    ui.label(RichText::new("🔥 40%+").size(10.0).color(Color32::from_rgb(255, 120, 0)).strong())
                                        .on_hover_text("High-ROI deal: Net return >= 40%");
                                }
                                if deal.is_npc_trade {
                                    ui.label(RichText::new("🏛️ NPC").size(10.0).color(Color32::from_rgb(110, 190, 255)).strong())
                                        .on_hover_text("NPC trade deal: Seeded commodity or 365-day NPC order");
                                }
                                if !crate::arbitrage::is_npc_station(deal.source_station_id) || !crate::arbitrage::is_npc_station(deal.dest_station_id) {
                                    ui.label(RichText::new("🏰").color(Color32::from_rgb(255, 185, 60)).strong())
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
                                ui.label(RichText::new(format_rate_isk(deal.profit_per_m3)).color(Color32::from_rgb(0, 220, 220)).strong());
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
                                    .color(Color32::from_rgb(0, 240, 150))
                                    .strong(),
                            );
                        });

                        // 13: Net ROI
                        row.col(|ui| {
                            let roi_col = if deal.is_high_roi() {
                                Color32::from_rgb(255, 140, 0)
                            } else {
                                Color32::from_rgb(0, 255, 180)
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
                            ui.label(RichText::new(jumps_tag).color(Color32::LIGHT_GRAY));
                        });

                        // 15: Profit / Jump
                        row.col(|ui| {
                            ui.label(
                                RichText::new(format!("{:.2}M", deal.profit_per_jump / 1_000_000.0))
                                    .color(Color32::from_rgb(255, 215, 0))
                                    .strong(),
                            );
                        });

                        // 16: Quick Actions
                        row.col(|ui| {
                            ui.horizontal(|ui| {
                                if ui.small_button("📋 Item").on_hover_text("Copy item name to clipboard").clicked() {
                                    copy_to_clipboard(ui, deal.item_name.clone());
                                    action_toast = Some(format!("Copied item '{}' to clipboard!", deal.item_name));
                                }
                                if ui.small_button("📋 Buy").on_hover_text(format!("Copy buy station '{}' to clipboard", deal.source_station_name)).clicked() {
                                    copy_to_clipboard(ui, deal.source_station_name.clone());
                                    action_toast = Some(format!("Copied buy station '{}' to clipboard!", deal.source_station_name));
                                }
                                if ui.small_button("📋 Sell").on_hover_text(format!("Copy sell station '{}' to clipboard", deal.dest_station_name)).clicked() {
                                    copy_to_clipboard(ui, deal.dest_station_name.clone());
                                    action_toast = Some(format!("Copied sell station '{}' to clipboard!", deal.dest_station_name));
                                }
                            });
                        });
                    });
                }
            });

        (toggle_select, action_toast, change_sort)
    }

    fn render_deal_dossier(
        &self,
        ui: &mut egui::Ui,
        deal: &ArbitrageDeal,
        is_side_panel: bool,
    ) -> (bool, bool, Option<String>) {
        let mut close_requested = false;
        let mut toggle_dock_requested = false;
        let mut toast = None;

        egui::Frame::none()
            .fill(Color32::from_rgb(18, 25, 42))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0, 200, 255)))
            .rounding(6.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                // Header row
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("🔍 DOSSIER: {}", deal.item_name))
                            .size(15.0)
                            .color(Color32::from_rgb(0, 220, 255))
                            .strong(),
                    );
                    if ui.small_button("📋 Copy Name").clicked() {
                        copy_to_clipboard(ui, deal.item_name.clone());
                        toast = Some(format!("Copied '{}'", deal.item_name));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✖ Close").clicked() {
                            close_requested = true;
                        }
                        let dock_text = if is_side_panel { "▬ Dock Bottom" } else { "◫ Dock Side" };
                        let can_dock_side = ui.available_width() >= 1150.0 || is_side_panel;
                        let dock_btn = egui::Button::new(RichText::new(dock_text).size(12.0));
                        let resp = ui.add_enabled(can_dock_side, dock_btn);
                        if resp.clicked() {
                            toggle_dock_requested = true;
                        }
                        if !can_dock_side && !is_side_panel {
                            resp.on_hover_text("Side docking requires >= 1150px window width");
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
                            egui::Frame::none()
                                .fill(Color32::from_rgb(14, 20, 32))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(30, 48, 76)))
                                .rounding(6.0)
                                .inner_margin(8.0)
                                .show(ui, |ui| {
                                    ui.heading(RichText::new("🛒 PURCHASE").size(13.0).color(Color32::from_rgb(100, 200, 255)));
                                    let src_is_npc = crate::arbitrage::is_npc_station(deal.source_station_id);
                                    let (src_badge, src_badge_color) = if src_is_npc {
                                        ("🏛️ NPC Station", Color32::from_rgb(100, 210, 255))
                                    } else {
                                        ("🏰 Upwell Citadel", Color32::from_rgb(255, 185, 60))
                                    };
                                    ui.label(RichText::new(&deal.source_station_name).size(12.0).strong());
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(src_badge).size(11.0).color(src_badge_color).strong());
                                        if deal.is_npc_trade {
                                            ui.label(RichText::new("📦 NPC Commodity").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());
                                        }
                                    });
                                    let src_sec_col = security_color(deal.source_security);
                                    ui.label(
                                        RichText::new(format!("System: {} [{:.1}] ({} jumps from home)", deal.source_system_name, deal.source_security, deal.jumps_from_home))
                                            .color(src_sec_col),
                                    );
                                    ui.label(format!("Buy Price / Item: {:.2} ISK (Sell Order)", deal.buy_price));
                                    ui.label(format!("Quantity: {} units", deal.quantity));
                                    ui.label(format!("Capital Cost: {:.2}M ISK", deal.capital_required / 1_000_000.0));
                                    if ui.small_button("📋 Copy Buy Station").clicked() {
                                        copy_to_clipboard(ui, deal.source_station_name.clone());
                                        *toast = Some(format!("Copied buy station '{}'", deal.source_station_name));
                                    }
                                });
                        };

                        let render_sale = |ui: &mut egui::Ui, toast: &mut Option<String>| {
                            egui::Frame::none()
                                .fill(Color32::from_rgb(14, 20, 32))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(30, 48, 76)))
                                .rounding(6.0)
                                .inner_margin(8.0)
                                .show(ui, |ui| {
                                    ui.heading(RichText::new("💰 SALE").size(13.0).color(Color32::from_rgb(255, 140, 240)));
                                    let dst_is_npc = crate::arbitrage::is_npc_station(deal.dest_station_id);
                                    let (dst_badge, dst_badge_color) = if dst_is_npc {
                                        ("🏛️ NPC Station", Color32::from_rgb(100, 210, 255))
                                    } else {
                                        ("🏰 Upwell Citadel", Color32::from_rgb(255, 185, 60))
                                    };
                                    ui.label(RichText::new(&deal.dest_station_name).size(12.0).strong());
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(dst_badge).size(11.0).color(dst_badge_color).strong());
                                        if deal.is_npc_trade {
                                            ui.label(RichText::new("📦 NPC Commodity").size(11.0).color(Color32::from_rgb(0, 220, 255)).strong());
                                        }
                                    });
                                    let dst_sec_col = security_color(deal.dest_security);
                                    ui.label(
                                        RichText::new(format!("System: {} [{:.1}] ({} jumps delivery)", deal.dest_system_name, deal.dest_security, deal.jumps_source_dest))
                                            .color(dst_sec_col),
                                    );
                                    ui.label(format!("Sell Price / Item: {:.2} ISK (Buy Order)", deal.sell_price));
                                    ui.label(format!("Gross Revenue: {:.2}M ISK", (deal.sell_price * deal.quantity as f64) / 1_000_000.0));
                                    ui.label(format!("Price Spread: +{:.1}%", deal.spread_pct));
                                    if ui.small_button("📋 Copy Sell Station").clicked() {
                                        copy_to_clipboard(ui, deal.dest_station_name.clone());
                                        *toast = Some(format!("Copied sell station '{}'", deal.dest_station_name));
                                    }
                                });
                        };

                        let ship = self.current_ship();
                        let max_vol = ship.max_volume;
                        let max_budget = ship.max_budget;
                        let vol_pct = (deal.volume_used / max_vol) * 100.0;
                        let budget_pct = (deal.capital_required / max_budget) * 100.0;

                        let render_financial = |ui: &mut egui::Ui| {
                            egui::Frame::none()
                                .fill(Color32::from_rgb(14, 20, 32))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(30, 48, 76)))
                                .rounding(6.0)
                                .inner_margin(8.0)
                                .show(ui, |ui| {
                                    ui.heading(RichText::new("📈 FINANCIAL SUMMARY").size(13.0).color(Color32::from_rgb(0, 240, 160)));
                                    ui.label(
                                        RichText::new(format!("NET PROFIT: +{:.2}M ISK", deal.net_profit / 1_000_000.0))
                                            .color(Color32::from_rgb(0, 255, 150))
                                            .strong(),
                                    );
                                    ui.label(
                                        RichText::new(format!("NET ROI: +{:.1}%", deal.net_profit_pct))
                                            .color(Color32::from_rgb(0, 240, 180))
                                            .strong(),
                                    );
                                    ui.label(
                                        RichText::new(format!("PROFIT / JUMP: {:.2}M ISK", deal.profit_per_jump / 1_000_000.0))
                                            .color(Color32::from_rgb(255, 215, 0))
                                            .strong(),
                                    );
                                    ui.label(format!("Profit Density: {} ISK/m³", format_rate_isk(deal.profit_per_m3)));
                                    ui.label(format!("Cargo: {:.1} m³ ({:.1}%)", deal.volume_used, vol_pct));
                                    ui.label(format!("Budget: {:.2}M ({:.1}%)", deal.capital_required / 1_000_000.0, budget_pct));
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

        (close_requested, toggle_dock_requested, toast)
    }

    fn render_manifest_card(
        &self,
        ui: &mut egui::Ui,
        idx: usize,
        m: &TradeCorridorManifest,
        max_vol: f64,
        max_budget: f64,
    ) -> Option<String> {
        let mut toast = None;
        let vol_pct = (m.total_volume_used / max_vol) * 100.0;
        let budget_pct = (m.total_capital_required / max_budget) * 100.0;
        let src_sec_col = security_color(m.source_security);
        let dst_sec_col = security_color(m.dest_security);

        egui::Frame::none()
            .fill(Color32::from_rgb(17, 24, 38))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 60, 90)))
            .rounding(6.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                // Header row with route & profit badge
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("MANIFEST #{}", idx + 1))
                            .color(Color32::from_rgb(255, 205, 0))
                            .size(14.0)
                            .strong(),
                    );
                    ui.separator();
                    ui.label(RichText::new(format!("[{:.1}]", m.source_security)).color(src_sec_col).strong());
                    ui.label(RichText::new(&m.source_system_name).color(Color32::from_rgb(0, 210, 255)).strong());
                    ui.label(RichText::new("➔").color(Color32::WHITE).strong());
                    ui.label(RichText::new(format!("[{:.1}]", m.dest_security)).color(dst_sec_col).strong());
                    ui.label(RichText::new(&m.dest_system_name).color(Color32::from_rgb(255, 140, 240)).strong());
                    ui.label(format!("({} jumps)", m.jumps));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("+{:.2}M ISK Net Profit (+{:.1}% ROI)", m.total_net_profit / 1_000_000.0, m.overall_roi_pct))
                                .color(Color32::from_rgb(0, 255, 140))
                                .size(13.5)
                                .strong(),
                        );
                    });
                });

                ui.add_space(6.0);

                // Metrics bar
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("⚡ Profit/Jump: {:.2}M/jump", m.profit_per_jump / 1_000_000.0))
                            .color(Color32::from_rgb(255, 215, 0))
                            .strong(),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!("📦 Cargo Hold: {:.1} / {:.0} m³ ({:.1}%)", m.total_volume_used, max_vol, vol_pct))
                            .color(Color32::from_rgb(180, 210, 255)),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!("💰 Budget Used: {:.2}M / {:.0}M ({:.1}%)", m.total_capital_required / 1_000_000.0, max_budget / 1_000_000.0, budget_pct))
                            .color(Color32::from_rgb(255, 200, 120)),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!("Items: {} distinct deals packed", m.deals.len()))
                            .color(Color32::LIGHT_GRAY),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("📋 Multi-Buy").on_hover_text("Copy in EVE Online Multi-Buy format: '<Item Name>\\t<Quantity>\\n' for in-game paste").clicked() {
                            let mb = m.deals.iter().map(|d| format!("{}\t{}", d.item_name, d.quantity)).collect::<Vec<_>>().join("\n");
                            copy_to_clipboard(ui, mb);
                            toast = Some(format!("Copied {} items in Multi-Buy format!", m.deals.len()));
                        }
                        if ui.small_button("📋 Copy Shopping List").on_hover_text("Copy item names and quantities to clipboard").clicked() {
                            let list = m.deals.iter().map(|d| format!("{}x {}", d.quantity, d.item_name)).collect::<Vec<_>>().join("\n");
                            copy_to_clipboard(ui, list);
                            toast = Some(format!("Copied {} manifest items to clipboard!", m.deals.len()));
                        }
                        if ui.small_button("📋 Copy Buy Station").clicked() {
                            copy_to_clipboard(ui, m.source_station_name.clone());
                            toast = Some(format!("Copied '{}'", m.source_station_name));
                        }
                        if ui.small_button("📋 Copy Sell Station").clicked() {
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
                        ui.label(RichText::new("PACKED ITEM").color(Color32::from_rgb(255, 205, 0)).strong());
                        ui.label(RichText::new("QTY").color(Color32::from_rgb(255, 205, 0)).strong());
                        ui.label(RichText::new("VOL (m³)").color(Color32::from_rgb(255, 205, 0)).strong());
                        ui.label(RichText::new("BUY / UNIT").color(Color32::from_rgb(255, 205, 0)).strong());
                        ui.label(RichText::new("SELL / UNIT").color(Color32::from_rgb(255, 205, 0)).strong());
                        ui.label(RichText::new("COST (M)").color(Color32::from_rgb(255, 205, 0)).strong());
                        ui.label(RichText::new("PROFIT (M)").color(Color32::from_rgb(255, 205, 0)).strong());
                        ui.label(RichText::new("ROI %").color(Color32::from_rgb(255, 205, 0)).strong());
                        ui.label(RichText::new("PROFIT/m³").color(Color32::from_rgb(255, 205, 0)).strong());
                        ui.end_row();

                        for d in &m.deals {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&d.item_name).strong());
                                if d.is_high_roi() {
                                    ui.label(RichText::new("🔥 40%+").size(10.0).color(Color32::from_rgb(255, 120, 0)).strong());
                                }
                            });
                            ui.label(format!("{}", d.quantity));
                            ui.label(format!("{:.1}", d.volume_used));
                            ui.label(format!("{:.2}", d.buy_price));
                            ui.label(format!("{:.2}", d.sell_price));
                            ui.label(format!("{:.2}M", d.capital_required / 1_000_000.0));
                            ui.label(RichText::new(format!("+{:.2}M", d.net_profit / 1_000_000.0)).color(Color32::from_rgb(0, 240, 150)).strong());
                            ui.label(RichText::new(format!("+{:.1}%", d.net_profit_pct)).color(Color32::from_rgb(0, 255, 180)).strong());
                            ui.label(RichText::new(format_rate_isk(d.profit_per_m3)).color(Color32::from_rgb(0, 220, 220)));
                            ui.end_row();
                        }
                    });
            });

        toast
    }

    fn render_manifests_tab(&mut self, ui: &mut egui::Ui) {
        if self.manifests.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.heading(
                    RichText::new("No Packed Corridor Manifests Available")
                        .size(22.0)
                        .color(Color32::from_rgb(150, 170, 200)),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "Manifests group multiple profitable items along the exact same flight path to pack your cargo hold and capital budget.\n\
                         Run a market scan (or enable +Adjacent Regions) to discover multi-item corridor opportunities.",
                    )
                    .size(14.0)
                    .color(Color32::GRAY),
                );
                ui.add_space(16.0);
                let btn = egui::Button::new(
                    RichText::new("⚡ Scan Market Now")
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

        // Header info card
        egui::Frame::none()
            .fill(Color32::from_rgb(20, 28, 46))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0, 200, 255)))
            .rounding(6.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        RichText::new(format!("📦 {} Packed Trade Corridor Manifests", self.manifests.len()))
                            .color(Color32::from_rgb(0, 220, 255))
                            .strong(),
                    );
                    ui.label(
                        RichText::new("│ Multi-Item Cargo Bundles Packed for Maximum Profit Density")
                            .size(12.0)
                            .color(Color32::from_rgb(140, 170, 200)),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let export_btn = egui::Button::new(
                            RichText::new("📥 Export Manifests to CSV")
                                .color(Color32::from_rgb(0, 220, 255))
                                .strong(),
                        );
                        if ui.add(export_btn).on_hover_text("Export all packed manifests to ./manifests_export.csv and clipboard").clicked() {
                            let csv_str = export_manifests_to_csv(&self.manifests);
                            copy_to_clipboard(ui, csv_str.clone());
                            let _ = std::fs::write("manifests_export.csv", &csv_str);
                            self.set_toast(format!("Exported {} manifests to ./manifests_export.csv & copied to clipboard!", self.manifests.len()));
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
                for chunk in manifest_indices.chunks(2) {
                    if chunk.len() == 2 {
                        let idx0 = chunk[0];
                        let idx1 = chunk[1];
                        let m0 = &self.manifests[idx0];
                        let m1 = &self.manifests[idx1];
                        ui.columns(2, |cols| {
                            if let Some(msg) = self.render_manifest_card(&mut cols[0], idx0, m0, max_vol, max_budget) {
                                manifest_toast = Some(msg);
                            }
                            if let Some(msg) = self.render_manifest_card(&mut cols[1], idx1, m1, max_vol, max_budget) {
                                manifest_toast = Some(msg);
                            }
                        });
                    } else {
                        let idx0 = chunk[0];
                        let m0 = &self.manifests[idx0];
                        ui.columns(2, |cols| {
                            if let Some(msg) = self.render_manifest_card(&mut cols[0], idx0, m0, max_vol, max_budget) {
                                manifest_toast = Some(msg);
                            }
                        });
                    }
                    ui.add_space(8.0);
                }
            } else {
                // Fluid single-column layout for standard/compact displays
                for (idx, m) in self.manifests.iter().enumerate() {
                    if let Some(msg) = self.render_manifest_card(ui, idx, m, max_vol, max_budget) {
                        manifest_toast = Some(msg);
                    }
                    ui.add_space(8.0);
                }
            }
        });

        if let Some(msg) = manifest_toast {
            self.set_toast(msg);
        }
    }

    fn render_route_stop_card(&self, ui: &mut egui::Ui, stop: &RouteStop) -> Option<String> {
        let mut toast = None;
        egui::Frame::none()
            .fill(Color32::from_rgb(17, 24, 38))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(40, 56, 85)))
            .rounding(5.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                // Stop header
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("[STOP #{}]", stop.step))
                            .color(Color32::from_rgb(255, 205, 0))
                            .size(14.0)
                            .strong(),
                    );
                    ui.label(format!("Travel {} jumps to pickup at", stop.jumps_from_prev));
                    ui.label(
                        RichText::new(&stop.deal.source_system_name)
                            .color(Color32::from_rgb(0, 210, 255))
                            .strong(),
                    );
                    if ui.small_button("📋 Copy System").clicked() {
                        copy_to_clipboard(ui, stop.deal.source_system_name.clone());
                        toast = Some(format!("Copied '{}'", stop.deal.source_system_name));
                    }
                });

                ui.add_space(6.0);

                // Actions breakdown
                ui.columns(2, |cols| {
                    // Buy side
                    cols[0].label(RichText::new("1. BUY AT SOURCE:").color(Color32::from_rgb(0, 230, 150)).strong());
                    cols[0].label(RichText::new(format!("{}x {}", stop.deal.quantity, stop.deal.item_name)).strong());
                    cols[0].label(format!("Unit: {:.2} ISK (Total: {:.2}M ISK)", stop.deal.buy_price, stop.deal.capital_required / 1_000_000.0));
                    cols[0].label(RichText::new(&stop.deal.source_station_name).color(Color32::GRAY).size(11.0));
                    if cols[0].small_button("📋 Copy Buy Station").clicked() {
                        copy_to_clipboard(&mut cols[0], stop.deal.source_station_name.clone());
                        toast = Some(format!("Copied buy station '{}'", stop.deal.source_station_name));
                    }

                    // Sell side
                    cols[1].label(RichText::new(format!("2. WARP {} JUMPS & SELL:", stop.deal.jumps_source_dest)).color(Color32::from_rgb(255, 180, 50)).strong());
                    cols[1].label(RichText::new(format!("Destination: {}", stop.deal.dest_system_name)).color(Color32::from_rgb(255, 140, 240)).strong());
                    cols[1].label(
                        RichText::new(format!("Net Profit: +{:.2}M ISK (+{:.1}% ROI)", stop.deal.net_profit / 1_000_000.0, stop.deal.net_profit_pct))
                            .color(Color32::from_rgb(0, 255, 140))
                            .strong(),
                    );
                    cols[1].label(RichText::new(&stop.deal.dest_station_name).color(Color32::GRAY).size(11.0));
                    if cols[1].small_button("📋 Copy Sell Station").clicked() {
                        copy_to_clipboard(&mut cols[1], stop.deal.dest_station_name.clone());
                        toast = Some(format!("Copied sell station '{}'", stop.deal.dest_station_name));
                    }
                });
            });
        toast
    }

    fn render_route_tab(&mut self, ui: &mut egui::Ui) {
        let route = match self.route.clone() {
            Some(r) if !r.stops.is_empty() => r,
            _ => {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    ui.heading("No Multi-Stop Route Planned Yet");
                    ui.label("Run a market scan to automatically plan an optimized multi-hop trade route chaining deals together.");
                });
                return;
            }
        };

        // Summary Card
        egui::Frame::none()
            .fill(Color32::from_rgb(20, 28, 46))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0, 200, 255)))
            .rounding(6.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        RichText::new("🗺️ Planned Multi-Stop Route Summary")
                            .color(Color32::from_rgb(0, 220, 255))
                            .strong(),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let copy_btn = egui::Button::new(
                            RichText::new("📋 Copy Autopilot Trail")
                                .color(Color32::BLACK)
                                .strong(),
                        )
                        .fill(Color32::from_rgb(0, 230, 170));

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
                    ui.label(RichText::new(format!("Total Jumps: {}", route.total_jumps)).strong().color(Color32::from_rgb(255, 205, 0)));
                    ui.separator();
                    ui.label(RichText::new(format!("Total Profit: +{:.2}M ISK", route.total_profit / 1_000_000.0)).strong().color(Color32::from_rgb(0, 255, 140)));
                    ui.separator();
                    ui.label(RichText::new(format!("Overall ROI: +{:.1}%", route_roi)).strong().color(Color32::from_rgb(0, 240, 180)));
                    ui.separator();
                    ui.label(RichText::new(format!("Avg Profit/Jump: {:.2}M ISK", route.avg_profit_per_jump / 1_000_000.0)).strong().color(Color32::from_rgb(255, 215, 0)));
                    ui.separator();
                    ui.label(RichText::new(format!("Capital Needed: {:.2}M ISK", route.total_capital_required / 1_000_000.0)).color(Color32::LIGHT_BLUE));
                    ui.separator();
                    ui.label(RichText::new(format!("Peak Cargo: {:.1} m³", route.max_volume_used)).color(Color32::from_rgb(255, 140, 240)));
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
                            if let Some(msg) = self.render_route_stop_card(&mut cols[0], &chunk[0]) {
                                route_toast = Some(msg);
                            }
                            if let Some(msg) = self.render_route_stop_card(&mut cols[1], &chunk[1]) {
                                route_toast = Some(msg);
                            }
                        });
                    } else {
                        ui.columns(2, |cols| {
                            if let Some(msg) = self.render_route_stop_card(&mut cols[0], &chunk[0]) {
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
                RichText::new(route.waypoint_names.join(" ➔ "))
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
        let tx = self.chain_tx.clone();

        let home_id = sde
            .get_system_id(&self.home_system)
            .unwrap_or(30000142);

        let cfg = crate::chain_route_planner::ChainConfig {
            home_system: home_id,
            max_legs: self.chain_max_legs,
            beam_width: 15,
            detour_enabled: self.chain_detour_enabled,
            max_detour_jumps: self.chain_max_detour,
            ship_max_volume: {
                let ship = self.current_ship();
                ship.max_volume
            },
            ship_max_budget: {
                let ship = self.current_ship();
                ship.max_budget
            },
            safe_only: self.safe_only,
        };

        thread::spawn(move || {
            let results = crate::chain_route_planner::find_chain_routes(&deals, &graph, &cfg);
            let _ = tx.send(ChainSearchResult::Success(results));
        });
    }

    fn render_chain_route_tab(&mut self, ui: &mut egui::Ui) {
        // 1. Controls Header Card
        egui::Frame::none()
            .fill(Color32::from_rgb(20, 28, 46))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0, 200, 255)))
            .rounding(6.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        RichText::new("🔗 Chain Route Plotter — Multi-Leg Hauling Optimizer")
                            .color(Color32::from_rgb(0, 220, 255))
                            .strong(),
                    );
                    ui.label(
                        RichText::new("Knapsack cargo packing & en-route bonus trade chaining")
                            .color(Color32::from_rgb(140, 175, 215))
                            .size(12.0),
                    );
                });

                ui.add_space(8.0);

                ui.horizontal_wrapped(|ui| {
                    ui.scope(|ui| {
                        // Distinct, visible rail styling with cyan border and filled track
                        ui.spacing_mut().slider_rail_height = 6.0;
                        let mut visuals = ui.visuals_mut().clone();
                        visuals.widgets.inactive.bg_fill = Color32::from_rgb(26, 42, 68);
                        visuals.widgets.inactive.bg_stroke = Stroke::new(1.5_f32, Color32::from_rgb(0, 180, 240));
                        visuals.widgets.hovered.bg_fill = Color32::from_rgb(36, 58, 92);
                        visuals.widgets.hovered.bg_stroke = Stroke::new(2.0_f32, Color32::from_rgb(0, 220, 255));
                        visuals.widgets.active.bg_fill = Color32::from_rgb(46, 74, 116);
                        visuals.widgets.active.bg_stroke = Stroke::new(2.0_f32, Color32::from_rgb(0, 255, 200));
                        visuals.selection.bg_fill = Color32::from_rgb(0, 170, 230);
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
                        RichText::new("✓ Finite Station Stock Enforced")
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
                                RichText::new("🔗 FIND CHAIN ROUTES")
                                    .color(Color32::BLACK)
                                    .strong(),
                            )
                            .fill(if can_search {
                                Color32::from_rgb(0, 220, 255)
                            } else {
                                Color32::from_rgb(60, 75, 95)
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
                ui.heading("No Market Deals Loaded");
                ui.label("Run a market scan first by clicking 'SCAN MARKET NOW' so the optimizer has trade deals to chain.");
            });
            return;
        }

        // 3. Results Section
        if self.chain_results.is_empty() && !self.chain_is_running {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.heading("No Chain Routes Generated Yet");
                ui.label("Configure your max legs and detour tolerance above, then click 'FIND CHAIN ROUTES'.");
            });
            return;
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            // Discovered Chains Selector Strip
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("Top Ranked Chains ({} found):", self.chain_results.len()))
                        .strong()
                        .color(Color32::from_rgb(200, 230, 255)),
                );
            });

            ui.add_space(6.0);

            // Horizontal card strip for selecting routes
            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (idx, route) in self.chain_results.iter().enumerate() {
                        let is_selected = self.chain_selected_idx == Some(idx);
                        let bg_color = if is_selected {
                            Color32::from_rgb(28, 42, 68)
                        } else {
                            Color32::from_rgb(16, 22, 36)
                        };
                        let border_color = if is_selected {
                            Color32::from_rgb(0, 220, 255)
                        } else {
                            Color32::from_rgb(38, 52, 78)
                        };

                        let frame = egui::Frame::none()
                            .fill(bg_color)
                            .stroke(Stroke::new(if is_selected { 1.5_f32 } else { 1.0_f32 }, border_color))
                            .rounding(6.0)
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0));

                        frame.show(ui, |ui| {
                            let resp = ui.vertical(|ui| {
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
                                    ui.label(RichText::new(format!("{} legs", route.legs.len())).color(Color32::LIGHT_GRAY).size(11.0));
                                });
                            });

                            if resp.response.interact(egui::Sense::click()).clicked() {
                                self.chain_selected_idx = Some(idx);
                            }
                        });
                    }
                });
            });

            ui.add_space(14.0);

            // Detailed Breakdown of Selected Route
            if let Some(selected_idx) = self.chain_selected_idx {
                if let Some(route) = self.chain_results.get(selected_idx).cloned() {
                    let can_split = ui.available_width() >= 1150.0;
                    // Top header strip for selected route with Split / Stacked view switcher
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("🔗 Chain Route #{}: Visual Stops Flowchart & Route Dossier", selected_idx + 1))
                                .size(16.0)
                                .color(Color32::from_rgb(0, 220, 255))
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
                                    .color(Color32::from_rgb(0, 220, 255))
                                    .strong(),
                            )
                            .fill(Color32::from_rgb(20, 32, 54));

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
    fn render_chain_flowchart(&mut self, ui: &mut egui::Ui, route: &crate::chain_route_planner::ChainRoute) {
        // Flowchart Header Frame
        egui::Frame::none()
            .fill(Color32::from_rgb(18, 25, 42))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0, 200, 255)))
            .rounding(6.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("🗺️ Visual Stops Flowchart")
                            .size(14.0)
                            .color(Color32::from_rgb(0, 220, 255))
                            .strong(),
                    );
                    ui.label(
                        RichText::new("Flight nodes with in-game quick-copy buttons")
                            .size(11.0)
                            .color(Color32::from_rgb(140, 175, 215)),
                    );
                });
            });

        ui.add_space(8.0);

        let mut stop_idx = 1;

        // ── 1. Start / Departure Node ──
        let home_id = self.sde.as_ref().and_then(|s| s.get_system_id(&self.home_system));
        let home_sec = home_id
            .and_then(|id| self.sde.as_ref().and_then(|s| s.get_system_security(id)))
            .unwrap_or(1.0);
        let home_sec_col = security_color(home_sec);

        egui::Frame::none()
            .fill(Color32::from_rgb(14, 20, 36))
            .stroke(Stroke::new(1.2_f32, Color32::from_rgb(0, 180, 255)))
            .rounding(6.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("🚀 [START]")
                            .size(13.0)
                            .color(Color32::from_rgb(0, 220, 255))
                            .strong(),
                    );
                    ui.label(RichText::new("DEPARTURE POINT").size(13.0).strong().color(Color32::WHITE));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let home_copy = self.home_system.clone();
                        let sys_btn = egui::Button::new(
                            RichText::new("📋 System").size(11.0).color(Color32::BLACK).strong(),
                        )
                        .fill(Color32::from_rgb(0, 200, 255));
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
                            .color(Color32::from_rgb(200, 230, 255))
                            .strong(),
                    );
                    ui.label(RichText::new("• Starting system location").size(11.0).color(Color32::GRAY));
                });
            });

        // ── Helper closure to draw flowchart connector ──
        let render_connector = |ui: &mut egui::Ui, jumps: u32| {
            ui.vertical_centered(|ui| {
                ui.add_space(3.0);
                ui.label(RichText::new("│").size(13.0).color(Color32::from_rgb(0, 180, 255)));
                let jump_text = if jumps == 0 {
                    "⚡ Intra-System Warp (0 jumps)".to_string()
                } else {
                    format!("⚡ {} Jump(s) • Safe Highsec", jumps)
                };
                egui::Frame::none()
                    .fill(Color32::from_rgb(24, 32, 50))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(255, 205, 0)))
                    .rounding(10.0)
                    .inner_margin(egui::Margin::symmetric(10.0, 3.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(jump_text)
                                .size(11.0)
                                .color(Color32::from_rgb(255, 215, 0))
                                .strong(),
                        );
                    });
                ui.label(RichText::new("▼").size(13.0).color(Color32::from_rgb(0, 180, 255)));
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
        render_connector(ui, first_jumps);

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
                    .or_else(|| self.sde.as_ref().and_then(|s| s.get_system_security(pickup.system_id)))
                    .unwrap_or(1.0);
                let sec_col = security_color(sec);

                let primary_station = pickup
                    .cargo_loaded
                    .first()
                    .map(|s| s.deal.source_station_name.clone())
                    .unwrap_or_default();

                egui::Frame::none()
                    .fill(Color32::from_rgb(16, 24, 38))
                    .stroke(Stroke::new(1.3_f32, stroke_col))
                    .rounding(6.0)
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        // Header line
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("[STOP #{}]", stop_idx))
                                    .size(13.0)
                                    .color(Color32::from_rgb(255, 205, 0))
                                    .strong(),
                            );
                            let badge_text = if is_detour {
                                format!("🛒 BUY STOP (En-Route +{}j)", pickup.detour_jumps)
                            } else {
                                "🛒 BUY STOP".to_string()
                            };
                            ui.label(RichText::new(badge_text).size(13.0).color(stroke_col).strong());

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if !primary_station.is_empty() {
                                    let st_copy = primary_station.clone();
                                    let st_btn = egui::Button::new(
                                        RichText::new("📋 Station").size(11.0).color(Color32::BLACK).strong(),
                                    )
                                    .fill(Color32::from_rgb(0, 230, 170));
                                    if ui.add(st_btn).clicked() {
                                        copy_to_clipboard(ui, st_copy.clone());
                                        self.set_toast(format!("Copied station '{}'", st_copy));
                                    }
                                }
                                let sys_copy = pickup.system_name.clone();
                                let sys_btn = egui::Button::new(
                                    RichText::new("📋 System").size(11.0).color(Color32::BLACK).strong(),
                                )
                                .fill(Color32::from_rgb(0, 200, 255));
                                if ui.add(sys_btn).clicked() {
                                    copy_to_clipboard(ui, sys_copy.clone());
                                    self.set_toast(format!("Copied system '{}'", sys_copy));
                                }
                            });
                        });

                        ui.add_space(3.0);

                        // Location info
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(format!("[{:.1}]", sec)).size(12.0).color(sec_col).strong());
                            ui.label(
                                RichText::new(&pickup.system_name)
                                    .size(13.0)
                                    .color(Color32::from_rgb(0, 220, 255))
                                    .strong(),
                            );
                            if !primary_station.is_empty() {
                                ui.label(
                                    RichText::new(format!("• {}", primary_station))
                                        .size(11.0)
                                        .color(Color32::from_rgb(180, 200, 225)),
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
                                    .fill(Color32::from_rgb(0, 100, 55))
                                    .rounding(3.0)
                                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new("BUY")
                                                .size(11.0)
                                                .color(Color32::from_rgb(0, 255, 140))
                                                .strong(),
                                        );
                                    });

                                ui.label(
                                    RichText::new(format!("{}x {}", slot.deal.quantity, slot.deal.item_name))
                                        .size(12.5)
                                        .color(Color32::WHITE)
                                        .strong(),
                                );

                                let item_copy = slot.deal.item_name.clone();
                                let item_btn = egui::Button::new(
                                    RichText::new("📋 Item").size(11.0).color(Color32::BLACK).strong(),
                                )
                                .fill(Color32::from_rgb(0, 220, 255));
                                if ui.add(item_btn).clicked() {
                                    copy_to_clipboard(ui, item_copy.clone());
                                    self.set_toast(format!("Copied item '{}'", item_copy));
                                }
                            });

                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(format!("Cost: {:.2}M ISK", slot.capital / 1_000_000.0))
                                        .size(11.0)
                                        .color(Color32::LIGHT_BLUE),
                                );
                                ui.label(
                                    RichText::new(format!("• Vol: {:.1} m³", slot.volume))
                                        .size(11.0)
                                        .color(Color32::from_rgb(255, 140, 240)),
                                );
                                ui.label(
                                    RichText::new(format!("• ➔ Deliver to {}", slot.sell_name))
                                        .size(11.0)
                                        .color(Color32::from_rgb(0, 220, 255)),
                                );
                                if slot.deal.source_order_volume_remain > 0 {
                                    ui.label(
                                        RichText::new(format!("• Stock: {}/{} avail", slot.deal.quantity, slot.deal.source_order_volume_remain))
                                            .size(11.0)
                                            .color(Color32::from_rgb(180, 210, 245)),
                                    );
                                }
                                ui.label(
                                    RichText::new(format!("(+{:.2}M ISK net)", slot.net_profit / 1_000_000.0))
                                        .size(11.0)
                                        .color(Color32::from_rgb(0, 255, 140)),
                                );
                            });

                            // If item has a specific station different from primary
                            if !slot.deal.source_station_name.is_empty()
                                && slot.deal.source_station_name != primary_station
                            {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("Station: {}", slot.deal.source_station_name))
                                            .size(10.5)
                                            .color(Color32::GRAY),
                                    );
                                    let st_copy = slot.deal.source_station_name.clone();
                                    if ui.small_button("📋 Station").clicked() {
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
                render_connector(ui, next_jumps);
            }

            // Sell Stop
            let sec = leg
                .sell
                .cargo_sold
                .first()
                .map(|s| s.deal.dest_security)
                .or_else(|| self.sde.as_ref().and_then(|s| s.get_system_security(leg.sell.system_id)))
                .unwrap_or(1.0);
            let sec_col = security_color(sec);

            let sell_station = leg
                .sell
                .cargo_sold
                .first()
                .map(|s| s.deal.dest_station_name.clone())
                .unwrap_or_default();

            egui::Frame::none()
                .fill(Color32::from_rgb(16, 24, 38))
                .stroke(Stroke::new(1.3_f32, Color32::from_rgb(0, 190, 240)))
                .rounding(6.0)
                .inner_margin(10.0)
                .show(ui, |ui| {
                    // Header line
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("[STOP #{}]", stop_idx))
                                .size(13.0)
                                .color(Color32::from_rgb(255, 205, 0))
                                .strong(),
                        );
                        ui.label(
                            RichText::new("💰 SELL STOP")
                                .size(13.0)
                                .color(Color32::from_rgb(0, 220, 255))
                                .strong(),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !sell_station.is_empty() {
                                let st_copy = sell_station.clone();
                                let st_btn = egui::Button::new(
                                    RichText::new("📋 Station").size(11.0).color(Color32::BLACK).strong(),
                                )
                                .fill(Color32::from_rgb(0, 230, 170));
                                if ui.add(st_btn).clicked() {
                                    copy_to_clipboard(ui, st_copy.clone());
                                    self.set_toast(format!("Copied station '{}'", st_copy));
                                }
                            }
                            let sys_copy = leg.sell.system_name.clone();
                            let sys_btn = egui::Button::new(
                                RichText::new("📋 System").size(11.0).color(Color32::BLACK).strong(),
                            )
                            .fill(Color32::from_rgb(0, 200, 255));
                            if ui.add(sys_btn).clicked() {
                                copy_to_clipboard(ui, sys_copy.clone());
                                self.set_toast(format!("Copied system '{}'", sys_copy));
                            }
                        });
                    });

                    ui.add_space(3.0);

                    // Location info
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("[{:.1}]", sec)).size(12.0).color(sec_col).strong());
                        ui.label(
                            RichText::new(&leg.sell.system_name)
                                .size(13.0)
                                .color(Color32::from_rgb(0, 255, 140))
                                .strong(),
                        );
                        if !sell_station.is_empty() {
                            ui.label(
                                RichText::new(format!("• {}", sell_station))
                                    .size(11.0)
                                    .color(Color32::from_rgb(180, 200, 225)),
                            );
                        }
                    });

                    ui.add_space(5.0);
                    ui.separator();
                    ui.add_space(3.0);

                    // Cargo items SOLD
                    for slot in &leg.sell.cargo_sold {
                        ui.horizontal(|ui| {
                            // Cyan SELL Pill
                            egui::Frame::none()
                                .fill(Color32::from_rgb(12, 60, 105))
                                .rounding(3.0)
                                .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new("SELL")
                                            .size(11.0)
                                            .color(Color32::from_rgb(0, 220, 255))
                                            .strong(),
                                    );
                                });

                            ui.label(
                                RichText::new(format!("{}x {}", slot.deal.quantity, slot.deal.item_name))
                                    .size(12.5)
                                    .color(Color32::WHITE)
                                    .strong(),
                            );

                            let item_copy = slot.deal.item_name.clone();
                            let item_btn = egui::Button::new(
                                RichText::new("📋 Item").size(11.0).color(Color32::BLACK).strong(),
                            )
                            .fill(Color32::from_rgb(0, 220, 255));
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
                                .color(Color32::from_rgb(0, 255, 140))
                                .strong(),
                            );
                            ui.label(
                                RichText::new(format!("• Freed: {:.1} m³", slot.volume))
                                    .size(11.0)
                                    .color(Color32::GRAY),
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
                render_connector(ui, next_leg_jumps);
            }
        }

        // ── 3. Route Finish Node ──
        ui.vertical_centered(|ui| {
            ui.add_space(3.0);
            ui.label(RichText::new("│").size(13.0).color(Color32::from_rgb(0, 255, 140)));
            ui.label(RichText::new("▼").size(13.0).color(Color32::from_rgb(0, 255, 140)));
            ui.add_space(3.0);
        });

        egui::Frame::none()
            .fill(Color32::from_rgb(18, 28, 46))
            .stroke(Stroke::new(1.2_f32, Color32::from_rgb(0, 255, 150)))
            .rounding(6.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("🏁 [FINISH]")
                            .size(13.0)
                            .color(Color32::from_rgb(0, 255, 140))
                            .strong(),
                    );
                    ui.label(RichText::new("CHAIN ROUTE COMPLETED").size(13.0).color(Color32::WHITE).strong());
                });
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("Total Profit: +{:.2}M ISK", route.total_profit / 1_000_000.0))
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
        egui::Frame::none()
            .fill(Color32::from_rgb(18, 25, 42))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0, 200, 255)))
            .rounding(6.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        RichText::new(format!("🔗 Chain Route #{}: Complete Visual Flight Dossier", selected_idx + 1))
                            .color(Color32::from_rgb(0, 220, 255))
                            .strong(),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let copy_btn = egui::Button::new(
                            RichText::new("📋 Copy Autopilot Route")
                                .color(Color32::BLACK)
                                .strong(),
                        )
                        .fill(Color32::from_rgb(0, 230, 170));

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
                    ui.label(RichText::new(format!("Total Jumps: {}", route.total_jumps)).strong().color(Color32::from_rgb(255, 205, 0)));
                    ui.separator();
                    ui.label(RichText::new(format!("Total Net Profit: +{:.2}M ISK", route.total_profit / 1_000_000.0)).strong().color(Color32::from_rgb(0, 255, 140)));
                    ui.separator();
                    ui.label(RichText::new(format!("Overall Net ROI: +{:.1}%", route.overall_roi_pct)).strong().color(Color32::from_rgb(0, 240, 180)));
                    ui.separator();
                    ui.label(RichText::new(format!("Profit / Jump: {:.2}M ISK", route.profit_per_jump / 1_000_000.0)).strong().color(Color32::from_rgb(255, 215, 0)));
                    ui.separator();
                    ui.label(RichText::new(format!("Capital Invested: {:.2}M ISK", route.total_capital / 1_000_000.0)).color(Color32::LIGHT_BLUE));
                    ui.separator();
                    ui.label(RichText::new(format!("Peak Cargo Hold: {:.1} m³", route.peak_volume_m3)).color(Color32::from_rgb(255, 140, 240)));
                });
            });

        ui.add_space(10.0);
        ui.label(RichText::new("Step-by-Step Leg Timeline & Knapsack Cargo Tracking:").strong().size(14.0));
        ui.add_space(6.0);

        // Render Each Leg
        for (leg_idx, leg) in route.legs.iter().enumerate() {
            egui::Frame::none()
                .fill(Color32::from_rgb(16, 22, 36))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 54, 82)))
                .rounding(6.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("LEG #{}", leg_idx + 1))
                                .size(15.0)
                                .strong()
                                .color(Color32::from_rgb(0, 220, 255)),
                        );
                        ui.separator();
                        ui.label(RichText::new(format!("{} jumps", leg.leg_total_jumps)).color(Color32::from_rgb(255, 205, 0)));
                        ui.separator();
                        ui.label(
                            RichText::new(format!("Leg Profit: +{:.2}M ISK", leg.leg_total_profit / 1_000_000.0))
                                .strong()
                                .color(Color32::from_rgb(0, 255, 140)),
                        );
                    });

                    ui.add_space(8.0);

                    // Pickup Stops in this leg
                    for pickup in &leg.pickups {
                        let is_detour = pickup.detour_jumps > 0;
                        let badge_col = if is_detour {
                            Color32::from_rgb(241, 196, 15) // Gold
                        } else {
                            Color32::from_rgb(0, 200, 255) // Cyan
                        };

                        let header_text = if is_detour {
                            format!("🛒 EN-ROUTE PICKUP — {} (+{} detour jumps)", pickup.system_name, pickup.detour_jumps)
                        } else {
                            format!("🛒 PICKUP STOP — {}", pickup.system_name)
                        };

                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(header_text).strong().color(badge_col));
                                if pickup.jumps_from_prev > 0 {
                                    ui.label(RichText::new(format!("({} jumps from prev)", pickup.jumps_from_prev)).color(Color32::GRAY));
                                }
                            });

                            ui.add_space(4.0);

                            // Item Slots
                            for slot in &pickup.cargo_loaded {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("+ {}x {}", slot.deal.quantity, slot.deal.item_name)).strong().color(Color32::WHITE));
                                    ui.label(RichText::new(format!("{:.1} m³", slot.volume)).color(Color32::from_rgb(255, 140, 240)));
                                    ui.label(RichText::new(format!("{:.2}M ISK", slot.capital / 1_000_000.0)).color(Color32::LIGHT_BLUE));
                                    ui.label(RichText::new(format!("➔ Sell at {}", slot.sell_name)).color(Color32::from_rgb(0, 220, 255)));
                                    ui.label(RichText::new(format!("(+{:.2}M ISK net)", slot.net_profit / 1_000_000.0)).color(Color32::from_rgb(0, 255, 140)));
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
                                .color(Color32::from_rgb(150, 180, 210)),
                            );
                        });

                        ui.add_space(4.0);
                    }

                    // Flight Arrow
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("▼ Fly {} jump(s) to destination", leg.sell.jumps_from_prev)).color(Color32::from_rgb(255, 205, 0)).strong());
                    });

                    ui.add_space(4.0);

                    // Sell Stop
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("💰 SELL DESTINATION — {}", leg.sell.system_name))
                                    .strong()
                                    .color(Color32::from_rgb(0, 255, 140)),
                            );
                        });

                        ui.add_space(4.0);

                        for slot in &leg.sell.cargo_sold {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("✔ Sold {}x {}", slot.deal.quantity, slot.deal.item_name)).color(Color32::WHITE));
                                ui.label(RichText::new(format!("+{:.2}M ISK net profit", slot.net_profit / 1_000_000.0)).color(Color32::from_rgb(0, 255, 140)).strong());
                                ui.label(RichText::new(format!("(freed {:.1} m³)", slot.volume)).color(Color32::GRAY));
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
            RichText::new(route.waypoint_names.join(" ➔ "))
                .size(13.0)
                .color(Color32::from_rgb(180, 215, 255)),
        );
    }

    fn render_settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Configuration & Tax Parameters");
        ui.add_space(10.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            // Card 0: Display, UI Scaling & Layout
            egui::Frame::none()
                .fill(Color32::from_rgb(18, 25, 40))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 45, 72)))
                .rounding(6.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.label(RichText::new("🖥️ Display, UI Scaling & Layout").size(15.0).color(Color32::from_rgb(0, 210, 255)).strong());
                    ui.label("Configure desktop scaling factors, responsive docking, and fullscreen display modes:");
                    ui.add_space(8.0);

                    ui.horizontal_wrapped(|ui| {
                        ui.label("UI Scale / Zoom Factor:");
                        let mut pct = (self.ui_scale * 100.0).round() as i32;
                        if ui.add(egui::Slider::new(&mut pct, 75..=150).suffix("%").step_by(5.0)).changed() {
                            self.ui_scale = (pct as f32 / 100.0).clamp(0.75, 1.50);
                            ui.ctx().set_zoom_factor(self.ui_scale);
                        }
                        if ui.button("↺ Reset 100%").clicked() {
                            self.ui_scale = 1.0;
                            ui.ctx().set_zoom_factor(1.0);
                            self.set_toast("UI Scale reset to 100%");
                        }
                        ui.label(RichText::new("(Shortcuts: Ctrl + / Ctrl - / Ctrl 0)").color(Color32::GRAY).size(11.0));
                    });

                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Window Mode:");
                        let fs_label = if self.is_fullscreen { "🗗 Exit Fullscreen (F11)" } else { "⛶ Enter Fullscreen (F11)" };
                        if ui.button(fs_label).clicked() {
                            self.is_fullscreen = !self.is_fullscreen;
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.is_fullscreen));
                            if self.is_fullscreen {
                                self.set_toast("Entered Fullscreen mode (Press F11 to exit)");
                            } else {
                                self.set_toast("Exited Fullscreen mode");
                            }
                        }

                        ui.separator();

                        ui.label("Deal Dossier Docking:");
                        if ui.radio_value(&mut self.dossier_side_panel, true, "◫ Side Panel (widescreen)").clicked() {
                            self.set_toast("Dossier docking set to Side Panel");
                        }
                        if ui.radio_value(&mut self.dossier_side_panel, false, "▬ Bottom Drawer").clicked() {
                            self.set_toast("Dossier docking set to Bottom Drawer");
                        }

                        ui.separator();

                        ui.label("Executive KPI Bar:");
                        if ui.radio_value(&mut self.compact_kpi, false, "Full Cards").clicked() {
                            self.set_toast("KPI bar set to Full Cards");
                        }
                        if ui.radio_value(&mut self.compact_kpi, true, "Compact Pills").clicked() {
                            self.set_toast("KPI bar set to Compact Pills");
                        }
                    });
                });

            ui.add_space(12.0);

            // Card 1: Trading Taxes & Character Skills
            egui::Frame::none()
                .fill(Color32::from_rgb(18, 25, 40))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 45, 72)))
                .rounding(6.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.label(RichText::new("💰 Market Taxes & Character Skills").size(15.0).color(Color32::from_rgb(0, 210, 255)).strong());
                    ui.label("Customize fee percentages according to your pilot's Accounting and Broker Relations skills:");
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.label("Broker Fee % (on buy order):");
                        ui.add(egui::DragValue::new(&mut self.broker_fee_pct).speed(0.1).range(1.0..=5.0).suffix("%"));
                        ui.label(RichText::new("(Default 3.0%, reduced by Broker Relations skill)").color(Color32::GRAY));
                    });

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Sales Tax % (on sell order):");
                        ui.add(egui::DragValue::new(&mut self.sales_tax_pct).speed(0.1).range(3.375..=8.0).suffix("%"));
                        ui.label(RichText::new("(Default 8.0%, reduced by Accounting skill to 3.375%)").color(Color32::GRAY));
                    });

                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Skill Presets:").strong());
                        if ui.small_button("Max Skills (Accounting V / Broker V)").on_hover_text("Accounting V: 3.375% sales tax\nBroker Relations V: 1.0% broker fee").clicked() {
                            self.sales_tax_pct = 3.375;
                            self.broker_fee_pct = 1.0;
                            self.set_toast("Applied Max Skills: 3.375% sales tax, 1.0% broker fee");
                        }
                        if ui.small_button("Level IV Skills").on_hover_text("Accounting IV: 4.3% sales tax\nBroker Relations IV: 1.4% broker fee").clicked() {
                            self.sales_tax_pct = 4.3;
                            self.broker_fee_pct = 1.4;
                            self.set_toast("Applied Level IV Skills: 4.3% sales tax, 1.4% broker fee");
                        }
                        if ui.small_button("Base / Untrained").on_hover_text("Accounting 0: 8.0% sales tax\nBroker Relations 0: 3.0% broker fee").clicked() {
                            self.sales_tax_pct = 8.0;
                            self.broker_fee_pct = 3.0;
                            self.set_toast("Applied Untrained: 8.0% sales tax, 3.0% broker fee");
                        }
                    });
                });

            ui.add_space(12.0);

            // Card: Vessel Cargo Capacity & Anti-Gank Collateral Budget
            egui::Frame::none()
                .fill(Color32::from_rgb(18, 25, 40))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 45, 72)))
                .rounding(6.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.label(RichText::new("🛡️ Vessel Cargo Capacity & Anti-Gank Collateral Budget").size(15.0).color(Color32::from_rgb(0, 210, 255)).strong());
                    ui.label("Configure cargo hold volume and ISK collateral caps to match your active ship hull and prevent suicide ganking:");
                    ui.add_space(8.0);

                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Profile Mode:").strong());
                        if ui.radio_value(&mut self.use_custom_ship, false, "Standard Hauler Presets").clicked() {
                            self.set_toast(if self.ship_is_big { "Active: Big Hauler (60k m³ · 500M)" } else { "Active: Small Hauler (30k m³ · 50M)" });
                        }
                        if ui.radio_value(&mut self.use_custom_ship, true, "Custom Ship & Anti-Gank Sizing").clicked() {
                            self.set_toast("Active: Custom Vessel & Anti-Gank Protection");
                        }
                    });

                    if !self.use_custom_ship {
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Active Preset:");
                            if ui.selectable_label(self.ship_is_big, "Big Hauler (Deep Space Transport: 60,000 m³ · 500M ISK)").clicked() {
                                self.ship_is_big = true;
                                self.set_toast("Selected Big Hauler preset (60k m³ · 500M ISK)");
                            }
                            if ui.selectable_label(!self.ship_is_big, "Small Hauler (T1 Industrial: 30,000 m³ · 50M ISK)").clicked() {
                                self.ship_is_big = false;
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
                                self.set_toast("Loaded T1 Hauler preset (30k m³ · 50M ISK cap)");
                            }
                            if ui.small_button("Deep Space Transport (60k m³ · 500M ISK)").on_hover_text("Occator, Bustard, Mastodon, Impel (Overheated tank)\nAnti-Gank Cap: 500M ISK").clicked() {
                                self.custom_cargo_vol = 60_000.0;
                                self.custom_budget_isk = 500_000_000.0;
                                self.set_toast("Loaded DST preset (60k m³ · 500M ISK cap)");
                            }
                            if ui.small_button("Blockade Runner (12k m³ · 300M ISK)").on_hover_text("Prowler, Prorator, Crane, Viator (Cloaky / Immune to Cargo Scanners)\nAnti-Gank Cap: 300M ISK").clicked() {
                                self.custom_cargo_vol = 12_000.0;
                                self.custom_budget_isk = 300_000_000.0;
                                self.set_toast("Loaded Blockade Runner preset (12k m³ · 300M ISK cap)");
                            }
                            if ui.small_button("Freighter (900k m³ · 2.0B ISK)").on_hover_text("Charon, Obelisk, Fenrir, Providence\nAnti-Gank Cap: 2.0B ISK (Tornado gank threshold)").clicked() {
                                self.custom_cargo_vol = 900_000.0;
                                self.custom_budget_isk = 2_000_000_000.0;
                                self.set_toast("Loaded Freighter preset (900k m³ · 2.0B ISK cap)");
                            }
                        });

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label("Cargo Hold Capacity (m³):");
                            ui.add(egui::DragValue::new(&mut self.custom_cargo_vol).speed(250.0).range(100.0..=1_500_000.0).suffix(" m³"));
                            ui.label(RichText::new(format!("({:.1}k m³)", self.custom_cargo_vol / 1000.0)).color(Color32::from_rgb(0, 220, 255)));
                        });

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label("Anti-Gank Collateral Budget (ISK):");
                            ui.add(egui::DragValue::new(&mut self.custom_budget_isk).speed(5_000_000.0).range(1_000_000.0..=100_000_000_000.0));
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
                                "💡 Anti-Gank Rule of Thumb: In Highsec (0.5 - 1.0), carrying more than 100M ISK in a T1 industrial or 1B - 2B ISK in a standard freighter makes you a prime target for suicide gankers (Catalysts / Tornadoes). Capping your trip budget prevents dangerous cargo exposure."
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
                    ui.label(RichText::new("🌐 Active Scan Regions").size(15.0).color(Color32::from_rgb(0, 210, 255)).strong());
                    ui.label("Select major trade hub regions to include in the arbitrage scanner:");
                    ui.add_space(8.0);

                    ui.checkbox(&mut self.region_the_forge, "The Forge (Jita IV-4) — Region 10000002");
                    ui.checkbox(&mut self.region_domain, "Domain (Amarr VIII) — Region 10000043");
                    ui.checkbox(&mut self.region_sinq_laison, "Sinq Laison (Dodixie IX-20) — Region 10000032");
                    ui.checkbox(&mut self.region_heimatar, "Heimatar (Rens VI-8) — Region 10000030");
                    ui.checkbox(&mut self.region_metropolis, "Metropolis (Hek VIII-12) — Region 10000042");
                });

            ui.add_space(12.0);

            // Card 3: Auto-Refresh Settings
            egui::Frame::none()
                .fill(Color32::from_rgb(18, 25, 40))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 45, 72)))
                .rounding(6.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.label(RichText::new("🔄 Automated Market Refresh").size(15.0).color(Color32::from_rgb(0, 210, 255)).strong());
                    ui.label("Automatically fetch fresh regional market data every 5 minutes (matching CCP ESI cache expiration):");
                    ui.add_space(8.0);

                    if ui.checkbox(&mut self.auto_refresh, "Enable 5-Minute Market Auto-Refresh").changed() {
                        if self.auto_refresh {
                            self.last_scan_completed = Some(Instant::now());
                            self.set_toast("Auto-Refresh enabled (5-minute interval)");
                        } else {
                            self.set_toast("Auto-Refresh disabled");
                        }
                    }
                    ui.add_space(4.0);
                    ui.label(RichText::new("💡 When enabled, a live countdown badge is displayed in the executive header next to the scan button.").color(Color32::GRAY));

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.label(RichText::new("🔔 High-Profit Sound Notification:").size(13.0).color(Color32::from_rgb(0, 210, 255)).strong());
                    ui.label("Plays an audible alert chime whenever a trade exceeding the ROI threshold is discovered during auto-refresh:");
                    ui.add_space(6.0);

                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut self.sound_alerts_enabled, "Play audio chime on high-profit trades").changed() {
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
                        ui.add(egui::DragValue::new(&mut self.sound_min_roi_pct).speed(1.0).range(10.0..=1000.0).suffix("%"));
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
            egui::Frame::none()
                .fill(Color32::from_rgb(18, 25, 40))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 45, 72)))
                .rounding(6.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.label(RichText::new("🏛️ Station & Structure Access Pool").size(15.0).color(Color32::from_rgb(0, 210, 255)).strong());
                    ui.label("Configure whether arbitrage deals should strictly target NPC stations or allow player-owned Citadels:");
                    ui.add_space(8.0);

                    if ui.radio_value(&mut self.npc_stations_only, true, "🏛️ Preferred: Only NPC Stations (Guaranteed public docking, no access lockouts)").changed() {
                        self.set_toast("Station Pool: Set to NPC Stations Only");
                    }
                    ui.add_space(4.0);
                    if ui.radio_value(&mut self.npc_stations_only, false, "🏰 Add Player Citadels / Upwell Structures to Pool (May require docking access ACL)").changed() {
                        self.set_toast("Station Pool: Set to Include Player Citadels");
                    }
                });

            ui.add_space(12.0);

            // Card 4: Enforced Invariants
            egui::Frame::none()
                .fill(Color32::from_rgb(18, 25, 40))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(32, 45, 72)))
                .rounding(6.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.label(RichText::new("🛡️ Domain Rules & Invariants").size(15.0).color(Color32::from_rgb(0, 210, 255)).strong());
                    ui.add_space(6.0);
                    ui.label("• Strict 10.0% Net Profitability: Deals producing under 10% net return after all taxes are rejected.");
                    ui.label("• Big Hauler Constraints: 60,000 m³ volume capacity / 500,000,000 ISK max budget.");
                    ui.label("• Small Hauler Constraints: 30,000 m³ volume capacity / 50,000,000 ISK max budget.");
                    ui.label("• True Profit-Per-Jump: Jumps required to fly to the pickup station are counted in total flight distance.");
                    ui.label("• Safe Stargate Navigation: Routes avoid lowsec (<0.45) and nullsec camps by default.");
                    ui.label("• Highsec Space Only: All trade routes and station orders are restricted to high security space.");
                });
        });
    }

    fn render_splash_screen(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(7, 9, 24))
                    .inner_margin(24.0),
            )
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
                    for (idx, r_factor) in [0.35_f32, 0.65_f32, 1.0_f32, 1.35_f32].iter().enumerate() {
                        let r = base_radius * r_factor;
                        let alpha = if idx == 2 { 60 } else { 25 };
                        painter.circle_stroke(
                            radar_center,
                            r,
                            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0, 229, 255, alpha)),
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

                    let (rect, _) = ui.allocate_exact_size(Vec2::new(bar_width, bar_height), egui::Sense::hover());
                    let bar_painter = ui.painter();

                    bar_painter.rect_filled(rect, 4.0, Color32::from_rgb(12, 17, 30));
                    bar_painter.rect_stroke(rect, 4.0, Stroke::new(1.0_f32, Color32::from_rgb(30, 48, 78)));

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
                        egui::Frame::none()
                            .fill(Color32::from_rgba_unmultiplied(48, 12, 16, 230))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(231, 76, 60)))
                            .rounding(5.0)
                            .inner_margin(10.0)
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("✘ Startup Warning: {}", err))
                                        .size(12.0)
                                        .color(Color32::from_rgb(255, 120, 120))
                                        .strong(),
                                );
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    if ui
                                        .button(RichText::new("⚡ Enter Offline Mode").color(Color32::from_rgb(0, 229, 255)))
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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("EVE Traveling Merchant — Arbitrage & Multi-Stop Route Planner")
            .with_inner_size([1280.0, 768.0])
            .with_min_inner_size([800.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "EVE Traveling Merchant",
        options,
        Box::new(|_cc| Ok(Box::new(app))),
    )
}
