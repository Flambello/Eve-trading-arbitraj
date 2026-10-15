use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "eve-merchant",
    author = "EVE Traveling Merchant",
    version = "0.1.0",
    about = "EVE Traveling Merchant — High-profit cross-station arbitrage engine & multi-stop route planner"
)]
pub struct Args {
    /// Ship profile to use: 'big' (60k m³, 500M ISK) or 'small' (30k m³, 50M ISK)
    #[arg(short, long)]
    pub ship: Option<String>,

    /// Starting solar system name (e.g. Jita, Amarr, Dodixie, Rens)
    #[arg(long)]
    pub system: Option<String>,

    /// Comma-separated region IDs to scan (e.g. 10000002 for The Forge)
    #[arg(long)]
    pub regions: Option<String>,

    /// Automatically scan stargate-connected neighbor regions of your starting system (default: true)
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub adjacent_regions: bool,

    /// Force highsec safe route only (avoid lowsec/nullsec)
    #[arg(long)]
    pub safe: bool,

    /// Allow lowsec and nullsec routes
    #[arg(long)]
    pub unsafe_routes: bool,

    /// Restrict search strictly to NPC stations (guaranteed public docking)
    #[arg(long)]
    pub npc_only: bool,

    /// Allow player-owned structures (Upwell Citadels) in search pool
    #[arg(long, aliases = ["allow-citadels", "include-structures"])]
    pub allow_structures: bool,

    /// Minimum net profit filter in ISK
    #[arg(long)]
    pub min_profit: Option<f64>,

    /// Minimum price spread percentage filter
    #[arg(long)]
    pub min_spread: Option<f64>,

    /// Minimum overall net profitability (ROI %) after all taxes (enforced default: 10.0%)
    #[arg(long)]
    pub min_roi: Option<f64>,

    /// Number of top deals to display in CLI mode
    #[arg(short, long, default_value_t = 25)]
    pub top: usize,

    /// Maximum number of stops in the planned route
    #[arg(long, default_value_t = 5)]
    pub max_stops: usize,

    /// Print planned route in CLI mode
    #[arg(long)]
    pub route: bool,

    /// Force desktop GUI window mode (default on all platforms)
    #[arg(long)]
    pub gui: bool,

    /// Automatically refresh market data every 5 minutes in GUI mode
    #[arg(long)]
    pub auto_refresh: bool,

    /// [DEPRECATED] Force terminal TUI mode (ratatui) - terminal mode is deprecated on all platforms
    #[arg(long)]
    pub tui: bool,

    /// [DEPRECATED] Disable interactive UI (both GUI and TUI) and output directly to console
    #[arg(long)]
    pub no_ui: bool,

    /// [DEPRECATED] Disable interactive TUI and output directly to console
    #[arg(long)]
    pub no_tui: bool,

    /// Export found deals to a CSV file
    #[arg(long)]
    pub export: Option<PathBuf>,

    /// Force update / re-download of the SDE database
    #[arg(long)]
    pub update_sde: bool,
}

impl Args {
    /// Determines whether the GUI should be launched based on parsed arguments and environment display state.
    ///
    /// The GUI is the default on all platforms whenever a graphical desktop environment is present.
    /// Terminal / TUI mode is deprecated and only launched if explicitly requested or on headless environments.
    pub fn should_launch_gui(&self, has_display: bool) -> bool {
        let explicitly_requested_terminal =
            self.tui || self.no_tui || self.no_ui || self.export.is_some();
        self.gui || (has_display && !explicitly_requested_terminal)
    }
}
