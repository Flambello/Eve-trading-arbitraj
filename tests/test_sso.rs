use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use eve_trade_scout::arbitrage::{ArbitrageDeal, TradeCorridorManifest};
use eve_trade_scout::chain_route_planner::{CargoSlot, ChainLeg, ChainRoute, PickupStop, SellStop};
use eve_trade_scout::esi::sso::{build_authorize_url, generate_pkce_pair, SsoToken};
use eve_trade_scout::hub_arbitrage::{self, CoreHub};
use eve_trade_scout::route_planner::{RouteStop, TradeRoute};
use ring::digest::{digest, SHA256};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn mock_deal(
    type_id: u32,
    name: &str,
    src_station: u64,
    dst_station: u64,
    src_sys: u32,
    dst_sys: u32,
) -> ArbitrageDeal {
    ArbitrageDeal {
        type_id,
        item_name: name.to_string(),
        source_order_id: 1001,
        dest_order_id: 2001,
        source_order_volume_remain: 100,
        dest_order_volume_remain: 100,
        dest_order_min_volume: 1,
        is_npc_trade: false,
        buy_price: 1_000.0,
        sell_price: 1_500.0,
        spread_pct: 50.0,
        source_station_id: src_station,
        source_system_id: src_sys,
        source_system_name: "Origin System".to_string(),
        source_station_name: "Origin Station".to_string(),
        source_security: 1.0,
        dest_station_id: dst_station,
        dest_system_id: dst_sys,
        dest_system_name: "Dest System".to_string(),
        dest_station_name: "Dest Station".to_string(),
        dest_security: 0.9,
        quantity: 50,
        volume_used: 50.0,
        capital_required: 50_000.0,
        gross_profit: 25_000.0,
        taxes_and_fees: 3_000.0,
        net_profit: 22_000.0,
        net_profit_pct: 44.0,
        jumps_from_home: 2,
        jumps_source_dest: 5,
        total_jumps: 7,
        profit_per_jump: 3_142.8,
        profit_per_m3: 440.0,
    }
}

#[test]
fn test_pkce_generation_and_challenge_derivation() {
    let (verifier, challenge) = generate_pkce_pair().expect("PKCE pair generation must succeed");

    // 48 random bytes in URL_SAFE_NO_PAD base64 produces exactly 64 characters
    assert_eq!(
        verifier.len(),
        64,
        "Verifier must be 64 base64url characters"
    );

    // Compute expected SHA256 digest and encode it base64url without padding
    let hash = digest(&SHA256, verifier.as_bytes());
    let expected_challenge = URL_SAFE_NO_PAD.encode(hash.as_ref());

    assert_eq!(
        challenge, expected_challenge,
        "Challenge must be SHA256 of verifier base64url encoded"
    );
}

#[test]
fn test_auth_url_construction() {
    let client_id = "test_client_id_12345";
    let challenge = "test_challenge_abcde";
    let state = "test_state_xyz";
    let port = 37373;

    let url = build_authorize_url(client_id, port, state, challenge);

    assert!(url.starts_with("https://login.eveonline.com/v2/oauth/authorize?"));
    assert!(url.contains("client_id=test_client_id_12345"));
    assert!(url.contains("code_challenge=test_challenge_abcde"));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A37373%2Fcallback"));
    assert!(url.contains("scope=esi-ui.write_waypoint.v1%20esi-location.read_location.v1%20esi-location.read_ship_type.v1"));
    assert!(url.contains("response_type=code"));
    assert!(url.contains("state=test_state_xyz"));
}

#[test]
fn test_sso_token_serde_and_expiration() {
    let now = now_secs();
    let token = SsoToken {
        access_token: "mock_jwt_access_token".to_string(),
        refresh_token: "mock_refresh_token_secret".to_string(),
        expires_at: now + 1200,
        token_type: "Bearer".to_string(),
        character_id: 2112345678,
        character_name: "Captain Trader".to_string(),
        client_id: Some("mock_client_id_123".to_string()),
    };

    let serialized = serde_json::to_string(&token).expect("Token serialization must succeed");
    let deserialized: SsoToken =
        serde_json::from_str(&serialized).expect("Token deserialization must succeed");

    assert_eq!(deserialized.character_name, "Captain Trader");
    assert_eq!(deserialized.character_id, 2112345678);
    assert_eq!(deserialized.access_token, "mock_jwt_access_token");
    assert_eq!(
        deserialized.client_id.as_deref(),
        Some("mock_client_id_123")
    );
    assert!(
        !deserialized.is_expired(),
        "Fresh token must not be expired"
    );
    assert!(
        !deserialized.needs_refresh(),
        "Token with 1200s TTL must not need refresh"
    );
    assert!(
        deserialized.seconds_until_expiration() > 1100,
        "Seconds until expiration should reflect remaining TTL"
    );

    // Token expiring within 5-minute proactive buffer (e.g. 250s left)
    let proactive_refresh_token = SsoToken {
        expires_at: now + 250,
        ..token.clone()
    };
    assert!(
        !proactive_refresh_token.is_expired(),
        "Token with 250s left is not yet expired"
    );
    assert!(
        proactive_refresh_token.needs_refresh(),
        "Token with 250s (< 300s) left must trigger proactive auto-refresh"
    );

    // Token with expiration in the past
    let expired_token = SsoToken {
        expires_at: now - 30,
        ..token.clone()
    };
    assert!(expired_token.is_expired(), "Past token must report expired");
    assert!(
        expired_token.needs_refresh(),
        "Expired token must need refresh"
    );
    assert_eq!(expired_token.seconds_until_expiration(), 0);

    // Token expiring within 60 second safety buffer
    let buffer_token = SsoToken {
        expires_at: now + 45,
        ..token
    };
    assert!(
        buffer_token.is_expired(),
        "Token expiring in 45s must trigger safety buffer refresh"
    );
    assert!(buffer_token.needs_refresh());
}

#[test]
fn test_deal_ordered_destination_ids_and_flight_sheet() {
    let deal = mock_deal(34, "Tritanium", 60003760, 60008494, 30000142, 30002187);

    let dests = deal.ordered_destination_ids();
    assert_eq!(dests, vec![60003760, 60008494]);

    // Same station deal
    let intra_deal = mock_deal(34, "Tritanium", 60003760, 60003760, 30000142, 30000142);
    assert_eq!(intra_deal.ordered_destination_ids(), vec![60003760]);

    let sheet = deal.format_eve_flight_sheet();
    assert!(sheet.contains("🚀 EVE TRADE FLIGHT SHEET: Tritanium"));
    assert!(sheet.contains("Origin Station"));
    assert!(sheet.contains("Dest Station"));
    assert!(sheet.contains("BUY AT:"));
    assert!(sheet.contains("SELL AT:"));
}

