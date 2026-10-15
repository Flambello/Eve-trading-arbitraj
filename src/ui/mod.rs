pub mod gui;
pub mod tui;

pub use gui::{run_desktop_gui, DesktopApp, StartupMsg, StartupProgress};
#[allow(deprecated)]
pub use tui::run_tui;

/// Formats an ISK unit price into a human-readable abbreviation (e.g. 1.25B, 45.00M, 500.00k, 12.50).
pub fn format_unit_price(val: f64) -> String {
    if val >= 1_000_000_000.0 {
        format!("{:.2}B", val / 1_000_000_000.0)
    } else if val >= 1_000_000.0 {
        format!("{:.2}M", val / 1_000_000.0)
    } else if val >= 1_000.0 {
        format!("{:.2}k", val / 1_000.0)
    } else {
        format!("{:.2}", val)
    }
}
