use eve_trade_scout::esi::models::MarketOrder;

#[test]
fn test_market_order_json_deserialization() {
    let json_data = r#"[
        {
            "order_id": 6829104812,
            "type_id": 34,
            "location_id": 60003760,
            "system_id": 30000142,
            "volume_total": 100000,
            "volume_remain": 85000,
            "min_volume": 1,
            "price": 5.42,
            "is_buy_order": false,
            "duration": 90,
            "issued": "2026-03-01T12:00:00Z",
            "range": "region"
        },
        {
            "order_id": 6829104999,
            "type_id": 34,
            "location_id": 60008494,
            "system_id": 30002187,
            "volume_total": 50000,
            "volume_remain": 40000,
            "min_volume": 1,
            "price": 7.15,
            "is_buy_order": true,
            "duration": 90,
            "issued": "2026-03-01T12:30:00Z",
            "range": "station"
        }
    ]"#;

    let orders: Vec<MarketOrder> = serde_json::from_str(json_data)
        .expect("ESI MarketOrder JSON array must deserialize correctly");

    assert_eq!(orders.len(), 2);

    let sell_order = &orders[0];
    assert_eq!(sell_order.order_id, 6829104812);
    assert_eq!(sell_order.type_id, 34);
    assert_eq!(sell_order.location_id, 60003760);
    assert_eq!(sell_order.system_id, 30000142);
    assert_eq!(sell_order.price, 5.42);
    assert_eq!(sell_order.volume_remain, 85000);
    assert!(!sell_order.is_buy_order, "First order is a sell order");

    let buy_order = &orders[1];
    assert_eq!(buy_order.order_id, 6829104999);
    assert_eq!(buy_order.type_id, 34);
    assert_eq!(buy_order.location_id, 60008494);
    assert_eq!(buy_order.system_id, 30002187);
    assert_eq!(buy_order.price, 7.15);
    assert_eq!(buy_order.volume_remain, 40000);
    assert!(buy_order.is_buy_order, "Second order is a buy order");
}

#[test]
fn test_market_order_json_deserialization_rejects_malformed_and_missing_fields() {
    // 1. Missing required field "price"
    let missing_price = r#"{
        "order_id": 1,
        "type_id": 34,
        "location_id": 60003760,
        "system_id": 30000142,
        "volume_total": 100,
        "volume_remain": 100,
        "min_volume": 1,
        "is_buy_order": false,
        "duration": 90
    }"#;
    assert!(
        serde_json::from_str::<MarketOrder>(missing_price).is_err(),
        "Deserialization must fail when required field 'price' is missing"
    );

    // 2. Missing required field "order_id"
    let missing_order_id = r#"{
        "type_id": 34,
        "location_id": 60003760,
        "system_id": 30000142,
        "volume_total": 100,
        "volume_remain": 100,
        "min_volume": 1,
        "price": 5.42,
        "is_buy_order": false,
        "duration": 90
    }"#;
    assert!(
        serde_json::from_str::<MarketOrder>(missing_order_id).is_err(),
        "Deserialization must fail when required field 'order_id' is missing"
    );

    // 3. String value where numeric expected
    let wrong_type = r#"{
        "order_id": "not_an_integer",
        "type_id": 34,
        "location_id": 60003760,
        "system_id": 30000142,
        "volume_total": 100,
        "volume_remain": 100,
        "min_volume": 1,
        "price": 5.42,
        "is_buy_order": false,
        "duration": 90
    }"#;
    assert!(
        serde_json::from_str::<MarketOrder>(wrong_type).is_err(),
        "Deserialization must fail when numeric field contains string"
    );

    // 4. Corrupt JSON syntax
    let corrupt_json = r#"[{"order_id": 12345, "type_id":"#;
    assert!(
        serde_json::from_str::<Vec<MarketOrder>>(corrupt_json).is_err(),
        "Deserialization must fail on truncated / invalid JSON syntax"
    );

    // 5. Empty object
    assert!(
        serde_json::from_str::<MarketOrder>("{}").is_err(),
        "Empty JSON object must fail deserialization"
    );
}

use eve_trade_scout::esi::market::{best_buy, cheapest_sell};

