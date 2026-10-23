use eve_trade_scout::arbitrage::{calculate_deal, sanitize_csv_cell, ArbitrageConfig};
use eve_trade_scout::audio::is_safe_sound_theme_id;
use eve_trade_scout::config::{SsoConfig, BIG_HAULER};
use eve_trade_scout::esi::models::MarketOrder;
use eve_trade_scout::esi::sso::{
    build_authorize_url, constant_time_eq, is_valid_client_id, parse_jwt_character_claims,
    sanitize_auth_url, sanitize_sso_error, save_token, token_storage_path, urldecode_simple,
    SsoToken,
};
use eve_trade_scout::graph::SystemGraph;
use eve_trade_scout::sde::loader::{ItemInfo, SdeLoader, MAX_DECOMPRESSED_SDE_BYTES};
use flate2::read::GzEncoder;
use flate2::Compression;
use rusqlite::Connection;
use std::io::{Cursor, Read};

static SSO_CONFIG_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

    // 3. At sign (@), Pipe (|), and Percent (%) formula triggers
    assert_eq!(sanitize_csv_cell("@SUM(1,2)"), "'@SUM(1,2)");
    assert_eq!(sanitize_csv_cell("|calc.exe"), "'|calc.exe");
    assert_eq!(sanitize_csv_cell("%COMSPEC%"), "'%COMSPEC%");

    // 4. Tab and Carriage Return triggers
    assert_eq!(sanitize_csv_cell("\t=cmd"), "'\t=cmd");
    assert_eq!(sanitize_csv_cell("\r=cmd"), "' =cmd");

    // 5. Leading whitespace before formula
    assert_eq!(sanitize_csv_cell("   =DDE()"), "'   =DDE()");

    // 6. Internal double quotes escaping
    assert_eq!(
        sanitize_csv_cell("Tritanium \"Super\" Ore"),
        "Tritanium \"\"Super\"\" Ore"
    );

    // 7. CRLF stripping to prevent CSV line injection
    assert_eq!(
        sanitize_csv_cell("Tritanium\r\nExploit"),
        "Tritanium  Exploit"
    );
    assert_eq!(
        sanitize_csv_cell("\r\n=calc"),
        "'  =calc"
    );

    // 8. Legitimate in-game names remain unmodified
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

#[tokio::test]
async fn test_max_pages_limit_enforced_at_runtime() {
    let temp_dir =
        std::env::temp_dir().join(format!("eve_test_pages_clamp_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mock_url = format!("http://{}/v1/markets/10000002/orders/", addr);

    let server_task = tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 1024];
        let n = socket.read(&mut buf).await.unwrap();
        let req = String::from_utf8_lossy(&buf[..n]);
        assert!(req.starts_with("GET "));

        let body = "[]";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-Pages: 1000\r\nETag: \"page-test\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket.write_all(response.as_bytes()).await.unwrap();
    });

    let client = eve_trade_scout::esi::client::EsiClient::new(&temp_dir).unwrap();
    let res: anyhow::Result<Vec<MarketOrder>> =
        client.get_all_pages_opts(&mock_url, &[], false).await;

    assert!(
        res.is_err(),
        "Must reject when X-Pages exceeds MAX_PAGES_LIMIT"
    );
    let err_msg = res.unwrap_err().to_string();
    assert!(
        err_msg.contains("exceeded maximum safe pagination limit (1000 pages; limit is 500)"),
        "Error message must specify page limit violation: {}",
        err_msg
    );

    server_task.await.unwrap();
    let _ = std::fs::remove_dir_all(&temp_dir);
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
    let _lock = SSO_CONFIG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
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

    let err =
        SdeLoader::decompress_and_validate_archive(Cursor::new(compressed_invalid), &temp_out);
    assert!(
        err.is_err(),
        "Decompressing invalid SQLite database must return Err"
    );
    assert!(
        err.unwrap_err()
            .to_string()
            .contains("valid SQLite database header"),
        "Must detect invalid SQLite header"
    );
    assert!(
        !temp_out.exists(),
        "Target database file must not exist on header failure"
    );

    // 2. Real production decompression bomb guard:
    // Compress a stream of zeros that exceeds MAX_DECOMPRESSED_SDE_BYTES (2 GB).
    // Because zeros compress at ~1000:1 ratio, 2.1 GB of zeros takes only ~2 MB compressed.
    let oversized_len = MAX_DECOMPRESSED_SDE_BYTES + 64 * 1024;
    let zero_source = std::io::repeat(0u8).take(oversized_len);
    let mut bomb_encoder = GzEncoder::new(zero_source, Compression::fast());
    let mut compressed_bomb = Vec::new();
    bomb_encoder.read_to_end(&mut compressed_bomb).unwrap();

    let bomb_err =
        SdeLoader::decompress_and_validate_archive(Cursor::new(compressed_bomb), &temp_out);
    assert!(
        bomb_err.is_err(),
        "Decompressing oversized stream must return Err"
    );
    let bomb_msg = bomb_err.unwrap_err().to_string();
    assert!(
        bomb_msg.contains("potential decompression bomb"),
        "Error message must indicate decompression bomb abort: {}",
        bomb_msg
    );
    assert!(
        !temp_out.exists(),
        "Database file must not be left behind after decompression bomb abort"
    );
    assert!(
        !temp_out.with_extension("sqlite.part").exists(),
        "Partial part file must be cleaned up"
    );

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
fn test_oauth_callback_query_parameter_split_once_resilience() {
    // Verifies that query parameter key-value extraction uses split_once to preserve values containing '='
    let query_str = "code=mock_code_123&state=state_abc=xyz&error_description=detail=part1=part2";
    let mut code = None;
    let mut state = None;
    let mut err_desc = None;

    for pair in query_str.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == "code" {
                code = Some(urldecode_simple(v));
            } else if k == "state" {
                state = Some(urldecode_simple(v));
            } else if k == "error_description" {
                err_desc = Some(urldecode_simple(v));
            }
        }
    }

    assert_eq!(code.as_deref(), Some("mock_code_123"));
    assert_eq!(
        state.as_deref(),
        Some("state_abc=xyz"),
        "Nested '=' in state must not be truncated"
    );
    assert_eq!(
        err_desc.as_deref(),
        Some("detail=part1=part2"),
        "Nested '=' in error_description must not be truncated"
    );
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
    assert_eq!(
        sanitize_sso_error(multiline_err),
        "Error line 1  Error line 2"
    );

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

