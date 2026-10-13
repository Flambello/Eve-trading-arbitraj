use eve_trade_scout::arbitrage::{calculate_deal, sanitize_csv_cell, ArbitrageConfig};
use eve_trade_scout::audio::is_safe_sound_theme_id;
use eve_trade_scout::config::{SsoConfig, BIG_HAULER};
use eve_trade_scout::esi::client::{MAX_NETWORK_RETRIES, MAX_PAGES_LIMIT};
use eve_trade_scout::esi::models::MarketOrder;
use eve_trade_scout::esi::sso::{
    build_authorize_url, is_valid_client_id, parse_jwt_character_claims, sanitize_sso_error,
    save_token, token_storage_path, urldecode_simple, SsoToken,
};
use eve_trade_scout::graph::SystemGraph;
use eve_trade_scout::sde::loader::{
    ItemInfo, SdeLoader, MAX_DECOMPRESSED_SDE_BYTES, MAX_SDE_COMPRESSED_BYTES,
    MIN_SDE_COMPRESSED_BYTES,
};
use flate2::read::GzEncoder;
use flate2::Compression;
use rusqlite::Connection;
use std::io::{Cursor, Read};

#[test]
fn test_csv_formula_injection_sanitization() {
    // 1. Equal sign formula injection
    assert_eq!(
        sanitize_csv_cell("=cmd|' /C calc'!A0"),
        "'=cmd|' /C calc'!A0"
    );

    // 2. Plus / Minus signs
    assert_eq!(sanitize_csv_cell("+1+2+cmd"), "'+1+2+cmd");
    assert_eq!(sanitize_csv_cell("-2+5+cmd"), "'-2+5+cmd");

    // 3. At sign (@)
    assert_eq!(sanitize_csv_cell("@SUM(1,2)"), "'@SUM(1,2)");

    // 4. Tab and Carriage Return triggers
    assert_eq!(sanitize_csv_cell("\t=cmd"), "'\t=cmd");
    assert_eq!(sanitize_csv_cell("\r=cmd"), "'\r=cmd");

    // 5. Leading whitespace before formula
    assert_eq!(sanitize_csv_cell("   =DDE()"), "'   =DDE()");

    // 6. Internal double quotes escaping
    assert_eq!(
        sanitize_csv_cell("Tritanium \"Super\" Ore"),
        "Tritanium \"\"Super\"\" Ore"
    );

    // 7. Legitimate in-game names remain unmodified
    assert_eq!(
        sanitize_csv_cell("Jita IV - Moon 4 - Caldari Navy Assembly Plant"),
        "Jita IV - Moon 4 - Caldari Navy Assembly Plant"
    );
    assert_eq!(sanitize_csv_cell("Tritanium"), "Tritanium");
    assert_eq!(sanitize_csv_cell("Perimeter"), "Perimeter");
}

#[test]
fn test_sound_theme_id_validation() {
    // 1. Valid safe identifiers
    assert!(is_safe_sound_theme_id("bell"));
    assert!(is_safe_sound_theme_id("message-new-instant"));
    assert!(is_safe_sound_theme_id("alarm_1"));
    assert!(is_safe_sound_theme_id("sound-theme-2026"));

    // 2. Path traversal attempts
    assert!(!is_safe_sound_theme_id("../../etc/passwd"));
    assert!(!is_safe_sound_theme_id("../stereo/bell"));
    assert!(!is_safe_sound_theme_id("/bell"));

    // 3. Command injection and shell metacharacters
    assert!(!is_safe_sound_theme_id("bell; rm -rf /"));
    assert!(!is_safe_sound_theme_id("bell && calc"));
    assert!(!is_safe_sound_theme_id("`id`"));
    assert!(!is_safe_sound_theme_id("$(id)"));

    // 4. Empty and whitespace
    assert!(!is_safe_sound_theme_id(""));
    assert!(!is_safe_sound_theme_id("   "));
    assert!(!is_safe_sound_theme_id("bell flag"));
}

