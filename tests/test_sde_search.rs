use eve_trade_scout::sde::{
    arabic_to_roman, format_station_name, roman_to_arabic, station_name_to_sde, SdeLoader,
};
use std::path::PathBuf;

#[test]
fn test_roman_numeral_conversion() {
    // 1 to 20
    let expected = [
        (1, "I"),
        (2, "II"),
        (3, "III"),
        (4, "IV"),
        (5, "V"),
        (6, "VI"),
        (7, "VII"),
        (8, "VIII"),
        (9, "IX"),
        (10, "X"),
        (11, "XI"),
        (12, "XII"),
        (13, "XIII"),
        (14, "XIV"),
        (15, "XV"),
        (16, "XVI"),
        (17, "XVII"),
        (18, "XVIII"),
        (19, "XIX"),
        (20, "XX"),
    ];

    for (arabic, roman) in expected {
        assert_eq!(
            arabic_to_roman(arabic),
            roman,
            "arabic_to_roman({}) failed",
            arabic
        );
        assert_eq!(
            roman_to_arabic(roman),
            Some(arabic),
            "roman_to_arabic({}) failed",
            roman
        );
        // Case-insensitivity test
        assert_eq!(roman_to_arabic(&roman.to_lowercase()), Some(arabic));
    }

    // Additional Roman numbers
    assert_eq!(arabic_to_roman(40), "XL");
    assert_eq!(arabic_to_roman(50), "L");
    assert_eq!(arabic_to_roman(90), "XC");
    assert_eq!(arabic_to_roman(100), "C");
    assert_eq!(arabic_to_roman(400), "CD");
    assert_eq!(arabic_to_roman(900), "CM");
    assert_eq!(arabic_to_roman(1999), "MCMXCIX");

    // Invalid Roman strings
    assert_eq!(roman_to_arabic(""), None);
    assert_eq!(roman_to_arabic("ABC"), None);
    assert_eq!(roman_to_arabic("IIZ"), None);
}

#[test]
fn test_station_name_formatting_and_canonicalization() {
    // 1. Planet number converted to Roman, moon number remains Arabic
    let sde_jita = "Jita 4 - Moon 4 - Caldari Navy Assembly Plant";
    let ingame_jita = format_station_name(sde_jita);
    assert_eq!(
        ingame_jita,
        "Jita IV - Moon 4 - Caldari Navy Assembly Plant"
    );
    assert_eq!(station_name_to_sde(&ingame_jita), sde_jita);

    // 2. Direct planet station without moon
    let sde_annaro = "Annaro 1 - CBD Corporation Storage";
    let ingame_annaro = format_station_name(sde_annaro);
    assert_eq!(ingame_annaro, "Annaro I - CBD Corporation Storage");
    assert_eq!(station_name_to_sde(&ingame_annaro), sde_annaro);

    // 3. Planet with 2 digits
    let sde_arnon = "Arnon 12 - CBD Corporation Storage";
    let ingame_arnon = format_station_name(sde_arnon);
    assert_eq!(ingame_arnon, "Arnon XII - CBD Corporation Storage");
    assert_eq!(station_name_to_sde(&ingame_arnon), sde_arnon);

    // 4. Station already in Roman numerals (e.g. Amarr Prime)
    let sde_amarr = "Amarr VIII (Oris) - Emperor Family Academy";
    let ingame_amarr = format_station_name(sde_amarr);
    assert_eq!(ingame_amarr, "Amarr VIII (Oris) - Emperor Family Academy");

    // 5. Non-planetary station (Zarzakh)
    let sde_zarzakh = "Zarzakh - Deathless Custodians - The Fulcrum";
    let ingame_zarzakh = format_station_name(sde_zarzakh);
    assert_eq!(
        ingame_zarzakh,
        "Zarzakh - Deathless Custodians - The Fulcrum"
    );
}

