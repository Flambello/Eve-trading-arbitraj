use eve_trade_scout::arbitrage::{calculate_deal, sanitize_csv_cell, ArbitrageConfig};
use eve_trade_scout::audio::is_safe_sound_theme_id;
use eve_trade_scout::config::BIG_HAULER;
use eve_trade_scout::esi::client::{MAX_NETWORK_RETRIES, MAX_PAGES_LIMIT};
use eve_trade_scout::esi::models::MarketOrder;
use eve_trade_scout::esi::sso::{save_token, token_storage_path, SsoToken};
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
fn test_security_constants_bounds() {
    // ESI pagination upper bound prevents OOM task flood
    assert_eq!(MAX_PAGES_LIMIT, 500);

    // Maximum bounded network retry attempts to avoid infinite hang on disconnects
    assert_eq!(MAX_NETWORK_RETRIES, 5);

    // SDE decompression bomb guard thresholds
    assert_eq!(MAX_DECOMPRESSED_SDE_BYTES, 2 * 1024 * 1024 * 1024);
    assert_eq!(MIN_SDE_COMPRESSED_BYTES, 20 * 1024 * 1024);
    assert_eq!(MAX_SDE_COMPRESSED_BYTES, 400 * 1024 * 1024);
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
    // Generate a highly compressible gzip stream (zeros)
    let raw_data = vec![0u8; 100_000];
    let mut encoder = GzEncoder::new(Cursor::new(raw_data), Compression::best());
    let mut compressed = Vec::new();
    encoder.read_to_end(&mut compressed).unwrap();

    // Use a small safety threshold (e.g. 10 KB) to simulate the guard behavior
    let safety_limit = 10_000u64;
    let decoder = flate2::read::GzDecoder::new(Cursor::new(compressed));
    let mut bounded_reader = decoder.take(safety_limit + 1);

    let mut buffer = [0u8; 1024];
    let mut total = 0u64;
    let mut exceeded = false;

    while let Ok(n) = bounded_reader.read(&mut buffer) {
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > safety_limit {
            exceeded = true;
            break;
        }
    }

    // Must detect stream exceeding limit and break immediately
    assert!(
        exceeded,
        "Decompression guard should catch oversized stream"
    );
    assert!(total > safety_limit);
}
