use eve_trade_scout::config::{profile_from_str, UserConfig, BIG_HAULER, SMALL_HAULER};

#[test]
fn test_ship_profile_parsing_valid() {
    let big_cases = ["big", "BIG", "Big Hauler", "b", "  big  ", "big hauler"];
    for case in big_cases {
        let profile = profile_from_str(case).expect("Should parse big hauler profile");
        assert_eq!(profile.name, "Big Hauler");
        assert_eq!(profile.max_volume, 60_000.0);
        assert_eq!(profile.max_budget, 500_000_000.0);
    }

    let small_cases = [
        "small",
        "SMALL",
        "Small Hauler",
        "s",
        "  small  ",
        "small hauler",
    ];
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
    // Verify Small Hauler profile resolves correctly through UserConfig
    let mut small_cfg = cfg.clone();
    small_cfg.ship.profile = "small".to_string();
    let small_profile = small_cfg
        .ship_profile()
        .expect("Small profile should resolve");
    assert_eq!(small_profile.max_volume, SMALL_HAULER.max_volume);
    assert_eq!(small_profile.max_budget, SMALL_HAULER.max_budget);
    assert_eq!(small_profile.name, "Small Hauler");

    // Location defaults
    assert_eq!(cfg.location.home_system, "Jita");

    // Scan defaults
    assert!(
        cfg.scan.regions.contains(&10000002),
        "Default regions must include The Forge (Jita)"
    );
    assert!(
        cfg.scan.safe_route_only,
        "Safe routing must default to true"
    );
    assert!(
        cfg.scan.npc_stations_only,
        "NPC stations only must default to true"
    );
    assert!(!cfg.scan.auto_refresh, "Auto-refresh must default to false");
    assert!(cfg.scan.sound_alerts, "Sound alerts must default to true");
    assert_eq!(
        cfg.scan.sound_min_roi_pct, 100.0,
        "Sound alert threshold must default to 100.0%"
    );

    // Filter invariants: 10% minimum net ROI
    assert_eq!(
        cfg.filters.min_net_profit_pct, 10.0,
        "Enforced minimum ROI must default to 10.0%"
    );
    assert!(cfg.filters.min_profit_isk >= 1_000_000.0);
    assert!(cfg.filters.min_spread_pct >= 3.0);

    // Fees defaults
    assert_eq!(cfg.fees.broker_fee_pct, 3.0);
    assert_eq!(cfg.fees.sales_tax_pct, 8.0);

    // Theme default: Sci-Fi Void
    assert_eq!(cfg.theme, eve_trade_scout::config::AppTheme::SciFiVoid);
}

#[test]
fn test_user_config_toml_roundtrip() {
    let mut cfg = UserConfig::default();
    cfg.ship.profile = "small".to_string();
    cfg.location.home_system = "Amarr".to_string();
    cfg.scan.npc_stations_only = false;
    cfg.scan.auto_refresh = true;
    cfg.scan.sound_alerts = true;
    cfg.scan.sound_min_roi_pct = 150.0;
    cfg.filters.min_net_profit_pct = 20.0;
    cfg.theme = eve_trade_scout::config::AppTheme::NeutralBlack;

    let toml_str = toml::to_string(&cfg).expect("Serialization to TOML must succeed");
    let parsed: UserConfig =
        toml::from_str(&toml_str).expect("Deserialization from TOML must succeed");

    assert_eq!(parsed.ship.profile, "small");
    assert_eq!(parsed.location.home_system, "Amarr");
    assert!(!parsed.scan.npc_stations_only);
    assert!(parsed.scan.auto_refresh);
    assert!(parsed.scan.sound_alerts);
    assert_eq!(parsed.scan.sound_min_roi_pct, 150.0);
    assert_eq!(parsed.filters.min_net_profit_pct, 20.0);
    assert_eq!(parsed.theme, eve_trade_scout::config::AppTheme::NeutralBlack);
}

