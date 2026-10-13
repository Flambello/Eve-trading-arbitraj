use crate::arbitrage::ArbitrageDeal;
use crate::config::ShipProfile;
use crate::route_planner::TradeRoute;
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table, TableState, Tabs},
    Frame, Terminal,
};
use std::io;

pub enum AppTab {
    Deals,
    Route,
    Help,
}

pub struct AppState<'a> {
    pub tab: AppTab,
    pub deals: &'a [ArbitrageDeal],
    pub route: &'a TradeRoute,
    pub ship: ShipProfile,
    pub home_system_name: String,
    pub safe_only: bool,
    pub npc_stations_only: bool,
    pub table_state: TableState,
}

impl<'a> AppState<'a> {
    pub fn new(
        deals: &'a [ArbitrageDeal],
        route: &'a TradeRoute,
        ship: ShipProfile,
        home_system_name: String,
        safe_only: bool,
        npc_stations_only: bool,
    ) -> Self {
        let mut table_state = TableState::default();
        if !deals.is_empty() {
            table_state.select(Some(0));
        }
        Self {
            tab: AppTab::Deals,
            deals,
            route,
            ship,
            home_system_name,
            safe_only,
            npc_stations_only,
            table_state,
        }
    }

    pub fn next_row(&mut self) {
        if self.deals.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.deals.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    pub fn prev_row(&mut self) {
        if self.deals.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.deals.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if let Err(e) = disable_raw_mode() {
            tracing::warn!(
                target: "eve_trade_scout",
                error = %e,
                "Failed to disable raw mode on TUI teardown"
            );
        }
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        let _ = execute!(io::stdout(), crossterm::cursor::Show);
    }
}

pub fn run_tui(
    deals: &[ArbitrageDeal],
    route: &TradeRoute,
    ship: ShipProfile,
    home_system: &str,
    safe_only: bool,
    npc_stations_only: bool,
) -> Result<()> {
    tracing::info!(
        target: "eve_trade_scout",
        deals_count = deals.len(),
        ship = %ship.name,
        home_system = %home_system,
        safe_only = safe_only,
        npc_stations_only = npc_stations_only,
        "Launching TUI interactive terminal session"
    );

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = AppState::new(
        deals,
        route,
        ship,
        home_system.to_string(),
        safe_only,
        npc_stations_only,
    );

    let res = main_loop(&mut terminal, &mut state);

    drop(_guard);
    let _ = terminal.show_cursor();

    if let Err(ref e) = res {
        tracing::error!(
            target: "eve_trade_scout",
            error_code = %crate::error::ErrorCode::ErrUiRuntimeInit,
            error = %e,
            "TUI terminal session exited with error: {e}"
        );
    } else {
        tracing::info!(
            target: "eve_trade_scout",
            "TUI terminal session terminated cleanly"
        );
    }

    res
}

fn main_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut AppState,
) -> Result<()> {
    loop {
        terminal.draw(|f| render_ui(f, state))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Tab => {
                    state.tab = match state.tab {
                        AppTab::Deals => AppTab::Route,
                        AppTab::Route => AppTab::Help,
                        AppTab::Help => AppTab::Deals,
                    };
                }
                KeyCode::Char('1') => state.tab = AppTab::Deals,
                KeyCode::Char('2') => state.tab = AppTab::Route,
                KeyCode::Char('3') => state.tab = AppTab::Help,
                KeyCode::Down | KeyCode::Char('j') => state.next_row(),
                KeyCode::Up | KeyCode::Char('k') => state.prev_row(),
                _ => {}
            }
        }
    }
}

fn render_ui(f: &mut Frame, state: &mut AppState) {
    let size = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header & Tabs
            Constraint::Min(10),   // Content
            Constraint::Length(2), // Status bar
        ])
        .split(size);

    render_header(f, chunks[0], state);

    match state.tab {
        AppTab::Deals => render_deals_table(f, chunks[1], state),
        AppTab::Route => render_route_view(f, chunks[1], state),
        AppTab::Help => render_help_view(f, chunks[1]),
    }

    render_status_bar(f, chunks[2], state);
}

fn render_header(f: &mut Frame, area: Rect, state: &AppState) {
    let titles = vec![
        Line::from(Span::raw("1: Deals Table")),
        Line::from(Span::raw("2: Planned Route")),
        Line::from(Span::raw("3: Help")),
    ];

    let current_index = match state.tab {
        AppTab::Deals => 0,
        AppTab::Route => 1,
        AppTab::Help => 2,
    };

    let title_line = format!(
        " EVE Traveling Merchant │ Ship: {} ({} m³ / {} M ISK) │ Home: {} │ Safe: {} │ Stations: {} ",
        state.ship.name,
        state.ship.max_volume,
        state.ship.max_budget / 1_000_000.0,
        state.home_system_name,
        if state.safe_only { "Highsec" } else { "Any" },
        if state.npc_stations_only { "NPC Only" } else { "+ Citadels" }
    );

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(title_line)
                .title_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .select(current_index)
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    f.render_widget(tabs, area);
}

