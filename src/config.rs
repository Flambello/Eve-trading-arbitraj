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

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SsoConfig {
    #[serde(default)]
    pub client_id: String,
    #[serde(default = "default_callback_port")]
    pub callback_port: u16,
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
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            ship: ShipConfig {
                profile: "big".to_string(),
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
        }
    }
}

impl UserConfig {
    /// Resolves the filesystem path to the user's `config.toml`.
    ///
    /// Prefers `~/.config/eve-traveling-merchant/config.toml`. If that does not exist but
    /// a legacy `~/.config/eve-trade-scout/config.toml` exists, returns the legacy path for backwards compatibility.
    /// Otherwise returns the modern path.
    pub fn config_path() -> Result<PathBuf> {
        let base = dirs::config_dir().context("Could not determine user config directory")?;
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
        let path = Self::config_path()?;
        if !path.exists() {
            let default_cfg = Self::default();
            default_cfg
                .save()
                .context("Failed to write default config file")?;
            return Ok(default_cfg);
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file at {:?}", path))?;
        let cfg: Self = toml::from_str(&content)
            .with_context(|| format!("Failed to parse TOML config from {:?}", path))?;
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }

    pub fn ship_profile(&self) -> Result<ShipProfile> {
        profile_from_str(&self.ship.profile)
    }
}
