# Data Model — Rust Types

All internal data structures the application works with, expressed as Rust types.

---

## 1. ShipProfile

```rust
/// Represents cargo and budget constraints for the player's ship.
#[derive(Debug, Clone, Copy)]
pub struct ShipProfile {
    pub name:       &'static str,
    pub max_volume: f64,   // m³
    pub max_budget: f64,   // ISK
}

pub const BIG_HAULER: ShipProfile = ShipProfile {
    name:       "Big Hauler",
    max_volume: 60_000.0,
    max_budget: 500_000_000.0,
};

pub const SMALL_HAULER: ShipProfile = ShipProfile {
    name:       "Small Hauler",
    max_volume: 30_000.0,
    max_budget:  50_000_000.0,
};
```

---

## 2. MarketOrder

A single order deserialized from the ESI response.

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct MarketOrder {
    pub order_id:      u64,
    pub type_id:       u32,
    pub location_id:   u64,   // station or structure ID
    pub system_id:     u32,
    pub price:         f64,   // ISK per unit
    pub volume_remain: u32,
    pub is_buy_order:  bool,
    pub min_volume:    u32,
    pub range:         String,
    pub issued:        DateTime<Utc>,
}
```

> **Buy vs Sell clarification:**
> - `is_buy_order = false` → **sell order** — player can *buy* from this
> - `is_buy_order = true`  → **buy order**  — player can *sell into* this

---

## 3. ItemInfo

Static data loaded from the SDE SQLite.

```rust
#[derive(Debug, Clone)]
pub struct ItemInfo {
    pub type_id:         u32,
    pub name:            String,
    pub packaged_volume: f64,    // m³ per unit (used for cargo calculation)
    pub group_id:        u32,
    pub published:       bool,
}
```

---

## 4. ArbitrageDeal

A single profitable opportunity: buy at station A, sell at station B.

```rust
#[derive(Debug, Clone)]
pub struct ArbitrageDeal {
    pub type_id:            u32,
    pub item_name:          String,

    pub buy_price:          f64,   // cheapest sell order price at source (you pay this)
    pub sell_price:         f64,   // highest buy order price at dest (you receive this)
    pub spread_pct:         f64,   // (sell - buy) / buy * 100

    pub source_station_id:  u64,
    pub source_system_id:   u32,
    pub source_system_name: String,

    pub dest_station_id:    u64,
    pub dest_system_id:     u32,
    pub dest_system_name:   String,

    pub quantity:           u32,
    pub volume_used:        f64,   // m³ for that quantity
    pub capital_required:   f64,   // ISK to buy that quantity

    pub gross_profit:       f64,
    pub taxes_and_fees:     f64,
    pub net_profit:         f64,

    pub jumps_from_home:    u32,   // home → source
    pub jumps_source_dest:  u32,   // source → destination
    pub total_jumps:        u32,

    pub profit_per_jump:    f64,   // net_profit / total_jumps  ← primary sort key
    pub profit_per_m3:      f64,   // net_profit / volume_used
}
```

---

## 5. TradeRoute

A planned multi-stop route chaining several deals.

```rust
#[derive(Debug, Clone)]
pub struct TradeRoute {
    pub deals:         Vec<ArbitrageDeal>,
    pub waypoints:     Vec<u32>,   // ordered system IDs
    pub total_jumps:   u32,
    pub total_profit:  f64,
    pub total_volume:  f64,        // peak volume committed at any point
    pub total_capital: f64,        // peak ISK committed at any point
}
```

---

## 6. UserConfig

Persistent settings stored in `~/.config/eve-trade-scout/config.toml`.

```rust
#[derive(Debug, Deserialize, Serialize)]
pub struct UserConfig {
    pub ship:     ShipConfig,
    pub location: LocationConfig,
    pub scan:     ScanConfig,
    pub filters:  FilterConfig,
    pub fees:     FeeConfig,
    pub cache:    CacheConfig,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ShipConfig {
    pub profile: String,           // "big" | "small"
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LocationConfig {
    pub home_system: String,       // e.g. "Jita"
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ScanConfig {
    pub regions:         Vec<u32>,
    pub safe_route_only: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FilterConfig {
    pub min_profit_isk: f64,
    pub min_spread_pct: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FeeConfig {
    pub broker_fee_pct: f64,
    pub sales_tax_pct:  f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CacheConfig {
    pub directory:        String,
    pub sde_max_age_days: u32,
}
```

---

## 7. ESI Cache Entry

Stored on disk (one JSON file per endpoint family) for ETag-based conditional requests.

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct CacheEntry {
    pub url:     String,
    pub etag:    String,
    pub expires: DateTime<Utc>,
    pub data:    serde_json::Value,
}
```

---

## 8. Graph Node

Stored internally in the `petgraph` graph. Each node carries this as its weight.

```rust
#[derive(Debug, Clone)]
pub struct SystemNode {
    pub system_id:       u32,
    pub name:            String,
    pub security_status: f64,
    pub region_id:       u32,
}
```

---

## Key Relationships

```
UserConfig
    └── ShipConfig → resolves to ShipProfile (BIG_HAULER | SMALL_HAULER)
    └── LocationConfig.home_system → system_id

MarketOrder (sell) ──────────┐
                              ├──→ ArbitrageDeal
MarketOrder (buy)  ──────────┘
    └── uses ItemInfo.packaged_volume for quantity
    └── uses petgraph Dijkstra for jump counts

Vec<ArbitrageDeal> ──────────→ TradeRoute
```

---

## Type Safety Tips

Use Rust **newtypes** for domain values to prevent mixing them up:

```rust
pub struct Isk(f64);
pub struct CubicMeters(f64);
pub struct SystemId(u32);
pub struct TypeId(u32);
```

This ensures the compiler catches `profit_per_jump: CubicMeters` mistakes at compile time.
