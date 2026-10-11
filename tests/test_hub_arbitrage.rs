use eve_trade_scout::arbitrage::ArbitrageDeal;
use eve_trade_scout::graph::SystemGraph;
use eve_trade_scout::hub_arbitrage::{
    compute_corridor_summary, filter_hub_deals, pack_hub_corridor_manifest, CoreHub,
    HubFilterOptions,
};
use eve_trade_scout::sde::SystemNode;

fn create_hub_test_graph() -> SystemGraph {
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
            system_id: 30002659,
            name: "Dodixie".to_string(),
            security_status: 0.85,
            region_id: 10000032,
        },
        SystemNode {
            system_id: 30002510,
            name: "Rens".to_string(),
            security_status: 0.85,
            region_id: 10000030,
        },
        SystemNode {
            system_id: 30002053,
            name: "Hek".to_string(),
            security_status: 0.80,
            region_id: 10000042,
        },
    ];

    let jumps = vec![
        (30000142, 30000144), // Jita <-> Perimeter
        (30000144, 30002187), // Perimeter <-> Amarr (direct, 2 jumps Jita->Amarr)
        (30002187, 30002659), // Amarr <-> Dodixie
        (30002659, 30002510), // Dodixie <-> Rens
        (30002510, 30002053), // Rens <-> Hek
    ];

    SystemGraph::from_nodes_and_jumps(systems, jumps)
}

#[allow(clippy::too_many_arguments)]
fn make_hub_deal(
    type_id: u32,
    name: &str,
    src_sys: u32,
    src_name: &str,
    src_station: u64,
    dst_sys: u32,
    dst_name: &str,
    dst_station: u64,
    qty: u32,
    vol: f64,
    cost: f64,
    profit: f64,
    is_npc: bool,
) -> ArbitrageDeal {
    let roi = (profit / cost) * 100.0;
    ArbitrageDeal {
        type_id,
        item_name: name.to_string(),
        source_order_id: 100_000 + type_id as u64,
        dest_order_id: 200_000 + type_id as u64,
        source_order_volume_remain: qty,
        dest_order_volume_remain: qty,
        dest_order_min_volume: 1,
        is_npc_trade: is_npc,
        buy_price: cost / (qty as f64),
        sell_price: (cost + profit * 1.2) / (qty as f64),
        spread_pct: 30.0,
        source_station_id: src_station,
        source_system_id: src_sys,
        source_system_name: src_name.to_string(),
        source_station_name: format!("{} Station {}", src_name, src_station),
        source_security: 0.9,
        dest_station_id: dst_station,
        dest_system_id: dst_sys,
        dest_system_name: dst_name.to_string(),
        dest_station_name: format!("{} Station {}", dst_name, dst_station),
        dest_security: 0.9,
        quantity: qty,
        volume_used: vol,
        capital_required: cost,
        gross_profit: profit * 1.2,
        taxes_and_fees: profit * 0.2,
        net_profit: profit,
        net_profit_pct: roi,
        jumps_from_home: 0,
        jumps_source_dest: 2,
        total_jumps: 2,
        profit_per_jump: profit / 2.0,
        profit_per_m3: profit / vol,
    }
}

#[test]
fn test_core_hub_metadata_and_resolution() {
    assert_eq!(CoreHub::ALL.len(), 5);

    // Test Jita
    assert_eq!(CoreHub::Jita.system_id(), 30000142);
    assert_eq!(CoreHub::Jita.region_id(), 10000002);
    assert_eq!(CoreHub::Jita.primary_station_id(), 60003760);
    assert_eq!(CoreHub::from_system_id(30000142), Some(CoreHub::Jita));
    assert_eq!(CoreHub::from_name("JITA"), Some(CoreHub::Jita));

    // Test Amarr
    assert_eq!(CoreHub::Amarr.system_id(), 30002187);
    assert_eq!(CoreHub::Amarr.region_id(), 10000043);
    assert_eq!(CoreHub::Amarr.primary_station_id(), 60008494);
    assert_eq!(CoreHub::from_system_id(30002187), Some(CoreHub::Amarr));
    assert_eq!(CoreHub::from_name("Amarr"), Some(CoreHub::Amarr));

    // Test Dodixie
    assert_eq!(CoreHub::Dodixie.system_id(), 30002659);
    assert_eq!(CoreHub::Dodixie.region_id(), 10000032);
    assert_eq!(CoreHub::Dodixie.primary_station_id(), 60011866);
    assert_eq!(CoreHub::from_system_id(30002659), Some(CoreHub::Dodixie));
    assert_eq!(CoreHub::from_name("dodixie"), Some(CoreHub::Dodixie));

    // Test Rens
    assert_eq!(CoreHub::Rens.system_id(), 30002510);
    assert_eq!(CoreHub::Rens.region_id(), 10000030);
    assert_eq!(CoreHub::Rens.primary_station_id(), 60004588);
    assert_eq!(CoreHub::from_system_id(30002510), Some(CoreHub::Rens));
    assert_eq!(CoreHub::from_name("Rens"), Some(CoreHub::Rens));

    // Test Hek
    assert_eq!(CoreHub::Hek.system_id(), 30002053);
    assert_eq!(CoreHub::Hek.region_id(), 10000042);
    assert_eq!(CoreHub::Hek.primary_station_id(), 60005686);
    assert_eq!(CoreHub::from_system_id(30002053), Some(CoreHub::Hek));
    assert_eq!(CoreHub::from_name("Hek"), Some(CoreHub::Hek));

    // Non-hub
    assert_eq!(CoreHub::from_system_id(30000144), None);
    assert_eq!(CoreHub::from_name("Perimeter"), None);
}

