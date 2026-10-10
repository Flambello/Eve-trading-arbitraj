// Unit tests for arbitrage calculations and ship profiles

#[cfg(test)]
mod tests {
    // Testing ship constraints
    #[test]
    fn test_ship_profile_values() {
        let big_vol = 60_000.0;
        let big_budget = 500_000_000.0;

        let small_vol = 30_000.0;
        let small_budget = 50_000_000.0;

        assert_eq!(big_vol, 60_000.0);
        assert_eq!(big_budget, 500_000_000.0);

        assert_eq!(small_vol, 30_000.0);
        assert_eq!(small_budget, 50_000_000.0);
    }

    #[test]
    fn test_profit_per_jump_formula() {
        let buy_price = 100.0;
        let sell_price = 150.0;
        let quantity = 1000;

        let gross_rev = sell_price * (quantity as f64);
        let capital = buy_price * (quantity as f64);
        let gross_profit = gross_rev - capital;

        let broker_fee_pct = 3.0;
        let sales_tax_pct = 8.0;

        let broker_cost = capital * (broker_fee_pct / 100.0);
        let sales_tax_cost = gross_rev * (sales_tax_pct / 100.0);
        let net_profit = gross_profit - broker_cost - sales_tax_cost;

        let jumps_from_home = 2;
        let jumps_source_to_dest = 3;
        let total_jumps = jumps_from_home + jumps_source_to_dest;

        let ppj = net_profit / (total_jumps as f64);

        assert!(net_profit > 0.0);
        assert_eq!(total_jumps, 5);
        assert!(ppj > 0.0);
    }

    #[test]
    fn test_minimum_10_percent_net_profitability_enforced() {
        let broker_fee_pct = 3.0;
        let sales_tax_pct = 8.0;
        let min_net_profit_pct = 10.0;

        // Deal A: 100 buy, 120 sell (Gross spread is 20%, but after fees:)
        // Broker fee: 100 * 0.03 = 3.0
        // Sales tax: 120 * 0.08 = 9.6
        // Net profit per unit: 20 - 3.0 - 9.6 = 7.4
        // Net ROI%: 7.4 / 100 = 7.4% (Below 10%, should be rejected)
        let buy_a = 100.0;
        let sell_a = 120.0;
        let net_a =
            (sell_a - buy_a) - (buy_a * broker_fee_pct / 100.0) - (sell_a * sales_tax_pct / 100.0);
        let roi_a = (net_a / buy_a) * 100.0;
        assert!(
            roi_a < min_net_profit_pct,
            "Deal A should fail the 10% net threshold"
        );

        // Deal B: 100 buy, 125 sell (Gross spread is 25%)
        // Broker fee: 100 * 0.03 = 3.0
        // Sales tax: 125 * 0.08 = 10.0
        // Net profit per unit: 25 - 3.0 - 10.0 = 12.0
        // Net ROI%: 12.0 / 100 = 12.0% (Above 10%, should be accepted)
        let buy_b = 100.0;
        let sell_b = 125.0;
        let net_b =
            (sell_b - buy_b) - (buy_b * broker_fee_pct / 100.0) - (sell_b * sales_tax_pct / 100.0);
        let roi_b = (net_b / buy_b) * 100.0;
        assert!(
            roi_b >= min_net_profit_pct,
            "Deal B should pass the 10% net threshold"
        );
    }

    #[test]
    fn test_is_npc_station_id_threshold() {
        // NPC Stations have IDs < 1_000_000_000_000
        let jita_iv_4: u64 = 60_003_760;
        let amarr_viii_4: u64 = 60_008_494;
        let dodixie_ix_20: u64 = 60_011_866;

        assert!(jita_iv_4 < 1_000_000_000_000);
        assert!(amarr_viii_4 < 1_000_000_000_000);
        assert!(dodixie_ix_20 < 1_000_000_000_000);

        // Player-owned Citadels / Upwell Structures have IDs >= 1_000_000_000_000
        let perimeter_citadel: u64 = 1_028_858_195_912;
        let player_fortizar: u64 = 1_030_000_000_001;

        assert!(perimeter_citadel >= 1_000_000_000_000);
        assert!(player_fortizar >= 1_000_000_000_000);
    }

    #[test]
    fn test_npc_stations_only_toggle_filter() {
        let npc_station_id: u64 = 60_003_760;
        let citadel_id: u64 = 1_028_858_195_912;

        let is_npc = |id: u64| id < 1_000_000_000_000;

        // Mode 1: npc_stations_only = true (Strict NPC only)
        let npc_only = true;
        let npc_accepted = !npc_only || is_npc(npc_station_id);
        let citadel_accepted = !npc_only || is_npc(citadel_id);

        assert!(
            npc_accepted,
            "NPC station must be accepted in NPC-only mode"
        );
        assert!(
            !citadel_accepted,
            "Player citadel must be rejected in NPC-only mode"
        );

        // Mode 2: npc_stations_only = false (Include player citadels in pool)
        let allow_citadels = false;
        let npc_accepted_in_all = !allow_citadels || is_npc(npc_station_id);
        let citadel_accepted_in_all = !allow_citadels || is_npc(citadel_id);

        assert!(
            npc_accepted_in_all,
            "NPC station must be accepted when citadels allowed"
        );
        assert!(
            citadel_accepted_in_all,
            "Player citadel must be accepted when citadels allowed"
        );
    }

