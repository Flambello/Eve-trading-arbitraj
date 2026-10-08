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

#[derive(PartialEq)]
enum ActiveTab {
    Deals,
    Route,
    Settings,
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
    min_roi_pct: f64,
    min_profit_isk: f64,
    max_route_stops: usize,
    regions: Vec<u32>,

    // Search filter in deals list
    filter_item_name: String,

    // Active state
    tab: ActiveTab,
    is_scanning: bool,
    status_text: String,
    deals: Vec<ArbitrageDeal>,
    route: Option<TradeRoute>,
    selected_deal_idx: Option<usize>,

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
        initial_min_roi: f64,
        initial_min_profit: f64,
        initial_regions: Vec<u32>,
        initial_deals: Vec<ArbitrageDeal>,
        initial_route: Option<TradeRoute>,
    ) -> Self {
        let (tx, rx) = channel();

        let initial_status = if initial_deals.is_empty() {
            "Ready to scan. Select ship and starting location, then click Scan Market.".to_string()
        } else {
            format!("Found {} profitable arbitrage deals.", initial_deals.len())
        };

        Self {
            sde,
            graph,
            esi,
            ship_is_big: initial_ship_is_big,
            home_system: initial_home_system,
            safe_only: initial_safe_only,
            min_roi_pct: initial_min_roi,
            min_profit_isk: initial_profit_fallback(initial_min_profit),
            max_route_stops: 5,
            regions: initial_regions,
            filter_item_name: String::new(),
            tab: ActiveTab::Deals,
            is_scanning: false,
            status_text: initial_status,
            deals: initial_deals,
            route: initial_route,
            selected_deal_idx: None,
            rx,
            tx,
        }
    }

    fn start_scan(&mut self) {
        if self.is_scanning {
            return;
        }

        self.is_scanning = true;
        self.status_text = "Fetching market orders from CCP ESI API...".to_string();

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
        let min_roi = self.min_roi_pct;
        let min_profit = self.min_profit_isk;
        let max_stops = self.max_route_stops;
        let regions = self.regions.clone();

        thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(ScanResult::Error(format!("Failed to start runtime: {}", e)));
                    return;
                }
            };

            rt.block_on(async move {
                let home_id = match sde.get_system_id(&home_name) {
                    Some(id) => id,
                    None => {
                        let _ = tx.send(ScanResult::Error(format!(
                            "Solar system '{}' not found in EVE universe.",
                            home_name
                        )));
                        return;
                    }
                };

                let markets = match fetch_markets(&esi, &regions).await {
                    Ok(m) => m,
                    Err(e) => {
                        let _ = tx.send(ScanResult::Error(format!("ESI error: {}", e)));
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
                    broker_fee: 3.0,
                    sales_tax: 8.0,
                    safe_only,
                    npc_stations_only: true,
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

fn initial_profit_fallback(val: f64) -> f64 {
    if val <= 0.0 {
        1_000_000.0
    } else {
        val
    }
}

impl eframe::App for DesktopApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for worker thread results
        if let Ok(res) = self.rx.try_recv() {
            self.is_scanning = false;
            match res {
                ScanResult::Success {
                    deals,
                    route,
                    total_sells,
                    total_buys,
                } => {
                    let len = deals.len();
                    self.status_text = format!(
                        "Scan complete! Analyzed {} sells and {} buys. Found {} profitable deals (ROI >= {:.1}%).",
                        total_sells, total_buys, len, self.min_roi_pct
                    );
                    self.deals = deals;
                    self.route = Some(route);
                    self.selected_deal_idx = None;
                }
                ScanResult::Error(err) => {
                    self.status_text = format!("Error: {}", err);
                }
            }
        }

        // Custom EVE sci-fi theme
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(14, 18, 28);
        visuals.window_fill = Color32::from_rgb(18, 24, 38);
        visuals.override_text_color = Some(Color32::from_rgb(220, 230, 245));
        ctx.set_visuals(visuals);

        // Top Header and Controls Panel
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading(
                    RichText::new("🚀 EVE Trade Scout")
                        .size(22.0)
                        .color(Color32::from_rgb(0, 215, 255))
                        .strong(),
                );
                ui.label(
                    RichText::new("│ Arbitrage Engine & Route Planner")
                        .size(14.0)
                        .color(Color32::GRAY),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.is_scanning {
                        ui.spinner();
                        ui.label(
                            RichText::new("Scanning...")
                                .color(Color32::from_rgb(255, 200, 0))
                                .strong(),
                        );
                    } else {
                        let btn = ui.add_sized(
                            Vec2::new(180.0, 32.0),
                            egui::Button::new(
                                RichText::new("🔄 SCAN MARKET")
                                    .color(Color32::BLACK)
                                    .strong(),
                            )
                            .fill(Color32::from_rgb(0, 230, 160)),
                        );
                        if btn.clicked() {
                            self.start_scan();
                        }
                    }
                });
            });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            // Controls row
            ui.horizontal_wrapped(|ui| {
                // Ship selection
                ui.label(RichText::new("Ship Profile:").strong());
                if ui
                    .selectable_label(self.ship_is_big, "Big Hauler (60k m³ / 500M)")
                    .clicked()
                {
                    self.ship_is_big = true;
                }
                if ui
                    .selectable_label(!self.ship_is_big, "Small Hauler (30k m³ / 50M)")
                    .clicked()
                {
                    self.ship_is_big = false;
                }

                ui.separator();

                // Starting System
                ui.label(RichText::new("Starting System:").strong());
                ui.add(egui::TextEdit::singleline(&mut self.home_system).desired_width(90.0));

                // Quick presets
                ui.label("Hubs:");
                if ui.small_button("Jita").clicked() {
                    self.home_system = "Jita".to_string();
                }
                if ui.small_button("Amarr").clicked() {
                    self.home_system = "Amarr".to_string();
                }
                if ui.small_button("Dodixie").clicked() {
                    self.home_system = "Dodixie".to_string();
                }
                if ui.small_button("Rens").clicked() {
                    self.home_system = "Rens".to_string();
                }

                ui.separator();

                // Safe Routing
                ui.checkbox(&mut self.safe_only, "Safe Highsec Only");

                ui.separator();

                // Minimum ROI % (Enforced >= 15%)
                ui.label(RichText::new("Min ROI %:").strong());
                ui.add(
                    egui::DragValue::new(&mut self.min_roi_pct)
                        .speed(0.5)
                        .range(15.0..=100.0)
                        .suffix("%"),
                );
            });

            ui.add_space(6.0);
            ui.separator();

            // Navigation Tabs
            ui.horizontal(|ui| {
                let deals_count = self.deals.len();
                let route_stops = self.route.as_ref().map(|r| r.stops.len()).unwrap_or(0);

                if ui
                    .selectable_label(
                        self.tab == ActiveTab::Deals,
                        format!("📊 Arbitrage Deals ({})", deals_count),
                    )
                    .clicked()
                {
                    self.tab = ActiveTab::Deals;
                }

                if ui
                    .selectable_label(
                        self.tab == ActiveTab::Route,
                        format!("🗺️ Planned Multi-Stop Route ({})", route_stops),
                    )
                    .clicked()
                {
                    self.tab = ActiveTab::Route;
                }

                if ui
                    .selectable_label(self.tab == ActiveTab::Settings, "⚙️ Settings & Info")
                    .clicked()
                {
                    self.tab = ActiveTab::Settings;
                }
            });
            ui.add_space(4.0);
        });

        // Bottom Status Bar
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&self.status_text)
                        .size(12.0)
                        .color(Color32::from_rgb(180, 200, 220)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new("EVE Trade Scout v0.1.0 • Linux Fedora Native")
                            .size(11.0)
                            .color(Color32::DARK_GRAY),
                    );
                });
            });
        });

        // Central Content Area
        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            ActiveTab::Deals => self.render_deals_tab(ui),
            ActiveTab::Route => self.render_route_tab(ui),
            ActiveTab::Settings => self.render_settings_tab(ui),
        });
    }
}

