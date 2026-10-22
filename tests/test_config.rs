use eve_trade_scout::config::{profile_from_str, UserConfig, BIG_HAULER, SMALL_HAULER};

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

    let _lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

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
    assert_eq!(cfg.ui.ui_scale, 1.0);
    assert!(!cfg.ui.is_fullscreen);
    assert_eq!(cfg.view.hub_source_hub, Some("Jita".to_string()));
    assert_eq!(cfg.view.hub_dest_hub, Some("Amarr".to_string()));
}

#[test]
fn test_ui_and_view_config_defaults() {
    let cfg = UserConfig::default();

    // UI defaults
    assert_eq!(cfg.ui.ui_scale, 1.0);
    assert!(!cfg.ui.is_fullscreen);
    assert!(cfg.ui.dossier_side_panel);
    assert!(cfg.ui.compact_kpi);
    assert_eq!(cfg.ui.max_route_stops, 5);

    // View defaults
    assert_eq!(cfg.view.deal_sort_by, "profit_per_jump");
    assert!(cfg.view.deal_sort_descending);
    assert_eq!(cfg.view.min_profit_filter_m, 0.0);
    assert_eq!(cfg.view.max_jumps_filter, 0);
    assert!(!cfg.view.hide_npc_trades);
    assert_eq!(cfg.view.max_deals_per_order, 2);

    assert_eq!(cfg.view.chain_max_legs, 4);
    assert_eq!(cfg.view.chain_max_dest_loads, 2);
    assert!(cfg.view.chain_include_npc);
    assert!(cfg.view.chain_detour_enabled);
    assert_eq!(cfg.view.chain_max_detour, 2);

    assert_eq!(cfg.view.hub_source_hub, Some("Jita".to_string()));
    assert_eq!(cfg.view.hub_dest_hub, Some("Amarr".to_string()));
    assert!(cfg.view.hub_include_npc);
    assert!(!cfg.view.hub_major_stations_only);
    assert_eq!(cfg.view.hub_min_profit_m, 0.5);
}

#[test]
fn test_custom_ship_profile_effective_resolution() {
    let mut cfg = UserConfig::default();

    // Standard preset active (use_custom_ship = false)
    assert!(!cfg.ship.use_custom_ship);
    let p_big = cfg.effective_ship_profile().expect("Big profile resolves");
    assert_eq!(p_big.name, "Big Hauler");
    assert_eq!(p_big.max_volume, 60_000.0);
    assert_eq!(p_big.max_budget, 500_000_000.0);

    cfg.ship.profile = "small".to_string();
    let p_small = cfg.effective_ship_profile().expect("Small profile resolves");
    assert_eq!(p_small.name, "Small Hauler");
    assert_eq!(p_small.max_volume, 30_000.0);
    assert_eq!(p_small.max_budget, 50_000_000.0);

    // Custom profile active (use_custom_ship = true)
    cfg.ship.use_custom_ship = true;
    cfg.ship.custom_cargo_vol = 12_500.0;
    cfg.ship.custom_budget_isk = 300_000_000.0;
    let p_custom = cfg.effective_ship_profile().expect("Custom profile resolves");
    assert_eq!(p_custom.name, "Custom Vessel Profile");
    assert_eq!(p_custom.max_volume, 12_500.0);
    assert_eq!(p_custom.max_budget, 300_000_000.0);

    // Clamping / safety bounds: cargo hold < 10m³ or budget < 100k ISK clamped
    cfg.ship.custom_cargo_vol = -50.0;
    cfg.ship.custom_budget_isk = 10.0;
    let p_clamped = cfg.effective_ship_profile().expect("Clamped custom profile resolves");
    assert_eq!(p_clamped.max_volume, 10.0);
    assert_eq!(p_clamped.max_budget, 100_000.0);
}

