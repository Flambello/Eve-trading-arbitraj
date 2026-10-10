// Unit tests for arbitrage calculations and ship profiles

#[cfg(test)]
mod tests {
    use eve_trade_scout::arbitrage::{
        calculate_deal, is_npc_station, ArbitrageConfig, ArbitrageDeal,
    };
    use eve_trade_scout::config::{BIG_HAULER, SMALL_HAULER};
    use eve_trade_scout::esi::models::MarketOrder;
    use eve_trade_scout::graph::SystemGraph;
    use eve_trade_scout::sde::{ItemInfo, SdeLoader, SystemNode};

    fn create_test_env() -> (SdeLoader, SystemGraph, ArbitrageConfig, ItemInfo) {
        let sde = SdeLoader::in_memory_with_fixtures().expect("Failed to initialize test SDE");
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
                system_id: 30003491,
                name: "Niarja".to_string(),
                security_status: 0.38, // Lowsec
                region_id: 10000043,
            },
        ];
        let jumps = vec![
            (30000142, 30000144), // Jita <-> Perimeter
            (30000144, 30002187), // Perimeter <-> Amarr
            (30000144, 30003491), // Perimeter <-> Niarja
        ];
        let graph = SystemGraph::from_nodes_and_jumps(systems, jumps);
        let cfg = ArbitrageConfig {
            home_system: 30000142, // Jita
            ship: BIG_HAULER,
            min_profit: 0.0,
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
        (sde, graph, cfg, item)
    }

    #[test]
    fn test_ship_profile_values() {
        // Must verify real domain constants from eve_trade_scout::config
        assert_eq!(BIG_HAULER.name, "Big Hauler");
        assert_eq!(BIG_HAULER.max_volume, 60_000.0);
        assert_eq!(BIG_HAULER.max_budget, 500_000_000.0);

        assert_eq!(SMALL_HAULER.name, "Small Hauler");
        assert_eq!(SMALL_HAULER.max_volume, 30_000.0);
        assert_eq!(SMALL_HAULER.max_budget, 50_000_000.0);
    }

    #[test]
    fn test_profit_per_jump_formula() {
        let (sde, graph, cfg, item) = create_test_env();

        // Source in Perimeter (1 jump from home Jita)
        let src_order = MarketOrder {
            order_id: 1001,
            type_id: 34,
            location_id: 60003761,
            system_id: 30000144, // Perimeter
            price: 100.0,
            volume_total: 1000,
            volume_remain: 1000,
            min_volume: 1,
            is_buy_order: false,
            duration: 90,
            issued: None,
            range: None,
        };
        // Dest in Amarr (1 jump from Perimeter)
        let dst_order = MarketOrder {
            order_id: 2001,
            type_id: 34,
            location_id: 60008494,
            system_id: 30002187, // Amarr
            price: 150.0,
            volume_total: 1000,
            volume_remain: 1000,
            min_volume: 1,
            is_buy_order: true,
            duration: 90,
            issued: None,
            range: None,
        };

        let deal = calculate_deal(34, &item, &src_order, &dst_order, &cfg, &sde, &graph)
            .expect("Deal must be calculated");

        // jumps_from_home (Jita -> Perimeter) = 1
        // jumps_source_dest (Perimeter -> Amarr) = 1
        // total_jumps = 2
        assert_eq!(deal.jumps_from_home, 1);
        assert_eq!(deal.jumps_source_dest, 1);
        assert_eq!(deal.total_jumps, 2);

        // Profit per jump must equal net_profit / total_jumps
        assert!(deal.net_profit > 0.0);
        let expected_ppj = deal.net_profit / 2.0;
        assert!((deal.profit_per_jump - expected_ppj).abs() < 1e-4);
        assert!((deal.profit_per_m3 - (deal.net_profit / deal.volume_used)).abs() < 1e-4);
    }

    #[test]
    fn test_minimum_10_percent_net_profitability_enforced() {
        let (sde, graph, cfg, item) = create_test_env();

        // Source order: 100 ISK
        let src = MarketOrder {
            order_id: 101,
            type_id: 34,
            location_id: 60003760,
            system_id: 30000142,
            price: 100.0,
            volume_total: 100,
            volume_remain: 100,
            min_volume: 1,
            is_buy_order: false,
            duration: 90,
            issued: None,
            range: None,
        };

        // Deal A: 120 ISK sell price
        // Broker fee: 100 * 3% = 3.0
        // Sales tax: 120 * 8% = 9.6
        // Net profit per unit: 20 - 3.0 - 9.6 = 7.4 (ROI 7.4% < 10.0%) -> REJECTED
        let dst_sub10 = MarketOrder {
            order_id: 201,
            type_id: 34,
            location_id: 60003761,
            system_id: 30000144,
            price: 120.0,
            volume_total: 100,
            volume_remain: 100,
            min_volume: 1,
            is_buy_order: true,
            duration: 90,
            issued: None,
            range: None,
        };

        let res_a = calculate_deal(34, &item, &src, &dst_sub10, &cfg, &sde, &graph);
        assert!(
            res_a.is_none(),
            "Deal with 7.4% net ROI must be rejected by calculate_deal"
        );

        // Deal B: 125 ISK sell price
        // Broker fee: 100 * 3% = 3.0
        // Sales tax: 125 * 8% = 10.0
        // Net profit per unit: 25 - 3.0 - 10.0 = 12.0 (ROI 12.0% >= 10.0%) -> ACCEPTED
        let dst_above10 = MarketOrder {
            order_id: 202,
            type_id: 34,
            location_id: 60003761,
            system_id: 30000144,
            price: 125.0,
            volume_total: 100,
            volume_remain: 100,
            min_volume: 1,
            is_buy_order: true,
            duration: 90,
            issued: None,
            range: None,
        };

        let res_b = calculate_deal(34, &item, &src, &dst_above10, &cfg, &sde, &graph);
        assert!(
            res_b.is_some(),
            "Deal with 12.0% net ROI must be accepted by calculate_deal"
        );
        let deal = res_b.unwrap();
        assert_eq!(deal.net_profit_pct, 12.0);
    }

    #[test]
    fn test_is_npc_station_id_threshold() {
        // Direct verification of eve_trade_scout::arbitrage::is_npc_station
        assert!(is_npc_station(60_003_760), "Jita IV-4 is an NPC station");
        assert!(is_npc_station(60_008_494), "Amarr VIII-4 is an NPC station");
        assert!(
            is_npc_station(60_011_866),
            "Dodixie IX-20 is an NPC station"
        );

        // Player-owned Citadels / Upwell Structures have IDs >= 1_000_000_000_000
        assert!(
            !is_npc_station(1_028_858_195_912),
            "Perimeter Citadel is NOT an NPC station"
        );
        assert!(
            !is_npc_station(1_030_000_000_001),
            "Fortizar is NOT an NPC station"
        );
    }

    #[test]
    fn test_npc_stations_only_toggle_filter() {
        let (sde, graph, mut cfg, item) = create_test_env();

        let src = MarketOrder {
            order_id: 101,
            type_id: 34,
            location_id: 60003760, // NPC station
            system_id: 30000142,
            price: 5.0,
            volume_total: 100,
            volume_remain: 100,
            min_volume: 1,
            is_buy_order: false,
            duration: 90,
            issued: None,
            range: None,
        };

        let dst_citadel = MarketOrder {
            order_id: 201,
            type_id: 34,
            location_id: 1_028_858_195_912, // Player Citadel structure!
            system_id: 30000144,
            price: 10.0,
            volume_total: 100,
            volume_remain: 100,
            min_volume: 1,
            is_buy_order: true,
            duration: 90,
            issued: None,
            range: None,
        };

        // When npc_stations_only = true:
        cfg.npc_stations_only = true;
        let res_rejected = calculate_deal(34, &item, &src, &dst_citadel, &cfg, &sde, &graph);
        assert!(
            res_rejected.is_none(),
            "Citadel trade must be rejected when npc_stations_only is true"
        );

        // When npc_stations_only = false:
        cfg.npc_stations_only = false;
        let res_accepted = calculate_deal(34, &item, &src, &dst_citadel, &cfg, &sde, &graph);
        assert!(
            res_accepted.is_some(),
            "Citadel trade must be accepted when npc_stations_only is false"
        );
    }

    #[test]
    fn test_highsec_only_filtering_excludes_lowsec_and_nullsec() {
        let (sde, graph, cfg, item) = create_test_env();

        // Direct SystemGraph security checks:
        assert!(graph.is_highsec(30000142)); // Jita (0.95)
        assert!(graph.is_highsec(30000144)); // Perimeter (0.90)
        assert!(!graph.is_highsec(30003491)); // Niarja (0.38 - lowsec)

        // Deal from Jita to Lowsec Niarja:
        let src = MarketOrder {
            order_id: 101,
            type_id: 34,
            location_id: 60003760,
            system_id: 30000142,
            price: 5.0,
            volume_total: 100,
            volume_remain: 100,
            min_volume: 1,
            is_buy_order: false,
            duration: 90,
            issued: None,
            range: None,
        };
        let dst_lowsec = MarketOrder {
            order_id: 201,
            type_id: 34,
            location_id: 60003762,
            system_id: 30003491, // Niarja (Lowsec)
            price: 10.0,
            volume_total: 100,
            volume_remain: 100,
            min_volume: 1,
            is_buy_order: true,
            duration: 90,
            issued: None,
            range: None,
        };

        let res = calculate_deal(34, &item, &src, &dst_lowsec, &cfg, &sde, &graph);
        assert!(
            res.is_none(),
            "calculate_deal must reject deals with lowsec destination"
        );
    }

    #[test]
    fn test_quantity_solver_constraints() {
        let (sde, graph, mut cfg, mut item) = create_test_env();
        cfg.ship = BIG_HAULER; // 60,000 m³ hold, 500M budget

        // Case 1: Constrained by volume
        // Item is 100 m³, hold is 60,000 m³ -> max by volume is 600 units
        item.packaged_volume = 100.0;
        let src_1 = MarketOrder {
            order_id: 101,
            type_id: 34,
            location_id: 60003760,
            system_id: 30000142,
            price: 10_000.0,
            volume_total: 5_000,
            volume_remain: 5_000,
            min_volume: 1,
            is_buy_order: false,
            duration: 90,
            issued: None,
            range: None,
        };
        let dst_1 = MarketOrder {
            order_id: 201,
            type_id: 34,
            location_id: 60003761,
            system_id: 30000144,
            price: 20_000.0,
            volume_total: 5_000,
            volume_remain: 5_000,
            min_volume: 1,
            is_buy_order: true,
            duration: 90,
            issued: None,
            range: None,
        };
        let deal_1 = calculate_deal(34, &item, &src_1, &dst_1, &cfg, &sde, &graph)
            .expect("Deal 1 should calculate");
        assert_eq!(
            deal_1.quantity, 600,
            "Should be constrained by cargo volume"
        );

        // Case 2: Constrained by budget
        // Item price 25M each, budget 500M -> max by budget is 20 units
        item.packaged_volume = 0.1;
        let src_2 = MarketOrder {
            order_id: 102,
            type_id: 34,
            location_id: 60003760,
            system_id: 30000142,
            price: 25_000_000.0,
            volume_total: 100,
            volume_remain: 100,
            min_volume: 1,
            is_buy_order: false,
            duration: 90,
            issued: None,
            range: None,
        };
        let dst_2 = MarketOrder {
            order_id: 202,
            type_id: 34,
            location_id: 60003761,
            system_id: 30000144,
            price: 35_000_000.0,
            volume_total: 100,
            volume_remain: 100,
            min_volume: 1,
            is_buy_order: true,
            duration: 90,
            issued: None,
            range: None,
        };
        let deal_2 = calculate_deal(34, &item, &src_2, &dst_2, &cfg, &sde, &graph)
            .expect("Deal 2 should calculate");
        assert_eq!(
            deal_2.quantity, 20,
            "Should be constrained by ISK capital budget"
        );

        // Case 3: Constrained by supply
        let mut src_3 = src_2.clone();
        src_3.volume_remain = 8;
        let deal_3 = calculate_deal(34, &item, &src_3, &dst_2, &cfg, &sde, &graph)
            .expect("Deal 3 should calculate");
        assert_eq!(deal_3.quantity, 8, "Should be constrained by supply");

        // Case 4: Constrained by demand
        let mut dst_4 = dst_2.clone();
        dst_4.volume_remain = 5;
        let deal_4 = calculate_deal(34, &item, &src_2, &dst_4, &cfg, &sde, &graph)
            .expect("Deal 4 should calculate");
        assert_eq!(deal_4.quantity, 5, "Should be constrained by demand");
    }

    #[test]
    fn test_deal_sorting_ppj_descending() {
        let make_dummy_deal = |id: u32, ppj: f64, net: f64| ArbitrageDeal {
            type_id: id,
            item_name: format!("Item {}", id),
            source_order_id: 100 + id as u64,
            dest_order_id: 200 + id as u64,
            source_order_volume_remain: 100,
            dest_order_volume_remain: 100,
            dest_order_min_volume: 1,
            is_npc_trade: false,
            buy_price: 100.0,
            sell_price: 150.0,
            spread_pct: 50.0,
            source_station_id: 60003760,
            source_system_id: 30000142,
            source_system_name: "Jita".to_string(),
            source_station_name: "Jita IV-4".to_string(),
            source_security: 0.95,
            dest_station_id: 60003761,
            dest_system_id: 30000144,
            dest_system_name: "Perimeter".to_string(),
            dest_station_name: "Perimeter I".to_string(),
            dest_security: 0.90,
            quantity: 100,
            volume_used: 10.0,
            capital_required: 10_000.0,
            gross_profit: net * 1.2,
            taxes_and_fees: net * 0.2,
            net_profit: net,
            net_profit_pct: 25.0,
            jumps_from_home: 0,
            jumps_source_dest: 1,
            total_jumps: 1,
            profit_per_jump: ppj,
            profit_per_m3: net / 10.0,
        };

        let mut deals = [
            make_dummy_deal(1, 1.5, 10_000_000.0),
            make_dummy_deal(2, 4.2, 8_000_000.0),
            make_dummy_deal(3, 2.1, 15_000_000.0),
            make_dummy_deal(4, 4.2, 12_000_000.0), // Same PPJ as #2, but higher net
        ];

        // Production sort comparator (from src/arbitrage.rs:204-213)
        deals.sort_by(|a, b| {
            b.profit_per_jump
                .partial_cmp(&a.profit_per_jump)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    b.net_profit
                        .partial_cmp(&a.net_profit)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
        });

        // Expected order:
        // 1st: ID 4 (PPJ 4.2, net 12M)
        // 2nd: ID 2 (PPJ 4.2, net 8M)
        // 3rd: ID 3 (PPJ 2.1, net 15M)
        // 4th: ID 1 (PPJ 1.5, net 10M)
        assert_eq!(deals[0].type_id, 4);
        assert_eq!(deals[1].type_id, 2);
        assert_eq!(deals[2].type_id, 3);
        assert_eq!(deals[3].type_id, 1);
    }

    #[test]
    fn test_is_high_roi() {
        use eve_trade_scout::arbitrage::ArbitrageDeal;

        let mut deal = ArbitrageDeal {
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
            source_station_name: "Jita IV-4".to_string(),
            source_security: 1.0,
            dest_station_id: 60008494,
            dest_system_id: 30002187,
            dest_system_name: "Amarr".to_string(),
            dest_station_name: "Amarr VIII-4".to_string(),
            dest_security: 0.9,
            quantity: 1000,
            volume_used: 10.0,
            capital_required: 5000.0,
            gross_profit: 5000.0,
            taxes_and_fees: 500.0,
            net_profit: 4500.0,
            net_profit_pct: 35.0,
            jumps_from_home: 0,
            jumps_source_dest: 9,
            total_jumps: 9,
            profit_per_jump: 500.0,
            profit_per_m3: 450.0,
        };

        assert!(!deal.is_high_roi(), "35% should not be high ROI");

        deal.net_profit_pct = 40.0;
        assert!(deal.is_high_roi(), "40% should qualify as high ROI");

        deal.net_profit_pct = 85.5;
        assert!(deal.is_high_roi(), "85.5% should qualify as high ROI");
    }

    #[test]
    fn test_bundle_route_cargo_knapsack_manifest() {
        use eve_trade_scout::arbitrage::{bundle_route_cargo, ArbitrageDeal};
        use eve_trade_scout::config::BIG_HAULER;

        let make_deal =
            |id: u32, name: &str, vol: f64, cost: f64, profit: f64, ppm3: f64| ArbitrageDeal {
                type_id: id,
                item_name: name.to_string(),
                source_order_id: 100_000 + id as u64,
                dest_order_id: 200_000 + id as u64,
                source_order_volume_remain: 100,
                dest_order_volume_remain: 100,
                dest_order_min_volume: 1,
                is_npc_trade: false,
                buy_price: cost / 100.0,
                sell_price: (cost + profit) / 100.0,
                spread_pct: 50.0,
                source_station_id: 60003760,
                source_system_id: 30000142,
                source_system_name: "Jita".to_string(),
                source_station_name: "Jita IV-4".to_string(),
                source_security: 1.0,
                dest_station_id: 60008494,
                dest_system_id: 30002187,
                dest_system_name: "Amarr".to_string(),
                dest_station_name: "Amarr VIII-4".to_string(),
                dest_security: 0.9,
                quantity: 100,
                volume_used: vol,
                capital_required: cost,
                gross_profit: profit * 1.1,
                taxes_and_fees: profit * 0.1,
                net_profit: profit,
                net_profit_pct: (profit / cost) * 100.0,
                jumps_from_home: 0,
                jumps_source_dest: 9,
                total_jumps: 9,
                profit_per_jump: profit / 9.0,
                profit_per_m3: ppm3,
            };

        let deals = vec![
            make_deal(1, "Item A", 20_000.0, 100_000_000.0, 25_000_000.0, 1250.0),
            make_deal(2, "Item B", 25_000.0, 150_000_000.0, 40_000_000.0, 1600.0),
            make_deal(3, "Item C", 30_000.0, 200_000_000.0, 30_000_000.0, 1000.0), // Won't fit with A & B (exceeds 60k m³)
        ];

        let manifests = bundle_route_cargo(&deals, BIG_HAULER);
        assert!(
            !manifests.is_empty(),
            "Should generate at least one packed corridor manifest"
        );

        let m = &manifests[0];
        assert_eq!(m.source_system_name, "Jita");
        assert_eq!(m.dest_system_name, "Amarr");
        assert_eq!(
            m.deals.len(),
            2,
            "Should pack exactly Item B and Item A (highest profit density)"
        );
        assert_eq!(m.deals[0].item_name, "Item B"); // higher ppm3
        assert_eq!(m.deals[1].item_name, "Item A");
        assert_eq!(m.total_volume_used, 45_000.0);
        assert!(m.total_volume_used <= BIG_HAULER.max_volume);
        assert_eq!(m.total_capital_required, 250_000_000.0);
        assert!(m.total_capital_required <= BIG_HAULER.max_budget);
        assert_eq!(m.total_net_profit, 65_000_000.0);
    }

    #[test]
    fn test_calculate_deal_same_station_and_min_volume() {
        use eve_trade_scout::arbitrage::{calculate_deal, ArbitrageConfig};
        use eve_trade_scout::config::BIG_HAULER;
        use eve_trade_scout::esi::models::MarketOrder;
        use eve_trade_scout::graph::SystemGraph;
        use eve_trade_scout::sde::{ItemInfo, SdeLoader, SystemNode};

        let sde = SdeLoader::in_memory_with_fixtures().expect("Failed to initialize test SDE");

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
        let graph = SystemGraph::from_nodes_and_jumps(systems, jumps);

        let cfg = ArbitrageConfig {
            home_system: 30000142,
            ship: BIG_HAULER,
            min_profit: 0.0,
            min_spread: 5.0,
            min_net_profit_pct: 10.0,
            broker_fee: 1.0,
            sales_tax: 3.6,
            safe_only: true,
            npc_stations_only: false,
        };

        let item = ItemInfo {
            type_id: 34,
            name: "Tritanium".to_string(),
            packaged_volume: 0.01,
            group_id: 18,
            category_id: 4,
            published: true,
        };

        let src_order = MarketOrder {
            order_id: 101,
            type_id: 34,
            location_id: 60003760, // Jita IV-4
            system_id: 30000142,
            price: 5.0,
            volume_total: 10_000,
            volume_remain: 10_000,
            min_volume: 1,
            is_buy_order: false,
            duration: 90,
            issued: Some("2026-10-09T00:00:00Z".to_string()),
            range: Some("region".to_string()),
        };

        // 1. Same-station buy & sell: MUST be rejected immediately
        let same_station_dst = MarketOrder {
            order_id: 102,
            type_id: 34,
            location_id: 60003760, // Same station!
            system_id: 30000142,
            price: 8.0,
            volume_total: 10_000,
            volume_remain: 10_000,
            min_volume: 1,
            is_buy_order: true,
            duration: 90,
            issued: Some("2026-10-09T00:00:00Z".to_string()),
            range: Some("region".to_string()),
        };

        let same_station_res =
            calculate_deal(34, &item, &src_order, &same_station_dst, &cfg, &sde, &graph);
        assert!(
            same_station_res.is_none(),
            "Same station trade must be strictly rejected"
        );

        // 2. Cross-station trade where dest min_volume > available volume
        let unmet_min_vol_dst = MarketOrder {
            order_id: 103,
            type_id: 34,
            location_id: 60003761, // Different station in Perimeter
            system_id: 30000144,
            price: 8.0,
            volume_total: 50_000,
            volume_remain: 10_000,
            min_volume: 50_000, // Requires 50,000, but supply is only 10,000
            is_buy_order: true,
            duration: 90,
            issued: Some("2026-10-09T00:00:00Z".to_string()),
            range: Some("region".to_string()),
        };

        let unmet_min_res = calculate_deal(
            34,
            &item,
            &src_order,
            &unmet_min_vol_dst,
            &cfg,
            &sde,
            &graph,
        );
        assert!(
            unmet_min_res.is_none(),
            "Deal must be rejected when buyer min_volume cannot be fulfilled"
        );

        // 3. Cross-station trade where dest min_volume is satisfied (<= available volume)
        let satisfied_min_vol_dst = MarketOrder {
            order_id: 104,
            type_id: 34,
            location_id: 60003761,
            system_id: 30000144,
            price: 8.0,
            volume_total: 10_000,
            volume_remain: 10_000,
            min_volume: 5_000, // Requires 5,000, supply is 10,000
            is_buy_order: true,
            duration: 90,
            issued: Some("2026-10-09T00:00:00Z".to_string()),
            range: Some("region".to_string()),
        };

        let ok_res = calculate_deal(
            34,
            &item,
            &src_order,
            &satisfied_min_vol_dst,
            &cfg,
            &sde,
            &graph,
        );
        assert!(
            ok_res.is_some(),
            "Deal must be accepted when buyer min_volume is fulfilled"
        );
        let deal = ok_res.unwrap();
        assert_eq!(deal.dest_order_min_volume, 5_000);
        assert_eq!(deal.quantity, 10_000);
        assert!(
            !deal.is_npc_trade,
            "Regular player mineral deal must not be flagged as NPC trade"
        );
    }

    #[test]
    fn test_npc_trade_deal_detection_and_filtering() {
        use eve_trade_scout::arbitrage::{calculate_deal, ArbitrageConfig, ArbitrageDeal};
        use eve_trade_scout::config::BIG_HAULER;
        use eve_trade_scout::esi::models::MarketOrder;
        use eve_trade_scout::graph::SystemGraph;
        use eve_trade_scout::sde::{ItemInfo, SdeLoader, SystemNode};

        let sde = SdeLoader::in_memory_with_fixtures().expect("Failed to initialize test SDE");

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
        let graph = SystemGraph::from_nodes_and_jumps(systems, jumps);

        let cfg = ArbitrageConfig {
            home_system: 30000142,
            ship: BIG_HAULER,
            min_profit: 0.0,
            min_spread: 5.0,
            min_net_profit_pct: 10.0,
            broker_fee: 1.0,
            sales_tax: 3.6,
            safe_only: true,
            npc_stations_only: false,
        };

        // 1. Spirits (Type ID 9850, Group 280, Category 17: Commodity) -> NPC trade good
        let spirits_info = sde
            .get_item_info(9850)
            .expect("Spirits should exist in SDE");
        assert!(
            spirits_info.is_npc_trade_commodity(),
            "Spirits must be detected as an NPC trade commodity"
        );

        let src_spirits = MarketOrder {
            order_id: 501,
            type_id: 9850,
            location_id: 60003760,
            system_id: 30000142,
            price: 50.0,
            volume_total: 1000,
            volume_remain: 1000,
            min_volume: 1,
            is_buy_order: false,
            duration: 90,
            issued: Some("2026-10-09T00:00:00Z".to_string()),
            range: Some("region".to_string()),
        };

        let dst_spirits = MarketOrder {
            order_id: 502,
            type_id: 9850,
            location_id: 60003761,
            system_id: 30000144,
            price: 100.0,
            volume_total: 1000,
            volume_remain: 1000,
            min_volume: 1,
            is_buy_order: true,
            duration: 90,
            issued: Some("2026-10-09T00:00:00Z".to_string()),
            range: Some("region".to_string()),
        };

        let deal_spirits = calculate_deal(
            9850,
            &spirits_info,
            &src_spirits,
            &dst_spirits,
            &cfg,
            &sde,
            &graph,
        );
        assert!(deal_spirits.is_some());
        let deal_spirits = deal_spirits.unwrap();
        assert!(
            deal_spirits.is_npc_trade,
            "Spirits deal must be flagged as an NPC trade"
        );

        // 2. Regular item with a 365-day NPC seeded buy order (duration = 365)
        let tritanium_info = ItemInfo {
            type_id: 34,
            name: "Tritanium".to_string(),
            packaged_volume: 0.01,
            group_id: 18,
            category_id: 4,
            published: true,
        };

        let src_player = MarketOrder {
            order_id: 601,
            type_id: 34,
            location_id: 60003760,
            system_id: 30000142,
            price: 5.0,
            volume_total: 10_000,
            volume_remain: 10_000,
            min_volume: 1,
            is_buy_order: false,
            duration: 90,
            issued: Some("2026-10-09T00:00:00Z".to_string()),
            range: Some("region".to_string()),
        };

        let dst_npc_order = MarketOrder {
            order_id: 602,
            type_id: 34,
            location_id: 60003761,
            system_id: 30000144,
            price: 8.0,
            volume_total: 10_000,
            volume_remain: 10_000,
            min_volume: 1,
            is_buy_order: true,
            duration: 365, // NPC seeded order!
            issued: Some("2026-10-09T00:00:00Z".to_string()),
            range: Some("region".to_string()),
        };

        let deal_npc_seeded = calculate_deal(
            34,
            &tritanium_info,
            &src_player,
            &dst_npc_order,
            &cfg,
            &sde,
            &graph,
        );
        assert!(deal_npc_seeded.is_some());
        let deal_npc_seeded = deal_npc_seeded.unwrap();
        assert!(
            deal_npc_seeded.is_npc_trade,
            "Deal with 365-day NPC order must be flagged as an NPC trade"
        );

        // 3. Regular player-to-player deal (duration = 90, non-commodity item)
        let dst_player_order = MarketOrder {
            order_id: 603,
            type_id: 34,
            location_id: 60003761,
            system_id: 30000144,
            price: 8.0,
            volume_total: 10_000,
            volume_remain: 10_000,
            min_volume: 1,
            is_buy_order: true,
            duration: 90,
            issued: Some("2026-10-09T00:00:00Z".to_string()),
            range: Some("region".to_string()),
        };

        let deal_player = calculate_deal(
            34,
            &tritanium_info,
            &src_player,
            &dst_player_order,
            &cfg,
            &sde,
            &graph,
        );
        assert!(deal_player.is_some());
        let deal_player = deal_player.unwrap();
        assert!(
            !deal_player.is_npc_trade,
            "Player-to-player deal must not be flagged as NPC trade"
        );

        // 4. Test filtering: with hide_npc_trades = true, NPC deals are excluded while player deals remain
        let all_deals = [deal_spirits, deal_npc_seeded, deal_player];
        let hide_npc_trades = true;
        let filtered: Vec<&ArbitrageDeal> = all_deals
            .iter()
            .filter(|d| !hide_npc_trades || !d.is_npc_trade)
            .collect();

        assert_eq!(
            filtered.len(),
            1,
            "Only the 1 player deal should pass when hide_npc_trades is active"
        );
        assert_eq!(filtered[0].item_name, "Tritanium");
        assert!(!filtered[0].is_npc_trade);
    }
}
