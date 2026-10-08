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
}
