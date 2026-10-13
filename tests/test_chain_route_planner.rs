use eve_trade_scout::arbitrage::ArbitrageDeal;
use eve_trade_scout::chain_route_planner::{
    detour_cost, find_chain_routes, knapsack_pack, ChainConfig,
};
use eve_trade_scout::graph::SystemGraph;
use eve_trade_scout::sde::SystemNode;
use std::collections::HashSet;

fn create_chain_test_graph() -> SystemGraph {
    let systems = vec![
        SystemNode {
            system_id: 30000142,
            name: "Jita".to_string(),
            security_status: 0.95,
            region_id: 10000002,
        },
        SystemNode {
            system_id: 30000144,
            name: "Perimeter".to_string(),
            security_status: 0.90,
            region_id: 10000002,
        },
        SystemNode {
            system_id: 30000145,
            name: "Motsu".to_string(), // Intermediate detour
            security_status: 0.85,
            region_id: 10000002,
        },
        SystemNode {
            system_id: 30002187,
            name: "Amarr".to_string(),
            security_status: 0.90,
            region_id: 10000043,
        },
        SystemNode {
            system_id: 30000032,
            name: "Dodixie".to_string(),
            security_status: 0.85,
            region_id: 10000032,
        },
        SystemNode {
            system_id: 30002056,
            name: "Tama".to_string(), // Lowsec
            security_status: 0.30,
            region_id: 10000033,
        },
    ];

    let jumps = vec![
        (30000142, 30000144), // Jita <-> Perimeter (1 jump)
        (30000144, 30002187), // Perimeter <-> Amarr (direct, 1 jump)
        (30000144, 30000145), // Perimeter <-> Motsu (1 jump)
        (30000145, 30002187), // Motsu <-> Amarr (1 jump) -> Perimeter -> Motsu -> Amarr is 2 jumps (1 detour)
        (30002187, 30000032), // Amarr <-> Dodixie (1 jump)
        (30000142, 30002056), // Jita <-> Tama (lowsec)
    ];

    SystemGraph::from_nodes_and_jumps(systems, jumps)
}

#[allow(clippy::too_many_arguments)]
fn make_chain_deal(
    type_id: u32,
    name: &str,
    src: (u32, &str),
    dst: (u32, &str),
    qty: u32,
    vol: f64,
    cost: f64,
    profit: f64,
) -> ArbitrageDeal {
    let roi = (profit / cost) * 100.0;
    ArbitrageDeal {
        type_id,
        item_name: name.to_string(),
        source_order_id: 100_000_000 + (type_id as u64 * 10_000) + (src.0 as u64 % 10_000),
        dest_order_id: 200_000_000 + (type_id as u64 * 10_000) + (dst.0 as u64 % 10_000),
        source_order_volume_remain: qty,
        dest_order_volume_remain: qty,
        dest_order_min_volume: 1,
        is_npc_trade: false,
        buy_price: cost / (qty as f64),
        sell_price: (cost + profit * 1.2) / (qty as f64),
        spread_pct: 30.0,
        source_station_id: 60000000 + src.0 as u64,
        source_system_id: src.0,
        source_system_name: src.1.to_string(),
        source_station_name: format!("{} Station", src.1),
        source_security: 0.9,
        dest_station_id: 60000000 + dst.0 as u64,
        dest_system_id: dst.0,
        dest_system_name: dst.1.to_string(),
        dest_station_name: format!("{} Station", dst.1),
        dest_security: 0.9,
        quantity: qty,
        volume_used: vol,
        capital_required: cost,
        gross_profit: profit * 1.2,
        taxes_and_fees: profit * 0.2,
        net_profit: profit,
        net_profit_pct: roi,
        jumps_from_home: 1,
        jumps_source_dest: 1,
        total_jumps: 2,
        profit_per_jump: profit / 2.0,
        profit_per_m3: profit / vol,
    }
}

#[test]
fn test_chain_no_deals_returns_empty() {
    let graph = create_chain_test_graph();
    let cfg = ChainConfig::default();
    let routes = find_chain_routes(&[], &graph, &cfg);
    assert!(routes.is_empty());
}

#[test]
fn test_chain_basic_single_leg() {
    let graph = create_chain_test_graph();
    let d1 = make_chain_deal(
        34,
        "Tritanium",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        1000,
        100.0,
        10_000.0,
        2_500.0,
    );

    let cfg = ChainConfig {
        home_system: 30000142,
        max_legs: 1,
        ..Default::default()
    };

    let routes = find_chain_routes(&[d1], &graph, &cfg);
    assert!(!routes.is_empty());
    let best = &routes[0];
    assert_eq!(best.legs.len(), 1);
    assert_eq!(best.legs[0].sell.system_id, 30000144);
    assert!((best.total_profit - 2_500.0).abs() < 1e-4);
}

#[test]
fn test_chain_knapsack_packs_multiple_deals_at_one_stop() {
    let d1 = make_chain_deal(
        34,
        "Tritanium",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        50.0,
        5_000.0,
        1_000.0,
    );
    let d2 = make_chain_deal(
        35,
        "Pyerite",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        30.0,
        3_000.0,
        900.0,
    );

    let used = HashSet::new();
    let (packed, total_vol, total_cap, total_profit) =
        knapsack_pack(&[&d1, &d2], 100.0, 50_000.0, &used);

    assert_eq!(packed.len(), 2);
    assert!((total_vol - 80.0).abs() < 1e-4);
    assert!((total_cap - 8_000.0).abs() < 1e-4);
    assert!((total_profit - 1_900.0).abs() < 1e-4);
}

#[test]
fn test_chain_knapsack_respects_volume_cap() {
    let d1 = make_chain_deal(
        34,
        "Bulky Item",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        10,
        100.0, // 10 m³ each
        10_000.0,
        3_000.0,
    );

    let used = HashSet::new();
    // Only 25 m³ available -> can only fit 2 units (20 m³)
    let (packed, total_vol, _, _) = knapsack_pack(&[&d1], 25.0, 50_000.0, &used);

    assert_eq!(packed.len(), 1);
    assert_eq!(packed[0].deal.quantity, 2);
    assert!((total_vol - 20.0).abs() < 1e-4);
}

#[test]
fn test_chain_knapsack_respects_budget_cap() {
    let d1 = make_chain_deal(
        34,
        "Expensive Item",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        10,
        10.0,
        10_000.0, // 1000 ISK each
        3_000.0,
    );

    let used = HashSet::new();
    // Only 2500 ISK budget -> can only fit 2 units
    let (packed, _, total_cap, _) = knapsack_pack(&[&d1], 100.0, 2500.0, &used);

    assert_eq!(packed.len(), 1);
    assert_eq!(packed[0].deal.quantity, 2);
    assert!((total_cap - 2000.0).abs() < 1e-4);
}

