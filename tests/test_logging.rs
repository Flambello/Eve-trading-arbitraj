use eve_trade_scout::logging::init_logging;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn test_temp_log_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("eve_scout_test_logs_{}", nanos));
    dir
}

#[test]
fn test_logging_initialization_with_dir() {
    let log_dir = test_temp_log_dir();
    assert!(!log_dir.exists(), "Test log directory should not exist prior to init");

    let guard = init_logging(Some(&log_dir), "debug");
    assert!(guard.is_ok(), "init_logging should return Ok");

    // Directory must be created
    assert!(log_dir.exists(), "Log directory must be created automatically");
    assert!(log_dir.is_dir(), "Log directory must be a directory");

    // Emit test structured log events
    tracing::info!(test_key = "test_value_123", "Logging integration test event");
    tracing::error!(
        error_code = "TEST_INTEGRATION_ERROR",
        status_code = 500,
        "Simulated test error event"
    );

    // Drop guard to flush buffer
    drop(guard);

    // Clean up
    let _ = fs::remove_dir_all(&log_dir);
}

#[test]
fn test_logging_initialization_stdout_only() {
    // When log_dir is None, only stdout layer is registered, no error should occur
    let guard = init_logging(None, "info");
    assert!(guard.is_ok(), "init_logging with None dir should succeed");

    tracing::warn!("Stdout-only warning event emitted");
}