use super::format_unit_price;

fn render_deals_table(f: &mut Frame, area: Rect, state: &mut AppState) {
    let header_cells = [
        "#",
        "Item",
        "Buy @ System",
        "Buy Price",
        "Sell @ System",
        "Sell Price",
        "Qty",
        "Vol(m³)",
        "Cost(M)",
        "Net Prof(M)",
        "ROI%",
        "Jumps",
        "Profit/Jump",
    ]
    .into_iter()
    .map(|h| {
        Cell::from(h).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    });

    let header = Row::new(header_cells)
        .style(Style::default().bg(Color::Rgb(25, 30, 45)))
        .height(1);

    let rows = state.deals.iter().enumerate().map(|(idx, d)| {
        let is_top3 = idx < 3;
        let item_style = if d.is_high_roi() {
            Style::default()
                .fg(Color::Rgb(255, 140, 0))
                .add_modifier(Modifier::BOLD)
        } else if is_top3 {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let ppj_color = if d.profit_per_jump >= 5_000_000.0 {
            Color::Rgb(0, 255, 128)
        } else if d.profit_per_jump >= 1_000_000.0 {
            Color::Cyan
        } else {
            Color::Yellow
        };

        let item_display = if d.is_high_roi() {
            format!("🔥 {}", d.item_name)
        } else {
            d.item_name.clone()
        };

        let buy_sys_text = format!("[{:.1}] {}", d.source_security, d.source_system_name);
        let sell_sys_text = format!("[{:.1}] {}", d.dest_security, d.dest_system_name);
        let jumps_text = format!(
            "{}+{}={}",
            d.jumps_from_home, d.jumps_source_dest, d.total_jumps
        );

        Row::new(vec![
            Cell::from(format!("{}", idx + 1)).style(Style::default().fg(Color::DarkGray)),
            Cell::from(item_display).style(item_style),
            Cell::from(buy_sys_text).style(Style::default().fg(Color::LightBlue)),
            Cell::from(format_unit_price(d.buy_price)).style(Style::default().fg(Color::Cyan)),
            Cell::from(sell_sys_text).style(Style::default().fg(Color::Magenta)),
            Cell::from(format_unit_price(d.sell_price))
                .style(Style::default().fg(Color::Rgb(255, 160, 240))),
            Cell::from(format!("{}", d.quantity)),
            Cell::from(format!("{:.1}", d.volume_used)),
            Cell::from(format!("{:.2}M", d.capital_required / 1_000_000.0)),
            Cell::from(format!("{:.2}M", d.net_profit / 1_000_000.0))
                .style(Style::default().fg(Color::Green)),
            Cell::from(format!("+{:.1}%", d.net_profit_pct)).style(
                Style::default()
                    .fg(Color::Rgb(0, 255, 128))
                    .add_modifier(Modifier::BOLD),
            ),
            Cell::from(jumps_text).style(Style::default().fg(Color::Gray)),
            Cell::from(format!("{:.2}M", d.profit_per_jump / 1_000_000.0))
                .style(Style::default().fg(ppj_color).add_modifier(Modifier::BOLD)),
        ])
    });

    let widths = [
        Constraint::Length(4),  // #
        Constraint::Length(24), // Item
        Constraint::Length(15), // Buy system
        Constraint::Length(10), // Buy price
        Constraint::Length(15), // Sell system
        Constraint::Length(10), // Sell price
        Constraint::Length(7),  // Qty
        Constraint::Length(8),  // Vol
        Constraint::Length(9),  // Cost
        Constraint::Length(12), // Net Profit
        Constraint::Length(8),  // ROI%
        Constraint::Length(9),  // Jumps
        Constraint::Length(12), // Profit/Jump
    ];

    let title = format!(
        " Arbitrage Deals ({}) - Ranked by Profit/Jump ",
        state.deals.len()
    );

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(title),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(40, 50, 75))
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(table, area, &mut state.table_state);
}