#[test]
fn test_chain_cargo_freed_after_sell() {
    let graph = create_chain_test_graph();
    // Leg 1: Jita -> Perimeter (fills 100 m³)
    let d1 = make_chain_deal(
        34,
        "Tritanium",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        100.0,
        10_000.0,
        2_000.0,
    );
    // Leg 2: Perimeter -> Amarr (fills 100 m³ again, possible only if freed)
    let d2 = make_chain_deal(
        35,
        "Pyerite",
        (30000144, "Perimeter"),
        (30002187, "Amarr"),
        100,
        100.0,
        10_000.0,
        3_000.0,
    );

    let cfg = ChainConfig {
        home_system: 30000142,
        max_legs: 2,
        ship_max_volume: 100.0, // exactly 100 m³ hold
        ..Default::default()
    };

    let routes = find_chain_routes(&[d1, d2], &graph, &cfg);
    assert!(!routes.is_empty());
    let best = &routes[0];
    assert_eq!(best.legs.len(), 2);
    assert!((best.total_profit - 5_000.0).abs() < 1e-4);
    assert_eq!(best.legs[0].sell.volume_freed, 100.0);
}

#[test]
fn test_chain_enroute_pickup_skipped_when_cargo_full() {
    let graph = create_chain_test_graph();
    // Perimeter -> Amarr (100 m³, saturates 100 m³ hold, 5,000 profit makes it dominant)
    let d1 = make_chain_deal(
        34,
        "Tritanium",
        (30000144, "Perimeter"),
        (30002187, "Amarr"),
        100,
        100.0,
        10_000.0,
        5_000.0,
    );
    // En-route Motsu -> Amarr
    let d2 = make_chain_deal(
        35,
        "Pyerite",
        (30000145, "Motsu"),
        (30002187, "Amarr"),
        50,
        50.0,
        5_000.0,
        1_500.0,
    );

    let cfg = ChainConfig {
        home_system: 30000144,
        max_legs: 1,
        ship_max_volume: 100.0,
        detour_enabled: true,
        max_detour_jumps: 2,
        ..Default::default()
    };

    let routes = find_chain_routes(&[d1, d2], &graph, &cfg);
    assert!(!routes.is_empty());
    let best = &routes[0];
    // Since cargo is 100% full at Perimeter, Motsu pickup must be skipped
    assert_eq!(best.legs[0].pickups.len(), 1);
    assert_eq!(best.legs[0].pickups[0].system_id, 30000144);
}

#[test]
fn test_chain_strict_en_route_only() {
    let graph = create_chain_test_graph();
    // Detour cost from Perimeter -> Motsu -> Amarr:
    // Perimeter <-> Amarr direct is 1 jump.
    // Perimeter <-> Motsu is 1, Motsu <-> Amarr is 1.
    // Detour is 1 + 1 - 1 = 1 jump.
    let cost = detour_cost(30000144, 30000145, 30002187, &graph, true);
    assert_eq!(cost, 1);

    let d1 = make_chain_deal(
        34,
        "Tritanium",
        (30000144, "Perimeter"),
        (30002187, "Amarr"),
        50,
        50.0,
        5_000.0,
        5_000.0, // Dominant profit makes Perimeter primary
    );
    let d2 = make_chain_deal(
        35,
        "Pyerite",
        (30000145, "Motsu"),
        (30002187, "Amarr"),
        50,
        50.0,
        5_000.0,
        1_500.0,
    );

    // With detour_enabled = false (strict mode), Motsu (detour 1) must NOT be picked up
    let cfg = ChainConfig {
        home_system: 30000144,
        max_legs: 1,
        ship_max_volume: 200.0,
        detour_enabled: false,
        ..Default::default()
    };

    let routes = find_chain_routes(&[d1.clone(), d2.clone()], &graph, &cfg);
    assert!(!routes.is_empty());
    let best = &routes[0];
    // In strict mode, only Perimeter deal is picked up
    assert_eq!(best.legs[0].pickups.len(), 1);
    assert_eq!(best.legs[0].pickups[0].system_id, 30000144);
}

#[test]
fn test_chain_detour_filter_excludes_too_far() {
    let graph = create_chain_test_graph();
    // Motsu detour is 1. If max_detour_jumps is 0, it should be excluded even if detour_enabled is true
    let d1 = make_chain_deal(
        34,
        "Tritanium",
        (30000144, "Perimeter"),
        (30002187, "Amarr"),
        50,
        50.0,
        5_000.0,
        1_000.0,
    );
    let d2 = make_chain_deal(
        35,
        "Pyerite",
        (30000145, "Motsu"),
        (30002187, "Amarr"),
        50,
        50.0,
        5_000.0,
        1_500.0,
    );

    let cfg = ChainConfig {
        home_system: 30000144,
        max_legs: 1,
        ship_max_volume: 200.0,
        detour_enabled: true,
        max_detour_jumps: 0, // Max 0 jumps detour -> excludes Motsu
        ..Default::default()
    };

    let routes = find_chain_routes(&[d1, d2], &graph, &cfg);
    assert!(!routes.is_empty());
    let best = &routes[0];
    assert_eq!(best.legs[0].pickups.len(), 1);
}

#[test]
fn test_chain_detour_zero_equals_strict_mode() {
    let graph = create_chain_test_graph();
    let d1 = make_chain_deal(
        34,
        "Tritanium",
        (30000144, "Perimeter"),
        (30002187, "Amarr"),
        50,
        50.0,
        5_000.0,
        1_000.0,
    );
    let d2 = make_chain_deal(
        35,
        "Pyerite",
        (30000145, "Motsu"),
        (30002187, "Amarr"),
        50,
        50.0,
        5_000.0,
        1_500.0,
    );

    let cfg_strict = ChainConfig {
        home_system: 30000144,
        max_legs: 1,
        ship_max_volume: 200.0,
        detour_enabled: false,
        max_detour_jumps: 2,
        ..Default::default()
    };
    let cfg_zero = ChainConfig {
        home_system: 30000144,
        max_legs: 1,
        ship_max_volume: 200.0,
        detour_enabled: true,
        max_detour_jumps: 0,
        ..Default::default()
    };

    let r_strict = find_chain_routes(&[d1.clone(), d2.clone()], &graph, &cfg_strict);
    let r_zero = find_chain_routes(&[d1, d2], &graph, &cfg_zero);

    assert_eq!(r_strict.len(), r_zero.len());
    assert!(
        !r_strict.is_empty(),
        "Chain routes must not be empty in strict mode"
    );
    assert_eq!(r_strict[0].total_profit, r_zero[0].total_profit);
    assert_eq!(r_strict[0].total_jumps, r_zero[0].total_jumps);
}

