//! Projections (PROGETTO.md §5, §7.5, §7.6): organization and permissions on
//! the causal past, documents and workflows, batches, aggregates, programs,
//! resources, board, notifications and reminders.
//!
//! The engine knows no domain: every concept comes from the configuration.

pub mod actions;
pub mod auth;
pub mod channels;
pub mod config;
mod conflicts;
mod handle;
pub mod notify;
pub mod ops;
pub mod projection;
pub mod rules;
pub mod time;
pub mod values;

pub use auth::{AuthState, Denied};
pub use projection::{EngineError, Projection, Verdict, rebuild};