#[test]
fn test_ui_and_view_config_roundtrip() {
    let mut cfg = UserConfig::default();
    cfg.ui.ui_scale = 1.15;
    cfg.ui.is_fullscreen = true;
    cfg.ui.dossier_side_panel = false;
    cfg.ui.compact_kpi = false;
    cfg.ui.max_route_stops = 10;

    cfg.view.deal_sort_by = "roi_pct".to_string();
    cfg.view.deal_sort_descending = false;
    cfg.view.min_profit_filter_m = 1.5;
    cfg.view.max_jumps_filter = 12;
    cfg.view.hide_npc_trades = true;
    cfg.view.max_deals_per_order = 5;
    cfg.view.chain_max_legs = 6;
    cfg.view.chain_max_dest_loads = 3;
    cfg.view.chain_include_npc = false;
    cfg.view.chain_detour_enabled = false;
    cfg.view.chain_max_detour = 4;
    cfg.view.hub_source_hub = Some("Dodixie".to_string());
    cfg.view.hub_dest_hub = Some("Rens".to_string());
    cfg.view.hub_include_npc = false;
    cfg.view.hub_major_stations_only = true;
    cfg.view.hub_min_profit_m = 2.5;

    let toml_str = toml::to_string(&cfg).expect("Serialization must succeed");
    let parsed: UserConfig = toml::from_str(&toml_str).expect("Deserialization must succeed");

    assert_eq!(parsed.ui.ui_scale, 1.15);
    assert!(parsed.ui.is_fullscreen);
    assert!(!parsed.ui.dossier_side_panel);
    assert!(!parsed.ui.compact_kpi);
    assert_eq!(parsed.ui.max_route_stops, 10);

    assert_eq!(parsed.view.deal_sort_by, "roi_pct");
    assert!(!parsed.view.deal_sort_descending);
    assert_eq!(parsed.view.min_profit_filter_m, 1.5);
    assert_eq!(parsed.view.max_jumps_filter, 12);
    assert!(parsed.view.hide_npc_trades);
    assert_eq!(parsed.view.max_deals_per_order, 5);
    assert_eq!(parsed.view.chain_max_legs, 6);
    assert_eq!(parsed.view.chain_max_dest_loads, 3);
    assert!(!parsed.view.chain_include_npc);
    assert!(!parsed.view.chain_detour_enabled);
    assert_eq!(parsed.view.chain_max_detour, 4);
    assert_eq!(parsed.view.hub_source_hub, Some("Dodixie".to_string()));
    assert_eq!(parsed.view.hub_dest_hub, Some("Rens".to_string()));
    assert!(!parsed.view.hub_include_npc);
    assert!(parsed.view.hub_major_stations_only);
    assert_eq!(parsed.view.hub_min_profit_m, 2.5);
}

#[test]
fn test_legacy_toml_without_ui_and_view_sections() {
    let minimal_legacy = r#"
[ship]
profile = "small"
[location]
home_system = "Hek"
[scan]
regions = [10000042]
safe_route_only = true
npc_stations_only = false
scan_adjacent_regions = false
[filters]
min_profit_isk = 500000.0
min_spread_pct = 2.0
min_net_profit_pct = 10.0
[fees]
broker_fee_pct = 2.5
sales_tax_pct = 5.0
[cache]
directory = "cache"
sde_max_age_days = 30
"#;
    let parsed: UserConfig = toml::from_str(minimal_legacy).expect("Minimal legacy TOML must parse without error");

    assert_eq!(parsed.ship.profile, "small");
    assert!(!parsed.ship.use_custom_ship);
    assert_eq!(parsed.ship.custom_cargo_vol, 60_000.0);
    assert_eq!(parsed.ship.custom_budget_isk, 500_000_000.0);

    assert_eq!(parsed.location.home_system, "Hek");
    assert!(!parsed.scan.npc_stations_only);

    // [ui] and [view] default values populated
    assert_eq!(parsed.ui.ui_scale, 1.0);
    assert!(!parsed.ui.is_fullscreen);
    assert_eq!(parsed.view.deal_sort_by, "profit_per_jump");
    assert!(parsed.view.deal_sort_descending);
    assert_eq!(parsed.view.hub_source_hub, Some("Jita".to_string()));
    assert_eq!(parsed.view.hub_dest_hub, Some("Amarr".to_string()));
    assert_eq!(parsed.view.hub_min_profit_m, 0.5);
}