#[test]
fn test_theme_parsing_and_all_variants() {
    use eve_trade_scout::config::AppTheme;
    use std::str::FromStr;

    // Verify all 4 themes in ALL constant
    assert_eq!(AppTheme::ALL.len(), 4);
    assert!(AppTheme::ALL.contains(&AppTheme::SciFiVoid));
    assert!(AppTheme::ALL.contains(&AppTheme::NeutralBlack));
    assert!(AppTheme::ALL.contains(&AppTheme::BlackRed));
    assert!(AppTheme::ALL.contains(&AppTheme::SolarLight));

    // Test string parsing and aliases
    assert_eq!(AppTheme::from_str("sci_fi_void").unwrap(), AppTheme::SciFiVoid);
    assert_eq!(AppTheme::from_str("scifi").unwrap(), AppTheme::SciFiVoid);
    assert_eq!(AppTheme::from_str("default").unwrap(), AppTheme::SciFiVoid);
    assert_eq!(AppTheme::from_str("dark").unwrap(), AppTheme::SciFiVoid);

    assert_eq!(AppTheme::from_str("neutral_black").unwrap(), AppTheme::NeutralBlack);
    assert_eq!(AppTheme::from_str("neutral").unwrap(), AppTheme::NeutralBlack);
    assert_eq!(AppTheme::from_str("black").unwrap(), AppTheme::NeutralBlack);
    assert_eq!(AppTheme::from_str("oled").unwrap(), AppTheme::NeutralBlack);
    assert_eq!(AppTheme::from_str("stealth").unwrap(), AppTheme::NeutralBlack);

    assert_eq!(AppTheme::from_str("black_red").unwrap(), AppTheme::BlackRed);
    assert_eq!(AppTheme::from_str("crimson").unwrap(), AppTheme::BlackRed);
    assert_eq!(AppTheme::from_str("red").unwrap(), AppTheme::BlackRed);
    assert_eq!(AppTheme::from_str("blood_raider").unwrap(), AppTheme::BlackRed);
    assert_eq!(AppTheme::from_str("triglavian").unwrap(), AppTheme::BlackRed);

    assert_eq!(AppTheme::from_str("solar_light").unwrap(), AppTheme::SolarLight);
    assert_eq!(AppTheme::from_str("light").unwrap(), AppTheme::SolarLight);
    assert_eq!(AppTheme::from_str("daylight").unwrap(), AppTheme::SolarLight);
    assert_eq!(AppTheme::from_str("solar").unwrap(), AppTheme::SolarLight);
    assert_eq!(AppTheme::from_str("white").unwrap(), AppTheme::SolarLight);

    // Invalid theme string rejection
    assert!(AppTheme::from_str("neon_cyberpunk").is_err());
    assert!(AppTheme::from_str("").is_err());

    // Legacy TOML backwards compatibility: missing theme field defaults to SciFiVoid
    let legacy_toml = r#"
[ship]
profile = "big"
[location]
home_system = "Jita"
[scan]
regions = [10000002]
safe_route_only = true
npc_stations_only = true
scan_adjacent_regions = true
[filters]
min_profit_isk = 1000000.0
min_spread_pct = 3.0
min_net_profit_pct = 10.0
[fees]
broker_fee_pct = 3.0
sales_tax_pct = 8.0
[cache]
directory = "cache"
sde_max_age_days = 30
"#;
    let legacy_cfg: UserConfig = toml::from_str(legacy_toml).expect("Legacy TOML without theme must parse");
    assert_eq!(legacy_cfg.theme, AppTheme::SciFiVoid);

    // Roundtrip all 4 themes through TOML
    for &theme in AppTheme::ALL {
        let cfg = UserConfig {
            theme,
            ..UserConfig::default()
        };
        let serialized = toml::to_string(&cfg).expect("Serialization must succeed");
        let deserialized: UserConfig = toml::from_str(&serialized).expect("Deserialization must succeed");
        assert_eq!(deserialized.theme, theme);
    }
}

