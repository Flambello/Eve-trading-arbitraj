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

#[test]
#[allow(deprecated)]
fn test_deprecated_tui_app_state_backward_compatibility() {
    let deals: Vec<ArbitrageDeal> = Vec::new();
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

    let mut state = AppState::new(
        &deals,
        &route,
        BIG_HAULER,
        "Jita".to_string(),
        true,
        true,
    );

    // Verify state methods work without panic
    state.next_row();
    state.prev_row();
    assert!(state.deals.is_empty());
}
