#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::{bail, Context, Result};
use clap::Parser;
use eve_trade_scout::arbitrage::{
    bundle_route_cargo, find_deals, sanitize_csv_cell, ArbitrageConfig, ArbitrageDeal,
};
use eve_trade_scout::config::UserConfig;
use eve_trade_scout::esi::{fetch_markets_fresh, EsiClient};
use eve_trade_scout::graph::SystemGraph;
use eve_trade_scout::route_planner::{plan_route, TradeRoute};
use eve_trade_scout::sde::SdeLoader;
use eve_trade_scout::ui;
use eve_trade_scout::Args;
use std::fs::File;
use std::io::{stdout, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn print_terminal_deprecation_notice() {
    eprintln!("╔══════════════════════════════════════════════════════════════════════════════╗");
    eprintln!("║ ⚠ DEPRECATION NOTICE: Terminal/TUI mode is deprecated on all platforms.      ║");
    eprintln!("║ EVE Traveling Merchant is a desktop GUI-first application.                   ║");
    eprintln!("║ The terminal interface will be removed in a future release.                  ║");
    eprintln!("╚══════════════════════════════════════════════════════════════════════════════╝");
    eprintln!();
}

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(target_os = "windows")]
    unsafe {
        // When executed from cmd.exe or PowerShell, attach to the caller's console
        // so that CLI options (--help, --version, --export, or deprecated CLI output) work properly.
        extern "system" {
            fn AttachConsole(dw_process_id: u32) -> i32;
        }
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }

    let log_dir = resolve_log_dir();
    let _log_guard = match eve_trade_scout::logging::init_logging(Some(&log_dir), "info") {
        Ok(guard) => Some(guard),
        Err(e) => {
            eprintln!(
                "[WARN] Failed to initialize file logger at {:?}: {}. Falling back to console logging.",
                log_dir, e
            );
            eve_trade_scout::logging::init_logging(None, "info").ok()
        }
    };

    let args = Args::parse();

    let mut cfg = UserConfig::load().unwrap_or_default();

    // Override config with CLI arguments
    if let Some(ref ship_str) = args.ship {
        cfg.ship.profile = ship_str.clone();
    }
    if let Some(ref sys_str) = args.system {
        cfg.location.home_system = sys_str.clone();
    }
    if args.safe {
        cfg.scan.safe_route_only = true;
    } else if args.unsafe_routes {
        cfg.scan.safe_route_only = false;
    }
    if args.npc_only {
        cfg.scan.npc_stations_only = true;
    } else if args.allow_structures {
        cfg.scan.npc_stations_only = false;
    }
    if let Some(min_p) = args.min_profit {
        cfg.filters.min_profit_isk = min_p;
    }
    if let Some(min_s) = args.min_spread {
        cfg.filters.min_spread_pct = min_s;
    }
    if let Some(min_r) = args.min_roi {
        if min_r < 10.0 {
            eprintln!(
                " ⚠ Warning: Minimum net ROI cannot be lower than 10.0% (enforced domain invariant). Clamping to 10.0%."
            );
            cfg.filters.min_net_profit_pct = 10.0;
        } else {
            cfg.filters.min_net_profit_pct = min_r;
        }
    } else if cfg.filters.min_net_profit_pct < 10.0 {
        cfg.filters.min_net_profit_pct = 10.0;
    }
    if let Some(parsed_regions) = args.parse_regions()? {
        cfg.scan.regions = parsed_regions;
    }
    if args.auto_refresh {
        cfg.scan.auto_refresh = true;
    }

    let ship_profile = cfg.ship_profile()?;

    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║                 EVE TRAVELING MERCHANT                        ║");
    println!("║       Arbitrage Engine & Multi-Stop Route Planner             ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    println!(
        " Selected Ship: {} (Max: {:.0} m³ │ Budget: {:.1} M ISK)",
        ship_profile.name,
        ship_profile.max_volume,
        ship_profile.max_budget / 1_000_000.0
    );
    println!(" Starting System: {}", cfg.location.home_system);
    println!(
        " Safe Highsec Routing: {}",
        if cfg.scan.safe_route_only {
            "Yes (Highsec Space Only)"
        } else {
            "No (All space)"
        }
    );
    println!(
        " Station Access Pool: {}",
        if cfg.scan.npc_stations_only {
            "NPC Stations Only (Preferred)"
        } else {
            "Include Player Citadels / Upwell Structures"
        }
    );
    println!(
        " Minimum Profitability After Taxes: {:.1}%",
        cfg.filters.min_net_profit_pct
    );
    println!();

    let is_desktop_os = cfg!(target_os = "windows") || cfg!(target_os = "macos");
    let has_linux_display =
        std::env::var("WAYLAND_DISPLAY").is_ok() || std::env::var("DISPLAY").is_ok();
    let has_display = is_desktop_os || has_linux_display;

    // GUI is the primary, default interface across all platforms.
    // Terminal and TUI modes are deprecated.
    let launch_gui = args.should_launch_gui(has_display);

    if !launch_gui && args.export.is_none() {
        if !has_display && !args.tui && !args.no_tui && !args.no_ui {
            eprintln!(
                " ⚠ NOTICE: No graphical display detected (neither WAYLAND_DISPLAY nor DISPLAY is set)."
            );
            eprintln!("   Falling back to deprecated terminal output.");
        }
        print_terminal_deprecation_notice();
    }

    let sde_path = resolve_sde_path();
    let cache_dir = resolve_cache_dir(&cfg.cache.directory);

    if launch_gui {
        println!(" Launching EVE Traveling Merchant Native Desktop GUI Window (Instant Startup & Splash Screen)...");
        let (startup_tx, startup_rx) = std::sync::mpsc::channel();
        let is_big = ship_profile.name == "Big Hauler";

        let sde_path_clone = sde_path.clone();
        let cache_dir_clone = cache_dir.clone();
        let home_system_clone = cfg.location.home_system.clone();
        let update_sde = args.update_sde;
        let safe_only = cfg.scan.safe_route_only;
        let npc_only = cfg.scan.npc_stations_only;
        let min_profit_isk = cfg.filters.min_profit_isk;
        let min_spread_pct = cfg.filters.min_spread_pct;
        let min_net_profit_pct = cfg.filters.min_net_profit_pct;
        let broker_fee_pct = cfg.fees.broker_fee_pct;
        let sales_tax_pct = cfg.fees.sales_tax_pct;
        let scan_adjacent = args.adjacent_regions && cfg.scan.scan_adjacent_regions;
        let explicit_regions = args.regions.clone();
        let max_stops = args.max_stops;
        let sde_max_age_days = cfg.cache.sde_max_age_days;

        tokio::spawn(async move {
            let _ = startup_tx.send(ui::StartupMsg::Progress(ui::StartupProgress {
                stage_index: 1,
                total_stages: 5,
                stage_name: "Static Universe Data (SDE)".to_string(),
                detail: "Opening EVE Static Data Export (SDE) SQLite universe maps...".to_string(),
                percent: 0.15,
                completed: false,
                error: None,
            }));

            if update_sde && sde_path_clone.exists() {
                let _ = std::fs::remove_file(&sde_path_clone);
            }

            let sde = match SdeLoader::open(&sde_path_clone, sde_max_age_days).await {
                Ok(s) => Arc::new(s),
                Err(e) => {
                    tracing::error!(
                        target: "eve_trade_scout",
                        error_code = %eve_trade_scout::error::ErrorCode::ErrSdeNotFound,
                        error = %e,
                        "Failed to load SDE database during startup: {e}"
                    );
                    let _ = startup_tx.send(ui::StartupMsg::Error(format!(
                        "Failed to load SDE database: {}",
                        e
                    )));
                    return;
                }
            };

            let home_system_id = sde.get_system_id(&home_system_clone).unwrap_or(30000142);
            let home_system_name = sde
                .get_system_name(home_system_id)
                .unwrap_or_else(|| home_system_clone.clone());

            let _ = startup_tx.send(ui::StartupMsg::Progress(ui::StartupProgress {
                stage_index: 2,
                total_stages: 5,
                stage_name: "Stargate Topology Network".to_string(),
                detail: "Compiling New Eden stargate graph (8,490 solar systems)...".to_string(),
                percent: 0.35,
                completed: false,
                error: None,
            }));

            let graph = match SystemGraph::from_sde(&sde) {
                Ok(g) => Arc::new(g),
                Err(e) => {
                    tracing::error!(
                        target: "eve_trade_scout",
                        error_code = %eve_trade_scout::error::ErrorCode::ErrGraphBuildFailed,
                        error = %e,
                        "Failed to construct stargate network during startup: {e}"
                    );
                    let _ = startup_tx.send(ui::StartupMsg::Error(format!(
                        "Failed to construct stargate network: {}",
                        e
                    )));
                    return;
                }
            };

            let scan_regions = if let Some(ref reg_str) = explicit_regions {
                reg_str
                    .split(',')
                    .filter_map(|s| s.trim().parse::<u32>().ok())
                    .collect()
            } else {
                let mut reg_set = std::collections::HashSet::new();
                if let Some(home_reg) = sde.get_system_region(home_system_id) {
                    reg_set.insert(home_reg);
                    if scan_adjacent {
                        for adj in graph.adjacent_regions(home_reg, safe_only) {
                            reg_set.insert(adj);
                        }
                    }
                }
                if reg_set.is_empty() {
                    reg_set.insert(10000002);
                }
                let mut list: Vec<u32> = reg_set.into_iter().collect();
                list.sort_unstable();
                list
            };

            let _ = startup_tx.send(ui::StartupMsg::Progress(ui::StartupProgress {
                stage_index: 3,
                total_stages: 5,
                stage_name: "Live Regional Order Books".to_string(),
                detail: format!(
                    "Connecting to CCP ESI: fetching order books for {} region(s)...",
                    scan_regions.len()
                ),
                percent: 0.55,
                completed: false,
                error: None,
            }));

            let esi = match EsiClient::new(&cache_dir_clone) {
                Ok(c) => Arc::new(c),
                Err(e) => {
                    tracing::error!(
                        target: "eve_trade_scout",
                        error_code = %eve_trade_scout::error::ErrorCode::ErrEsiClientInit,
                        error = %e,
                        "Failed to initialize ESI API client during startup: {e}"
                    );
                    let _ = startup_tx.send(ui::StartupMsg::Error(format!(
                        "Failed to initialize ESI API client: {}",
                        e
                    )));
                    return;
                }
            };

            let markets = match fetch_markets_fresh(&esi, &scan_regions).await {
                Ok(m) => m,
                Err(e) => {
                    tracing::error!(
                        target: "eve_trade_scout",
                        error_code = %eve_trade_scout::error::ErrorCode::ErrEsiNetworkFailed,
                        error = %e,
                        "Failed to fetch initial ESI regional market data during startup: {e}"
                    );
                    let _ = startup_tx.send(ui::StartupMsg::Error(format!(
                        "Failed to fetch ESI market data: {}",
                        e
                    )));
                    return;
                }
            };
            let _ = esi.save_cache().await;

            let total_sells: usize = markets
                .values()
                .map(|m| m.sell_orders.values().map(|v| v.len()).sum::<usize>())
                .sum();
            let total_buys: usize = markets
                .values()
                .map(|m| m.buy_orders.values().map(|v| v.len()).sum::<usize>())
                .sum();

            let _ = startup_tx.send(ui::StartupMsg::Progress(ui::StartupProgress {
                stage_index: 4,
                total_stages: 5,
                stage_name: "Cross-Station Arbitrage Solver".to_string(),
                detail: format!("Parsed {} sell & {} buy orders. Calculating cross-station deals ({:.0}% ROI)...", total_sells, total_buys, min_net_profit_pct),
                percent: 0.80,
                completed: false,
                error: None,
            }));

            let arb_cfg = ArbitrageConfig {
                ship: ship_profile,
                home_system: home_system_id,
                min_profit: min_profit_isk,
                min_spread: min_spread_pct,
                min_net_profit_pct,
                broker_fee: broker_fee_pct,
                sales_tax: sales_tax_pct,
                safe_only,
                npc_stations_only: npc_only,
            };

            let deals = find_deals(&markets, &sde, &graph, &arb_cfg);

            let _ = startup_tx.send(ui::StartupMsg::Progress(ui::StartupProgress {
                stage_index: 5,
                total_stages: 5,
                stage_name: "Corridor Manifests & Route Planning".to_string(),
                detail: format!(
                    "Found {} profitable trade deals! Consolidating corridors & planning route...",
                    deals.len()
                ),
                percent: 0.95,
                completed: false,
                error: None,
            }));

            let manifests = bundle_route_cargo(&deals, ship_profile);
            let route = plan_route(
                &deals,
                ship_profile,
                home_system_id,
                &graph,
                safe_only,
                max_stops,
            );

            let _ = startup_tx.send(ui::StartupMsg::Ready {
                sde,
                graph,
                esi,
                home_system_name,
                scan_regions,
                deals,
                manifests,
                route: Some(route),
            });
        });

        let app = ui::DesktopApp::new_loading(
            is_big,
            cfg.location.home_system.clone(),
            cfg.scan.safe_route_only,
            cfg.scan.npc_stations_only,
            cfg.filters.min_net_profit_pct,
            cfg.filters.min_profit_isk,
            cfg.scan.auto_refresh,
            cfg.scan.sound_alerts,
            cfg.scan.sound_min_roi_pct,
            startup_rx,
        );

        if let Err(e) = ui::run_desktop_gui(app) {
            tracing::error!(
                target: "eve_trade_scout",
                error_code = %eve_trade_scout::error::ErrorCode::ErrUiRuntimeInit,
                error = %e,
                "Desktop GUI runtime error: {e}"
            );
            return Err(anyhow::anyhow!("Desktop GUI runtime error: {}", e));
        }
        return Ok(());
    }

    // 1. Initialize SDE (Terminal / CLI / Headless mode)
    if args.update_sde && sde_path.exists() {
        let _ = std::fs::remove_file(&sde_path);
    }

    let sde = SdeLoader::open(&sde_path, cfg.cache.sde_max_age_days)
        .await
        .context("Failed to load EVE Static Data Export (SDE)")?;
    let sde = Arc::new(sde);

    // 2. Resolve Starting System ID
    let home_system_id = match sde.get_system_id(&cfg.location.home_system) {
        Some(id) => id,
        None => {
            bail!(
                "Could not find starting solar system '{}' in the EVE universe. Please check spelling.",
                cfg.location.home_system
            );
        }
    };

    let home_system_name = sde
        .get_system_name(home_system_id)
        .unwrap_or_else(|| cfg.location.home_system.clone());

    println!(" Building New Eden Stargate Network Graph...");
    let graph = SystemGraph::from_sde(&sde).context("Failed to construct stargate network")?;
    let graph = Arc::new(graph);
    println!(" Stargate Graph ready.");

    if !graph.is_highsec(home_system_id) {
        println!(
            " ⚠ Notice: Starting system '{}' is Lowsec/Nullsec (sec < 0.45). Only Highsec trades will be scanned.",
            home_system_name
        );
    }

    // 3. Resolve Scan Regions (Home Region + stargate-connected adjacent neighbor regions)
    let scan_regions = if let Some(ref reg_str) = args.regions {
        reg_str
            .split(',')
            .filter_map(|s| s.trim().parse::<u32>().ok())
            .collect()
    } else {
        let mut reg_set = std::collections::HashSet::new();
        if let Some(home_reg) = sde.get_system_region(home_system_id) {
            reg_set.insert(home_reg);
            if args.adjacent_regions && cfg.scan.scan_adjacent_regions {
                for adj in graph.adjacent_regions(home_reg, cfg.scan.safe_route_only) {
                    reg_set.insert(adj);
                }
            }
        } else {
            for &r in &cfg.scan.regions {
                reg_set.insert(r);
            }
        }
        if reg_set.is_empty() {
            reg_set.insert(10000002);
        }
        let mut list: Vec<u32> = reg_set.into_iter().collect();
        list.sort_unstable();
        list
    };

    // 4. Initialize ESI Client & Fetch Market Data
    let cache_dir = resolve_cache_dir(&cfg.cache.directory);
    let esi = EsiClient::new(&cache_dir).context("Failed to initialize ESI API client")?;
    let esi = Arc::new(esi);

    println!(
        " Fetching live market orders from ESI for {} region(s)...",
        scan_regions.len()
    );
    let markets = fetch_markets_fresh(&esi, &scan_regions)
        .await
        .context("Failed to fetch market data from EVE ESI")?;
    let _ = esi.save_cache().await;

    let total_sells: usize = markets
        .values()
        .map(|m| m.sell_orders.values().map(|v| v.len()).sum::<usize>())
        .sum();
    let total_buys: usize = markets
        .values()
        .map(|m| m.buy_orders.values().map(|v| v.len()).sum::<usize>())
        .sum();
    println!(
        " Retrieved {} sell orders and {} buy orders.",
        total_sells, total_buys
    );

    // 5. Calculate Arbitrage Deals
    println!(" Calculating cross-station arbitrage opportunities & jump paths...");
    let arb_cfg = ArbitrageConfig {
        ship: ship_profile,
        home_system: home_system_id,
        min_profit: cfg.filters.min_profit_isk,
        min_spread: cfg.filters.min_spread_pct,
        min_net_profit_pct: cfg.filters.min_net_profit_pct,
        broker_fee: cfg.fees.broker_fee_pct,
        sales_tax: cfg.fees.sales_tax_pct,
        safe_only: cfg.scan.safe_route_only,
        npc_stations_only: cfg.scan.npc_stations_only,
    };

    let deals = find_deals(&markets, &sde, &graph, &arb_cfg);
    println!(" Found {} profitable trade deals!", deals.len());
    println!();

    let is_interactive =
        stdout().is_terminal() && !args.no_tui && !args.no_ui && args.export.is_none();

    if deals.is_empty() {
        println!(" No profitable deals met your current filters.");
        println!(" Suggestions:");
        println!(
            "   - Try lowering --min-profit (currently {:.0} ISK)",
            cfg.filters.min_profit_isk
        );
        println!(
            "   - Try lowering --min-spread (currently {:.1}%)",
            cfg.filters.min_spread_pct
        );
        println!("   - Add more scanning regions with --regions");
        return Ok(());
    }

    // 6. Plan Multi-Stop Route
    let route = plan_route(
        &deals,
        ship_profile,
        home_system_id,
        &graph,
        cfg.scan.safe_route_only,
        args.max_stops,
    );

    // 7. Optional CSV Export
    if let Some(ref csv_path) = args.export {
        export_to_csv(&deals, csv_path)?;
        println!(" Exported {} deals to {:?}", deals.len(), csv_path);
    }

    // 8. Output Mode (Terminal / CLI)
    if is_interactive {
        println!(" Launching Interactive Terminal UI (press [Q] to exit)...");
        #[allow(deprecated)]
        ui::run_tui(
            &deals,
            &route,
            ship_profile,
            &home_system_name,
            cfg.scan.safe_route_only,
            cfg.scan.npc_stations_only,
        )?;
    } else {
        print_cli_summary(&deals, &route, ship_profile, args.top, args.route);
    }

    Ok(())
}

