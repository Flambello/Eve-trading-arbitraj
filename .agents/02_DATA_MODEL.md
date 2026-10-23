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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketOrder {
    pub order_id:      u64,
    pub type_id:       u32,
    pub location_id:   u64,   // station or structure ID
    pub system_id:     u32,
    pub volume_total:  u32,
    pub volume_remain: u32,
    pub min_volume:    u32,
    pub price:         f64,   // ISK per unit
    pub is_buy_order:  bool,
    pub duration:      u32,
    pub issued:        Option<String>,
    pub range:         Option<String>,
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
    pub category_id:     u32,
    pub published:       bool,
}

impl ItemInfo {
    pub fn is_npc_trade_commodity(&self) -> bool;
}
```

---

## 4. ArbitrageDeal

A single profitable opportunity: buy at station A, sell at station B.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageDeal {
    pub type_id:                    u32,
    pub item_name:                  String,
    pub source_order_id:            u64,
    pub dest_order_id:              u64,
    pub source_order_volume_remain: u32,
    pub dest_order_volume_remain:   u32,
    pub dest_order_min_volume:      u32,
    pub is_npc_trade:               bool,

    pub buy_price:                  f64,   // cheapest sell order price at source (you pay this)
    pub sell_price:                 f64,   // highest buy order price at dest (you receive this)
    pub spread_pct:                 f64,   // (sell - buy) / buy * 100

    pub source_station_id:          u64,
    pub source_system_id:           u32,
    pub source_system_name:         String,
    pub source_station_name:        String,
    pub source_security:            f64,

    pub dest_station_id:            u64,
    pub dest_system_id:             u32,
    pub dest_system_name:           String,
    pub dest_station_name:          String,
    pub dest_security:              f64,

    pub quantity:                   u32,
    pub volume_used:                f64,   // m³ for that quantity
    pub capital_required:           f64,   // ISK to buy that quantity

    pub gross_profit:               f64,
    pub taxes_and_fees:             f64,
    pub net_profit:                 f64,
    pub net_profit_pct:             f64,   // (net_profit / capital_required) * 100 (enforced >= 10.0%)

    pub jumps_from_home:            u32,   // home → source
    pub jumps_source_dest:          u32,   // source → destination
    pub total_jumps:                u32,

    pub profit_per_jump:            f64,   // net_profit / total_jumps  ← primary sort key
    pub profit_per_m3:              f64,   // net_profit / volume_used
}

impl ArbitrageDeal {
    pub fn is_high_roi(&self) -> bool; // net_profit_pct >= 40.0%
}
```

---

## 5. TradeRoute & RouteStop

