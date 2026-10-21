//! Centralized error codes and diagnostic classification for EVE Trade Scout.
//!
//! Provides a standardized taxonomy of machine-readable error codes (`ErrorCode`)
//! across all subsystems: SDE, ESI, SSO, Graph, Arbitrage, Config, UI, and System.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Standardized machine-readable error codes for structured logging and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    // ── 1000..1999: SDE & Universe Database ─────────────────────────────────
    /// SDE SQLite file not found on disk or at configured path.
    ErrSdeNotFound,
    /// Failed to download the remote SDE database archive from CCP / Fuzzwork.
    ErrSdeDownloadFailed,
    /// Gzip decompression failed or SQLite file header is invalid.
    ErrSdeDecompressionFailed,
    /// Archive decompressed bytes exceeded safety ceiling (decompression bomb guard).
    ErrSdeDecompressionBomb,
    /// SQLite schema validation failed (missing tables or expected columns).
    ErrSdeSchemaInvalid,
    /// Rusqlite database query execution failed.
    ErrSdeQueryFailed,
    /// Solar system name could not be resolved to a system ID.
    ErrSdeSystemNotFound,
    /// Station ID could not be resolved to a station record.
    ErrSdeStationNotFound,

    // ── 2000..2999: CCP ESI REST API & Market ──────────────────────────────
    /// Failed to initialize ESI HTTP client or local cache directory.
    ErrEsiClientInit,
    /// Network connection dropped, timed out, or host unreachable.
    ErrEsiNetworkFailed,
    /// ESI rate limit encountered (HTTP 420 or 429) after retry budget exhausted.
    ErrEsiRateLimited,
    /// ESI upstream server error (HTTP 5xx) after retry budget exhausted.
    ErrEsiServerError,
    /// ESI client request rejected (HTTP 4xx client error).
    ErrEsiClientError,
    /// JSON parsing of ESI response failed or returned unexpected payload.
    ErrEsiParseFailed,
    /// ETag HTTP caching layer failed to serialize or persist cache map.
    ErrEsiEtagCache,

    // ── 3000..3999: EVE Single Sign-On (SSO) & Autopilot ───────────────────
    /// SSO configuration is missing or developer client ID is invalid.
    ErrSsoConfigInvalid,
    /// Local loopback TCP listener could not bind for OAuth callback.
    ErrSsoAuthServerFailed,
    /// OAuth authorization code exchange with login.eveonline.com failed.
    ErrSsoTokenExchange,
    /// Access token refresh failed (invalid grant, revoked token, or network error).
    ErrSsoTokenRefresh,
    /// Failed to save token to disk or set 0600 secure permissions on Unix.
    ErrSsoTokenSave,
    /// Live character location query failed from CCP ESI.
    ErrSsoLocationFailed,
    /// Autopilot waypoint injection failed in EVE game client.
    ErrSsoAutopilotFailed,

    // ── 4000..4999: Stargate Topology & Pathfinding ────────────────────────
    /// Constructing petgraph stargate network from SDE jump tables failed.
    ErrGraphBuildFailed,
    /// Requested destination system is unreachable or safe highsec path does not exist.
    ErrGraphRouteUnreachable,

    // ── 5000..5999: Arbitrage Engine & Knapsack Optimizer ──────────────────
    /// Arbitrage filter or configuration constraint is invalid (<10% ROI, negative price).
    ErrArbConfigInvalid,
    /// Ship cargo volume or budget evaluates to zero or negative capacity.
    ErrArbZeroCapacity,

    // ── 6000..6999: Configuration & Persistence ────────────────────────────
    /// Failed to load or deserialize user `config.toml`.
    ErrConfigLoadFailed,
    /// Failed to serialize or write user `config.toml`.
    ErrConfigSaveFailed,

    // ── 7000..7999: User Interface & Audio ─────────────────────────────────
    /// Tokio runtime or eframe native GUI window failed to initialize.
    ErrUiRuntimeInit,
    /// Background worker thread channel disconnected or panicked.
    ErrUiWorkerChannel,
    /// Audio alert playback failed (audio device error or sound theme decoding error).
    ErrUiAudioPlayback,
    /// Failed to export deals or manifests to CSV file on disk.
    ErrUiCsvExport,
    /// Application startup initialization sequence failed across one or more subsystems.
    ErrUiStartupFailed,

    // ── 9000..9999: System & Panic Handling ────────────────────────────────
    /// Unhandled application panic caught by the global panic hook.
    ErrSysPanic,
}

