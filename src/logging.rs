//! Production-grade structured error logging and diagnostics for EVE Trade Scout.

use std::backtrace::Backtrace;
use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{filter::EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// RAII Guard that keeps the background log writer thread active.
/// Ensures all buffered log entries are flushed to disk on shutdown.
pub struct LoggingGuard {
    _file_guard: Option<WorkerGuard>,
}

/// Initializes the global tracing subscriber with dual targets:
/// 1. Human-readable stdout with ANSI colors.
/// 2. Daily rolling structured JSON file appender in `logs/app.log`.
pub fn init_logging(
    log_dir: Option<&Path>,
    default_level: &str,
) -> Result<LoggingGuard, Box<dyn std::error::Error + Send + Sync>> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    // Target 1: Formatted stdout for terminal/developer output
    let stdout_layer = fmt::layer()
        .with_target(true)
        .with_line_number(true)
        .compact();

    // Target 2: Daily rolling structured file log
    let (file_layer, guard) = if let Some(dir) = log_dir {
        std::fs::create_dir_all(dir)?;
        let file_appender = tracing_appender::rolling::daily(dir, "app.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        let layer = fmt::layer()
            .json()
            .with_target(true)
            .with_current_span(true)
            .with_line_number(true)
            .with_file(true)
            .with_writer(non_blocking);

        (Some(layer), Some(guard))
    } else {
        (None, None)
    };

    // Initialize global subscriber if not already initialized in the current process
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer)
        .with(file_layer)
        .try_init();

    // Register panic hook to capture unhandled panics and backtraces
    init_panic_hook();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        target_dir = ?log_dir,
        "Logging core initialized successfully"
    );

    Ok(LoggingGuard { _file_guard: guard })
}

/// Registers a custom panic hook that emits structured ERROR logs with full backtraces.
pub fn init_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let payload = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            *s
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.as_str()
        } else {
            "Unknown panic payload"
        };

        let location = panic_info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());

        let backtrace = Backtrace::capture();

        tracing::error!(
            error_code = %crate::error::ErrorCode::ErrSysPanic,
            panic_payload = %payload,
            panic_location = %location,
            backtrace = %backtrace,
            "FATAL: Application encountered an unhandled panic"
        );

        default_hook(panic_info);
    }));
}