#[test]
fn test_filter_hub_deals_specific_corridor() {
    let d1 = make_hub_deal(
        1, "Jita to Amarr Deal",
        30000142, "Jita", 60003760,
        30002187, "Amarr", 60008494,
        100, 100.0, 1_000_000.0, 500_000.0, false,
    );
    let d2 = make_hub_deal(
        2, "Jita to Dodixie Deal",
        30000142, "Jita", 60003760,
        30002659, "Dodixie", 60011866,
        100, 100.0, 1_000_000.0, 400_000.0, false,
    );
    let d3 = make_hub_deal(
        3, "Amarr to Jita Deal",
        30002187, "Amarr", 60008494,
        30000142, "Jita", 60003760,
        100, 100.0, 1_000_000.0, 600_000.0, false,
    );

    let deals = vec![d1, d2, d3];

    let opts = HubFilterOptions {
        source_hub: Some(CoreHub::Jita),
        dest_hub: Some(CoreHub::Amarr),
        min_profit: 0.0,
        ..Default::default()
    };

    let filtered = filter_hub_deals(&deals, &opts);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].item_name, "Jita to Amarr Deal");
}

#[test]
fn test_filter_hub_deals_wildcards() {
    let d1 = make_hub_deal(
        1, "Jita to Amarr",
        30000142, "Jita", 60003760,
        30002187, "Amarr", 60008494,
        100, 100.0, 1_000_000.0, 500_000.0, false,
    );
    let d2 = make_hub_deal(
        2, "Dodixie to Amarr",
        30002659, "Dodixie", 60011866,
        30002187, "Amarr", 60008494,
        100, 100.0, 1_000_000.0, 400_000.0, false,
    );
    let d3 = make_hub_deal(
        3, "Jita to Rens",
        30000142, "Jita", 60003760,
        30002510, "Rens", 60004588,
        100, 100.0, 1_000_000.0, 600_000.0, false,
    );

    let deals = vec![d1, d2, d3];

    // Wildcard Source: All Hubs -> Amarr
    let opts_all_to_amarr = HubFilterOptions {
        source_hub: None,
        dest_hub: Some(CoreHub::Amarr),
        min_profit: 0.0,
        ..Default::default()
    };
    let res1 = filter_hub_deals(&deals, &opts_all_to_amarr);
    assert_eq!(res1.len(), 2);

    // Wildcard Destination: Jita -> All Hubs
    let opts_jita_to_all = HubFilterOptions {
        source_hub: Some(CoreHub::Jita),
        dest_hub: None,
        min_profit: 0.0,
        ..Default::default()
    };
    let res2 = filter_hub_deals(&deals, &opts_jita_to_all);
    assert_eq!(res2.len(), 2);

    // Wildcard Both: All Hubs -> All Hubs
    let opts_all_to_all = HubFilterOptions {
        source_hub: None,
        dest_hub: None,
        min_profit: 0.0,
        ..Default::default()
    };
    let res3 = filter_hub_deals(&deals, &opts_all_to_all);
    assert_eq!(res3.len(), 3);
}