#[tokio::test]
async fn test_sde_search_systems_autocomplete() {
    let sde_path = PathBuf::from("data").join("eve_sde.sqlite");
    if !sde_path.exists() {
        return;
    }

    let sde = SdeLoader::open(&sde_path, 30)
        .await
        .expect("Failed to open SDE");

    // Exact search / prefix search for "Jit"
    let results = sde.search_systems("Jit", 5);
    assert!(!results.is_empty(), "Expected results for query 'Jit'");
    assert_eq!(results[0].system_name, "Jita");
    assert_eq!(results[0].system_id, 30000142);
    assert_eq!(results[0].region_name, "The Forge");
    assert!(results[0].security_status >= 0.9);

    // Prefix search for "Ama"
    let amarr_results = sde.search_systems("Ama", 5);
    assert!(!amarr_results.is_empty());
    let found_amarr = amarr_results.iter().any(|r| r.system_name == "Amarr");
    assert!(found_amarr, "Expected 'Amarr' in results for query 'Ama'");

    // Empty query returns empty
    let empty_results = sde.search_systems("", 5);
    assert!(empty_results.is_empty());

    // Roman numeral planet in autocomplete query extracts system
    let jita_roman_res = sde.search_systems("Jita IV", 5);
    assert!(!jita_roman_res.is_empty());
    assert_eq!(jita_roman_res[0].system_name, "Jita");

    let dodixie_roman_res = sde.search_systems("Dodixie IX", 5);
    assert!(!dodixie_roman_res.is_empty());
    assert_eq!(dodixie_roman_res[0].system_name, "Dodixie");
}

#[tokio::test]
async fn test_sde_station_canonical_name_and_system_resolution() {
    let sde_path = PathBuf::from("data").join("eve_sde.sqlite");
    if !sde_path.exists() {
        return;
    }

    let sde = SdeLoader::open(&sde_path, 30)
        .await
        .expect("Failed to open SDE");

    // 1. Station name canonicalization on read (station 60003760 is Jita 4-4)
    let sta_name = sde
        .get_station_name(60003760)
        .expect("Station 60003760 should exist");
    assert_eq!(
        sta_name, "Jita IV - Moon 4 - Caldari Navy Assembly Plant",
        "get_station_name must return canonical in-game format with Roman planet numeral"
    );

    // 2. Exact system resolution
    assert_eq!(sde.get_system_id("Jita"), Some(30000142));
    assert_eq!(sde.get_system_id("  jita  "), Some(30000142));
    assert_eq!(sde.get_system_id("\"Jita\""), Some(30000142));

    // 3. Full in-game station name with Roman numerals (copied from game client)
    assert_eq!(
        sde.get_system_id("Jita IV - Moon 4 - Caldari Navy Assembly Plant"),
        Some(30000142)
    );
    assert_eq!(
        sde.get_system_id("Dodixie IX - Moon 20 - Federation Navy Assembly Plant"),
        Some(30002659)
    );
    assert_eq!(
        sde.get_system_id("Amarr VIII (Oris) - Emperor Family Academy"),
        Some(30002187)
    );

    // 4. SDE station name with Arabic numbers
    assert_eq!(
        sde.get_system_id("Jita 4 - Moon 4 - Caldari Navy Assembly Plant"),
        Some(30000142)
    );

    // 5. Hub shorthand variations
    assert_eq!(sde.get_system_id("Jita 4-4"), Some(30000142));
    assert_eq!(sde.get_system_id("Jita IV-4"), Some(30000142));
    assert_eq!(sde.get_system_id("Jita IV - 4"), Some(30000142));
    assert_eq!(sde.get_system_id("Jita 4 - 4"), Some(30000142));
    assert_eq!(sde.get_system_id("Jita IV"), Some(30000142));
    assert_eq!(sde.get_system_id("Jita 4"), Some(30000142));
    assert_eq!(sde.get_system_id("Dodixie IX-20"), Some(30002659));
    assert_eq!(sde.get_system_id("Dodixie 9-20"), Some(30002659));
    assert_eq!(sde.get_system_id("Rens VI-8"), Some(30002510));
    assert_eq!(sde.get_system_id("Hek VIII-12"), Some(30002053));

    // 6. Multi-word solar systems
    assert_eq!(
        sde.get_system_id("New Caldari IV - Moon 1 - Poksu Mineral Group Mineral Reserve"),
        Some(30000145)
    );
    assert_eq!(sde.get_system_id("New Caldari 4-1"), Some(30000145));
    assert_eq!(sde.get_system_id("New Caldari IV-1"), Some(30000145));
    assert_eq!(
        sde.get_system_id("Old Man Star V - Moon 1 - Federal Freight Storage"),
        Some(30005000)
    );

    // 7. Security rating prefix (copied from chat / overview)
    assert_eq!(
        sde.get_system_id("[1.0] Jita IV - Moon 4 - Caldari Navy Assembly Plant"),
        Some(30000142)
    );
    assert_eq!(sde.get_system_id("[0.9] Amarr"), Some(30002187));

    // 8. Player structure in known system
    assert_eq!(
        sde.get_system_id("Perimeter - Tranquility Trading Tower"),
        Some(30000144)
    );
}
