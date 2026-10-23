use crate::config::ShipProfile;
use crate::esi::market::RegionMarket;
use crate::esi::models::MarketOrder;
use crate::graph::SystemGraph;
use crate::sde::{ItemInfo, SdeLoader};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const MAX_STATIONS_PER_TYPE: usize = 50;
pub const MAX_DEALS_COLLECTED: usize = 50_000;
pub const MAX_MANIFEST_CANDIDATE_DEALS: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageDeal {
    pub type_id: u32,
    pub item_name: String,
    pub source_order_id: u64,
    pub dest_order_id: u64,
    pub source_order_volume_remain: u32,
    pub dest_order_volume_remain: u32,
    #[serde(default)]
    pub dest_order_min_volume: u32,
    #[serde(default)]
    pub is_npc_trade: bool,

    pub buy_price: f64,  // Price at source (player buys from sell order)
    pub sell_price: f64, // Price at destination (player sells into buy order)
    pub spread_pct: f64, // (sell_price - buy_price) / buy_price * 100

    pub source_station_id: u64,
    pub source_system_id: u32,
    pub source_system_name: String,
    pub source_station_name: String,
    pub source_security: f64,

    pub dest_station_id: u64,
    pub dest_system_id: u32,
    pub dest_system_name: String,
    pub dest_station_name: String,
    pub dest_security: f64,

    pub quantity: u32,
    pub volume_used: f64,
    pub capital_required: f64,

    pub gross_profit: f64,
    pub taxes_and_fees: f64,
    pub net_profit: f64,
    pub net_profit_pct: f64, // Net profitability / ROI % after all taxes and fees

    pub jumps_from_home: u32,
    pub jumps_source_dest: u32,
    pub total_jumps: u32,

    pub profit_per_jump: f64, // net_profit / total_jumps
    pub profit_per_m3: f64,
}

impl ArbitrageDeal {
    pub fn is_high_roi(&self) -> bool {
        self.net_profit_pct >= 40.0
    }
}

/// A consolidated cargo manifest grouping multiple profitable deals along an identical station-to-station corridor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeCorridorManifest {
    pub source_system_id: u32,
    pub source_system_name: String,
    pub source_station_name: String,
    pub source_security: f64,
    pub dest_system_id: u32,
    pub dest_system_name: String,
    pub dest_station_name: String,
    pub dest_security: f64,
    pub deals: Vec<ArbitrageDeal>,
    pub total_volume_used: f64,
    pub total_capital_required: f64,
    pub total_net_profit: f64,
    pub overall_roi_pct: f64,
    pub jumps: u32,
    pub profit_per_jump: f64,
}

/// Configuration parameters and filters for the cross-station arbitrage engine.
#[derive(Debug, Clone)]
pub struct ArbitrageConfig {
    pub ship: ShipProfile,
    pub home_system: u32,
    pub min_profit: f64,
    pub min_spread: f64,
    pub min_net_profit_pct: f64, // Minimum profitability % after taxes (e.g. 10.0%)
    pub broker_fee: f64,
    pub sales_tax: f64,
    pub safe_only: bool,
    pub npc_stations_only: bool,
}

/// Identifies whether a given location ID corresponds to an NPC station.
///
/// In EVE Online, NPC stations have positive location IDs below the Upwell Structure threshold
/// of 1,000,000,000,000 (`10^12`). Location IDs $\ge 10^{12}$ denote player-owned Citadels / Engineering Complexes.
pub fn is_npc_station(location_id: u64) -> bool {
    // NPC stations have positive IDs below the Upwell Structure threshold (1_000_000_000_000)
    location_id > 0 && location_id < 1_000_000_000_000
}

