//! # EVE Trade Scout (`eve_trade_scout`)
//!
//! A high-performance location-centric arbitrage calculation engine, multi-item
//! corridor cargo knapsack optimizer, and route planner for EVE Online.
//!
//! ## Subsystems
//! - [`arbitrage`]: Core cross-station market spread evaluation and quantity solver.
//! - [`audio`]: Audio alerts and desktop notifications for high-ROI opportunities.
//! - [`chain_route_planner`]: Multi-leg beam search chained route planner with knapsack consolidation.
//! - [`config`]: Persistent TOML user configuration, ship profiles, and cockpit themes.
//! - [`error`]: Standardized machine-readable error codes ([`ErrorCode`]) and diagnostic taxonomy.
//! - [`esi`]: Asynchronous CCP EVE Swagger Interface (ESI) client, market fetcher, and SSO v2 PKCE authentication.
//! - [`graph`]: Universe stargate topology graph with Dijkstra pathfinding and LRU jump caching.
//! - [`hub_arbitrage`]: Trade hub matrix scanner across the 5 primary empire trading hubs.
//! - [`logging`]: Production-grade structured tracing subscriber with rotating daily JSON file logs.
//! - [`route_planner`]: Greedy multi-stop sequential route builder with in-game flight sheet formatting.
//! - [`sde`]: Embedded EVE Static Data Export (SDE) SQLite loader and real-time autocomplete engine.
//! - [`ui`]: Native desktop graphical user interface powered by `eframe` and `egui`.

pub mod arbitrage;
pub mod audio;
pub mod chain_route_planner;
pub mod config;
pub mod error;
pub mod esi;
pub mod graph;
pub mod hub_arbitrage;
pub mod logging;
pub mod route_planner;
pub mod sde;
pub mod ui;

pub use error::ErrorCode;
