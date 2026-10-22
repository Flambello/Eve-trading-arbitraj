use eframe::egui;
use eve_trade_scout::config::{profile_from_str, BIG_HAULER};
use eve_trade_scout::error::ErrorCode;
use eve_trade_scout::ui::{DesktopApp, StartupMsg};
use std::sync::mpsc;

#[test]
fn test_startup_worker_channel_disconnect_detected_as_error() {
    let (tx, rx) = mpsc::channel::<StartupMsg>();
    // Drop the sender immediately to simulate a crashed or aborted worker thread
    drop(tx);

    let mut app = DesktopApp::new_loading(&eve_trade_scout::config::UserConfig::default(), rx);

    assert!(app.is_startup, "App must start in loading splash state");
    assert!(
        app.startup_progress.error.is_none(),
        "No error should be set initially"
    );
    assert!(
        app.startup_progress.error_code.is_none(),
        "No error code should be set initially"
    );

    let ctx = egui::Context::default();
    app.poll_startup(&ctx);

    // The catch-all must have detected worker termination/disconnection
    assert!(
        app.startup_progress.error.is_some(),
        "Catch-all must populate startup_progress.error on channel disconnect"
    );
    assert_eq!(
        app.startup_progress.error_code,
        Some(ErrorCode::ErrUiWorkerChannel),
        "Error code must be ErrUiWorkerChannel"
    );
    let err_msg = app.startup_progress.error.as_ref().unwrap();
    assert!(
        err_msg.contains("disconnected unexpectedly"),
        "Error message must clearly explain worker disconnection: {}",
        err_msg
    );

    // Verify recovery into Offline Mode works
    app.is_startup = false;
    assert!(
        app.is_offline(),
        "App must report is_offline() when dependencies are not loaded"
    );
    assert!(
        !app.is_scanning(),
        "App must not initiate market scanning while offline"
    );
    assert_eq!(
        app.startup_progress.error_code,
        Some(ErrorCode::ErrUiWorkerChannel),
        "Error code must remain preserved in offline mode for user inspection"
    );
}

#[test]
fn test_startup_structured_error_message_sets_error_code() {
    let (tx, rx) = mpsc::channel::<StartupMsg>();

    let mut app = DesktopApp::new_loading(&eve_trade_scout::config::UserConfig::default(), rx);

    let error_detail = "Corrupted SQLite header at eve_sde.sqlite (expected SQLite format 3)";
    tx.send(StartupMsg::error(ErrorCode::ErrSdeNotFound, error_detail))
        .expect("Should send startup error message");

    let ctx = egui::Context::default();
    app.poll_startup(&ctx);

    assert_eq!(
        app.startup_progress.error.as_deref(),
        Some(error_detail),
        "Error message must match dispatched message"
    );
    assert_eq!(
        app.startup_progress.error_code,
        Some(ErrorCode::ErrSdeNotFound),
        "Error code must match dispatched ErrSdeNotFound"
    );

    // Dropping sender after an explicit error must NOT overwrite the specific error with a generic channel disconnect
    drop(tx);
    app.poll_startup(&ctx);
    assert_eq!(
        app.startup_progress.error_code,
        Some(ErrorCode::ErrSdeNotFound),
        "Original specific error code must be preserved after channel close"
    );
}

#[test]
fn test_startup_offline_mode_scan_safety() {
    let (_tx, rx) = mpsc::channel::<StartupMsg>();
    let mut app = DesktopApp::new_loading(&eve_trade_scout::config::UserConfig::default(), rx);

    // Transition to offline mode without SDE or ESI
    app.is_startup = false;
    assert!(
        app.is_offline(),
        "App must report is_offline() when dependencies are not loaded"
    );

    // Trigger scan while offline: must safely update status text with actionable warning rather than panic
    app.start_scan();
    assert!(
        !app.is_scanning(),
        "Scan must not be initiated when offline dependencies are missing"
    );
    assert!(
        app.status_text().contains("Cannot scan live market"),
        "Status text must inform user that market scanning is unavailable: {}",
        app.status_text()
    );
    assert!(
        app.status_text().contains("offline"),
        "Status text must mention offline status: {}",
        app.status_text()
    );
}

