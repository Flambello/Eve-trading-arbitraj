mod arbitrage;
mod config;
mod esi;
mod graph;
mod route_planner;
mod sde;
mod ui;

use anyhow::{bail, Context, Result};
use arbitrage::{find_deals, ArbitrageConfig, ArbitrageDeal};
use clap::Parser;
use config::{profile_from_str, UserConfig};
use esi::{fetch_markets, EsiClient};
use graph::SystemGraph;
use route_planner::{plan_route, TradeRoute};
use sde::SdeLoader;
use std::fs::File;
use std::io::{stdout, IsTerminal, Write};
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(
    name = "eve-trade",
    author = "EVE Online Arbitrage Scout",
    version = "0.1.0",
    about = "Finds the closest and most profitable cross-station arbitrage deals and plans trade routes in EVE Online"
)]
struct Args {
    /// Ship profile to use: 'big' (60k m³, 500M ISK) or 'small' (30k m³, 50M ISK)
    #[arg(short, long)]
    ship: Option<String>,

    /// Starting solar system name (e.g. Jita, Amarr, Dodixie, Rens)
    #[arg(long)]
    system: Option<String>,

    /// Comma-separated region IDs to scan (e.g. 10000002 for The Forge)
    #[arg(long)]
    regions: Option<String>,

    /// Force highsec safe route only (avoid lowsec/nullsec)
    #[arg(long)]
    safe: bool,

    /// Allow lowsec and nullsec routes
    #[arg(long)]
    unsafe_routes: bool,

    /// Minimum net profit filter in ISK
    #[arg(long)]
    min_profit: Option<f64>,

    /// Minimum price spread percentage filter
    #[arg(long)]
    min_spread: Option<f64>,

    /// Minimum overall net profitability (ROI %) after all taxes (enforced default: 15.0%)
    #[arg(long)]
    min_roi: Option<f64>,

    /// Number of top deals to display in CLI mode
    #[arg(short, long, default_value_t = 25)]
    top: usize,

    /// Maximum number of stops in the planned route
    #[arg(long, default_value_t = 5)]
    max_stops: usize,

    /// Print planned route in CLI mode
    #[arg(long)]
    route: bool,

    /// Disable interactive TUI and output directly to console
    #[arg(long)]
    no_tui: bool,

    /// Export found deals to a CSV file
    #[arg(long)]
    export: Option<PathBuf>,

    /// Force update / re-download of the SDE database
    #[arg(long)]
    update_sde: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
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
    if let Some(min_p) = args.min_profit {
        cfg.filters.min_profit_isk = min_p;
    }
    if let Some(min_s) = args.min_spread {
        cfg.filters.min_spread_pct = min_s;
    }
    if let Some(min_r) = args.min_roi {
        cfg.filters.min_net_profit_pct = min_r;
    }
    if let Some(ref reg_str) = args.regions {
        let parsed_regions: Vec<u32> = reg_str
            .split(',')
            .filter_map(|s| s.trim().parse::<u32>().ok())
            .collect();
        if !parsed_regions.is_empty() {
            cfg.scan.regions = parsed_regions;
        }
    }

    let ship_profile = profile_from_str(&cfg.ship.profile)?;

    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║                   EVE ONLINE TRADE SCOUT                      ║");
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
            "Yes (Avoid Lowsec/Nullsec)"
        } else {
            "No (All space)"
        }
    );
    println!(
        " Minimum Profitability After Taxes: {:.1}%",
        cfg.filters.min_net_profit_pct
    );
    println!();

    // 1. Initialize SDE
    let sde_path = PathBuf::from("data").join("eve_sde.sqlite");
    if args.update_sde && sde_path.exists() {
        let _ = std::fs::remove_file(&sde_path);
    }

    let sde = SdeLoader::open(&sde_path, cfg.cache.sde_max_age_days)
        .await
        .context("Failed to load EVE Static Data Export (SDE)")?;

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
    println!(" Stargate Graph ready.");

    // 3. Initialize ESI Client
    let cache_dir = PathBuf::from(&cfg.cache.directory);
    let esi = EsiClient::new(&cache_dir).context("Failed to initialize ESI API client")?;

    // 4. Fetch Market Data
    println!(
        " Fetching live market orders from ESI for {} region(s)...",
        cfg.scan.regions.len()
    );
    let markets = fetch_markets(&esi, &cfg.scan.regions)
        .await
        .context("Failed to fetch market data from EVE ESI")?;
    let _ = esi.save_cache().await;

    let total_sells: usize = markets.values().map(|m| m.sell_orders.values().map(|v| v.len()).sum::<usize>()).sum();
    let total_buys: usize = markets.values().map(|m| m.buy_orders.values().map(|v| v.len()).sum::<usize>()).sum();
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
        npc_stations_only: true,
    };

    let deals = find_deals(&markets, &sde, &graph, &arb_cfg);
    println!(" Found {} profitable trade deals!", deals.len());
    println!();

    if deals.is_empty() {
        println!(" No profitable deals met your current filters.");
        println!(" Suggestions:");
        println!("   - Try lowering --min-profit (currently {:.0} ISK)", cfg.filters.min_profit_isk);
        println!("   - Try lowering --min-spread (currently {:.1}%)", cfg.filters.min_spread_pct);
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

    // 8. Output Mode
    let is_interactive = stdout().is_terminal() && !args.no_tui && args.export.is_none();

    if is_interactive {
        println!(" Launching Interactive Terminal UI (press [Q] to exit)...");
        ui::run_tui(
            &deals,
            &route,
            ship_profile,
            &home_system_name,
            cfg.scan.safe_route_only,
        )?;
    } else {
        print_cli_summary(&deals, &route, args.top, args.route);
    }

    Ok(())
}

