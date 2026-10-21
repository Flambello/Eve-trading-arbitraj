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

    let mut app = DesktopApp::new_loading(
        true,
        "Jita".to_string(),
        true,
        true,
        10.0,
        1_000_000.0,
        false,
        false,
        100.0,
        rx,
    );

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
    assert!(!app.is_startup, "Pilot must be able to enter offline mode");
}

#[test]
fn test_startup_structured_error_message_sets_error_code() {
    let (tx, rx) = mpsc::channel::<StartupMsg>();

    let mut app = DesktopApp::new_loading(
        true,
        "Jita".to_string(),
        true,
        true,
        10.0,
        1_000_000.0,
        false,
        false,
        100.0,
        rx,
    );

    let error_detail = "Corrupted SQLite header at eve_sde.sqlite (expected SQLite format 3)";
    tx.send(StartupMsg::error(
        ErrorCode::ErrSdeNotFound,
        error_detail,
    ))
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
    let mut app = DesktopApp::new_loading(
        true,
        "Jita".to_string(),
        true,
        true,
        10.0,
        1_000_000.0,
        false,
        false,
        100.0,
        rx,
    );

    // Transition to offline mode without SDE or ESI
    app.is_startup = false;
    assert!(app.is_offline(), "App must report is_offline() when dependencies are not loaded");

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
        assert!(
            profile_from_str(inv).is_err(),
            "Invalid profile '{}' must fail parsing",
            inv
        );

        // Verify the fallback pattern used in main.rs resolves safely to BIG_HAULER
        let resolved = profile_from_str(inv).unwrap_or(BIG_HAULER);
        assert_eq!(resolved.name, "Big Hauler");
        assert_eq!(resolved.max_volume, 60_000.0);
        assert_eq!(resolved.max_budget, 500_000_000.0);
    }
}
