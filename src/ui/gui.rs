use crate::arbitrage::{find_deals, ArbitrageConfig, ArbitrageDeal};
use crate::config::{BIG_HAULER, SMALL_HAULER};
use crate::esi::{fetch_markets, EsiClient};
use crate::graph::SystemGraph;
use crate::route_planner::{plan_route, TradeRoute};
use crate::sde::SdeLoader;
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

#[derive(PartialEq, Clone, Copy)]
enum ActiveTab {
    Deals,
    Route,
    Settings,
}

#[derive(PartialEq, Clone, Copy)]
enum DealSortBy {
    ProfitPerJump,
    TotalProfit,
    RoiPct,
    Jumps,
    Cost,
}

enum ScanResult {
    Success {
        deals: Vec<ArbitrageDeal>,
        route: TradeRoute,
        total_sells: usize,
        total_buys: usize,
    },
    Error(String),
}

pub struct DesktopApp {
    sde: Arc<SdeLoader>,
    graph: Arc<SystemGraph>,
    esi: Arc<EsiClient>,

    // Controls
    ship_is_big: bool,
    home_system: String,
    safe_only: bool,
    npc_stations_only: bool,
    min_roi_pct: f64,
    min_profit_isk: f64,
    broker_fee_pct: f64,
    sales_tax_pct: f64,
    max_route_stops: usize,

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

    // Active state
    tab: ActiveTab,
    is_scanning: bool,
    status_text: String,
    deals: Vec<ArbitrageDeal>,
    route: Option<TradeRoute>,
    selected_deal_idx: Option<usize>,
    toast: Option<(String, Instant)>,

    // Async worker channel
    rx: Receiver<ScanResult>,
    tx: Sender<ScanResult>,
}

