#![allow(deprecated)]

use clap::{CommandFactory, Parser};
use eve_trade_scout::arbitrage::ArbitrageDeal;
use eve_trade_scout::config::BIG_HAULER;
use eve_trade_scout::route_planner::TradeRoute;
use eve_trade_scout::ui::tui::AppState;
use eve_trade_scout::Args;
use std::path::PathBuf;

#[test]
fn test_default_cli_args_and_gui_launch_logic() {
    let args = Args::parse_from(["eve-merchant"]);
    assert!(!args.gui);
    assert!(!args.tui);
    assert!(!args.no_ui);
    assert!(!args.no_tui);
    assert!(args.export.is_none());

    // When display is present (e.g. Windows desktop or Linux X11/Wayland), GUI launches by default
    assert!(
        args.should_launch_gui(true),
        "GUI should launch by default when a display environment is available"
    );

    // When no display is detected (headless Linux environment), GUI is not launched
    assert!(
        !args.should_launch_gui(false),
        "Headless environment without display should not attempt GUI launch"
    );
}

#[test]
fn test_explicit_gui_flag_forces_gui_even_if_no_display() {
    let args = Args::parse_from(["eve-merchant", "--gui"]);
    assert!(args.gui);
    assert!(args.should_launch_gui(true));
    assert!(
        args.should_launch_gui(false),
        "Explicit --gui flag must force GUI launch attempt"
    );
}

#[test]
fn test_deprecated_terminal_flags_suppress_gui() {
    // --tui (deprecated)
    let args_tui = Args::parse_from(["eve-merchant", "--tui"]);
    assert!(args_tui.tui);
    assert!(
        !args_tui.should_launch_gui(true),
        "--tui must suppress GUI even when display is available"
    );

    // --no-ui (deprecated)
    let args_no_ui = Args::parse_from(["eve-merchant", "--no-ui"]);
    assert!(args_no_ui.no_ui);
    assert!(
        !args_no_ui.should_launch_gui(true),
        "--no-ui must suppress GUI"
    );

    // --no-tui (deprecated)
    let args_no_tui = Args::parse_from(["eve-merchant", "--no-tui"]);
    assert!(args_no_tui.no_tui);
    assert!(
        !args_no_tui.should_launch_gui(true),
        "--no-tui must suppress GUI"
    );
}

#[test]
fn test_export_flag_suppresses_gui() {
    let args_export = Args::parse_from(["eve-merchant", "--export", "output.csv"]);
    assert_eq!(args_export.export, Some(PathBuf::from("output.csv")));
    assert!(
        !args_export.should_launch_gui(true),
        "--export flag must bypass GUI and output CSV"
    );
}

#[test]
fn test_cli_help_contains_deprecation_notices() {
    let mut cmd = Args::command();
    let mut help_buf = Vec::new();
    cmd.write_long_help(&mut help_buf).unwrap();
    let help_text = String::from_utf8(help_buf).expect("Help text is valid UTF-8");

    // Verify deprecated options are explicitly marked in CLI help
    assert!(
        help_text.contains("[DEPRECATED] Force terminal TUI mode"),
        "Help text must mark --tui as [DEPRECATED]"
    );
    assert!(
        help_text.contains("[DEPRECATED] Disable interactive UI"),
        "Help text must mark --no-ui as [DEPRECATED]"
    );
    assert!(
        help_text.contains("[DEPRECATED] Disable interactive TUI"),
        "Help text must mark --no-tui as [DEPRECATED]"
    );

    // Verify GUI option is marked as default
    assert!(
        help_text.contains("Force desktop GUI window mode (default on all platforms)"),
        "Help text must describe --gui as default on all platforms"
    );
}

fn make_test_deal(type_id: u32, name: &str) -> ArbitrageDeal {
    ArbitrageDeal {
        type_id,
        item_name: name.to_string(),
        source_order_id: 100 + type_id as u64,
        dest_order_id: 200 + type_id as u64,
        source_order_volume_remain: 100,
        dest_order_volume_remain: 100,
        dest_order_min_volume: 1,
        is_npc_trade: false,
        buy_price: 100.0,
        sell_price: 150.0,
        spread_pct: 50.0,
        source_station_id: 60003760,
        source_system_id: 30000142,
        source_system_name: "Jita".to_string(),
        source_station_name: "Jita IV-4".to_string(),
        source_security: 0.95,
        dest_station_id: 60008494,
        dest_system_id: 30002187,
        dest_system_name: "Amarr".to_string(),
        dest_station_name: "Amarr VIII-4".to_string(),
        dest_security: 0.90,
        quantity: 10,
        volume_used: 10.0,
        capital_required: 1000.0,
        gross_profit: 500.0,
        taxes_and_fees: 50.0,
        net_profit: 450.0,
        net_profit_pct: 45.0,
        jumps_from_home: 0,
        jumps_source_dest: 4,
        total_jumps: 4,
        profit_per_jump: 112.5,
        profit_per_m3: 45.0,
    }
}

#[test]
#[allow(deprecated)]
fn test_deprecated_tui_app_state_backward_compatibility() {
    let deals = vec![
        make_test_deal(34, "Tritanium"),
        make_test_deal(35, "Pyerite"),
        make_test_deal(36, "Mexallon"),
    ];

    let route = TradeRoute {
        stops: Vec::new(),
        waypoints: Vec::new(),
        waypoint_names: Vec::new(),
        total_jumps: 0,
        total_profit: 0.0,
        total_capital_required: 0.0,
        max_volume_used: 0.0,
        avg_profit_per_jump: 0.0,
    };

    let mut state = AppState::new(&deals, &route, BIG_HAULER, "Jita".to_string(), true, true);

    // Initial state: first row is selected
    assert_eq!(state.deals.len(), 3);
    assert_eq!(state.table_state.selected(), Some(0));

    // next_row() advances selection index sequentially
    state.next_row();
    assert_eq!(state.table_state.selected(), Some(1));
    state.next_row();
    assert_eq!(state.table_state.selected(), Some(2));

    // Boundary wrap-around: advancing past last row wraps to index 0
    state.next_row();
    assert_eq!(state.table_state.selected(), Some(0));

    // prev_row() wraps backwards from index 0 to last row (index 2)
    state.prev_row();
    assert_eq!(state.table_state.selected(), Some(2));
    state.prev_row();
    assert_eq!(state.table_state.selected(), Some(1));
    state.prev_row();
    assert_eq!(state.table_state.selected(), Some(0));

    // Empty state boundary: with 0 deals, navigation must maintain None without panic
    let mut empty_state = AppState::new(&[], &route, BIG_HAULER, "Jita".to_string(), true, true);
    assert_eq!(empty_state.table_state.selected(), None);
    empty_state.next_row();
    assert_eq!(empty_state.table_state.selected(), None);
    empty_state.prev_row();
    assert_eq!(empty_state.table_state.selected(), None);
}
