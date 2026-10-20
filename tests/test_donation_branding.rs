use eve_trade_scout::ui::gui::{
    format_donation_instructions, APP_ICON_PNG_BYTES, CORP_DONATION_RECIPIENT, CORP_ICON_PNG_BYTES,
};
use image::GenericImageView;

#[test]
fn test_donation_recipient_and_instruction_formatting() {
    assert!(
        !CORP_DONATION_RECIPIENT.is_empty(),
        "Donation recipient corporation must not be empty"
    );

    // Verify formatting of general and specific in-game donation instructions incorporates CORP_DONATION_RECIPIENT
    let general_instructions = format_donation_instructions(None);
    assert!(
        general_instructions.contains(CORP_DONATION_RECIPIENT),
        "General donation instructions must mention CORP_DONATION_RECIPIENT"
    );
    assert_eq!(
        general_instructions,
        format!(
            "Send ISK donation to corporation: {}",
            CORP_DONATION_RECIPIENT
        )
    );

    let specific_instructions = format_donation_instructions(Some(50));
    assert!(
        specific_instructions.contains(CORP_DONATION_RECIPIENT),
        "Specific donation instructions must mention CORP_DONATION_RECIPIENT"
    );
    assert_eq!(
        specific_instructions,
        format!(
            "Send 50M ISK donation to corporation: {}",
            CORP_DONATION_RECIPIENT
        )
    );
}

#[test]
fn test_embedded_corp_icon_bytes_valid_png() {
    assert!(
        !CORP_ICON_PNG_BYTES.is_empty(),
        "Embedded corp icon bytes must not be empty"
    );

    // PNG file magic header check: 0x89, 'P', 'N', 'G', 0x0D, 0x0A, 0x1A, 0x0A
    let png_magic = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    assert!(
        CORP_ICON_PNG_BYTES.starts_with(&png_magic),
        "Embedded corp icon must start with valid PNG magic header"
    );

    // Decode and verify image metadata
    let img = image::load_from_memory(CORP_ICON_PNG_BYTES)
        .expect("Embedded corp icon must decode into a valid image");

    let (width, height) = img.dimensions();
    assert_eq!(width, 124, "Corp icon width must be 124px");
    assert_eq!(height, 124, "Corp icon height must be 124px");

    let rgba = img.to_rgba8();
    assert_eq!(
        rgba.as_raw().len(),
        124 * 124 * 4,
        "Decoded RGBA buffer size must match width * height * 4"
    );
}

#[test]
fn test_disk_asset_matches_embedded_bytes() {
    let disk_bytes = std::fs::read("assets/icons/expandabliss.png")
        .expect("assets/icons/expandabliss.png must exist on disk");
    assert_eq!(
        disk_bytes.as_slice(),
        CORP_ICON_PNG_BYTES,
        "Disk asset must match embedded compile-time byte slice"
    );
}

#[test]
fn test_embedded_app_icon_bytes_valid_png() {
    assert!(
        !APP_ICON_PNG_BYTES.is_empty(),
        "Embedded app icon bytes must not be empty"
    );

    let png_magic = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    assert!(
        APP_ICON_PNG_BYTES.starts_with(&png_magic),
        "Embedded app icon must start with valid PNG magic header"
    );

    let img = image::load_from_memory(APP_ICON_PNG_BYTES)
        .expect("Embedded app icon must decode into a valid image");

    let (width, height) = img.dimensions();
    assert_eq!(width, 256, "App icon width must be 256px");
    assert_eq!(height, 256, "App icon height must be 256px");

    let rgba = img.to_rgba8();
    assert_eq!(
        rgba.as_raw().len(),
        256 * 256 * 4,
        "Decoded RGBA buffer size must match width * height * 4"
    );
}

#[test]
fn test_embedded_app_icon_decodes_via_eframe_icon_data() {
    let icon_res = eframe::icon_data::from_png_bytes(APP_ICON_PNG_BYTES);
    assert!(
        icon_res.is_ok(),
        "eframe::icon_data::from_png_bytes must successfully parse embedded app icon"
    );

    let icon = icon_res.unwrap();
    assert_eq!(icon.width, 256, "eframe IconData width must be 256");
    assert_eq!(icon.height, 256, "eframe IconData height must be 256");
    assert_eq!(
        icon.rgba.len(),
        256 * 256 * 4,
        "eframe IconData rgba byte length must equal width * height * 4"
    );
}

#[test]
fn test_app_icon_disk_asset_matches_embedded_bytes() {
    let disk_bytes = std::fs::read("assets/icons/io.github.flambello.eve_merchant.png")
        .expect("assets/icons/io.github.flambello.eve_merchant.png must exist on disk");
    assert_eq!(
        disk_bytes.as_slice(),
        APP_ICON_PNG_BYTES,
        "Disk asset must match embedded compile-time byte slice for app icon"
    );
}
