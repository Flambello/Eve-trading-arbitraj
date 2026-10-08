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
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FilterConfig {
    pub min_profit_isk: f64,
    pub min_spread_pct: f64,
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
pub struct UserConfig {
    pub ship: ShipConfig,
    pub location: LocationConfig,
    pub scan: ScanConfig,
    pub filters: FilterConfig,
    pub fees: FeeConfig,
    pub cache: CacheConfig,
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
            },
            filters: FilterConfig {
                min_profit_isk: 1_000_000.0,
                min_spread_pct: 3.0,
            },
            fees: FeeConfig {
                broker_fee_pct: 3.0,
                sales_tax_pct: 8.0,
            },
            cache: CacheConfig {
                directory: "cache".to_string(),
                sde_max_age_days: 30,
            },
        }
    }
}

impl UserConfig {
    pub fn config_path() -> Result<PathBuf> {
        let base = dirs::config_dir().context("Could not determine user config directory")?;
        Ok(base.join("eve-trade-scout").join("config.toml"))
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

    #[allow(dead_code)]
    pub fn ship_profile(&self) -> Result<ShipProfile> {
        profile_from_str(&self.ship.profile)
    }
}