impl ErrorCode {
    /// Canonical alphanumeric uppercase string identifier (e.g. `"ERR_ESI_NETWORK_FAILED"`).
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ErrSdeNotFound => "ERR_SDE_NOT_FOUND",
            Self::ErrSdeDownloadFailed => "ERR_SDE_DOWNLOAD_FAILED",
            Self::ErrSdeDecompressionFailed => "ERR_SDE_DECOMPRESSION_FAILED",
            Self::ErrSdeDecompressionBomb => "ERR_SDE_DECOMPRESSION_BOMB",
            Self::ErrSdeSchemaInvalid => "ERR_SDE_SCHEMA_INVALID",
            Self::ErrSdeQueryFailed => "ERR_SDE_QUERY_FAILED",
            Self::ErrSdeSystemNotFound => "ERR_SDE_SYSTEM_NOT_FOUND",
            Self::ErrSdeStationNotFound => "ERR_SDE_STATION_NOT_FOUND",

            Self::ErrEsiClientInit => "ERR_ESI_CLIENT_INIT",
            Self::ErrEsiNetworkFailed => "ERR_ESI_NETWORK_FAILED",
            Self::ErrEsiRateLimited => "ERR_ESI_RATE_LIMITED",
            Self::ErrEsiServerError => "ERR_ESI_SERVER_ERROR",
            Self::ErrEsiClientError => "ERR_ESI_CLIENT_ERROR",
            Self::ErrEsiParseFailed => "ERR_ESI_PARSE_FAILED",
            Self::ErrEsiEtagCache => "ERR_ESI_ETAG_CACHE",

            Self::ErrSsoConfigInvalid => "ERR_SSO_CONFIG_INVALID",
            Self::ErrSsoAuthServerFailed => "ERR_SSO_AUTH_SERVER_FAILED",
            Self::ErrSsoTokenExchange => "ERR_SSO_TOKEN_EXCHANGE",
            Self::ErrSsoTokenRefresh => "ERR_SSO_TOKEN_REFRESH",
            Self::ErrSsoTokenSave => "ERR_SSO_TOKEN_SAVE",
            Self::ErrSsoLocationFailed => "ERR_SSO_LOCATION_FAILED",
            Self::ErrSsoAutopilotFailed => "ERR_SSO_AUTOPILOT_FAILED",

            Self::ErrGraphBuildFailed => "ERR_GRAPH_BUILD_FAILED",
            Self::ErrGraphRouteUnreachable => "ERR_GRAPH_ROUTE_UNREACHABLE",

            Self::ErrArbConfigInvalid => "ERR_ARB_CONFIG_INVALID",
            Self::ErrArbZeroCapacity => "ERR_ARB_ZERO_CAPACITY",

            Self::ErrConfigLoadFailed => "ERR_CONFIG_LOAD_FAILED",
            Self::ErrConfigSaveFailed => "ERR_CONFIG_SAVE_FAILED",

            Self::ErrUiRuntimeInit => "ERR_UI_RUNTIME_INIT",
            Self::ErrUiWorkerChannel => "ERR_UI_WORKER_CHANNEL",
            Self::ErrUiAudioPlayback => "ERR_UI_AUDIO_PLAYBACK",
            Self::ErrUiCsvExport => "ERR_UI_CSV_EXPORT",
            Self::ErrUiStartupFailed => "ERR_UI_STARTUP_FAILED",

