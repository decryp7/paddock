//! The project graph of a repository (ADR 0022 §6; the planner's and, later, Pacenotes' view of it).
//!
//! - [`Graph`]: projects (a folder, a name, a workspace, the folders they depend on), [`Graph::node_of`] (the deepest
//!   project that contains a path) and [`Graph::dependents`] (the reverse closure: everything that reaches a change).
//! - [`npm_graph`]: npm packages, plus the edges that bypass npm: a Tauri shell whose `frontendDist` points into another
//!   package, and path references (`../..`) in `tsconfig*.json`, `angular.json` and `eslint.config.*`.
//! - [`cargo_graph`] and [`parse_metadata`]: Cargo path dependencies of every workspace, from `cargo metadata` output
//!   that a [`CargoMetadata`] port supplies.
//! - [`lock_diff`]: the npm workspaces whose resolved packages differ between two `package-lock.json` texts.
//! - [`History`]: the port for tracked files, changed files between revisions, a file at a revision and the log.
//!
//! Features: `git-history` ([`GitHistory`]) and `cargo-metadata` ([`CargoCli`]) run `git` and `cargo` through
//! `paddock-proc`; both are off by default. The core reads files only through [`FileSource`].

mod cargo;
mod error;
mod graph;
mod history;
mod lock;
mod npm;
mod paths;
mod source;

#[cfg(feature = "cargo-metadata")]
pub use cargo::CargoCli;
pub use cargo::{CargoMetadata, cargo_graph, parse_metadata};
pub use error::GraphError;
pub use graph::{Graph, Node};
#[cfg(feature = "git-history")]
pub use history::GitHistory;
pub use history::{Commit, History, parse_log};
pub use lock::lock_diff;
pub use npm::npm_graph;
pub use paths::{dirname, join_normalized, relative};
pub use source::{FileSource, MemoryFiles};