#[test]
fn test_user_config_real_disk_atomic_save_and_load() {
    let _lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let orig_eve = std::env::var_os("EVE_CONFIG_DIR");

    let temp_dir = std::env::temp_dir().join(format!("eve_test_disk_save_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::env::set_var("EVE_CONFIG_DIR", &temp_dir);

    let mut cfg = UserConfig::default();
    cfg.location.home_system = "Amarr".to_string();
    cfg.ship.profile = "small".to_string();
    cfg.ship.use_custom_ship = true;
    cfg.ship.custom_cargo_vol = 42_000.0;
    cfg.ship.custom_budget_isk = 350_000_000.0;
    cfg.scan.safe_route_only = false;
    cfg.scan.npc_stations_only = false;
    cfg.scan.auto_refresh = true;
    cfg.scan.sound_alerts = true;
    cfg.scan.sound_min_roi_pct = 75.0;
    cfg.filters.min_net_profit_pct = 18.5;
    cfg.filters.min_profit_isk = 2_000_000.0;
    cfg.fees.broker_fee_pct = 2.1;
    cfg.fees.sales_tax_pct = 4.8;
    cfg.theme = eve_trade_scout::config::AppTheme::BlackRed;
    cfg.ui.ui_scale = 1.20;
    cfg.ui.is_fullscreen = true;
    cfg.ui.dossier_side_panel = false;
    cfg.ui.compact_kpi = false;
    cfg.ui.max_route_stops = 7;
    cfg.view.deal_sort_by = "total_profit".to_string();
    cfg.view.deal_sort_descending = false;
    cfg.view.min_profit_filter_m = 3.0;
    cfg.view.max_jumps_filter = 15;
    cfg.view.hide_npc_trades = true;
    cfg.view.max_deals_per_order = 4;
    cfg.view.chain_max_legs = 5;
    cfg.view.chain_max_dest_loads = 3;
    cfg.view.chain_include_npc = false;
    cfg.view.chain_detour_enabled = false;
    cfg.view.chain_max_detour = 1;
    cfg.view.hub_source_hub = Some("Rens".to_string());
    cfg.view.hub_dest_hub = Some("Dodixie".to_string());
    cfg.view.hub_include_npc = false;
    cfg.view.hub_major_stations_only = true;
    cfg.view.hub_min_profit_m = 1.2;

    // Save directly to disk
    cfg.save().expect("Real disk atomic save must succeed");

    // Verify physical file was written on disk
    let expected_file = UserConfig::config_path().expect("config_path must resolve");
    assert!(expected_file.exists(), "config.toml must physically exist on disk");
    let metadata = std::fs::metadata(&expected_file).expect("File metadata must be accessible");
    assert!(metadata.len() > 100, "Saved config file must not be empty");

    // Verify no temporary .tmp file is left dangling
    let dangling_tmp = expected_file.with_extension("toml.tmp");
    assert!(!dangling_tmp.exists(), "Temporary write file must be atomically renamed and not left dangling");

    // Load back from disk using production UserConfig::load()
    let loaded = UserConfig::load().expect("UserConfig::load must successfully read written config");

    assert_eq!(loaded.location.home_system, "Amarr");
    assert_eq!(loaded.ship.profile, "small");
    assert!(loaded.ship.use_custom_ship);
    assert_eq!(loaded.ship.custom_cargo_vol, 42_000.0);
    assert_eq!(loaded.ship.custom_budget_isk, 350_000_000.0);
    assert!(!loaded.scan.safe_route_only);
    assert!(!loaded.scan.npc_stations_only);
    assert!(loaded.scan.auto_refresh);
    assert!(loaded.scan.sound_alerts);
    assert_eq!(loaded.scan.sound_min_roi_pct, 75.0);
    assert_eq!(loaded.filters.min_net_profit_pct, 18.5);
    assert_eq!(loaded.filters.min_profit_isk, 2_000_000.0);
    assert_eq!(loaded.fees.broker_fee_pct, 2.1);
    assert_eq!(loaded.fees.sales_tax_pct, 4.8);
    assert_eq!(loaded.theme, eve_trade_scout::config::AppTheme::BlackRed);
    assert_eq!(loaded.ui.ui_scale, 1.20);
    assert!(loaded.ui.is_fullscreen);
    assert!(!loaded.ui.dossier_side_panel);
    assert!(!loaded.ui.compact_kpi);
    assert_eq!(loaded.ui.max_route_stops, 7);
    assert_eq!(loaded.view.deal_sort_by, "total_profit");
    assert!(!loaded.view.deal_sort_descending);
    assert_eq!(loaded.view.min_profit_filter_m, 3.0);
    assert_eq!(loaded.view.max_jumps_filter, 15);
    assert!(loaded.view.hide_npc_trades);
    assert_eq!(loaded.view.max_deals_per_order, 4);
    assert_eq!(loaded.view.chain_max_legs, 5);
    assert_eq!(loaded.view.chain_max_dest_loads, 3);
    assert!(!loaded.view.chain_include_npc);
    assert!(!loaded.view.chain_detour_enabled);
    assert_eq!(loaded.view.chain_max_detour, 1);
    assert_eq!(loaded.view.hub_source_hub, Some("Rens".to_string()));
    assert_eq!(loaded.view.hub_dest_hub, Some("Dodixie".to_string()));
    assert!(!loaded.view.hub_include_npc);
    assert!(loaded.view.hub_major_stations_only);
    assert_eq!(loaded.view.hub_min_profit_m, 1.2);

    // Cleanup and restore environment
    let _ = std::fs::remove_dir_all(&temp_dir);
    if let Some(ref val) = orig_eve {
        std::env::set_var("EVE_CONFIG_DIR", val);
    } else {
        std::env::remove_var("EVE_CONFIG_DIR");
    }
}

#[cfg(unix)]
#[test]
fn test_user_config_file_permissions_unix() {
    use std::os::unix::fs::PermissionsExt;

    let _lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let orig_eve = std::env::var_os("EVE_CONFIG_DIR");

    let temp_dir = std::env::temp_dir().join(format!("eve_test_perms_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::env::set_var("EVE_CONFIG_DIR", &temp_dir);

    let cfg = UserConfig::default();
    cfg.save().expect("Save must succeed");

    let config_path = UserConfig::config_path().expect("config_path must resolve");
    let metadata = std::fs::metadata(&config_path).expect("File metadata must exist");
    let mode = metadata.permissions().mode();
    // Verify standard 0600 mode (-rw-------)
    assert_eq!(
        mode & 0o777,
        0o600,
        "User configuration file must have restricted 0600 permissions to protect pilot data"
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
    if let Some(ref val) = orig_eve {
        std::env::set_var("EVE_CONFIG_DIR", val);
    } else {
        std::env::remove_var("EVE_CONFIG_DIR");
    }
}

#[test]
fn test_user_config_nan_and_infinite_numeric_sanitization() {
    let _lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let orig_eve = std::env::var_os("EVE_CONFIG_DIR");

    let temp_dir = std::env::temp_dir().join(format!("eve_test_nan_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::env::set_var("EVE_CONFIG_DIR", &temp_dir);

    // Write a corrupted configuration with non-finite floats and hostile values
    let corrupted_toml = r#"
[ship]
profile = "big"
use_custom_ship = true
custom_cargo_vol = nan
custom_budget_isk = -inf

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
min_net_profit_pct = nan

[fees]
broker_fee_pct = inf
sales_tax_pct = nan

[cache]
directory = "cache"
sde_max_age_days = 30

[ui]
ui_scale = nan
max_route_stops = 999999

[view]
deal_sort_by = "profit_per_jump"
min_profit_filter_m = -nan
chain_max_legs = 88888
chain_max_dest_loads = 55555
chain_max_detour = 9999
hub_min_profit_m = inf
"#;

    let config_path = UserConfig::config_path().expect("config_path must resolve");
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&config_path, corrupted_toml).unwrap();

    let loaded = UserConfig::load().expect("Corrupted floats must be safely sanitized without error");

    // Verify 10% ROI floor is maintained despite NaN
    assert!(loaded.filters.min_net_profit_pct.is_finite());
    assert_eq!(loaded.filters.min_net_profit_pct, 10.0, "NaN ROI must be sanitized to 10.0%");

    // Verify UI scale is reset to 1.0 rather than propagating NaN
    assert!(loaded.ui.ui_scale.is_finite());
    assert_eq!(loaded.ui.ui_scale, 1.0, "NaN ui_scale must be sanitized to 1.0");

    // Verify fees are sanitized to canonical defaults
    assert!(loaded.fees.broker_fee_pct.is_finite());
    assert_eq!(loaded.fees.broker_fee_pct, 3.0);
    assert!(loaded.fees.sales_tax_pct.is_finite());
    assert_eq!(loaded.fees.sales_tax_pct, 8.0);

    // Verify custom vessel limits are sanitized
    assert!(loaded.ship.custom_cargo_vol.is_finite());
    assert_eq!(loaded.ship.custom_cargo_vol, 60_000.0);
    assert!(loaded.ship.custom_budget_isk.is_finite());
    assert_eq!(loaded.ship.custom_budget_isk, 500_000_000.0);

    // Verify unbounded collection/hop limits are clamped
    assert_eq!(loaded.ui.max_route_stops, 20, "max_route_stops must be clamped to 20");
    assert_eq!(loaded.view.chain_max_legs, 10, "chain_max_legs must be clamped to 10");
    assert_eq!(loaded.view.chain_max_dest_loads, 10, "chain_max_dest_loads must be clamped to 10");
    assert_eq!(loaded.view.chain_max_detour, 10, "chain_max_detour must be clamped to 10");
    assert_eq!(loaded.view.min_profit_filter_m, 0.0);
    assert_eq!(loaded.view.hub_min_profit_m, 0.5);

    let _ = std::fs::remove_dir_all(&temp_dir);
    if let Some(ref val) = orig_eve {
        std::env::set_var("EVE_CONFIG_DIR", val);
    } else {
        std::env::remove_var("EVE_CONFIG_DIR");
    }
}

#[test]
fn test_user_config_save_failure_on_unwritable_destination() {
    let _lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let orig_eve = std::env::var_os("EVE_CONFIG_DIR");

    let temp_dir = std::env::temp_dir().join(format!("eve_test_unwritable_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    // Create a regular file where the config directory should be, making create_dir_all fail
    let blocker_file = temp_dir.join("blocked_dir");
    std::fs::write(&blocker_file, "blocking regular file").unwrap();

    let invalid_config_dir = blocker_file.join("sub_config");
    std::env::set_var("EVE_CONFIG_DIR", &invalid_config_dir);

    let cfg = UserConfig::default();
    let res = cfg.save();
    assert!(res.is_err(), "Saving to an invalid directory path must return an Err");

    let _ = std::fs::remove_dir_all(&temp_dir);
    if let Some(ref val) = orig_eve {
        std::env::set_var("EVE_CONFIG_DIR", val);
    } else {
        std::env::remove_var("EVE_CONFIG_DIR");
    }
}
