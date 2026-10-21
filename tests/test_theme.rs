use eve_trade_scout::config::AppTheme;
use eve_trade_scout::ui::theme::theme_palette;
use eframe::egui::{self, Color32};

#[test]
fn test_all_themes_metadata_and_distinctions() {
    let mut names = std::collections::HashSet::new();
    let mut descriptions = std::collections::HashSet::new();
    let mut as_strs = std::collections::HashSet::new();

    for &theme in AppTheme::ALL {
        let name = theme.display_name();
        let desc = theme.description();
        let s = theme.as_str();

        assert!(!name.is_empty(), "Theme name cannot be empty");
        assert!(!desc.is_empty(), "Theme description cannot be empty");
        assert!(!s.is_empty(), "Theme as_str identifier cannot be empty");

        assert!(names.insert(name), "Theme names must be unique: {}", name);
        assert!(descriptions.insert(desc), "Theme descriptions must be unique: {}", desc);
        assert!(as_strs.insert(s), "Theme identifiers must be unique: {}", s);
    }
}

#[test]
fn test_theme_dark_mode_classifications() {
    // 3 dark themes, 1 light theme
    assert!(AppTheme::SciFiVoid.is_dark());
    assert!(AppTheme::NeutralBlack.is_dark());
    assert!(AppTheme::BlackRed.is_dark());
    assert!(!AppTheme::SolarLight.is_dark());

    // Palettes must mirror is_dark()
    for &theme in AppTheme::ALL {
        let palette = theme.palette();
        assert_eq!(palette.dark_mode, theme.is_dark(), "Palette dark_mode must match theme.is_dark() for {:?}", theme);
    }
}

#[test]
fn test_theme_swatches_and_colors() {
    for &theme in AppTheme::ALL {
        let palette = theme.palette();
        let (bg, p_acc, s_acc) = palette.preview_swatches();

        assert_eq!(bg, palette.card_bg);
        assert_eq!(p_acc, palette.accent_primary);
        assert_eq!(s_acc, palette.accent_secondary);

        // Verify non-zero alpha and reasonable color components
        assert_eq!(bg.a(), 255);
        assert_eq!(p_acc.a(), 255);
        assert_eq!(s_acc.a(), 255);

        // Accent colors must be visually distinct from the card background
        assert_ne!(bg, p_acc, "Primary accent must not match background for {:?}", theme);
        assert_ne!(bg, s_acc, "Secondary accent must not match background for {:?}", theme);
        assert_ne!(p_acc, s_acc, "Primary and secondary accents must be distinct for {:?}", theme);
    }
}

#[test]
fn test_theme_text_contrast_against_background() {
    fn luminance(c: Color32) -> f32 {
        0.2126 * (c.r() as f32 / 255.0) + 0.7152 * (c.g() as f32 / 255.0) + 0.0722 * (c.b() as f32 / 255.0)
    }

    for &theme in AppTheme::ALL {
        let palette = theme.palette();
        let text_lum = luminance(palette.text_primary);
        let bg_lum = luminance(palette.bg_central);
        let card_lum = luminance(palette.card_bg);

        let contrast_central = (text_lum - bg_lum).abs();
        let contrast_card = (text_lum - card_lum).abs();

        // Ensure significant luminance contrast between text and background
        assert!(
            contrast_central > 0.40,
            "Insufficient central contrast ({:.2}) for theme {:?}",
            contrast_central,
            theme
        );
        assert!(
            contrast_card > 0.40,
            "Insufficient card contrast ({:.2}) for theme {:?}",
            contrast_card,
            theme
        );

        if theme.is_dark() {
            // Dark themes: text is brighter than background
            assert!(text_lum > bg_lum, "Text must be brighter than background in dark theme {:?}", theme);
        } else {
            // Light themes: text is darker than background
            assert!(text_lum < bg_lum, "Text must be darker than background in light theme {:?}", theme);
        }
    }
}

#[test]
fn test_theme_frame_generators() {
    for &theme in AppTheme::ALL {
        let palette = theme_palette(theme);

        let card_frame = palette.card_frame();
        assert_eq!(card_frame.fill, palette.card_bg);
        assert_eq!(card_frame.stroke, palette.card_stroke);

        let sub_card_frame = palette.sub_card_frame();
        assert_eq!(sub_card_frame.fill, palette.card_sub_bg);
        assert_eq!(sub_card_frame.stroke, palette.card_sub_stroke);

        let top_frame = palette.top_panel_frame();
        assert_eq!(top_frame.fill, palette.bg_panel);
        assert_eq!(top_frame.stroke, palette.bg_panel_stroke);

        let bottom_frame = palette.bottom_panel_frame();
        assert_eq!(bottom_frame.fill, palette.bg_panel);
        assert_eq!(bottom_frame.stroke, palette.bg_panel_stroke);

        let central_frame = palette.central_panel_frame();
        assert_eq!(central_frame.fill, palette.bg_central);
    }
}

