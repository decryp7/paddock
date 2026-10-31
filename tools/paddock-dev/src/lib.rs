//! The developer and CI tool of the Paddock monorepo (ADR 0022), following `paddock-cli`'s conventions: `--json` makes
//! standard output exactly one JSON object, errors name the next step, and the exit codes are `paddock_cli::Exit`'s.
//!
//! R0 (this package) holds the skeleton and three groups of commands:
//!
//! - `self hash`: the content address of the binary ([`input_hash`]);
//! - `ci slot acquire|release|status`: job-isolation slots over kernel file locks ([`slots`], backed by `paddock-slots`);
//! - `graph affected`: the affected closure of a change set, the first consumer of `paddock-repo-graph` ([`plan`]). It is
//!   the oracle-tested port of `scripts/affected.mjs`'s core; `affected.mjs` stays CI's planner until R4;
//! - `host prune cap|worktrees`: the four build-folder pruners in one command, with a dry run ([`prune`], R5).

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod cli;
mod fds;
pub mod input_hash;
pub mod plan;
pub mod prune;
mod prune_cmd;
pub mod prune_fs;
pub mod slots;

pub use cli::run;
