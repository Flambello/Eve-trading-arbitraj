# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
