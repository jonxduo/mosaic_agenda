//! Multi-device simulator and package scenario runner (PROGETTO.md §15.3, §15.4).

pub mod scenario;
pub mod world;

pub use scenario::{ScenarioError, run_scenario};
pub use world::{ADMIN, World, WorldError};
