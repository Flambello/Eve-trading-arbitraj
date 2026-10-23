use crate::arbitrage::ArbitrageDeal;
use crate::chain_route_planner::{knapsack_pack_constrained, ConstrainedPackResult};
use crate::graph::SystemGraph;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// The 5 major empire trade hubs in EVE Online.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CoreHub {
    Jita,
    Amarr,
    Dodixie,
    Rens,
    Hek,
}

impl CoreHub {
    pub const ALL: [CoreHub; 5] = [
        CoreHub::Jita,
        CoreHub::Amarr,
        CoreHub::Dodixie,
        CoreHub::Rens,
        CoreHub::Hek,
    ];

    /// Solar system ID.
    pub fn system_id(&self) -> u32 {
        match self {
            CoreHub::Jita => 30000142,
            CoreHub::Amarr => 30002187,
            CoreHub::Dodixie => 30002659,
            CoreHub::Rens => 30002510,
            CoreHub::Hek => 30002053,
        }
    }

    /// Region ID.
    pub fn region_id(&self) -> u32 {
        match self {
            CoreHub::Jita => 10000002,    // The Forge
            CoreHub::Amarr => 10000043,   // Domain
            CoreHub::Dodixie => 10000032, // Sinq Laison
            CoreHub::Rens => 10000030,    // Heimatar
            CoreHub::Hek => 10000042,     // Metropolis
        }
    }

    /// Region name.
    pub fn region_name(&self) -> &'static str {
        match self {
            CoreHub::Jita => "The Forge",
            CoreHub::Amarr => "Domain",
            CoreHub::Dodixie => "Sinq Laison",
            CoreHub::Rens => "Heimatar",
            CoreHub::Hek => "Metropolis",
        }
    }

    /// Primary 4-4 / main market station ID.
    pub fn primary_station_id(&self) -> u64 {
        match self {
            CoreHub::Jita => 60003760, // Jita IV - Moon 4 - Caldari Navy Assembly Plant
            CoreHub::Amarr => 60008494, // Amarr VIII (Oris) - Emperor Family Academy
            CoreHub::Dodixie => 60011866, // Dodixie IX - Moon 20 - Federation Navy Assembly Plant
            CoreHub::Rens => 60004588, // Rens VI - Moon 8 - Brutor Tribe Treasury
            CoreHub::Hek => 60005686,  // Hek VIII - Moon 12 - Boundless Creation Factory
        }
    }

    /// Primary station short label.
    pub fn primary_station_label(&self) -> &'static str {
        match self {
            CoreHub::Jita => "Jita IV - Moon 4 (CNAP)",
            CoreHub::Amarr => "Amarr VIII - Emperor Family",
            CoreHub::Dodixie => "Dodixie IX - Moon 20 (FNAP)",
            CoreHub::Rens => "Rens VI - Moon 8 (BTT)",
            CoreHub::Hek => "Hek VIII - Moon 12 (BCF)",
        }
    }

    /// System display name.
    pub fn name(&self) -> &'static str {
        match self {
            CoreHub::Jita => "Jita",
            CoreHub::Amarr => "Amarr",
            CoreHub::Dodixie => "Dodixie",
            CoreHub::Rens => "Rens",
            CoreHub::Hek => "Hek",
        }
    }

    /// Resolve CoreHub from a solar system ID.
    pub fn from_system_id(id: u32) -> Option<CoreHub> {
        match id {
            30000142 => Some(CoreHub::Jita),
            30002187 => Some(CoreHub::Amarr),
            30002659 => Some(CoreHub::Dodixie),
            30002510 => Some(CoreHub::Rens),
            30002053 => Some(CoreHub::Hek),
            _ => None,
        }
    }

    /// Resolve CoreHub from a system name string (case-insensitive).
    pub fn from_name(name: &str) -> Option<CoreHub> {
        match name.trim().to_lowercase().as_str() {
            "jita" => Some(CoreHub::Jita),
            "amarr" => Some(CoreHub::Amarr),
            "dodixie" => Some(CoreHub::Dodixie),
            "rens" => Some(CoreHub::Rens),
            "hek" => Some(CoreHub::Hek),
            _ => None,
        }
    }
}

/// Filter criteria for querying core hub arbitrage trades.
#[derive(Debug, Clone)]
pub struct HubFilterOptions {
    pub source_hub: Option<CoreHub>, // None = Any Core Hub
    pub dest_hub: Option<CoreHub>,   // None = Any Core Hub
    pub include_npc: bool,           // Include seeded NPC trade goods
    pub major_stations_only: bool,   // Only include primary market stations
    pub min_profit: f64,             // Minimum net profit in ISK
    pub search_query: String,        // Item or station name query
}