#[test]
fn test_trade_corridor_manifest_destination_ids_and_flight_sheet() {
    let d1 = mock_deal(34, "Tritanium", 60003760, 60008494, 30000142, 30002187);
    let d2 = mock_deal(35, "Pyerite", 60003760, 60008494, 30000142, 30002187);

    let manifest = TradeCorridorManifest {
        source_system_id: 30000142,
        source_system_name: "Jita".to_string(),
        source_station_name: "Jita IV - Moon 4".to_string(),
        source_security: 0.95,
        dest_system_id: 30002187,
        dest_system_name: "Amarr".to_string(),
        dest_station_name: "Amarr VIII - Moon 2".to_string(),
        dest_security: 1.0,
        deals: vec![d1, d2],
        total_volume_used: 100.0,
        total_capital_required: 100_000.0,
        total_net_profit: 44_000.0,
        overall_roi_pct: 44.0,
        jumps: 9,
        profit_per_jump: 4_888.8,
    };

    assert_eq!(manifest.ordered_destination_ids(), vec![60003760, 60008494]);

    let sheet = manifest.format_eve_flight_sheet(1);
    assert!(sheet.contains("📦 EVE CORRIDOR MANIFEST #1: Jita ➔ Amarr"));
    assert!(sheet.contains("Tritanium"));
    assert!(sheet.contains("Pyerite"));
    assert!(sheet.contains("BUY AT: Jita IV - Moon 4 (Jita)"));
    assert!(sheet.contains("SELL AT: Amarr VIII - Moon 2 (Amarr)"));
}

#[test]
fn test_chain_route_ordered_destination_ids_and_flight_sheet() {
    let deal1 = mock_deal(34, "Tritanium", 60003760, 60004588, 30000142, 30002672);
    let deal2 = mock_deal(35, "Pyerite", 60004588, 60008494, 30002672, 30002187);

    let slot1 = CargoSlot {
        deal: deal1,
        sell_system: 30002672,
        sell_name: "Dodixie".to_string(),
        volume: 50.0,
        capital: 50_000.0,
        net_profit: 22_000.0,
    };

    let slot2 = CargoSlot {
        deal: deal2,
        sell_system: 30002187,
        sell_name: "Amarr".to_string(),
        volume: 50.0,
        capital: 50_000.0,
        net_profit: 22_000.0,
    };

    let leg1 = ChainLeg {
        pickups: vec![PickupStop {
            system_id: 30000142,
            system_name: "Jita".to_string(),
            jumps_from_prev: 0,
            detour_jumps: 0,
            cargo_loaded: vec![slot1.clone()],
            volume_loaded: 50.0,
            capital_spent: 50_000.0,
        }],
        sell: SellStop {
            system_id: 30002672,
            system_name: "Dodixie".to_string(),
            jumps_from_prev: 15,
            cargo_sold: vec![slot1],
            total_profit: 22_000.0,
            volume_freed: 50.0,
        },
        leg_total_jumps: 15,
        leg_total_profit: 22_000.0,
    };

    let leg2 = ChainLeg {
        pickups: vec![PickupStop {
            system_id: 30002672,
            system_name: "Dodixie".to_string(),
            jumps_from_prev: 0,
            detour_jumps: 0,
            cargo_loaded: vec![slot2.clone()],
            volume_loaded: 50.0,
            capital_spent: 50_000.0,
        }],
        sell: SellStop {
            system_id: 30002187,
            system_name: "Amarr".to_string(),
            jumps_from_prev: 18,
            cargo_sold: vec![slot2],
            total_profit: 22_000.0,
            volume_freed: 50.0,
        },
        leg_total_jumps: 18,
        leg_total_profit: 22_000.0,
    };

    let route = ChainRoute {
        legs: vec![leg1, leg2],
        total_jumps: 33,
        total_profit: 44_000.0,
        total_capital: 100_000.0,
        peak_volume_m3: 50.0,
        profit_per_jump: 1_333.3,
        overall_roi_pct: 44.0,
        waypoints: vec![30000142, 30002672, 30002187],
        waypoint_names: vec![
            "Jita".to_string(),
            "Dodixie".to_string(),
            "Amarr".to_string(),
        ],
    };

    // The sequence of stations across both legs:
    // Leg 1 pickup: 60003760 (Jita)
    // Leg 1 sell:   60004588 (Dodixie)
    // Leg 2 pickup: 60004588 (Dodixie - deduplicated!)
    // Leg 2 sell:   60008494 (Amarr)
    let dests = route.ordered_destination_ids();
    assert_eq!(dests, vec![60003760, 60004588, 60008494]);

    let sheet = route.format_eve_flight_sheet(0);
    assert!(sheet.contains("🔗 CHAIN ROUTE #1"));
    assert!(sheet.contains("LEG #1"));
    assert!(sheet.contains("LEG #2"));
    assert!(sheet.contains("Autopilot trail: Jita ➔ Dodixie ➔ Amarr"));
}

#[test]
fn test_trade_route_ordered_destination_ids_and_flight_sheet() {
    let d1 = mock_deal(34, "Tritanium", 60003760, 60004588, 30000142, 30002672);
    let d2 = mock_deal(35, "Pyerite", 60004588, 60008494, 30002672, 30002187);

    let stop1 = RouteStop {
        step: 1,
        deal: d1,
        jumps_from_prev: 0,
        prev_system_id: 30000142,
        prev_system_name: "Jita".to_string(),
    };
    let stop2 = RouteStop {
        step: 2,
        deal: d2,
        jumps_from_prev: 15,
        prev_system_id: 30002672,
        prev_system_name: "Dodixie".to_string(),
    };

    let trade_route = TradeRoute {
        stops: vec![stop1, stop2],
        waypoints: vec![30000142, 30002672, 30002187],
        waypoint_names: vec![
            "Jita".to_string(),
            "Dodixie".to_string(),
            "Amarr".to_string(),
        ],
        total_jumps: 33,
        total_profit: 44_000.0,
        total_capital_required: 100_000.0,
        max_volume_used: 50.0,
        avg_profit_per_jump: 1_333.3,
    };

    // Sequential deduplication: [60003760, 60004588, 60008494]
    assert_eq!(
        trade_route.ordered_destination_ids(),
        vec![60003760, 60004588, 60008494]
    );

    let sheet = trade_route.format_eve_flight_sheet();
    assert!(sheet.contains("🗺️ MULTI-STOP ROUTE | 2 Stops"));
    assert!(sheet.contains("STOP #1"));
    assert!(sheet.contains("STOP #2"));
    assert!(sheet.contains("Autopilot trail: Jita ➔ Dodixie ➔ Amarr"));
}

#[test]
fn test_hub_corridor_destination_ids_and_flight_sheet() {
    let jita = CoreHub::Jita;
    let amarr = CoreHub::Amarr;

    let dests = hub_arbitrage::hub_corridor_destination_ids(jita, amarr, &None);
    assert_eq!(
        dests,
        vec![jita.primary_station_id(), amarr.primary_station_id()]
    );
    assert_eq!(dests, vec![60003760, 60008494]);

    // Test with manifest
    let deal = mock_deal(34, "Tritanium", 60003760, 60008494, 30000142, 30002187);
    let manifest = hub_arbitrage::pack_hub_corridor_manifest(&[&deal], 60_000.0, 500_000_000.0);

    let manifest_dests =
        hub_arbitrage::hub_corridor_destination_ids(jita, amarr, &Some(manifest.clone()));
    assert_eq!(manifest_dests, vec![60003760, 60008494]);

    let sheet = hub_arbitrage::format_hub_flight_sheet(jita, amarr, 9, &manifest);
    assert!(sheet.contains("🏛️ HUB CORRIDOR RUN: Jita ➔ Amarr (⚡ 9 Jumps)"));
    assert!(sheet.contains("BUY AT ORIGIN HUB: Jita"));
    assert!(sheet.contains("WARP 9 JUMPS & SELL AT DESTINATION HUB: Amarr"));
    assert!(sheet.contains("Autopilot route: Jita (60003760) ➔ Amarr (60008494)"));
}

