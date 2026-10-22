//! CPU taming: rules, profiles, the escalation engine and its undo log (ADR 0004).

pub mod engine;
pub mod model;

pub use engine::{Command, Engine, Situation, Tick};
pub use model::*;