A planned multi-stop route chaining several deals sequentially.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteStop {
    pub step:              usize,
    pub deal:              ArbitrageDeal,
    pub jumps_from_prev:   u32,
    pub prev_system_id:    u32,
    pub prev_system_name:  String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRoute {
    pub stops:                  Vec<RouteStop>,
    pub waypoints:              Vec<u32>,        // ordered system IDs
    pub waypoint_names:         Vec<String>,     // ordered system names
    pub total_jumps:            u32,
    pub total_profit:           f64,
    pub total_capital_required: f64,             // peak capital committed
    pub max_volume_used:        f64,             // peak volume committed
    pub avg_profit_per_jump:    f64,
}
```

---

## 6. UserConfig

Persistent settings stored at `~/.config/eve-traveling-merchant/config.toml` (with legacy fallback to `~/.config/eve-trade-scout/config.toml`).

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserConfig {
    pub ship:     ShipConfig,
    pub location: LocationConfig,
    pub scan:     ScanConfig,
    pub filters:  FilterConfig,
    pub fees:     FeeConfig,
    pub cache:    CacheConfig,
    pub sso:      SsoConfig,
    pub theme:    AppTheme,
    pub ui:       UiConfig,
    pub view:     ViewConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ShipConfig {
    pub profile:           String, // "big" | "small"
    pub use_custom_ship:   bool,
    pub custom_cargo_vol:  f64,
    pub custom_budget_isk: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LocationConfig {
    pub home_system: String,       // e.g. "Jita"
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScanConfig {
    pub regions:               Vec<u32>,
    pub safe_route_only:       bool,
    pub npc_stations_only:     bool,   // Default: true (restrict search strictly to NPC stations vs allow player citadels)
    pub scan_adjacent_regions: bool,   // Default: true (scan stargate-connected neighbor regions)
    pub auto_refresh:          bool,   // Default: false (auto-refresh every 5 minutes in GUI)
    pub sound_alerts:          bool,   // Default: true (audible alerts for >= 100% ROI deals)
    pub sound_min_roi_pct:     f64,    // Default: 100.0%
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FilterConfig {
    pub min_profit_isk:     f64,
    pub min_spread_pct:     f64,
    pub min_net_profit_pct: f64,   // Default: 10.0% enforced minimum ROI after taxes
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FeeConfig {
    pub broker_fee_pct: f64,
    pub sales_tax_pct:  f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CacheConfig {
    pub directory:        String,
    pub sde_max_age_days: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SsoConfig {
    pub client_id:          String,  // Optional custom Client ID override; empty string uses DEFAULT_PUBLIC_CLIENT_ID
    pub callback_port:      u16,     // Loopback TCP port (default: 37373)
    pub auto_sync_location: bool,    // Auto-sync starting system from active character
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UiConfig {
    pub ui_scale:           f32,     // 0.75..=1.50
    pub is_fullscreen:      bool,
    pub dossier_side_panel: bool,
    pub compact_kpi:        bool,
    pub max_route_stops:    usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ViewConfig {
    pub hide_npc_trades:         bool,
    pub max_deals_per_order:     usize,
    pub min_profit_filter_m:     f64,
    pub max_jumps_filter:        u32,
    pub deal_sort_by:            String,
    pub deal_sort_descending:    bool,
    pub chain_max_legs:          usize,
    pub chain_max_dest_loads:    usize,
    pub chain_include_npc:       bool,
    pub chain_detour_enabled:    bool,
    pub chain_max_detour:        u32,
    pub hub_source_hub:          Option<String>,
    pub hub_dest_hub:            Option<String>,
    pub hub_include_npc:         bool,
    pub hub_major_stations_only: bool,
    pub hub_min_profit_m:        f64,
}
```

---

## 7. Autopilot Navigation (`AutopilotNavigable`) & Hubs (`CoreHub`)

```rust
/// Abstraction for route entities that yield an ordered sequence of EVE station IDs for autopilot injection.
pub trait AutopilotNavigable {
    fn ordered_destination_ids(&self) -> Vec<u64>;
}

// Implemented for: ArbitrageDeal, TradeCorridorManifest, TradeRoute, ChainRoute

/// The 5 major empire trade hubs in EVE Online.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CoreHub {
    Jita,
    Amarr,
    Dodixie,
    Rens,
    Hek,
}
```

### SSO Token & Local Storage (`SsoToken`)

Persisted to `~/.config/eve-traveling-merchant/sso_token.json` (Linux) with POSIX `0600` file / `0700` dir permissions, or `%APPDATA%\eve-traveling-merchant\sso_token.json` (Windows).

```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct SsoToken {
    pub access_token:   String,       // Masked as [REDACTED] in std::fmt::Debug
    pub refresh_token:  String,       // Masked as [REDACTED] in std::fmt::Debug
    pub token_type:     String,
    pub expires_at:     i64,          // Unix timestamp in seconds
    pub character_id:   u64,
    pub character_name: String,
    pub client_id:      Option<String>,
}
```

> **Security Note:** In PKCE public client OAuth2 (RFC 7636), no `client_secret` is ever compiled, requested, or persisted.

---

## 8. ESI Cache Entry (StoredEtag)

Stored on disk in `cache/etags.json` for ETag-based conditional HTTP GET requests.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEtag {
    pub etag:      String,
    pub expires:   Option<DateTime<Utc>>,
    pub body_json: String,
}
```

---

## 9. Graph Node

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
