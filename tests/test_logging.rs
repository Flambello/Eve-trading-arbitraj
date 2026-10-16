use eve_trade_scout::logging::{init_logging, init_panic_hook};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn test_temp_log_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("eve_scout_test_logs_{}", nanos))
}

#[test]
fn test_logging_initialization_with_dir() {
    let log_dir = test_temp_log_dir();
    assert!(
        !log_dir.exists(),
        "Test log directory should not exist prior to init"
    );

    let guard = init_logging(Some(&log_dir), "debug");
    assert!(guard.is_ok(), "init_logging should return Ok");
    assert!(
        guard.as_ref().unwrap().has_file_writer(),
        "Logging with directory must activate persistent file writer"
    );

    // Directory must be created
    assert!(
        log_dir.exists(),
        "Log directory must be created automatically"
    );
    assert!(log_dir.is_dir(), "Log directory must be a directory");

    // Emit test structured log events
    tracing::info!(
        test_key = "test_value_123",
        "Logging integration test event"
    );
    tracing::error!(
        error_code = "ERR_ESI_NETWORK_FAILED",
        status_code = 500,
        "Simulated test error event"
    );

    // Drop guard to flush buffer to disk
    drop(guard);

    // Verify log files created in directory
    let entries: Vec<PathBuf> = fs::read_dir(&log_dir)
        .expect("Should be able to read log_dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();

    for path in entries {
        let content = fs::read_to_string(&path).expect("Log file should be readable");
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let val: Result<serde_json::Value, _> = serde_json::from_str(line);
            assert!(
                val.is_ok(),
                "Each log line must be valid JSON: {}\nError: {:?}",
                line,
                val.err()
            );
            let json = val.unwrap();
            assert!(
                json.get("timestamp").is_some(),
                "Log entry must contain timestamp"
            );
            assert!(json.get("level").is_some(), "Log entry must contain level");
            assert!(
                json.get("target").is_some(),
                "Log entry must contain target"
            );
            assert!(
                json.get("fields").is_some(),
                "Log entry must contain fields"
            );
        }
    }

    // Clean up
    let _ = fs::remove_dir_all(&log_dir);
}

#[test]
fn test_logging_initialization_stdout_only() {
    // When log_dir is None, only stdout layer is registered, no error should occur
    let guard_res = init_logging(None, "info");
    assert!(
        guard_res.is_ok(),
        "init_logging with None dir should succeed"
    );

    let guard = guard_res.unwrap();
    assert!(
        !guard.has_file_writer(),
        "Stdout-only logging must not activate a persistent file writer"
    );

    tracing::warn!("Stdout-only warning event emitted");
}

#[test]
fn test_panic_hook_registration() {
    // Register the custom panic hook
    init_panic_hook();

    // Verify panic interception and payload preservation via catch_unwind
    let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        panic!("Controlled QA panic verification test payload");
    }));

    assert!(
        panic_result.is_err(),
        "Panic must be caught by catch_unwind without aborting process"
    );

    let panic_err = panic_result.unwrap_err();
    let panic_msg = if let Some(s) = panic_err.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = panic_err.downcast_ref::<String>() {
        s.clone()
    } else {
        String::new()
    };

    assert_eq!(
        panic_msg, "Controlled QA panic verification test payload",
        "Panic hook must preserve the exact panic payload"
    );
}
