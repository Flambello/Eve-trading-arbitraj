use eve_trade_scout::error::ErrorCode;
use std::collections::HashSet;

const ALL_ERROR_CODES: &[ErrorCode] = &[
    ErrorCode::ErrSdeNotFound,
    ErrorCode::ErrSdeDownloadFailed,
    ErrorCode::ErrSdeDecompressionFailed,
    ErrorCode::ErrSdeDecompressionBomb,
    ErrorCode::ErrSdeSchemaInvalid,
    ErrorCode::ErrSdeQueryFailed,
    ErrorCode::ErrSdeSystemNotFound,
    ErrorCode::ErrSdeStationNotFound,
    ErrorCode::ErrEsiClientInit,
    ErrorCode::ErrEsiNetworkFailed,
    ErrorCode::ErrEsiRateLimited,
    ErrorCode::ErrEsiServerError,
    ErrorCode::ErrEsiClientError,
    ErrorCode::ErrEsiParseFailed,
    ErrorCode::ErrEsiEtagCache,
    ErrorCode::ErrSsoConfigInvalid,
    ErrorCode::ErrSsoAuthServerFailed,
    ErrorCode::ErrSsoTokenExchange,
    ErrorCode::ErrSsoTokenRefresh,
    ErrorCode::ErrSsoTokenSave,
    ErrorCode::ErrSsoLocationFailed,
    ErrorCode::ErrSsoAutopilotFailed,
    ErrorCode::ErrGraphBuildFailed,
    ErrorCode::ErrGraphRouteUnreachable,
    ErrorCode::ErrArbConfigInvalid,
    ErrorCode::ErrArbZeroCapacity,
    ErrorCode::ErrConfigLoadFailed,
    ErrorCode::ErrConfigSaveFailed,
    ErrorCode::ErrUiRuntimeInit,
    ErrorCode::ErrUiWorkerChannel,
    ErrorCode::ErrUiAudioPlayback,
    ErrorCode::ErrUiCsvExport,
    ErrorCode::ErrSysPanic,
];

#[test]
fn test_all_error_codes_have_unique_numbers() {
    let mut seen_numbers = HashSet::new();
    for &code in ALL_ERROR_CODES {
        let num = code.code_num();
        assert!(
            seen_numbers.insert(num),
            "Duplicate numeric error code detected: {} ({})",
            num,
            code.as_str()
        );
    }
    assert_eq!(
        seen_numbers.len(),
        ALL_ERROR_CODES.len(),
        "Every error code variant must have a unique numeric identifier"
    );
}

#[test]
fn test_all_error_codes_have_unique_string_identifiers() {
    let mut seen_strings = HashSet::new();
    for &code in ALL_ERROR_CODES {
        let ident = code.as_str();
        assert!(
            ident.starts_with("ERR_"),
            "Error code identifier must start with ERR_: {}",
            ident
        );
        assert!(
            ident.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
            "Error code identifier must be UPPERCASE_SNAKE_CASE: {}",
            ident
        );
        assert!(
            seen_strings.insert(ident),
            "Duplicate string identifier: {}",
            ident
        );
    }
}

#[test]
fn test_error_code_numeric_ranges_match_subsystems() {
    for &code in ALL_ERROR_CODES {
        let num = code.code_num();
        let name = code.as_str();
        match num {
            1001..=1999 => assert!(
                name.starts_with("ERR_SDE_"),
                "Code {num} in 1000 range should be SDE: {name}"
            ),
            2001..=2999 => assert!(
                name.starts_with("ERR_ESI_"),
                "Code {num} in 2000 range should be ESI: {name}"
            ),
            3001..=3999 => assert!(
                name.starts_with("ERR_SSO_"),
                "Code {num} in 3000 range should be SSO: {name}"
            ),
            4001..=4999 => assert!(
                name.starts_with("ERR_GRAPH_"),
                "Code {num} in 4000 range should be GRAPH: {name}"
            ),
            5001..=5999 => assert!(
                name.starts_with("ERR_ARB_"),
                "Code {num} in 5000 range should be ARB: {name}"
            ),
            6001..=6999 => assert!(
                name.starts_with("ERR_CONFIG_"),
                "Code {num} in 6000 range should be CONFIG: {name}"
            ),
            7001..=7999 => assert!(
                name.starts_with("ERR_UI_"),
                "Code {num} in 7000 range should be UI: {name}"
            ),
            9001..=9999 => assert!(
                name.starts_with("ERR_SYS_"),
                "Code {num} in 9000 range should be SYS: {name}"
            ),
            _ => panic!("Error code {num} ({name}) outside all known subsystem ranges!"),
        }
    }
}

#[test]
fn test_error_code_descriptions_are_non_empty() {
    for &code in ALL_ERROR_CODES {
        let desc = code.description();
        assert!(
            !desc.trim().is_empty(),
            "Error code description must not be empty for {}",
            code.as_str()
        );
        assert!(
            desc.len() >= 10,
            "Error code description should be informative (>= 10 chars): '{}' for {}",
            desc,
            code.as_str()
        );
    }
}

#[test]
fn test_error_code_display_formatting() {
    for &code in ALL_ERROR_CODES {
        assert_eq!(
            format!("{}", code),
            code.as_str(),
            "Display impl must match as_str() exactly"
        );
    }
}

#[test]
fn test_error_code_serde_json_roundtrip() {
    for &code in ALL_ERROR_CODES {
        let json_str =
            serde_json::to_string(&code).expect("ErrorCode should serialize to JSON successfully");

        // Should be serialized as screaming snake string: e.g. "\"ERR_ESI_NETWORK_FAILED\""
        let expected_json = format!("\"{}\"", code.as_str());
        assert_eq!(
            json_str, expected_json,
            "JSON representation must match as_str() quoted"
        );

        let deserialized: ErrorCode = serde_json::from_str(&json_str)
            .expect("ErrorCode should deserialize from JSON successfully");
        assert_eq!(
            deserialized, code,
            "Deserialized ErrorCode must match original variant"
        );
    }

    // Invalid string should return Deserialization error
    let invalid_res: Result<ErrorCode, _> = serde_json::from_str("\"NON_EXISTENT_ERROR_CODE\"");
    assert!(
        invalid_res.is_err(),
        "Invalid error code string must fail deserialization"
    );
}