#[test]
fn test_character_location_deserialization() {
    use eve_trade_scout::esi::sso::CharacterLocation;

    // Case 1: In space (no station or structure)
    let json_space = r#"{"solar_system_id": 30000142}"#;
    let loc_space: CharacterLocation = serde_json::from_str(json_space).unwrap();
    assert_eq!(loc_space.solar_system_id, 30000142);
    assert_eq!(loc_space.station_id, None);
    assert_eq!(loc_space.structure_id, None);

    // Case 2: Docked in NPC Station (e.g. Jita IV-4)
    let json_station = r#"{"solar_system_id": 30000142, "station_id": 60003760}"#;
    let loc_station: CharacterLocation = serde_json::from_str(json_station).unwrap();
    assert_eq!(loc_station.solar_system_id, 30000142);
    assert_eq!(loc_station.station_id, Some(60003760));
    assert_eq!(loc_station.structure_id, None);

    // Case 3: Tethered/docked in Player Citadel / Upwell Structure
    let json_structure = r#"{"solar_system_id": 30000144, "structure_id": 1028854491024}"#;
    let loc_structure: CharacterLocation = serde_json::from_str(json_structure).unwrap();
    assert_eq!(loc_structure.solar_system_id, 30000144);
    assert_eq!(loc_structure.station_id, None);
    assert_eq!(loc_structure.structure_id, Some(1028854491024));
}

#[test]
fn test_config_auto_sync_location_roundtrip() {
    use eve_trade_scout::config::UserConfig;

    let mut cfg = UserConfig::default();
    assert!(!cfg.sso.auto_sync_location);

    cfg.sso.auto_sync_location = true;
    cfg.sso.client_id = "test_client_id_xyz".to_string();

    let toml_str = toml::to_string(&cfg).unwrap();
    assert!(toml_str.contains("auto_sync_location = true"));

    let loaded: UserConfig = toml::from_str(&toml_str).unwrap();
    assert!(loaded.sso.auto_sync_location);
    assert_eq!(loaded.sso.client_id, "test_client_id_xyz");
}

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvGuard {
    var: &'static str,
    orig: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(var: &'static str, val: &std::path::Path) -> Self {
        let orig = std::env::var_os(var);
        std::env::set_var(var, val);
        Self { var, orig }
    }

    fn set_str(var: &'static str, val: &str) -> Self {
        let orig = std::env::var_os(var);
        std::env::set_var(var, val);
        Self { var, orig }
    }

    fn remove(var: &'static str) -> Self {
        let orig = std::env::var_os(var);
        std::env::remove_var(var);
        Self { var, orig }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(ref val) = self.orig {
            std::env::set_var(self.var, val);
        } else {
            std::env::remove_var(self.var);
        }
    }
}

struct ConfigDirGuard {
    _xdg: EnvGuard,
    _eve: EnvGuard,
}

impl ConfigDirGuard {
    fn set(path: &std::path::Path) -> Self {
        Self {
            _xdg: EnvGuard::set("XDG_CONFIG_HOME", path),
            _eve: EnvGuard::set("EVE_CONFIG_DIR", path),
        }
    }
}

