use crate::config::AppTheme;
use crate::hub_arbitrage::CoreHub;
use eframe::egui::{self, Color32, Stroke, Vec2};

/// Complete color palette tokens and frame builders for the desktop GUI.
#[derive(Debug, Clone)]
pub struct ThemePalette {
    /// True if dark mode theme (Sci-Fi Void, Neutral Black, Black & Red); false if light mode (Solar Light).
    pub dark_mode: bool,

    // Base surface fills
    pub bg_app: Color32,
    pub bg_panel: Color32,
    pub bg_panel_stroke: Stroke,
    pub bg_central: Color32,

    // Card & container frames
    pub card_bg: Color32,
    pub card_stroke: Stroke,
    pub card_sub_bg: Color32,
    pub card_sub_stroke: Stroke,

    // Widget states
    pub widget_noninteractive_bg: Color32,
    pub widget_noninteractive_stroke: Stroke,
    pub widget_inactive_bg: Color32,
    pub widget_inactive_stroke: Stroke,
    pub widget_hovered_bg: Color32,
    pub widget_hovered_stroke: Stroke,
    pub widget_active_bg: Color32,
    pub widget_active_stroke: Stroke,

    // Selection
    pub selection_bg: Color32,
    pub selection_stroke: Stroke,

    // Text hierarchy
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,

    // Brand / Accent colors
    pub accent_primary: Color32,
    pub accent_secondary: Color32,
    pub accent_hover: Color32,
    pub accent_primary_text: Color32,
    pub accent_secondary_text: Color32,

    // Semantic indicators
    pub success_green: Color32,
    pub warning_yellow: Color32,
    pub error_red: Color32,
    pub info_blue: Color32,

    // Tabs & status fills
    pub tab_active_text: Color32,
    pub tab_active_fill: Color32,
    pub tab_inactive_text: Color32,
    pub toast_bg: Color32,
    pub toast_stroke: Color32,
    pub kpi_card_bg: Color32,
}

impl ThemePalette {
    /// Returns preview color swatches: (Background, Primary Accent, Secondary Accent).
    pub fn preview_swatches(&self) -> (Color32, Color32, Color32) {
        (self.card_bg, self.accent_primary, self.accent_secondary)
    }

    /// Primary card frame (settings cards, dossiers, toolbar containers).
    pub fn card_frame(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.card_bg)
            .stroke(self.card_stroke)
            .rounding(6.0)
            .inner_margin(12.0)
    }

    /// Nested sub-card frame (well containers, inner sections).
    pub fn sub_card_frame(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.card_sub_bg)
            .stroke(self.card_sub_stroke)
            .rounding(4.0)
            .inner_margin(8.0)
    }

    /// Top navigation and controls panel frame.
    pub fn top_panel_frame(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.bg_panel)
            .stroke(self.bg_panel_stroke)
            .inner_margin(12.0)
    }

    /// Bottom status bar frame.
    pub fn bottom_panel_frame(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.bg_panel)
            .stroke(self.bg_panel_stroke)
            .inner_margin(8.0)
    }

    /// Central workspace panel frame.
    pub fn central_panel_frame(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.bg_central)
            .inner_margin(12.0)
    }

    /// Returns high-contrast silver badge color adapted for dark vs light themes.
    pub fn rank_silver(&self) -> Color32 {
        if self.dark_mode {
            Color32::from_rgb(200, 215, 230)
        } else {
            Color32::from_rgb(90, 105, 125)
        }
    }

    /// Returns theme-adapted faction hub badge color with high readability.
    pub fn hub_badge_color(&self, hub: Option<CoreHub>) -> Color32 {
        if self.dark_mode {
            match hub {
                Some(CoreHub::Jita) => Color32::from_rgb(0, 190, 255),
                Some(CoreHub::Amarr) => Color32::from_rgb(255, 205, 50),
                Some(CoreHub::Dodixie) => Color32::from_rgb(50, 220, 120),
                Some(CoreHub::Rens) => Color32::from_rgb(240, 80, 60),
                Some(CoreHub::Hek) => Color32::from_rgb(240, 130, 60),
                None => self.text_muted,
            }
        } else {
            match hub {
                Some(CoreHub::Jita) => Color32::from_rgb(0, 115, 200),
                Some(CoreHub::Amarr) => Color32::from_rgb(175, 105, 0),
                Some(CoreHub::Dodixie) => Color32::from_rgb(16, 135, 55),
                Some(CoreHub::Rens) => Color32::from_rgb(195, 35, 30),
                Some(CoreHub::Hek) => Color32::from_rgb(180, 75, 15),
                None => self.text_muted,
            }
        }
    }

    /// Primary action button (e.g. Autopilot Push or Main Scan).
    pub fn primary_button<'a>(&self, text: impl Into<egui::WidgetText>) -> egui::Button<'a> {
        egui::Button::new(text).fill(self.accent_primary)
    }

    /// Secondary action button (e.g. Flight Sheet or Notepad copy).
    pub fn secondary_button<'a>(&self, text: impl Into<egui::WidgetText>) -> egui::Button<'a> {
        egui::Button::new(text).fill(self.accent_secondary)
    }

    /// Applies this palette to the `egui::Context` visuals and styling.
    pub fn apply_to_ctx(&self, ctx: &egui::Context) {
        let mut visuals = if self.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };

        visuals.panel_fill = self.bg_panel;
        visuals.window_fill = self.bg_central;
        visuals.extreme_bg_color = self.bg_app;
        visuals.override_text_color = Some(self.text_secondary);

        visuals.widgets.noninteractive.bg_fill = self.widget_noninteractive_bg;
        visuals.widgets.noninteractive.bg_stroke = self.widget_noninteractive_stroke;
        visuals.widgets.noninteractive.fg_stroke.color = self.text_primary;

        visuals.widgets.inactive.bg_fill = self.widget_inactive_bg;
        visuals.widgets.inactive.bg_stroke = self.widget_inactive_stroke;
        visuals.widgets.inactive.fg_stroke.color = self.text_primary;

        visuals.widgets.hovered.bg_fill = self.widget_hovered_bg;
        visuals.widgets.hovered.bg_stroke = self.widget_hovered_stroke;
        visuals.widgets.hovered.fg_stroke.color = self.text_primary;

        visuals.widgets.active.bg_fill = self.widget_active_bg;
        visuals.widgets.active.bg_stroke = self.widget_active_stroke;
        visuals.widgets.active.fg_stroke.color = if self.dark_mode {
            Color32::WHITE
        } else {
            Color32::BLACK
        };

        visuals.selection.bg_fill = self.selection_bg;
        visuals.selection.stroke = self.selection_stroke;
        visuals.slider_trailing_fill = true;

        ctx.set_visuals(visuals);

        let mut style = (*ctx.style()).clone();
        style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(4.0);
        style.visuals.widgets.inactive.rounding = egui::Rounding::same(4.0);
        style.visuals.widgets.hovered.rounding = egui::Rounding::same(4.0);
        style.visuals.widgets.active.rounding = egui::Rounding::same(4.0);
        style.spacing.item_spacing = Vec2::new(8.0, 5.0);
        style.spacing.slider_rail_height = 5.0;
        ctx.set_style(style);
    }
}

