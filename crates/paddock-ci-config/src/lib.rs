//! The CI job model (ADR 0123, decision 81): ci.yml's jobs, matrices, `needs` and the projects each job runs for,
//! built once from the workflow text, so every reader of a run asks one model instead of re-typing the layout.
//!
//! A pure core: no file or process I/O; workflow texts and declared data are passed in. [`yaml`] and [`expr`] are the one
//! YAML reader and expression evaluator of the CI model (ADR 0060); [`jobs`] is the model; [`settings`] declares the repository
//! variables and secrets the tools read and returns the setup derived from them (decision 82).

pub mod expr;
pub mod jobs;
pub mod settings;
pub mod yaml;
