#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use eve_trade_scout::arbitrage::{bundle_route_cargo, find_deals, ArbitrageConfig};
use eve_trade_scout::config::UserConfig;
use eve_trade_scout::esi::{count_market_orders, fetch_markets_fresh, EsiClient};
use eve_trade_scout::graph::SystemGraph;
use eve_trade_scout::route_planner::plan_route;
use eve_trade_scout::sde::SdeLoader;
use eve_trade_scout::ui;
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
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

    let result = run_app().await;
    if let Err(ref e) = result {
        tracing::error!(
            target: "eve_trade_scout",
            error_code = %eve_trade_scout::error::ErrorCode::ErrUiStartupFailed,
            error = %e,
            "Fatal application startup/runtime failure: {e}"
        );
        eprintln!("[FATAL] eve-merchant startup/runtime failure: {e}");
    }
    result
}

async fn run_app() -> Result<()> {
    let cfg = match UserConfig::load() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                target: "eve_trade_scout",
                error_code = %eve_trade_scout::error::ErrorCode::ErrConfigLoadFailed,
                error = %e,
                "Failed to load user config file; falling back to default configuration"
            );
            UserConfig::default()
        }
    };

    let ship_profile = match cfg.effective_ship_profile() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(
                target: "eve_trade_scout",
                error_code = %eve_trade_scout::error::ErrorCode::ErrConfigLoadFailed,
                error = %e,
                "Invalid ship profile in config; defaulting to Big Hauler"
            );
            eve_trade_scout::config::BIG_HAULER
        }
    };

    let sde_path = resolve_sde_path();
    let cache_dir = resolve_cache_dir(&cfg.cache.directory);

    let (startup_tx, startup_rx) = std::sync::mpsc::channel();
    let startup_tx_supervisor = startup_tx.clone();

    let sde_path_clone = sde_path.clone();
    let cache_dir_clone = cache_dir.clone();
    let home_system_clone = cfg.location.home_system.clone();
    let safe_only = cfg.scan.safe_route_only;
    let npc_only = cfg.scan.npc_stations_only;
    let min_profit_isk = cfg.filters.min_profit_isk;
    let min_spread_pct = cfg.filters.min_spread_pct;
    let min_net_profit_pct = cfg.filters.min_net_profit_pct;
    let broker_fee_pct = cfg.fees.broker_fee_pct;
    let sales_tax_pct = cfg.fees.sales_tax_pct;
    let scan_adjacent = cfg.scan.scan_adjacent_regions;
    let sde_max_age_days = cfg.cache.sde_max_age_days;
    let default_regions = cfg.scan.regions.clone();

    let worker_handle = tokio::spawn(async move {
        let _ = startup_tx.send(ui::StartupMsg::Progress(ui::StartupProgress {
            stage_index: 1,
            total_stages: 5,
            stage_name: "Static Universe Data (SDE)".to_string(),
            detail: "Opening EVE Static Data Export (SDE) SQLite universe maps...".to_string(),
            percent: 0.15,
            completed: false,
            error: None,
            error_code: None,
        }));

        let sde = match SdeLoader::open(&sde_path_clone, sde_max_age_days).await {
            Ok(s) => Arc::new(s),
            Err(e) => {
                tracing::error!(
                    target: "eve_trade_scout",
                    error_code = %eve_trade_scout::error::ErrorCode::ErrSdeNotFound,
                    error = %e,
                    "Failed to load SDE database during startup: {e}"
                );
                let _ = startup_tx.send(ui::StartupMsg::error(
                    eve_trade_scout::error::ErrorCode::ErrSdeNotFound,
                    format!("Failed to load SDE database: {}", e),
                ));
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
            error_code: None,
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
                let _ = startup_tx.send(ui::StartupMsg::error(
                    eve_trade_scout::error::ErrorCode::ErrGraphBuildFailed,
                    format!("Failed to construct stargate network: {}", e),
                ));
                return;
            }
        };

        let scan_regions = {
            let mut reg_set = std::collections::HashSet::new();
            if let Some(home_reg) = sde.get_system_region(home_system_id) {
                reg_set.insert(home_reg);
                if scan_adjacent {
                    for adj in graph.adjacent_regions(home_reg, safe_only) {
                        reg_set.insert(adj);
                    }
                }
            } else {
                for &r in &default_regions {
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
            error_code: None,
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
                let _ = startup_tx.send(ui::StartupMsg::error(
                    eve_trade_scout::error::ErrorCode::ErrEsiClientInit,
                    format!("Failed to initialize ESI API client: {}", e),
                ));
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
                let _ = startup_tx.send(ui::StartupMsg::error(
                    eve_trade_scout::error::ErrorCode::ErrEsiNetworkFailed,
                    format!("Failed to fetch ESI market data: {}", e),
                ));
                return;
            }
        };
        let _ = esi.save_cache().await;

        let (total_sells, total_buys) = count_market_orders(&markets);

        let _ = startup_tx.send(ui::StartupMsg::Progress(ui::StartupProgress {
            stage_index: 4,
            total_stages: 5,
            stage_name: "Cross-Station Arbitrage Solver".to_string(),
            detail: format!(
                "Parsed {} sell & {} buy orders. Calculating cross-station deals ({:.0}% ROI)...",
                total_sells, total_buys, min_net_profit_pct
            ),
            percent: 0.80,
            completed: false,
            error: None,
            error_code: None,
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
            error_code: None,
        }));

        let manifests = bundle_route_cargo(&deals, ship_profile);
        let route = plan_route(&deals, ship_profile, home_system_id, &graph, safe_only, 5);

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

    let supervisor_tx = startup_tx_supervisor;
    tokio::spawn(async move {
        if let Err(join_err) = worker_handle.await {
            if join_err.is_panic() {
                tracing::error!(
                    target: "eve_trade_scout",
                    error_code = %eve_trade_scout::error::ErrorCode::ErrSysPanic,
                    error = %join_err,
                    "FATAL: Background startup worker panicked unexpectedly"
                );
                let _ = supervisor_tx.send(ui::StartupMsg::error(
                    eve_trade_scout::error::ErrorCode::ErrSysPanic,
                    format!("Startup worker panicked unexpectedly: {join_err}. Check log file for backtrace."),
                ));
            } else if join_err.is_cancelled() {
                tracing::error!(
                    target: "eve_trade_scout",
                    error_code = %eve_trade_scout::error::ErrorCode::ErrUiStartupFailed,
                    error = %join_err,
                    "Startup worker task was cancelled prematurely"
                );
                let _ = supervisor_tx.send(ui::StartupMsg::error(
                    eve_trade_scout::error::ErrorCode::ErrUiStartupFailed,
                    "Startup worker task was cancelled prematurely.".to_string(),
                ));
            }
        }
    });

    let app = ui::DesktopApp::new_loading(&cfg, startup_rx);

    if let Err(e) = ui::run_desktop_gui(app) {
        tracing::error!(
            target: "eve_trade_scout",
            error_code = %eve_trade_scout::error::ErrorCode::ErrUiRuntimeInit,
            error = %e,
            "Desktop GUI runtime error: {e}"
        );
        return Err(anyhow::anyhow!("Desktop GUI runtime error: {}", e));
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