impl Default for HubFilterOptions {
    fn default() -> Self {
        Self {
            source_hub: Some(CoreHub::Jita),
            dest_hub: Some(CoreHub::Amarr),
            include_npc: true,
            major_stations_only: false,
            min_profit: 500_000.0,
            search_query: String::new(),
        }
    }
}

/// Zero-allocation case-insensitive ASCII substring search.
#[inline]
pub fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let n_len = needle.len();
    if n_len == 0 {
        return true;
    }
    if haystack.len() < n_len {
        return false;
    }
    haystack
        .as_bytes()
        .windows(n_len)
        .any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Zero-allocation case-insensitive ASCII string comparison for sort closures.
#[inline]
pub fn cmp_ignore_ascii_case(a: &str, b: &str) -> std::cmp::Ordering {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    for (x, y) in a_bytes.iter().zip(b_bytes.iter()) {
        let ord = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
        if ord != std::cmp::Ordering::Equal {
            return ord;
        }
    }
    a_bytes.len().cmp(&b_bytes.len())
}

/// Filter arbitrage deals strictly between core empire trade hubs.
pub fn filter_hub_deals<'a>(
    deals: &'a [ArbitrageDeal],
    opts: &HubFilterOptions,
) -> Vec<&'a ArbitrageDeal> {
    let query = opts.search_query.trim();

    deals
        .iter()
        .filter(|d| {
            let src_hub = match CoreHub::from_system_id(d.source_system_id) {
                Some(h) => h,
                None => return false,
            };
            let dst_hub = match CoreHub::from_system_id(d.dest_system_id) {
                Some(h) => h,
                None => return false,
            };

            // Strictly inter-hub trades (source != dest)
            if src_hub == dst_hub {
                return false;
            }

            // Check source hub filter
            if let Some(target_src) = opts.source_hub {
                if src_hub != target_src {
                    return false;
                }
            }

            // Check dest hub filter
            if let Some(target_dst) = opts.dest_hub {
                if dst_hub != target_dst {
                    return false;
                }
            }

            // NPC commodities toggle
            if !opts.include_npc && d.is_npc_trade {
                return false;
            }

            // Major market stations only
            if opts.major_stations_only
                && (d.source_station_id != src_hub.primary_station_id()
                    || d.dest_station_id != dst_hub.primary_station_id())
            {
                return false;
            }

            // Minimum profit threshold
            if d.net_profit < opts.min_profit {
                return false;
            }

            // Strict 10.0% ROI domain invariant
            if d.net_profit_pct < 10.0 {
                return false;
            }

            // Item or station text search (Zero-Allocation)
            if !query.is_empty() {
                let item_match = contains_ignore_ascii_case(&d.item_name, query);
                let src_match = contains_ignore_ascii_case(&d.source_station_name, query);
                let dst_match = contains_ignore_ascii_case(&d.dest_station_name, query);
                if !item_match && !src_match && !dst_match {
                    return false;
                }
            }

            true
        })
        .collect()
}

/// Knapsack pack an optimal ship cargo manifest for a specific core hub trade corridor.
pub fn pack_hub_corridor_manifest(
    deals: &[&ArbitrageDeal],
    ship_volume: f64,
    ship_budget: f64,
) -> ConstrainedPackResult {
    let empty_used = HashSet::new();
    let empty_consumed_src = HashMap::new();
    let empty_consumed_dst = HashMap::new();
    let empty_exhausted_src = HashSet::new();
    let empty_exhausted_dst = HashSet::new();

    knapsack_pack_constrained(
        deals,
        ship_volume,
        ship_budget,
        &empty_used,
        &empty_consumed_src,
        &empty_consumed_dst,
        &empty_exhausted_src,
        &empty_exhausted_dst,
    )
}

/// Corridor telemetry summary between two core trade hubs.
#[derive(Debug, Clone)]
pub struct HubCorridorSummary {
    pub source_hub: CoreHub,
    pub dest_hub: CoreHub,
    pub safe_jumps: u32,
    pub deal_count: usize,
    pub total_potential_profit: f64,
    pub max_single_deal_profit: f64,
    pub top_item_name: Option<String>,
}

