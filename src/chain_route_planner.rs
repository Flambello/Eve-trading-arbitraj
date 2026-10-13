use crate::arbitrage::ArbitrageDeal;
use crate::graph::SystemGraph;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// An item batch loaded at a pickup stop, destined for a specific sell system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoSlot {
    pub deal: ArbitrageDeal,
    pub sell_system: u32,
    pub sell_name: String,
    pub volume: f64,
    pub capital: f64,
    pub net_profit: f64,
}

/// A pickup stop: fly here, load cargo, then continue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PickupStop {
    pub system_id: u32,
    pub system_name: String,
    pub jumps_from_prev: u32,
    pub detour_jumps: u32,
    pub cargo_loaded: Vec<CargoSlot>,
    pub volume_loaded: f64,
    pub capital_spent: f64,
}

/// A sell stop: fly here, sell designated cargo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SellStop {
    pub system_id: u32,
    pub system_name: String,
    pub jumps_from_prev: u32,
    pub cargo_sold: Vec<CargoSlot>,
    pub total_profit: f64,
    pub volume_freed: f64,
}

/// One full leg: [pickup stop(s)] → [sell stop].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainLeg {
    pub pickups: Vec<PickupStop>,
    pub sell: SellStop,
    pub leg_total_jumps: u32,
    pub leg_total_profit: f64,
}

/// A full planned chain route.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainRoute {
    pub legs: Vec<ChainLeg>,
    pub total_jumps: u32,
    pub total_profit: f64,
    pub total_capital: f64,
    pub peak_volume_m3: f64,
    pub profit_per_jump: f64,
    pub overall_roi_pct: f64,
    pub waypoints: Vec<u32>,
    pub waypoint_names: Vec<String>,
}

impl ChainRoute {
    /// Returns the ordered sequence of unique station / system IDs for ESI in-game autopilot.
    pub fn ordered_destination_ids(&self) -> Vec<u64> {
        let mut ids = Vec::new();
        for leg in &self.legs {
            for pickup in &leg.pickups {
                for slot in &pickup.cargo_loaded {
                    let st_id = slot.deal.source_station_id;
                    if ids.last().copied() != Some(st_id) {
                        ids.push(st_id);
                    }
                }
            }
            for slot in &leg.sell.cargo_sold {
                let st_id = slot.deal.dest_station_id;
                if ids.last().copied() != Some(st_id) {
                    ids.push(st_id);
                }
            }
        }
        ids
    }

    /// Formats a complete in-game flight dossier suitable for EVE Notepad (Alt+N).
    pub fn format_eve_flight_sheet(&self, route_idx: usize) -> String {
        let mut s = String::new();
        s.push_str("═══════════════════════════════════════════════════════════\n");
        s.push_str(&format!(
            "🔗 CHAIN ROUTE #{} | {} Jumps | Net Profit: +{:.2}M ISK | ROI: {:.1}%\n",
            route_idx + 1,
            self.total_jumps,
            self.total_profit / 1_000_000.0,
            self.overall_roi_pct
        ));
        s.push_str(&format!(
            "Capital: {:.2}M ISK | Peak Volume: {:.1} m³ | PPJ: {:.2}M/jump\n",
            self.total_capital / 1_000_000.0,
            self.peak_volume_m3,
            self.profit_per_jump / 1_000_000.0
        ));
        s.push_str("═══════════════════════════════════════════════════════════\n\n");

        for (leg_idx, leg) in self.legs.iter().enumerate() {
            s.push_str(&format!(
                "▶ LEG #{} ({} Jumps, +{:.2}M ISK):\n",
                leg_idx + 1,
                leg.leg_total_jumps,
                leg.leg_total_profit / 1_000_000.0
            ));

            for pickup in &leg.pickups {
                s.push_str(&format!(
                    "  [BUY STOP] {} (⚡ {} jumps)\n",
                    pickup.system_name, pickup.jumps_from_prev
                ));
                for slot in &pickup.cargo_loaded {
                    s.push_str(&format!(
                        "    • Station: {}\n",
                        slot.deal.source_station_name
                    ));
                    s.push_str(&format!(
                        "    • BUY: {}x {} @ {:.2} ISK (Cost: {:.2}M ISK, {:.1} m³)\n",
                        slot.deal.quantity,
                        slot.deal.item_name,
                        slot.deal.buy_price,
                        slot.capital / 1_000_000.0,
                        slot.volume
                    ));
                }
            }

            s.push_str(&format!(
                "  [SELL STOP] {} (⚡ {} jumps)\n",
                leg.sell.system_name, leg.sell.jumps_from_prev
            ));
            for slot in &leg.sell.cargo_sold {
                s.push_str(&format!("    • Station: {}\n", slot.deal.dest_station_name));
                s.push_str(&format!(
                    "    • SELL: {}x {} @ {:.2} ISK (Gross: {:.2}M ISK, Profit: +{:.2}M ISK)\n",
                    slot.deal.quantity,
                    slot.deal.item_name,
                    slot.deal.sell_price,
                    (slot.deal.sell_price * slot.deal.quantity as f64) / 1_000_000.0,
                    slot.net_profit / 1_000_000.0
                ));
            }
            s.push('\n');
        }

        s.push_str("═══════════════════════════════════════════════════════════\n");
        s.push_str(&format!(
            "Autopilot trail: {}\n",
            self.waypoint_names.join(" ➔ ")
        ));
        s.push_str("═══════════════════════════════════════════════════════════\n");
        s
    }
}

