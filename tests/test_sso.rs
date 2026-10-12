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
    };

    let serialized = serde_json::to_string(&token).expect("Token serialization must succeed");
    let deserialized: SsoToken =
        serde_json::from_str(&serialized).expect("Token deserialization must succeed");

    assert_eq!(deserialized.character_name, "Captain Trader");
    assert_eq!(deserialized.character_id, 2112345678);
    assert_eq!(deserialized.access_token, "mock_jwt_access_token");
    assert!(
        !deserialized.is_expired(),
        "Fresh token must not be expired"
    );

    // Token with expiration in the past
    let expired_token = SsoToken {
        expires_at: now - 30,
        ..token.clone()
    };
    assert!(expired_token.is_expired(), "Past token must report expired");

    // Token expiring within 60 second safety buffer
    let buffer_token = SsoToken {
        expires_at: now + 45,
        ..token
    };
    assert!(
        buffer_token.is_expired(),
        "Token expiring in 45s must trigger safety buffer refresh"
    );
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

#[test]
fn test_developer_client_id_resolution() {
    use eve_trade_scout::esi::sso::{get_developer_client_id, resolve_client_id};

    // Explicit custom ID resolves directly
    let custom = resolve_client_id("explicit_custom_key_123").unwrap();
    assert_eq!(custom, "explicit_custom_key_123");

    // Test environment variable resolution
    std::env::set_var("EVE_CLIENT_ID", "env_injected_client_id_456");
    let from_env = get_developer_client_id();
    assert_eq!(from_env.as_deref(), Some("env_injected_client_id_456"));

    let resolved_from_env = resolve_client_id("").unwrap();
    assert_eq!(resolved_from_env, "env_injected_client_id_456");

    // Clean up env
    std::env::remove_var("EVE_CLIENT_ID");

    // Test private config file resolution (~/.config/eve-trade-scout/developer_client_id)
    let from_file = get_developer_client_id();
    assert!(
        from_file.is_some(),
        "Must load client ID from private local config file"
    );
    assert_eq!(
        from_file.as_deref(),
        Some("5793fc7aa2cb49e385d3cfb5d52061c7")
    );
}
