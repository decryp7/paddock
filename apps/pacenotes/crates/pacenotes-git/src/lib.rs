#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! Pacenotes' history adapter (ADR 0004): [`GixHistory`] implements the core's `History` port over gix, `log` lists what
//! changed in each commit, and [`churn`] and [`rank_hot_spots`] turn that into figures.
//!
//! A repository is hostile input. [`open`] checks that nothing leads out of the workspace (a `.git` file, symlinks,
//! `commondir`, alternates) and opens gix isolated; the crate never starts a process and never asks gix for a worktree, an
//! index, a filter, a blob diff or an attribute, so nothing a repository configures (`core.fsmonitor`, `filter.*`,
//! `diff.*.textconv`, hooks) can run.

pub mod churn;
pub mod history;
pub mod open;

pub use churn::{CommitChanges, FileChurn, HotSpot, churn, rank_hot_spots};
pub use history::{GixHistory, MAX_LOG_COMMITS};
pub use open::{OpenError, open};