#[test]
fn test_sanitize_auth_url_redacts_state_parameter() {
    let url = "https://login.eveonline.com/v2/oauth/authorize?response_type=code&client_id=my_client&redirect_uri=http%3A%2F%2Flocalhost%3A37373%2Fcallback&scope=esi-ui.write_waypoint.v1&state=super_secret_csrf_state_12345&code_challenge=xyz789&code_challenge_method=S256";
    let sanitized = sanitize_auth_url(url);

    assert!(!sanitized.contains("super_secret_csrf_state_12345"));
    assert!(sanitized.contains("state=[REDACTED]"));
    assert!(sanitized.contains("client_id=my_client"));
    assert!(sanitized.contains("code_challenge=xyz789"));
    assert!(sanitized.contains("code_challenge_method=S256"));

    // URL without state remains unchanged
    let no_state = "https://login.eveonline.com/v2/oauth/authorize?response_type=code";
    assert_eq!(sanitize_auth_url(no_state), no_state);
}

#[test]
fn test_sanitize_sso_error_case_insensitive_and_extended_keywords() {
    // 1. Case-insensitive token detection
    let mixed_case = "Server error with ACCESS_TOKEN=abcde in header";
    assert_eq!(
        sanitize_sso_error(mixed_case),
        "OAuth token endpoint error (details withheld for security)"
    );

    let refresh_mixed = "Failed Refresh_Token verification";
    assert_eq!(
        sanitize_sso_error(refresh_mixed),
        "OAuth token endpoint error (details withheld for security)"
    );

    // 2. Bearer and code_verifier detection
    let bearer_leak = "Invalid Bearer eyJhbGciOiJSUzI1NiIs...";
    assert_eq!(
        sanitize_sso_error(bearer_leak),
        "OAuth token endpoint error (details withheld for security)"
    );

    let verifier_leak = "Mismatched code_verifier parameter in payload";
    assert_eq!(
        sanitize_sso_error(verifier_leak),
        "OAuth token endpoint error (details withheld for security)"
    );

    // 3. Bypass prevention: token keyword located after byte 256
    let long_prefix = "A".repeat(300);
    let overflow_leak = format!("{} and access_token=stolen", long_prefix);
    assert_eq!(
        sanitize_sso_error(&overflow_leak),
        "OAuth token endpoint error (details withheld for security)",
        "Must detect sensitive tokens occurring past byte 256"
    );
}