#[test]
#[allow(clippy::assertions_on_constants)]
fn test_security_constants_bounds() {
    // ESI pagination upper bound prevents OOM task flood
    assert_eq!(MAX_PAGES_LIMIT, 500);
    assert!(MAX_PAGES_LIMIT > 0 && MAX_PAGES_LIMIT <= 1000);

    // Maximum bounded network retry attempts to avoid infinite hang on disconnects
    assert_eq!(MAX_NETWORK_RETRIES, 5);
    assert!(MAX_NETWORK_RETRIES >= 3 && MAX_NETWORK_RETRIES <= 10);

    // SDE decompression bomb guard thresholds
    assert_eq!(MAX_DECOMPRESSED_SDE_BYTES, 2 * 1024 * 1024 * 1024);
    assert_eq!(MIN_SDE_COMPRESSED_BYTES, 20 * 1024 * 1024);
    assert_eq!(MAX_SDE_COMPRESSED_BYTES, 400 * 1024 * 1024);

    // Mathematical ordering and safety invariant enforcement
    assert!(MIN_SDE_COMPRESSED_BYTES < MAX_SDE_COMPRESSED_BYTES);
    assert!(MAX_SDE_COMPRESSED_BYTES < MAX_DECOMPRESSED_SDE_BYTES);
}

#[test]
fn test_sde_loader_in_memory_integrity_check() {
    // SdeLoader with fixtures initializes cleanly with integrity checks passing
    let sde = SdeLoader::in_memory_with_fixtures();
    assert!(sde.is_ok());

    let loader = sde.unwrap();
    let jita = loader.get_system_id("Jita");
    assert_eq!(jita, Some(30000142));
}

#[test]
fn test_sde_schema_validation_catches_missing_tables_and_empty_data() {
    // 1. Connection with all required tables passes
    let valid_conn = Connection::open_in_memory().unwrap();
    valid_conn
        .execute_batch(
            "
        CREATE TABLE mapSolarSystems (solarSystemID INTEGER);
        CREATE TABLE staStations (stationID INTEGER);
        CREATE TABLE invTypes (typeID INTEGER);
        CREATE TABLE mapSolarSystemJumps (fromSolarSystemID INTEGER);
        INSERT INTO mapSolarSystems VALUES (30000142);
    ",
        )
        .unwrap();
    assert!(SdeLoader::validate_schema(&valid_conn).is_ok());

    // 2. Missing mapSolarSystemJumps table fails
    let missing_conn = Connection::open_in_memory().unwrap();
    missing_conn
        .execute_batch(
            "
        CREATE TABLE mapSolarSystems (solarSystemID INTEGER);
        CREATE TABLE staStations (stationID INTEGER);
        CREATE TABLE invTypes (typeID INTEGER);
        INSERT INTO mapSolarSystems VALUES (30000142);
    ",
        )
        .unwrap();
    let err = SdeLoader::validate_schema(&missing_conn);
    assert!(err.is_err());
    assert!(err
        .unwrap_err()
        .to_string()
        .contains("missing essential universe table: mapSolarSystemJumps"));

    // 3. Empty mapSolarSystems table fails
    let empty_conn = Connection::open_in_memory().unwrap();
    empty_conn
        .execute_batch(
            "
        CREATE TABLE mapSolarSystems (solarSystemID INTEGER);
        CREATE TABLE staStations (stationID INTEGER);
        CREATE TABLE invTypes (typeID INTEGER);
        CREATE TABLE mapSolarSystemJumps (fromSolarSystemID INTEGER);
    ",
        )
        .unwrap();
    let empty_err = SdeLoader::validate_schema(&empty_conn);
    assert!(empty_err.is_err());
    assert!(empty_err
        .unwrap_err()
        .to_string()
        .contains("contains 0 solar systems"));
}