/// Configuration for the chain search.
#[derive(Debug, Clone)]
pub struct ChainConfig {
    pub home_system: u32,
    pub max_legs: usize,
    pub beam_width: usize,
    pub detour_enabled: bool,
    pub max_detour_jumps: u32,
    pub max_dest_loads: usize, // Max loads of an item delivered to any destination station (default: 2)
    pub ship_max_volume: f64,
    pub ship_max_budget: f64,
    pub safe_only: bool,
    pub include_npc: bool, // Controls whether NPC trade goods are considered (default: true)
}

impl Default for ChainConfig {
    fn default() -> Self {
        Self {
            home_system: 30000142, // Jita
            max_legs: 4,
            beam_width: 15,
            detour_enabled: true,
            max_detour_jumps: 2,
            max_dest_loads: 2, // Default: max 2 loads per destination station to prevent supply & demand price collapse
            ship_max_volume: 60_000.0,
            ship_max_budget: 500_000_000.0,
            safe_only: true,
            include_npc: true,
        }
    }
}

/// Internal candidate state tracked during beam search.
#[derive(Clone)]
struct BeamCandidate {
    current_system: u32,
    remaining_vol: f64,
    remaining_isk: f64,
    current_vol_used: f64,
    peak_volume_m3: f64,
    cargo_on_board: Vec<CargoSlot>,
    legs: Vec<ChainLeg>,
    total_jumps: u32,
    total_profit: f64,
    total_capital_spent: f64,
    used_deal_keys: HashSet<(u32, u64, u64)>, // (type_id, source_station_id, dest_station_id)
    consumed_source_orders: HashMap<u64, u32>, // source_order_id -> quantity bought across route
    consumed_dest_orders: HashMap<u64, u32>,  // dest_order_id -> quantity sold across route
    exhausted_source_stations: HashSet<(u32, u64)>, // (type_id, source_station_id)
    delivered_dest_loads: HashMap<(u32, u64), usize>, // (type_id, dest_station_id) -> count of loads delivered across route
    exhausted_dest_stations: HashSet<(u32, u64)>, // (type_id, dest_station_id) that hit max_dest_loads
}

/// Calculates extra jumps if routing `from` → `waypoint` → `to` vs `from` → `to` direct.
pub fn detour_cost(from: u32, waypoint: u32, to: u32, graph: &SystemGraph, safe: bool) -> u32 {
    if waypoint == from || waypoint == to {
        return 0;
    }
    let direct = graph.jump_count(from, to, safe);
    if direct >= 9999 {
        return 9999;
    }
    let j1 = graph.jump_count(from, waypoint, safe);
    if j1 >= 9999 {
        return 9999;
    }
    let j2 = graph.jump_count(waypoint, to, safe);
    if j2 >= 9999 {
        return 9999;
    }
    (j1 + j2).saturating_sub(direct)
}

/// Result of finite-stock constrained knapsack packing across route stops and legs.
#[derive(Debug, Clone)]
pub struct ConstrainedPackResult {
    pub packed: Vec<CargoSlot>,
    pub total_volume: f64,
    pub total_capital: f64,
    pub total_profit: f64,
    pub consumed_sources: HashMap<u64, u32>,
    pub consumed_dests: HashMap<u64, u32>,
    pub exhausted_sources: HashSet<(u32, u64)>,
    pub exhausted_dests: HashSet<(u32, u64)>,
}

/// Knapsack packing at a single station / system.
/// Fills up to `remaining_vol` and `remaining_isk`.
/// Sorts available deals by profit density (net_profit / volume_used) DESC.
/// Enforces finite order stock and prevents multiplying source orders beyond available volume.
pub fn knapsack_pack(
    deals: &[&ArbitrageDeal],
    remaining_vol: f64,
    remaining_isk: f64,
    used_keys: &HashSet<(u32, u64, u64)>,
) -> (Vec<CargoSlot>, f64, f64, f64) {
    let empty_sources = HashMap::new();
    let empty_dests = HashMap::new();
    let empty_exhausted = HashSet::new();
    let empty_exhausted_dests = HashSet::new();
    let res = knapsack_pack_constrained(
        deals,
        remaining_vol,
        remaining_isk,
        used_keys,
        &empty_sources,
        &empty_dests,
        &empty_exhausted,
        &empty_exhausted_dests,
    );
    (
        res.packed,
        res.total_volume,
        res.total_capital,
        res.total_profit,
    )
}