#[test]
fn test_cheapest_sell_and_best_buy_selection() {
    let mixed_orders = [
        // Sells
        MarketOrder {
            order_id: 1,
            type_id: 34,
            location_id: 60003760,
            system_id: 30000142,
            volume_total: 100,
            volume_remain: 100,
            min_volume: 1,
            price: 6.20,
            is_buy_order: false,
            duration: 90,
            issued: None,
            range: None,
        },
        MarketOrder {
            order_id: 2,
            type_id: 34,
            location_id: 60003760,
            system_id: 30000142,
            volume_total: 100,
            volume_remain: 100,
            min_volume: 1,
            price: 5.80, // Legitimate cheapest sell
            is_buy_order: false,
            duration: 90,
            issued: None,
            range: None,
        },
        MarketOrder {
            order_id: 3,
            type_id: 34,
            location_id: 60003760,
            system_id: 30000142,
            volume_total: 100,
            volume_remain: 0, // Exhausted volume - MUST be ignored by cheapest_sell!
            min_volume: 1,
            price: 4.50,
            is_buy_order: false,
            duration: 90,
            issued: None,
            range: None,
        },
        // Buys
        MarketOrder {
            order_id: 4,
            type_id: 34,
            location_id: 60008494,
            system_id: 30002187,
            volume_total: 100,
            volume_remain: 100,
            min_volume: 1,
            price: 7.50,
            is_buy_order: true,
            duration: 90,
            issued: None,
            range: None,
        },
        MarketOrder {
            order_id: 5,
            type_id: 34,
            location_id: 60008494,
            system_id: 30002187,
            volume_total: 100,
            volume_remain: 100,
            min_volume: 1,
            price: 8.10, // Legitimate best buy
            is_buy_order: true,
            duration: 90,
            issued: None,
            range: None,
        },
        MarketOrder {
            order_id: 6,
            type_id: 34,
            location_id: 60008494,
            system_id: 30002187,
            volume_total: 100,
            volume_remain: 0, // Exhausted volume - MUST be ignored by best_buy!
            price: 9.99,
            min_volume: 1,
            is_buy_order: true,
            duration: 90,
            issued: None,
            range: None,
        },
    ];

    // Call real production functions:
    let cheapest = cheapest_sell(&mixed_orders).expect("Must find cheapest sell");
    assert_eq!(cheapest.order_id, 2);
    assert_eq!(cheapest.price, 5.80);
    assert!(!cheapest.is_buy_order);
    assert!(cheapest.volume_remain > 0);

    let best = best_buy(&mixed_orders).expect("Must find best buy");
    assert_eq!(best.order_id, 5);
    assert_eq!(best.price, 8.10);
    assert!(best.is_buy_order);
    assert!(best.volume_remain > 0);

    // Empty orders return None
    assert!(cheapest_sell(&[]).is_none());
    assert!(best_buy(&[]).is_none());

    // Buy-only orders return None for cheapest_sell
    let buy_only: Vec<_> = mixed_orders
        .iter()
        .filter(|o| o.is_buy_order)
        .cloned()
        .collect();
    assert!(cheapest_sell(&buy_only).is_none());

    // Sell-only orders return None for best_buy
    let sell_only: Vec<_> = mixed_orders
        .iter()
        .filter(|o| !o.is_buy_order)
        .cloned()
        .collect();
    assert!(best_buy(&sell_only).is_none());
}

use eve_trade_scout::esi::{parse_http_expires_str, StoredEtag};

#[test]
fn test_stored_etag_freshness() {
    let now = chrono::Utc::now();

    // 1. Entry with future expiration date is fresh
    let fresh_entry = StoredEtag {
        etag: "\"abcdef\"".to_string(),
        expires: Some(now + chrono::Duration::seconds(300)),
        body_json: "[]".to_string(),
    };
    assert!(
        fresh_entry.is_fresh(),
        "Entry expiring in 300s must be fresh"
    );

    // 2. Entry with past expiration date is stale
    let expired_entry = StoredEtag {
        etag: "\"abcdef\"".to_string(),
        expires: Some(now - chrono::Duration::seconds(10)),
        body_json: "[]".to_string(),
    };
    assert!(
        !expired_entry.is_fresh(),
        "Entry expired 10s ago must be stale"
    );

    // 3. Entry with None expires (legacy cache entries) must be treated as stale
    let legacy_entry = StoredEtag {
        etag: "\"abcdef\"".to_string(),
        expires: None,
        body_json: "[]".to_string(),
    };
    assert!(
        !legacy_entry.is_fresh(),
        "Legacy entry with expires: None must be stale"
    );
}