#[test]
fn test_calculate_deal_non_finite_and_negative_price_rejection() {
    let sde = SdeLoader::in_memory_with_fixtures().unwrap();
    let graph = SystemGraph::from_sde(&sde).unwrap();

    let cfg = ArbitrageConfig {
        ship: BIG_HAULER,
        home_system: 30000142,
        min_profit: 10_000.0,
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

    let make_order = |id: u64, price: f64, is_buy: bool, loc: u64, sys: u32| MarketOrder {
        order_id: id,
        type_id: 34,
        location_id: loc,
        system_id: sys,
        volume_total: 10_000,
        volume_remain: 10_000,
        min_volume: 1,
        price,
        is_buy_order: is_buy,
        duration: 90,
        issued: None,
        range: None,
    };

    // 1. NaN buy price rejected
    let src_nan = make_order(1, f64::NAN, false, 60003760, 30000142);
    let dst_ok = make_order(2, 10.0, true, 60003761, 30000144);
    assert!(calculate_deal(34, &item, &src_nan, &dst_ok, &cfg, &sde, &graph).is_none());

    // 2. Infinity sell price rejected
    let src_ok = make_order(1, 5.0, false, 60003760, 30000142);
    let dst_inf = make_order(2, f64::INFINITY, true, 60003761, 30000144);
    assert!(calculate_deal(34, &item, &src_ok, &dst_inf, &cfg, &sde, &graph).is_none());

    // 3. Zero or negative price rejected
    let src_zero = make_order(1, 0.0, false, 60003760, 30000142);
    assert!(calculate_deal(34, &item, &src_zero, &dst_ok, &cfg, &sde, &graph).is_none());

    let dst_neg = make_order(2, -5.0, true, 60003761, 30000144);
    assert!(calculate_deal(34, &item, &src_ok, &dst_neg, &cfg, &sde, &graph).is_none());

    // 4. Valid finite positive trade succeeds
    let deal = calculate_deal(34, &item, &src_ok, &dst_ok, &cfg, &sde, &graph);
    assert!(deal.is_some());
}

#[test]
fn test_sso_token_save_unix_permissions() {
    let token = SsoToken {
        access_token: "test_access_token".to_string(),
        refresh_token: "test_refresh_token".to_string(),
        token_type: "Bearer".to_string(),
        expires_at: 9999999999,
        character_id: 2112345678,
        character_name: "Security Test Pilot".to_string(),
        client_id: None,
    };

    let save_res = save_token(&token);
    assert!(save_res.is_ok(), "save_token must succeed");

    let path = token_storage_path();
    assert!(path.exists(), "Token file must exist after save");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(&path).expect("File metadata must be accessible");
        let mode = metadata.permissions().mode();
        // Mask with 0o777 to get standard permission bits: must be 0o600 (-rw-------)
        assert_eq!(
            mode & 0o777,
            0o600,
            "SSO token file must have restricted 0600 permissions"
        );
    }
}

