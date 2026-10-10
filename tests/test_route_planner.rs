use eve_trade_scout::arbitrage::ArbitrageDeal;
use eve_trade_scout::config::BIG_HAULER;
use eve_trade_scout::graph::SystemGraph;
use eve_trade_scout::route_planner::plan_route;
use eve_trade_scout::sde::SystemNode;

fn create_route_test_graph() -> SystemGraph {
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
    ];

    let jumps = vec![
        (30000142, 30000144), // Jita <-> Perimeter
        (30000144, 30002187), // Perimeter <-> Amarr
        (30002187, 30000032), // Amarr <-> Dodixie
    ];

    SystemGraph::from_nodes_and_jumps(systems, jumps)
}

fn make_test_deal(
    type_id: u32,
    name: &str,
    src_sys: (u32, &str),
    dst_sys: (u32, &str),
    net_profit: f64,
    jumps_src_dest: u32,
) -> ArbitrageDeal {
    ArbitrageDeal {
        type_id,
        item_name: name.to_string(),
        source_order_id: 100_000_000 + (type_id as u64 * 10_000) + (src_sys.0 as u64 % 10_000),
        dest_order_id: 200_000_000 + (type_id as u64 * 10_000) + (dst_sys.0 as u64 % 10_000),
        source_order_volume_remain: 100,
        dest_order_volume_remain: 100,
        dest_order_min_volume: 1,
        is_npc_trade: false,
        buy_price: 100.0,
        sell_price: 150.0,
        spread_pct: 50.0,
        source_station_id: 60003760,
        source_system_id: src_sys.0,
        source_system_name: src_sys.1.to_string(),
        source_station_name: format!("{} Station", src_sys.1),
        source_security: 0.9,
        dest_station_id: 60008494,
        dest_system_id: dst_sys.0,
        dest_system_name: dst_sys.1.to_string(),
        dest_station_name: format!("{} Station", dst_sys.1),
        dest_security: 0.9,
        quantity: 100,
        volume_used: 100.0,
        capital_required: 10000.0,
        gross_profit: 5000.0,
        taxes_and_fees: 500.0,
        net_profit,
        net_profit_pct: (net_profit / 10000.0) * 100.0,
        jumps_from_home: 1,
        jumps_source_dest: jumps_src_dest,
        total_jumps: 1 + jumps_src_dest,
        profit_per_jump: net_profit / ((1 + jumps_src_dest) as f64),
        profit_per_m3: net_profit / 100.0,
    }
}

#[test]
fn test_route_planner_empty_deals() {
    let graph = create_route_test_graph();
    let home = 30000142; // Jita

    let route = plan_route(&[], BIG_HAULER, home, &graph, true, 5);
    assert_eq!(route.stops.len(), 0);
    assert_eq!(route.total_jumps, 0);
    assert_eq!(route.total_profit, 0.0);
}

#[test]
fn test_route_planner_greedy_chaining_and_max_stops() {
    let graph = create_route_test_graph();
    let home = 30000142; // Jita

    // Deal 1: Jita -> Perimeter (1 jump)
    let d1 = make_test_deal(
        34,
        "Tritanium",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        10_000_000.0,
        1,
    );
    // Deal 2: Perimeter -> Amarr (1 jump)
    let d2 = make_test_deal(
        35,
        "Pyerite",
        (30000144, "Perimeter"),
        (30002187, "Amarr"),
        20_000_000.0,
        1,
    );
    // Deal 3: Amarr -> Dodixie (1 jump)
    let d3 = make_test_deal(
        36,
        "Mexallon",
        (30002187, "Amarr"),
        (30000032, "Dodixie"),
        15_000_000.0,
        1,
    );

    let deals = vec![d1, d2, d3];

    // Max stops = 2
    let route = plan_route(&deals, BIG_HAULER, home, &graph, true, 2);

    assert_eq!(route.stops.len(), 2, "Route must respect max_stops limit");
    assert!(route.total_profit > 0.0);
    assert!(route.total_jumps > 0);
    assert!(route.avg_profit_per_jump > 0.0);
}

#[test]
fn test_route_planner_exhausted_order_pruned() {
    let graph = create_route_test_graph();
    let home = 30000142; // Jita

    // Deal 1: Jita -> Perimeter (Spirits, source_order_id = 12345)
    let mut d1 = make_test_deal(
        9850,
        "Spirits",
        (30000142, "Jita"),
        (30000144, "Perimeter"),
        10_000_000.0,
        1,
    );
    d1.source_order_id = 12345;
    d1.source_station_id = 60003760;

    // Deal 2: Jita -> Amarr (Spirits, shares same source_order_id = 12345)
    let mut d2 = make_test_deal(
        9850,
        "Spirits",
        (30000142, "Jita"),
        (30002187, "Amarr"),
        9_000_000.0,
        1,
    );
    d2.source_order_id = 12345;
    d2.source_station_id = 60003760;

    // Deal 3: Perimeter -> Amarr (Pyerite, independent order)
    let mut d3 = make_test_deal(
        35,
        "Pyerite",
        (30000144, "Perimeter"),
        (30002187, "Amarr"),
        8_000_000.0,
        1,
    );
    d3.source_order_id = 54321;
    d3.source_station_id = 60003761;

    let deals = vec![d1, d2, d3];

    // Plan route with up to 5 stops
    let route = plan_route(&deals, BIG_HAULER, home, &graph, true, 5);

    // Stop 1 selects Deal 1. Deal 2 must be pruned and never selected!
    assert_eq!(route.stops.len(), 2);
    assert_eq!(route.stops[0].deal.item_name, "Spirits");
    assert_eq!(route.stops[1].deal.item_name, "Pyerite");
}