#[test]
fn test_sso_token_debug_redacts_secrets() {
    let token = SsoToken {
        access_token: "super_secret_access_jwt_value_12345".to_string(),
        refresh_token: "super_secret_refresh_token_67890".to_string(),
        token_type: "Bearer".to_string(),
        expires_at: 1800000000,
        character_id: 2112345678,
        character_name: "Fleet Commander".to_string(),
        client_id: Some("public_client_1".to_string()),
    };

    let debug_output = format!("{:?}", token);

    // Ensure raw secrets are NEVER in debug representation
    assert!(
        !debug_output.contains("super_secret_access_jwt_value_12345"),
        "access_token must not appear in Debug output"
    );
    assert!(
        !debug_output.contains("super_secret_refresh_token_67890"),
        "refresh_token must not appear in Debug output"
    );

    // Ensure redaction marker is present
    assert!(debug_output.contains("access_token: \"[REDACTED]\""));
    assert!(debug_output.contains("refresh_token: \"[REDACTED]\""));
    assert!(debug_output.contains("character_name: \"Fleet Commander\""));
    assert!(debug_output.contains("character_id: 2112345678"));
}

#[test]
fn test_constant_time_eq_slices() {
    let a = b"exact_secret_state_match_128bit";
    let b = b"exact_secret_state_match_128bit";
    let c = b"exact_secret_state_match_128biX";
    let d = b"short";

    assert!(constant_time_eq(a, b), "Identical slices must match");
    assert!(
        !constant_time_eq(a, c),
        "Single differing byte must return false"
    );
    assert!(
        !constant_time_eq(a, d),
        "Differing lengths must return false"
    );
}

#[test]
fn test_config_rejects_empty_or_malformed_regions() {
    use eve_trade_scout::config::parse_region_ids;

    // 1. Valid comma-separated region IDs
    let parsed = parse_region_ids("10000002, 10000043,10000032")
        .expect("Valid regions must parse successfully");
    assert_eq!(parsed, vec![10000002, 10000043, 10000032]);

    // 2. Empty string ""
    let err = parse_region_ids("").expect_err("Empty regions must return an error");
    assert!(err.to_string().contains("Invalid regions argument"));

    // 3. Whitespace-only string "   "
    let err = parse_region_ids("   ").expect_err("Whitespace-only regions must return an error");
    assert!(err.to_string().contains("Invalid regions argument"));

    // 4. Non-numeric alphanumeric garbage "the_forge,domain"
    let err = parse_region_ids("the_forge,domain").expect_err("Alpha regions must return an error");
    assert!(err.to_string().contains("Invalid regions argument"));
}