fn render_route_view(f: &mut Frame, area: Rect, state: &AppState) {
    let r = state.route;

    let mut text_lines = Vec::new();

    text_lines.push(Line::from(vec![Span::styled(
        format!(
            "═══ Planned Multi-Stop Route Summary ({} Stops) ═══\n",
            r.stops.len()
        ),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )]));

    text_lines.push(Line::from(vec![
        Span::raw(" Total Jumps: "),
        Span::styled(
            format!("{}", r.total_jumps),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" │ Total Net Profit: "),
        Span::styled(
            format!("{:.2} M ISK", r.total_profit / 1_000_000.0),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" │ Avg Profit / Jump: "),
        Span::styled(
            format!("{:.2} M ISK/jump", r.avg_profit_per_jump / 1_000_000.0),
            Style::default()
                .fg(Color::Rgb(0, 255, 128))
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    text_lines.push(Line::from(vec![
        Span::raw(" Capital Required: "),
        Span::styled(
            format!("{:.2} M ISK", r.total_capital_required / 1_000_000.0),
            Style::default().fg(Color::LightBlue),
        ),
        Span::raw(" │ Peak Cargo Used: "),
        Span::styled(
            format!("{:.1} / {:.1} m³", r.max_volume_used, state.ship.max_volume),
            Style::default().fg(Color::Magenta),
        ),
    ]));

    text_lines.push(Line::from(""));
    text_lines.push(Line::from(Span::styled(
        "─── Step-by-Step Waypoints & Trades ───",
        Style::default().fg(Color::Yellow),
    )));

    for stop in &r.stops {
        text_lines.push(Line::from(""));
        text_lines.push(Line::from(vec![
            Span::styled(
                format!(" [STOP {}]", stop.step),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(" Travel {} jumps to ", stop.jumps_from_prev)),
            Span::styled(
                &stop.deal.source_system_name,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        text_lines.push(Line::from(vec![
            Span::raw("   BUY:  "),
            Span::styled(
                format!("{}x {}", stop.deal.quantity, stop.deal.item_name),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(" @ {:.2} ISK/unit", stop.deal.buy_price)),
            Span::styled(
                format!(
                    " (Total: {:.2}M ISK)",
                    stop.deal.capital_required / 1_000_000.0
                ),
                Style::default().fg(Color::Gray),
            ),
        ]));
        text_lines.push(Line::from(vec![
            Span::raw("   AT:   "),
            Span::styled(
                &stop.deal.source_station_name,
                Style::default().fg(Color::DarkGray),
            ),
        ]));

        text_lines.push(Line::from(vec![
            Span::raw(format!(
                "   WARP: {} jumps to ",
                stop.deal.jumps_source_dest
            )),
            Span::styled(
                &stop.deal.dest_system_name,
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        text_lines.push(Line::from(vec![
            Span::raw("   SELL: "),
            Span::styled(
                format!("{}x {}", stop.deal.quantity, stop.deal.item_name),
                Style::default().fg(Color::White),
            ),
            Span::raw(format!(" @ {:.2} ISK/unit", stop.deal.sell_price)),
            Span::styled(
                format!(
                    " -> Profit: +{:.2}M ISK",
                    stop.deal.net_profit / 1_000_000.0
                ),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        text_lines.push(Line::from(vec![
            Span::raw("   AT:   "),
            Span::styled(
                &stop.deal.dest_station_name,
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    text_lines.push(Line::from(""));
    text_lines.push(Line::from(Span::styled(
        "── Full Waypoint Trail (for in-game Autopilot) ──",
        Style::default().fg(Color::DarkGray),
    )));
    let trail = r.waypoint_names.join(" -> ");
    text_lines.push(Line::from(Span::raw(trail)));

    let paragraph = Paragraph::new(text_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Route Plan "),
    );

    f.render_widget(paragraph, area);
}

fn render_help_view(f: &mut Frame, area: Rect) {
    let help_text = vec![
        Line::from(Span::styled("EVE Traveling Merchant - Quick Reference", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from("Keybindings:"),
        Line::from("  [Tab]       Switch between Deals Table, Route Plan, and Help"),
        Line::from("  [1]         Go directly to Deals Table"),
        Line::from("  [2]         Go directly to Route Plan"),
        Line::from("  [3]         Go directly to Help"),
        Line::from("  [↑ / k]     Navigate up in the table"),
        Line::from("  [↓ / j]     Navigate down in the table"),
        Line::from("  [q / Esc]   Quit application"),
        Line::from(""),
        Line::from("Ship Settings:"),
        Line::from("  • Big Hauler:   60,000 m³ cargo capacity │ 500,000,000 ISK max budget"),
        Line::from("  • Small Hauler: 30,000 m³ cargo capacity │  50,000,000 ISK max budget"),
        Line::from(""),
        Line::from("Profit per Jump (PPJ):"),
        Line::from("  Total jumps = (Jumps from starting system to buy station) + (Jumps from buy to sell station)."),
        Line::from("  PPJ = Net Profit (after taxes & broker fees) / Total Jumps."),
        Line::from("  Deals are prioritized by PPJ to ensure you make the most ISK for every jump traveled!"),
    ];

    let p = Paragraph::new(help_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Help & Info "),
    );
    f.render_widget(p, area);
}

fn render_status_bar(f: &mut Frame, area: Rect, _state: &AppState) {
    let bar = Paragraph::new(Line::from(vec![Span::styled(
        " [Tab] Switch Tab │ [↑/↓] Select Deal │ [Q] Quit ",
        Style::default().fg(Color::DarkGray),
    )]));
    f.render_widget(bar, area);
}