impl DesktopApp {
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
    ) -> Self {
        let (tx, rx) = channel();

        let initial_status = if initial_deals.is_empty() {
            "Ready. Configure ship & location, then click SCAN MARKET.".to_string()
        } else {
            format!("Loaded {} profitable arbitrage deals (ROI >= 15.0%).", initial_deals.len())
        };

        let has_forge = initial_regions.contains(&10000002) || initial_regions.is_empty();
        let has_domain = initial_regions.contains(&10000043);
        let has_sinq = initial_regions.contains(&10000032);
        let has_heim = initial_regions.contains(&10000030);
        let has_metr = initial_regions.contains(&10000042);

        Self {
            sde,
            graph,
            esi,
            ship_is_big: initial_ship_is_big,
            home_system: initial_home_system,
            safe_only: initial_safe_only,
            npc_stations_only: initial_npc_stations_only,
            min_roi_pct: initial_min_roi.max(15.0),
            min_profit_isk: if initial_min_profit <= 0.0 { 1_000_000.0 } else { initial_min_profit },
            broker_fee_pct: 3.0,
            sales_tax_pct: 8.0,
            max_route_stops: 5,

            region_the_forge: has_forge,
            region_domain: has_domain,
            region_sinq_laison: has_sinq,
            region_heimatar: has_heim,
            region_metropolis: has_metr,

            filter_item_name: String::new(),
            sort_by: DealSortBy::ProfitPerJump,
            sort_descending: true,
            min_profit_filter_m: 0.0,

            tab: ActiveTab::Deals,
            is_scanning: false,
            status_text: initial_status,
            deals: initial_deals,
            route: initial_route,
            selected_deal_idx: None,
            toast: None,
            rx,
            tx,
        }
    }

    fn set_toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    fn selected_regions(&self) -> Vec<u32> {
        let mut list = Vec::new();
        if self.region_the_forge { list.push(10000002); }
        if self.region_domain { list.push(10000043); }
        if self.region_sinq_laison { list.push(10000032); }
        if self.region_heimatar { list.push(10000030); }
        if self.region_metropolis { list.push(10000042); }
        if list.is_empty() {
            list.push(10000002); // Default to The Forge (Jita)
        }
        list
    }

    fn start_scan(&mut self) {
        if self.is_scanning {
            return;
        }

        self.is_scanning = true;
        self.status_text = "Connecting to CCP ESI API & streaming market orders...".to_string();

        let sde = Arc::clone(&self.sde);
        let graph = Arc::clone(&self.graph);
        let esi = Arc::clone(&self.esi);
        let tx = self.tx.clone();

        let ship = if self.ship_is_big {
            BIG_HAULER
        } else {
            SMALL_HAULER
        };

        let home_name = self.home_system.clone();
        let safe_only = self.safe_only;
        let npc_stations_only = self.npc_stations_only;
        let min_roi = self.min_roi_pct.max(15.0); // Strict 15% invariant
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
                let route = plan_route(&deals, ship, home_id, &graph, safe_only, max_stops);

                let _ = tx.send(ScanResult::Success {
                    deals,
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
        // Poll worker thread
        if let Ok(res) = self.rx.try_recv() {
            self.is_scanning = false;
            match res {
                ScanResult::Success {
                    deals,
                    route,
                    total_sells,
                    total_buys,
                } => {
                    let count = deals.len();
                    self.status_text = format!(
                        "✔ Scan Complete! Parsed {} sell orders & {} buy orders. Discovered {} profitable deals (ROI >= {:.1}%).",
                        total_sells, total_buys, count, self.min_roi_pct
                    );
                    self.deals = deals;
                    self.route = Some(route);
                    self.selected_deal_idx = None;
                    self.set_toast(format!("Discovered {} profitable arbitrage deals!", count));
                }
                ScanResult::Error(err) => {
                    self.status_text = format!("✘ {}", err);
                    self.set_toast(format!("Error: {}", err));
                }
            }
        }

        // Apply Premium EVE Sci-Fi Theme
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(12, 16, 26);
        visuals.window_fill = Color32::from_rgb(16, 22, 36);
        visuals.extreme_bg_color = Color32::from_rgb(9, 12, 20);
        visuals.override_text_color = Some(Color32::from_rgb(230, 240, 255));
        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(18, 25, 42);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(35, 48, 76));
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(22, 31, 52);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(32, 45, 75);
        visuals.widgets.active.bg_fill = Color32::from_rgb(0, 160, 220);
        ctx.set_visuals(visuals);

        // Top Control and Navigation Panel
        egui::TopBottomPanel::top("top_panel")
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(13, 18, 30))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(30, 42, 68)))
                    .inner_margin(12.0),
            )
            .show(ctx, |ui| {
                self.render_header_and_controls(ui);
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
                    ui.separator();
                    ui.label(RichText::new(&self.status_text).size(12.0).color(Color32::from_rgb(200, 220, 245)));

                    // Toast message if active
                    if let Some((ref msg, time)) = self.toast {
                        if time.elapsed().as_secs() < 4 {
                            ui.separator();
                            ui.label(
                                RichText::new(format!("💬 {}", msg))
                                    .size(12.0)
                                    .color(Color32::from_rgb(0, 230, 255))
                                    .strong(),
                            );
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("EVE Trade Scout • Linux Fedora Native")
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
                ActiveTab::Route => self.render_route_tab(ui),
                ActiveTab::Settings => self.render_settings_tab(ui),
            });
    }
}

