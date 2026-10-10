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
    if !r_strict.is_empty() {
        assert_eq!(r_strict[0].total_profit, r_zero[0].total_profit);
        assert_eq!(r_strict[0].total_jumps, r_zero[0].total_jumps);
    }
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
    assert_eq!(total_packed_units, 100, "Should not exceed the single order volume of 100 units");
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
    assert_eq!(best.legs.len(), 2, "Chain must have 2 legs because capital was freed and recycled");
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
    assert!(routes.is_empty(), "Should reject deal because ship capacity cannot meet buyer's min_volume requirement");

    // Now give enough cargo space to fit 60 units (>= 50 min_volume)
    let cfg_ok = ChainConfig {
        home_system: 30000142,
        max_legs: 1,
        ship_max_budget: 50_000.0,
        ship_max_volume: 60.0,
        ..Default::default()
    };

    let routes_ok = find_chain_routes(&[d1], &graph, &cfg_ok);
    assert!(!routes_ok.is_empty(), "Should accept deal when ship capacity can meet min_volume");
}