/// Calculate highsec jump distance and summary statistics for a core hub trade corridor.
pub fn compute_corridor_summary(
    deals: &[&ArbitrageDeal],
    graph: &SystemGraph,
    source_hub: CoreHub,
    dest_hub: CoreHub,
    safe_only: bool,
) -> HubCorridorSummary {
    let safe_jumps = graph.jump_count(source_hub.system_id(), dest_hub.system_id(), safe_only);
    let mut deal_count = 0;
    let mut total_potential_profit = 0.0;
    let mut max_single_deal_profit = 0.0;
    let mut top_item_name = None;

    for &d in deals {
        if d.source_system_id == source_hub.system_id() && d.dest_system_id == dest_hub.system_id()
        {
            deal_count += 1;
            total_potential_profit += d.net_profit;
            if d.net_profit > max_single_deal_profit {
                max_single_deal_profit = d.net_profit;
                top_item_name = Some(d.item_name.clone());
            }
        }
    }

    HubCorridorSummary {
        source_hub,
        dest_hub,
        safe_jumps,
        deal_count,
        total_potential_profit,
        max_single_deal_profit,
        top_item_name,
    }
}

/// Returns the ordered station IDs [source_station, dest_station] for ESI in-game autopilot.
pub fn hub_corridor_destination_ids(
    source_hub: CoreHub,
    dest_hub: CoreHub,
    manifest: &Option<ConstrainedPackResult>,
) -> Vec<u64> {
    let mut ids = Vec::with_capacity(2);
    if let Some(m) = manifest {
        if let Some(first) = m.packed.first() {
            crate::arbitrage::push_dedup_destination(&mut ids, first.deal.source_station_id);
            crate::arbitrage::push_dedup_destination(&mut ids, first.deal.dest_station_id);
            return ids;
        }
    }
    crate::arbitrage::push_dedup_destination(&mut ids, source_hub.primary_station_id());
    crate::arbitrage::push_dedup_destination(&mut ids, dest_hub.primary_station_id());
    ids
}

/// Formats a complete in-game flight sheet for EVE Notepad (Alt+N) for a hub trade run.
pub fn format_hub_flight_sheet(
    source_hub: CoreHub,
    dest_hub: CoreHub,
    safe_jumps: u32,
    manifest: &ConstrainedPackResult,
) -> String {
    let mut s = String::new();
    s.push_str("═══════════════════════════════════════════════════════════\n");
    s.push_str(&format!(
        "🏛️ HUB CORRIDOR RUN: {} ➔ {} (⚡ {} Jumps)\n",
        source_hub.name(),
        dest_hub.name(),
        safe_jumps
    ));
    s.push_str(&format!(
        "Total Profit: +{:.2}M ISK | Capital: {:.2}M ISK | Volume: {:.1} m³\n",
        manifest.total_profit / 1_000_000.0,
        manifest.total_capital / 1_000_000.0,
        manifest.total_volume
    ));
    s.push_str("═══════════════════════════════════════════════════════════\n\n");

    s.push_str(&format!(
        "▶ 1. BUY AT ORIGIN HUB: {} ({})\n",
        source_hub.name(),
        source_hub.primary_station_label()
    ));
    for (idx, slot) in manifest.packed.iter().enumerate() {
        s.push_str(&format!(
            "  [{}] BUY {}x {} @ {:.2} ISK (Cost: {:.2}M ISK, {:.1} m³)\n",
            idx + 1,
            slot.deal.quantity,
            slot.deal.item_name,
            slot.deal.buy_price,
            slot.capital / 1_000_000.0,
            slot.volume
        ));
    }
    s.push('\n');

    s.push_str(&format!(
        "▶ 2. WARP {} JUMPS & SELL AT DESTINATION HUB: {} ({})\n",
        safe_jumps,
        dest_hub.name(),
        dest_hub.primary_station_label()
    ));
    for (idx, slot) in manifest.packed.iter().enumerate() {
        s.push_str(&format!(
            "  [{}] SELL {}x {} @ {:.2} ISK (Gross: {:.2}M ISK, Profit: +{:.2}M ISK, {:.1}%)\n",
            idx + 1,
            slot.deal.quantity,
            slot.deal.item_name,
            slot.deal.sell_price,
            (slot.deal.sell_price * slot.deal.quantity as f64) / 1_000_000.0,
            slot.net_profit / 1_000_000.0,
            slot.deal.net_profit_pct
        ));
    }
    s.push('\n');

    s.push_str("═══════════════════════════════════════════════════════════\n");
    s.push_str(&format!(
        "Autopilot route: {} ({}) ➔ {} ({})\n",
        source_hub.name(),
        source_hub.primary_station_id(),
        dest_hub.name(),
        dest_hub.primary_station_id()
    ));
    s.push_str("═══════════════════════════════════════════════════════════\n");
    s
}