#[test]
fn test_chain_safe_only_excludes_lowsec_paths() {
    let graph = create_chain_test_graph();
    // Deal to Tama (security 0.30 - lowsec)
    let d_lowsec = make_chain_deal(
        34,
        "Tritanium",
        (30000142, "Jita"),
        (30002056, "Tama"),
        100,
        50.0,
        5_000.0,
        2_000.0,
    );

    let cfg_safe = ChainConfig {
        home_system: 30000142,
        safe_only: true,
        ..Default::default()
    };

    let routes = find_chain_routes(&[d_lowsec], &graph, &cfg_safe);
    assert!(routes.is_empty());
}

#[test]
fn test_chain_max_legs_respected() {
    let graph = create_chain_test_graph();
    let d1 = make_chain_deal(
        34,
        "D1",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        10,
        10.0,
        1_000.0,
        500.0,
    );
    let d2 = make_chain_deal(
        35,
        "D2",
        (30000144, "Perimeter"),
        (30002187, "Amarr"),
        10,
        10.0,
        1_000.0,
        500.0,
    );
    let d3 = make_chain_deal(
        36,
        "D3",
        (30002187, "Amarr"),
        (30000032, "Dodixie"),
        10,
        10.0,
        1_000.0,
        500.0,
    );

    let cfg = ChainConfig {
        home_system: 30000142,
        max_legs: 2, // Max 2 legs even though 3 sequential deals exist
        ..Default::default()
    };

    let routes = find_chain_routes(&[d1, d2, d3], &graph, &cfg);
    assert!(!routes.is_empty());
    for r in &routes {
        assert!(r.legs.len() <= 2);
    }
}

#[test]
fn test_chain_10pct_roi_invariant_maintained() {
    let graph = create_chain_test_graph();
    // Sub-10% ROI deal: 5% ROI
    let d_sub10 = make_chain_deal(
        34,
        "Low ROI",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        50.0,
        10_000.0,
        500.0, // 5% ROI (below 10.0% invariant)
    );

    let cfg = ChainConfig {
        home_system: 30000142,
        ..Default::default()
    };

    let routes = find_chain_routes(&[d_sub10], &graph, &cfg);
    assert!(routes.is_empty());
}

#[test]
fn test_chain_cannot_multiply_same_source_order_in_single_stop() {
    let mut d1 = make_chain_deal(
        9850,
        "Spirits",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        50.0,
        10_000.0,
        2_500.0,
    );
    d1.source_order_id = 99991;
    d1.source_order_volume_remain = 100;
    d1.dest_order_id = 88881;
    d1.dest_order_volume_remain = 100;

    let mut d2 = make_chain_deal(
        9850,
        "Spirits",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        50.0,
        10_000.0,
        2_500.0,
    );
    // Same source order!
    d2.source_order_id = 99991;
    d2.source_order_volume_remain = 100;
    d2.dest_station_id = 60000099; // Different destination station
    d2.dest_order_id = 88882;
    d2.dest_order_volume_remain = 100;

    let used = HashSet::new();
    let (packed, total_vol, total_cap, total_profit) =
        knapsack_pack(&[&d1, &d2], 200.0, 50_000.0, &used);

    // Because both share the same source order of 100 units, total packed cannot exceed 100 units!
    let total_packed_units: u32 = packed.iter().map(|s| s.deal.quantity).sum();
    assert_eq!(
        total_packed_units, 100,
        "Should not exceed the single order volume of 100 units"
    );
    assert!((total_vol - 50.0).abs() < 1e-4);
    assert!((total_cap - 10_000.0).abs() < 1e-4);
    assert!((total_profit - 2_500.0).abs() < 1e-4);
}

#[test]
fn test_chain_cannot_rebuy_same_station_order_on_subsequent_legs() {
    let graph = create_chain_test_graph();

    // Deal 1: Jita Station -> Perimeter (Spirits)
    let mut d1 = make_chain_deal(
        9850,
        "Spirits",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        50.0,
        10_000.0,
        3_000.0,
    );
    d1.source_station_id = 60003760;
    d1.source_order_id = 99991;
    d1.source_order_volume_remain = 100;

    // Deal 2: Jita Station -> Amarr (Spirits) - shares same station & item
    let mut d2 = make_chain_deal(
        9850,
        "Spirits",
        (30000142, "Jita"),
        (30002187, "Amarr"),
        100,
        50.0,
        10_000.0,
        4_000.0,
    );
    d2.source_station_id = 60003760;
    d2.source_order_id = 99991;
    d2.source_order_volume_remain = 100;

    // Deal 3: Perimeter -> Amarr (Food)
    let d3 = make_chain_deal(
        3715,
        "Frozen Food",
        (30000144, "Perimeter"),
        (30002187, "Amarr"),
        100,
        50.0,
        10_000.0,
        2_500.0,
    );

    let cfg = ChainConfig {
        home_system: 30000142,
        max_legs: 3,
        detour_enabled: false,
        ..Default::default()
    };

    let routes = find_chain_routes(&[d1, d2, d3], &graph, &cfg);
    assert!(!routes.is_empty());

    // Verify no route visits Jita Station a second time to re-buy Spirits
    for route in &routes {
        let mut spirits_pickups_at_jita = 0;
        for leg in &route.legs {
            for pickup in &leg.pickups {
                if pickup.system_id == 30000142 {
                    for slot in &pickup.cargo_loaded {
                        if slot.deal.type_id == 9850 && slot.deal.source_station_id == 60003760 {
                            spirits_pickups_at_jita += 1;
                        }
                    }
                }
            }
        }
        assert!(
            spirits_pickups_at_jita <= 1,
            "Must never buy the deal multiple times at the same station! Found {}",
            spirits_pickups_at_jita
        );
    }
}