#[test]
fn test_developer_client_id_resolution() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    use eve_trade_scout::esi::sso::{get_developer_client_id, resolve_client_id};

    // 1. Explicit custom ID resolves directly
    let custom = resolve_client_id("explicit_custom_key_123").unwrap();
    assert_eq!(custom, "explicit_custom_key_123");

    // Create an isolated temporary config directory to avoid coupling to the host system
    let temp_dir = std::env::temp_dir().join(format!("eve_test_sso_cfg_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).expect("Must create test temp dir");

    // Guard configuration directories so they restore on drop
    let _cfg_guard = ConfigDirGuard::set(&temp_dir);

    // 2. Test environment variable resolution (takes precedence over config file)
    {
        let _env_guard = EnvGuard::set_str("EVE_CLIENT_ID", "env_injected_client_id_456");
        let from_env = get_developer_client_id();
        assert_eq!(from_env.as_deref(), Some("env_injected_client_id_456"));

        let resolved_from_env = resolve_client_id("").unwrap();
        assert_eq!(resolved_from_env, "env_injected_client_id_456");
    }

    // Ensure EVE_CLIENT_ID is unset for the file resolution tests
    let _env_unset = EnvGuard::remove("EVE_CLIENT_ID");

    // 3. Test private config file resolution in isolated config dir (~/.config/eve-traveling-merchant/developer_client_id)
    let app_config_dir = temp_dir.join("eve-traveling-merchant");
    std::fs::create_dir_all(&app_config_dir).expect("Must create app config dir");
    let file_path = app_config_dir.join("developer_client_id");
    std::fs::write(&file_path, "mock_file_client_id_789\n")
        .expect("Must write mock client id file");

    let from_file = get_developer_client_id();
    assert_eq!(
        from_file.as_deref(),
        Some("mock_file_client_id_789"),
        "Must load client ID from isolated mock config file"
    );

    let resolved_from_file = resolve_client_id("").unwrap();
    assert_eq!(resolved_from_file, "mock_file_client_id_789");

    // 4. Test legacy path fallback (~/.config/eve-trade-scout/developer_client_id)
    std::fs::remove_file(&file_path).expect("Must remove primary config file");
    let legacy_config_dir = temp_dir.join("eve-trade-scout");
    std::fs::create_dir_all(&legacy_config_dir).expect("Must create legacy config dir");
    let legacy_file_path = legacy_config_dir.join("developer_client_id");
    std::fs::write(&legacy_file_path, "legacy_file_client_id_000\n")
        .expect("Must write legacy mock file");

    let from_legacy = get_developer_client_id();
    assert_eq!(
        from_legacy.as_deref(),
        Some("legacy_file_client_id_000"),
        "Must load client ID from legacy mock config file"
    );

    let resolved_from_legacy = resolve_client_id("").unwrap();
    assert_eq!(resolved_from_legacy, "legacy_file_client_id_000");

    // 5. Test unconfigured state when no credentials exist - falls back to DEFAULT_PUBLIC_CLIENT_ID
    std::fs::remove_file(&legacy_file_path).expect("Must remove legacy config file");

    let unconfigured = get_developer_client_id();
    assert!(
        unconfigured.is_none(),
        "Must return None when neither env var nor config file exists"
    );

    use eve_trade_scout::esi::sso::DEFAULT_PUBLIC_CLIENT_ID;
    let default_resolved = resolve_client_id("").unwrap();
    assert_eq!(
        default_resolved, DEFAULT_PUBLIC_CLIENT_ID,
        "Must fall back to DEFAULT_PUBLIC_CLIENT_ID when no override exists"
    );

    // 6. Test invalid custom client ID format is rejected
    let bad_err = resolve_client_id("invalid client id with spaces!");
    assert!(bad_err.is_err(), "Must reject invalid client ID format");
    let err_msg = bad_err.unwrap_err().to_string();
    assert!(
        err_msg.contains("Invalid EVE Application Client ID format"),
        "Error message should explain invalid client ID format: {}",
        err_msg
    );

    // Clean up temporary directory
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_load_saved_token_auto_backfills_missing_client_id() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    use eve_trade_scout::esi::sso::{load_saved_token, token_storage_path};

    let temp_dir =
        std::env::temp_dir().join(format!("eve_test_backfill_cfg_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).expect("Must create test temp dir");

    let _cfg_guard = ConfigDirGuard::set(&temp_dir);
    let _env_unset = EnvGuard::remove("EVE_CLIENT_ID");

    // Create developer_client_id file
    let app_config_dir = temp_dir.join("eve-traveling-merchant");
    std::fs::create_dir_all(&app_config_dir).expect("Must create app config dir");
    std::fs::write(
        app_config_dir.join("developer_client_id"),
        "auto_backfilled_client_id_888\n",
    )
    .expect("Must write dev client id");

    // Write a legacy token with missing client_id (null)
    let legacy_token_json = serde_json::json!({
        "access_token": "legacy_jwt",
        "refresh_token": "legacy_refresh",
        "token_type": "Bearer",
        "expires_at": now_secs() + 1000,
        "character_id": 99999,
        "character_name": "Legacy Pilot"
    });
    let token_file = token_storage_path();
    std::fs::write(
        &token_file,
        serde_json::to_string_pretty(&legacy_token_json).unwrap(),
    )
    .expect("Must write legacy token");

    // Calling load_saved_token should backfill client_id from get_developer_client_id()
    let loaded = load_saved_token().expect("Token must load successfully");
    assert_eq!(loaded.character_name, "Legacy Pilot");
    assert_eq!(
        loaded.client_id.as_deref(),
        Some("auto_backfilled_client_id_888"),
        "Loaded token must have client_id backfilled"
    );

    // Verify token on disk was also updated with the backfilled client_id
    let disk_raw = std::fs::read_to_string(&token_file).expect("Must read re-saved token file");
    let disk_token: SsoToken = serde_json::from_str(&disk_raw).expect("Must parse disk token");
    assert_eq!(
        disk_token.client_id.as_deref(),
        Some("auto_backfilled_client_id_888")
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(&token_file).unwrap().permissions();
        assert_eq!(
            perms.mode() & 0o777,
            0o600,
            "Re-saved token file must maintain 0600 permissions"
        );
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_ensure_fresh_token_unexpired_skips_network() {
    use eve_trade_scout::esi::sso::ensure_fresh_token;

    let client = reqwest::Client::new();
    let mut token = SsoToken {
        access_token: "active_token".to_string(),
        refresh_token: "refresh_secret".to_string(),
        expires_at: now_secs() + 1000,
        token_type: "Bearer".to_string(),
        character_id: 12345,
        character_name: "Pilot Alpha".to_string(),
        client_id: None,
    };

    let res = ensure_fresh_token(&client, "", &mut token).await;
    assert!(res.is_ok(), "Unexpired token must not trigger network call");
    assert_eq!(token.access_token, "active_token");
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_ensure_fresh_token_expired_invalid_client_id_returns_error() {
    use eve_trade_scout::esi::sso::ensure_fresh_token;

    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let temp_dir = std::env::temp_dir().join(format!("eve_test_token_err_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let _cfg_guard = ConfigDirGuard::set(&temp_dir);
    let _env_unset = EnvGuard::remove("EVE_CLIENT_ID");

    let client = reqwest::Client::new();
    let mut expired_token = SsoToken {
        access_token: "expired_access".to_string(),
        refresh_token: "refresh_secret".to_string(),
        expires_at: now_secs() - 60, // Expired
        token_type: "Bearer".to_string(),
        character_id: 12345,
        character_name: "Pilot Alpha".to_string(),
        client_id: None,
    };

    // Explicitly invalid client ID format must fail validation
    let res = ensure_fresh_token(
        &client,
        "invalid client ID with spaces!",
        &mut expired_token,
    )
    .await;
    assert!(
        res.is_err(),
        "Must fail when custom client ID format is invalid"
    );
    let err_msg = res.unwrap_err().to_string();
    assert!(err_msg.contains("Invalid EVE Application Client ID format"));

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_ensure_fresh_token_expired_invalid_refresh_fails() {
    use eve_trade_scout::esi::sso::ensure_fresh_token;

    let client = reqwest::Client::new();
    let mut expired_token = SsoToken {
        access_token: "expired_access".to_string(),
        refresh_token: "invalid_refresh_token_payload".to_string(),
        expires_at: now_secs() - 100, // Expired
        token_type: "Bearer".to_string(),
        character_id: 12345,
        character_name: "Pilot Alpha".to_string(),
        client_id: Some("mock_client_id".to_string()),
    };

    let res = ensure_fresh_token(&client, "mock_client_id", &mut expired_token).await;
    assert!(
        res.is_err(),
        "Refreshing expired token with invalid credentials must return Err"
    );
}

#[test]
fn test_token_response_deserialization_standard_ccp_refresh_omits_refresh_token() {
    use eve_trade_scout::esi::sso::TokenResponse;

    // Standard CCP EVE SSO v2 response when refreshing (refresh_token omitted by CCP server)
    let json_payload = r#"{
        "access_token": "renewed_access_token_12345",
        "token_type": "Bearer",
        "expires_in": 1199
    }"#;

    let token_resp: TokenResponse = serde_json::from_str(json_payload).expect(
        "CCP standard refresh response must deserialize cleanly even without refresh_token",
    );

    assert_eq!(token_resp.access_token, "renewed_access_token_12345");
    assert_eq!(token_resp.token_type, "Bearer");
    assert_eq!(token_resp.expires_in, 1199);
    assert_eq!(token_resp.refresh_token, None);
}

#[test]
fn test_token_response_deserialization_rotating_refresh_includes_refresh_token() {
    use eve_trade_scout::esi::sso::TokenResponse;

    // Future-proof CCP EVE SSO response with rotated refresh token
    let json_payload = r#"{
        "access_token": "renewed_access_token_67890",
        "token_type": "Bearer",
        "expires_in": 1200,
        "refresh_token": "rotated_refresh_secret_abcde"
    }"#;

    let token_resp: TokenResponse = serde_json::from_str(json_payload)
        .expect("Rotating refresh response must deserialize with new refresh_token");

    assert_eq!(token_resp.access_token, "renewed_access_token_67890");
    assert_eq!(token_resp.token_type, "Bearer");
    assert_eq!(token_resp.expires_in, 1200);
    assert_eq!(
        token_resp.refresh_token,
        Some("rotated_refresh_secret_abcde".to_string())
    );
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_ensure_fresh_token_at_url_preserves_refresh_token_on_standard_ccp_response() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let temp_dir = std::env::temp_dir().join(format!(
        "eve_test_refresh_std_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let _cfg_guard = ConfigDirGuard::set(&temp_dir);

    use eve_trade_scout::esi::sso::ensure_fresh_token_at_url;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Must bind test listener");
    let addr = listener.local_addr().expect("Must get local addr");
    let mock_url = format!("http://{}/v2/oauth/token", addr);

    let server_task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("Accept failed");
        let mut buf = vec![0u8; 2048];
        let n = socket.read(&mut buf).await.expect("Read failed");
        let request_str = String::from_utf8_lossy(&buf[..n]);

        // Verify request method and form payload
        assert!(request_str.starts_with("POST /v2/oauth/token"));
        assert!(request_str.contains("grant_type=refresh_token"));
        assert!(request_str.contains("refresh_token=my_secret_refresh_token_123"));
        assert!(request_str.contains("client_id=test_client_id_val"));

        // Respond with standard CCP refresh payload (omitting refresh_token)
        let body =
            r#"{"access_token":"brand_new_access_jwt","token_type":"Bearer","expires_in":1199}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("Write failed");
    });

    let client = reqwest::Client::new();
    let mut token = SsoToken {
        access_token: "old_expired_access".to_string(),
        refresh_token: "my_secret_refresh_token_123".to_string(),
        expires_at: now_secs() - 50, // Expired
        token_type: "Bearer".to_string(),
        character_id: 2112345678,
        character_name: "Captain Trader".to_string(),
        client_id: Some("test_client_id_val".to_string()),
    };

    let res = ensure_fresh_token_at_url(&client, &mock_url, "test_client_id_val", &mut token).await;
    assert!(
        res.is_ok(),
        "Token refresh against mock CCP endpoint must succeed: {:?}",
        res.err()
    );

    // Verify token was renewed and existing refresh token was preserved!
    assert_eq!(token.access_token, "brand_new_access_jwt");
    assert_eq!(token.refresh_token, "my_secret_refresh_token_123");
    assert!(token.expires_at > now_secs() + 1100);

    let _ = server_task.await;
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_ensure_fresh_token_at_url_rotates_refresh_token_when_present() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let temp_dir = std::env::temp_dir().join(format!(
        "eve_test_refresh_rot_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let _cfg_guard = ConfigDirGuard::set(&temp_dir);

    use eve_trade_scout::esi::sso::ensure_fresh_token_at_url;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Must bind test listener");
    let addr = listener.local_addr().expect("Must get local addr");
    let mock_url = format!("http://{}/v2/oauth/token", addr);

    let server_task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("Accept failed");
        let mut buf = vec![0u8; 2048];
        let n = socket.read(&mut buf).await.expect("Read failed");
        let request_str = String::from_utf8_lossy(&buf[..n]);

        assert!(request_str.starts_with("POST /v2/oauth/token"));
        assert!(request_str.contains("grant_type=refresh_token"));
        assert!(request_str.contains("refresh_token=old_refresh_secret"));

        // Respond with rotated refresh payload
        let body = r#"{"access_token":"rotated_access_jwt","token_type":"Bearer","expires_in":1200,"refresh_token":"new_rotated_refresh_secret"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("Write failed");
    });

    let client = reqwest::Client::new();
    let mut token = SsoToken {
        access_token: "old_access".to_string(),
        refresh_token: "old_refresh_secret".to_string(),
        expires_at: now_secs() - 10,
        token_type: "Bearer".to_string(),
        character_id: 2112345678,
        character_name: "Captain Trader".to_string(),
        client_id: Some("test_client_id_val".to_string()),
    };

    let res = ensure_fresh_token_at_url(&client, &mock_url, "test_client_id_val", &mut token).await;
    assert!(res.is_ok(), "Token refresh with rotation must succeed");

    assert_eq!(token.access_token, "rotated_access_jwt");
    assert_eq!(token.refresh_token, "new_rotated_refresh_secret");
    assert!(token.expires_at > now_secs() + 1100);

    let _ = server_task.await;
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_ensure_fresh_token_at_url_handles_invalid_grant_error() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let temp_dir = std::env::temp_dir().join(format!(
        "eve_test_refresh_rev_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let _cfg_guard = ConfigDirGuard::set(&temp_dir);

    use eve_trade_scout::esi::sso::ensure_fresh_token_at_url;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Must bind test listener");
    let addr = listener.local_addr().expect("Must get local addr");
    let mock_url = format!("http://{}/v2/oauth/token", addr);

    let server_task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("Accept failed");
        let mut buf = vec![0u8; 2048];
        let _ = socket.read(&mut buf).await;

        let body =
            r#"{"error":"invalid_grant","error_description":"Token has been revoked or expired"}"#;
        let response = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("Write failed");
    });

    let client = reqwest::Client::new();
    let mut token = SsoToken {
        access_token: "old_access".to_string(),
        refresh_token: "revoked_secret".to_string(),
        expires_at: now_secs() - 10,
        token_type: "Bearer".to_string(),
        character_id: 2112345678,
        character_name: "Captain Trader".to_string(),
        client_id: Some("test_client_id_val".to_string()),
    };

    let res = ensure_fresh_token_at_url(&client, &mock_url, "test_client_id_val", &mut token).await;
    assert!(res.is_err(), "Must return error on invalid_grant");
    let err_msg = res.unwrap_err().to_string();
    assert!(err_msg.contains("EVE SSO session has expired or was revoked"));

    let _ = server_task.await;
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_dry_run_pkce_auth_flow_with_public_client_id() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let temp_dir = std::env::temp_dir().join(format!(
        "eve_test_dry_run_pkce_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let _cfg_guard = ConfigDirGuard::set(&temp_dir);
    let _env_unset = EnvGuard::remove("EVE_CLIENT_ID");

    use eve_trade_scout::esi::sso::{
        exchange_code_for_token_at_url, listen_for_auth_code, DEFAULT_PUBLIC_CLIENT_ID,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // 1. Resolve client ID with zero configuration or overrides -> resolves public client ID
    let client_id =
        eve_trade_scout::esi::sso::resolve_client_id("").expect("Must resolve default public ID");
    assert_eq!(client_id, DEFAULT_PUBLIC_CLIENT_ID);

    // 2. Generate cryptographic PKCE verifier and S256 challenge
    let (code_verifier, code_challenge) = generate_pkce_pair().expect("Must generate PKCE pair");
    assert_eq!(code_verifier.len(), 64);

    // 3. Construct authorization URL
    let state = "test_dry_run_state_456";
    let auth_url = build_authorize_url(&client_id, 37373, state, &code_challenge);
    assert!(auth_url.contains(&format!("client_id={}", DEFAULT_PUBLIC_CLIENT_ID)));
    assert!(auth_url.contains(&format!("code_challenge={}", code_challenge)));
    assert!(auth_url.contains("code_challenge_method=S256"));
    assert!(auth_url.contains("response_type=code"));

    // 4. Bind ephemeral loopback listener and simulate browser callback
    let loopback_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Must bind ephemeral loopback listener");
    let loopback_port = loopback_listener.local_addr().unwrap().port();
    drop(loopback_listener); // Release so listen_for_auth_code can bind it

    let auth_listen_task = tokio::spawn(async move {
        listen_for_auth_code(
            loopback_port,
            "test_dry_run_state_456",
            std::time::Duration::from_secs(5),
        )
        .await
    });

    // Small delay to ensure listener is accepting
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Simulate browser redirect to localhost callback
    let mut browser_stream = tokio::net::TcpStream::connect(("127.0.0.1", loopback_port))
        .await
        .expect("Must connect to loopback callback listener");
    let callback_request = format!(
        "GET /callback?code=mock_dry_run_auth_code_999&state=test_dry_run_state_456 HTTP/1.1\r\nHost: localhost:{}\r\nConnection: close\r\n\r\n",
        loopback_port
    );
    browser_stream
        .write_all(callback_request.as_bytes())
        .await
        .expect("Write callback request failed");

    let mut browser_resp_buf = vec![0u8; 2048];
    let n = browser_stream
        .read(&mut browser_resp_buf)
        .await
        .expect("Read callback response failed");
    let browser_resp_str = String::from_utf8_lossy(&browser_resp_buf[..n]);
    assert!(
        browser_resp_str.starts_with("HTTP/1.1 200 OK"),
        "Callback listener must return 200 OK HTML"
    );
    assert!(browser_resp_str.contains("Character Connected!"));

    let received_auth_code = auth_listen_task
        .await
        .expect("Listener task panicked")
        .expect("Listener must successfully extract code");
    assert_eq!(received_auth_code, "mock_dry_run_auth_code_999");

    // 5. Spin up mock CCP token endpoint to verify token exchange payload
    let token_server_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Must bind mock token server");
    let token_server_addr = token_server_listener.local_addr().unwrap();
    let mock_token_url = format!("http://{}/v2/oauth/token", token_server_addr);

    let verifier_clone = code_verifier.clone();
    let token_server_task = tokio::spawn(async move {
        let (mut socket, _) = token_server_listener.accept().await.expect("Accept failed");
        let mut buf = vec![0u8; 4096];
        let n = socket.read(&mut buf).await.expect("Read failed");
        let request_str = String::from_utf8_lossy(&buf[..n]);

        // Verify that the request uses POST /v2/oauth/token and compliant User-Agent
        assert!(request_str.starts_with("POST /v2/oauth/token"));
        assert!(request_str.contains("user-agent: eve-trade-scout/0.1.0"));
        assert!(request_str.contains("https://github.com/Flambello/eve-traveling-merchant"));

        // Verify PKCE token exchange form parameters
        assert!(request_str.contains("grant_type=authorization_code"));
        assert!(request_str.contains(&format!("client_id={}", DEFAULT_PUBLIC_CLIENT_ID)));
        assert!(request_str.contains("code=mock_dry_run_auth_code_999"));
        assert!(request_str.contains(&format!("code_verifier={}", verifier_clone)));

        // In PKCE public client flow, NO client_secret must ever be present
        assert!(
            !request_str.contains("client_secret"),
            "Public client flow must never send client_secret"
        );

        // Generate mock valid JWT with character name and character ID
        let payload = serde_json::json!({
            "name": "Public PKCE Explorer",
            "sub": "CHARACTER:EVE:2119999999"
        });
        let header_b64 = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"RS256\",\"typ\":\"JWT\"}");
        let payload_b64 = URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
        let mock_jwt = format!("{}.{}.mock_signature_bytes", header_b64, payload_b64);

        let body = serde_json::json!({
            "access_token": mock_jwt,
            "token_type": "Bearer",
            "expires_in": 1200,
            "refresh_token": "mock_refresh_token_xyz"
        })
        .to_string();

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("Write token response failed");
    });

    let client = reqwest::Client::new();
    let token = exchange_code_for_token_at_url(
        &client,
        &mock_token_url,
        &client_id,
        &received_auth_code,
        &code_verifier,
        loopback_port,
    )
    .await
    .expect("Token exchange must succeed without client secret");

    assert_eq!(token.character_name, "Public PKCE Explorer");
    assert_eq!(token.character_id, 2119999999);
    assert_eq!(token.refresh_token, "mock_refresh_token_xyz");
    assert_eq!(
        token.client_id.as_deref(),
        Some(DEFAULT_PUBLIC_CLIENT_ID),
        "Saved token must retain the public Client ID"
    );
    assert!(
        !token.is_expired(),
        "Newly exchanged token with 1200s TTL must not be expired"
    );

    let _ = token_server_task.await;
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cross_platform_browser_command_syntax() {
    let test_url = "https://login.eveonline.com/v2/oauth/authorize?test=1&param=abc";

    // Validate Windows command builder syntax: rundll32 url.dll,FileProtocolHandler "<url>"
    let mut win_cmd = std::process::Command::new("rundll32");
    win_cmd.args(["url.dll,FileProtocolHandler", test_url]);
    let win_program = win_cmd.get_program().to_string_lossy();
    assert_eq!(win_program, "rundll32");
    let win_args: Vec<String> = win_cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    assert_eq!(win_args, vec!["url.dll,FileProtocolHandler", test_url]);

    // Validate Linux command builder syntax: xdg-open "<url>"
    let mut linux_cmd = std::process::Command::new("xdg-open");
    linux_cmd.arg(test_url);
    assert_eq!(linux_cmd.get_program().to_string_lossy(), "xdg-open");
    let linux_args: Vec<String> = linux_cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    assert_eq!(linux_args, vec![test_url]);

    // Validate macOS command builder syntax: open "<url>"
    let mut mac_cmd = std::process::Command::new("open");
    mac_cmd.arg(test_url);
    assert_eq!(mac_cmd.get_program().to_string_lossy(), "open");
    let mac_args: Vec<String> = mac_cmd
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    assert_eq!(mac_args, vec![test_url]);
}

#[tokio::test]
async fn test_listen_for_auth_code_addr_in_use_actionable_error() {
    // Bind a real socket on loopback to occupy an ephemeral port
    let occupying_socket =
        std::net::TcpListener::bind("127.0.0.1:0").expect("Must bind ephemeral port");
    let occupied_port = occupying_socket
        .local_addr()
        .expect("Must have local addr")
        .port();

    let res = eve_trade_scout::esi::sso::listen_for_auth_code(
        occupied_port,
        "dummy_state",
        std::time::Duration::from_millis(50),
    )
    .await;

    assert!(res.is_err(), "Must fail when port is already bound");
    let err_str = res.unwrap_err().to_string();
    assert!(
        err_str.contains(&format!(
            "Local port {} is already in use by another application",
            occupied_port
        )),
        "Error message must be actionable and specify the conflicting port, got: {}",
        err_str
    );
    assert!(
        err_str.contains(
            "Please close conflicting applications or configure an alternate callback port"
        ),
        "Error message must provide user guidance on how to resolve the collision, got: {}",
        err_str
    );
}

#[test]
fn test_esi_user_agent_header_compliance() {
    use eve_trade_scout::esi::client::USER_AGENT_VAL as CLIENT_UA;
    use eve_trade_scout::esi::sso::USER_AGENT_VAL as SSO_UA;

    // Both constants must match and identify app, version, repository, and maintainer contact
    assert_eq!(CLIENT_UA, SSO_UA);
    assert!(
        CLIENT_UA.starts_with("eve-trade-scout/0.1.0"),
        "User-Agent must start with app name and version"
    );
    assert!(
        CLIENT_UA.contains("https://github.com/Flambello/eve-traveling-merchant"),
        "User-Agent must contain repository link"
    );
    assert!(
        CLIENT_UA.contains("contact: Flambello"),
        "User-Agent must contain maintainer contact"
    );
}

#[tokio::test]
async fn test_listen_for_auth_code_immediate_cancellation_on_oauth_error() {
    use eve_trade_scout::esi::sso::listen_for_auth_code;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let loopback_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Must bind ephemeral loopback listener");
    let port = loopback_listener.local_addr().unwrap().port();
    drop(loopback_listener);

    let auth_task = tokio::spawn(async move {
        listen_for_auth_code(
            port,
            "test_expected_state",
            std::time::Duration::from_secs(5),
        )
        .await
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("Connect failed");

    let req = format!(
        "GET /callback?error=access_denied&error_description=The+user+denied+the+request&state=test_expected_state HTTP/1.1\r\nHost: localhost:{}\r\nConnection: close\r\n\r\n",
        port
    );
    stream.write_all(req.as_bytes()).await.unwrap();

    let mut resp_buf = vec![0u8; 2048];
    let n = stream.read(&mut resp_buf).await.unwrap();
    let resp_str = String::from_utf8_lossy(&resp_buf[..n]);

    // 1. Verify defensive HTTP security headers are present in response
    assert!(resp_str.starts_with("HTTP/1.1 200 OK"));
    assert!(resp_str.contains("X-Frame-Options: DENY"));
    assert!(resp_str.contains("Content-Security-Policy: default-src 'none'"));
    assert!(resp_str.contains("X-Content-Type-Options: nosniff"));
    assert!(resp_str.contains("Cache-Control: no-store"));
    assert!(resp_str.contains("Authorization Cancelled"));

    // 2. Verify listener returned immediate error without timing out
    let auth_res = auth_task.await.expect("Task panicked");
    assert!(auth_res.is_err(), "Must return error on access_denied");
    let err_msg = auth_res.unwrap_err().to_string();
    assert!(err_msg.contains("access_denied"));
    assert!(err_msg.contains("The user denied the request"));
}

#[tokio::test]
async fn test_listen_for_auth_code_defensive_http_headers_on_success() {
    use eve_trade_scout::esi::sso::listen_for_auth_code;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let loopback_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Must bind ephemeral loopback listener");
    let port = loopback_listener.local_addr().unwrap().port();
    drop(loopback_listener);

    let auth_task = tokio::spawn(async move {
        listen_for_auth_code(
            port,
            "secret_csrf_state_123",
            std::time::Duration::from_secs(5),
        )
        .await
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("Connect failed");

    let req = format!(
        "GET /callback?code=mock_success_code&state=secret_csrf_state_123 HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
        port
    );
    stream.write_all(req.as_bytes()).await.unwrap();

    let mut resp_buf = vec![0u8; 2048];
    let n = stream.read(&mut resp_buf).await.unwrap();
    let resp_str = String::from_utf8_lossy(&resp_buf[..n]);

    assert!(resp_str.starts_with("HTTP/1.1 200 OK"));
    assert!(resp_str.contains("X-Frame-Options: DENY"));
    assert!(resp_str.contains("Content-Security-Policy: default-src 'none'"));
    assert!(resp_str.contains("X-Content-Type-Options: nosniff"));
    assert!(resp_str.contains("Cache-Control: no-store"));
    assert!(resp_str.contains("Character Connected!"));

    let code = auth_task
        .await
        .expect("Task panicked")
        .expect("Must extract code");
    assert_eq!(code, "mock_success_code");
}

#[tokio::test]
async fn test_listen_for_auth_code_rejects_invalid_host_and_non_get() {
    use eve_trade_scout::esi::sso::listen_for_auth_code;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let loopback_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Must bind ephemeral loopback listener");
    let port = loopback_listener.local_addr().unwrap().port();
    drop(loopback_listener);

    let auth_task = tokio::spawn(async move {
        listen_for_auth_code(port, "valid_state_abc", std::time::Duration::from_secs(5)).await
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // 1. Non-GET request should be rejected with 400 Bad Request
    let mut bad_method_stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("Connect failed");
    let bad_method_req = format!(
        "POST /callback?code=stolen_code&state=valid_state_abc HTTP/1.1\r\nHost: localhost:{}\r\nConnection: close\r\n\r\n",
        port
    );
    bad_method_stream
        .write_all(bad_method_req.as_bytes())
        .await
        .unwrap();
    let mut resp_buf = vec![0u8; 1024];
    let n = bad_method_stream.read(&mut resp_buf).await.unwrap();
    let resp_str = String::from_utf8_lossy(&resp_buf[..n]);
    assert!(resp_str.starts_with("HTTP/1.1 400 Bad Request"));

    // 2. DNS rebinding attempt with attacker Host header should be rejected
    let mut rebinding_stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("Connect failed");
    let rebinding_req = format!(
        "GET /callback?code=stolen_code&state=valid_state_abc HTTP/1.1\r\nHost: evil-attacker-domain.com:{}\r\nConnection: close\r\n\r\n",
        port
    );
    rebinding_stream
        .write_all(rebinding_req.as_bytes())
        .await
        .unwrap();
    let mut resp_buf2 = vec![0u8; 1024];
    let n2 = rebinding_stream.read(&mut resp_buf2).await.unwrap();
    let resp_str2 = String::from_utf8_lossy(&resp_buf2[..n2]);
    assert!(resp_str2.starts_with("HTTP/1.1 400 Bad Request"));

    // 3. Legitimate request should succeed after rejecting attacks
    let mut legit_stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("Connect failed");
    let legit_req = format!(
        "GET /callback?code=verified_code_xyz&state=valid_state_abc HTTP/1.1\r\nHost: localhost:{}\r\nConnection: close\r\n\r\n",
        port
    );
    legit_stream.write_all(legit_req.as_bytes()).await.unwrap();
    let mut resp_buf3 = vec![0u8; 2048];
    let n3 = legit_stream.read(&mut resp_buf3).await.unwrap();
    let resp_str3 = String::from_utf8_lossy(&resp_buf3[..n3]);
    assert!(resp_str3.starts_with("HTTP/1.1 200 OK"));

    let code = auth_task
        .await
        .expect("Task panicked")
        .expect("Must extract code");
    assert_eq!(code, "verified_code_xyz");
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_ensure_fresh_token_opts_force_bypasses_expiration_check() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let temp_dir = std::env::temp_dir().join(format!(
        "eve_test_force_refresh_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let _cfg_guard = ConfigDirGuard::set(&temp_dir);

    use eve_trade_scout::esi::sso::ensure_fresh_token_opts;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Must bind test listener");
    let addr = listener.local_addr().expect("Must get local addr");
    let mock_url = format!("http://{}/v2/oauth/token", addr);

    let server_task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("Accept failed");
        let mut buf = vec![0u8; 2048];
        let n = socket.read(&mut buf).await.expect("Read failed");
        let request_str = String::from_utf8_lossy(&buf[..n]);

        assert!(request_str.starts_with("POST /v2/oauth/token"));
        assert!(request_str.contains("grant_type=refresh_token"));
        assert!(request_str.contains("refresh_token=my_valid_refresh_token"));

        let body =
            r#"{"access_token":"forced_fresh_access_jwt","token_type":"Bearer","expires_in":1200}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("Write failed");
    });

    let client = reqwest::Client::new();
    let mut token = SsoToken {
        access_token: "existing_valid_token".to_string(),
        refresh_token: "my_valid_refresh_token".to_string(),
        expires_at: now_secs() + 1000,
        token_type: "Bearer".to_string(),
        character_id: 2112345678,
        character_name: "Captain Trader".to_string(),
        client_id: Some("test_client_id_val".to_string()),
    };

    assert!(!token.needs_refresh(), "Token must not need normal refresh");

    let res =
        ensure_fresh_token_opts(&client, &mock_url, "test_client_id_val", &mut token, true).await;
    assert!(
        res.is_ok(),
        "Forced token refresh must succeed: {:?}",
        res.err()
    );
    assert_eq!(token.access_token, "forced_fresh_access_jwt");

    let _ = server_task.await;
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_get_character_location_recovers_from_401_via_transparent_token_refresh() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let temp_dir = std::env::temp_dir().join(format!(
        "eve_test_location_401_retry_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let _cfg_guard = ConfigDirGuard::set(&temp_dir);

    use eve_trade_scout::esi::sso::{get_character_location_with_token_opts, save_token};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // Save initial token with unexpired timestamp to disk
    let initial_token = SsoToken {
        access_token: "stale_access_jwt_that_will_401".to_string(),
        refresh_token: "valid_refresh_secret".to_string(),
        expires_at: now_secs() + 1000,
        token_type: "Bearer".to_string(),
        character_id: 2112345678,
        character_name: "Captain Trader".to_string(),
        client_id: Some("test_client_id_val".to_string()),
    };
    save_token(&initial_token).expect("Must save initial token");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Must bind test listener");
    let addr = listener.local_addr().expect("Must get local addr");
    let mock_token_url = format!("http://{}/v2/oauth/token", addr);
    let mock_esi_base = format!("http://{}", addr);

    // Mock server handles 3 sequential requests:
    // 1. GET /v1/characters/2112345678/location/ with stale token -> responds HTTP 401
    // 2. POST /v2/oauth/token with refresh token -> responds HTTP 200 with new access token
    // 3. GET /v1/characters/2112345678/location/ with new token -> responds HTTP 200 with location
    let server_task = tokio::spawn(async move {
        // Request 1: ESI returns 401
        let (mut socket1, _) = listener.accept().await.expect("Accept 1 failed");
        let mut buf1 = vec![0u8; 2048];
        let n1 = socket1.read(&mut buf1).await.expect("Read 1 failed");
        let req1 = String::from_utf8_lossy(&buf1[..n1]);
        assert!(req1.starts_with("GET /v1/characters/2112345678/location/"));
        assert!(req1.contains("stale_access_jwt_that_will_401"));

        let resp1 = "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: 26\r\nConnection: close\r\n\r\n{\"error\":\"token expired\"}";
        socket1
            .write_all(resp1.as_bytes())
            .await
            .expect("Write 1 failed");
        drop(socket1);

        // Request 2: Token refresh
        let (mut socket2, _) = listener.accept().await.expect("Accept 2 failed");
        let mut buf2 = vec![0u8; 2048];
        let n2 = socket2.read(&mut buf2).await.expect("Read 2 failed");
        let req2 = String::from_utf8_lossy(&buf2[..n2]);
        assert!(req2.starts_with("POST /v2/oauth/token"));
        assert!(req2.contains("valid_refresh_secret"));

        let token_body = r#"{"access_token":"brand_new_refreshed_access_jwt","token_type":"Bearer","expires_in":1200}"#;
        let resp2 = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            token_body.len(),
            token_body
        );
        socket2
            .write_all(resp2.as_bytes())
            .await
            .expect("Write 2 failed");
        drop(socket2);

        // Request 3: ESI retried with new token
        let (mut socket3, _) = listener.accept().await.expect("Accept 3 failed");
        let mut buf3 = vec![0u8; 2048];
        let n3 = socket3.read(&mut buf3).await.expect("Read 3 failed");
        let req3 = String::from_utf8_lossy(&buf3[..n3]);
        assert!(req3.starts_with("GET /v1/characters/2112345678/location/"));
        assert!(req3.contains("brand_new_refreshed_access_jwt"));

        let loc_body = r#"{"solar_system_id":30000142,"station_id":60003760}"#;
        let resp3 = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            loc_body.len(),
            loc_body
        );
        socket3
            .write_all(resp3.as_bytes())
            .await
            .expect("Write 3 failed");
        drop(socket3);
    });

    let client = reqwest::Client::new();
    let location_res = get_character_location_with_token_opts(
        &client,
        &mock_token_url,
        &mock_esi_base,
        "test_client_id_val",
    )
    .await;

    assert!(
        location_res.is_ok(),
        "Must transparently recover from 401 and succeed on retry: {:?}",
        location_res.err()
    );
    let location = location_res.unwrap();
    assert_eq!(location.solar_system_id, 30000142);
    assert_eq!(location.station_id, Some(60003760));

    let _ = server_task.await;
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_push_waypoints_recovers_from_401_via_transparent_token_refresh() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let temp_dir = std::env::temp_dir().join(format!(
        "eve_test_push_401_retry_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let _cfg_guard = ConfigDirGuard::set(&temp_dir);

    use eve_trade_scout::esi::sso::{push_waypoints_with_token_opts, save_token};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let initial_token = SsoToken {
        access_token: "stale_token_before_autopilot".to_string(),
        refresh_token: "valid_autopilot_refresh".to_string(),
        expires_at: now_secs() + 1000,
        token_type: "Bearer".to_string(),
        character_id: 2112345678,
        character_name: "Captain Trader".to_string(),
        client_id: Some("test_client_id_val".to_string()),
    };
    save_token(&initial_token).expect("Must save initial token");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Must bind test listener");
    let addr = listener.local_addr().expect("Must get local addr");
    let mock_token_url = format!("http://{}/v2/oauth/token", addr);
    let mock_waypoint_base = format!("http://{}/v2/user/ui/autopilot/waypoint/", addr);

    let server_task = tokio::spawn(async move {
        // Request 1: Waypoint injection returns 401
        let (mut socket1, _) = listener.accept().await.expect("Accept 1 failed");
        let mut buf1 = vec![0u8; 2048];
        let n1 = socket1.read(&mut buf1).await.expect("Read 1 failed");
        let req1 = String::from_utf8_lossy(&buf1[..n1]);
        assert!(req1.starts_with("POST /v2/user/ui/autopilot/waypoint/"));
        assert!(req1.contains("stale_token_before_autopilot"));

        let resp1 = "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: 26\r\nConnection: close\r\n\r\n{\"error\":\"token expired\"}";
        socket1
            .write_all(resp1.as_bytes())
            .await
            .expect("Write 1 failed");
        drop(socket1);

        // Request 2: Token refresh
        let (mut socket2, _) = listener.accept().await.expect("Accept 2 failed");
        let mut buf2 = vec![0u8; 2048];
        let n2 = socket2.read(&mut buf2).await.expect("Read 2 failed");
        let req2 = String::from_utf8_lossy(&buf2[..n2]);
        assert!(req2.starts_with("POST /v2/oauth/token"));
        assert!(req2.contains("valid_autopilot_refresh"));

        let token_body =
            r#"{"access_token":"renewed_autopilot_token","token_type":"Bearer","expires_in":1200}"#;
        let resp2 = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            token_body.len(),
            token_body
        );
        socket2
            .write_all(resp2.as_bytes())
            .await
            .expect("Write 2 failed");
        drop(socket2);

        // Request 3: Waypoint injection retried with new token
        let (mut socket3, _) = listener.accept().await.expect("Accept 3 failed");
        let mut buf3 = vec![0u8; 2048];
        let n3 = socket3.read(&mut buf3).await.expect("Read 3 failed");
        let req3 = String::from_utf8_lossy(&buf3[..n3]);
        assert!(req3.starts_with("POST /v2/user/ui/autopilot/waypoint/"));
        assert!(req3.contains("renewed_autopilot_token"));

        let resp3 = "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n";
        socket3
            .write_all(resp3.as_bytes())
            .await
            .expect("Write 3 failed");
        drop(socket3);
    });

    let client = reqwest::Client::new();
    let push_res = push_waypoints_with_token_opts(
        &client,
        &mock_token_url,
        &mock_waypoint_base,
        "test_client_id_val",
        &[60003760],
    )
    .await;

    assert!(
        push_res.is_ok(),
        "Must transparently recover from 401 and inject waypoint: {:?}",
        push_res.err()
    );
    assert_eq!(push_res.unwrap(), 1);

    let _ = server_task.await;
    let _ = std::fs::remove_dir_all(&temp_dir);
}