#[test]
fn test_decompression_bomb_guard_stops_excessive_stream() {
    let temp_dir = std::env::temp_dir().join(format!("eve_bomb_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let temp_out = temp_dir.join("test_sde.sqlite");

    // 1. Invalid SQLite header test on real production decompress_and_validate_archive
    let fake_data = b"NOT_A_SQLITE_DATABASE_HEADER_DATA_HERE";
    let mut encoder = GzEncoder::new(Cursor::new(fake_data), Compression::fast());
    let mut compressed_invalid = Vec::new();
    encoder.read_to_end(&mut compressed_invalid).unwrap();

    let err = SdeLoader::decompress_and_validate_archive(Cursor::new(compressed_invalid), &temp_out);
    assert!(err.is_err(), "Decompressing invalid SQLite database must return Err");
    assert!(
        err.unwrap_err().to_string().contains("valid SQLite database header"),
        "Must detect invalid SQLite header"
    );
    assert!(!temp_out.exists(), "Target database file must not exist on header failure");

    // 2. Real production decompression bomb guard:
    // Compress a stream of zeros that exceeds MAX_DECOMPRESSED_SDE_BYTES (2 GB).
    // Because zeros compress at ~1000:1 ratio, 2.1 GB of zeros takes only ~2 MB compressed.
    let oversized_len = MAX_DECOMPRESSED_SDE_BYTES + 64 * 1024;
    let zero_source = std::io::repeat(0u8).take(oversized_len);
    let mut bomb_encoder = GzEncoder::new(zero_source, Compression::fast());
    let mut compressed_bomb = Vec::new();
    bomb_encoder.read_to_end(&mut compressed_bomb).unwrap();

    let bomb_err = SdeLoader::decompress_and_validate_archive(Cursor::new(compressed_bomb), &temp_out);
    assert!(bomb_err.is_err(), "Decompressing oversized stream must return Err");
    let bomb_msg = bomb_err.unwrap_err().to_string();
    assert!(
        bomb_msg.contains("potential decompression bomb"),
        "Error message must indicate decompression bomb abort: {}",
        bomb_msg
    );
    assert!(!temp_out.exists(), "Database file must not be left behind after decompression bomb abort");
    assert!(!temp_out.with_extension("sqlite.part").exists(), "Partial part file must be cleaned up");

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_build_authorize_url_encodes_client_id_and_rejects_injection() {
    let malicious_client_id = "test_client_id&redirect_uri=https://evil.com#";
    let url = build_authorize_url(malicious_client_id, 37373, "state123", "challenge456");

    // The client_id must be properly URL-encoded so that injected parameters cannot alter the query string
    assert!(url.contains("client_id=test_client_id%26redirect_uri%3Dhttps%3A%2F%2Fevil.com%23"));
    assert!(!url.contains("&redirect_uri=https://evil.com#"));
}

#[test]
fn test_client_id_validation() {
    // Valid Client IDs
    assert!(is_valid_client_id("test_client_id_12345"));
    assert!(is_valid_client_id("abc-def-123"));
    assert!(is_valid_client_id("my_app_client"));

    // Invalid Client IDs containing injection or invalid characters
    assert!(!is_valid_client_id(""));
    assert!(!is_valid_client_id("   "));
    assert!(!is_valid_client_id("client; rm -rf /"));
    assert!(!is_valid_client_id("client&param=value"));
    assert!(!is_valid_client_id("client with spaces"));
    assert!(!is_valid_client_id("client#hash"));
}

#[test]
fn test_oauth_callback_urldecode_simple() {
    assert_eq!(urldecode_simple("hello%20world"), "hello world");
    assert_eq!(urldecode_simple("code%2B123%2Fxyz"), "code+123/xyz");
    assert_eq!(urldecode_simple("state%3Dabc%26xyz"), "state=abc&xyz");
    assert_eq!(urldecode_simple("simple+plus+test"), "simple plus test");
    assert_eq!(urldecode_simple("no_encoding_needed"), "no_encoding_needed");
    assert_eq!(urldecode_simple("incomplete%2"), "incomplete%2");
}

#[test]
fn test_sanitize_sso_error() {
    // Strips token references
    let leak_error = "Error: invalid grant for access_token=secret_12345";
    assert_eq!(
        sanitize_sso_error(leak_error),
        "OAuth token endpoint error (details withheld for security)"
    );

    // Strips refresh token references
    let refresh_leak = "Failed: refresh_token is bad";
    assert_eq!(
        sanitize_sso_error(refresh_leak),
        "OAuth token endpoint error (details withheld for security)"
    );

    // Replaces newlines and carriage returns
    let multiline_err = "Error line 1\r\nError line 2";
    assert_eq!(sanitize_sso_error(multiline_err), "Error line 1  Error line 2");

    // Truncates overly long strings
    let long_err = "a".repeat(500);
    assert_eq!(sanitize_sso_error(&long_err).len(), 256);
}

#[test]
fn test_jwt_claims_parsing_rejects_zero_or_missing_character_id() {
    // 1. Missing or zero character ID should return None
    let header_b64 = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";
    // Payload with character ID 0: {"name":"Test Pilot","sub":"CHARACTER:EVE:0"}
    let payload_zero = "eyJuYW1lIjoiVGVzdCBQaWxvdCIsInN1YiI6IkNIQVJBQ1RFUjpFVkU6MCJ9";
    let token_zero = format!("{}.{}.fake_sig", header_b64, payload_zero);
    assert!(parse_jwt_character_claims(&token_zero).is_none());

    // Payload with valid character ID: {"name":"Test Pilot","sub":"CHARACTER:EVE:2112345678"}
    let payload_valid = "eyJuYW1lIjoiVGVzdCBQaWxvdCIsInN1YiI6IkNIQVJBQ1RFUjpFVkU6MjExMjM0NTY3OCJ9";
    let token_valid = format!("{}.{}.fake_sig", header_b64, payload_valid);
    let parsed = parse_jwt_character_claims(&token_valid);
    assert_eq!(parsed, Some((2112345678, "Test Pilot".to_string())));

    // Payload with empty name should return None
    let payload_empty_name = "eyJuYW1lIjoiICIsInN1YiI6IkNIQVJBQ1RFUjpFVkU6MjExMjM0NTY3OCJ9";
    let token_empty_name = format!("{}.{}.fake_sig", header_b64, payload_empty_name);
    assert!(parse_jwt_character_claims(&token_empty_name).is_none());
}

#[test]
fn test_sso_config_validated_callback_port() {
    let privileged_cfg = SsoConfig {
        client_id: "test".to_string(),
        callback_port: 80,
        auto_sync_location: false,
    };
    assert_eq!(privileged_cfg.validated_callback_port(), 37373);

    let zero_cfg = SsoConfig {
        client_id: "test".to_string(),
        callback_port: 0,
        auto_sync_location: false,
    };
    assert_eq!(zero_cfg.validated_callback_port(), 37373);

    let valid_cfg = SsoConfig {
        client_id: "test".to_string(),
        callback_port: 8080,
        auto_sync_location: false,
    };
    assert_eq!(valid_cfg.validated_callback_port(), 8080);
}