#[test]
fn test_chain_system_and_station_finite_stock_enforced() {
    let graph = create_chain_test_graph();

    // In Jita, Station 1 sells Spirits (100 units)
    let mut d1 = make_chain_deal(
        9850,
        "Spirits",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        50.0,
        10_000.0,
        3_000.0,
    );
    d1.source_station_id = 60003760;
    d1.source_order_id = 99991;
    d1.source_order_volume_remain = 100;

    // In Jita, Station 2 ALSO sells Spirits (separate station in same system, 100 units)
    let mut d2 = make_chain_deal(
        9850,
        "Spirits",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        50.0,
        10_000.0,
        3_000.0,
    );
    d2.source_station_id = 60003761; // Different station!
    d2.source_order_id = 99992;
    d2.source_order_volume_remain = 100;

    let cfg = ChainConfig {
        home_system: 30000142,
        max_legs: 2,
        ..Default::default()
    };

    let routes = find_chain_routes(&[d1, d2], &graph, &cfg);
    assert!(!routes.is_empty());
    // Station 1 and Station 2 can each be bought once (up to 200 units total),
    // but neither station can be bought multiple times
    for route in &routes {
        for leg in &route.legs {
            for pickup in &leg.pickups {
                let st1_qty: u32 = pickup
                    .cargo_loaded
                    .iter()
                    .filter(|s| s.deal.source_station_id == 60003760)
                    .map(|s| s.deal.quantity)
                    .sum();
                let st2_qty: u32 = pickup
                    .cargo_loaded
                    .iter()
                    .filter(|s| s.deal.source_station_id == 60003761)
                    .map(|s| s.deal.quantity)
                    .sum();
                assert!(st1_qty <= 100, "Station 1 stock must not be exceeded");
                assert!(st2_qty <= 100, "Station 2 stock must not be exceeded");
            }
        }
    }
}

#[test]
fn test_chain_capital_recycled_after_delivery() {
    let graph = create_chain_test_graph();

    // Deal 1: Jita -> Perimeter
    // Capital required: 10,000 ISK, Net profit: 3,000 ISK
    let d1 = make_chain_deal(
        1001,
        "Item Leg 1",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        50.0,
        10_000.0,
        3_000.0,
    );

    // Deal 2: Perimeter -> Amarr
    // Capital required: 10,000 ISK, Net profit: 3,000 ISK
    let d2 = make_chain_deal(
        1002,
        "Item Leg 2",
        (30000144, "Perimeter"),
        (30002187, "Amarr"),
        100,
        50.0,
        10_000.0,
        3_000.0,
    );

    // Set budget limit strictly to 12,000 ISK.
    // If capital is NOT recycled upon delivery in Perimeter:
    // Remaining ISK after buying Leg 1 = 12,000 - 10,000 = 2,000 ISK.
    // If only net profit (3,000) was added, balance would be 5,000 ISK < 10,000 ISK, so Leg 2 could NEVER be bought!
    // But with capital recycled (10,000 capital + 3,000 profit freed), balance is 12,000 ISK (capped at max budget),
    // which allows purchasing Deal 2!
    let cfg = ChainConfig {
        home_system: 30000142,
        max_legs: 2,
        ship_max_budget: 12_000.0,
        ship_max_volume: 5_000.0,
        ..Default::default()
    };

    let routes = find_chain_routes(&[d1, d2], &graph, &cfg);
    assert!(!routes.is_empty(), "Should find a 2-leg chain route");
    let best = &routes[0];
    assert_eq!(
        best.legs.len(),
        2,
        "Chain must have 2 legs because capital was freed and recycled"
    );
    assert_eq!(best.legs[0].sell.system_name, "Perimeter");
    assert_eq!(best.legs[1].sell.system_name, "Amarr");
}

#[test]
fn test_chain_knapsack_respects_dest_order_min_volume() {
    let graph = create_chain_test_graph();

    let mut d1 = make_chain_deal(
        1001,
        "Item Min Volume",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        100.0,
        10_000.0,
        3_000.0,
    );
    // Buyer requires minimum 50 units
    d1.dest_order_min_volume = 50;

    // Constrain volume so that ship can only fit 20 units (volume cap = 20.0 m³, item is 1 m³/unit)
    let cfg = ChainConfig {
        home_system: 30000142,
        max_legs: 1,
        ship_max_budget: 50_000.0,
        ship_max_volume: 20.0, // Cannot fit 50 units!
        ..Default::default()
    };

    let routes = find_chain_routes(&[d1.clone()], &graph, &cfg);
    assert!(
        routes.is_empty(),
        "Should reject deal because ship capacity cannot meet buyer's min_volume requirement"
    );

    // Now give enough cargo space to fit 60 units (>= 50 min_volume)
    let cfg_ok = ChainConfig {
        home_system: 30000142,
        max_legs: 1,
        ship_max_budget: 50_000.0,
        ship_max_volume: 60.0,
        ..Default::default()
    };

    let routes_ok = find_chain_routes(&[d1], &graph, &cfg_ok);
    assert!(
        !routes_ok.is_empty(),
        "Should accept deal when ship capacity can meet min_volume"
    );
}

