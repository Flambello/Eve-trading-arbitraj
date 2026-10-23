use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShipProfile {
    pub name: &'static str,
    pub max_volume: f64, // m³
    pub max_budget: f64, // ISK
}

pub const BIG_HAULER: ShipProfile = ShipProfile {
    name: "Big Hauler",
    max_volume: 60_000.0,
    max_budget: 500_000_000.0,
};

pub const SMALL_HAULER: ShipProfile = ShipProfile {
    name: "Small Hauler",
    max_volume: 30_000.0,
    max_budget: 50_000_000.0,
};

pub fn profile_from_str(s: &str) -> Result<ShipProfile> {
    match s.trim().to_lowercase().as_str() {
        "big" | "b" | "big hauler" => Ok(BIG_HAULER),
        "small" | "s" | "small hauler" => Ok(SMALL_HAULER),
        other => bail!(
            "Unknown ship profile '{}'. Valid choices are 'big' or 'small'.",
            other
        ),
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ShipConfig {
    pub profile: String,
    #[serde(default)]
    pub use_custom_ship: bool,
    #[serde(default = "default_custom_cargo_vol")]
    pub custom_cargo_vol: f64,
    #[serde(default = "default_custom_budget_isk")]
    pub custom_budget_isk: f64,
}

fn default_custom_cargo_vol() -> f64 {
    60_000.0
}

fn default_custom_budget_isk() -> f64 {
    500_000_000.0
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LocationConfig {
    pub home_system: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScanConfig {
    pub regions: Vec<u32>,
    pub safe_route_only: bool,
    #[serde(default = "default_npc_stations_only")]
    pub npc_stations_only: bool,
    #[serde(default = "default_scan_adjacent_regions")]
    pub scan_adjacent_regions: bool,
    #[serde(default)]
    pub auto_refresh: bool,
    #[serde(default = "default_sound_alerts")]
    pub sound_alerts: bool,
    #[serde(default = "default_sound_min_roi_pct")]
    pub sound_min_roi_pct: f64,
}

fn default_sound_alerts() -> bool {
    true
}

fn default_sound_min_roi_pct() -> f64 {
    100.0
}

fn default_npc_stations_only() -> bool {
    true
}

fn default_scan_adjacent_regions() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FilterConfig {
    pub min_profit_isk: f64,
    pub min_spread_pct: f64,
    #[serde(default = "default_min_net_profit_pct")]
    pub min_net_profit_pct: f64,
}

fn default_min_net_profit_pct() -> f64 {
    10.0
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FeeConfig {
    pub broker_fee_pct: f64,
    pub sales_tax_pct: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CacheConfig {
    pub directory: String,
    pub sde_max_age_days: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UiConfig {
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,
    #[serde(default)]
    pub is_fullscreen: bool,
    #[serde(default = "default_dossier_side_panel")]
    pub dossier_side_panel: bool,
    #[serde(default = "default_compact_kpi")]
    pub compact_kpi: bool,
    #[serde(default = "default_max_route_stops")]
    pub max_route_stops: usize,
}

fn default_ui_scale() -> f32 {
    1.0
}

fn default_dossier_side_panel() -> bool {
    true
}

fn default_compact_kpi() -> bool {
    true
}

fn default_max_route_stops() -> usize {
    5
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            ui_scale: default_ui_scale(),
            is_fullscreen: false,
            dossier_side_panel: default_dossier_side_panel(),
            compact_kpi: default_compact_kpi(),
            max_route_stops: default_max_route_stops(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ViewConfig {
    #[serde(default)]
    pub hide_npc_trades: bool,
    #[serde(default = "default_max_deals_per_order")]
    pub max_deals_per_order: usize,
    #[serde(default)]
    pub min_profit_filter_m: f64,
    #[serde(default)]
    pub max_jumps_filter: u32,
    #[serde(default = "default_deal_sort_by")]
    pub deal_sort_by: String,
    #[serde(default = "default_deal_sort_descending")]
    pub deal_sort_descending: bool,
    #[serde(default = "default_chain_max_legs")]
    pub chain_max_legs: usize,
    #[serde(default = "default_chain_max_dest_loads")]
    pub chain_max_dest_loads: usize,
    #[serde(default = "default_chain_include_npc")]
    pub chain_include_npc: bool,
    #[serde(default = "default_chain_detour_enabled")]
    pub chain_detour_enabled: bool,
    #[serde(default = "default_chain_max_detour")]
    pub chain_max_detour: u32,
    #[serde(default = "default_hub_source")]
    pub hub_source_hub: Option<String>,
    #[serde(default = "default_hub_dest")]
    pub hub_dest_hub: Option<String>,
    #[serde(default = "default_hub_include_npc")]
    pub hub_include_npc: bool,
    #[serde(default)]
    pub hub_major_stations_only: bool,
    #[serde(default = "default_hub_min_profit_m")]
    pub hub_min_profit_m: f64,
}

fn default_max_deals_per_order() -> usize {
    2
}
fn default_deal_sort_by() -> String {
    "profit_per_jump".to_string()
}
fn default_deal_sort_descending() -> bool {
    true
}
fn default_chain_max_legs() -> usize {
    4
}
fn default_chain_max_dest_loads() -> usize {
    2
}
fn default_chain_include_npc() -> bool {
    true
}
fn default_chain_detour_enabled() -> bool {
    true
}
fn default_chain_max_detour() -> u32 {
    2
}
fn default_hub_source() -> Option<String> {
    Some("Jita".to_string())
}
fn default_hub_dest() -> Option<String> {
    Some("Amarr".to_string())
}
fn default_hub_include_npc() -> bool {
    true
}
fn default_hub_min_profit_m() -> f64 {
    0.5
}

impl Default for ViewConfig {
    fn default() -> Self {
        Self {
            hide_npc_trades: false,
            max_deals_per_order: default_max_deals_per_order(),
            min_profit_filter_m: 0.0,
            max_jumps_filter: 0,
            deal_sort_by: default_deal_sort_by(),
            deal_sort_descending: default_deal_sort_descending(),
            chain_max_legs: default_chain_max_legs(),
            chain_max_dest_loads: default_chain_max_dest_loads(),
            chain_include_npc: default_chain_include_npc(),
            chain_detour_enabled: default_chain_detour_enabled(),
            chain_max_detour: default_chain_max_detour(),
            hub_source_hub: default_hub_source(),
            hub_dest_hub: default_hub_dest(),
            hub_include_npc: default_hub_include_npc(),
            hub_major_stations_only: false,
            hub_min_profit_m: default_hub_min_profit_m(),
        }
    }
}

/// EVE Single Sign-On (SSO) v2 PKCE OAuth2 settings.
///
/// Public client flow: No client secret is required or used. The public Client ID
/// is embedded in the application binary by default (`DEFAULT_PUBLIC_CLIENT_ID`).
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SsoConfig {
    /// Optional custom Client ID override. When empty, defaults to the embedded public Client ID.
    #[serde(default)]
    pub client_id: String,
    /// Loopback TCP callback port for browser authorization (default: 37373).
    #[serde(default = "default_callback_port")]
    pub callback_port: u16,
    /// Whether to automatically query character location on startup and Deal calculation.
    #[serde(default)]
    pub auto_sync_location: bool,
}

fn default_callback_port() -> u16 {
    crate::esi::sso::DEFAULT_CALLBACK_PORT
}

impl SsoConfig {
    /// Returns the callback port, ensuring it is in the safe unprivileged range (1024..=65535).
    pub fn validated_callback_port(&self) -> u16 {
        if self.callback_port < 1024 {
            default_callback_port()
        } else {
            self.callback_port
        }
    }
}

/// Available cockpit color scheme themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AppTheme {
    /// Deep dark navy space void with neon cyan telemetry highlights and EVE gold accents.
    #[default]
    SciFiVoid,
    /// Minimalist dark charcoal and OLED pitch black with crisp silver/white contrast.
    NeutralBlack,
    /// Obsidian black palette with aggressive crimson red highlights and tactical amber accents.
    BlackRed,
    /// Clean, high-contrast daylight theme with crisp off-white panels and cobalt blue accents.
    SolarLight,
}

impl AppTheme {
    pub const ALL: &'static [AppTheme] = &[
        AppTheme::SciFiVoid,
        AppTheme::NeutralBlack,
        AppTheme::BlackRed,
        AppTheme::SolarLight,
    ];

    pub fn display_name(&self) -> &'static str {
        match self {
            AppTheme::SciFiVoid => "Sci-Fi Void",
            AppTheme::NeutralBlack => "Neutral Black",
            AppTheme::BlackRed => "Black & Red",
            AppTheme::SolarLight => "Solar Light",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            AppTheme::SciFiVoid => "Deep navy void with neon cyan telemetry & gold accents",
            AppTheme::NeutralBlack => {
                "Minimalist charcoal & OLED black with crisp monochrome contrast"
            }
            AppTheme::BlackRed => {
                "Obsidian void with tactical crimson red & combat pirate aesthetics"
            }
            AppTheme::SolarLight => {
                "High-contrast clean daylight workstation theme with cobalt accents"
            }
        }
    }

    pub fn is_dark(&self) -> bool {
        !matches!(self, AppTheme::SolarLight)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            AppTheme::SciFiVoid => "sci_fi_void",
            AppTheme::NeutralBlack => "neutral_black",
            AppTheme::BlackRed => "black_red",
            AppTheme::SolarLight => "solar_light",
        }
    }
}

impl std::fmt::Display for AppTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

impl std::str::FromStr for AppTheme {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s
            .trim()
            .to_lowercase()
            .replace('&', "and")
            .replace(['-', ' '], "_");
        let cleaned = normalized.replace("__", "_");
        match cleaned.as_str() {
            "sci_fi_void" | "scifi" | "sci_fi" | "void" | "default" | "navy" | "dark" => {
                Ok(AppTheme::SciFiVoid)
            }
            "neutral_black" | "neutral" | "black" | "oled" | "stealth" | "charcoal" => {
                Ok(AppTheme::NeutralBlack)
            }
            "black_red"
            | "black_and_red"
            | "black_&_red"
            | "black&red"
            | "blackandred"
            | "crimson"
            | "red"
            | "blood_raider"
            | "triglavian" => Ok(AppTheme::BlackRed),
            "solar_light" | "light" | "daylight" | "solar" | "white" => Ok(AppTheme::SolarLight),
            other => anyhow::bail!(
                "Unknown theme '{}'. Valid choices: 'sci_fi_void', 'neutral_black', 'black_red', 'solar_light'.",
                other
            ),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserConfig {
    pub ship: ShipConfig,
    pub location: LocationConfig,
    pub scan: ScanConfig,
    pub filters: FilterConfig,
    pub fees: FeeConfig,
    pub cache: CacheConfig,
    #[serde(default)]
    pub sso: SsoConfig,
    #[serde(default)]
    pub theme: AppTheme,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub view: ViewConfig,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            ship: ShipConfig {
                profile: "big".to_string(),
                use_custom_ship: false,
                custom_cargo_vol: 60_000.0,
                custom_budget_isk: 500_000_000.0,
            },
            location: LocationConfig {
                home_system: "Jita".to_string(),
            },
            scan: ScanConfig {
                // Default to The Forge (10000002) - Jita's region
                regions: vec![10000002],
                safe_route_only: true,
                npc_stations_only: true,
                scan_adjacent_regions: true,
                auto_refresh: false,
                sound_alerts: true,
                sound_min_roi_pct: 100.0,
            },
            filters: FilterConfig {
                min_profit_isk: 1_000_000.0,
                min_spread_pct: 3.0,
                min_net_profit_pct: 10.0, // Enforced minimum 10% after taxes
            },
            fees: FeeConfig {
                broker_fee_pct: 3.0,
                sales_tax_pct: 8.0,
            },
            cache: CacheConfig {
                directory: "cache".to_string(),
                sde_max_age_days: 30,
            },
            sso: SsoConfig::default(),
            theme: AppTheme::default(),
            ui: UiConfig::default(),
            view: ViewConfig::default(),
        }
    }
}

/// Returns the base configuration directory for the application across all platforms.
///
/// Precedence:
/// 1. `EVE_CONFIG_DIR` environment variable (if set and non-empty)
/// 2. `XDG_CONFIG_HOME` environment variable (if set and non-empty, cross-platform)
/// 3. Platform default via `dirs::config_dir()` (%APPDATA% on Windows, ~/.config on Linux, ~/Library/Application Support on macOS)
pub fn config_base_dir() -> Option<PathBuf> {
    if let Some(val) = std::env::var_os("EVE_CONFIG_DIR") {
        if !val.is_empty() {
            return Some(PathBuf::from(val));
        }
    }
    if let Some(val) = std::env::var_os("XDG_CONFIG_HOME") {
        if !val.is_empty() {
            return Some(PathBuf::from(val));
        }
    }
    dirs::config_dir()
}

impl UserConfig {
    /// Resolves the filesystem path to the user's `config.toml`.
    ///
    /// Prefers `~/.config/eve-traveling-merchant/config.toml`. If that does not exist but
    /// a legacy `~/.config/eve-trade-scout/config.toml` exists, returns the legacy path for backwards compatibility.
    /// Otherwise returns the modern path.
    pub fn config_path() -> Result<PathBuf> {
        let base = config_base_dir().context("Could not determine user config directory")?;
        let new_path = base.join("eve-traveling-merchant").join("config.toml");
        if new_path.exists() {
            return Ok(new_path);
        }
        let legacy_path = base.join("eve-trade-scout").join("config.toml");
        if legacy_path.exists() {
            return Ok(legacy_path);
        }
        Ok(new_path)
    }

    pub fn load() -> Result<Self> {
        let path = match Self::config_path() {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    error_code = %crate::error::ErrorCode::ErrConfigLoadFailed,
                    error = %e,
                    "Failed to determine config directory, using defaults"
                );
                return Ok(Self::default());
            }
        };

        if !path.exists() {
            let default_cfg = Self::default();
            let _ = default_cfg.save();
            return Ok(default_cfg);
        }

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(
                    target: "eve_trade_scout",
                    error_code = %crate::error::ErrorCode::ErrConfigLoadFailed,
                    path = ?path,
                    error = %e,
                    "Failed to read config file, falling back to defaults"
                );
                return Ok(Self::default());
            }
        };

        match toml::from_str::<UserConfig>(&content) {
            Ok(mut cfg) => {
                // Invariant: Enforce >= 10.0% net ROI floor with strict NaN defense
                if !cfg.filters.min_net_profit_pct.is_finite()
                    || cfg.filters.min_net_profit_pct < 10.0
                {
                    cfg.filters.min_net_profit_pct = 10.0;
                }
                if !cfg.ui.ui_scale.is_finite() || cfg.ui.ui_scale < 0.75 || cfg.ui.ui_scale > 1.50
                {
                    cfg.ui.ui_scale = 1.0;
                }
                if !cfg.fees.broker_fee_pct.is_finite()
                    || cfg.fees.broker_fee_pct < 1.0
                    || cfg.fees.broker_fee_pct > 5.0
                {
                    cfg.fees.broker_fee_pct = 3.0;
                }
                if !cfg.fees.sales_tax_pct.is_finite()
                    || cfg.fees.sales_tax_pct < 3.0
                    || cfg.fees.sales_tax_pct > 8.0
                {
                    cfg.fees.sales_tax_pct = 8.0;
                }
                if !cfg.ship.custom_cargo_vol.is_finite() || cfg.ship.custom_cargo_vol < 10.0 {
                    cfg.ship.custom_cargo_vol = 60_000.0;
                }
                if !cfg.ship.custom_budget_isk.is_finite() || cfg.ship.custom_budget_isk < 100_000.0
                {
                    cfg.ship.custom_budget_isk = 500_000_000.0;
                }

                // Cap collection limits and search depths to prevent excessive allocation or recursion
                cfg.ui.max_route_stops = cfg.ui.max_route_stops.clamp(1, 20);
                cfg.view.max_deals_per_order = cfg.view.max_deals_per_order.clamp(1, 10);
                cfg.view.chain_max_legs = cfg.view.chain_max_legs.clamp(1, 10);
                cfg.view.chain_max_dest_loads = cfg.view.chain_max_dest_loads.clamp(1, 10);
                cfg.view.chain_max_detour = cfg.view.chain_max_detour.clamp(0, 10);
                if !cfg.view.min_profit_filter_m.is_finite() || cfg.view.min_profit_filter_m < 0.0 {
                    cfg.view.min_profit_filter_m = 0.0;
                }
                if !cfg.view.hub_min_profit_m.is_finite() || cfg.view.hub_min_profit_m < 0.0 {
                    cfg.view.hub_min_profit_m = 0.5;
                }
                Ok(cfg)
            }
            Err(e) => {
                tracing::error!(
                    target: "eve_trade_scout",
                    error_code = %crate::error::ErrorCode::ErrConfigLoadFailed,
                    path = ?path,
                    error = %e,
                    "Failed to parse TOML configuration from disk, using defaults"
                );
                Ok(Self::default())
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        let tmp_path = path.with_extension("toml.tmp");
        if let Err(e) = fs::write(&tmp_path, &content) {
            tracing::error!(
                target: "eve_trade_scout",
                error_code = %crate::error::ErrorCode::ErrConfigSaveFailed,
                path = ?tmp_path,
                error = %e,
                "Failed to write temporary configuration file to disk"
            );
            return Err(e.into());
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            if let Err(e) = std::fs::set_permissions(&tmp_path, perms) {
                tracing::warn!(
                    target: "eve_trade_scout",
                    error = %e,
                    path = ?tmp_path,
                    "Failed to set 0600 permissions on config file"
                );
            }
        }

        if let Err(e) = fs::rename(&tmp_path, &path) {
            tracing::error!(
                target: "eve_trade_scout",
                error_code = %crate::error::ErrorCode::ErrConfigSaveFailed,
                from = ?tmp_path,
                to = ?path,
                error = %e,
                "Failed to atomically rename configuration file to destination path"
            );
            let _ = fs::remove_file(&tmp_path);
            return Err(e.into());
        }
        Ok(())
    }

    pub fn ship_profile(&self) -> Result<ShipProfile> {
        profile_from_str(&self.ship.profile)
    }

    /// Resolves the effective ship profile, respecting custom cargo hold volume
    /// and anti-gank collateral budget if custom ship mode is enabled.
    pub fn effective_ship_profile(&self) -> Result<ShipProfile> {
        if self.ship.use_custom_ship {
            let max_volume = if self.ship.custom_cargo_vol.is_finite() {
                self.ship.custom_cargo_vol.clamp(10.0, 1_500_000.0)
            } else {
                default_custom_cargo_vol()
            };
            let max_budget = if self.ship.custom_budget_isk.is_finite() {
                self.ship.custom_budget_isk.clamp(100_000.0, 100_000_000_000.0)
            } else {
                default_custom_budget_isk()
            };
            Ok(ShipProfile {
                name: "Custom Vessel Profile",
                max_volume,
                max_budget,
            })
        } else {
            self.ship_profile()
        }
    }
}

/// Parses and validates a comma-separated list of numeric region IDs.
///
/// Returns `Ok(Vec<u32>)` if valid numeric IDs were parsed, or an error if the input
/// is empty, whitespace-only, or contains no valid numeric IDs.
pub fn parse_region_ids(input: &str) -> Result<Vec<u32>> {
    let parsed: Vec<u32> = input
        .split(',')
        .filter_map(|s| s.trim().parse::<u32>().ok())
        .collect();
    if parsed.is_empty() {
        anyhow::bail!(
            "Invalid regions argument '{}'. Expected comma-separated numeric region IDs (e.g. '10000002, 10000043').",
            input
        );
    }
    Ok(parsed)
}
