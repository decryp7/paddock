//! The developer and CI tool of the Paddock monorepo (ADR 0022), following `paddock-cli`'s conventions: `--json` makes
//! standard output exactly one JSON object, errors name the next step, and the exit codes are `paddock_cli::Exit`'s.
//!
//! R0 (this package) holds the skeleton and these groups of commands:
//!
//! - `self hash`: the content address of the binary ([`input_hash`]);
//! - `ci slot acquire|release|status`: job-isolation slots over kernel file locks ([`slots`], backed by `paddock-slots`);
//! - `wait`: one bounded wait for a plock job, a process, a log line or a CI run ([`wait`]);
//! - `check ipc-endpoints`: C102, the register of every listen site and the ratchet of connection endings
//!   ([`ipc_endpoints`], ADR 0072);
//! - `graph affected`: the affected closure of a change set, the first consumer of `paddock-repo-graph` ([`plan`]). It is
//!   the oracle-tested port of `scripts/affected.mjs`'s core; `affected.mjs` stays CI's planner until R4.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod cli;
mod fds;
pub mod input_hash;
pub mod ipc_endpoints;
pub mod plan;
pub mod slots;
pub mod wait;

pub use cli::run;
