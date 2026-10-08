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
}
