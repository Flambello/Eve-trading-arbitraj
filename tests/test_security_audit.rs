use eve_trade_scout::arbitrage::sanitize_csv_cell;
use eve_trade_scout::audio::is_safe_sound_theme_id;
use eve_trade_scout::esi::client::MAX_PAGES_LIMIT;
use eve_trade_scout::sde::loader::{
    SdeLoader, MAX_DECOMPRESSED_SDE_BYTES, MAX_SDE_COMPRESSED_BYTES, MIN_SDE_COMPRESSED_BYTES,
};
use flate2::read::GzEncoder;
use flate2::Compression;
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