impl DesktopApp {
    fn render_header_and_controls(&mut self, ui: &mut egui::Ui) {
        // App title & big scan action button
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("🚀 EVE TRADE SCOUT")
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
                if self.is_scanning {
                    ui.spinner();
                    ui.label(
                        RichText::new("FETCHING ESI ORDERS...")
                            .size(14.0)
                            .color(Color32::from_rgb(255, 205, 0))
                            .strong(),
                    );
                } else {
                    let scan_btn = egui::Button::new(
                        RichText::new("⚡ SCAN MARKET NOW")
                            .size(15.0)
                            .color(Color32::BLACK)
                            .strong(),
                    )
                    .fill(Color32::from_rgb(0, 235, 170))
                    .min_size(Vec2::new(200.0, 36.0));

                    if ui.add(scan_btn).clicked() {
                        self.start_scan();
                    }
                }
            });
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        // Control parameters row
        ui.horizontal_wrapped(|ui| {
            // Ship Selector
            ui.label(RichText::new("SHIP PROFILE:").size(12.0).color(Color32::from_rgb(150, 180, 220)).strong());
            let big_label = if self.ship_is_big { "✔ Big Hauler (60k m³ · 500M)" } else { "Big Hauler (60k m³ · 500M)" };
            if ui.selectable_label(self.ship_is_big, RichText::new(big_label).strong()).clicked() {
                self.ship_is_big = true;
            }
            let small_label = if !self.ship_is_big { "✔ Small Hauler (30k m³ · 50M)" } else { "Small Hauler (30k m³ · 50M)" };
            if ui.selectable_label(!self.ship_is_big, RichText::new(small_label).strong()).clicked() {
                self.ship_is_big = false;
            }

            ui.separator();

            // Home System Input
            ui.label(RichText::new("📍 STARTING SYSTEM:").size(12.0).color(Color32::from_rgb(150, 180, 220)).strong());
            ui.add(egui::TextEdit::singleline(&mut self.home_system).desired_width(95.0));

            // Quick hub chips
            if ui.small_button("Jita").on_hover_text("Set origin to Jita (The Forge)").clicked() {
                self.home_system = "Jita".to_string();
            }
            if ui.small_button("Amarr").on_hover_text("Set origin to Amarr (Domain)").clicked() {
                self.home_system = "Amarr".to_string();
            }
            if ui.small_button("Dodixie").on_hover_text("Set origin to Dodixie (Sinq Laison)").clicked() {
                self.home_system = "Dodixie".to_string();
            }
            if ui.small_button("Rens").on_hover_text("Set origin to Rens (Heimatar)").clicked() {
                self.home_system = "Rens".to_string();
            }

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
                    .range(15.0..=100.0)
                    .suffix("%"),
            );
            ui.label(RichText::new("(≥15% Enforced)").size(11.0).color(Color32::GRAY));
        });

        ui.add_space(8.0);
        ui.separator();

        // Navigation Tabs & KPI strip
        ui.horizontal(|ui| {
            let deals_badge = format!("📊 Deals ({})", self.deals.len());
            let route_stops = self.route.as_ref().map(|r| r.stops.len()).unwrap_or(0);
            let route_badge = format!("🗺️ Planned Route ({})", route_stops);

            if ui.selectable_label(self.tab == ActiveTab::Deals, RichText::new(deals_badge).size(14.0).strong()).clicked() {
                self.tab = ActiveTab::Deals;
            }
            if ui.selectable_label(self.tab == ActiveTab::Route, RichText::new(route_badge).size(14.0).strong()).clicked() {
                self.tab = ActiveTab::Route;
            }
            if ui.selectable_label(self.tab == ActiveTab::Settings, RichText::new("⚙️ Settings & Fees").size(14.0).strong()).clicked() {
                self.tab = ActiveTab::Settings;
            }

            // Executive KPI strip on the right
            if !self.deals.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(top_deal) = self.deals.first() {
                        ui.label(
                            RichText::new(format!("⚡ Top PPJ: {:.2}M/jump", top_deal.profit_per_jump / 1_000_000.0))
                                .size(12.0)
                                .color(Color32::from_rgb(255, 215, 0))
                                .strong(),
                        );
                        ui.separator();
                        ui.label(
                            RichText::new(format!("💎 Best Profit: +{:.2}M ISK", top_deal.net_profit / 1_000_000.0))
                                .size(12.0)
                                .color(Color32::from_rgb(0, 240, 160))
                                .strong(),
                        );
                    }
                });
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
                ui.horizontal(|ui| {
                    ui.label(RichText::new("🔍 Search Item:").strong());
                    ui.add(egui::TextEdit::singleline(&mut self.filter_item_name).desired_width(180.0));
                    if !self.filter_item_name.is_empty() && ui.button("✖ Clear").clicked() {
                        self.filter_item_name.clear();
                    }

                    ui.separator();

                    ui.label(RichText::new("Sort By:").strong());
                    if ui.selectable_label(self.sort_by == DealSortBy::ProfitPerJump, "Profit/Jump").clicked() {
                        self.sort_by = DealSortBy::ProfitPerJump;
                    }
                    if ui.selectable_label(self.sort_by == DealSortBy::TotalProfit, "Total Profit").clicked() {
                        self.sort_by = DealSortBy::TotalProfit;
                    }
                    if ui.selectable_label(self.sort_by == DealSortBy::RoiPct, "Net ROI %").clicked() {
                        self.sort_by = DealSortBy::RoiPct;
                    }
                    if ui.selectable_label(self.sort_by == DealSortBy::Jumps, "Fewest Jumps").clicked() {
                        self.sort_by = DealSortBy::Jumps;
                    }
                    if ui.selectable_label(self.sort_by == DealSortBy::Cost, "Lowest Cost").clicked() {
                        self.sort_by = DealSortBy::Cost;
                    }

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
                });
            });

        ui.add_space(8.0);

        // Filter and sort candidate deals
        let filter = self.filter_item_name.to_lowercase();
        let min_profit_threshold = self.min_profit_filter_m * 1_000_000.0;

        let mut filtered: Vec<(usize, &ArbitrageDeal)> = self
            .deals
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                (filter.is_empty() || d.item_name.to_lowercase().contains(&filter))
                    && d.net_profit >= min_profit_threshold
            })
            .collect();

        match self.sort_by {
            DealSortBy::ProfitPerJump => {
                filtered.sort_by(|(_, a), (_, b)| b.profit_per_jump.partial_cmp(&a.profit_per_jump).unwrap_or(std::cmp::Ordering::Equal));
            }
            DealSortBy::TotalProfit => {
                filtered.sort_by(|(_, a), (_, b)| b.net_profit.partial_cmp(&a.net_profit).unwrap_or(std::cmp::Ordering::Equal));
            }
            DealSortBy::RoiPct => {
                filtered.sort_by(|(_, a), (_, b)| b.net_profit_pct.partial_cmp(&a.net_profit_pct).unwrap_or(std::cmp::Ordering::Equal));
            }
            DealSortBy::Jumps => {
                filtered.sort_by(|(_, a), (_, b)| a.total_jumps.cmp(&b.total_jumps));
            }
            DealSortBy::Cost => {
                filtered.sort_by(|(_, a), (_, b)| a.capital_required.partial_cmp(&b.capital_required).unwrap_or(std::cmp::Ordering::Equal));
            }
        }

        if !self.sort_descending {
            filtered.reverse();
        }

        // Split view: Deals table on top, Selected Deal Dossier at bottom (if selected)
        let total_avail_height = ui.available_height();
        let table_height = if self.selected_deal_idx.is_some() {
            (total_avail_height - 230.0).max(200.0)
        } else {
            total_avail_height
        };

        let mut toggle_select: Option<usize> = None;
        let mut action_toast: Option<String> = None;
        let current_selected_idx = self.selected_deal_idx;

        ui.allocate_ui(Vec2::new(ui.available_width(), table_height), |ui| {
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("deals_grid")
                        .striped(true)
                        .spacing(Vec2::new(12.0, 7.0))
                        .min_col_width(45.0)
                        .show(ui, |ui| {
                            // Column Headers
                            ui.label(RichText::new("#").color(Color32::from_rgb(255, 205, 0)).strong());
                            ui.label(RichText::new("ITEM").color(Color32::from_rgb(255, 205, 0)).strong());
                            ui.label(RichText::new("BUY SYSTEM").color(Color32::from_rgb(255, 205, 0)).strong());
                            ui.label(RichText::new("SELL SYSTEM").color(Color32::from_rgb(255, 205, 0)).strong());
                            ui.label(RichText::new("QTY").color(Color32::from_rgb(255, 205, 0)).strong());
                            ui.label(RichText::new("VOL (m³)").color(Color32::from_rgb(255, 205, 0)).strong());
                            ui.label(RichText::new("COST (M)").color(Color32::from_rgb(255, 205, 0)).strong());
                            ui.label(RichText::new("NET PROFIT").color(Color32::from_rgb(255, 205, 0)).strong());
                            ui.label(RichText::new("NET ROI").color(Color32::from_rgb(255, 205, 0)).strong());
                            ui.label(RichText::new("JUMPS").color(Color32::from_rgb(255, 205, 0)).strong());
                            ui.label(RichText::new("PROFIT/JUMP").color(Color32::from_rgb(255, 205, 0)).strong());
                            ui.label(RichText::new("QUICK ACTIONS").color(Color32::from_rgb(255, 205, 0)).strong());
                            ui.end_row();

                            for (display_rank, (orig_idx, deal)) in filtered.iter().enumerate() {
                                let is_selected = current_selected_idx == Some(*orig_idx);

                                // Rank badge
                                let rank_color = match display_rank {
                                    0 => Color32::from_rgb(255, 215, 0),   // Gold
                                    1 => Color32::from_rgb(200, 215, 230), // Silver
                                    2 => Color32::from_rgb(205, 127, 50),  // Bronze
                                    _ => Color32::GRAY,
                                };
                                ui.label(RichText::new(format!("#{}", display_rank + 1)).color(rank_color).strong());

                                // Item Name with selection toggle and citadel badge if applicable
                                let item_color = if is_selected {
                                    Color32::from_rgb(0, 230, 255)
                                } else {
                                    Color32::WHITE
                                };
                                ui.horizontal(|ui| {
                                    if ui.selectable_label(is_selected, RichText::new(&deal.item_name).color(item_color).strong()).clicked() {
                                        toggle_select = Some(*orig_idx);
                                    }
                                    if !crate::arbitrage::is_npc_station(deal.source_station_id) || !crate::arbitrage::is_npc_station(deal.dest_station_id) {
                                        ui.label(RichText::new("🏰").color(Color32::from_rgb(255, 185, 60)).strong())
                                            .on_hover_text("Includes player-owned Upwell Citadel / Structure");
                                    }
                                });

                                ui.label(RichText::new(&deal.source_system_name).color(Color32::from_rgb(120, 200, 255)));
                                ui.label(RichText::new(&deal.dest_system_name).color(Color32::from_rgb(255, 140, 240)));
                                ui.label(format!("{}", deal.quantity));
                                ui.label(format!("{:.1}", deal.volume_used));
                                ui.label(format!("{:.2}M", deal.capital_required / 1_000_000.0));

                                // Net profit glowing green
                                ui.label(
                                    RichText::new(format!("+{:.2}M ISK", deal.net_profit / 1_000_000.0))
                                        .color(Color32::from_rgb(0, 240, 150))
                                        .strong(),
                                );

                                // ROI
                                ui.label(
                                    RichText::new(format!("+{:.1}%", deal.net_profit_pct))
                                        .color(Color32::from_rgb(0, 255, 180))
                                        .strong(),
                                );

                                // Jumps breakdown
                                let jumps_tag = format!("{}+{}={}", deal.jumps_from_home, deal.jumps_source_dest, deal.total_jumps);
                                ui.label(RichText::new(jumps_tag).color(Color32::LIGHT_GRAY));

                                // Profit per jump
                                ui.label(
                                    RichText::new(format!("{:.2}M", deal.profit_per_jump / 1_000_000.0))
                                        .color(Color32::from_rgb(255, 215, 0))
                                        .strong(),
                                );

                                // Quick copy buttons
                                ui.horizontal(|ui| {
                                    if ui.small_button("📋 Buy").on_hover_text("Copy buy system to clipboard").clicked() {
                                        ui.output_mut(|o| o.copied_text = deal.source_system_name.clone());
                                        action_toast = Some(format!("Copied buy system '{}' to clipboard!", deal.source_system_name));
                                    }
                                    if ui.small_button("📋 Sell").on_hover_text("Copy sell system to clipboard").clicked() {
                                        ui.output_mut(|o| o.copied_text = deal.dest_system_name.clone());
                                        action_toast = Some(format!("Copied sell system '{}' to clipboard!", deal.dest_system_name));
                                    }
                                });

                                ui.end_row();
                            }
                        });
                });
        });

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

        // Bottom Selected Deal Dossier
        if let Some(idx) = self.selected_deal_idx {
            if let Some(deal) = self.deals.get(idx).cloned() {
                ui.add_space(8.0);
                self.render_deal_dossier(ui, &deal);
            }
        }
    }

    fn render_deal_dossier(&mut self, ui: &mut egui::Ui, deal: &ArbitrageDeal) {
        egui::Frame::none()
            .fill(Color32::from_rgb(18, 25, 42))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0, 200, 255)))
            .rounding(6.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("🔍 DEAL DOSSIER: {}", deal.item_name))
                            .size(16.0)
                            .color(Color32::from_rgb(0, 220, 255))
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✖ Close Dossier").clicked() {
                            self.selected_deal_idx = None;
                        }
                    });
                });

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                ui.columns(3, |cols| {
                    // Column 1: Buy Details
                    cols[0].heading(RichText::new("🛒 PURCHASE").size(13.0).color(Color32::from_rgb(100, 200, 255)));
                    let src_is_npc = crate::arbitrage::is_npc_station(deal.source_station_id);
                    let (src_badge, src_badge_color) = if src_is_npc {
                        ("🏛️ NPC Station", Color32::from_rgb(100, 210, 255))
                    } else {
                        ("🏰 Upwell Citadel", Color32::from_rgb(255, 185, 60))
                    };
                    cols[0].label(RichText::new(&deal.source_station_name).size(12.0).strong());
                    cols[0].label(RichText::new(src_badge).size(11.0).color(src_badge_color).strong());
                    cols[0].label(format!("System: {} ({} jumps from home)", deal.source_system_name, deal.jumps_from_home));
                    cols[0].label(format!("Unit Price: {:.2} ISK", deal.buy_price));
                    cols[0].label(format!("Quantity: {} units", deal.quantity));
                    cols[0].label(format!("Capital Cost: {:.2}M ISK", deal.capital_required / 1_000_000.0));
                    if cols[0].small_button("📋 Copy Buy Station").clicked() {
                        cols[0].output_mut(|o| o.copied_text = deal.source_station_name.clone());
                    }

                    // Column 2: Sell Details
                    cols[1].heading(RichText::new("💰 SALE").size(13.0).color(Color32::from_rgb(255, 140, 240)));
                    let dst_is_npc = crate::arbitrage::is_npc_station(deal.dest_station_id);
                    let (dst_badge, dst_badge_color) = if dst_is_npc {
                        ("🏛️ NPC Station", Color32::from_rgb(100, 210, 255))
                    } else {
                        ("🏰 Upwell Citadel", Color32::from_rgb(255, 185, 60))
                    };
                    cols[1].label(RichText::new(&deal.dest_station_name).size(12.0).strong());
                    cols[1].label(RichText::new(dst_badge).size(11.0).color(dst_badge_color).strong());
                    cols[1].label(format!("System: {} ({} jumps delivery)", deal.dest_system_name, deal.jumps_source_dest));
                    cols[1].label(format!("Unit Price: {:.2} ISK", deal.sell_price));
                    cols[1].label(format!("Gross Revenue: {:.2}M ISK", (deal.sell_price * deal.quantity as f64) / 1_000_000.0));
                    cols[1].label(format!("Price Spread: +{:.1}%", deal.spread_pct));
                    if cols[1].small_button("📋 Copy Sell Station").clicked() {
                        cols[1].output_mut(|o| o.copied_text = deal.dest_station_name.clone());
                    }

                    // Column 3: Net Profit & Ship Utilization
                    cols[2].heading(RichText::new("📈 FINANCIAL SUMMARY").size(13.0).color(Color32::from_rgb(0, 240, 160)));
                    cols[2].label(
                        RichText::new(format!("NET PROFIT: +{:.2}M ISK", deal.net_profit / 1_000_000.0))
                            .color(Color32::from_rgb(0, 255, 150))
                            .strong(),
                    );
                    cols[2].label(
                        RichText::new(format!("NET ROI: +{:.1}%", deal.net_profit_pct))
                            .color(Color32::from_rgb(0, 240, 180))
                            .strong(),
                    );
                    cols[2].label(
                        RichText::new(format!("PROFIT / JUMP: {:.2}M ISK", deal.profit_per_jump / 1_000_000.0))
                            .color(Color32::from_rgb(255, 215, 0))
                            .strong(),
                    );
                    cols[2].label(format!("Cargo Used: {:.1} m³", deal.volume_used));
                    cols[2].label(format!("Total Travel: {} jumps", deal.total_jumps));
                });
            });
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
                            ui.output_mut(|o| o.copied_text = trail);
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

        egui::ScrollArea::vertical().show(ui, |ui| {
            for stop in &route.stops {
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
                                ui.output_mut(|o| o.copied_text = stop.deal.source_system_name.clone());
                                self.set_toast(format!("Copied '{}'", stop.deal.source_system_name));
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
                                cols[0].output_mut(|o| o.copied_text = stop.deal.source_station_name.clone());
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
                                cols[1].output_mut(|o| o.copied_text = stop.deal.dest_station_name.clone());
                            }
                        });
                    });
                ui.add_space(6.0);
            }

            ui.add_space(10.0);
            ui.label(RichText::new("Complete In-Game Autopilot Waypoints Trail:").strong());
            ui.label(
                RichText::new(route.waypoint_names.join(" ➔ "))
                    .size(13.0)
                    .color(Color32::from_rgb(180, 215, 255)),
            );
        });
    }

    fn render_settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Configuration & Tax Parameters");
        ui.add_space(10.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
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
                        ui.add(egui::DragValue::new(&mut self.sales_tax_pct).speed(0.1).range(3.6..=8.0).suffix("%"));
                        ui.label(RichText::new("(Default 8.0%, reduced by Accounting skill)").color(Color32::GRAY));
                    });
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

            // Card 3: Station & Structure Access Pool
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
                    ui.label("• Strict 15.0% Net Profitability: Deals producing under 15% net return after all taxes are rejected.");
                    ui.label("• Big Hauler Constraints: 60,000 m³ volume capacity / 500,000,000 ISK max budget.");
                    ui.label("• Small Hauler Constraints: 30,000 m³ volume capacity / 50,000,000 ISK max budget.");
                    ui.label("• True Profit-Per-Jump: Jumps required to fly to the pickup station are counted in total flight distance.");
                    ui.label("• Safe Stargate Navigation: Routes avoid lowsec (<0.45) and nullsec camps by default.");
                    ui.label("• Highsec Space Only: All trade routes and station orders are restricted to high security space.");
                });
        });
    }
}

pub fn run_desktop_gui(app: DesktopApp) -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("EVE Trade Scout — Arbitrage & Multi-Stop Route Planner")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([960.0, 540.0]),
        ..Default::default()
    };

    eframe::run_native(
        "EVE Trade Scout",
        options,
        Box::new(|_cc| Ok(Box::new(app))),
    )
}
