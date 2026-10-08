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
fn test_cheapest_sell_and_best_buy_selection() {
    let sells = vec![
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
            price: 5.80, // Cheapest sell
            is_buy_order: false,
            duration: 90,
            issued: None,
            range: None,
        },
    ];

    let buys = vec![
        MarketOrder {
            order_id: 3,
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
            order_id: 4,
            type_id: 34,
            location_id: 60008494,
            system_id: 30002187,
            volume_total: 100,
            volume_remain: 100,
            min_volume: 1,
            price: 8.10, // Best buy
            is_buy_order: true,
            duration: 90,
            issued: None,
            range: None,
        },
    ];

    let cheapest = sells.iter().min_by(|a, b| a.price.partial_cmp(&b.price).unwrap()).unwrap();
    let best = buys.iter().max_by(|a, b| a.price.partial_cmp(&b.price).unwrap()).unwrap();

    assert_eq!(cheapest.order_id, 2);
    assert_eq!(cheapest.price, 5.80);
    assert_eq!(best.order_id, 4);
    assert_eq!(best.price, 8.10);
}
