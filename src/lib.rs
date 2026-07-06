//! # Sonar — the world's strongest battleship AI engine.
//!
//! Sonar is a pure-Rust battleship engine that combines three state-of-the-art
//! techniques to play the classic 10×10 fleet game (ships of length 5, 4, 3, 3, 2):
//!
//! 1. **PDF density targeting** — for every cell on the board we compute the
//!    number of legal ship placements that pass through it, given the current
//!    observations (misses, hits, sinks). We fire at the cell with the
//!    highest density.
//! 2. **Bayesian hypothesis filter** — we maintain a sample of full fleet
//!    configurations that are consistent with observations. After every shot
//!    we discard inconsistent hypotheses. The decision is the argmax over
//!    the surviving hypotheses.
//! 3. **Constraint-dispersal placement** — our own fleet is placed by
//!    sampling N random legal configurations and picking the one with the
//!    smallest penalty (edge touches, ship-to-ship contact, parity imbalance,
//!    corner clustering).
//!
//! ## Time-limited, not count-limited
//!
//! Sonar does **not** cap the number of hypotheses. The only knob is a hard
//! time budget per move (default 20 s, configurable 1 s – 60 s via the
//! [`Engine`] API). The hypothesis generator runs cooperatively and yields
//! the best answer found so far when the deadline expires.
//!
//! ## 100% controllable API
//!
//! Everything Sonar does is exposed through the [`Engine`] struct. You can:
//!
//! - place your fleet manually or let Sonar place it,
//! - ask Sonar for its next shot (with a custom deadline),
//! - feed back shot results,
//! - inspect the internal probability matrix / density matrix,
//! - reset, save and load learning state,
//! - swap in custom targeting strategies via the [`TargetingStrategy`] trait.
//!
//! ## Example
//!
//! ```no_run
//! use sonar::{Engine, EngineConfig, Deadline, ShotResult};
//!
//! let mut engine = Engine::new(EngineConfig::default());
//! engine.place_fleet_random();          // our fleet
//! let deadline = Deadline::from_secs(20);
//! let (r, c) = engine.choose_move(deadline);
//! println!("Sonar fires at ({}, {})", r, c);
//! ```
//!
//! ## License
//!
//! Apache-2.0. See `LICENSE` file for full text.

#![feature(test)]
#![allow(dead_code)]

// ─────────────────────────────────────────────────────────────────────────────
// Public modules — these form the stable API surface.
// ─────────────────────────────────────────────────────────────────────────────

pub mod api;
pub mod benchmark;
pub mod bitboard;
pub mod board;
pub mod engine;
pub mod external;
pub mod fleet;
pub mod game;
pub mod helpers;
pub mod hypothesis;
pub mod json_server;
pub mod learning;
pub mod placement;
pub mod player;
pub mod reference_bots;
pub mod rng;
pub mod rules;
pub mod targeting;
pub mod time_limit;

// ─────────────────────────────────────────────────────────────────────────────
// Re-exports — the most commonly used types are reachable at the crate root.
// ─────────────────────────────────────────────────────────────────────────────

pub use api::{Engine, EngineConfig, EngineSnapshot, MoveSuggestion};
pub use bitboard::BitBoard;
pub use board::{Board, Cell, OwnerCell, ShotResult};
pub use engine::Game;
pub use fleet::FLEET;
pub use helpers::{cell_index, cell_rc, is_valid_cell, parse_coordinate};
pub use learning::{LearningDB, GameRecord};
pub use player::{BotPlayer, HumanPlayer, PdfBot, Player, RandomBot};
pub use rng::Xoshiro256;
pub use rules::{ContactRule, GameRules, SunkRule};
pub use targeting::{EnemyView, HybridTargeting, PdfConfig, PdfTargeting, TargetingStrategy};
pub use time_limit::Deadline;