            Self::ErrSysPanic => "ERR_SYS_PANIC",
        }
    }

    /// Numeric error code identifier (e.g. `2002`).
    pub const fn code_num(&self) -> u16 {
        match self {
            Self::ErrSdeNotFound => 1001,
            Self::ErrSdeDownloadFailed => 1002,
            Self::ErrSdeDecompressionFailed => 1003,
            Self::ErrSdeDecompressionBomb => 1004,
            Self::ErrSdeSchemaInvalid => 1005,
            Self::ErrSdeQueryFailed => 1006,
            Self::ErrSdeSystemNotFound => 1007,
            Self::ErrSdeStationNotFound => 1008,

            Self::ErrEsiClientInit => 2001,
            Self::ErrEsiNetworkFailed => 2002,
            Self::ErrEsiRateLimited => 2003,
            Self::ErrEsiServerError => 2004,
            Self::ErrEsiClientError => 2005,
            Self::ErrEsiParseFailed => 2006,
            Self::ErrEsiEtagCache => 2007,

            Self::ErrSsoConfigInvalid => 3001,
            Self::ErrSsoAuthServerFailed => 3002,
            Self::ErrSsoTokenExchange => 3003,
            Self::ErrSsoTokenRefresh => 3004,
            Self::ErrSsoTokenSave => 3005,
            Self::ErrSsoLocationFailed => 3006,
            Self::ErrSsoAutopilotFailed => 3007,

            Self::ErrGraphBuildFailed => 4001,
            Self::ErrGraphRouteUnreachable => 4002,

            Self::ErrArbConfigInvalid => 5001,
            Self::ErrArbZeroCapacity => 5002,

            Self::ErrConfigLoadFailed => 6001,
            Self::ErrConfigSaveFailed => 6002,

            Self::ErrUiRuntimeInit => 7001,
            Self::ErrUiWorkerChannel => 7002,
            Self::ErrUiAudioPlayback => 7003,
            Self::ErrUiCsvExport => 7004,
            Self::ErrUiStartupFailed => 7005,

            Self::ErrSysPanic => 9001,
        }
    }

    /// Human-readable explanation of the error condition.
    pub const fn description(&self) -> &'static str {
        match self {
            Self::ErrSdeNotFound => "SDE SQLite database file not found on disk",
            Self::ErrSdeDownloadFailed => "Failed to download remote SDE archive",
            Self::ErrSdeDecompressionFailed => "Gzip decompression failed or SQLite header invalid",
            Self::ErrSdeDecompressionBomb => "Decompressed archive exceeded safety ceiling",
            Self::ErrSdeSchemaInvalid => "SDE database schema missing required tables or columns",
            Self::ErrSdeQueryFailed => "Rusqlite database query execution error",
            Self::ErrSdeSystemNotFound => "Solar system name not found in universe database",
            Self::ErrSdeStationNotFound => "Station record not found in universe database",

            Self::ErrEsiClientInit => "Failed to initialize ESI HTTP client or cache directory",
            Self::ErrEsiNetworkFailed => {
                "Network connection dropped, timed out, or host unreachable"
            }
            Self::ErrEsiRateLimited => "ESI rate limit (HTTP 420/429) backoff budget exhausted",
            Self::ErrEsiServerError => "ESI upstream server error (HTTP 5xx) retries exhausted",
            Self::ErrEsiClientError => "ESI client error response (HTTP 4xx)",
            Self::ErrEsiParseFailed => "Failed to parse JSON response from ESI endpoint",
            Self::ErrEsiEtagCache => "ETag HTTP cache persistence error",

            Self::ErrSsoConfigInvalid => "EVE SSO configuration or client ID invalid",
            Self::ErrSsoAuthServerFailed => "Local loopback HTTP server failed to bind for OAuth",
            Self::ErrSsoTokenExchange => "OAuth authorization code exchange failed",
            Self::ErrSsoTokenRefresh => "EVE SSO access token refresh failed",
            Self::ErrSsoTokenSave => "Failed to persist token file or enforce Unix permissions",
            Self::ErrSsoLocationFailed => "Failed to query pilot live location from CCP ESI",
            Self::ErrSsoAutopilotFailed => "Failed to inject autopilot waypoint into EVE client",

            Self::ErrGraphBuildFailed => "Failed to compile stargate network graph from SDE",
            Self::ErrGraphRouteUnreachable => "Solar system unreachable under routing constraints",

            Self::ErrArbConfigInvalid => "Arbitrage optimizer parameters or budget invalid",
            Self::ErrArbZeroCapacity => "Ship volume or budget capacity evaluates to zero",

            Self::ErrConfigLoadFailed => "Failed to load user configuration file",
            Self::ErrConfigSaveFailed => "Failed to save user configuration file",

            Self::ErrUiRuntimeInit => "Failed to initialize async runtime or desktop GUI window",
            Self::ErrUiWorkerChannel => "Worker thread channel disconnected or panicked",
            Self::ErrUiAudioPlayback => "Audio alert playback failed",
            Self::ErrUiCsvExport => "Failed to write CSV export to disk",
            Self::ErrUiStartupFailed => {
                "Application startup initialization sequence failed across one or more subsystems"
            }

            Self::ErrSysPanic => "Unhandled application panic",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
