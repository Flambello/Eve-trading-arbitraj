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
fn test_chain_15pct_roi_invariant_maintained() {
    let graph = create_chain_test_graph();
    // Sub-15% ROI deal: 10% ROI
    let d_sub15 = make_chain_deal(
        34,
        "Low ROI",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        100,
        50.0,
        10_000.0,
        1_000.0, // 10% ROI
    );

    let cfg = ChainConfig {
        home_system: 30000142,
        ..Default::default()
    };

    let routes = find_chain_routes(&[d_sub15], &graph, &cfg);
    assert!(routes.is_empty());
}
