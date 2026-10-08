use eve_trade_scout::config::{
    profile_from_str, UserConfig, BIG_HAULER, SMALL_HAULER,
};

#[test]
fn test_ship_profile_parsing_valid() {
    let big_cases = ["big", "BIG", "Big Hauler", "b", "  big  ", "big hauler"];
    for case in big_cases {
        let profile = profile_from_str(case).expect("Should parse big hauler profile");
        assert_eq!(profile.name, "Big Hauler");
        assert_eq!(profile.max_volume, 60_000.0);
        assert_eq!(profile.max_budget, 500_000_000.0);
    }

    let small_cases = ["small", "SMALL", "Small Hauler", "s", "  small  ", "small hauler"];
    for case in small_cases {
        let profile = profile_from_str(case).expect("Should parse small hauler profile");
        assert_eq!(profile.name, "Small Hauler");
        assert_eq!(profile.max_volume, 30_000.0);
        assert_eq!(profile.max_budget, 50_000_000.0);
    }
}

#[test]
fn test_ship_profile_parsing_invalid() {
    let invalid_cases = ["medium", "freighter", "titan", "", "unknown", "123"];
    for case in invalid_cases {
        assert!(
            profile_from_str(case).is_err(),
            "Expected error parsing invalid profile '{}'",
            case
        );
    }
}

#[test]
fn test_user_config_defaults_and_invariants() {
    let cfg = UserConfig::default();

    // Ship defaults
    assert_eq!(cfg.ship.profile, "big");
    let profile = cfg.ship_profile().expect("Default profile should resolve");
    assert_eq!(profile.max_volume, BIG_HAULER.max_volume);
    assert_eq!(profile.max_budget, BIG_HAULER.max_budget);
    assert_eq!(SMALL_HAULER.max_volume, 30_000.0);
    assert_eq!(SMALL_HAULER.max_budget, 50_000_000.0);

    // Location defaults
    assert_eq!(cfg.location.home_system, "Jita");

    // Scan defaults
    assert!(cfg.scan.regions.contains(&10000002), "Default regions must include The Forge (Jita)");
    assert!(cfg.scan.safe_route_only, "Safe routing must default to true");
    assert!(cfg.scan.npc_stations_only, "NPC stations only must default to true");

    // Filter invariants: 15% minimum net ROI
    assert_eq!(
        cfg.filters.min_net_profit_pct, 15.0,
        "Enforced minimum ROI must default to 15.0%"
    );
    assert!(cfg.filters.min_profit_isk >= 1_000_000.0);
    assert!(cfg.filters.min_spread_pct >= 3.0);

    // Fees defaults
    assert_eq!(cfg.fees.broker_fee_pct, 3.0);
    assert_eq!(cfg.fees.sales_tax_pct, 8.0);
}

#[test]
fn test_user_config_toml_roundtrip() {
    let mut cfg = UserConfig::default();
    cfg.ship.profile = "small".to_string();
    cfg.location.home_system = "Amarr".to_string();
    cfg.scan.npc_stations_only = false;
    cfg.filters.min_net_profit_pct = 20.0;

    let toml_str = toml::to_string(&cfg).expect("Serialization to TOML must succeed");
    let parsed: UserConfig = toml::from_str(&toml_str).expect("Deserialization from TOML must succeed");

    assert_eq!(parsed.ship.profile, "small");
    assert_eq!(parsed.location.home_system, "Amarr");
    assert_eq!(parsed.scan.npc_stations_only, false);
    assert_eq!(parsed.filters.min_net_profit_pct, 20.0);
}