impl DesktopApp {
    fn render_deals_tab(&mut self, ui: &mut egui::Ui) {
        if self.deals.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(100.0);
                ui.heading(
                    RichText::new("No Deals Found Yet")
                        .size(20.0)
                        .color(Color32::GRAY),
                );
                ui.label("Click the 'SCAN MARKET' button above to fetch live orders and calculate profitable trades.");
                ui.add_space(20.0);
                if ui
                    .button(
                        RichText::new("🚀 Start Scan Now")
                            .size(16.0)
                            .color(Color32::BLACK),
                    )
                    .clicked()
                {
                    self.start_scan();
                }
            });
            return;
        }

        ui.horizontal(|ui| {
            ui.label("Filter by Item Name:");
            ui.add(egui::TextEdit::singleline(&mut self.filter_item_name).desired_width(200.0));
            if !self.filter_item_name.is_empty() && ui.button("Clear").clicked() {
                self.filter_item_name.clear();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new("Ranked by Profit / Jump (Includes travel to source)")
                        .size(12.0)
                        .color(Color32::from_rgb(0, 200, 255)),
                );
            });
        });
        ui.add_space(6.0);

        let filter = self.filter_item_name.to_lowercase();
        let filtered_deals: Vec<(usize, &ArbitrageDeal)> = self
            .deals
            .iter()
            .enumerate()
            .filter(|(_, d)| filter.is_empty() || d.item_name.to_lowercase().contains(&filter))
            .collect();

        // Split view: Table on top/left, Selected Deal Details on bottom/right
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("deals_grid")
                    .striped(true)
                    .spacing(Vec2::new(12.0, 6.0))
                    .min_col_width(50.0)
                    .show(ui, |ui| {
                        // Headers
                        ui.label(RichText::new("#").strong().color(Color32::YELLOW));
                        ui.label(RichText::new("Item").strong().color(Color32::YELLOW));
                        ui.label(RichText::new("Buy System").strong().color(Color32::YELLOW));
                        ui.label(RichText::new("Sell System").strong().color(Color32::YELLOW));
                        ui.label(RichText::new("Qty").strong().color(Color32::YELLOW));
                        ui.label(RichText::new("Vol m³").strong().color(Color32::YELLOW));
                        ui.label(RichText::new("Cost (M)").strong().color(Color32::YELLOW));
                        ui.label(RichText::new("Profit (M)").strong().color(Color32::YELLOW));
                        ui.label(RichText::new("Net ROI %").strong().color(Color32::YELLOW));
                        ui.label(RichText::new("Jumps").strong().color(Color32::YELLOW));
                        ui.label(RichText::new("Profit/Jump").strong().color(Color32::YELLOW));
                        ui.label(RichText::new("Action").strong().color(Color32::YELLOW));
                        ui.end_row();

                        // Rows
                        for (original_idx, deal) in filtered_deals {
                            let is_selected = self.selected_deal_idx == Some(original_idx);

                            let row_color = if original_idx < 3 {
                                Color32::from_rgb(0, 255, 160)
                            } else {
                                Color32::WHITE
                            };

                            let rank_text = format!("{}", original_idx + 1);
                            ui.label(RichText::new(rank_text).color(Color32::GRAY));

                            let item_label = ui.selectable_label(
                                is_selected,
                                RichText::new(&deal.item_name).color(row_color).strong(),
                            );
                            if item_label.clicked() {
                                self.selected_deal_idx = Some(original_idx);
                            }

                            ui.label(RichText::new(&deal.source_system_name).color(Color32::from_rgb(130, 200, 255)));
                            ui.label(RichText::new(&deal.dest_system_name).color(Color32::from_rgb(255, 140, 255)));
                            ui.label(format!("{}", deal.quantity));
                            ui.label(format!("{:.1}", deal.volume_used));
                            ui.label(format!("{:.2}M", deal.capital_required / 1_000_000.0));

                            ui.label(
                                RichText::new(format!("{:.2}M", deal.net_profit / 1_000_000.0))
                                    .color(Color32::from_rgb(0, 255, 128))
                                    .strong(),
                            );

                            ui.label(
                                RichText::new(format!("+{:.1}%", deal.net_profit_pct))
                                    .color(Color32::from_rgb(0, 255, 160))
                                    .strong(),
                            );

                            let jumps_str = format!("{}+{}={}", deal.jumps_from_home, deal.jumps_source_dest, deal.total_jumps);
                            ui.label(RichText::new(jumps_str).color(Color32::LIGHT_GRAY));

                            ui.label(
                                RichText::new(format!("{:.2}M", deal.profit_per_jump / 1_000_000.0))
                                    .color(Color32::from_rgb(255, 215, 0))
                                    .strong(),
                            );

                            ui.horizontal(|ui| {
                                if ui.small_button("📋 Copy Buy").on_hover_text("Copy buy system to clipboard").clicked() {
                                    ui.output_mut(|o| o.copied_text = deal.source_system_name.clone());
                                }
                                if ui.small_button("📋 Copy Sell").on_hover_text("Copy sell system to clipboard").clicked() {
                                    ui.output_mut(|o| o.copied_text = deal.dest_system_name.clone());
                                }
                            });

                            ui.end_row();
                        }
                    });
            });
    }

    fn render_route_tab(&mut self, ui: &mut egui::Ui) {
        let route = match &self.route {
            Some(r) if !r.stops.is_empty() => r,
            _ => {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    ui.heading("No Route Planned Yet");
                    ui.label("Run a market scan to automatically plan an optimized multi-hop trade route.");
                });
                return;
            }
        };

        // Summary Card
        egui::Frame::none()
            .fill(Color32::from_rgb(24, 32, 50))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0, 180, 230)))
            .rounding(6.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        RichText::new("🗺️ Multi-Stop Route Summary")
                            .color(Color32::from_rgb(0, 220, 255))
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("📋 Copy Autopilot Trail").clicked() {
                            let trail = route.waypoint_names.join(" -> ");
                            ui.output_mut(|o| o.copied_text = trail);
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
                    ui.label(RichText::new(format!("Total Jumps: {}", route.total_jumps)).strong().color(Color32::YELLOW));
                    ui.separator();
                    ui.label(RichText::new(format!("Total Profit: +{:.2} M ISK", route.total_profit / 1_000_000.0)).strong().color(Color32::from_rgb(0, 255, 128)));
                    ui.separator();
                    ui.label(RichText::new(format!("Overall ROI: +{:.1}%", route_roi)).strong().color(Color32::from_rgb(0, 255, 180)));
                    ui.separator();
                    ui.label(RichText::new(format!("Profit / Jump: {:.2} M ISK", route.avg_profit_per_jump / 1_000_000.0)).strong().color(Color32::from_rgb(255, 215, 0)));
                    ui.separator();
                    ui.label(RichText::new(format!("Capital: {:.2} M ISK", route.total_capital_required / 1_000_000.0)).color(Color32::LIGHT_BLUE));
                    ui.separator();
                    ui.label(RichText::new(format!("Peak Cargo: {:.1} m³", route.max_volume_used)).color(Color32::from_rgb(255, 140, 255)));
                });
            });

        ui.add_space(10.0);
        ui.heading("Step-by-Step Waypoint Steps:");
        ui.add_space(4.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            for stop in &route.stops {
                egui::Frame::none()
                    .fill(Color32::from_rgb(20, 26, 40))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(50, 70, 100)))
                    .rounding(4.0)
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("[STOP {}]", stop.step))
                                    .color(Color32::YELLOW)
                                    .strong(),
                            );
                            ui.label(format!("Travel {} jumps to", stop.jumps_from_prev));
                            ui.label(
                                RichText::new(&stop.deal.source_system_name)
                                    .color(Color32::from_rgb(100, 200, 255))
                                    .strong(),
                            );
                            if ui.small_button("📋 Copy").clicked() {
                                ui.output_mut(|o| o.copied_text = stop.deal.source_system_name.clone());
                            }
                        });

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("  BUY:").color(Color32::LIGHT_GREEN).strong());
                            ui.label(RichText::new(format!("{}x {}", stop.deal.quantity, stop.deal.item_name)).strong());
                            ui.label(format!("@ {:.2} ISK (Cost: {:.2}M ISK)", stop.deal.buy_price, stop.deal.capital_required / 1_000_000.0));
                        });
                        ui.label(RichText::new(format!("  AT:   {}", stop.deal.source_station_name)).color(Color32::GRAY));

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(format!("  WARP: {} jumps to", stop.deal.jumps_source_dest));
                            ui.label(
                                RichText::new(&stop.deal.dest_system_name)
                                    .color(Color32::from_rgb(255, 140, 255))
                                    .strong(),
                            );
                            if ui.small_button("📋 Copy").clicked() {
                                ui.output_mut(|o| o.copied_text = stop.deal.dest_system_name.clone());
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("  SELL:").color(Color32::from_rgb(255, 180, 0)).strong());
                            ui.label(RichText::new(format!("{}x {}", stop.deal.quantity, stop.deal.item_name)).strong());
                            ui.label(format!("@ {:.2} ISK", stop.deal.sell_price));
                            ui.label(
                                RichText::new(format!("-> Profit: +{:.2}M ISK (+{:.1}% ROI)", stop.deal.net_profit / 1_000_000.0, stop.deal.net_profit_pct))
                                    .color(Color32::from_rgb(0, 255, 128))
                                    .strong(),
                            );
                        });
                        ui.label(RichText::new(format!("  AT:   {}", stop.deal.dest_station_name)).color(Color32::GRAY));
                    });
                ui.add_space(6.0);
            }

            ui.add_space(10.0);
            ui.label(RichText::new("Full In-Game Autopilot Waypoints Trail:").strong());
            ui.label(
                RichText::new(route.waypoint_names.join(" ➔ "))
                    .color(Color32::from_rgb(180, 210, 240)),
            );
        });
    }

    fn render_settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Application Configuration");
        ui.add_space(10.0);

        ui.label(RichText::new("Enforced Trading Rules:").strong());
        ui.label("• Minimum Net Profitability: At least 15.0% net return after all taxes & broker fees.");
        ui.label("• Big Hauler Constraints: 60,000 m³ cargo / 500,000,000 ISK budget.");
        ui.label("• Small Hauler Constraints: 30,000 m³ cargo / 50,000,000 ISK budget.");
        ui.label("• Profit-Per-Jump Ranking: Jumps to reach the buy station are factored into total jumps.");

        ui.add_space(15.0);
        ui.label(RichText::new("Configured Scan Regions:").strong());
        ui.label("• 10000002 (The Forge - Jita)");
        ui.label("• 10000043 (Domain - Amarr)");
        ui.label("• 10000032 (Sinq Laison - Dodixie)");
        ui.label("• 10000030 (Heimatar - Rens)");
        ui.label("• 10000042 (Metropolis - Hek)");

        ui.add_space(15.0);
        ui.label(RichText::new("Local Database & Caching:").strong());
        ui.label("• EVE Static Data Export (SDE): SQLite database in data/eve_sde.sqlite.");
        ui.label("• ESI API Cache: ETag headers cached in cache/etags.json.");
    }
}

pub fn run_desktop_gui(app: DesktopApp) -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("EVE Trade Scout — Arbitrage & Route Planner")
            .with_inner_size([1240.0, 780.0])
            .with_min_inner_size([900.0, 500.0]),
        ..Default::default()
    };

    eframe::run_native(
        "EVE Trade Scout",
        options,
        Box::new(|_cc| Ok(Box::new(app))),
    )
}