/// Extended knapsack packing that tracks and updates order volume consumption
/// across all stops and legs of the route, ensuring no source order is over-allocated
/// and no destination station exceeds its market supply and demand load limit.
#[allow(clippy::too_many_arguments)]
pub fn knapsack_pack_constrained(
    deals: &[&ArbitrageDeal],
    mut remaining_vol: f64,
    mut remaining_isk: f64,
    used_keys: &HashSet<(u32, u64, u64)>,
    prior_consumed_sources: &HashMap<u64, u32>,
    prior_consumed_dests: &HashMap<u64, u32>,
    prior_exhausted_sources: &HashSet<(u32, u64)>,
    prior_exhausted_dests: &HashSet<(u32, u64)>,
) -> ConstrainedPackResult {
    let mut consumed_sources = prior_consumed_sources.clone();
    let mut consumed_dests = prior_consumed_dests.clone();
    let mut exhausted_sources = prior_exhausted_sources.clone();
    let exhausted_dests = prior_exhausted_dests.clone();

    let mut available: Vec<&&ArbitrageDeal> = deals
        .iter()
        .filter(|d| {
            if used_keys.contains(&(d.type_id, d.source_station_id, d.dest_station_id))
                || d.net_profit_pct < 10.0
                || exhausted_sources.contains(&(d.type_id, d.source_station_id))
                || exhausted_dests.contains(&(d.type_id, d.dest_station_id))
            {
                return false;
            }

            if d.source_order_id > 0 {
                let consumed = consumed_sources
                    .get(&d.source_order_id)
                    .copied()
                    .unwrap_or(0);
                let total_avail = if d.source_order_volume_remain > 0 {
                    d.source_order_volume_remain
                } else {
                    d.quantity
                };
                if consumed >= total_avail {
                    return false;
                }
            }

            if d.dest_order_id > 0 {
                let consumed = consumed_dests.get(&d.dest_order_id).copied().unwrap_or(0);
                let total_avail = if d.dest_order_volume_remain > 0 {
                    d.dest_order_volume_remain
                } else {
                    d.quantity
                };
                if consumed >= total_avail {
                    return false;
                }
            }

            true
        })
        .collect();

    // Sort by profit_per_m3 DESC (highest value density items first)
    available.sort_by(|a, b| {
        b.profit_per_m3
            .partial_cmp(&a.profit_per_m3)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut packed = Vec::new();
    let mut total_vol = 0.0;
    let mut total_cap = 0.0;
    let mut total_profit = 0.0;

    for deal in available {
        if remaining_vol <= 0.001 || remaining_isk <= 0.001 {
            break;
        }

        // Check if this source or dest station/type was marked exhausted
        if exhausted_sources.contains(&(deal.type_id, deal.source_station_id))
            || exhausted_dests.contains(&(deal.type_id, deal.dest_station_id))
        {
            continue;
        }

        let unit_vol = deal.volume_used / (deal.quantity.max(1) as f64);
        let unit_cost = deal.buy_price;
        let unit_profit = deal.net_profit / (deal.quantity.max(1) as f64);

        if unit_vol <= 0.0 || unit_cost <= 0.0 {
            continue;
        }

        let max_by_vol = (remaining_vol / unit_vol).floor() as u32;
        let max_by_cost = (remaining_isk / unit_cost).floor() as u32;

        let avail_source = if deal.source_order_id > 0 {
            let consumed = consumed_sources
                .get(&deal.source_order_id)
                .copied()
                .unwrap_or(0);
            let total_avail = if deal.source_order_volume_remain > 0 {
                deal.source_order_volume_remain
            } else {
                deal.quantity
            };
            total_avail.saturating_sub(consumed)
        } else {
            deal.quantity
        };

        let avail_dest = if deal.dest_order_id > 0 {
            let consumed = consumed_dests
                .get(&deal.dest_order_id)
                .copied()
                .unwrap_or(0);
            let total_avail = if deal.dest_order_volume_remain > 0 {
                deal.dest_order_volume_remain
            } else {
                deal.quantity
            };
            total_avail.saturating_sub(consumed)
        } else {
            deal.quantity
        };

        let fit_qty = max_by_vol
            .min(max_by_cost)
            .min(deal.quantity)
            .min(avail_source)
            .min(avail_dest);

        let min_req = deal.dest_order_min_volume.max(1);
        if fit_qty < min_req {
            continue;
        }

        let slot_vol = unit_vol * (fit_qty as f64);
        let slot_cost = unit_cost * (fit_qty as f64);
        let slot_profit = unit_profit * (fit_qty as f64);

        let mut scaled_deal = (*deal).clone();
        scaled_deal.quantity = fit_qty;
        scaled_deal.volume_used = slot_vol;
        scaled_deal.capital_required = slot_cost;
        scaled_deal.gross_profit = (deal.sell_price - deal.buy_price) * (fit_qty as f64);
        scaled_deal.taxes_and_fees =
            (deal.taxes_and_fees / (deal.quantity.max(1) as f64)) * (fit_qty as f64);
        scaled_deal.net_profit = slot_profit;
        scaled_deal.net_profit_pct = deal.net_profit_pct; // Preserved invariant

        packed.push(CargoSlot {
            deal: scaled_deal,
            sell_system: deal.dest_system_id,
            sell_name: deal.dest_system_name.clone(),
            volume: slot_vol,
            capital: slot_cost,
            net_profit: slot_profit,
        });

        // Update consumed supplies
        if deal.source_order_id > 0 {
            let c = consumed_sources.entry(deal.source_order_id).or_insert(0);
            *c += fit_qty;
            let total_avail = if deal.source_order_volume_remain > 0 {
                deal.source_order_volume_remain
            } else {
                deal.quantity
            };
            if *c >= total_avail {
                exhausted_sources.insert((deal.type_id, deal.source_station_id));
            }
        } else {
            // Fallback when order ID is 0: mark station/type exhausted once packed
            exhausted_sources.insert((deal.type_id, deal.source_station_id));
        }

        if deal.dest_order_id > 0 {
            let c = consumed_dests.entry(deal.dest_order_id).or_insert(0);
            *c += fit_qty;
        }

        remaining_vol -= slot_vol;
        remaining_isk -= slot_cost;
        total_vol += slot_vol;
        total_cap += slot_cost;
        total_profit += slot_profit;
    }

    // Once a stop has packed its deals, mark all packed (type_id, source_station_id) as exhausted
    // so subsequent legs cannot re-buy from this station
    for slot in &packed {
        exhausted_sources.insert((slot.deal.type_id, slot.deal.source_station_id));
    }

    ConstrainedPackResult {
        packed,
        total_volume: total_vol,
        total_capital: total_cap,
        total_profit,
        consumed_sources,
        consumed_dests,
        exhausted_sources,
        exhausted_dests,
    }
}

/// Builds flat list of ordered waypoints and their names.
pub fn build_all_waypoints(
    home: u32,
    legs: &[ChainLeg],
    graph: &SystemGraph,
) -> (Vec<u32>, Vec<String>) {
    let mut waypoints = Vec::new();
    let mut names = Vec::new();

    waypoints.push(home);
    names.push(
        graph
            .system_name(home)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("System {}", home)),
    );

    for leg in legs {
        for pickup in &leg.pickups {
            if waypoints.last() != Some(&pickup.system_id) {
                waypoints.push(pickup.system_id);
                names.push(pickup.system_name.clone());
            }
        }
        if waypoints.last() != Some(&leg.sell.system_id) {
            waypoints.push(leg.sell.system_id);
            names.push(leg.sell.system_name.clone());
        }
    }

    (waypoints, names)
}

/// Multi-leg chained trade route optimizer.
/// Pure CPU calculation, thread-safe, no async or I/O.
pub fn find_chain_routes(
    deals: &[ArbitrageDeal],
    graph: &SystemGraph,
    config: &ChainConfig,
) -> Vec<ChainRoute> {
    if deals.is_empty() || config.max_legs == 0 {
        return Vec::new();
    }

    // 1. Pre-filter deals by highsec if safe_only, 10% ROI invariant, and NPC commodity toggle
    let valid_deals: Vec<&ArbitrageDeal> = deals
        .iter()
        .filter(|d| {
            if !config.include_npc && d.is_npc_trade {
                return false;
            }
            if config.safe_only
                && (!graph.is_highsec(d.source_system_id) || !graph.is_highsec(d.dest_system_id))
            {
                return false;
            }
            if d.net_profit_pct < 10.0 {
                return false;
            }
            let min_qty = d.dest_order_min_volume.max(1) as f64;
            let unit_vol = d.volume_used / (d.quantity.max(1) as f64);
            let unit_cost = d.buy_price;
            let min_vol = unit_vol * min_qty;
            let min_cost = unit_cost * min_qty;
            min_vol <= config.ship_max_volume && min_cost <= config.ship_max_budget
        })
        .collect();

    if valid_deals.is_empty() {
        return Vec::new();
    }

    // 2. Group deals by source system and by (source, dest) corridor
    let mut deals_by_source: HashMap<u32, Vec<&ArbitrageDeal>> = HashMap::new();
    let mut deals_by_corridor: HashMap<(u32, u32), Vec<&ArbitrageDeal>> = HashMap::new();

    for &deal in &valid_deals {
        deals_by_source
            .entry(deal.source_system_id)
            .or_default()
            .push(deal);
        deals_by_corridor
            .entry((deal.source_system_id, deal.dest_system_id))
            .or_default()
            .push(deal);
    }

    let initial_candidate = BeamCandidate {
        current_system: config.home_system,
        remaining_vol: config.ship_max_volume,
        remaining_isk: config.ship_max_budget,
        current_vol_used: 0.0,
        peak_volume_m3: 0.0,
        cargo_on_board: Vec::new(),
        legs: Vec::new(),
        total_jumps: 0,
        total_profit: 0.0,
        total_capital_spent: 0.0,
        used_deal_keys: HashSet::new(),
        consumed_source_orders: HashMap::new(),
        consumed_dest_orders: HashMap::new(),
        exhausted_source_stations: HashSet::new(),
        delivered_dest_loads: HashMap::new(),
        exhausted_dest_stations: HashSet::new(),
    };

    let mut beam = vec![initial_candidate];
    let mut completed_routes: Vec<ChainRoute> = Vec::new();

    for leg_idx in 0..config.max_legs {
        let is_last_leg = leg_idx + 1 == config.max_legs;
        let mut next_beam = Vec::new();

        for candidate in &beam {
            // Case 1: Candidate has cargo on board that must be delivered
            if !candidate.cargo_on_board.is_empty() {
                let target_dest = candidate.cargo_on_board[0].sell_system;
                let target_name = candidate.cargo_on_board[0].sell_name.clone();

                let mut current_pos = candidate.current_system;
                let mut current_vol = candidate.current_vol_used;
                let mut current_rem_vol = candidate.remaining_vol;
                let mut current_rem_isk = candidate.remaining_isk;
                let mut peak_vol = candidate.peak_volume_m3;
                let mut leg_jumps = 0;
                let mut pickups = Vec::new();
                let mut candidate_cargo = candidate.cargo_on_board.clone();
                let mut candidate_used_keys = candidate.used_deal_keys.clone();
                let mut candidate_consumed_sources = candidate.consumed_source_orders.clone();
                let mut candidate_consumed_dests = candidate.consumed_dest_orders.clone();
                let mut candidate_exhausted_sources = candidate.exhausted_source_stations.clone();
                let mut candidate_delivered_dests = candidate.delivered_dest_loads.clone();
                let mut candidate_exhausted_dests = candidate.exhausted_dest_stations.clone();
                let mut leg_capital_spent = 0.0;

                // A. Check if we can pack more deals at current system going to target_dest
                if current_rem_vol > 0.001 && current_rem_isk > 0.001 {
                    if let Some(src_deals) = deals_by_corridor.get(&(current_pos, target_dest)) {
                        let pack_res = knapsack_pack_constrained(
                            src_deals,
                            current_rem_vol,
                            current_rem_isk,
                            &candidate_used_keys,
                            &candidate_consumed_sources,
                            &candidate_consumed_dests,
                            &candidate_exhausted_sources,
                            &candidate_exhausted_dests,
                        );
                        candidate_consumed_sources = pack_res.consumed_sources;
                        candidate_consumed_dests = pack_res.consumed_dests;
                        candidate_exhausted_sources = pack_res.exhausted_sources;
                        candidate_exhausted_dests = pack_res.exhausted_dests;

                        if !pack_res.packed.is_empty() {
                            for slot in &pack_res.packed {
                                candidate_used_keys.insert((
                                    slot.deal.type_id,
                                    slot.deal.source_station_id,
                                    slot.deal.dest_station_id,
                                ));
                                candidate_cargo.push(slot.clone());
                            }
                            current_vol += pack_res.total_volume;
                            current_rem_vol -= pack_res.total_volume;
                            current_rem_isk -= pack_res.total_capital;
                            leg_capital_spent += pack_res.total_capital;
                            peak_vol = peak_vol.max(current_vol);

                            pickups.push(PickupStop {
                                system_id: current_pos,
                                system_name: graph
                                    .system_name(current_pos)
                                    .map(|s| s.to_string())
                                    .unwrap_or_else(|| format!("System {}", current_pos)),
                                jumps_from_prev: 0,
                                detour_jumps: 0,
                                cargo_loaded: pack_res.packed,
                                volume_loaded: pack_res.total_volume,
                                capital_spent: pack_res.total_capital,
                            });
                        }
                    }
                }

                // B. En-route pickups along path to target_dest
                let direct_jumps = graph.jump_count(current_pos, target_dest, config.safe_only);
                if direct_jumps < 9999 {
                    if current_rem_vol > 0.001 && current_rem_isk > 0.001 {
                        // Gather potential en-route systems
                        let mut candidates_enroute = Vec::new();
                        for (&sys_id, sys_deals) in &deals_by_source {
                            if sys_id == current_pos || sys_id == target_dest {
                                continue;
                            }
                            let detour = detour_cost(
                                current_pos,
                                sys_id,
                                target_dest,
                                graph,
                                config.safe_only,
                            );
                            if config.detour_enabled {
                                if detour <= config.max_detour_jumps {
                                    candidates_enroute.push((sys_id, detour, sys_deals));
                                }
                            } else if detour == 0 {
                                candidates_enroute.push((sys_id, 0, sys_deals));
                            }
                        }

                        // Sort by detour jumps ASC
                        candidates_enroute.sort_by_key(|&(_, d, _)| d);

                        for (enroute_sys, detour, enroute_deals) in candidates_enroute {
                            if current_rem_vol <= 0.001 || current_rem_isk <= 0.001 {
                                break;
                            }

                            // Filter deals at this system destined for target_dest, excluding exhausted destinations
                            let matching_deals: Vec<&ArbitrageDeal> = enroute_deals
                                .iter()
                                .filter(|d| {
                                    d.dest_system_id == target_dest
                                        && !candidate_exhausted_dests
                                            .contains(&(d.type_id, d.dest_station_id))
                                })
                                .copied()
                                .collect();

                            if matching_deals.is_empty() {
                                continue;
                            }

                            let bonus_res = knapsack_pack_constrained(
                                &matching_deals,
                                current_rem_vol,
                                current_rem_isk,
                                &candidate_used_keys,
                                &candidate_consumed_sources,
                                &candidate_consumed_dests,
                                &candidate_exhausted_sources,
                                &candidate_exhausted_dests,
                            );

                            if !bonus_res.packed.is_empty() {
                                candidate_consumed_sources = bonus_res.consumed_sources;
                                candidate_consumed_dests = bonus_res.consumed_dests;
                                candidate_exhausted_sources = bonus_res.exhausted_sources;
                                candidate_exhausted_dests = bonus_res.exhausted_dests;

                                let jumps_from_prev =
                                    graph.jump_count(current_pos, enroute_sys, config.safe_only);
                                leg_jumps += jumps_from_prev;
                                current_pos = enroute_sys;

                                for slot in &bonus_res.packed {
                                    candidate_used_keys.insert((
                                        slot.deal.type_id,
                                        slot.deal.source_station_id,
                                        slot.deal.dest_station_id,
                                    ));
                                    candidate_cargo.push(slot.clone());
                                }
                                current_vol += bonus_res.total_volume;
                                current_rem_vol -= bonus_res.total_volume;
                                current_rem_isk -= bonus_res.total_capital;
                                leg_capital_spent += bonus_res.total_capital;
                                peak_vol = peak_vol.max(current_vol);

                                pickups.push(PickupStop {
                                    system_id: enroute_sys,
                                    system_name: graph
                                        .system_name(enroute_sys)
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| format!("System {}", enroute_sys)),
                                    jumps_from_prev,
                                    detour_jumps: detour,
                                    cargo_loaded: bonus_res.packed,
                                    volume_loaded: bonus_res.total_volume,
                                    capital_spent: bonus_res.total_capital,
                                });
                            }
                        }
                    }

                    let jumps_to_sell =
                        graph.jump_count(current_pos, target_dest, config.safe_only);
                    leg_jumps += jumps_to_sell;

                    // C. Sell at target_dest
                    let mut cargo_sold = Vec::new();
                    let mut retained_cargo = Vec::new();
                    let mut total_profit_sold = 0.0;
                    let mut volume_freed = 0.0;
                    let mut capital_freed = 0.0;

                    for slot in candidate_cargo {
                        if slot.sell_system == target_dest {
                            total_profit_sold += slot.net_profit;
                            volume_freed += slot.volume;
                            capital_freed += slot.capital;
                            cargo_sold.push(slot);
                        } else {
                            retained_cargo.push(slot);
                        }
                    }

                    if !cargo_sold.is_empty() {
                        // Track delivery counts for each destination station
                        let mut unique_delivered = HashSet::new();
                        for slot in &cargo_sold {
                            unique_delivered.insert((slot.deal.type_id, slot.deal.dest_station_id));
                        }
                        for (type_id, dest_station_id) in unique_delivered {
                            let count = candidate_delivered_dests
                                .entry((type_id, dest_station_id))
                                .or_insert(0);
                            *count += 1;
                            if *count >= config.max_dest_loads {
                                candidate_exhausted_dests.insert((type_id, dest_station_id));
                            }
                        }

                        let sell_stop = SellStop {
                            system_id: target_dest,
                            system_name: target_name,
                            jumps_from_prev: jumps_to_sell,
                            cargo_sold,
                            total_profit: total_profit_sold,
                            volume_freed,
                        };

                        let new_leg = ChainLeg {
                            pickups,
                            sell: sell_stop,
                            leg_total_jumps: leg_jumps.max(1),
                            leg_total_profit: total_profit_sold,
                        };

                        let mut new_legs = candidate.legs.clone();
                        new_legs.push(new_leg);

                        let new_total_jumps = candidate.total_jumps + leg_jumps.max(1);
                        let new_total_profit = candidate.total_profit + total_profit_sold;
                        let new_rem_vol =
                            (current_rem_vol + volume_freed).min(config.ship_max_volume);
                        let new_rem_isk = (current_rem_isk + capital_freed + total_profit_sold)
                            .min(config.ship_max_budget);
                        let new_curr_vol = (current_vol - volume_freed).max(0.0);

                        let new_cand = BeamCandidate {
                            current_system: target_dest,
                            remaining_vol: new_rem_vol,
                            remaining_isk: new_rem_isk,
                            current_vol_used: new_curr_vol,
                            peak_volume_m3: peak_vol,
                            cargo_on_board: retained_cargo,
                            legs: new_legs,
                            total_jumps: new_total_jumps,
                            total_profit: new_total_profit,
                            total_capital_spent: candidate.total_capital_spent + leg_capital_spent,
                            used_deal_keys: candidate_used_keys,
                            consumed_source_orders: candidate_consumed_sources,
                            consumed_dest_orders: candidate_consumed_dests,
                            exhausted_source_stations: candidate_exhausted_sources,
                            delivered_dest_loads: candidate_delivered_dests,
                            exhausted_dest_stations: candidate_exhausted_dests,
                        };

                        next_beam.push(new_cand);
                    }
                }
                continue;
            }

            // Case 2: Candidate has empty cargo — start a new pickup corridor
            // Continuous onward chaining:
            // 1. Determine eligible departure systems radiating outward from candidate.current_system.
            let is_initial_leg = candidate.legs.is_empty();

            // Find the closest distance to any source system with available (non-exhausted) deals
            let mut min_dist = 9999;
            for (&src_sys, deals) in &deals_by_source {
                let has_available = deals.iter().any(|d| {
                    !candidate.used_deal_keys.contains(&(
                        d.type_id,
                        d.source_station_id,
                        d.dest_station_id,
                    )) && !candidate
                        .exhausted_source_stations
                        .contains(&(d.type_id, d.source_station_id))
                        && !candidate
                            .exhausted_dest_stations
                            .contains(&(d.type_id, d.dest_station_id))
                });
                if !has_available {
                    continue;
                }

                let j = graph.jump_count(candidate.current_system, src_sys, config.safe_only);
                if j < min_dist {
                    min_dist = j;
                }
            }

            if min_dist >= 9999 {
                continue;
            }

            // For continuous chaining:
            // - Initial leg: evaluate all reachable safe highsec corridors radiating outward from
            //   the player's starting location. Distance from starting system is directly accounted
            //   for in the leg score: net_profit / (jumps_to_source + jumps_source_to_dest).
            // - Subsequent legs: enforce continuous onward chaining from the arrival destination.
            //   The hauler can pick up at the arrival system (0 jumps) or within local proximity (<= 2 jumps).
            //   If no continuous trades exist within 2 jumps, conclude the route at this destination!
            let max_allowed_departure_dist = if is_initial_leg {
                u32::MAX
            } else if min_dist <= 2 {
                2
            } else {
                continue;
            };

            // Find top candidate corridors (src_sys, dst_sys)
            struct EvaluatedCorridor {
                src_sys: u32,
                dst_sys: u32,
                score: f64,
                jumps_to_src: u32,
                pack_res: ConstrainedPackResult,
            }

            let mut corridor_candidates: Vec<EvaluatedCorridor> = Vec::new();

            for (&(src_sys, dst_sys), corr_deals) in &deals_by_corridor {
                let j_to_src =
                    graph.jump_count(candidate.current_system, src_sys, config.safe_only);
                if j_to_src >= 9999 || j_to_src > max_allowed_departure_dist {
                    continue;
                }
                let j_src_dst = graph.jump_count(src_sys, dst_sys, config.safe_only);
                if j_src_dst >= 9999 {
                    continue;
                }

                // Pack with actual ship capacity limits and consumed order state
                let pack_res = knapsack_pack_constrained(
                    corr_deals,
                    candidate.remaining_vol,
                    candidate.remaining_isk,
                    &candidate.used_deal_keys,
                    &candidate.consumed_source_orders,
                    &candidate.consumed_dest_orders,
                    &candidate.exhausted_source_stations,
                    &candidate.exhausted_dest_stations,
                );

                if pack_res.packed.is_empty() || pack_res.total_profit <= 0.0 {
                    continue;
                }

                let total_j = (j_to_src + j_src_dst).max(1);
                let score = pack_res.total_profit / (total_j as f64);
                corridor_candidates.push(EvaluatedCorridor {
                    src_sys,
                    dst_sys,
                    score,
                    jumps_to_src: j_to_src,
                    pack_res,
                });
            }

            // Sort corridors by score (true profit per jump for this leg) DESC and evaluate top candidates
            corridor_candidates.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| {
                        b.pack_res
                            .total_profit
                            .partial_cmp(&a.pack_res.total_profit)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
            });
            corridor_candidates.truncate(config.beam_width.max(20));

            for cand_corr in corridor_candidates {
                let src_sys = cand_corr.src_sys;
                let dst_sys = cand_corr.dst_sys;
                let primary_res = cand_corr.pack_res;
                let jumps_to_src = cand_corr.jumps_to_src;

                let mut candidate_used_keys = candidate.used_deal_keys.clone();
                let mut candidate_consumed_sources = primary_res.consumed_sources.clone();
                let mut candidate_consumed_dests = primary_res.consumed_dests.clone();
                let mut candidate_exhausted_sources = primary_res.exhausted_sources.clone();
                let mut candidate_delivered_dests = candidate.delivered_dest_loads.clone();
                let mut candidate_exhausted_dests = candidate.exhausted_dest_stations.clone();

                for slot in &primary_res.packed {
                    candidate_used_keys.insert((
                        slot.deal.type_id,
                        slot.deal.source_station_id,
                        slot.deal.dest_station_id,
                    ));
                }

                let mut leg_jumps = jumps_to_src;
                let mut current_pos = src_sys;
                let mut current_vol = primary_res.total_volume;
                let mut current_rem_vol = candidate.remaining_vol - primary_res.total_volume;
                let mut current_rem_isk = candidate.remaining_isk - primary_res.total_capital;
                let mut peak_vol = candidate.peak_volume_m3.max(current_vol);
                let mut leg_capital_spent = primary_res.total_capital;

                let mut pickups = vec![PickupStop {
                    system_id: src_sys,
                    system_name: graph
                        .system_name(src_sys)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("System {}", src_sys)),
                    jumps_from_prev: jumps_to_src,
                    detour_jumps: 0,
                    cargo_loaded: primary_res.packed.clone(),
                    volume_loaded: primary_res.total_volume,
                    capital_spent: primary_res.total_capital,
                }];

                let mut candidate_cargo = primary_res.packed;

                // En-route bonus pickups between src_sys and dst_sys (only if cargo hold has room)
                if current_rem_vol > 0.001 && current_rem_isk > 0.001 {
                    let mut candidates_enroute = Vec::new();
                    for (&sys_id, sys_deals) in &deals_by_source {
                        if sys_id == src_sys || sys_id == dst_sys {
                            continue;
                        }
                        let detour = detour_cost(src_sys, sys_id, dst_sys, graph, config.safe_only);
                        if config.detour_enabled {
                            if detour <= config.max_detour_jumps {
                                candidates_enroute.push((sys_id, detour, sys_deals));
                            }
                        } else if detour == 0 {
                            candidates_enroute.push((sys_id, 0, sys_deals));
                        }
                    }

                    candidates_enroute.sort_by_key(|&(_, d, _)| d);

                    for (enroute_sys, detour, enroute_deals) in candidates_enroute {
                        if current_rem_vol <= 0.001 || current_rem_isk <= 0.001 {
                            break;
                        }

                        let matching_deals: Vec<&ArbitrageDeal> = enroute_deals
                            .iter()
                            .filter(|d| d.dest_system_id == dst_sys)
                            .copied()
                            .collect();

                        if matching_deals.is_empty() {
                            continue;
                        }

                        let b_res = knapsack_pack_constrained(
                            &matching_deals,
                            current_rem_vol,
                            current_rem_isk,
                            &candidate_used_keys,
                            &candidate_consumed_sources,
                            &candidate_consumed_dests,
                            &candidate_exhausted_sources,
                            &candidate_exhausted_dests,
                        );

                        if !b_res.packed.is_empty() {
                            candidate_consumed_sources = b_res.consumed_sources;
                            candidate_consumed_dests = b_res.consumed_dests;
                            candidate_exhausted_sources = b_res.exhausted_sources;

                            let jumps_from_prev =
                                graph.jump_count(current_pos, enroute_sys, config.safe_only);
                            leg_jumps += jumps_from_prev;
                            current_pos = enroute_sys;

                            for slot in &b_res.packed {
                                candidate_used_keys.insert((
                                    slot.deal.type_id,
                                    slot.deal.source_station_id,
                                    slot.deal.dest_station_id,
                                ));
                                candidate_cargo.push(slot.clone());
                            }
                            current_vol += b_res.total_volume;
                            current_rem_vol -= b_res.total_volume;
                            current_rem_isk -= b_res.total_capital;
                            leg_capital_spent += b_res.total_capital;
                            peak_vol = peak_vol.max(current_vol);

                            pickups.push(PickupStop {
                                system_id: enroute_sys,
                                system_name: graph
                                    .system_name(enroute_sys)
                                    .map(|s| s.to_string())
                                    .unwrap_or_else(|| format!("System {}", enroute_sys)),
                                jumps_from_prev,
                                detour_jumps: detour,
                                cargo_loaded: b_res.packed,
                                volume_loaded: b_res.total_volume,
                                capital_spent: b_res.total_capital,
                            });
                        }
                    }
                }

                let jumps_to_dst = graph.jump_count(current_pos, dst_sys, config.safe_only);
                if jumps_to_dst >= 9999 {
                    continue;
                }
                leg_jumps += jumps_to_dst;

                // Deliver at dst_sys
                let mut cargo_sold = Vec::new();
                let mut retained_cargo = Vec::new();
                let mut total_profit_sold = 0.0;
                let mut volume_freed = 0.0;
                let mut capital_freed = 0.0;

                for slot in candidate_cargo {
                    if slot.sell_system == dst_sys {
                        total_profit_sold += slot.net_profit;
                        volume_freed += slot.volume;
                        capital_freed += slot.capital;
                        cargo_sold.push(slot);
                    } else {
                        retained_cargo.push(slot);
                    }
                }

                // If this is the last leg, all cargo must be delivered
                if is_last_leg && !retained_cargo.is_empty() {
                    continue;
                }

                if !cargo_sold.is_empty() {
                    // Track delivery counts for each destination station
                    let mut unique_delivered = HashSet::new();
                    for slot in &cargo_sold {
                        unique_delivered.insert((slot.deal.type_id, slot.deal.dest_station_id));
                    }
                    for (type_id, dest_station_id) in unique_delivered {
                        let count = candidate_delivered_dests
                            .entry((type_id, dest_station_id))
                            .or_insert(0);
                        *count += 1;
                        if *count >= config.max_dest_loads {
                            candidate_exhausted_dests.insert((type_id, dest_station_id));
                        }
                    }
                }

                let sell_stop = SellStop {
                    system_id: dst_sys,
                    system_name: graph
                        .system_name(dst_sys)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("System {}", dst_sys)),
                    jumps_from_prev: jumps_to_dst,
                    cargo_sold,
                    total_profit: total_profit_sold,
                    volume_freed,
                };

                let new_leg = ChainLeg {
                    pickups,
                    sell: sell_stop,
                    leg_total_jumps: leg_jumps.max(1),
                    leg_total_profit: total_profit_sold,
                };

                let mut new_legs = candidate.legs.clone();
                new_legs.push(new_leg);

                let new_total_jumps = candidate.total_jumps + leg_jumps.max(1);
                let new_total_profit = candidate.total_profit + total_profit_sold;
                let new_rem_vol = (current_rem_vol + volume_freed).min(config.ship_max_volume);
                let new_rem_isk = (current_rem_isk + capital_freed + total_profit_sold)
                    .min(config.ship_max_budget);
                let new_curr_vol = (current_vol - volume_freed).max(0.0);

                let new_cand = BeamCandidate {
                    current_system: dst_sys,
                    remaining_vol: new_rem_vol,
                    remaining_isk: new_rem_isk,
                    current_vol_used: new_curr_vol,
                    peak_volume_m3: peak_vol,
                    cargo_on_board: retained_cargo,
                    legs: new_legs,
                    total_jumps: new_total_jumps,
                    total_profit: new_total_profit,
                    total_capital_spent: candidate.total_capital_spent + leg_capital_spent,
                    used_deal_keys: candidate_used_keys,
                    consumed_source_orders: candidate_consumed_sources,
                    consumed_dest_orders: candidate_consumed_dests,
                    exhausted_source_stations: candidate_exhausted_sources,
                    delivered_dest_loads: candidate_delivered_dests,
                    exhausted_dest_stations: candidate_exhausted_dests,
                };

                next_beam.push(new_cand);
            }
        }

        if next_beam.is_empty() {
            break;
        }

        // Rank beam candidates by profit per jump DESC
        next_beam.sort_by(|a, b| {
            let score_a = a.total_profit / (a.total_jumps.max(1) as f64);
            let score_b = b.total_profit / (b.total_jumps.max(1) as f64);
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Retain candidates with completed deliveries into completed_routes
        for cand in &next_beam {
            if cand.cargo_on_board.is_empty() && !cand.legs.is_empty() {
                let (waypoints, waypoint_names) =
                    build_all_waypoints(config.home_system, &cand.legs, graph);
                let ppj = cand.total_profit / (cand.total_jumps.max(1) as f64);
                let roi = if cand.total_capital_spent > 0.0 {
                    (cand.total_profit / cand.total_capital_spent) * 100.0
                } else {
                    0.0
                };

                completed_routes.push(ChainRoute {
                    legs: cand.legs.clone(),
                    total_jumps: cand.total_jumps,
                    total_profit: cand.total_profit,
                    total_capital: cand.total_capital_spent,
                    peak_volume_m3: cand.peak_volume_m3,
                    profit_per_jump: ppj,
                    overall_roi_pct: roi,
                    waypoints,
                    waypoint_names,
                });
            }
        }

        next_beam.truncate(config.beam_width.max(1));
        beam = next_beam;
    }

    // Sort completed routes by profit_per_jump DESC (with ties broken by total_profit DESC)
    completed_routes.sort_by(|a, b| {
        b.profit_per_jump
            .partial_cmp(&a.profit_per_jump)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.total_profit
                    .partial_cmp(&a.total_profit)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    // Deduplicate routes with identical waypoints
    let mut seen_waypoints: HashSet<Vec<u32>> = HashSet::new();
    let mut unique_routes = Vec::new();

    for route in completed_routes {
        if seen_waypoints.insert(route.waypoints.clone()) {
            unique_routes.push(route);
        }
    }

    unique_routes.truncate(15);
    unique_routes
}
