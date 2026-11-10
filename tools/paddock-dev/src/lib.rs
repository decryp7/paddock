//! The developer and CI tool of the Paddock monorepo (ADR 0022), following `paddock-cli`'s conventions: `--json` makes
//! standard output exactly one JSON object, errors name the next step, and the exit codes are `paddock_cli::Exit`'s.
//!
//! R0 (this package) holds the skeleton and these groups of commands:
//!
//! - `self hash`: the content address of the binary ([`input_hash`]);
//! - `ci slot acquire|release|status`: job-isolation slots over kernel file locks ([`slots`], backed by `paddock-slots`);
//! - `ci isolation`: the CI kit's job isolation (the port of `conformance/ci/job-isolation.sh`, `job-tmpdir.sh` and `ci-home.sh`,
//!   [`ci_isolation`], ADR 0025);
//! - `wait`: one bounded wait for a plock job, a process, a log line or a CI run ([`wait`]);
//! - `check ipc-endpoints`: C102, the register of every listen site and the ratchet of connection endings
//!   ([`ipc_endpoints`], ADR 0072);
//! - `graph affected`: the affected closure of a change set, the first consumer of `paddock-repo-graph` ([`plan`], with the
//!   `.gitea` narrowing in [`plan_gitea`] and the git reader in [`plan_repo`]);
//! - `plan`: the CI planner, the whole command line of `scripts/affected.mjs` (`<base> [<head>]`, `--files`, `--all`, `--ci
//!   [--app]`, `--commit-range`, `--explain`): the matrix, the base of a run and the commit lint's range in [`plan_ci`], the
//!   command in [`plan_cmd`]. Oracle-tested against the script until it is retired (ADR 0022 R4). `plan --since-run <run>`
//!   reruns only what failed in a finished run ([`plan_since`]).

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod cli;
pub mod ci_isolation;
mod fds;
pub mod gates;
pub mod input_hash;
pub mod ipc_endpoints;
pub mod plan;
pub mod plan_ci;
pub mod plan_cmd;
pub mod plan_gitea;
pub mod plan_repo;
pub mod plan_since;
pub mod slots;
pub mod wait;

pub use cli::run;