fn print_cli_summary(
    deals: &[ArbitrageDeal],
    route: &TradeRoute,
    top_limit: usize,
    show_route: bool,
) {
    println!("══════════════════════════════════════════════════════════════════════════════════════════════════════════════");
    println!(" TOP {} ARBITRAGE OPPORTUNITIES (Ranked by Profit / Jump)", top_limit.min(deals.len()));
    println!("══════════════════════════════════════════════════════════════════════════════════════════════════════════════");
    println!(
        "{:<3} {:<22} {:<13} {:<13} {:>6} {:>7} {:>9} {:>10} {:>7} {:>9} {:>12}",
        "#", "Item", "Buy System", "Sell System", "Qty", "Vol m³", "Cost (M)", "Profit (M)", "ROI %", "Jumps", "Profit/Jump"
    );
    println!("─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────");

    for (idx, d) in deals.iter().take(top_limit).enumerate() {
        let jumps_str = format!("{}+{}={}", d.jumps_from_home, d.jumps_source_dest, d.total_jumps);
        let name_truncated = if d.item_name.len() > 21 {
            format!("{}…", &d.item_name[..20])
        } else {
            d.item_name.clone()
        };

        println!(
            "{:<3} {:<22} {:<13} {:<13} {:>6} {:>7.1} {:>8.2}M {:>9.2}M {:>6.1}% {:>9} {:>10.2}M",
            idx + 1,
            name_truncated,
            d.source_system_name,
            d.dest_system_name,
            d.quantity,
            d.volume_used,
            d.capital_required / 1_000_000.0,
            d.net_profit / 1_000_000.0,
            d.net_profit_pct,
            jumps_str,
            d.profit_per_jump / 1_000_000.0,
        );
    }
    println!("─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────");
    println!();

    if show_route || deals.len() > 0 {
        let route_roi = if route.total_capital_required > 0.0 {
            (route.total_profit / route.total_capital_required) * 100.0
        } else {
            0.0
        };

        println!("══════════════════════════════════════════════════════════════════════════════════════════════════════════════");
        println!(" PLANNED MULTI-STOP TRADE ROUTE ({} Stops)", route.stops.len());
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
    let mut file = File::create(path)?;
    writeln!(
        file,
        "type_id,item_name,buy_price,sell_price,spread_pct,net_profit_pct,source_system,source_station,dest_system,dest_station,quantity,volume_m3,capital_required_isk,net_profit_isk,jumps_from_home,jumps_source_dest,total_jumps,profit_per_jump_isk"
    )?;

    for d in deals {
        // Escape quotes if needed
        let safe_name = d.item_name.replace('"', "\"\"");
        let safe_src_sta = d.source_station_name.replace('"', "\"\"");
        let safe_dst_sta = d.dest_station_name.replace('"', "\"\"");

        writeln!(
            file,
            "{},\"{}\",{:.2},{:.2},{:.2},{:.2},\"{}\",\"{}\",\"{}\",\"{}\",{},{:.2},{:.2},{:.2},{},{},{},{:.2}",
            d.type_id,
            safe_name,
            d.buy_price,
            d.sell_price,
            d.spread_pct,
            d.net_profit_pct,
            d.source_system_name,
            safe_src_sta,
            d.dest_system_name,
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