#[test]
fn test_config_persistence_security_and_permission_isolation() {
    let _lock = SSO_CONFIG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let temp_dir = std::env::temp_dir().join(format!("eve_sec_cfg_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let orig_eve = std::env::var_os("EVE_CONFIG_DIR");
    std::env::set_var("EVE_CONFIG_DIR", &temp_dir);

    let mut cfg = eve_trade_scout::config::UserConfig::default();
    cfg.sso.client_id = "test_public_client_id_security".to_string();
    cfg.sso.callback_port = 37373;
    cfg.save().expect("Configuration save must succeed");

    let config_path = eve_trade_scout::config::UserConfig::config_path()
        .expect("Configuration path must resolve");
    assert!(config_path.exists(), "Configuration file must be created");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(&config_path).expect("Metadata must be readable");
        assert_eq!(
            meta.permissions().mode() & 0o777,
            0o600,
            "Saved config file must have 0600 permissions to isolate client ID and user preferences"
        );
    }

    // Verify atomic rename does not leave .tmp file behind
    let tmp_path = config_path.with_extension("toml.tmp");
    assert!(
        !tmp_path.exists(),
        "Atomic write must remove temporary file upon rename"
    );

    // Clean up
    let _ = std::fs::remove_dir_all(&temp_dir);
    if let Some(ref val) = orig_eve {
        std::env::set_var("EVE_CONFIG_DIR", val);
    } else {
        std::env::remove_var("EVE_CONFIG_DIR");
    }
}

#[test]
fn test_order_book_poisoning_defense() {
    use eve_trade_scout::esi::market::{best_buy, cheapest_sell};
    use eve_trade_scout::esi::models::MarketOrder;

    let valid_sell = MarketOrder {
        order_id: 1,
        type_id: 34,
        location_id: 60003760,
        system_id: 30000142,
        volume_total: 100,
        volume_remain: 100,
        min_volume: 1,
        price: 1000.0,
        is_buy_order: false,
        duration: 90,
        issued: None,
        range: None,
    };

    let zero_sell = MarketOrder {
        order_id: 2,
        type_id: 34,
        location_id: 60003760,
        system_id: 30000142,
        volume_total: 100,
        volume_remain: 100,
        min_volume: 1,
        price: 0.0,
        is_buy_order: false,
        duration: 90,
        issued: None,
        range: None,
    };

    let neg_sell = MarketOrder {
        order_id: 3,
        type_id: 34,
        location_id: 60003760,
        system_id: 30000142,
        volume_total: 100,
        volume_remain: 100,
        min_volume: 1,
        price: -50.0,
        is_buy_order: false,
        duration: 90,
        issued: None,
        range: None,
    };

    let nan_sell = MarketOrder {
        order_id: 4,
        type_id: 34,
        location_id: 60003760,
        system_id: 30000142,
        volume_total: 100,
        volume_remain: 100,
        min_volume: 1,
        price: f64::NAN,
        is_buy_order: false,
        duration: 90,
        issued: None,
        range: None,
    };

    let sells = vec![zero_sell, neg_sell, nan_sell, valid_sell.clone()];
    let best_s = cheapest_sell(&sells).expect("Must find valid sell order");
    assert_eq!(best_s.price, 1000.0, "Must ignore non-positive and NaN prices");

    let valid_buy = MarketOrder {
        order_id: 5,
        type_id: 34,
        location_id: 60003761,
        system_id: 30000144,
        volume_total: 100,
        volume_remain: 100,
        min_volume: 1,
        price: 2000.0,
        is_buy_order: true,
        duration: 90,
        issued: None,
        range: None,
    };

    let zero_buy = MarketOrder {
        order_id: 6,
        type_id: 34,
        location_id: 60003761,
        system_id: 30000144,
        volume_total: 100,
        volume_remain: 100,
        min_volume: 1,
        price: 0.0,
        is_buy_order: true,
        duration: 90,
        issued: None,
        range: None,
    };

    let nan_buy = MarketOrder {
        order_id: 7,
        type_id: 34,
        location_id: 60003761,
        system_id: 30000144,
        volume_total: 100,
        volume_remain: 100,
        min_volume: 1,
        price: f64::NAN,
        is_buy_order: true,
        duration: 90,
        issued: None,
        range: None,
    };

    let buys = vec![zero_buy, nan_buy, valid_buy.clone()];
    let best_b = best_buy(&buys).expect("Must find valid buy order");
    assert_eq!(best_b.price, 2000.0, "Must ignore zero and NaN buy prices");
}

#[tokio::test]
async fn test_find_deals_ignores_poisoned_orders() {
    use eve_trade_scout::arbitrage::find_deals;
    use eve_trade_scout::esi::market::RegionMarket;
    use std::collections::HashMap;

    let sde = SdeLoader::in_memory_with_fixtures().expect("Failed to init in-memory SDE fixtures");
    let graph = eve_trade_scout::graph::SystemGraph::from_sde(&sde).unwrap();
    let cfg = ArbitrageConfig {
        ship: BIG_HAULER,
        home_system: 30000142,
        safe_only: true,
        min_profit: 1000.0,
        min_spread: 10.0,
        min_net_profit_pct: 10.0,
        broker_fee: 3.0,
        sales_tax: 8.0,
        npc_stations_only: false,
    };

    // Item 34 is Tritanium in fixtures
    let valid_sell = MarketOrder {
        order_id: 101,
        type_id: 34,
        location_id: 60003760,
        system_id: 30000142,
        volume_total: 10_000,
        volume_remain: 10_000,
        min_volume: 1,
        price: 5.0,
        is_buy_order: false,
        duration: 90,
        issued: None,
        range: None,
    };

    let poison_zero_sell = MarketOrder {
        order_id: 102,
        type_id: 34,
        location_id: 60003760,
        system_id: 30000142,
        volume_total: 10_000,
        volume_remain: 10_000,
        min_volume: 1,
        price: 0.0,
        is_buy_order: false,
        duration: 90,
        issued: None,
        range: None,
    };

    let poison_nan_sell = MarketOrder {
        order_id: 103,
        type_id: 34,
        location_id: 60003760,
        system_id: 30000142,
        volume_total: 10_000,
        volume_remain: 10_000,
        min_volume: 1,
        price: f64::NAN,
        is_buy_order: false,
        duration: 90,
        issued: None,
        range: None,
    };

    let valid_buy = MarketOrder {
        order_id: 201,
        type_id: 34,
        location_id: 60003761,
        system_id: 30000144,
        volume_total: 10_000,
        volume_remain: 10_000,
        min_volume: 1,
        price: 15.0,
        is_buy_order: true,
        duration: 90,
        issued: None,
        range: None,
    };

    let poison_nan_buy = MarketOrder {
        order_id: 202,
        type_id: 34,
        location_id: 60003761,
        system_id: 30000144,
        volume_total: 10_000,
        volume_remain: 10_000,
        min_volume: 1,
        price: f64::NAN,
        is_buy_order: true,
        duration: 90,
        issued: None,
        range: None,
    };

    let mut sell_orders = HashMap::new();
    sell_orders.insert(34, vec![poison_zero_sell, poison_nan_sell, valid_sell]);

    let mut buy_orders = HashMap::new();
    buy_orders.insert(34, vec![poison_nan_buy, valid_buy]);

    let mut markets = HashMap::new();
    markets.insert(
        10000002,
        RegionMarket {
            region_id: 10000002,
            sell_orders,
            buy_orders,
        },
    );

    let deals = find_deals(&markets, &sde, &graph, &cfg);
    assert!(!deals.is_empty(), "Valid deal must be found despite poisoned orders in market");
    assert_eq!(deals[0].type_id, 34);
    assert_eq!(deals[0].buy_price, 5.0);
    assert_eq!(deals[0].sell_price, 15.0);
    assert!(deals[0].spread_pct.is_finite());
    assert!(deals[0].net_profit > 0.0);
}

#[test]
fn test_sde_autocomplete_wildcard_escaping_and_case_priority() {
    use eve_trade_scout::sde::escape_sqlite_like;

    // 1. Literal wildcard escaping
    assert_eq!(escape_sqlite_like("Jita"), "Jita");
    assert_eq!(escape_sqlite_like("Jita%"), "Jita\\%");
    assert_eq!(escape_sqlite_like("Jita_"), "Jita\\_");
    assert_eq!(escape_sqlite_like("Jita\\"), "Jita\\\\");
    assert_eq!(escape_sqlite_like("%_%\\"), "\\%\\_\\%\\\\");

    // 2. Autocomplete search query behavior
    let sde = SdeLoader::in_memory_with_fixtures().expect("Failed to init in-memory SDE fixtures");

    // Search for capitalized "Jita" should rank "Jita" first via exact match
    let results = sde.search_systems("Jita", 5);
    assert!(!results.is_empty());
    assert_eq!(results[0].system_name, "Jita");

    // Search for lowercase "jita" should also rank "Jita" first
    let lower_results = sde.search_systems("jita", 5);
    assert!(!lower_results.is_empty());
    assert_eq!(lower_results[0].system_name, "Jita");
}

#[test]
fn test_ship_profile_nan_defense() {
    let mut cfg = eve_trade_scout::config::UserConfig::default();
    cfg.ship.use_custom_ship = true;

    // 1. Test NaN and Infinite inputs (must fallback to default vessel limits)
    cfg.ship.custom_cargo_vol = f64::NAN;
    cfg.ship.custom_budget_isk = f64::INFINITY;
    let profile = cfg.effective_ship_profile().expect("Profile resolution must succeed");
    assert!(profile.max_volume.is_finite());
    assert!(profile.max_budget.is_finite());
    assert_eq!(profile.max_volume, 60_000.0);
    assert_eq!(profile.max_budget, 500_000_000.0);

    // 2. Test negative inputs (finite values must be safely clamped to minimum bounds)
    cfg.ship.custom_cargo_vol = -500.0;
    cfg.ship.custom_budget_isk = -1_000_000.0;
    let profile_neg = cfg.effective_ship_profile().expect("Profile resolution must succeed");
    assert!(profile_neg.max_volume.is_finite());
    assert!(profile_neg.max_budget.is_finite());
    assert_eq!(profile_neg.max_volume, 10.0);
    assert_eq!(profile_neg.max_budget, 100_000.0);

    // 3. Test valid custom inputs
    cfg.ship.custom_cargo_vol = 45_000.0;
    cfg.ship.custom_budget_isk = 250_000_000.0;
    let profile_custom = cfg.effective_ship_profile().expect("Profile resolution must succeed");
    assert_eq!(profile_custom.max_volume, 45_000.0);
    assert_eq!(profile_custom.max_budget, 250_000_000.0);
}
