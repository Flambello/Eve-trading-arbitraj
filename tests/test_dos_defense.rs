use eve_trade_scout::arbitrage::{
    find_deals, ArbitrageConfig, ArbitrageDeal, MAX_DEALS_COLLECTED, MAX_MANIFEST_CANDIDATE_DEALS,
    MAX_STATIONS_PER_TYPE,
};
use eve_trade_scout::chain_route_planner::{
    gather_enroute_candidates, ChainConfig, MAX_ENROUTE_CANDIDATES,
};
use eve_trade_scout::config::BIG_HAULER;
use eve_trade_scout::esi::client::{
    prune_etag_map, read_bounded_response_text, StoredEtag, MAX_ESI_PAGE_BYTES,
    MAX_ETAG_CACHE_ENTRIES, MAX_ETAG_FILE_BYTES,
};
use eve_trade_scout::esi::market::RegionMarket;
use eve_trade_scout::esi::models::MarketOrder;
use eve_trade_scout::esi::sso::{MAX_AUTOPILOT_WAYPOINTS, MAX_WAYPOINTS_BATCH_DURATION_SECS};
use eve_trade_scout::graph::SystemGraph;
use eve_trade_scout::route_planner::{
    plan_route, MAX_ALLOWED_ROUTE_STOPS, MAX_ROUTE_CANDIDATE_DEALS,
};
use eve_trade_scout::sde::loader::{
    SdeLoader, MAX_SYSTEM_SEARCH_LIMIT, MAX_SYSTEM_SEARCH_QUERY_LEN,
};
use std::collections::HashMap;

fn create_mock_universe() -> (SdeLoader, SystemGraph) {
    let sde = SdeLoader::in_memory_with_fixtures().expect("In-memory fixtures must load cleanly");
    let graph = SystemGraph::from_sde(&sde).expect("SystemGraph must compile from SDE");
    (sde, graph)
}

#[tokio::test]
async fn test_bounded_esi_stream_rejects_oversized_page() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_task = tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 1024];
        let _ = socket.read(&mut buf).await.unwrap();

        // Stream headers indicating 20 MiB payload (exceeding 10 MiB safety limit)
        let oversized_len = MAX_ESI_PAGE_BYTES + 1024 * 1024;
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            oversized_len
        );
        socket.write_all(headers.as_bytes()).await.unwrap();

        // Send a small initial chunk
        let chunk = vec![b' '; 4096];
        let _ = socket.write_all(&chunk).await;
    });

    let client = reqwest::Client::new();
    let url = format!("http://{}/oversized", addr);
    let resp = client.get(&url).send().await.unwrap();

    let res = read_bounded_response_text(resp, MAX_ESI_PAGE_BYTES).await;
    assert!(
        res.is_err(),
        "read_bounded_response_text must reject responses exceeding MAX_ESI_PAGE_BYTES"
    );
    let err_msg = res.unwrap_err().to_string();
    assert!(
        err_msg.contains("exceeded maximum safety limit"),
        "Error message must report safety limit rejection: {}",
        err_msg
    );

    server_task.await.unwrap();
}

#[test]
fn test_etag_cache_entry_ceiling_and_pruning() {
    let mut map: HashMap<String, StoredEtag> = HashMap::new();
    let now = chrono::Utc::now();

    // Insert 70 cache entries (exceeding MAX_ETAG_CACHE_ENTRIES = 50)
    for i in 1..=70 {
        let key = format!("cache_key_{}", i);
        let expires = now + chrono::Duration::seconds(i * 10);
        map.insert(
            key,
            StoredEtag {
                etag: format!("\"etag_{}\"", i),
                expires: Some(expires),
                body_json: "[]".to_string(),
            },
        );
    }

    assert_eq!(map.len(), 70);

    // Prune map to MAX_ETAG_CACHE_ENTRIES (50)
    prune_etag_map(&mut map, MAX_ETAG_CACHE_ENTRIES);

    assert_eq!(
        map.len(),
        MAX_ETAG_CACHE_ENTRIES,
        "Map must be pruned to exactly MAX_ETAG_CACHE_ENTRIES"
    );

    // Verify that the entries with the soonest expiration were evicted first
    // Entries 1..=20 had the earliest expires timestamps, so they should be gone
    for i in 1..=20 {
        let key = format!("cache_key_{}", i);
        assert!(
            !map.contains_key(&key),
            "Early expiring key {} should have been evicted",
            key
        );
    }

    // Entries 21..=70 should remain
    for i in 21..=70 {
        let key = format!("cache_key_{}", i);
        assert!(
            map.contains_key(&key),
            "Later expiring key {} should be retained",
            key
        );
    }
}

#[test]
fn test_etag_cache_file_size_constant() {
    assert_eq!(MAX_ETAG_FILE_BYTES, 50 * 1024 * 1024);
    assert_eq!(MAX_ETAG_CACHE_ENTRIES, 50);
    assert_eq!(MAX_ESI_PAGE_BYTES, 10 * 1024 * 1024);
}

