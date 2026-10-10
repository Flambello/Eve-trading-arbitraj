use crate::arbitrage::ArbitrageDeal;
use crate::config::ShipProfile;
use crate::graph::SystemGraph;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteStop {
    pub step: usize,
    pub deal: ArbitrageDeal,
    pub jumps_from_prev: u32,
    pub prev_system_id: u32,
    pub prev_system_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRoute {
    pub stops: Vec<RouteStop>,
    pub waypoints: Vec<u32>,
    pub waypoint_names: Vec<String>,
    pub total_jumps: u32,
    pub total_profit: f64,
    pub total_capital_required: f64,
    pub max_volume_used: f64,
    pub avg_profit_per_jump: f64,
}

pub fn plan_route(
    deals: &[ArbitrageDeal],
    _ship: ShipProfile,
    home_system: u32,
    graph: &SystemGraph,
    safe_only: bool,
    max_stops: usize,
) -> TradeRoute {
    let mut unused_deals: Vec<ArbitrageDeal> = deals.to_vec();
    let mut stops = Vec::new();
    let mut current_system = home_system;
    let mut total_jumps = 0;
    let mut total_profit = 0.0;
    let mut total_capital_required = 0.0;
    let mut max_volume_used = 0.0;
    let mut waypoints = vec![home_system];

    for step in 1..=max_stops {
        if unused_deals.is_empty() {
            break;
        }

        // Re-score candidate deals based on current position
        let mut best_idx: Option<usize> = None;
        let mut best_score = -1.0;
        let mut best_leg_jumps = 0;

        for (idx, deal) in unused_deals.iter().enumerate() {
            let jumps_to_src = graph.jump_count(current_system, deal.source_system_id, safe_only);
            if safe_only && jumps_to_src >= 9999 {
                continue;
            }

            let jumps_src_to_dst = deal.jumps_source_dest;
            let leg_jumps = (jumps_to_src + jumps_src_to_dst).max(1);
            let score = deal.net_profit / (leg_jumps as f64);

            if score > best_score
                || (best_idx.is_some()
                    && (score - best_score).abs() < 1e-4
                    && deal.net_profit_pct > unused_deals[best_idx.unwrap()].net_profit_pct)
            {
                best_score = score;
                best_idx = Some(idx);
                best_leg_jumps = leg_jumps;
            }
        }

        let chosen_idx = match best_idx {
            Some(i) => i,
            None => break,
        };

        let chosen_deal = unused_deals.remove(chosen_idx);
        let chosen_src_order = chosen_deal.source_order_id;
        let chosen_src_station = chosen_deal.source_station_id;
        let chosen_type = chosen_deal.type_id;

        // Prune alternative deals sharing the same exhausted source order or station stock
        unused_deals.retain(|d| {
            if chosen_src_order > 0 && d.source_order_id == chosen_src_order {
                return false;
            }
            if d.type_id == chosen_type && d.source_station_id == chosen_src_station {
                return false;
            }
            true
        });

        let departure_system = current_system;
        let prev_name = graph
            .system_name(departure_system)
            .unwrap_or("Unknown")
            .to_string();

        // Trace detailed waypoints for in-game autopilot
        let path_to_src = graph.shortest_path(departure_system, chosen_deal.source_system_id, safe_only);
        for &sys in path_to_src.iter().skip(1) {
            waypoints.push(sys);
        }

        let path_to_dst = graph.shortest_path(chosen_deal.source_system_id, chosen_deal.dest_system_id, safe_only);
        for &sys in path_to_dst.iter().skip(1) {
            waypoints.push(sys);
        }

        total_jumps += best_leg_jumps;
        total_profit += chosen_deal.net_profit;
        if chosen_deal.capital_required > total_capital_required {
            total_capital_required = chosen_deal.capital_required;
        }
        if chosen_deal.volume_used > max_volume_used {
            max_volume_used = chosen_deal.volume_used;
        }

        current_system = chosen_deal.dest_system_id;

        stops.push(RouteStop {
            step,
            deal: chosen_deal,
            jumps_from_prev: best_leg_jumps,
            prev_system_id: departure_system,
            prev_system_name: prev_name,
        });
    }

    let waypoint_names = waypoints
        .iter()
        .map(|&id| {
            graph
                .system_name(id)
                .unwrap_or("Unknown")
                .to_string()
        })
        .collect();

    let avg_profit_per_jump = if total_jumps > 0 {
        total_profit / (total_jumps as f64)
    } else {
        0.0
    };

    TradeRoute {
        stops,
        waypoints,
        waypoint_names,
        total_jumps,
        total_profit,
        total_capital_required,
        max_volume_used,
        avg_profit_per_jump,
    }
}
