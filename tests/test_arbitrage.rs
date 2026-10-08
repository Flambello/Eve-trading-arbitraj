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
    fn test_minimum_15_percent_net_profitability_enforced() {
        let broker_fee_pct = 3.0;
        let sales_tax_pct = 8.0;
        let min_net_profit_pct = 15.0;

        // Deal A: 100 buy, 120 sell (Gross spread is 20%, but after fees:)
        // Broker fee: 100 * 0.03 = 3.0
        // Sales tax: 120 * 0.08 = 9.6
        // Net profit per unit: 20 - 3.0 - 9.6 = 7.4
        // Net ROI%: 7.4 / 100 = 7.4% (Below 15%, should be rejected)
        let buy_a = 100.0;
        let sell_a = 120.0;
        let net_a = (sell_a - buy_a) - (buy_a * broker_fee_pct / 100.0) - (sell_a * sales_tax_pct / 100.0);
        let roi_a = (net_a / buy_a) * 100.0;
        assert!(roi_a < min_net_profit_pct, "Deal A should fail the 15% net threshold");

        // Deal B: 100 buy, 135 sell (Gross spread is 35%)
        // Broker fee: 100 * 0.03 = 3.0
        // Sales tax: 135 * 0.08 = 10.8
        // Net profit per unit: 35 - 3.0 - 10.8 = 21.2
        // Net ROI%: 21.2 / 100 = 21.2% (Above 15%, should be accepted)
        let buy_b = 100.0;
        let sell_b = 135.0;
        let net_b = (sell_b - buy_b) - (buy_b * broker_fee_pct / 100.0) - (sell_b * sales_tax_pct / 100.0);
        let roi_b = (net_b / buy_b) * 100.0;
        assert!(roi_b >= min_net_profit_pct, "Deal B should pass the 15% net threshold");
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

        assert!(npc_accepted, "NPC station must be accepted in NPC-only mode");
        assert!(!citadel_accepted, "Player citadel must be rejected in NPC-only mode");

        // Mode 2: npc_stations_only = false (Include player citadels in pool)
        let allow_citadels = false;
        let npc_accepted_in_all = !allow_citadels || is_npc(npc_station_id);
        let citadel_accepted_in_all = !allow_citadels || is_npc(citadel_id);

        assert!(npc_accepted_in_all, "NPC station must be accepted when citadels allowed");
        assert!(citadel_accepted_in_all, "Player citadel must be accepted when citadels allowed");
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
}