#[test]
fn test_chain_cannot_exceed_max_dest_loads_on_subsequent_legs() {
    let graph = create_chain_test_graph();

    // Deal A1: Jita (Station 1) -> Perimeter (Station 60000144) (Spirits, type_id 1001)
    let mut a1 = make_chain_deal(
        1001,
        "Spirits 1",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        100.0,
        10_000.0,
        5_000.0,
    );
    a1.source_order_id = 101;
    a1.dest_order_id = 1101;
    a1.source_station_id = 60000142;
    a1.dest_station_id = 60000144;

    // Deal B1: Perimeter -> Jita (Pyerite)
    let mut b1 = make_chain_deal(
        2001,
        "Pyerite 1",
        (30000144, "Perimeter"),
        (30000142, "Jita"),
        100,
        100.0,
        10_000.0,
        5_000.0,
    );
    b1.source_order_id = 201;
    b1.dest_order_id = 1201;
    b1.source_station_id = 60000144;
    b1.dest_station_id = 60000142;

    // Deal A2: Jita (Station 2) -> Perimeter (Station 60000144) (Spirits load 2, type_id 1001)
    let mut a2 = make_chain_deal(
        1001,
        "Spirits 2",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        100.0,
        10_000.0,
        5_000.0,
    );
    a2.source_order_id = 102;
    a2.dest_order_id = 1102;
    a2.source_station_id = 60000143; // different source station to avoid source station exhaustion
    a2.dest_station_id = 60000144;

    // Deal B2: Perimeter -> Jita (Mexallon)
    let mut b2 = make_chain_deal(
        2002,
        "Mexallon 2",
        (30000144, "Perimeter"),
        (30000142, "Jita"),
        100,
        100.0,
        10_000.0,
        5_000.0,
    );
    b2.source_order_id = 202;
    b2.dest_order_id = 1202;
    b2.source_station_id = 60000148; // different source station at Perimeter
    b2.dest_station_id = 60000142;

    // Deal A3: Jita (Station 3) -> Perimeter (Station 60000144) (Spirits load 3, type_id 1001 - MUST BE BLOCKED)
    let mut a3 = make_chain_deal(
        1001,
        "Spirits 3 (Excess Load)",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        100.0,
        10_000.0,
        5_000.0,
    );
    a3.source_order_id = 103;
    a3.dest_order_id = 1103;
    a3.source_station_id = 60000145; // different source station
    a3.dest_station_id = 60000144;

    let cfg = ChainConfig {
        home_system: 30000142,
        max_legs: 5,
        max_dest_loads: 2,
        ship_max_budget: 50_000.0,
        ship_max_volume: 100.0, // Each leg fits exactly one deal
        ..Default::default()
    };

    let routes = find_chain_routes(
        &[a1.clone(), b1.clone(), a2.clone(), b2.clone(), a3.clone()],
        &graph,
        &cfg,
    );
    assert!(!routes.is_empty(), "Should find chain routes");

    // Check all routes: none should have 5 legs because A3 is blocked after 2 loads of Spirits to Perimeter!
    for r in &routes {
        assert!(
            r.legs.len() <= 4,
            "Route cannot have 5 legs because 3rd load of Spirits is blocked"
        );
        let spirits_to_perimeter_count =
            r.legs
                .iter()
                .filter(|leg| {
                    leg.sell.cargo_sold.iter().any(|slot| {
                        slot.deal.type_id == 1001 && slot.deal.dest_station_id == 60000144
                    })
                })
                .count();
        assert!(
            spirits_to_perimeter_count <= 2,
            "Cannot deliver more than 2 loads of Spirits to Perimeter station"
        );
    }

    // Now add Deal C: Jita -> Perimeter (Wheat, type_id 3001) which has 0 loads delivered to Perimeter
    let mut c = make_chain_deal(
        3001,
        "Wheat",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        100.0,
        10_000.0,
        5_000.0,
    );
    c.source_order_id = 301;
    c.dest_order_id = 1301;
    c.source_station_id = 60000146;
    c.dest_station_id = 60000144;

    let routes_with_c = find_chain_routes(&[a1, b1, a2, b2, a3, c], &graph, &cfg);
    // Filter for routes that achieved 5 legs
    let five_leg_routes: Vec<_> = routes_with_c.iter().filter(|r| r.legs.len() == 5).collect();
    assert!(
        !five_leg_routes.is_empty(),
        "Should successfully find a 5-leg route when distinct commodity (Wheat) is available"
    );
    let five_leg = five_leg_routes[0];
    assert_eq!(five_leg.legs[4].sell.cargo_sold[0].deal.item_name, "Wheat");
}

#[test]
fn test_chain_max_dest_loads_one_strictly_enforced() {
    let graph = create_chain_test_graph();

    let mut a1 = make_chain_deal(
        1001,
        "Spirits 1",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        50.0,
        10_000.0,
        5_000.0,
    );
    a1.source_order_id = 101;
    a1.source_station_id = 60000142;
    a1.dest_station_id = 60000144;

    let mut b1 = make_chain_deal(
        2001,
        "Pyerite",
        (30000144, "Perimeter"),
        (30000142, "Jita"),
        100,
        50.0,
        10_000.0,
        4_000.0,
    );
    b1.source_order_id = 201;
    b1.source_station_id = 60000144;
    b1.dest_station_id = 60000142;

    let mut a2 = make_chain_deal(
        1001,
        "Spirits 2",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        50.0,
        10_000.0,
        3_000.0,
    );
    a2.source_order_id = 102;
    a2.source_station_id = 60000143;
    a2.dest_station_id = 60000144;

    let cfg = ChainConfig {
        home_system: 30000142,
        max_legs: 3,
        max_dest_loads: 1, // Strictly capped at 1 load
        ship_max_budget: 100_000.0,
        ship_max_volume: 5_000.0,
        ..Default::default()
    };

    let routes = find_chain_routes(&[a1, b1, a2], &graph, &cfg);
    assert!(!routes.is_empty(), "Should find chain routes");
    let has_two_leg = routes.iter().any(|r| r.legs.len() == 2);
    assert!(has_two_leg, "Should find 2-leg route");
    let max_legs = routes.iter().map(|r| r.legs.len()).max().unwrap_or(0);
    assert_eq!(
        max_legs, 2,
        "Cannot form 3rd leg because max_dest_loads=1 blocks second load of Spirits to Perimeter"
    );
}

#[test]
fn test_chain_different_items_same_destination_allowed() {
    let graph = create_chain_test_graph();

    let mut a = make_chain_deal(
        1001,
        "Spirits",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        50,
        25.0,
        5_000.0,
        2_500.0,
    );
    a.dest_station_id = 60000144;

    let mut b = make_chain_deal(
        1002,
        "Wheat",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        50,
        25.0,
        5_000.0,
        2_000.0,
    );
    b.dest_station_id = 60000144;

    let cfg = ChainConfig {
        home_system: 30000142,
        max_legs: 1,
        max_dest_loads: 1,
        ship_max_budget: 50_000.0,
        ship_max_volume: 500.0,
        ..Default::default()
    };

    let routes = find_chain_routes(&[a, b], &graph, &cfg);
    assert!(
        !routes.is_empty(),
        "Should find route packing both deals to Perimeter"
    );
    let best = &routes[0];
    assert_eq!(
        best.legs[0].pickups[0].cargo_loaded.len(),
        2,
        "Both distinct items delivered to same destination station"
    );
}

#[test]
fn test_chain_include_npc_toggle_disabled() {
    let graph = create_chain_test_graph();

    // Deal 1: NPC trade (e.g. Spirits)
    let mut d_npc = make_chain_deal(
        9850,
        "Spirits (NPC Commodity)",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        50.0,
        10_000.0,
        8_000.0, // Very lucrative
    );
    d_npc.is_npc_trade = true;

    // Deal 2: Player market deal (e.g. Tritanium)
    let mut d_player = make_chain_deal(
        34,
        "Tritanium (Player Market)",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        50.0,
        10_000.0,
        3_000.0,
    );
    d_player.is_npc_trade = false;

    // Config with include_npc = false
    let cfg = ChainConfig {
        home_system: 30000142,
        max_legs: 1,
        include_npc: false,
        ship_max_budget: 50_000.0,
        ship_max_volume: 500.0,
        ..Default::default()
    };

    let routes = find_chain_routes(&[d_npc, d_player], &graph, &cfg);
    assert!(!routes.is_empty(), "Should find route with player deal");
    let best = &routes[0];
    assert_eq!(best.legs[0].sell.cargo_sold.len(), 1);
    assert_eq!(
        best.legs[0].sell.cargo_sold[0].deal.item_name, "Tritanium (Player Market)",
        "NPC trade must be completely excluded when include_npc is false"
    );
}

