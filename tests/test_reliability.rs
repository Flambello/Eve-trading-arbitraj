use eve_trade_scout::graph::SystemGraph;
use eve_trade_scout::sde::{SdeLoader, SystemNode, TempFileCleanupGuard};
use std::sync::Arc;

#[test]
fn test_temp_file_cleanup_guard_removes_on_error_and_preserves_on_disarm() {
    let temp_dir = std::env::temp_dir().join(format!("eve_rel_guard_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);

    let test_file = temp_dir.join("test_download.part");
    std::fs::write(&test_file, b"partial download data").expect("Failed to write test file");
    assert!(test_file.exists());

    // 1. Simulate an early return / error: guard drops without disarm
    {
        let _guard = TempFileCleanupGuard::new(&test_file);
        // Simulate error exit
    }
    assert!(
        !test_file.exists(),
        "Guard must automatically remove temporary file when dropped without disarm"
    );

    // 2. Simulate success: guard is explicitly disarmed before drop
    std::fs::write(&test_file, b"completed validated data").expect("Failed to write test file");
    assert!(test_file.exists());
    {
        let guard = TempFileCleanupGuard::new(&test_file);
        // Successful validation and atomic rename simulated: disarm guard
        guard.disarm();
    }
    assert!(
        test_file.exists(),
        "Guard must preserve temporary file when explicitly disarmed"
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_graph_jump_cache_poison_recovery() {
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
    ];
    let jumps = vec![(30000142, 30000144)];
    let graph = Arc::new(SystemGraph::from_nodes_and_jumps(systems, jumps));

    // Populate initial cache
    let initial_jumps = graph.jump_count(30000142, 30000144, true);
    assert_eq!(initial_jumps, 1);

    // Intentionally poison graph.jump_cache mutex to simulate worker thread panic
    graph.poison_jump_cache_for_test();

    assert!(
        graph.is_jump_cache_poisoned(),
        "jump_cache mutex must be poisoned by simulated worker panic"
    );

    // Verify jump_count recovers from the poisoned mutex without panicking
    let recovered_jumps = graph.jump_count(30000142, 30000144, true);
    assert_eq!(
        recovered_jumps, 1,
        "jump_count must recover from poisoned mutex and return correct distance"
    );

    // Verify cache can still receive new entries after poisoning recovery
    let same_system = graph.jump_count(30000142, 30000142, true);
    assert_eq!(same_system, 0);
}

#[test]
fn test_sde_loader_mutex_poison_recovery() {
    let sde = Arc::new(
        SdeLoader::in_memory_with_fixtures().expect("Failed to initialize in-memory SDE fixtures"),
    );

    // Verify baseline query works
    assert_eq!(sde.get_system_id("Jita"), Some(30000142));

    // Intentionally poison sde.conn mutex to simulate worker thread panic
    sde.poison_conn_for_test();

    assert!(
        sde.is_conn_poisoned(),
        "sde.conn mutex must be poisoned by simulated worker panic"
    );

    // 1. Verify get_system_id recovers from poisoned connection
    assert_eq!(
        sde.get_system_id("Jita"),
        Some(30000142),
        "get_system_id must recover from poisoned mutex"
    );

    // 2. Verify all_systems recovers from poisoned connection
    let systems = sde
        .all_systems()
        .expect("all_systems must recover from poisoned mutex");
    assert!(!systems.is_empty(), "Systems list must not be empty");

    // 3. Verify all_jumps recovers from poisoned connection
    let jumps = sde
        .all_jumps()
        .expect("all_jumps must recover from poisoned mutex");
    assert!(!jumps.is_empty(), "Jumps list must not be empty");

    // 4. Verify get_item_info recovers from poisoned connection
    let item = sde
        .get_item_info(34)
        .expect("Item info query must recover from poisoned mutex");
    assert_eq!(item.name, "Tritanium");
}

#[tokio::test]
async fn test_regional_market_fetch_aborts_tasks_on_network_error() {
    use eve_trade_scout::esi::fetch_markets_opts;
    use eve_trade_scout::esi::EsiClient;

    let temp_cache = std::env::temp_dir().join(format!("eve_market_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_cache);

    let client = EsiClient::new(&temp_cache).expect("Client init must succeed");

    // Pass invalid region IDs to trigger network failure and task abortion
    let res = fetch_markets_opts(&client, &[999999991, 999999992, 999999993], true).await;
    assert!(
        res.is_err(),
        "fetch_markets_opts must return error on invalid regions"
    );

    let _ = std::fs::remove_dir_all(&temp_cache);
}

#[tokio::test]
async fn test_save_cache_cleans_up_tmp_file_on_error() {
    use eve_trade_scout::esi::EsiClient;

    let temp_cache_dir =
        std::env::temp_dir().join(format!("eve_save_cache_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_cache_dir);

    let client = EsiClient::new(&temp_cache_dir).expect("Client init must succeed");

    // Create a directory at the destination path `etags.json` so that renaming tmp_file to etags_file fails
    let target_etags_file = temp_cache_dir.join("etags.json");
    std::fs::create_dir(&target_etags_file).expect("Failed to create blocking dir at target path");

    let res = client.save_cache().await;
    assert!(
        res.is_err(),
        "save_cache must fail when target rename destination is a directory"
    );

    let tmp_file = temp_cache_dir.join("etags.json.tmp");
    assert!(
        !tmp_file.exists(),
        "etags.json.tmp must be cleaned up and not exist after failed save_cache"
    );

    let _ = std::fs::remove_dir_all(&temp_cache_dir);
}

#[test]
fn test_chain_route_nan_and_non_finite_defensive_safety() {
    use eve_trade_scout::arbitrage::ArbitrageDeal;
    use eve_trade_scout::chain_route_planner::{
        find_chain_routes, knapsack_pack_constrained, ChainConfig,
    };
    use eve_trade_scout::graph::SystemGraph;
    use std::collections::{HashMap, HashSet};

    let graph = SystemGraph::from_nodes_and_jumps(vec![], vec![]);
    let deal = ArbitrageDeal {
        type_id: 34,
        item_name: "Tritanium".to_string(),
        source_order_id: 1001,
        dest_order_id: 2001,
        source_order_volume_remain: 1000,
        dest_order_volume_remain: 1000,
        dest_order_min_volume: 1,
        is_npc_trade: false,
        buy_price: 5.0,
        sell_price: 10.0,
        spread_pct: 100.0,
        source_station_id: 60003760,
        source_system_id: 30000142,
        source_system_name: "Jita".to_string(),
        source_station_name: "Jita IV - 4".to_string(),
        source_security: 0.95,
        dest_station_id: 60003761,
        dest_system_id: 30000144,
        dest_system_name: "Perimeter".to_string(),
        dest_station_name: "Perimeter I".to_string(),
        dest_security: 0.90,
        quantity: 100,
        volume_used: 1.0,
        capital_required: 500.0,
        gross_profit: 500.0,
        taxes_and_fees: 100.0,
        net_profit: 400.0,
        net_profit_pct: 80.0,
        jumps_from_home: 0,
        jumps_source_dest: 1,
        total_jumps: 1,
        profit_per_jump: 400.0,
        profit_per_m3: 400.0,
    };

    // 1. find_chain_routes must gracefully return empty vec on NaN / negative capacities or budgets
    let mut config = ChainConfig {
        ship_max_volume: f64::NAN,
        ..Default::default()
    };
    let routes = find_chain_routes(std::slice::from_ref(&deal), &graph, &config);
    assert!(
        routes.is_empty(),
        "Must return empty vec for NaN cargo capacity"
    );

    config.ship_max_volume = 30_000.0;
    config.ship_max_budget = f64::NAN;
    let routes = find_chain_routes(std::slice::from_ref(&deal), &graph, &config);
    assert!(
        routes.is_empty(),
        "Must return empty vec for NaN max budget"
    );

    config.ship_max_volume = -50.0;
    config.ship_max_budget = 50_000_000.0;
    let routes = find_chain_routes(std::slice::from_ref(&deal), &graph, &config);
    assert!(
        routes.is_empty(),
        "Must return empty vec for negative cargo capacity"
    );

    // 2. knapsack_pack_constrained must skip deals with NaN volume_used or non-finite unit economics
    let mut bad_deal = deal.clone();
    bad_deal.volume_used = f64::NAN;

    let mut bad_deal_2 = deal.clone();
    bad_deal_2.buy_price = f64::NAN;

    let used_keys = HashSet::new();
    let consumed_sources = HashMap::new();
    let consumed_dests = HashMap::new();
    let exhausted_sources = HashSet::new();
    let exhausted_dests = HashSet::new();

    let pack = knapsack_pack_constrained(
        &[&bad_deal, &bad_deal_2, &deal],
        30_000.0,
        50_000_000.0,
        &used_keys,
        &consumed_sources,
        &consumed_dests,
        &exhausted_sources,
        &exhausted_dests,
    );
    // Bad deals with NaN volume or price were skipped; valid deal was packed cleanly
    assert_eq!(pack.packed.len(), 1);
    assert_eq!(pack.packed[0].deal.type_id, 34);
    assert!(pack.total_volume.is_finite());
    assert!(pack.total_capital.is_finite());
}

#[test]
fn test_arbitrage_deal_negative_or_nan_fees_rejected() {
    use eve_trade_scout::arbitrage::{calculate_deal, ArbitrageConfig};
    use eve_trade_scout::config::BIG_HAULER;
    use eve_trade_scout::esi::models::MarketOrder;
    use eve_trade_scout::sde::ItemInfo;

    let sde =
        SdeLoader::in_memory_with_fixtures().expect("Failed to initialize in-memory SDE fixtures");
    let graph = SystemGraph::from_sde(&sde).unwrap();

    let mut cfg = ArbitrageConfig {
        ship: BIG_HAULER,
        home_system: 30000142,
        min_profit: 10_000.0,
        min_spread: 3.0,
        min_net_profit_pct: 10.0,
        broker_fee: 3.0,
        sales_tax: 8.0,
        safe_only: true,
        npc_stations_only: true,
    };

    let item = ItemInfo {
        type_id: 34,
        name: "Tritanium".to_string(),
        packaged_volume: 0.01,
        group_id: 18,
        category_id: 4,
        published: true,
    };

    let src_ok = MarketOrder {
        order_id: 1,
        type_id: 34,
        location_id: 60003760,
        system_id: 30000142,
        volume_total: 10_000,
        volume_remain: 10_000,
        min_volume: 1,
        price: 5.0,
        is_buy_order: false,
        duration: 90,
        issued: None,
        range: None,
    };

    let dst_ok = MarketOrder {
        order_id: 2,
        type_id: 34,
        location_id: 60003761,
        system_id: 30000144,
        volume_total: 10_000,
        volume_remain: 10_000,
        min_volume: 1,
        price: 10.0,
        is_buy_order: true,
        duration: 90,
        issued: None,
        range: None,
    };

    // Baseline: Valid deal succeeds
    assert!(calculate_deal(34, &item, &src_ok, &dst_ok, &cfg, &sde, &graph).is_some());

    // 1. Negative broker fee rejected
    cfg.broker_fee = -1.0;
    assert!(calculate_deal(34, &item, &src_ok, &dst_ok, &cfg, &sde, &graph).is_none());

    // 2. NaN broker fee rejected
    cfg.broker_fee = f64::NAN;
    assert!(calculate_deal(34, &item, &src_ok, &dst_ok, &cfg, &sde, &graph).is_none());
    cfg.broker_fee = 3.0;

    // 3. Negative sales tax rejected
    cfg.sales_tax = -0.5;
    assert!(calculate_deal(34, &item, &src_ok, &dst_ok, &cfg, &sde, &graph).is_none());

    // 4. NaN sales tax rejected
    cfg.sales_tax = f64::NAN;
    assert!(calculate_deal(34, &item, &src_ok, &dst_ok, &cfg, &sde, &graph).is_none());
    cfg.sales_tax = 8.0;

    // 5. Non-finite or negative ship limits rejected
    cfg.ship.max_volume = f64::NAN;
    assert!(calculate_deal(34, &item, &src_ok, &dst_ok, &cfg, &sde, &graph).is_none());
    cfg.ship.max_volume = -100.0;
    assert!(calculate_deal(34, &item, &src_ok, &dst_ok, &cfg, &sde, &graph).is_none());
    cfg.ship = BIG_HAULER;

    cfg.ship.max_budget = f64::NAN;
    assert!(calculate_deal(34, &item, &src_ok, &dst_ok, &cfg, &sde, &graph).is_none());
    cfg.ship.max_budget = 0.0;
    assert!(calculate_deal(34, &item, &src_ok, &dst_ok, &cfg, &sde, &graph).is_none());
    cfg.ship = BIG_HAULER;

    // 6. Non-finite or non-positive item volume rejected
    let mut bad_item = item.clone();
    bad_item.packaged_volume = f64::NAN;
    assert!(calculate_deal(34, &bad_item, &src_ok, &dst_ok, &cfg, &sde, &graph).is_none());
    bad_item.packaged_volume = 0.0;
    assert!(calculate_deal(34, &bad_item, &src_ok, &dst_ok, &cfg, &sde, &graph).is_none());
    bad_item.packaged_volume = -1.0;
    assert!(calculate_deal(34, &bad_item, &src_ok, &dst_ok, &cfg, &sde, &graph).is_none());
}

#[test]
fn test_sde_loader_header_validation_file_lock_safety() {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    let temp_dir = std::env::temp_dir().join(format!("eve_sde_lock_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_dir);
    let target_dest = temp_dir.join("test_sde.sqlite");

    // Create a gzip archive containing invalid SQLite bytes (not starting with "SQLite format 3\0")
    let mut gz_encoder = GzEncoder::new(Vec::new(), Compression::default());
    gz_encoder
        .write_all(b"NOT A SQLITE DATABASE HEADER AT ALL - INVALID DATA")
        .expect("Write gz data");
    let compressed_bytes = gz_encoder.finish().expect("Finish compression");

    let res = SdeLoader::decompress_and_validate_archive(&compressed_bytes[..], &target_dest);
    assert!(
        res.is_err(),
        "decompress_and_validate_archive must fail when database header is invalid"
    );

    let part_path = target_dest.with_extension("sqlite.part");
    assert!(
        !part_path.exists(),
        "Temporary .part file must be cleaned up and not locked"
    );
    assert!(
        !target_dest.exists(),
        "Target dest file must not exist on failed header validation"
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_fetch_character_location_retry_resilience() {
    use eve_trade_scout::esi::sso::{fetch_character_location_at_url, SsoToken};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Must bind test listener");
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{}", addr);

    let server_task = tokio::spawn(async move {
        // First connection: return HTTP 500 Internal Server Error
        let (mut socket1, _) = listener.accept().await.expect("Accept 1 failed");
        let mut buf1 = vec![0u8; 1024];
        let _ = socket1.read(&mut buf1).await.expect("Read 1 failed");
        let resp_500 =
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        socket1
            .write_all(resp_500.as_bytes())
            .await
            .expect("Write 1 failed");
        let _ = socket1.flush().await;

        // Second connection: return HTTP 200 OK with valid location JSON
        let (mut socket2, _) = listener.accept().await.expect("Accept 2 failed");
        let mut buf2 = vec![0u8; 1024];
        let _ = socket2.read(&mut buf2).await.expect("Read 2 failed");
        let body = r#"{"solar_system_id": 30000142, "station_id": 60003760}"#;
        let resp_200 = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket2
            .write_all(resp_200.as_bytes())
            .await
            .expect("Write 2 failed");
        let _ = socket2.flush().await;
    });

    let client = reqwest::Client::new();
    let token = SsoToken {
        access_token: "mock_access_token".to_string(),
        refresh_token: "mock_refresh_token".to_string(),
        token_type: "Bearer".to_string(),
        expires_at: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp(),
        character_id: 12345678,
        character_name: "Mock Pilot".to_string(),
        client_id: None,
    };

    let loc_res = fetch_character_location_at_url(&client, &base_url, &token).await;
    assert!(
        loc_res.is_ok(),
        "fetch_character_location_at_url must retry after 500 and succeed: {:?}",
        loc_res.err()
    );
    let loc = loc_res.unwrap();
    assert_eq!(loc.solar_system_id, 30000142);
    assert_eq!(loc.station_id, Some(60003760));

    server_task.await.expect("Server task failed");
}

#[tokio::test]
async fn test_esi_client_http_408_retry_resilience() {
    use eve_trade_scout::esi::models::MarketOrder;
    use eve_trade_scout::esi::EsiClient;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let temp_cache_dir = std::env::temp_dir().join(format!("eve_408_test_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&temp_cache_dir);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Must bind test listener");
    let addr = listener.local_addr().unwrap();
    let mock_url = format!("http://{}/v1/markets/10000002/orders/", addr);

    let server_task = tokio::spawn(async move {
        // Attempt 1: return HTTP 408 Request Timeout
        let (mut socket1, _) = listener.accept().await.expect("Accept 1 failed");
        let mut buf1 = vec![0u8; 1024];
        let _ = socket1.read(&mut buf1).await.expect("Read 1 failed");
        let resp_408 =
            "HTTP/1.1 408 Request Timeout\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        socket1
            .write_all(resp_408.as_bytes())
            .await
            .expect("Write 1 failed");
        let _ = socket1.flush().await;

        // Attempt 2: return HTTP 200 OK with empty order array
        let (mut socket2, _) = listener.accept().await.expect("Accept 2 failed");
        let mut buf2 = vec![0u8; 1024];
        let _ = socket2.read(&mut buf2).await.expect("Read 2 failed");
        let body = "[]";
        let resp_200 = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-Pages: 1\r\nETag: \"etag-408\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket2
            .write_all(resp_200.as_bytes())
            .await
            .expect("Write 2 failed");
        let _ = socket2.flush().await;
    });

    let client = EsiClient::new(&temp_cache_dir).expect("Client init must succeed");
    let res = client
        .get_all_pages_opts::<MarketOrder>(&mock_url, &[("order_type", "all".to_string())], true)
        .await;
    assert!(
        res.is_ok(),
        "get_all_pages_opts must retry after HTTP 408 and succeed: {:?}",
        res.err()
    );
    let orders = res.unwrap();
    assert!(orders.is_empty());

    server_task.await.expect("Server task failed");
    let _ = std::fs::remove_dir_all(&temp_cache_dir);
}
