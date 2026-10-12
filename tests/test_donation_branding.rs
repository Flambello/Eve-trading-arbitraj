use eve_trade_scout::ui::gui::{CORP_DONATION_RECIPIENT, CORP_ICON_PNG_BYTES};
use image::GenericImageView;

#[test]
fn test_donation_recipient_constant() {
    assert_eq!(
        CORP_DONATION_RECIPIENT, "Expandabliss",
        "Donation recipient corporation must be Expandabliss"
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
