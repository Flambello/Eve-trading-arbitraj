#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketOrder {
    pub order_id: u64,
    pub type_id: u32,
    pub location_id: u64,
    pub system_id: u32,
    pub volume_total: u32,
    pub volume_remain: u32,
    pub min_volume: u32,
    pub price: f64,
    pub is_buy_order: bool,
    #[serde(default)]
    pub duration: u32,
    #[serde(default)]
    pub issued: Option<String>,
    #[serde(default)]
    pub range: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsiSystem {
    pub system_id: u32,
    pub name: String,
    pub security_status: f64,
    pub constellation_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EsiType {
    pub type_id: u32,
    pub name: String,
    #[serde(default)]
    pub volume: Option<f64>,
    #[serde(default)]
    pub packaged_volume: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry<T> {
    pub etag: Option<String>,
    pub expires: Option<DateTime<Utc>>,
    pub data: T,
}