#[test]
fn test_chain_include_npc_toggle_enabled() {
    let graph = create_chain_test_graph();

    let mut d_npc = make_chain_deal(
        9850,
        "Spirits (NPC Commodity)",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        50.0,
        10_000.0,
        8_000.0, // More lucrative than player deal
    );
    d_npc.is_npc_trade = true;

    let mut d_player = make_chain_deal(
        34,
        "Tritanium (Player Market)",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        50.0,
        10_000.0,
        3_000.0,
    );
    d_player.is_npc_trade = false;

    // Config with include_npc = true (default)
    let cfg = ChainConfig {
        home_system: 30000142,
        max_legs: 1,
        include_npc: true,
        ship_max_budget: 50_000.0,
        ship_max_volume: 50.0, // Only fits 1 deal
        ..Default::default()
    };

    let routes = find_chain_routes(&[d_npc, d_player], &graph, &cfg);
    assert!(!routes.is_empty(), "Should find route");
    let best = &routes[0];
    assert_eq!(
        best.legs[0].sell.cargo_sold[0].deal.item_name, "Spirits (NPC Commodity)",
        "Higher profit NPC commodity should be chosen when include_npc is true"
    );
}

#[test]
fn test_continuous_onward_chaining_no_distant_leaps() {
    // Build graph:
    // Local chain: 100 (Home) <-> 101 (Dest 1) <-> 102 (Dest 2)
    // Distant chain: 100 -> ... 40 jumps ... -> 200 <-> 201
    let mut systems = Vec::new();
    let mut jumps = Vec::new();

    systems.push(SystemNode {
        system_id: 100,
        name: "Home".to_string(),
        security_status: 0.9,
        region_id: 10000002,
    });
    systems.push(SystemNode {
        system_id: 101,
        name: "Dest 1".to_string(),
        security_status: 0.9,
        region_id: 10000002,
    });
    systems.push(SystemNode {
        system_id: 102,
        name: "Dest 2".to_string(),
        security_status: 0.9,
        region_id: 10000002,
    });
    jumps.push((100, 101));
    jumps.push((101, 102));

    // 40 jumps from 100 to 200
    let mut prev = 100;
    for id in 1000..1040 {
        systems.push(SystemNode {
            system_id: id,
            name: format!("Inter {}", id),
            security_status: 0.9,
            region_id: 10000002,
        });
        jumps.push((prev, id));
        prev = id;
    }
    systems.push(SystemNode {
        system_id: 200,
        name: "Distant Hub".to_string(),
        security_status: 0.9,
        region_id: 10000003,
    });
    systems.push(SystemNode {
        system_id: 201,
        name: "Distant Dest".to_string(),
        security_status: 0.9,
        region_id: 10000003,
    });
    jumps.push((prev, 200));
    jumps.push((200, 201));

    let graph = SystemGraph::from_nodes_and_jumps(systems, jumps);

    // Deal 1: 100 -> 101 (Leg 1)
    let d1 = make_chain_deal(
        34,
        "Leg 1 Item",
        (100, "Home"),
        (101, "Dest 1"),
        100,
        100.0,
        10_000.0,
        20_000.0,
    );

    // Deal 2: 101 -> 102 (Continuous onward from Dest 1)
    let d2 = make_chain_deal(
        35,
        "Leg 2 Local Item",
        (101, "Dest 1"),
        (102, "Dest 2"),
        100,
        100.0,
        10_000.0,
        25_000.0,
    );

    // Distant Deal: 200 -> 201 (Huge profit, but 40+ jumps away!)
    let d_distant = make_chain_deal(
        36,
        "Distant Megaprofit",
        (200, "Distant Hub"),
        (201, "Distant Dest"),
        100,
        100.0,
        10_000.0,
        500_000.0,
    );

    let cfg = ChainConfig {
        home_system: 100,
        max_legs: 2,
        ship_max_volume: 5_000.0,
        ship_max_budget: 100_000.0,
        ..Default::default()
    };

    let routes = find_chain_routes(&[d1, d2, d_distant], &graph, &cfg);
    assert!(!routes.is_empty(), "Should discover chained routes");

    // The 2-leg route must chain continuously from 100 -> 101 -> 102!
    let two_leg = routes
        .iter()
        .find(|r| r.legs.len() == 2)
        .expect("Must find 2-leg route");
    assert_eq!(two_leg.legs[0].pickups[0].system_id, 100);
    assert_eq!(two_leg.legs[0].sell.system_id, 101);
    assert_eq!(
        two_leg.legs[1].pickups[0].system_id, 101,
        "Leg 2 MUST pick up at Dest 1 (101) onward, NOT leap 40+ jumps to Distant Hub (200)"
    );
    assert_eq!(
        two_leg.legs[1].pickups[0].jumps_from_prev, 0,
        "Leg 2 must start seamlessly from the arrival system (0 empty reposition jumps)"
    );
    assert_eq!(two_leg.legs[1].sell.system_id, 102);
}

#[test]
fn test_chain_stops_when_no_continuous_trades_nearby() {
    let mut systems = Vec::new();
    let mut jumps = Vec::new();

    // 100 (Home) -> 101 (Dest)
    systems.push(SystemNode {
        system_id: 100,
        name: "Home".to_string(),
        security_status: 0.9,
        region_id: 10000002,
    });
    systems.push(SystemNode {
        system_id: 101,
        name: "Deadend Dest".to_string(),
        security_status: 0.9,
        region_id: 10000002,
    });
    jumps.push((100, 101));

    // Linear chain of 8 jumps away from 101: 101 -> 102 -> ... -> 108
    for id in 102..=108 {
        systems.push(SystemNode {
            system_id: id,
            name: format!("Sys {}", id),
            security_status: 0.9,
            region_id: 10000002,
        });
        jumps.push((id - 1, id));
    }
    systems.push(SystemNode {
        system_id: 109,
        name: "Far Dest".to_string(),
        security_status: 0.9,
        region_id: 10000002,
    });
    jumps.push((108, 109));

    let graph = SystemGraph::from_nodes_and_jumps(systems, jumps);

    // Deal 1: 100 -> 101
    let d1 = make_chain_deal(
        34,
        "Leg 1 Item",
        (100, "Home"),
        (101, "Deadend Dest"),
        100,
        100.0,
        10_000.0,
        20_000.0,
    );

    // Deal 2: 108 -> 109 (7 jumps away from 101, beyond local proximity limit of 2 jumps)
    let d2 = make_chain_deal(
        35,
        "Far Item",
        (108, "Sys 108"),
        (109, "Far Dest"),
        100,
        100.0,
        10_000.0,
        50_000.0,
    );

    let cfg = ChainConfig {
        home_system: 100,
        max_legs: 3,
        ..Default::default()
    };

    let routes = find_chain_routes(&[d1, d2], &graph, &cfg);
    assert!(!routes.is_empty(), "Should find 1-leg route");
    // All routes must conclude cleanly at 101, without hopping 7 empty jumps to 108!
    let max_legs = routes.iter().map(|r| r.legs.len()).max().unwrap_or(0);
    assert_eq!(
        max_legs, 1,
        "Chain route must conclude at destination when no continuous trades exist within 2 jumps"
    );
}