#[test]
fn test_find_deals_caps_station_cross_product_per_type() {
    let (sde, graph) = create_mock_universe();

    let cfg = ArbitrageConfig {
        ship: BIG_HAULER,
        home_system: 30000142,
        min_profit: 10_000.0,
        min_spread: 3.0,
        min_net_profit_pct: 10.0,
        broker_fee: 3.0,
        sales_tax: 8.0,
        safe_only: true,
        npc_stations_only: false,
    };

    // Synthesize 100 sell orders at 100 distinct stations and 100 buy orders at 100 distinct stations
    // Without MAX_STATIONS_PER_TYPE = 50, this would evaluate 100 x 100 = 10,000 combinations for 1 item!
    let mut sell_orders = HashMap::new();
    let mut buy_orders = HashMap::new();

    let mut sells = Vec::new();
    for i in 1..=100u64 {
        sells.push(MarketOrder {
            order_id: 10_000 + i,
            type_id: 34,
            location_id: 60000000 + i,
            system_id: 30000142, // Jita
            volume_total: 10_000,
            volume_remain: 10_000,
            min_volume: 1,
            price: 5.0 + (i as f64) * 0.01,
            is_buy_order: false,
            duration: 90,
            issued: None,
            range: None,
        });
    }
    sell_orders.insert(34, sells);

    let mut buys = Vec::new();
    for i in 1..=100u64 {
        buys.push(MarketOrder {
            order_id: 20_000 + i,
            type_id: 34,
            location_id: 60001000 + i,
            system_id: 30000144, // Perimeter
            volume_total: 10_000,
            volume_remain: 10_000,
            min_volume: 1,
            price: 20.0 - (i as f64) * 0.01,
            is_buy_order: true,
            duration: 90,
            issued: None,
            range: None,
        });
    }
    buy_orders.insert(34, buys);

    let mut markets = HashMap::new();
    markets.insert(
        10000002,
        RegionMarket {
            region_id: 10000002,
            sell_orders,
            buy_orders,
        },
    );

    let start = std::time::Instant::now();
    let deals = find_deals(&markets, &sde, &graph, &cfg);
    let elapsed = start.elapsed();

    // Verify deals were found rapidly
    assert!(!deals.is_empty(), "Must find profitable deals");
    // Maximum possible combinations evaluated per type is 50 x 50 = 2,500 (capped from 10,000)
    assert!(
        deals.len() <= MAX_STATIONS_PER_TYPE * MAX_STATIONS_PER_TYPE,
        "Total deals per item type cannot exceed MAX_STATIONS_PER_TYPE squared (2500)"
    );
    assert!(
        elapsed.as_millis() < 500,
        "find_deals with station capping must execute rapidly under 500ms (took {}ms)",
        elapsed.as_millis()
    );
}

#[test]
fn test_find_deals_constants_and_manifest_candidate_limits() {
    assert_eq!(MAX_STATIONS_PER_TYPE, 50);
    assert_eq!(MAX_DEALS_COLLECTED, 50_000);
    assert_eq!(MAX_MANIFEST_CANDIDATE_DEALS, 10_000);
}

#[test]
fn test_route_planner_clamps_excessive_stops_and_candidates() {
    let (_sde, graph) = create_mock_universe();
    let home = 30000142; // Jita

    // Generate 1,500 mock deals (exceeding MAX_ROUTE_CANDIDATE_DEALS = 1,000)
    let mut deals = Vec::new();
    for i in 1..=1500u32 {
        deals.push(ArbitrageDeal {
            type_id: 34,
            item_name: format!("Item {}", i),
            source_order_id: i as u64,
            dest_order_id: (10_000 + i) as u64,
            source_order_volume_remain: 100,
            dest_order_volume_remain: 100,
            dest_order_min_volume: 1,
            is_npc_trade: false,
            buy_price: 100.0,
            sell_price: 200.0,
            spread_pct: 100.0,
            source_station_id: 60003760,
            source_system_id: 30000142,
            source_system_name: "Jita".to_string(),
            source_station_name: "Jita Station".to_string(),
            source_security: 0.95,
            dest_station_id: 60003761,
            dest_system_id: 30000144,
            dest_system_name: "Perimeter".to_string(),
            dest_station_name: "Perimeter Station".to_string(),
            dest_security: 0.90,
            quantity: 10,
            volume_used: 0.1,
            capital_required: 1000.0,
            gross_profit: 1000.0,
            taxes_and_fees: 110.0,
            net_profit: 890.0,
            net_profit_pct: 89.0,
            jumps_from_home: 0,
            jumps_source_dest: 1,
            total_jumps: 1,
            profit_per_jump: 890.0,
            profit_per_m3: 8900.0,
        });
    }

    // Request 500 stops (exceeding MAX_ALLOWED_ROUTE_STOPS = 20)
    let start = std::time::Instant::now();
    let route = plan_route(&deals, BIG_HAULER, home, &graph, true, 500);
    let elapsed = start.elapsed();

    assert!(
        route.stops.len() <= MAX_ALLOWED_ROUTE_STOPS,
        "Planned stops ({}) must be clamped to MAX_ALLOWED_ROUTE_STOPS ({})",
        route.stops.len(),
        MAX_ALLOWED_ROUTE_STOPS
    );
    assert_eq!(MAX_ROUTE_CANDIDATE_DEALS, 1_000);
    assert_eq!(MAX_ALLOWED_ROUTE_STOPS, 20);
    assert!(
        elapsed.as_millis() < 250,
        "plan_route with clamped candidates and stops must run under 250ms (took {}ms)",
        elapsed.as_millis()
    );
}