#[test]
fn test_parse_http_expires_header() {
    // Standard RFC 2822 HTTP date from ESI
    let rfc2822_header = "Tue, 13 Oct 2026 12:30:00 GMT";
    let parsed = parse_http_expires_str(Some(rfc2822_header), 300);
    assert_eq!(
        parsed.to_rfc3339(),
        "2026-10-13T12:30:00+00:00",
        "Should correctly parse RFC 2822 HTTP date"
    );

    // Fallback when header is None
    let before = chrono::Utc::now();
    let fallback = parse_http_expires_str(None, 300);
    let after = chrono::Utc::now();
    assert!(fallback >= before + chrono::Duration::seconds(299));
    assert!(fallback <= after + chrono::Duration::seconds(301));

    // Fallback when header is invalid / malformed
    let invalid_fallback = parse_http_expires_str(Some("not-a-valid-date"), 300);
    assert!(invalid_fallback >= before + chrono::Duration::seconds(299));
    assert!(invalid_fallback <= after + chrono::Duration::seconds(301));
}

#[test]
fn test_corrupted_etags_file_on_disk_graceful_recovery() {
    let temp_dir = std::env::temp_dir().join(format!(
        "eve_test_corrupted_etags_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let etags_file = temp_dir.join("etags.json");
    // Write corrupted non-JSON garbage to disk
    std::fs::write(&etags_file, b"{{{{ corrupted invalid json payload !!!").unwrap();

    // Initializing EsiClient must not crash or fail with deserialization error
    let client_res = eve_trade_scout::esi::client::EsiClient::new(&temp_dir);
    assert!(
        client_res.is_ok(),
        "EsiClient must gracefully handle corrupted etags.json on disk"
    );

    let client = client_res.unwrap();
    // Saving cache should overwrite the corrupted file cleanly
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(client.save_cache())
        .expect("save_cache must succeed");

    let saved_content = std::fs::read_to_string(&etags_file).unwrap();
    assert_eq!(
        saved_content.trim(),
        "{}",
        "Corrupted etags.json must be replaced with valid empty JSON object"
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_corrupted_cached_body_purged_on_fetch() {
    let temp_dir = std::env::temp_dir().join(format!(
        "eve_test_corrupt_body_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    // Start a mock HTTP server to simulate the fresh ESI endpoint
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Must bind test listener");
    let addr = listener.local_addr().unwrap();
    let mock_url = format!("http://{}/v1/markets/10000002/orders/", addr);

    // Seed etags.json with a fresh TTL entry whose body_json is corrupted (wrong schema for MarketOrder)
    let cache_key = format!("{}:all", mock_url);
    let corrupted_stored = serde_json::json!({
        cache_key: {
            "etag": "\"corrupt-etag\"",
            "expires": (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
            "body_json": "[{\"order_id\": \"not_a_number\", \"type_id\": 34}]"
        }
    });
    std::fs::write(
        temp_dir.join("etags.json"),
        serde_json::to_string(&corrupted_stored).unwrap(),
    )
    .unwrap();

    let client = eve_trade_scout::esi::client::EsiClient::new(&temp_dir).unwrap();

    // Mock server task returns a valid MarketOrder JSON array
    let server_task = tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut socket, _) = listener.accept().await.expect("Accept failed");
        let mut buf = vec![0u8; 2048];
        let n = socket.read(&mut buf).await.expect("Read failed");
        let req_str = String::from_utf8_lossy(&buf[..n]);
        assert!(req_str.starts_with("GET "));

        let body = r#"[{"order_id":999888,"type_id":34,"location_id":60003760,"system_id":30000142,"volume_total":100,"volume_remain":100,"min_volume":1,"price":5.5,"is_buy_order":false,"duration":90,"issued":"2026-03-01T12:00:00Z","range":"region"}]"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-Pages: 1\r\nETag: \"new-etag\"\r\nExpires: Tue, 13 Oct 2027 12:30:00 GMT\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("Write failed");
    });

    // Calling get_all_pages_opts will hit the corrupted cached entry, catch the deserialization error,
    // purge it, and fetch from the mock server!
    let orders: Vec<MarketOrder> = client
        .get_all_pages_opts(&mock_url, &[], false)
        .await
        .expect(
            "get_all_pages_opts must succeed by falling through to fresh fetch on corrupted cache",
        );

    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0].order_id, 999888);
    assert_eq!(orders[0].price, 5.5);

    server_task.await.unwrap();

    // Now save cache and verify etags.json no longer has the corrupted body
    client.save_cache().await.unwrap();
    let saved_content = std::fs::read_to_string(temp_dir.join("etags.json")).unwrap();
    assert!(
        !saved_content.contains("not_a_number"),
        "Corrupted body must be evicted from cache"
    );
    assert!(
        saved_content.contains("999888"),
        "Fresh valid order must be saved in cache"
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}