#[test]
fn test_filter_hub_deals_excludes_non_hub_and_intra_hub() {
    // Non-hub source (Perimeter -> Amarr)
    let d_non_hub_src = make_hub_deal(
        1, "Perimeter to Amarr",
        30000144, "Perimeter", 60003761,
        30002187, "Amarr", 60008494,
        100, 100.0, 1_000_000.0, 500_000.0, false,
    );
    // Non-hub dest (Jita -> Perimeter)
    let d_non_hub_dst = make_hub_deal(
        2, "Jita to Perimeter",
        30000142, "Jita", 60003760,
        30000144, "Perimeter", 60003761,
        100, 100.0, 1_000_000.0, 500_000.0, false,
    );
    // Intra-hub trade (Jita 4-4 to another station in Jita)
    let d_intra_hub = make_hub_deal(
        3, "Jita Internal",
        30000142, "Jita", 60003760,
        30000142, "Jita", 60003769,
        100, 100.0, 1_000_000.0, 500_000.0, false,
    );

    let deals = vec![d_non_hub_src, d_non_hub_dst, d_intra_hub];
    let opts = HubFilterOptions {
        source_hub: None,
        dest_hub: None,
        min_profit: 0.0,
        ..Default::default()
    };

    let filtered = filter_hub_deals(&deals, &opts);
    assert!(filtered.is_empty(), "All non-hub and intra-hub trades must be excluded");
}

#[test]
fn test_filter_hub_deals_npc_toggle_and_major_stations() {
    let d_npc = make_hub_deal(
        1, "Spirits",
        30000142, "Jita", 60003760,
        30002187, "Amarr", 60008494,
        100, 100.0, 1_000_000.0, 500_000.0, true, // NPC trade
    );
    let d_player_secondary_station = make_hub_deal(
        2, "Tritanium",
        30000142, "Jita", 60003769, // Non-primary Jita station
        30002187, "Amarr", 60008494,
        100, 100.0, 1_000_000.0, 500_000.0, false,
    );

    let deals = vec![d_npc, d_player_secondary_station];

    // When include_npc = false: excludes d_npc
    let opts_no_npc = HubFilterOptions {
        source_hub: Some(CoreHub::Jita),
        dest_hub: Some(CoreHub::Amarr),
        include_npc: false,
        major_stations_only: false,
        min_profit: 0.0,
        ..Default::default()
    };
    let res1 = filter_hub_deals(&deals, &opts_no_npc);
    assert_eq!(res1.len(), 1);
    assert_eq!(res1[0].item_name, "Tritanium");

    // When major_stations_only = true: excludes d_player_secondary_station
    let opts_major_only = HubFilterOptions {
        source_hub: Some(CoreHub::Jita),
        dest_hub: Some(CoreHub::Amarr),
        include_npc: true,
        major_stations_only: true,
        min_profit: 0.0,
        ..Default::default()
    };
    let res2 = filter_hub_deals(&deals, &opts_major_only);
    assert_eq!(res2.len(), 1);
    assert_eq!(res2[0].item_name, "Spirits");
}

#[test]
fn test_pack_hub_corridor_manifest() {
    let d1 = make_hub_deal(
        1, "High Value Density Item",
        30000142, "Jita", 60003760,
        30002187, "Amarr", 60008494,
        10, 10.0, 10_000.0, 5_000.0, false, // 500 ISK/m³
    );
    let mut d2 = make_hub_deal(
        2, "Low Value Density Item",
        30000142, "Jita", 60003760,
        30002187, "Amarr", 60008494,
        10, 50.0, 10_000.0, 2_000.0, false, // 40 ISK/m³
    );
    // Requires minimum 10 units so it cannot partially pack in 5 m³
    d2.dest_order_min_volume = 10;

    let deals = vec![&d1, &d2];
    // Volume constraint allows 15 m³ (only d1 fits, d2 needs 50 m³)
    let pack = pack_hub_corridor_manifest(&deals, 15.0, 50_000.0);

    assert_eq!(pack.packed.len(), 1);
    assert_eq!(pack.packed[0].deal.item_name, "High Value Density Item");
    assert_eq!(pack.total_profit, 5_000.0);
}

#[test]
fn test_compute_corridor_summary() {
    let graph = create_hub_test_graph();
    let d1 = make_hub_deal(
        1, "Deal 1",
        30000142, "Jita", 60003760,
        30002187, "Amarr", 60008494,
        10, 10.0, 10_000.0, 5_000.0, false,
    );
    let d2 = make_hub_deal(
        2, "Deal 2",
        30000142, "Jita", 60003760,
        30002187, "Amarr", 60008494,
        10, 10.0, 10_000.0, 7_000.0, false,
    );

    let deals = vec![&d1, &d2];
    let summary = compute_corridor_summary(&deals, &graph, CoreHub::Jita, CoreHub::Amarr, true);

    assert_eq!(summary.safe_jumps, 2);
    assert_eq!(summary.deal_count, 2);
    assert_eq!(summary.total_potential_profit, 12_000.0);
    assert_eq!(summary.max_single_deal_profit, 7_000.0);
    assert_eq!(summary.top_item_name.as_deref(), Some("Deal 2"));
}