#[test]
fn test_theme_apply_to_ctx_without_panic() {
    let ctx = egui::Context::default();

    for &theme in AppTheme::ALL {
        let palette = theme.palette();
        palette.apply_to_ctx(&ctx);

        let style = ctx.style();
        assert_eq!(
            style.visuals.dark_mode, palette.dark_mode,
            "ctx dark_mode must match palette dark_mode for {:?}",
            theme
        );
        assert_eq!(style.visuals.panel_fill, palette.bg_panel);
        assert_eq!(style.visuals.window_fill, palette.bg_central);
        assert_eq!(style.visuals.extreme_bg_color, palette.bg_app);
        assert_eq!(style.visuals.override_text_color, Some(palette.text_secondary));
    }
}

#[test]
fn test_black_red_and_solar_light_refinements() {
    // 1. Black & Red: Pure Obsidian Chassis + Vibrant Crimson Accents (not brownish/solarized)
    let br = AppTheme::BlackRed.palette();
    // Obsidian black surfaces
    assert!(br.bg_app.r() <= 10 && br.bg_app.g() <= 10 && br.bg_app.b() <= 10, "bg_app must be deep black");
    assert!(br.card_bg.r() <= 18 && br.card_bg.g() <= 18 && br.card_bg.b() <= 18, "card_bg must be deep black");
    // Vibrant crimson accent (r is high, green and blue low, no brownish undertone)
    assert!(br.accent_primary.r() >= 230, "accent_primary must be bright red");
    assert!(br.accent_primary.g() <= 60, "accent_primary green component must be low to avoid solarized/orange tint");
    assert!(br.accent_primary.b() <= 70, "accent_primary blue component must be low");
    // Card stroke is crimson
    assert!(br.card_stroke.color.r() >= 150, "card_stroke must be crimson");

    // 2. Solar Light: High-clarity typography with solid black-on-white text
    let sl = AppTheme::SolarLight.palette();
    // Pure black text for maximum sharpness
    assert_eq!(sl.text_primary, Color32::from_rgb(0, 0, 0), "text_primary must be pure black");
    assert_eq!(sl.card_bg, Color32::from_rgb(255, 255, 255), "card_bg must be pure white");
    // Near-black secondary text
    assert!(sl.text_secondary.r() <= 25 && sl.text_secondary.g() <= 25 && sl.text_secondary.b() <= 25);
    // Dark mode must be false
    assert!(!sl.dark_mode);

    // 3. Context styling contrast checks
    let ctx = egui::Context::default();
    sl.apply_to_ctx(&ctx);
    assert_eq!(ctx.style().visuals.widgets.active.fg_stroke.color, Color32::BLACK);

    br.apply_to_ctx(&ctx);
    assert_eq!(ctx.style().visuals.widgets.active.fg_stroke.color, Color32::WHITE);
}

#[test]
fn test_accent_button_text_and_badge_contrasts() {
    fn luminance(c: Color32) -> f32 {
        0.2126 * (c.r() as f32 / 255.0) + 0.7152 * (c.g() as f32 / 255.0) + 0.0722 * (c.b() as f32 / 255.0)
    }

    for &theme in AppTheme::ALL {
        let palette = theme.palette();

        // 1. Primary button text contrast
        let p_acc_lum = luminance(palette.accent_primary);
        let p_txt_lum = luminance(palette.accent_primary_text);
        let p_contrast = (p_acc_lum - p_txt_lum).abs();
        assert!(
            p_contrast >= 0.35,
            "Primary button text contrast ({:.2}) must be high for {:?}",
            p_contrast,
            theme
        );

        // 2. Secondary button text contrast
        let s_acc_lum = luminance(palette.accent_secondary);
        let s_txt_lum = luminance(palette.accent_secondary_text);
        let s_contrast = (s_acc_lum - s_txt_lum).abs();
        assert!(
            s_contrast >= 0.35,
            "Secondary button text contrast ({:.2}) must be high for {:?}",
            s_contrast,
            theme
        );

        // 3. Silver rank badge contrast against card_bg
        let silver = palette.rank_silver();
        let card_lum = luminance(palette.card_bg);
        let silver_lum = luminance(silver);
        let silver_contrast = (silver_lum - card_lum).abs();
        assert!(
            silver_contrast >= 0.35,
            "Silver badge contrast ({:.2}) must be high against card_bg for {:?}",
            silver_contrast,
            theme
        );

        // 4. Primary and secondary button helpers construct and render buttons without panic
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let _ = ui.add(palette.primary_button("Test Push"));
                let _ = ui.add(palette.secondary_button("Test Copy"));
            });
        });
    }
}