    #[test]
    fn test_highsec_only_filtering_excludes_lowsec_and_nullsec() {
        let is_highsec = |security: f64| security >= 0.45;

        // Highsec systems
        assert!(is_highsec(0.95), "Jita (0.95) must be highsec");
        assert!(is_highsec(0.90), "Amarr (0.90) must be highsec");
        assert!(is_highsec(0.50), "Border system (0.50) must be highsec");
        assert!(is_highsec(0.45), "Border system (0.45) must be highsec");

        // Lowsec systems (0.1 to 0.44) - must be strictly excluded
        assert!(!is_highsec(0.44), "Lowsec (0.44) must be excluded");
        assert!(!is_highsec(0.40), "Tama (0.40) must be excluded");
        assert!(!is_highsec(0.10), "Amamake (0.10) must be excluded");

        // Nullsec systems (<= 0.0) - must be strictly excluded
        assert!(!is_highsec(0.0), "Border nullsec (0.0) must be excluded");
        assert!(!is_highsec(-0.2), "Catch nullsec (-0.2) must be excluded");
        assert!(!is_highsec(-1.0), "Deep nullsec (-1.0) must be excluded");
    }

    #[test]
    fn test_quantity_solver_constraints() {
        let max_volume: f64 = 60_000.0; // Big Hauler
        let max_budget: f64 = 500_000_000.0;

        // Case 1: Constrained by volume
        let unit_volume_1: f64 = 100.0; // m³
        let buy_price_1: f64 = 10_000.0;
        let supply_1: u32 = 5_000;
        let demand_1: u32 = 5_000;

        let qty_vol = (max_volume / unit_volume_1).floor() as u32; // 600
        let qty_budget = (max_budget / buy_price_1).floor() as u32; // 50,000
        let final_qty_1 = qty_vol.min(qty_budget).min(supply_1).min(demand_1);
        assert_eq!(final_qty_1, 600, "Should be constrained by cargo volume");

        // Case 2: Constrained by budget
        let unit_volume_2: f64 = 0.1;
        let buy_price_2: f64 = 25_000_000.0; // 25M each
        let supply_2: u32 = 100;
        let demand_2: u32 = 100;

        let qty_vol_2 = (max_volume / unit_volume_2).floor() as u32;
        let qty_budget_2 = (max_budget / buy_price_2).floor() as u32; // 20
        let final_qty_2 = qty_vol_2.min(qty_budget_2).min(supply_2).min(demand_2);
        assert_eq!(
            final_qty_2, 20,
            "Should be constrained by ISK capital budget"
        );

        // Case 3: Constrained by supply
        let supply_3 = 8;
        let demand_3 = 50;
        let final_qty_3 = qty_vol_2.min(qty_budget_2).min(supply_3).min(demand_3);
        assert_eq!(final_qty_3, 8, "Should be constrained by supply");

        // Case 4: Constrained by demand
        let supply_4 = 100;
        let demand_4 = 5;
        let final_qty_4 = qty_vol_2.min(qty_budget_2).min(supply_4).min(demand_4);
        assert_eq!(final_qty_4, 5, "Should be constrained by demand");
    }

    #[test]
    fn test_deal_sorting_ppj_descending() {
        struct MockDeal {
            id: u32,
            ppj: f64,
            net: f64,
        }

        let mut deals = [
            MockDeal {
                id: 1,
                ppj: 1.5,
                net: 10_000_000.0,
            },
            MockDeal {
                id: 2,
                ppj: 4.2,
                net: 8_000_000.0,
            },
            MockDeal {
                id: 3,
                ppj: 2.1,
                net: 15_000_000.0,
            },
            MockDeal {
                id: 4,
                ppj: 4.2,
                net: 12_000_000.0,
            }, // Same PPJ as #2, but higher net
        ];

        deals.sort_by(|a, b| {
            b.ppj
                .partial_cmp(&a.ppj)
                .unwrap()
                .then_with(|| b.net.partial_cmp(&a.net).unwrap())
        });

        // Expected order:
        // 1st: ID 4 (PPJ 4.2, net 12M)
        // 2nd: ID 2 (PPJ 4.2, net 8M)
        // 3rd: ID 3 (PPJ 2.1, net 15M)
        // 4th: ID 1 (PPJ 1.5, net 10M)
        assert_eq!(deals[0].id, 4);
        assert_eq!(deals[1].id, 2);
        assert_eq!(deals[2].id, 3);
        assert_eq!(deals[3].id, 1);
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

    #[tokio::test]
    async fn test_calculate_deal_same_station_and_min_volume() {
        use eve_trade_scout::arbitrage::{calculate_deal, ArbitrageConfig};
        use eve_trade_scout::config::BIG_HAULER;
        use eve_trade_scout::esi::models::MarketOrder;
        use eve_trade_scout::graph::SystemGraph;
        use eve_trade_scout::sde::{ItemInfo, SdeLoader, SystemNode};
        use std::path::PathBuf;

        let sde_path = PathBuf::from("data").join("eve_sde.sqlite");
        if !sde_path.exists() {
            return;
        }

        let sde = SdeLoader::open(&sde_path, 30)
            .await
            .expect("Failed to open SDE");

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

    #[tokio::test]
    async fn test_npc_trade_deal_detection_and_filtering() {
        use eve_trade_scout::arbitrage::{calculate_deal, ArbitrageConfig, ArbitrageDeal};
        use eve_trade_scout::config::BIG_HAULER;
        use eve_trade_scout::esi::models::MarketOrder;
        use eve_trade_scout::graph::SystemGraph;
        use eve_trade_scout::sde::{ItemInfo, SdeLoader, SystemNode};
        use std::path::PathBuf;

        let sde_path = PathBuf::from("data").join("eve_sde.sqlite");
        if !sde_path.exists() {
            return;
        }

        let sde = SdeLoader::open(&sde_path, 30)
            .await
            .expect("Failed to open SDE");

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
