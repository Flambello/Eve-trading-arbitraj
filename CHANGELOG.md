# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Visual Theme System & Cockpit Color Schemes**: Introduced a tokenized styling architecture (`src/ui/theme.rs`) offering four finely tuned cockpit themes:
  - **Sci-Fi Void (Default)**: Deep obsidian space navy with neon telemetry cyan and Caldari/EVE gold accents.
  - **Neutral Black**: True OLED pitch black with crisp monochrome contrast.
  - **Black & Red**: Triglavian/pirate obsidian surfaces with high-saturation tactical crimson red accents.
  - **Solar Light**: High-clarity daylight workstation featuring pure black-on-white text (`#000000` text on `#ffffff` cards) with cobalt blue accents.
- **Interactive Theme Switcher**: Added responsive theme selection cards with live preview color chips, descriptions, active badges, and persistent configuration in `config.toml` (`theme = "sci_fi_void"`).
- **Theme Test Suite**: Added comprehensive automated tests in `tests/test_theme.rs` verifying color contrast, frame generators, context styling, dark mode classification, semantic indicators, button contrasts, and hub badges.

### Changed
- **Cross-Tab Theme Integration**: Harmonized visual styles across the entire application:
  - **NPC Hub Arbitrage**: Replaced hardcoded panels, button fills, and hub badges with dynamic theme tokens, adaptive silver badges, and brand primary/secondary buttons.
  - **Deals Table & Dossier**: Applied theme-aware row highlights, gold/silver/bronze rank badges, and highsec security pills.
  - **Manifests & Route Planner**: Upgraded flight sheet timelines, origin/destination cards, and knapsack cargo tables to utilize theme frame tokens.
  - **Executive Telemetry & Top Bar**: Styled KPI summary cards, filter bars, and quick hub buttons using active theme surfaces.

## [0.2.0] - 2026-10-16

### Removed
- **CLI & Argument Parsers**: Fully decommissioned `clap` and removed all command-line argument parsers, flags, and `src/cli.rs`.
- **Terminal UI (TUI) & Console Outputs**: Completely removed `ratatui` and `crossterm` terminal interfaces (`src/ui/tui.rs`), terminal progress bars (`indicatif`), console table formatters, and redundant CLI-only CSV exporters.
- **Dependency Manifest Pruning**: Pruned `clap`, `ratatui`, `crossterm`, and `indicatif` from `Cargo.toml` and lockfile, reducing compilation overhead and binary footprint.

### Changed
- **Pure Desktop GUI Architecture**: `eve-merchant` now directly executes the native Desktop GUI window on all platforms with zero CLI overhead.
- **Logging Integration**: Replaced terminal progress bars in `src/sde/loader.rs` with structured `tracing::info!` events.

## [0.1.1] - 2026-10-15

### Changed
- **Default Application Interface**: Made the native Desktop GUI the universal default on all platforms (Windows, Linux, macOS). Running `eve-merchant` or double-clicking `eve-merchant.exe` now always launches the graphical interface without requiring `--gui`.
- **Windows Packaging & Subsystem**: Added `#![windows_subsystem = "windows"]` for Windows release builds to prevent terminal popup windows, added console attachment (`AttachConsole`) for CLI usage from PowerShell/CMD, and included `start-gui.bat` in the standalone Windows release bundle.
- **Terminal UI / CLI Deprecation**: Formally deprecated the interactive Terminal UI (`--tui`, `ui::run_tui`) and console output (`--no-ui`, `--no-tui`) across all platforms with compile-time deprecation attributes, clap documentation flags, and runtime banners.

### Added
- **UI Dispatch Test Suite**: Added `tests/test_ui_dispatch.rs` validating default GUI launch behavior, display detection, and deprecation notices.

## [0.1.0] - 2026-10-10

### Changed
- **License**: Re-licensed project under the GNU General Public License v3.0 (`GPL-3.0-or-later`) for copyleft open-source software distribution.

### Added
- **Native Desktop GUI**: Dark-mode sci-fi interface powered by `egui` and `eframe` with instant startup, real-time SDE autocomplete, telemetry pills, and 1-click clipboard actions.
- **Knapsack Cargo Manifests**: Multi-item cargo consolidation engine grouping commodities onto identical station corridors to pack the cargo hold and wallet budget.
- **Multi-Leg Chained Optimizer**: Greedy beam search algorithm calculating sequential trading routes with en-route detour pickups and capital recycling.
- **Live ESI Market Integration**: Asynchronous concurrent order fetching across multiple regions with ETag disk caching and rate-limiting backoff.
- **Stargate Dijkstra Router**: High-performance graph pathfinding with LRU memoization and highsec safe routing avoidance.
- **Portable Linux & Windows Packaging**: Standalone release archives with native desktop launchers, multi-resolution application icons, and automated multi-platform CI/CD.
- **Automated Test Suites**: 49+ integration tests covering arbitrage invariants, profile bounds, Dijkstra pathfinding, knapsack optimization, and ESI serialization.
- **Sound Alerts**: Audible alerts and desktop notification banners upon finding high-ROI trades.