#[test]
fn test_err_ui_startup_failed_code_properties() {
    let code = ErrorCode::ErrUiStartupFailed;

    assert_eq!(code.as_str(), "ERR_UI_STARTUP_FAILED");
    assert_eq!(code.code_num(), 7005);
    assert_eq!(format!("{}", code), "ERR_UI_STARTUP_FAILED");
    assert!(code
        .description()
        .contains("Application startup initialization sequence failed"));

    // Verify serde JSON roundtrip
    let json = serde_json::to_string(&code).expect("Serialization must succeed");
    assert_eq!(json, "\"ERR_UI_STARTUP_FAILED\"");
    let parsed: ErrorCode = serde_json::from_str(&json).expect("Deserialization must succeed");
    assert_eq!(parsed, code);
}

#[test]
fn test_config_ship_profile_fallback_resilience() {
    let invalid_profiles = ["dreadnought", "freighter", "titan", "carrier", "invalid"];
    for inv in invalid_profiles {
        let mut cfg = eve_trade_scout::config::UserConfig::default();
        cfg.ship.profile = inv.to_string();

        // 1. Invalid profile string fails direct parsing and profile_from_str
        assert!(
            profile_from_str(inv).is_err(),
            "profile_from_str must fail on invalid profile '{}'",
            inv
        );
        assert!(
            cfg.ship_profile().is_err(),
            "Invalid profile '{}' must fail ship_profile()",
            inv
        );
        assert!(
            cfg.effective_ship_profile().is_err(),
            "Invalid profile '{}' must fail effective_ship_profile()",
            inv
        );

        // 2. Production fallback pattern used in main.rs resolves safely to BIG_HAULER
        let resolved = cfg.effective_ship_profile().unwrap_or(BIG_HAULER);
        assert_eq!(resolved.name, "Big Hauler");
        assert_eq!(resolved.max_volume, 60_000.0);
        assert_eq!(resolved.max_budget, 500_000_000.0);

        // 3. Desktop app apply_user_config normalizes invalid profile safely to Big Hauler
        let (_tx, rx) = mpsc::channel::<StartupMsg>();
        let mut app = DesktopApp::new_loading(&eve_trade_scout::config::UserConfig::default(), rx);
        app.apply_user_config(&cfg);
        assert_eq!(
            app.current_ship().name,
            "Big Hauler",
            "App must default to Big Hauler when profile is '{}'",
            inv
        );
        assert_eq!(
            app.current_ship().max_volume,
            60_000.0,
            "App cargo volume must be 60,000 m³"
        );
        assert_eq!(
            app.current_ship().max_budget,
            500_000_000.0,
            "App budget must be 500M ISK"
        );
        let exported = app.export_user_config();
        assert_eq!(
            exported.ship.profile, "big",
            "Exported profile must be normalized to 'big'"
        );
    }
}