#[test]
fn test_hub_badge_colors_across_themes() {
    use eve_trade_scout::hub_arbitrage::CoreHub;

    fn luminance(c: Color32) -> f32 {
        0.2126 * (c.r() as f32 / 255.0) + 0.7152 * (c.g() as f32 / 255.0) + 0.0722 * (c.b() as f32 / 255.0)
    }

    for &theme in AppTheme::ALL {
        let palette = theme.palette();
        let card_lum = luminance(palette.card_bg);

        for &hub in &CoreHub::ALL {
            let badge_color = palette.hub_badge_color(Some(hub));
            let badge_lum = luminance(badge_color);
            let contrast = (badge_lum - card_lum).abs();

            assert!(
                contrast >= 0.20,
                "Hub badge for {:?} in theme {:?} has insufficient contrast ({:.2}) against card_bg",
                hub,
                theme,
                contrast
            );
        }

        // Wildcard / None hub badge
        let none_color = palette.hub_badge_color(None);
        assert_eq!(none_color, palette.text_muted);
    }
}

#[test]
fn test_semantic_indicators_contrast_and_distinction() {
    fn luminance(c: Color32) -> f32 {
        0.2126 * (c.r() as f32 / 255.0) + 0.7152 * (c.g() as f32 / 255.0) + 0.0722 * (c.b() as f32 / 255.0)
    }

    for &theme in AppTheme::ALL {
        let palette = theme.palette();
        let card_lum = luminance(palette.card_bg);

        let indicators = [
            ("success_green", palette.success_green),
            ("warning_yellow", palette.warning_yellow),
            ("error_red", palette.error_red),
            ("info_blue", palette.info_blue),
        ];

        // 1. Contrast against card background (at least 0.25 luminance delta)
        for (name, color) in indicators {
            let color_lum = luminance(color);
            let contrast = (color_lum - card_lum).abs();
            assert!(
                contrast >= 0.25,
                "Semantic indicator '{}' in theme {:?} has insufficient contrast ({:.2}) against card_bg",
                name,
                theme,
                contrast
            );
        }

        // 2. Mutual distinction between semantic colors within the theme
        assert_ne!(palette.success_green, palette.warning_yellow);
        assert_ne!(palette.success_green, palette.error_red);
        assert_ne!(palette.success_green, palette.info_blue);
        assert_ne!(palette.warning_yellow, palette.error_red);
        assert_ne!(palette.warning_yellow, palette.info_blue);
        assert_ne!(palette.error_red, palette.info_blue);
    }
}

#[test]
fn test_theme_display_and_from_str_fidelity() {
    use std::str::FromStr;

    for &theme in AppTheme::ALL {
        // Display format must match display_name()
        assert_eq!(format!("{}", theme), theme.display_name());

        // Canonical identifier roundtrip through FromStr
        let parsed = AppTheme::from_str(theme.as_str()).expect("Canonical as_str must parse");
        assert_eq!(parsed, theme);

        // Display name parsed with case/space tolerance
        let parsed_display = AppTheme::from_str(theme.display_name()).expect("Display name must parse");
        assert_eq!(parsed_display, theme);
    }

    // Extended case, whitespace, and kebab-case alias tests
    assert_eq!(AppTheme::from_str("  SCI-FI-VOID  ").unwrap(), AppTheme::SciFiVoid);
    assert_eq!(AppTheme::from_str("neutral-black").unwrap(), AppTheme::NeutralBlack);
    assert_eq!(AppTheme::from_str("Black-And-Red").unwrap(), AppTheme::BlackRed);
    assert_eq!(AppTheme::from_str("  triglavian ").unwrap(), AppTheme::BlackRed);
    assert_eq!(AppTheme::from_str("solar-light").unwrap(), AppTheme::SolarLight);
    assert_eq!(AppTheme::from_str("Daylight").unwrap(), AppTheme::SolarLight);

    // Invalid theme names
    assert!(AppTheme::from_str("cyberpunk_neon").is_err());
    assert!(AppTheme::from_str("").is_err());
}