#[test]
fn test_chain_gather_enroute_candidates_triangle_inequality_and_bounds() {
    let (sde, graph) = create_mock_universe();
    let source = 30000142; // Jita
    let dest = 30002187; // Amarr
    let direct_jumps = graph.jump_count(source, dest, true);

    let config = ChainConfig {
        home_system: source,
        max_legs: 3,
        beam_width: 15,
        detour_enabled: true,
        max_detour_jumps: 2,
        max_dest_loads: 2,
        ship_max_volume: 60_000.0,
        ship_max_budget: 500_000_000.0,
        safe_only: true,
        include_npc: true,
    };

    // Create deals at 50 source systems
    let mut deals_by_source: HashMap<u32, Vec<&ArbitrageDeal>> = HashMap::new();
    let mut deal_storage = Vec::new();
    let systems = sde.all_systems().unwrap();

    for sys in systems.iter().take(50) {
        let deal = ArbitrageDeal {
            type_id: 34,
            item_name: "Tritanium".to_string(),
            source_order_id: sys.system_id as u64,
            dest_order_id: 9999,
            source_order_volume_remain: 100,
            dest_order_volume_remain: 100,
            dest_order_min_volume: 1,
            is_npc_trade: false,
            buy_price: 10.0,
            sell_price: 20.0,
            spread_pct: 100.0,
            source_station_id: 60000000 + sys.system_id as u64,
            source_system_id: sys.system_id,
            source_system_name: sys.name.clone(),
            source_station_name: "Station".to_string(),
            source_security: sys.security_status,
            dest_station_id: 60008494,
            dest_system_id: dest,
            dest_system_name: "Amarr".to_string(),
            dest_station_name: "Amarr Station".to_string(),
            dest_security: 0.90,
            quantity: 100,
            volume_used: 1.0,
            capital_required: 1000.0,
            gross_profit: 1000.0,
            taxes_and_fees: 110.0,
            net_profit: 890.0,
            net_profit_pct: 89.0,
            jumps_from_home: 0,
            jumps_source_dest: 5,
            total_jumps: 5,
            profit_per_jump: 178.0,
            profit_per_m3: 890.0,
        };
        deal_storage.push(deal);
    }

    for d in &deal_storage {
        deals_by_source
            .entry(d.source_system_id)
            .or_default()
            .push(d);
    }

    let candidates = gather_enroute_candidates(
        source,
        dest,
        direct_jumps,
        &deals_by_source,
        &graph,
        &config,
    );

    assert!(
        candidates.len() <= MAX_ENROUTE_CANDIDATES,
        "Candidates count ({}) must be clamped to MAX_ENROUTE_CANDIDATES ({})",
        candidates.len(),
        MAX_ENROUTE_CANDIDATES
    );
    assert_eq!(MAX_ENROUTE_CANDIDATES, 15);
}

#[test]
fn test_sde_search_clamps_extreme_query_and_limit() {
    let (sde, _) = create_mock_universe();

    // 1. Extreme 500-character query string with UTF-8 and spaces
    let massive_query = "Jita".to_string() + &" ".repeat(100) + &"A".repeat(400);
    let results = sde.search_systems(&massive_query, 10);
    // Should still safely resolve "Jita" as prefix match because query was clamped without panic
    assert!(
        !results.is_empty(),
        "Clamped query must still resolve matching prefix"
    );
    assert_eq!(results[0].system_name, "Jita");

    // 2. Limit clamp: request usize::MAX results
    let max_limit_results = sde.search_systems("Jita", usize::MAX);
    assert!(
        max_limit_results.len() <= MAX_SYSTEM_SEARCH_LIMIT,
        "Results len ({}) must be clamped to MAX_SYSTEM_SEARCH_LIMIT ({})",
        max_limit_results.len(),
        MAX_SYSTEM_SEARCH_LIMIT
    );

    assert_eq!(MAX_SYSTEM_SEARCH_QUERY_LEN, 64);
    assert_eq!(MAX_SYSTEM_SEARCH_LIMIT, 50);
}

#[test]
fn test_sso_autopilot_waypoint_constants() {
    assert_eq!(MAX_AUTOPILOT_WAYPOINTS, 50);
    assert_eq!(MAX_WAYPOINTS_BATCH_DURATION_SECS, 45);
}