#[test]
fn test_desktop_app_session_memory_roundtrip() {
    let (_tx, rx) = mpsc::channel::<StartupMsg>();
    let mut app = DesktopApp::new_loading(&eve_trade_scout::config::UserConfig::default(), rx);

    // Apply custom settings simulating a user tweaking preferences in Cockpit
    let mut custom_cfg = eve_trade_scout::config::UserConfig::default();
    custom_cfg.location.home_system = "Amarr".to_string();
    custom_cfg.ship.profile = "small".to_string();
    custom_cfg.ship.use_custom_ship = true;
    custom_cfg.ship.custom_cargo_vol = 45_000.0;
    custom_cfg.ship.custom_budget_isk = 250_000_000.0;
    custom_cfg.scan.safe_route_only = false;
    custom_cfg.scan.npc_stations_only = false;
    custom_cfg.scan.auto_refresh = true;
    custom_cfg.scan.sound_alerts = true;
    custom_cfg.scan.sound_min_roi_pct = 50.0;
    custom_cfg.filters.min_net_profit_pct = 15.0;
    custom_cfg.filters.min_profit_isk = 5_000_000.0;
    custom_cfg.fees.broker_fee_pct = 1.4;
    custom_cfg.fees.sales_tax_pct = 4.3;
    custom_cfg.theme = eve_trade_scout::config::AppTheme::NeutralBlack;
    custom_cfg.ui.ui_scale = 1.25;
    custom_cfg.ui.is_fullscreen = true;
    custom_cfg.ui.dossier_side_panel = false;
    custom_cfg.ui.compact_kpi = false;
    custom_cfg.ui.max_route_stops = 8;
    custom_cfg.view.deal_sort_by = "total_profit".to_string();
    custom_cfg.view.deal_sort_descending = false;
    custom_cfg.view.hub_source_hub = Some("Amarr".to_string());
    custom_cfg.view.hub_dest_hub = Some("Dodixie".to_string());

    app.apply_user_config(&custom_cfg);

    // Export back to UserConfig
    let exported = app.export_user_config();

    assert_eq!(exported.location.home_system, "Amarr");
    assert_eq!(exported.ship.profile, "small");
    assert!(exported.ship.use_custom_ship);
    assert_eq!(exported.ship.custom_cargo_vol, 45_000.0);
    assert_eq!(exported.ship.custom_budget_isk, 250_000_000.0);
    assert!(!exported.scan.safe_route_only);
    assert!(!exported.scan.npc_stations_only);
    assert!(exported.scan.auto_refresh);
    assert!(exported.scan.sound_alerts);
    assert_eq!(exported.scan.sound_min_roi_pct, 50.0);
    assert_eq!(exported.filters.min_net_profit_pct, 15.0);
    assert_eq!(exported.filters.min_profit_isk, 5_000_000.0);
    assert_eq!(exported.fees.broker_fee_pct, 1.4);
    assert_eq!(exported.fees.sales_tax_pct, 4.3);
    assert_eq!(
        exported.theme,
        eve_trade_scout::config::AppTheme::NeutralBlack
    );
    assert_eq!(exported.ui.ui_scale, 1.25);
    assert!(exported.ui.is_fullscreen);
    assert!(!exported.ui.dossier_side_panel);
    assert!(!exported.ui.compact_kpi);
    assert_eq!(exported.ui.max_route_stops, 8);
    assert_eq!(exported.view.deal_sort_by, "total_profit");
    assert!(!exported.view.deal_sort_descending);
    assert_eq!(exported.view.hub_source_hub, Some("Amarr".to_string()));
    assert_eq!(exported.view.hub_dest_hub, Some("Dodixie".to_string()));

    // Verify effective ship profile reflects custom sizing
    let effective = exported
        .effective_ship_profile()
        .expect("Effective ship profile must resolve");
    assert_eq!(effective.max_volume, 45_000.0);
    assert_eq!(effective.max_budget, 250_000_000.0);
}

#[test]
fn test_desktop_app_legacy_loading_compatibility() {
    let (_tx, rx) = mpsc::channel::<StartupMsg>();
    let app = DesktopApp::new_loading_legacy(
        false, // small hauler
        "Dodixie".to_string(),
        false, // safe only false
        false, // npc only false
        12.5,
        2_500_000.0,
        true,
        true,
        80.0,
        rx,
    );

    let exported = app.export_user_config();
    assert_eq!(exported.location.home_system, "Dodixie");
    assert_eq!(exported.ship.profile, "small");
    assert!(!exported.scan.safe_route_only);
    assert!(!exported.scan.npc_stations_only);
    assert_eq!(exported.filters.min_net_profit_pct, 12.5);
    assert_eq!(exported.filters.min_profit_isk, 2_500_000.0);
    assert!(exported.scan.auto_refresh);
    assert!(exported.scan.sound_alerts);
    assert_eq!(exported.scan.sound_min_roi_pct, 80.0);
}
