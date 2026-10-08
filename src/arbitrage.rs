use crate::config::ShipProfile;
use crate::esi::market::RegionMarket;
use crate::esi::models::MarketOrder;
use crate::graph::SystemGraph;
use crate::sde::{ItemInfo, SdeLoader};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageDeal {
    pub type_id: u32,
    pub item_name: String,

    pub buy_price: f64,  // Price at source (player buys from sell order)
    pub sell_price: f64, // Price at destination (player sells into buy order)
    pub spread_pct: f64, // (sell_price - buy_price) / buy_price * 100

    pub source_station_id: u64,
    pub source_system_id: u32,
    pub source_system_name: String,
    pub source_station_name: String,

    pub dest_station_id: u64,
    pub dest_system_id: u32,
    pub dest_system_name: String,
    pub dest_station_name: String,

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

#[derive(Debug, Clone)]
pub struct ArbitrageConfig {
    pub ship: ShipProfile,
    pub home_system: u32,
    pub min_profit: f64,
    pub min_spread: f64,
    pub min_net_profit_pct: f64, // Minimum profitability % after taxes (e.g. 15.0%)
    pub broker_fee: f64,
    pub sales_tax: f64,
    pub safe_only: bool,
    pub npc_stations_only: bool,
}

pub fn is_npc_station(location_id: u64) -> bool {
    // Upwell structures have IDs > 1_000_000_000_000
    location_id < 1_000_000_000_000
}

pub fn find_deals(
    markets: &HashMap<u32, RegionMarket>,
    sde: &SdeLoader,
    graph: &SystemGraph,
    cfg: &ArbitrageConfig,
) -> Vec<ArbitrageDeal> {
    // 1. Group all sell orders and buy orders across the queried regions by type_id
    let mut all_sells: HashMap<u32, Vec<&MarketOrder>> = HashMap::new();
    let mut all_buys: HashMap<u32, Vec<&MarketOrder>> = HashMap::new();

    for market in markets.values() {
        for (&type_id, orders) in &market.sell_orders {
            let entry = all_sells.entry(type_id).or_default();
            for o in orders {
                // Strictly exclude lowsec and nullsec systems: only highsec allowed
                if !graph.is_highsec(o.system_id) {
                    continue;
                }
                if !cfg.npc_stations_only || is_npc_station(o.location_id) {
                    entry.push(o);
                }
            }
        }

        for (&type_id, orders) in &market.buy_orders {
            let entry = all_buys.entry(type_id).or_default();
            for o in orders {
                // Strictly exclude lowsec and nullsec systems: only highsec allowed
                if !graph.is_highsec(o.system_id) {
                    continue;
                }
                if !cfg.npc_stations_only || is_npc_station(o.location_id) {
                    entry.push(o);
                }
            }
        }
    }

    let mut deals = Vec::new();

    // 2. For each type_id that exists in both buys and sells
    for (&type_id, sell_list) in &all_sells {
        let buy_list = match all_buys.get(&type_id) {
            Some(b) => b,
            None => continue,
        };

        let item_info = match sde.get_item_info(type_id) {
            Some(info) if info.published => info,
            _ => continue,
        };

        // For this item, find best sell price per station
        let mut best_sell_by_station: HashMap<u64, &MarketOrder> = HashMap::new();
        for &order in sell_list {
            best_sell_by_station
                .entry(order.location_id)
                .and_modify(|best| {
                    if order.price < best.price {
                        *best = order;
                    }
                })
                .or_insert(order);
        }

        // And best buy price per station
        let mut best_buy_by_station: HashMap<u64, &MarketOrder> = HashMap::new();
        for &order in buy_list {
            best_buy_by_station
                .entry(order.location_id)
                .and_modify(|best| {
                    if order.price > best.price {
                        *best = order;
                    }
                })
                .or_insert(order);
        }

        // 3. Test all (source_station, dest_station) pairs
        for (&src_station, &src_order) in &best_sell_by_station {
            for (&dst_station, &dst_order) in &best_buy_by_station {
                // Must have positive spread
                if dst_order.price <= src_order.price {
                    continue;
                }

                let spread_pct =
                    ((dst_order.price - src_order.price) / src_order.price) * 100.0;
                if spread_pct < cfg.min_spread {
                    continue;
                }

                if let Some(deal) = calculate_deal(
                    type_id,
                    &item_info,
                    src_order,
                    dst_order,
                    src_station,
                    dst_station,
                    cfg,
                    sde,
                    graph,
                ) {
                    deals.push(deal);
                }
            }
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

pub fn calculate_deal(
    type_id: u32,
    item: &ItemInfo,
    src_order: &MarketOrder,
    dst_order: &MarketOrder,
    src_station: u64,
    dst_station: u64,
    cfg: &ArbitrageConfig,
    sde: &SdeLoader,
    graph: &SystemGraph,
) -> Option<ArbitrageDeal> {
    let buy_price = src_order.price;
    let sell_price = dst_order.price;

    let src_system_id = src_order.system_id;
    let dst_system_id = dst_order.system_id;

    // Strictly exclude lowsec and nullsec: only highsec systems allowed
    if !graph.is_highsec(src_system_id) || !graph.is_highsec(dst_system_id) {
        return None;
    }

    // Station preference: if npc_stations_only is enabled, both stations must be NPC stations
    if cfg.npc_stations_only && (!is_npc_station(src_station) || !is_npc_station(dst_station)) {
        return None;
    }

    // Jump calculations
    let jumps_from_home = graph.jump_count(cfg.home_system, src_system_id, cfg.safe_only);
    if cfg.safe_only && jumps_from_home >= 9999 {
        return None;
    }

    let jumps_source_dest = graph.jump_count(src_system_id, dst_system_id, cfg.safe_only);
    if cfg.safe_only && jumps_source_dest >= 9999 {
        return None;
    }

    let total_jumps = (jumps_from_home + jumps_source_dest).max(1);

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
    if net_profit_pct < cfg.min_net_profit_pct {
        return None;
    }

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

    let src_station_name = sde
        .get_station_name(src_station)
        .unwrap_or_else(|| {
            if is_npc_station(src_station) {
                format!("Station #{}", src_station)
            } else {
                format!("Citadel #{} ({})", src_station, src_system_name)
            }
        });

    let dst_station_name = sde
        .get_station_name(dst_station)
        .unwrap_or_else(|| {
            if is_npc_station(dst_station) {
                format!("Station #{}", dst_station)
            } else {
                format!("Citadel #{} ({})", dst_station, dst_system_name)
            }
        });

    Some(ArbitrageDeal {
        type_id,
        item_name: item.name.clone(),
        buy_price,
        sell_price,
        spread_pct,
        source_station_id: src_station,
        source_system_id: src_system_id,
        source_system_name: src_system_name,
        source_station_name: src_station_name,
        dest_station_id: dst_station,
        dest_system_id: dst_system_id,
        dest_system_name: dst_system_name,
        dest_station_name: dst_station_name,
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