#[test]
fn test_chain_corridor_scored_by_ship_capacity_not_unconstrained_sum() {
    let mut systems = Vec::new();
    let mut jumps = Vec::new();

    systems.push(SystemNode {
        system_id: 100,
        name: "Home".to_string(),
        security_status: 0.9,
        region_id: 10000002,
    });
    systems.push(SystemNode {
        system_id: 101,
        name: "LocalDest".to_string(),
        security_status: 0.9,
        region_id: 10000002,
    });
    jumps.push((100, 101));

    // Linear chain 100 -> 102 -> 103 -> 104 -> 105 -> 200 -> 201 (6 jumps total to 201)
    for id in 102..=105 {
        systems.push(SystemNode {
            system_id: id,
            name: format!("Sys {}", id),
            security_status: 0.9,
            region_id: 10000002,
        });
    }
    systems.push(SystemNode {
        system_id: 200,
        name: "DistantSrc".to_string(),
        security_status: 0.9,
        region_id: 10000002,
    });
    systems.push(SystemNode {
        system_id: 201,
        name: "DistantDest".to_string(),
        security_status: 0.9,
        region_id: 10000002,
    });

    jumps.push((100, 102));
    jumps.push((102, 103));
    jumps.push((103, 104));
    jumps.push((104, 105));
    jumps.push((105, 200)); // 100 -> 200 is 5 jumps
    jumps.push((200, 201)); // 200 -> 201 is 1 jump (total 6 jumps)

    let graph = SystemGraph::from_nodes_and_jumps(systems, jumps);

    // Local Deal: 100 -> 101. Fits 100% in a 100 m³ ship hold. Profit: 15,000 ISK in 1 jump -> 15,000 PPJ!
    let d_local = make_chain_deal(
        34,
        "Local Item",
        (100, "Home"),
        (101, "LocalDest"),
        100,
        100.0,
        10_000.0,
        15_000.0,
    );

    // Distant Deals: 200 -> 201.
    let mut deals = vec![d_local];
    for i in 1..=10 {
        deals.push(make_chain_deal(
            100 + i,
            &format!("Distant Hub Item {}", i),
            (200, "DistantSrc"),
            (201, "DistantDest"),
            100,
            100.0,
            10_000.0,
            10_000.0,
        ));
    }

    let cfg = ChainConfig {
        home_system: 100,
        max_legs: 1,
        ship_max_volume: 100.0, // Strictly fits only 100 m³!
        ship_max_budget: 100_000.0,
        ..Default::default()
    };

    let routes = find_chain_routes(&deals, &graph, &cfg);
    assert!(!routes.is_empty(), "Should find a chain route");
    let best = &routes[0];
    assert_eq!(
        best.legs[0].pickups[0].system_id, 100,
        "Optimizer must pick local high-PPJ corridor over distant hub"
    );
    assert_eq!(best.legs[0].sell.system_id, 101);
    assert!((best.profit_per_jump - 15_000.0).abs() < 1e-4);
}

#[test]
fn test_chain_routes_sorted_by_profit_per_jump_descending() {
    let graph = create_chain_test_graph();

    // Deal 1: Fast local trade (Jita -> Perimeter): 1 jump, 4,000 profit -> 4,000 PPJ
    let d1 = make_chain_deal(
        34,
        "Fast High-PPJ Item",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        10,
        10.0,
        1_000.0,
        4_000.0,
    );

    // Deal 2: Perimeter -> Amarr: 1 jump, 1,000 profit
    // Combined 2-leg route: 5,000 total profit in 2 jumps -> 2,500 PPJ
    let d2 = make_chain_deal(
        35,
        "Slow Diluting Item",
        (30000144, "Perimeter"),
        (30002187, "Amarr"),
        10,
        10.0,
        1_000.0,
        1_000.0,
    );

    let cfg_ppj = ChainConfig {
        home_system: 30000142,
        max_legs: 2,
        ..Default::default()
    };

    let routes = find_chain_routes(&[d1, d2], &graph, &cfg_ppj);
    assert!(routes.len() >= 2, "Should find both 1-leg and 2-leg routes");

    // Routes must be ordered by profit_per_jump DESC!
    assert!(
        routes[0].profit_per_jump >= routes[1].profit_per_jump,
        "Routes must be ordered by profit_per_jump DESC"
    );
    assert_eq!(
        routes[0].legs.len(),
        1,
        "1-leg route has 4,000 PPJ vs 2,500 PPJ for 2-leg route"
    );
}

