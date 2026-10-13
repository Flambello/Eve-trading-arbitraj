// Unit tests for arbitrage calculations and ship profiles

#[cfg(test)]
mod tests {
    use eve_trade_scout::arbitrage::{
        calculate_deal, filter_deals_by_npc_status, filter_deals_by_order_limit, is_npc_station,
        prune_redundant_order_deals, ArbitrageConfig, ArbitrageDeal,
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

        // Verify behavioral capacity constraints in calculate_deal
        let (sde, graph, mut cfg, mut item) = create_test_env();
        item.packaged_volume = 100.0; // 100 m3 per unit

        let src = MarketOrder {
            order_id: 1,
            type_id: 34,
            location_id: 60003760,
            system_id: 30000142,
            price: 1_000.0,
            volume_total: 10_000,
            volume_remain: 10_000,
            min_volume: 1,
            is_buy_order: false,
            duration: 90,
            issued: None,
            range: None,
        };
        let dst = MarketOrder {
            order_id: 2,
            type_id: 34,
            location_id: 60003761,
            system_id: 30000144,
            price: 2_000.0,
            volume_total: 10_000,
            volume_remain: 10_000,
            min_volume: 1,
            is_buy_order: true,
            duration: 90,
            issued: None,
            range: None,
        };

        // Big Hauler: 60,000 m3 / 100 m3 = 600 units max
        cfg.ship = BIG_HAULER;
        let big_deal = calculate_deal(34, &item, &src, &dst, &cfg, &sde, &graph)
            .expect("Big hauler deal should calculate");
        assert_eq!(big_deal.quantity, 600);
        assert_eq!(big_deal.volume_used, 60_000.0);
        assert!(big_deal.capital_required <= BIG_HAULER.max_budget);

        // Small Hauler: 30,000 m3 / 100 m3 = 300 units max
        cfg.ship = SMALL_HAULER;
        let small_deal = calculate_deal(34, &item, &src, &dst, &cfg, &sde, &graph)
            .expect("Small hauler deal should calculate");
        assert_eq!(small_deal.quantity, 300);
        assert_eq!(small_deal.volume_used, 30_000.0);
        assert!(small_deal.capital_required <= SMALL_HAULER.max_budget);
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

        // Exact 10.0% boundary tests (with zero fees for exact arithmetic precision):
        let mut zero_fee_cfg = cfg.clone();
        zero_fee_cfg.broker_fee = 0.0;
        zero_fee_cfg.sales_tax = 0.0;
        zero_fee_cfg.min_spread = 0.0;

        let src_boundary = MarketOrder {
            price: 100.0,
            ..src.clone()
        };

        // 9.99% ROI (109.99 sell price) -> must be strictly rejected
        let dst_9_99 = MarketOrder {
            price: 109.99,
            ..dst_sub10.clone()
        };
        assert!(
            calculate_deal(34, &item, &src_boundary, &dst_9_99, &zero_fee_cfg, &sde, &graph)
                .is_none(),
            "Deal with 9.99% net ROI must be rejected by 10.0% invariant"
        );

        // 10.00% ROI (110.00 sell price) -> must be accepted
        let dst_10_00 = MarketOrder {
            price: 110.00,
            ..dst_sub10.clone()
        };
        let deal_10_00 =
            calculate_deal(34, &item, &src_boundary, &dst_10_00, &zero_fee_cfg, &sde, &graph)
                .expect("Deal with exactly 10.00% net ROI must be accepted");
        assert!((deal_10_00.net_profit_pct - 10.0).abs() < 1e-4);

        // 10.01% ROI (110.01 sell price) -> must be accepted
        let dst_10_01 = MarketOrder {
            price: 110.01,
            ..dst_sub10.clone()
        };
        assert!(
            calculate_deal(34, &item, &src_boundary, &dst_10_01, &zero_fee_cfg, &sde, &graph)
                .is_some(),
            "Deal with 10.01% net ROI must be accepted"
        );
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

        // Exact boundary checks:
        assert!(
            is_npc_station(999_999_999_999),
            "999_999_999_999 must be classified as an NPC station"
        );
        assert!(
            !is_npc_station(1_000_000_000_000),
            "1_000_000_000_000 (Upwell Citadel boundary) must NOT be an NPC station"
        );
        assert!(
            !is_npc_station(0),
            "Station ID 0 must NOT be classified as an NPC station"
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

        // Exact boundary conditions:
        deal.net_profit_pct = 39.999;
        assert!(!deal.is_high_roi(), "39.999% must NOT qualify as high ROI");

        deal.net_profit_pct = 40.0;
        assert!(deal.is_high_roi(), "Exactly 40.0% must qualify as high ROI");

        deal.net_profit_pct = 40.001;
        assert!(deal.is_high_roi(), "40.001% must qualify as high ROI");

        deal.net_profit_pct = 85.5;
        assert!(deal.is_high_roi(), "85.5% should qualify as high ROI");

        // Negative, zero, and non-finite ROI edge cases:
        deal.net_profit_pct = -10.0;
        assert!(!deal.is_high_roi(), "Negative ROI must NOT qualify as high ROI");

        deal.net_profit_pct = 0.0;
        assert!(!deal.is_high_roi(), "0.0% ROI must NOT qualify as high ROI");

        deal.net_profit_pct = f64::NAN;
        assert!(!deal.is_high_roi(), "NaN ROI must NOT qualify as high ROI");
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

        // 1. Spirits (Type ID 9850, Group 280, Category 17: Commodity) -> Genuine NPC trade good commodity
        let spirits_info = sde
            .get_item_info(9850)
            .expect("Spirits should exist in SDE");
        assert!(
            spirits_info.is_npc_trade_commodity(),
            "Spirits must be detected as an NPC trade commodity"
        );

        // Verify that items in Category 17 that are NOT trade goods (insignias, tags, datacores)
        // and Category 43 (PI) are NOT flagged as NPC trade commodities
        let insignia_info = sde
            .get_item_info(15587)
            .expect("Federation Navy Midshipman Insignia I must exist in SDE fixtures");
        assert_eq!(insignia_info.group_id, 409);
        assert_eq!(insignia_info.category_id, 17);
        assert!(
            !insignia_info.is_npc_trade_commodity(),
            "Insignias must NOT be flagged as NPC trade commodities"
        );

        let tag_info = sde
            .get_item_info(12304)
            .expect("Angel Copper Tag must exist in SDE fixtures");
        assert_eq!(tag_info.group_id, 370);
        assert_eq!(tag_info.category_id, 17);
        assert!(
            !tag_info.is_npc_trade_commodity(),
            "Pirate criminal tags must NOT be flagged as NPC trade commodities"
        );

        let pi_info = sde
            .get_item_info(9832)
            .expect("Coolant must exist in SDE fixtures");
        assert_eq!(pi_info.category_id, 43);
        assert!(
            !pi_info.is_npc_trade_commodity(),
            "Planetary interaction commodities must NOT be flagged as NPC trade commodities"
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
            "Spirits deal must be flagged as an NPC trade (trade good commodity)"
        );

        // 2. Insignia player-to-player trade (both orders placed by players with duration = 90)
        let insignia_info = ItemInfo {
            type_id: 15587,
            name: "Federation Navy Midshipman Insignia I".to_string(),
            packaged_volume: 0.1,
            group_id: 409,
            category_id: 17,
            published: true,
        };

        let src_insignia_player = MarketOrder {
            order_id: 510,
            type_id: 15587,
            location_id: 60003760,
            system_id: 30000142,
            price: 1000.0,
            volume_total: 100,
            volume_remain: 100,
            min_volume: 1,
            is_buy_order: false,
            duration: 90,
            issued: Some("2026-10-09T00:00:00Z".to_string()),
            range: Some("region".to_string()),
        };

        let dst_insignia_player = MarketOrder {
            order_id: 511,
            type_id: 15587,
            location_id: 60003761,
            system_id: 30000144,
            price: 5000.0,
            volume_total: 100,
            volume_remain: 100,
            min_volume: 1,
            is_buy_order: true,
            duration: 90,
            issued: Some("2026-10-09T00:00:00Z".to_string()),
            range: Some("region".to_string()),
        };

        let deal_insignia_player = calculate_deal(
            15587,
            &insignia_info,
            &src_insignia_player,
            &dst_insignia_player,
            &cfg,
            &sde,
            &graph,
        );
        assert!(deal_insignia_player.is_some());
        let deal_insignia_player = deal_insignia_player.unwrap();
        assert!(
            !deal_insignia_player.is_npc_trade,
            "Player-to-player insignia deal must NOT be flagged as an NPC trade"
        );

        // 3. Insignia deal selling into an NPC seeded buy order (duration = 365)
        let dst_insignia_npc_order = MarketOrder {
            order_id: 512,
            type_id: 15587,
            location_id: 60003761,
            system_id: 30000144,
            price: 8000.0,
            volume_total: 100,
            volume_remain: 100,
            min_volume: 1,
            is_buy_order: true,
            duration: 365, // NPC military station order
            issued: Some("2026-10-09T00:00:00Z".to_string()),
            range: Some("region".to_string()),
        };

        let deal_insignia_npc = calculate_deal(
            15587,
            &insignia_info,
            &src_insignia_player,
            &dst_insignia_npc_order,
            &cfg,
            &sde,
            &graph,
        );
        assert!(deal_insignia_npc.is_some());
        let deal_insignia_npc = deal_insignia_npc.unwrap();
        assert!(
            deal_insignia_npc.is_npc_trade,
            "Deal selling into 365-day NPC order must be flagged as NPC trade"
        );

        // 4. Regular mineral item with a 365-day NPC seeded buy order (duration = 365)
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

        // 5. Regular player-to-player mineral deal (duration = 90)
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

        // 6. Test filtering: with hide_npc_trades = true, NPC deals (spirits, 365-day orders)
        // are excluded, while genuine player deals (insignias, minerals) pass through!
        let all_deals = [
            deal_spirits,
            deal_insignia_npc,
            deal_insignia_player,
            deal_npc_seeded,
            deal_player,
        ];
        // Call production filter function:
        let filtered = filter_deals_by_npc_status(&all_deals, false);

        assert_eq!(
            filtered.len(),
            2,
            "Both the player insignia deal and the player mineral deal must pass when hide_npc_trades is active"
        );
        assert_eq!(
            filtered[0].item_name,
            "Federation Navy Midshipman Insignia I"
        );
        assert!(!filtered[0].is_npc_trade);
        assert_eq!(filtered[1].item_name, "Tritanium");
        assert!(!filtered[1].is_npc_trade);
    }

    #[test]
    fn test_filter_top_deals_per_sell_order_prunes_excess_destinations() {
        // Create 1 sell order (ID 1001) mapped to 5 buy orders (IDs 2001..2005)
        // with decreasing profit-per-jump (and increasing total jumps).
        let make_deal = |dst_order_id: u64, dst_station_id: u64, ppj: f64, jumps: u32, net: f64| {
            ArbitrageDeal {
                type_id: 34,
                item_name: "Tritanium".to_string(),
                source_order_id: 1001,
                dest_order_id: dst_order_id,
                source_order_volume_remain: 10000,
                dest_order_volume_remain: 10000,
                dest_order_min_volume: 1,
                is_npc_trade: false,
                buy_price: 5.0,
                sell_price: 10.0,
                spread_pct: 100.0,
                source_station_id: 60003760,
                source_system_id: 30000142,
                source_system_name: "Jita".to_string(),
                source_station_name: "Jita IV-4".to_string(),
                source_security: 0.95,
                dest_station_id: dst_station_id,
                dest_system_id: 30000144,
                dest_system_name: "Perimeter".to_string(),
                dest_station_name: format!("Station {}", dst_station_id),
                dest_security: 0.90,
                quantity: 1000,
                volume_used: 10.0,
                capital_required: 5000.0,
                gross_profit: net * 1.2,
                taxes_and_fees: net * 0.2,
                net_profit: net,
                net_profit_pct: 50.0,
                jumps_from_home: 0,
                jumps_source_dest: jumps,
                total_jumps: jumps,
                profit_per_jump: ppj,
                profit_per_m3: net / 10.0,
            }
        };

        let deals = vec![
            make_deal(2001, 60000001, 5.0, 1, 50000.0), // Top 1: 1 jump, 5.0M/jump
            make_deal(2002, 60000002, 3.0, 2, 60000.0), // Top 2: 2 jumps, 3.0M/jump
            make_deal(2003, 60000003, 2.0, 4, 80000.0), // 4 jumps, 2.0M/jump (filler)
            make_deal(2004, 60000004, 1.0, 8, 80000.0), // 8 jumps, 1.0M/jump (filler)
            make_deal(2005, 60000005, 0.5, 12, 60000.0), // 12 jumps, 0.5M/jump (filler)
        ];

        // Pruning to top 2 destinations per sell order (unlimited buy side)
        let pruned = prune_redundant_order_deals(&deals, 2, 0);
        assert_eq!(
            pruned.len(),
            2,
            "Must keep strictly the top 2 destinations for sell order 1001"
        );
        assert_eq!(
            pruned[0].dest_order_id, 2001,
            "Top 1 destination must be 2001 (5.0 PPJ)"
        );
        assert_eq!(
            pruned[1].dest_order_id, 2002,
            "Top 2 destination must be 2002 (3.0 PPJ)"
        );
    }

    #[test]
    fn test_filter_top_deals_per_buy_order_prunes_excess_sources() {
        // Create 1 buy order (ID 2001) mapped from 5 sell orders (IDs 1001..1005)
        let make_deal = |src_order_id: u64, src_station_id: u64, ppj: f64, jumps: u32, net: f64| {
            ArbitrageDeal {
                type_id: 34,
                item_name: "Tritanium".to_string(),
                source_order_id: src_order_id,
                dest_order_id: 2001,
                source_order_volume_remain: 10000,
                dest_order_volume_remain: 10000,
                dest_order_min_volume: 1,
                is_npc_trade: false,
                buy_price: 5.0,
                sell_price: 10.0,
                spread_pct: 100.0,
                source_station_id: src_station_id,
                source_system_id: 30000142,
                source_system_name: "Jita".to_string(),
                source_station_name: format!("Station {}", src_station_id),
                source_security: 0.95,
                dest_station_id: 60003761,
                dest_system_id: 30000144,
                dest_system_name: "Perimeter".to_string(),
                dest_station_name: "Perimeter I".to_string(),
                dest_security: 0.90,
                quantity: 1000,
                volume_used: 10.0,
                capital_required: 5000.0,
                gross_profit: net * 1.2,
                taxes_and_fees: net * 0.2,
                net_profit: net,
                net_profit_pct: 50.0,
                jumps_from_home: 0,
                jumps_source_dest: jumps,
                total_jumps: jumps,
                profit_per_jump: ppj,
                profit_per_m3: net / 10.0,
            }
        };

        let deals = vec![
            make_deal(1001, 60000010, 8.0, 1, 80000.0), // Top 1: 1 jump, 8.0M/jump
            make_deal(1002, 60000020, 6.0, 2, 120000.0), // Top 2: 2 jumps, 6.0M/jump
            make_deal(1003, 60000030, 4.0, 3, 120000.0), // 3 jumps, 4.0M/jump (filler)
            make_deal(1004, 60000040, 2.0, 6, 120000.0), // 6 jumps, 2.0M/jump (filler)
            make_deal(1005, 60000050, 1.0, 10, 100000.0), // 10 jumps, 1.0M/jump (filler)
        ];

        // Pruning to top 2 sources per buy order (unlimited sell side)
        let pruned = prune_redundant_order_deals(&deals, 0, 2);
        assert_eq!(
            pruned.len(),
            2,
            "Must keep strictly the top 2 sources for buy order 2001"
        );
        assert_eq!(
            pruned[0].source_order_id, 1001,
            "Top 1 source must be 1001 (8.0 PPJ)"
        );
        assert_eq!(
            pruned[1].source_order_id, 1002,
            "Top 2 source must be 1002 (6.0 PPJ)"
        );
    }

    #[test]
    fn test_filter_top_deals_simultaneous_sell_and_buy_limits() {
        let make_deal = |src_id: u64, dst_id: u64, ppj: f64| ArbitrageDeal {
            type_id: 34,
            item_name: "Tritanium".to_string(),
            source_order_id: src_id,
            dest_order_id: dst_id,
            source_order_volume_remain: 1000,
            dest_order_volume_remain: 1000,
            dest_order_min_volume: 1,
            is_npc_trade: false,
            buy_price: 5.0,
            sell_price: 10.0,
            spread_pct: 100.0,
            source_station_id: src_id,
            source_system_id: 30000142,
            source_system_name: "Jita".to_string(),
            source_station_name: format!("Station {}", src_id),
            source_security: 0.95,
            dest_station_id: dst_id,
            dest_system_id: 30000144,
            dest_system_name: "Perimeter".to_string(),
            dest_station_name: format!("Station {}", dst_id),
            dest_security: 0.90,
            quantity: 100,
            volume_used: 1.0,
            capital_required: 500.0,
            gross_profit: 500.0,
            taxes_and_fees: 50.0,
            net_profit: 450.0,
            net_profit_pct: 90.0,
            jumps_from_home: 0,
            jumps_source_dest: 1,
            total_jumps: 1,
            profit_per_jump: ppj,
            profit_per_m3: 450.0,
        };

        // Sell order 101 mapped to B201, B202, B203
        // Sell order 102 mapped to B201, B202, B204
        let deals = vec![
            make_deal(101, 201, 10.0), // S101 -> B201 (S101: 1/2, B201: 1/2) -> kept
            make_deal(101, 202, 9.0),  // S101 -> B202 (S101: 2/2, B202: 1/2) -> kept
            make_deal(101, 203, 8.0),  // S101 -> B203 (S101: 3/2 -> rejected)
            make_deal(102, 201, 7.0),  // S102 -> B201 (S102: 1/2, B201: 2/2) -> kept
            make_deal(102, 202, 6.0),  // S102 -> B202 (S102: 2/2, B202: 2/2) -> kept
            make_deal(102, 204, 5.0),  // S102 -> B204 (S102: 3/2 -> rejected)
            make_deal(103, 201, 4.0),  // S103 -> B201 (S103: 1/2, B201: 3/2 -> rejected)
        ];

        let pruned = prune_redundant_order_deals(&deals, 2, 2);
        assert_eq!(
            pruned.len(),
            4,
            "Must enforce max 2 per sell and max 2 per buy"
        );
        assert_eq!(
            (pruned[0].source_order_id, pruned[0].dest_order_id),
            (101, 201)
        );
        assert_eq!(
            (pruned[1].source_order_id, pruned[1].dest_order_id),
            (101, 202)
        );
        assert_eq!(
            (pruned[2].source_order_id, pruned[2].dest_order_id),
            (102, 201)
        );
        assert_eq!(
            (pruned[3].source_order_id, pruned[3].dest_order_id),
            (102, 202)
        );
    }

    #[test]
    fn test_filter_top_deals_zero_limit_preserves_all_deals() {
        let make_deal = |id: u64| ArbitrageDeal {
            type_id: 34,
            item_name: "Tritanium".to_string(),
            source_order_id: 101,
            dest_order_id: 200 + id,
            source_order_volume_remain: 100,
            dest_order_volume_remain: 100,
            dest_order_min_volume: 1,
            is_npc_trade: false,
            buy_price: 5.0,
            sell_price: 10.0,
            spread_pct: 100.0,
            source_station_id: 101,
            source_system_id: 30000142,
            source_system_name: "Jita".to_string(),
            source_station_name: "Station".to_string(),
            source_security: 0.95,
            dest_station_id: 200 + id,
            dest_system_id: 30000144,
            dest_system_name: "Perimeter".to_string(),
            dest_station_name: "Station".to_string(),
            dest_security: 0.90,
            quantity: 100,
            volume_used: 1.0,
            capital_required: 500.0,
            gross_profit: 500.0,
            taxes_and_fees: 50.0,
            net_profit: 450.0,
            net_profit_pct: 90.0,
            jumps_from_home: 0,
            jumps_source_dest: 1,
            total_jumps: 1,
            profit_per_jump: 450.0,
            profit_per_m3: 450.0,
        };

        let deals = vec![make_deal(1), make_deal(2), make_deal(3), make_deal(4)];
        let unpruned = prune_redundant_order_deals(&deals, 0, 0);
        assert_eq!(
            unpruned.len(),
            4,
            "Zero limit must preserve 100% of candidate deals"
        );
    }

    #[test]
    fn test_filter_top_deals_preserves_fastest_highest_profit_routes() {
        let make_indexed_deal = |idx: usize, src_id: u64, dst_id: u64, ppj: f64| {
            (
                idx,
                ArbitrageDeal {
                    type_id: 34,
                    item_name: "Tritanium".to_string(),
                    source_order_id: src_id,
                    dest_order_id: dst_id,
                    source_order_volume_remain: 100,
                    dest_order_volume_remain: 100,
                    dest_order_min_volume: 1,
                    is_npc_trade: false,
                    buy_price: 5.0,
                    sell_price: 10.0,
                    spread_pct: 100.0,
                    source_station_id: src_id,
                    source_system_id: 30000142,
                    source_system_name: "Jita".to_string(),
                    source_station_name: "Station".to_string(),
                    source_security: 0.95,
                    dest_station_id: dst_id,
                    dest_system_id: 30000144,
                    dest_system_name: "Perimeter".to_string(),
                    dest_station_name: "Station".to_string(),
                    dest_security: 0.90,
                    quantity: 100,
                    volume_used: 1.0,
                    capital_required: 500.0,
                    gross_profit: 500.0,
                    taxes_and_fees: 50.0,
                    net_profit: 450.0,
                    net_profit_pct: 90.0,
                    jumps_from_home: 0,
                    jumps_source_dest: 1,
                    total_jumps: 1,
                    profit_per_jump: ppj,
                    profit_per_m3: 450.0,
                },
            )
        };

        let indexed_deals = vec![
            make_indexed_deal(10, 501, 601, 15.0),
            make_indexed_deal(11, 501, 602, 12.0),
            make_indexed_deal(12, 501, 603, 9.0), // 3rd destination for 501 -> rejected
            make_indexed_deal(13, 502, 601, 8.0),
        ];

        let filtered = filter_deals_by_order_limit(indexed_deals, 2, 2, |(_, d)| d);

        assert_eq!(filtered.len(), 3);
        assert_eq!(filtered[0].0, 10);
        assert_eq!(filtered[1].0, 11);
        assert_eq!(filtered[2].0, 13);
    }

    #[test]
    fn test_filter_deals_by_npc_status() {
        let make_deal_with_npc = |id: u64, is_npc: bool| ArbitrageDeal {
            type_id: 34,
            item_name: "Tritanium".to_string(),
            source_order_id: 1000 + id,
            dest_order_id: 2000 + id,
            source_order_volume_remain: 100,
            dest_order_volume_remain: 100,
            dest_order_min_volume: 1,
            is_npc_trade: is_npc,
            buy_price: 5.0,
            sell_price: 10.0,
            spread_pct: 100.0,
            source_station_id: 101,
            source_system_id: 30000142,
            source_system_name: "Jita".to_string(),
            source_station_name: "Station".to_string(),
            source_security: 0.95,
            dest_station_id: 201,
            dest_system_id: 30000144,
            dest_system_name: "Perimeter".to_string(),
            dest_station_name: "Station".to_string(),
            dest_security: 0.90,
            quantity: 100,
            volume_used: 1.0,
            capital_required: 500.0,
            gross_profit: 500.0,
            taxes_and_fees: 50.0,
            net_profit: 450.0,
            net_profit_pct: 90.0,
            jumps_from_home: 0,
            jumps_source_dest: 1,
            total_jumps: 1,
            profit_per_jump: 450.0,
            profit_per_m3: 450.0,
        };

        let deals = vec![
            make_deal_with_npc(1, false),
            make_deal_with_npc(2, true),
            make_deal_with_npc(3, false),
            make_deal_with_npc(4, true),
        ];

        // When include_npc is true, all deals must be retained
        let all_deals = filter_deals_by_npc_status(&deals, true);
        assert_eq!(all_deals.len(), 4);
        assert_eq!(all_deals.iter().map(|d| d.source_order_id).collect::<Vec<_>>(), vec![1001, 1002, 1003, 1004]);

        // When include_npc is false, NPC trades must be excluded
        let player_deals = filter_deals_by_npc_status(&deals, false);
        assert_eq!(player_deals.len(), 2);
        assert_eq!(player_deals.iter().map(|d| d.source_order_id).collect::<Vec<_>>(), vec![1001, 1003]);
        assert!(player_deals.iter().all(|d| !d.is_npc_trade));

        // Edge case: empty input slice
        let empty_deals: Vec<ArbitrageDeal> = Vec::new();
        assert!(filter_deals_by_npc_status(&empty_deals, true).is_empty());
        assert!(filter_deals_by_npc_status(&empty_deals, false).is_empty());
    }
}