use eve_trade_scout::ui::format_unit_price;

fn print_cli_summary(
    deals: &[ArbitrageDeal],
    route: &TradeRoute,
    ship: eve_trade_scout::config::ShipProfile,
    top_limit: usize,
    show_route: bool,
) {
    println!("══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════");
    println!(
        " TOP {} ARBITRAGE OPPORTUNITIES (Ranked by Profit / Jump)",
        top_limit.min(deals.len())
    );
    println!("══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════");
    println!(
        "{:<3} {:<24} {:<15} {:>10} {:<15} {:>10} {:>6} {:>7} {:>9} {:>10} {:>7} {:>9} {:>12}",
        "#",
        "Item",
        "Buy System",
        "Buy Price",
        "Sell System",
        "Sell Price",
        "Qty",
        "Vol m³",
        "Cost (M)",
        "Profit (M)",
        "ROI %",
        "Jumps",
        "Profit/Jump"
    );
    println!("──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────");

    for (idx, d) in deals.iter().take(top_limit).enumerate() {
        let jumps_str = format!(
            "{}+{}={}",
            d.jumps_from_home, d.jumps_source_dest, d.total_jumps
        );
        let name_display = if d.is_high_roi() {
            format!("🔥 {}", d.item_name)
        } else {
            d.item_name.clone()
        };
        let name_truncated = if name_display.len() > 23 {
            format!("{}…", &name_display[..22])
        } else {
            name_display
        };
        let src_str = format!("[{:.1}] {}", d.source_security, d.source_system_name);
        let dst_str = format!("[{:.1}] {}", d.dest_security, d.dest_system_name);

        println!(
            "{:<3} {:<24} {:<15} {:>10} {:<15} {:>10} {:>6} {:>7.1} {:>8.2}M {:>9.2}M {:>6.1}% {:>9} {:>10.2}M",
            idx + 1,
            name_truncated,
            src_str,
            format_unit_price(d.buy_price),
            dst_str,
            format_unit_price(d.sell_price),
            d.quantity,
            d.volume_used,
            d.capital_required / 1_000_000.0,
            d.net_profit / 1_000_000.0,
            d.net_profit_pct,
            jumps_str,
            d.profit_per_jump / 1_000_000.0,
        );
    }
    println!("──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────");
    println!();

    // Packed Cargo Corridor Manifests
    let manifests = bundle_route_cargo(deals, ship);
    if !manifests.is_empty() {
        println!("══════════════════════════════════════════════════════════════════════════════════════════════════════════════════");
        println!(" PACKED TRADE CORRIDOR MANIFESTS (Multi-Item Knapsack Cargo Consolidation)");
        println!("══════════════════════════════════════════════════════════════════════════════════════════════════════════════════");
        for (i, m) in manifests.iter().take(5).enumerate() {
            let vol_pct = (m.total_volume_used / ship.max_volume) * 100.0;
            let budget_pct = (m.total_capital_required / ship.max_budget) * 100.0;
            println!(
                " [MANIFEST #{}] [{:.1}] {} -> [{:.1}] {} ({} jumps) │ +{:.2}M ISK Net Profit (+{:.1}% ROI) │ {:.2}M/jump",
                i + 1,
                m.source_security,
                m.source_system_name,
                m.dest_security,
                m.dest_system_name,
                m.jumps,
                m.total_net_profit / 1_000_000.0,
                m.overall_roi_pct,
                m.profit_per_jump / 1_000_000.0,
            );
            println!(
                "   Cargo: {:.1} / {:.0} m³ ({:.1}%) │ Budget: {:.2}M / {:.0}M ISK ({:.1}%) │ {} packed items",
                m.total_volume_used,
                ship.max_volume,
                vol_pct,
                m.total_capital_required / 1_000_000.0,
                ship.max_budget / 1_000_000.0,
                budget_pct,
                m.deals.len(),
            );
            for d in &m.deals {
                println!(
                    "     • {}x {} (Vol: {:.1} m³, Cost: {:.2}M, Profit: +{:.2}M, +{:.1}%)",
                    d.quantity,
                    d.item_name,
                    d.volume_used,
                    d.capital_required / 1_000_000.0,
                    d.net_profit / 1_000_000.0,
                    d.net_profit_pct,
                );
            }
            println!();
        }
    }

    if show_route || !deals.is_empty() {
        let route_roi = if route.total_capital_required > 0.0 {
            (route.total_profit / route.total_capital_required) * 100.0
        } else {
            0.0
        };

        println!("══════════════════════════════════════════════════════════════════════════════════════════════════════════════");
        println!(
            " PLANNED MULTI-STOP TRADE ROUTE ({} Stops)",
            route.stops.len()
        );
        println!("══════════════════════════════════════════════════════════════════════════════════════════════════════════════");
        println!(
            " Total Jumps: {} │ Total Net Profit: {:.2} M ISK (+{:.1}% ROI) │ Average Profit/Jump: {:.2} M ISK/jump",
            route.total_jumps,
            route.total_profit / 1_000_000.0,
            route_roi,
            route.avg_profit_per_jump / 1_000_000.0
        );
        println!(
            " Capital Required: {:.2} M ISK │ Peak Cargo Space Used: {:.1} m³",
            route.total_capital_required / 1_000_000.0,
            route.max_volume_used
        );
        println!();

        for stop in &route.stops {
            println!(
                " [STOP {}] Travel {} jumps to {}",
                stop.step, stop.jumps_from_prev, stop.deal.source_system_name
            );
            println!(
                "   BUY:  {}x {} @ {:.2} ISK (Cost: {:.2}M ISK)",
                stop.deal.quantity,
                stop.deal.item_name,
                stop.deal.buy_price,
                stop.deal.capital_required / 1_000_000.0
            );
            println!("   AT:   {}", stop.deal.source_station_name);
            println!(
                "   WARP: {} jumps to {}",
                stop.deal.jumps_source_dest, stop.deal.dest_system_name
            );
            println!(
                "   SELL: {}x {} @ {:.2} ISK (Profit: +{:.2}M ISK | +{:.1}% ROI)",
                stop.deal.quantity,
                stop.deal.item_name,
                stop.deal.sell_price,
                stop.deal.net_profit / 1_000_000.0,
                stop.deal.net_profit_pct
            );
            println!("   AT:   {}", stop.deal.dest_station_name);
            println!();
        }

        println!(" Autopilot Waypoints Trail:");
        println!("   {}", route.waypoint_names.join(" -> "));
        println!("══════════════════════════════════════════════════════════════════════════════════════════════════════════════");
    }
}