/// Scans regional order books for profitable cross-station arbitrage opportunities.
///
/// Filters candidate trades by:
/// - Volume and capital limits defined in [`ArbitrageConfig::ship`]
/// - Minimum gross price spread and net profit thresholds
/// - Enforced minimum after-tax net profitability ($\ge 10.0\%$ ROI)
/// - Safe highsec routing ($\ge 0.45$ security status)
/// - Optional NPC stations only restriction
///
/// Returns a list of [`ArbitrageDeal`] sorted by `profit_per_jump` descending, breaking ties by `net_profit` descending.
pub fn find_deals(
    markets: &HashMap<u32, RegionMarket>,
    sde: &SdeLoader,
    graph: &SystemGraph,
    cfg: &ArbitrageConfig,
) -> Vec<ArbitrageDeal> {
    // 1. Find cheapest sell order and highest buy order per (type_id, station_id) directly
    let mut best_sells_by_type_station: HashMap<(u32, u64), &MarketOrder> =
        HashMap::with_capacity(4096);
    let mut best_buys_by_type_station: HashMap<(u32, u64), &MarketOrder> =
        HashMap::with_capacity(4096);

    for market in markets.values() {
        for (&type_id, orders) in &market.sell_orders {
            for o in orders {
                // Strictly require positive finite prices to prevent order-book poisoning
                if !o.price.is_finite() || o.price <= 0.0 {
                    continue;
                }
                // Strictly exclude lowsec and nullsec systems: only highsec allowed
                if !graph.is_highsec(o.system_id) {
                    continue;
                }
                if !cfg.npc_stations_only || is_npc_station(o.location_id) {
                    best_sells_by_type_station
                        .entry((type_id, o.location_id))
                        .and_modify(|best| {
                            if o.price < best.price {
                                *best = o;
                            }
                        })
                        .or_insert(o);
                }
            }
        }

        for (&type_id, orders) in &market.buy_orders {
            for o in orders {
                // Strictly require positive finite prices to prevent order-book poisoning
                if !o.price.is_finite() || o.price <= 0.0 {
                    continue;
                }
                // Strictly exclude lowsec and nullsec systems: only highsec allowed
                if !graph.is_highsec(o.system_id) {
                    continue;
                }
                if !cfg.npc_stations_only || is_npc_station(o.location_id) {
                    best_buys_by_type_station
                        .entry((type_id, o.location_id))
                        .and_modify(|best| {
                            if o.price > best.price {
                                *best = o;
                            }
                        })
                        .or_insert(o);
                }
            }
        }
    }

    // 2. Group deduplicated station bests by type_id and bound station cross-product
    let mut sells_by_type: HashMap<u32, Vec<&MarketOrder>> = HashMap::with_capacity(1024);
    for ((type_id, _), order) in best_sells_by_type_station {
        sells_by_type.entry(type_id).or_default().push(order);
    }
    for list in sells_by_type.values_mut() {
        list.sort_unstable_by(|a, b| {
            a.price
                .partial_cmp(&b.price)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        list.truncate(MAX_STATIONS_PER_TYPE);
    }

    let mut buys_by_type: HashMap<u32, Vec<&MarketOrder>> = HashMap::with_capacity(1024);
    for ((type_id, _), order) in best_buys_by_type_station {
        buys_by_type.entry(type_id).or_default().push(order);
    }
    for list in buys_by_type.values_mut() {
        list.sort_unstable_by(|a, b| {
            b.price
                .partial_cmp(&a.price)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        list.truncate(MAX_STATIONS_PER_TYPE);
    }

    let mut deals = Vec::with_capacity(2048);

    // 3. For each type_id that exists in both buys and sells
    for (&type_id, sell_list) in &sells_by_type {
        let buy_list = match buys_by_type.get(&type_id) {
            Some(b) => b,
            None => continue,
        };

        let item_info = match sde.get_item_info(type_id) {
            Some(info) if info.published => info,
            _ => continue,
        };

        let mut hit_ceiling = false;

        // Test all (source_station, dest_station) pairs
        for &src_order in sell_list {
            // Early prune: skip source stations unreachable from home in safe mode
            if cfg.safe_only && graph.jump_count(cfg.home_system, src_order.system_id, true) >= 9999
            {
                continue;
            }

            for &dst_order in buy_list {
                // Must be cross-station trade (not same station)
                if src_order.location_id == dst_order.location_id {
                    continue;
                }

                // Must have positive spread
                if dst_order.price <= src_order.price {
                    continue;
                }

                let spread_pct = ((dst_order.price - src_order.price) / src_order.price) * 100.0;
                if spread_pct < cfg.min_spread {
                    continue;
                }

                if let Some(deal) =
                    calculate_deal(type_id, &item_info, src_order, dst_order, cfg, sde, graph)
                {
                    deals.push(deal);
                    if deals.len() >= MAX_DEALS_COLLECTED {
                        tracing::warn!(
                            max_deals = MAX_DEALS_COLLECTED,
                            "Arbitrage candidate deal collection reached defensive safety ceiling; truncating search"
                        );
                        hit_ceiling = true;
                        break;
                    }
                }
            }
            if hit_ceiling {
                break;
            }
        }
        if hit_ceiling {
            break;
        }
    }

    // 4. Sort deals primarily by profit_per_jump DESC, then net_profit DESC
    deals.sort_by(|a, b| {
        b.profit_per_jump
            .partial_cmp(&a.profit_per_jump)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.net_profit
                    .partial_cmp(&a.net_profit)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    deals
}

/// Calculates the trade financials for a single candidate deal between a source sell order and destination buy order.
///
/// Returns `None` if:
/// - Source or destination station are identical
/// - Prices are non-positive, NaN, or infinite
/// - Either system is lowsec/nullsec (when highsec safety is enforced)
/// - Stations violate the NPC station filter
/// - Origin or destination systems are unreachable
/// - Quantity solves to 0 units (e.g. item cannot fit ship cargo or exceeds budget)
/// - Quantity does not satisfy buyer's `min_volume` requirement
/// - Net profit is below `cfg.min_profit`
/// - Net ROI percentage is below `cfg.min_net_profit_pct` (enforced minimum 10.0%)
pub fn calculate_deal(
    type_id: u32,
    item: &ItemInfo,
    src_order: &MarketOrder,
    dst_order: &MarketOrder,
    cfg: &ArbitrageConfig,
    sde: &SdeLoader,
    graph: &SystemGraph,
) -> Option<ArbitrageDeal> {
    let buy_price = src_order.price;
    let sell_price = dst_order.price;

    // Validate prices are positive and finite (reject NaN, Infinity, zero, negative)
    if !buy_price.is_finite() || !sell_price.is_finite() || buy_price <= 0.0 || sell_price <= 0.0 {
        return None;
    }

    // Validate config parameters and item dimensions are finite and valid
    if !cfg.broker_fee.is_finite()
        || cfg.broker_fee < 0.0
        || !cfg.sales_tax.is_finite()
        || cfg.sales_tax < 0.0
        || !cfg.ship.max_volume.is_finite()
        || cfg.ship.max_volume <= 0.0
        || !cfg.ship.max_budget.is_finite()
        || cfg.ship.max_budget <= 0.0
        || !item.packaged_volume.is_finite()
        || item.packaged_volume <= 0.0
        || !cfg.min_profit.is_finite()
        || !cfg.min_net_profit_pct.is_finite()
    {
        return None;
    }

    let src_system_id = src_order.system_id;
    let dst_system_id = dst_order.system_id;
    let src_station = src_order.location_id;
    let dst_station = dst_order.location_id;

    // Must be cross-station trade (not same station)
    if src_station == dst_station {
        return None;
    }

    // Strictly exclude lowsec and nullsec: only highsec systems allowed
    if !graph.is_highsec(src_system_id) || !graph.is_highsec(dst_system_id) {
        return None;
    }

    // Station preference: if npc_stations_only is enabled, both stations must be NPC stations
    if cfg.npc_stations_only && (!is_npc_station(src_station) || !is_npc_station(dst_station)) {
        return None;
    }

    // Quantity constraints:
    // 1. Cargo volume limit
    let max_by_volume = if item.packaged_volume > 0.0 {
        (cfg.ship.max_volume / item.packaged_volume).floor() as u32
    } else {
        0
    };

    // 2. Budget limit
    let max_by_budget = if buy_price > 0.0 {
        (cfg.ship.max_budget / buy_price).floor() as u32
    } else {
        0
    };

    // 3. Available supply at source
    let max_by_supply = src_order.volume_remain;

    // 4. Available demand at destination
    let max_by_demand = dst_order.volume_remain;

    let quantity = max_by_volume
        .min(max_by_budget)
        .min(max_by_supply)
        .min(max_by_demand);

    if quantity == 0 {
        return None;
    }

    // Enforce buy order min_volume requirement:
    // In EVE Online, if a buyer set min_volume > 1, sales of fewer units are rejected
    if quantity < dst_order.min_volume {
        return None;
    }

    let volume_used = (quantity as f64) * item.packaged_volume;
    let capital_required = (quantity as f64) * buy_price;

    // Fees:
    // broker fee on buy (conservative) + sales tax on sell
    let broker_fee_amount = capital_required * (cfg.broker_fee / 100.0);
    let gross_revenue = (quantity as f64) * sell_price;
    let sales_tax_amount = gross_revenue * (cfg.sales_tax / 100.0);
    let total_taxes_and_fees = broker_fee_amount + sales_tax_amount;

    let gross_profit = gross_revenue - capital_required;
    let net_profit = gross_profit - total_taxes_and_fees;

    if net_profit < cfg.min_profit {
        return None;
    }

    let net_profit_pct = if capital_required > 0.0 {
        (net_profit / capital_required) * 100.0
    } else {
        0.0
    };

    // Enforce minimum overall profitability percentage after taxes
    // Mandatory domain invariant: Deals must achieve at least 10.0% net profit after taxes and fees
    let effective_min_roi = cfg.min_net_profit_pct.max(10.0);
    if net_profit_pct < effective_min_roi {
        return None;
    }

    // Jump calculations: evaluate graph distances only for viable profitable deals
    let jumps_from_home = graph.jump_count(cfg.home_system, src_system_id, cfg.safe_only);
    if cfg.safe_only && jumps_from_home >= 9999 {
        return None;
    }

    let jumps_source_dest = graph.jump_count(src_system_id, dst_system_id, cfg.safe_only);
    if cfg.safe_only && jumps_source_dest >= 9999 {
        return None;
    }

    let total_jumps = (jumps_from_home + jumps_source_dest).max(1);

    let spread_pct = ((sell_price - buy_price) / buy_price) * 100.0;
    let profit_per_jump = net_profit / (total_jumps as f64);
    let profit_per_m3 = if volume_used > 0.0 {
        net_profit / volume_used
    } else {
        0.0
    };

    let src_system_name = graph
        .system_name(src_system_id)
        .map(|s| s.to_string())
        .or_else(|| sde.get_system_name(src_system_id))
        .unwrap_or_else(|| format!("System #{}", src_system_id));

    let dst_system_name = graph
        .system_name(dst_system_id)
        .map(|s| s.to_string())
        .or_else(|| sde.get_system_name(dst_system_id))
        .unwrap_or_else(|| format!("System #{}", dst_system_id));

    let src_station_name = sde.get_station_name(src_station).unwrap_or_else(|| {
        if is_npc_station(src_station) {
            format!("Station #{}", src_station)
        } else {
            format!("Citadel #{} ({})", src_station, src_system_name)
        }
    });

    let dst_station_name = sde.get_station_name(dst_station).unwrap_or_else(|| {
        if is_npc_station(dst_station) {
            format!("Station #{}", dst_station)
        } else {
            format!("Citadel #{} ({})", dst_station, dst_system_name)
        }
    });

    let src_security = graph.security(src_system_id).unwrap_or(1.0);
    let dst_security = graph.security(dst_system_id).unwrap_or(1.0);

    // NPC trade detection:
    // 1. Either order is an NPC seeded order (duration == 365 days in ESI, while player orders max out at 90 days).
    // 2. The item is an NPC trade commodity (groups 280-284: Wheat, Spirits, Dairy Products, etc.).
    // Player-to-player trades for insignias, tags, datacores, components, and PI have duration <= 90 and evaluate to false.
    let is_npc_trade =
        src_order.duration == 365 || dst_order.duration == 365 || item.is_npc_trade_commodity();

    Some(ArbitrageDeal {
        type_id,
        item_name: item.name.clone(),
        source_order_id: src_order.order_id,
        dest_order_id: dst_order.order_id,
        source_order_volume_remain: src_order.volume_remain,
        dest_order_volume_remain: dst_order.volume_remain,
        dest_order_min_volume: dst_order.min_volume,
        is_npc_trade,
        buy_price,
        sell_price,
        spread_pct,
        source_station_id: src_station,
        source_system_id: src_system_id,
        source_system_name: src_system_name,
        source_station_name: src_station_name,
        source_security: src_security,
        dest_station_id: dst_station,
        dest_system_id: dst_system_id,
        dest_system_name: dst_system_name,
        dest_station_name: dst_station_name,
        dest_security: dst_security,
        quantity,
        volume_used,
        capital_required,
        gross_profit,
        taxes_and_fees: total_taxes_and_fees,
        net_profit,
        net_profit_pct,
        jumps_from_home,
        jumps_source_dest,
        total_jumps,
        profit_per_jump,
        profit_per_m3,
    })
}

/// Consolidates individual arbitrage deals sharing identical station-to-station corridors into packed multi-item cargo manifests.
///
/// Uses knapsack value-density packing (`profit_per_m3` descending) to maximize total profit while respecting
/// ship volume and capital budget constraints.
pub fn bundle_route_cargo(
    deals: &[ArbitrageDeal],
    ship: ShipProfile,
) -> Vec<TradeCorridorManifest> {
    let candidate_deals = if deals.len() > MAX_MANIFEST_CANDIDATE_DEALS {
        &deals[..MAX_MANIFEST_CANDIDATE_DEALS]
    } else {
        deals
    };

    // Group deals by (source_station_id, dest_station_id)
    let mut by_corridor: HashMap<(u64, u64), Vec<&ArbitrageDeal>> = HashMap::new();
    for deal in candidate_deals {
        by_corridor
            .entry((deal.source_station_id, deal.dest_station_id))
            .or_default()
            .push(deal);
    }

    let mut manifests = Vec::new();

    for ((_src_st, _dst_st), mut corridor_deals) in by_corridor {
        // Sort deals inside the corridor by profit_per_m3 DESC to pack the highest value density items
        corridor_deals.sort_by(|a, b| {
            b.profit_per_m3
                .partial_cmp(&a.profit_per_m3)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut remaining_vol = ship.max_volume;
        let mut remaining_budget = ship.max_budget;
        let mut packed_deals = Vec::new();
        let mut total_vol = 0.0;
        let mut total_cap = 0.0;
        let mut total_profit = 0.0;
        let mut used_orders: std::collections::HashSet<u64> = std::collections::HashSet::new();
        let mut used_types: std::collections::HashSet<u32> = std::collections::HashSet::new();

        for deal in corridor_deals {
            if deal.volume_used <= remaining_vol && deal.capital_required <= remaining_budget {
                if deal.source_order_id > 0 && used_orders.contains(&deal.source_order_id) {
                    continue;
                }
                if used_types.contains(&deal.type_id) {
                    continue;
                }
                packed_deals.push((*deal).clone());
                if deal.source_order_id > 0 {
                    used_orders.insert(deal.source_order_id);
                }
                used_types.insert(deal.type_id);
                remaining_vol -= deal.volume_used;
                remaining_budget -= deal.capital_required;
                total_vol += deal.volume_used;
                total_cap += deal.capital_required;
                total_profit += deal.net_profit;
            }
        }

        if packed_deals.is_empty() {
            continue;
        }

        let first = &packed_deals[0];
        let total_jumps = first.total_jumps.max(1);
        let ppj = total_profit / (total_jumps as f64);
        let roi = if total_cap > 0.0 {
            (total_profit / total_cap) * 100.0
        } else {
            0.0
        };

        manifests.push(TradeCorridorManifest {
            source_system_id: first.source_system_id,
            source_system_name: first.source_system_name.clone(),
            source_station_name: first.source_station_name.clone(),
            source_security: first.source_security,
            dest_system_id: first.dest_system_id,
            dest_system_name: first.dest_system_name.clone(),
            dest_station_name: first.dest_station_name.clone(),
            dest_security: first.dest_security,
            deals: packed_deals,
            total_volume_used: total_vol,
            total_capital_required: total_cap,
            total_net_profit: total_profit,
            overall_roi_pct: roi,
            jumps: total_jumps,
            profit_per_jump: ppj,
        });
    }

    // Rank manifests by profit_per_jump DESC
    manifests.sort_by(|a, b| {
        b.profit_per_jump
            .partial_cmp(&a.profit_per_jump)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.total_net_profit
                    .partial_cmp(&a.total_net_profit)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    manifests
}

/// Neutralize CSV Formula Injection (CWE-1236) and CRLF injection.
/// If a cell starts with formula trigger characters (`=`, `+`, `-`, `@`, `|`, `%`, `\t`, `\r`),
/// prepends a single quote `'` so spreadsheet applications treat it strictly as literal text.
/// Strips raw newline characters (`\r`, `\n`) to prevent CSV line injection, and escapes internal double quotes (`"` -> `""`).
pub fn sanitize_csv_cell(val: &str) -> String {
    let clean = val.replace(['\r', '\n'], " ");
    let trimmed = clean.trim_start();
    let escaped = if trimmed.starts_with('=')
        || trimmed.starts_with('+')
        || trimmed.starts_with('-')
        || trimmed.starts_with('@')
        || trimmed.starts_with('|')
        || trimmed.starts_with('%')
        || trimmed.starts_with('\t')
        || trimmed.starts_with('\r')
    {
        format!("'{}", clean)
    } else {
        clean
    };
    escaped.replace('"', "\"\"")
}

/// Prunes redundant order pairings by limiting the number of deals per unique sell order and buy order.
///
/// Input `deals` are evaluated in priority order (typically `profit_per_jump DESC`,
/// with ties broken by fewest `total_jumps` and highest `net_profit`).
///
/// - For each unique sell order (`source_order_id`), retains at most `max_per_sell_order` destinations.
/// - For each unique buy order (`dest_order_id`), retains at most `max_per_buy_order` sources.
///
/// If a limit is `0`, that side is unrestricted (unlimited).
/// When both limits are `0`, the input deals are returned unmodified.
pub fn filter_deals_by_order_limit<T, F>(
    deals: impl IntoIterator<Item = T>,
    max_per_sell_order: usize,
    max_per_buy_order: usize,
    get_deal: F,
) -> Vec<T>
where
    F: Fn(&T) -> &ArbitrageDeal,
{
    if max_per_sell_order == 0 && max_per_buy_order == 0 {
        return deals.into_iter().collect();
    }

    let mut sell_order_counts: HashMap<u64, usize> = HashMap::new();
    let mut buy_order_counts: HashMap<u64, usize> = HashMap::new();
    let mut retained = Vec::new();

    for item in deals {
        let deal = get_deal(&item);

        // Unique identifier for sell order:
        // Use source_order_id if present; fallback to composite (type_id, source_station_id) if order_id is 0
        let sell_id = if deal.source_order_id > 0 {
            deal.source_order_id
        } else {
            ((deal.type_id as u64) << 32) | (deal.source_station_id & 0xFFFF_FFFF)
        };

        // Unique identifier for buy order:
        // Use dest_order_id if present; fallback to composite (type_id, dest_station_id) if order_id is 0
        let buy_id = if deal.dest_order_id > 0 {
            deal.dest_order_id
        } else {
            ((deal.type_id as u64) << 32) | (deal.dest_station_id & 0xFFFF_FFFF)
        };

        if max_per_sell_order > 0 {
            let count = sell_order_counts.get(&sell_id).copied().unwrap_or(0);
            if count >= max_per_sell_order {
                continue;
            }
        }

        if max_per_buy_order > 0 {
            let count = buy_order_counts.get(&buy_id).copied().unwrap_or(0);
            if count >= max_per_buy_order {
                continue;
            }
        }

        *sell_order_counts.entry(sell_id).or_default() += 1;
        *buy_order_counts.entry(buy_id).or_default() += 1;
        retained.push(item);
    }

    retained
}

/// Convenience function for slicing and filtering ArbitrageDeal vectors directly.
pub fn prune_redundant_order_deals(
    deals: &[ArbitrageDeal],
    max_per_sell: usize,
    max_per_buy: usize,
) -> Vec<ArbitrageDeal> {
    filter_deals_by_order_limit(deals.iter().cloned(), max_per_sell, max_per_buy, |d| d)
}

/// Abstraction for route entities that yield an ordered sequence of EVE station IDs for autopilot injection.
pub trait AutopilotNavigable {
    fn ordered_destination_ids(&self) -> Vec<u64>;
}

/// Appends a destination ID to a sequence if it differs from the last destination (deduplication).
#[inline]
pub fn push_dedup_destination(ids: &mut Vec<u64>, dest_id: u64) {
    if ids.last().copied() != Some(dest_id) {
        ids.push(dest_id);
    }
}

impl AutopilotNavigable for ArbitrageDeal {
    fn ordered_destination_ids(&self) -> Vec<u64> {
        let mut ids = Vec::with_capacity(2);
        push_dedup_destination(&mut ids, self.source_station_id);
        push_dedup_destination(&mut ids, self.dest_station_id);
        ids
    }
}

impl ArbitrageDeal {
    /// Returns the station destination IDs for pushing to EVE Autopilot HUD.
    pub fn ordered_destination_ids(&self) -> Vec<u64> {
        <Self as AutopilotNavigable>::ordered_destination_ids(self)
    }

    /// Formats a clean flight sheet for EVE Notepad (Alt+N).
    pub fn format_eve_flight_sheet(&self) -> String {
        let mut s = String::new();
        s.push_str("═══════════════════════════════════════════════════════════\n");
        s.push_str(&format!("🚀 EVE TRADE FLIGHT SHEET: {}\n", self.item_name));
        s.push_str(&format!(
            "Net Profit: +{:.2}M ISK | Net ROI: +{:.1}% | Distance: {} Jumps\n",
            self.net_profit / 1_000_000.0,
            self.net_profit_pct,
            self.total_jumps
        ));
        s.push_str("═══════════════════════════════════════════════════════════\n\n");
        s.push_str(&format!(
            "▶ 1. BUY AT: {} ({})\n",
            self.source_station_name, self.source_system_name
        ));
        s.push_str(&format!(
            "     Quantity: {} units | Buy Price: {:.2} ISK | Capital: {:.2}M ISK | Vol: {:.1} m³\n\n",
            self.quantity,
            self.buy_price,
            self.capital_required / 1_000_000.0,
            self.volume_used
        ));
        s.push_str(&format!(
            "▶ 2. SELL AT: {} ({})\n",
            self.dest_station_name, self.dest_system_name
        ));
        s.push_str(&format!(
            "     Sell Price: {:.2} ISK | Revenue: {:.2}M ISK | Net Return: +{:.2}M ISK\n",
            self.sell_price,
            (self.sell_price * self.quantity as f64) / 1_000_000.0,
            self.net_profit / 1_000_000.0
        ));
        s.push_str("═══════════════════════════════════════════════════════════\n");
        s
    }
}

impl AutopilotNavigable for TradeCorridorManifest {
    fn ordered_destination_ids(&self) -> Vec<u64> {
        let mut ids = Vec::with_capacity(2);
        if let Some(first) = self.deals.first() {
            push_dedup_destination(&mut ids, first.source_station_id);
            push_dedup_destination(&mut ids, first.dest_station_id);
        }
        ids
    }
}

impl TradeCorridorManifest {
    /// Returns the station destination IDs for pushing to EVE Autopilot HUD.
    pub fn ordered_destination_ids(&self) -> Vec<u64> {
        <Self as AutopilotNavigable>::ordered_destination_ids(self)
    }

    /// Formats a clean flight sheet for EVE Notepad (Alt+N).
    pub fn format_eve_flight_sheet(&self, idx: usize) -> String {
        let mut s = String::new();
        s.push_str("═══════════════════════════════════════════════════════════\n");
        s.push_str(&format!(
            "📦 EVE CORRIDOR MANIFEST #{}: {} ➔ {}\n",
            idx, self.source_system_name, self.dest_system_name
        ));
        s.push_str(&format!(
            "Total Profit: +{:.2}M ISK | Net ROI: +{:.1}% | Distance: {} Jumps\n",
            self.total_net_profit / 1_000_000.0,
            self.overall_roi_pct,
            self.jumps
        ));
        s.push_str(&format!(
            "Volume: {:.1} m³ | Capital: {:.2}M ISK | Items: {} distinct deals\n",
            self.total_volume_used,
            self.total_capital_required / 1_000_000.0,
            self.deals.len()
        ));
        s.push_str("═══════════════════════════════════════════════════════════\n\n");
        s.push_str(&format!(
            "▶ 1. BUY AT: {} ({})\n",
            self.source_station_name, self.source_system_name
        ));
        for (i, d) in self.deals.iter().enumerate() {
            s.push_str(&format!(
                "  [{}] {}x {} @ {:.2} ISK (Cost: {:.2}M ISK, {:.1} m³)\n",
                i + 1,
                d.quantity,
                d.item_name,
                d.buy_price,
                d.capital_required / 1_000_000.0,
                d.volume_used
            ));
        }
        s.push('\n');
        s.push_str(&format!(
            "▶ 2. SELL AT: {} ({})\n",
            self.dest_station_name, self.dest_system_name
        ));
        for (i, d) in self.deals.iter().enumerate() {
            s.push_str(&format!(
                "  [{}] SELL {}x {} @ {:.2} ISK (Profit: +{:.2}M ISK, {:.1}%)\n",
                i + 1,
                d.quantity,
                d.item_name,
                d.sell_price,
                d.net_profit / 1_000_000.0,
                d.net_profit_pct
            ));
        }
        s.push_str("═══════════════════════════════════════════════════════════\n");
        s
    }
}

/// Filters deals based on NPC trade status.
/// When `include_npc` is true, all deals are retained.
/// When `include_npc` is false, deals flagged with `is_npc_trade == true` are excluded.
pub fn filter_deals_by_npc_status(
    deals: &[ArbitrageDeal],
    include_npc: bool,
) -> Vec<&ArbitrageDeal> {
    deals
        .iter()
        .filter(|d| include_npc || !d.is_npc_trade)
        .collect()
}