/// Resolves a complete `ThemePalette` from an `AppTheme`.
pub fn theme_palette(theme: AppTheme) -> ThemePalette {
    match theme {
        AppTheme::SciFiVoid => ThemePalette {
            dark_mode: true,
            bg_app: Color32::from_rgb(5, 7, 18),
            bg_panel: Color32::from_rgb(13, 18, 30),
            bg_panel_stroke: Stroke::new(1.0_f32, Color32::from_rgb(30, 42, 68)),
            bg_central: Color32::from_rgb(15, 20, 32),

            card_bg: Color32::from_rgb(18, 25, 40),
            card_stroke: Stroke::new(1.0_f32, Color32::from_rgb(32, 45, 72)),
            card_sub_bg: Color32::from_rgb(14, 20, 32),
            card_sub_stroke: Stroke::new(1.0_f32, Color32::from_rgb(26, 38, 60)),

            widget_noninteractive_bg: Color32::from_rgb(14, 19, 38),
            widget_noninteractive_stroke: Stroke::new(1.0_f32, Color32::from_rgb(28, 40, 65)),
            widget_inactive_bg: Color32::from_rgb(18, 26, 48),
            widget_inactive_stroke: Stroke::new(1.0_f32, Color32::from_rgb(0, 180, 240)),
            widget_hovered_bg: Color32::from_rgb(28, 40, 72),
            widget_hovered_stroke: Stroke::new(1.5_f32, Color32::from_rgb(0, 220, 255)),
            widget_active_bg: Color32::from_rgb(0, 160, 220),
            widget_active_stroke: Stroke::new(2.0_f32, Color32::from_rgb(0, 255, 200)),

            selection_bg: Color32::from_rgb(0, 170, 230),
            selection_stroke: Stroke::new(1.0_f32, Color32::from_rgb(0, 230, 255)),

            text_primary: Color32::from_rgb(240, 248, 255),
            text_secondary: Color32::from_rgb(200, 220, 245),
            text_muted: Color32::from_rgb(130, 155, 190),

            accent_primary: Color32::from_rgb(0, 225, 255),
            accent_secondary: Color32::from_rgb(255, 205, 0),
            accent_hover: Color32::from_rgb(0, 245, 255),
            accent_primary_text: Color32::BLACK,
            accent_secondary_text: Color32::BLACK,

            success_green: Color32::from_rgb(0, 235, 170),
            warning_yellow: Color32::from_rgb(255, 200, 0),
            error_red: Color32::from_rgb(255, 80, 80),
            info_blue: Color32::from_rgb(0, 200, 255),

            tab_active_text: Color32::from_rgb(0, 220, 255),
            tab_active_fill: Color32::from_rgba_unmultiplied(0, 180, 255, 22),
            tab_inactive_text: Color32::from_rgb(130, 165, 205),
            toast_bg: Color32::from_rgba_unmultiplied(22, 28, 44, 235),
            toast_stroke: Color32::from_rgb(218, 165, 32),
            kpi_card_bg: Color32::from_rgb(16, 26, 46),
        },
        AppTheme::NeutralBlack => ThemePalette {
            dark_mode: true,
            bg_app: Color32::from_rgb(8, 8, 8),
            bg_panel: Color32::from_rgb(14, 14, 14),
            bg_panel_stroke: Stroke::new(1.0_f32, Color32::from_rgb(32, 32, 32)),
            bg_central: Color32::from_rgb(16, 16, 16),

            card_bg: Color32::from_rgb(24, 24, 24),
            card_stroke: Stroke::new(1.0_f32, Color32::from_rgb(44, 44, 44)),
            card_sub_bg: Color32::from_rgb(18, 18, 18),
            card_sub_stroke: Stroke::new(1.0_f32, Color32::from_rgb(36, 36, 36)),

            widget_noninteractive_bg: Color32::from_rgb(20, 20, 20),
            widget_noninteractive_stroke: Stroke::new(1.0_f32, Color32::from_rgb(38, 38, 38)),
            widget_inactive_bg: Color32::from_rgb(28, 28, 28),
            widget_inactive_stroke: Stroke::new(1.0_f32, Color32::from_rgb(70, 70, 70)),
            widget_hovered_bg: Color32::from_rgb(44, 44, 44),
            widget_hovered_stroke: Stroke::new(1.5_f32, Color32::from_rgb(130, 130, 130)),
            widget_active_bg: Color32::from_rgb(64, 64, 64),
            widget_active_stroke: Stroke::new(2.0_f32, Color32::from_rgb(200, 200, 200)),

            selection_bg: Color32::from_rgb(50, 50, 50),
            selection_stroke: Stroke::new(1.0_f32, Color32::from_rgb(160, 160, 160)),

            text_primary: Color32::from_rgb(250, 250, 250),
            text_secondary: Color32::from_rgb(215, 215, 215),
            text_muted: Color32::from_rgb(140, 140, 140),

            accent_primary: Color32::from_rgb(230, 235, 242),
            accent_secondary: Color32::from_rgb(160, 175, 195),
            accent_hover: Color32::WHITE,
            accent_primary_text: Color32::BLACK,
            accent_secondary_text: Color32::BLACK,

            success_green: Color32::from_rgb(52, 211, 153),
            warning_yellow: Color32::from_rgb(251, 191, 36),
            error_red: Color32::from_rgb(248, 113, 113),
            info_blue: Color32::from_rgb(96, 165, 250),

            tab_active_text: Color32::WHITE,
            tab_active_fill: Color32::from_rgba_unmultiplied(255, 255, 255, 24),
            tab_inactive_text: Color32::from_rgb(160, 160, 160),
            toast_bg: Color32::from_rgba_unmultiplied(26, 26, 26, 240),
            toast_stroke: Color32::from_rgb(100, 100, 100),
            kpi_card_bg: Color32::from_rgb(22, 22, 22),
        },
        AppTheme::BlackRed => ThemePalette {
            dark_mode: true,
            bg_app: Color32::from_rgb(5, 5, 5),
            bg_panel: Color32::from_rgb(10, 10, 10),
            bg_panel_stroke: Stroke::new(1.0_f32, Color32::from_rgb(140, 15, 28)),
            bg_central: Color32::from_rgb(8, 8, 8),

            card_bg: Color32::from_rgb(14, 14, 14),
            card_stroke: Stroke::new(1.0_f32, Color32::from_rgb(180, 25, 42)),
            card_sub_bg: Color32::from_rgb(18, 18, 18),
            card_sub_stroke: Stroke::new(1.0_f32, Color32::from_rgb(90, 12, 22)),

            widget_noninteractive_bg: Color32::from_rgb(12, 12, 12),
            widget_noninteractive_stroke: Stroke::new(1.0_f32, Color32::from_rgb(45, 12, 18)),
            widget_inactive_bg: Color32::from_rgb(18, 18, 18),
            widget_inactive_stroke: Stroke::new(1.0_f32, Color32::from_rgb(200, 25, 45)),
            widget_hovered_bg: Color32::from_rgb(34, 12, 16),
            widget_hovered_stroke: Stroke::new(1.5_f32, Color32::from_rgb(255, 45, 65)),
            widget_active_bg: Color32::from_rgb(215, 20, 42),
            widget_active_stroke: Stroke::new(2.0_f32, Color32::from_rgb(255, 75, 95)),

            selection_bg: Color32::from_rgb(150, 20, 36),
            selection_stroke: Stroke::new(1.0_f32, Color32::from_rgb(255, 45, 65)),

            text_primary: Color32::from_rgb(255, 255, 255),
            text_secondary: Color32::from_rgb(230, 230, 230),
            text_muted: Color32::from_rgb(160, 160, 160),

            accent_primary: Color32::from_rgb(255, 30, 55),
            accent_secondary: Color32::from_rgb(255, 75, 95),
            accent_hover: Color32::from_rgb(255, 70, 90),
            accent_primary_text: Color32::WHITE,
            accent_secondary_text: Color32::BLACK,

            success_green: Color32::from_rgb(46, 204, 113),
            warning_yellow: Color32::from_rgb(255, 180, 0),
            error_red: Color32::from_rgb(255, 30, 50),
            info_blue: Color32::from_rgb(255, 65, 85),

            tab_active_text: Color32::from_rgb(255, 45, 65),
            tab_active_fill: Color32::from_rgba_unmultiplied(255, 30, 55, 32),
            tab_inactive_text: Color32::from_rgb(160, 160, 160),
            toast_bg: Color32::from_rgba_unmultiplied(16, 16, 16, 245),
            toast_stroke: Color32::from_rgb(255, 30, 55),
            kpi_card_bg: Color32::from_rgb(14, 14, 14),
        },
        AppTheme::SolarLight => ThemePalette {
            dark_mode: false,
            bg_app: Color32::from_rgb(240, 242, 246),
            bg_panel: Color32::from_rgb(255, 255, 255),
            bg_panel_stroke: Stroke::new(1.0_f32, Color32::from_rgb(205, 212, 222)),
            bg_central: Color32::from_rgb(245, 247, 250),

            card_bg: Color32::from_rgb(255, 255, 255),
            card_stroke: Stroke::new(1.0_f32, Color32::from_rgb(200, 208, 220)),
            card_sub_bg: Color32::from_rgb(246, 248, 251),
            card_sub_stroke: Stroke::new(1.0_f32, Color32::from_rgb(215, 222, 232)),

            widget_noninteractive_bg: Color32::from_rgb(242, 245, 249),
            widget_noninteractive_stroke: Stroke::new(1.0_f32, Color32::from_rgb(200, 208, 220)),
            widget_inactive_bg: Color32::from_rgb(255, 255, 255),
            widget_inactive_stroke: Stroke::new(1.0_f32, Color32::from_rgb(185, 195, 210)),
            widget_hovered_bg: Color32::from_rgb(234, 240, 248),
            widget_hovered_stroke: Stroke::new(1.5_f32, Color32::from_rgb(2, 132, 199)),
            widget_active_bg: Color32::from_rgb(2, 132, 199),
            widget_active_stroke: Stroke::new(2.0_f32, Color32::from_rgb(3, 105, 161)),

            selection_bg: Color32::from_rgb(186, 230, 253),
            selection_stroke: Stroke::new(1.0_f32, Color32::from_rgb(2, 132, 199)),

            text_primary: Color32::from_rgb(0, 0, 0),
            text_secondary: Color32::from_rgb(18, 18, 18),
            text_muted: Color32::from_rgb(85, 90, 100),

            accent_primary: Color32::from_rgb(2, 132, 199),
            accent_secondary: Color32::from_rgb(217, 119, 6),
            accent_hover: Color32::from_rgb(3, 105, 161),
            accent_primary_text: Color32::WHITE,
            accent_secondary_text: Color32::WHITE,

            success_green: Color32::from_rgb(12, 130, 78),
            warning_yellow: Color32::from_rgb(175, 95, 0),
            error_red: Color32::from_rgb(210, 30, 30),
            info_blue: Color32::from_rgb(2, 132, 199),

            tab_active_text: Color32::from_rgb(2, 132, 199),
            tab_active_fill: Color32::from_rgba_unmultiplied(2, 132, 199, 22),
            tab_inactive_text: Color32::from_rgb(60, 65, 75),
            toast_bg: Color32::from_rgba_unmultiplied(255, 255, 255, 250),
            toast_stroke: Color32::from_rgb(2, 132, 199),
            kpi_card_bg: Color32::from_rgb(255, 255, 255),
        },
    }
}

impl AppTheme {
    /// Resolves the design system theme palette.
    pub fn palette(&self) -> ThemePalette {
        theme_palette(*self)
    }
}