fn export_to_csv(deals: &[ArbitrageDeal], path: &Path) -> Result<()> {
    // Security check: Refuse to export through symlinks to prevent symlink traversal attacks
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            bail!(
                "Refusing to export to a symlink destination for security reasons: {:?}",
                path
            );
        }
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create parent export directory: {:?}", parent)
            })?;
        }
    }
    let mut file = File::create(path)?;
    writeln!(
        file,
        "type_id,item_name,buy_price,sell_price,spread_pct,net_profit_pct,source_system,source_station,dest_system,dest_station,quantity,volume_m3,capital_required_isk,net_profit_isk,jumps_from_home,jumps_source_dest,total_jumps,profit_per_jump_isk"
    )?;

    for d in deals {
        let safe_name = sanitize_csv_cell(&d.item_name);
        let safe_src_sys = sanitize_csv_cell(&d.source_system_name);
        let safe_src_sta = sanitize_csv_cell(&d.source_station_name);
        let safe_dst_sys = sanitize_csv_cell(&d.dest_system_name);
        let safe_dst_sta = sanitize_csv_cell(&d.dest_station_name);

        writeln!(
            file,
            "{},\"{}\",{:.2},{:.2},{:.2},{:.2},\"{}\",\"{}\",\"{}\",\"{}\",{},{:.2},{:.2},{:.2},{},{},{},{:.2}",
            d.type_id,
            safe_name,
            d.buy_price,
            d.sell_price,
            d.spread_pct,
            d.net_profit_pct,
            safe_src_sys,
            safe_src_sta,
            safe_dst_sys,
            safe_dst_sta,
            d.quantity,
            d.volume_used,
            d.capital_required,
            d.net_profit,
            d.jumps_from_home,
            d.jumps_source_dest,
            d.total_jumps,
            d.profit_per_jump
        )?;
    }
    Ok(())
}

fn resolve_sde_path() -> PathBuf {
    let local = PathBuf::from("data").join("eve_sde.sqlite");
    if local.exists() {
        return local;
    }
    if let Some(data_dir) = dirs::data_dir() {
        let app_dir = data_dir.join("eve-traveling-merchant");
        let _ = std::fs::create_dir_all(&app_dir);
        return app_dir.join("eve_sde.sqlite");
    }
    local
}

fn resolve_cache_dir(cfg_dir: &str) -> PathBuf {
    let local = PathBuf::from(cfg_dir);
    if local.exists() {
        return local;
    }
    if let Some(cache_dir) = dirs::cache_dir() {
        let app_dir = cache_dir.join("eve-traveling-merchant");
        let _ = std::fs::create_dir_all(&app_dir);
        return app_dir;
    }
    local
}

fn resolve_log_dir() -> PathBuf {
    let local = PathBuf::from("logs");
    if local.is_dir() {
        return local;
    }
    if let Some(data_local) = dirs::data_local_dir() {
        let app_dir = data_local.join("eve-traveling-merchant").join("logs");
        if std::fs::create_dir_all(&app_dir).is_ok() {
            return app_dir;
        }
    }
    local
}