#[test]
fn test_chain_prefers_higher_ppj_deal_over_closer_low_ppj_deal() {
    let mut systems = Vec::new();
    let mut jumps = Vec::new();

    // Home: 100 (Geras)
    systems.push(SystemNode {
        system_id: 100,
        name: "Geras".to_string(),
        security_status: 0.7,
        region_id: 10000002,
    });

    // Poinen: 200 (2 jumps from Geras: 100 -> 101 -> 200)
    systems.push(SystemNode {
        system_id: 101,
        name: "Way1".to_string(),
        security_status: 0.8,
        region_id: 10000002,
    });
    systems.push(SystemNode {
        system_id: 200,
        name: "Poinen".to_string(),
        security_status: 0.6,
        region_id: 10000002,
    });
    jumps.push((100, 101));
    jumps.push((101, 200));

    // Oimmo: 208 (8 jumps from Poinen: 200 -> 201 -> ... -> 208)
    for id in 201..=208 {
        systems.push(SystemNode {
            system_id: id,
            name: if id == 208 {
                "Oimmo".to_string()
            } else {
                format!("Sys {}", id)
            },
            security_status: 0.6,
            region_id: 10000002,
        });
        jumps.push((id - 1, id));
    }

    // Josameto: 300 (3 jumps from Geras: 100 -> 102 -> 103 -> 300)
    systems.push(SystemNode {
        system_id: 102,
        name: "Way2".to_string(),
        security_status: 0.8,
        region_id: 10000002,
    });
    systems.push(SystemNode {
        system_id: 103,
        name: "Way3".to_string(),
        security_status: 0.8,
        region_id: 10000002,
    });
    systems.push(SystemNode {
        system_id: 300,
        name: "Josameto".to_string(),
        security_status: 0.6,
        region_id: 10000002,
    });
    jumps.push((100, 102));
    jumps.push((102, 103));
    jumps.push((103, 300));

    // Tuurlainas: 304 (4 jumps from Josameto: 300 -> 301 -> 302 -> 303 -> 304)
    for id in 301..=304 {
        systems.push(SystemNode {
            system_id: id,
            name: if id == 304 {
                "Tuurlainas".to_string()
            } else {
                format!("Sys {}", id)
            },
            security_status: 0.6,
            region_id: 10000002,
        });
        jumps.push((id - 1, id));
    }

    let graph = SystemGraph::from_nodes_and_jumps(systems, jumps);

    // Deal 1: Protein Delicacies at Poinen (200) -> Oimmo (208)
    // 2 jumps empty + 8 jumps haul = 10 jumps total. Profit: 1,060,000 ISK -> PPJ = 106,000 ISK/j
    let d_poinen = make_chain_deal(
        1001,
        "Protein Delicacies",
        (200, "Poinen"),
        (208, "Oimmo"),
        1000,
        15_000.0,
        5_000_000.0,
        1_060_000.0,
    );

    // Deal 2: Holoreels at Josameto (300) -> Tuurlainas (304)
    // 3 jumps empty + 4 jumps haul = 7 jumps total. Profit: 1,120,000 ISK -> PPJ = 160,000 ISK/j
    let d_josameto = make_chain_deal(
        1002,
        "Holoreels",
        (300, "Josameto"),
        (304, "Tuurlainas"),
        1000,
        19_000.0,
        8_000_000.0,
        1_120_000.0,
    );

    let cfg = ChainConfig {
        home_system: 100, // Geras
        max_legs: 1,
        ship_max_volume: 30_000.0,
        ship_max_budget: 50_000_000.0,
        ..Default::default()
    };

    let routes = find_chain_routes(&[d_poinen, d_josameto], &graph, &cfg);
    assert!(!routes.is_empty(), "Must find chain routes");

    // Route #1 MUST be Holoreels from Josameto (160k PPJ), NOT Protein Delicacies (106k PPJ)
    let best = &routes[0];
    assert_eq!(
        best.legs[0].pickups[0].system_id, 300,
        "Chain route must pick Josameto (Holoreels) at 160,000 PPJ over closer Poinen at 106,000 PPJ"
    );
    assert_eq!(best.legs[0].sell.system_id, 304);
    assert_eq!(
        best.legs[0].pickups[0].cargo_loaded[0].deal.item_name,
        "Holoreels"
    );
    assert_eq!(best.total_jumps, 7);
    assert!((best.profit_per_jump - 160_000.0).abs() < 1e-4);
}

#[test]
fn test_chain_multi_leg_whole_trip_ppj_maximization() {
    let mut systems = Vec::new();
    let mut jumps = Vec::new();

    // 100 (Home) -> 101 (Pickup 1) -> 102 (Dest 1 / Pickup 2) -> 103 (Dest 2)
    systems.push(SystemNode {
        system_id: 100,
        name: "Home".to_string(),
        security_status: 0.9,
        region_id: 10000002,
    });
    systems.push(SystemNode {
        system_id: 101,
        name: "Pickup1".to_string(),
        security_status: 0.9,
        region_id: 10000002,
    });
    systems.push(SystemNode {
        system_id: 102,
        name: "Dest1".to_string(),
        security_status: 0.9,
        region_id: 10000002,
    });
    systems.push(SystemNode {
        system_id: 103,
        name: "Dest2".to_string(),
        security_status: 0.9,
        region_id: 10000002,
    });

    jumps.push((100, 101)); // 1 jump empty
    jumps.push((101, 102)); // 1 jump haul
    jumps.push((102, 103)); // 1 jump haul onward

    let graph = SystemGraph::from_nodes_and_jumps(systems, jumps);

    // Leg 1: 101 -> 102. Profit 20,000 in 2 jumps (1 empty + 1 haul) -> 10,000 PPJ
    let d1 = make_chain_deal(
        34,
        "Leg 1 Item",
        (101, "Pickup1"),
        (102, "Dest1"),
        100,
        100.0,
        10_000.0,
        20_000.0,
    );

    // Leg 2: 102 -> 103. Profit 40,000 in 1 jump onward (0 empty + 1 haul)
    // Combined: 60,000 profit in 3 jumps -> 20,000 PPJ (higher than Leg 1 alone!)
    let d2 = make_chain_deal(
        35,
        "Leg 2 Item",
        (102, "Dest1"),
        (103, "Dest2"),
        100,
        100.0,
        10_000.0,
        40_000.0,
    );

    let cfg = ChainConfig {
        home_system: 100,
        max_legs: 2,
        ..Default::default()
    };

    let routes = find_chain_routes(&[d1, d2], &graph, &cfg);
    assert!(!routes.is_empty(), "Must find chain routes");

    // Route #1 must be the 2-leg route because 20,000 PPJ > 10,000 PPJ!
    assert_eq!(routes[0].legs.len(), 2);
    assert!((routes[0].profit_per_jump - 20_000.0).abs() < 1e-4);
}

#[test]
fn test_chain_zero_max_legs_and_zero_budget() {
    let graph = create_chain_test_graph();
    let d1 = make_chain_deal(
        34,
        "Tritanium",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        1000,
        100.0,
        10_000.0,
        2_500.0,
    );

    // 1. max_legs = 0 returns empty route list without panicking or looping
    let cfg_zero_legs = ChainConfig {
        home_system: 30000142,
        max_legs: 0,
        ..Default::default()
    };
    let routes_zero_legs = find_chain_routes(&[d1.clone()], &graph, &cfg_zero_legs);
    assert!(
        routes_zero_legs.is_empty(),
        "max_legs = 0 must return 0 routes"
    );

    // 2. ship_max_budget = 0.0 cannot purchase deals, returns empty route list
    let cfg_zero_budget = ChainConfig {
        home_system: 30000142,
        max_legs: 2,
        ship_max_budget: 0.0,
        ..Default::default()
    };
    let routes_zero_budget = find_chain_routes(&[d1], &graph, &cfg_zero_budget);
    assert!(
        routes_zero_budget.is_empty(),
        "0.0 budget must return 0 routes"
    );
}