#[test]
fn test_user_config_toml_rejects_corrupted_syntax_and_types() {
    // 1. Broken TOML syntax
    let broken_syntax = "[ship\nprofile = 'big'";
    assert!(
        toml::from_str::<UserConfig>(broken_syntax).is_err(),
        "Must fail deserialization on syntax error"
    );

    // 2. Type mismatch (number instead of string for ship profile)
    let type_mismatch = "[ship]\nprofile = 12345";
    assert!(
        toml::from_str::<UserConfig>(type_mismatch).is_err(),
        "Must fail when profile is not a string"
    );

    // 3. Invalid boolean type
    let invalid_bool = "[scan]\nsafe_route_only = 'yes'";
    assert!(
        toml::from_str::<UserConfig>(invalid_bool).is_err(),
        "Must fail when boolean field is given as string"
    );
}

#[test]
fn test_config_base_dir_resolution_and_precedence() {
    use eve_trade_scout::config::config_base_dir;

    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let orig_eve = std::env::var_os("EVE_CONFIG_DIR");
    let orig_xdg = std::env::var_os("XDG_CONFIG_HOME");

    let temp_custom = std::env::temp_dir().join("eve_test_cfg_precedence_custom");
    let temp_xdg = std::env::temp_dir().join("eve_test_cfg_precedence_xdg");

    // Case 1: EVE_CONFIG_DIR takes absolute precedence over XDG_CONFIG_HOME
    std::env::set_var("EVE_CONFIG_DIR", &temp_custom);
    std::env::set_var("XDG_CONFIG_HOME", &temp_xdg);
    assert_eq!(
        config_base_dir(),
        Some(temp_custom.clone()),
        "EVE_CONFIG_DIR must take highest precedence"
    );

    // Case 2: XDG_CONFIG_HOME takes precedence when EVE_CONFIG_DIR is unset or empty
    std::env::remove_var("EVE_CONFIG_DIR");
    assert_eq!(
        config_base_dir(),
        Some(temp_xdg.clone()),
        "XDG_CONFIG_HOME must be used when EVE_CONFIG_DIR is unset"
    );

    std::env::set_var("EVE_CONFIG_DIR", "");
    assert_eq!(
        config_base_dir(),
        Some(temp_xdg.clone()),
        "Empty EVE_CONFIG_DIR must be ignored in favor of XDG_CONFIG_HOME"
    );

    // Case 3: Empty XDG_CONFIG_HOME falls back to dirs::config_dir()
    std::env::remove_var("EVE_CONFIG_DIR");
    std::env::remove_var("XDG_CONFIG_HOME");
    assert_eq!(
        config_base_dir(),
        dirs::config_dir(),
        "Must fall back to platform default dirs::config_dir() when no env overrides exist"
    );

    // Restore environment
    if let Some(ref val) = orig_eve {
        std::env::set_var("EVE_CONFIG_DIR", val);
    } else {
        std::env::remove_var("EVE_CONFIG_DIR");
    }
    if let Some(ref val) = orig_xdg {
        std::env::set_var("XDG_CONFIG_HOME", val);
    } else {
        std::env::remove_var("XDG_CONFIG_HOME");
    }
}

#[test]
fn test_config_example_template_is_valid() {
    let example_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.example.toml");
    let content = std::fs::read_to_string(&example_path).expect("config.example.toml must exist");
    let cfg: UserConfig = toml::from_str(&content).expect("config.example.toml must be valid TOML deserializing into UserConfig");
    assert_eq!(cfg.theme, eve_trade_scout::config::AppTheme::SciFiVoid);
    assert_eq!(cfg.ship.profile, "big");
    assert_eq!(cfg.location.home_system, "Jita");
    assert!(cfg.filters.min_net_profit_pct >= 10.0);
}
