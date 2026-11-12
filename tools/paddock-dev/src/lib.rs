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
//! - `check run`: the host of the repository checks in Rust, C39 and C53 first ([`check`], RT-5);
//! - `check ipc-endpoints`: C102, the register of every listen site and the ratchet of connection endings
//!   ([`ipc_endpoints`], ADR 0072);
//! - `hook pre-tool-use` and `hooks install`: Claude Code's PreToolUse guard and its installer ([`hook`], ADR 0083);
//! - `merge check`: did a merge silently drop one side's hunks or tests? ([`merge_check`], ADR 0089 outcomes);
//! - `graph affected`: the affected closure of a change set, the first consumer of `paddock-repo-graph` ([`plan`], with the
//!   `.gitea` narrowing in [`plan_gitea`] and the git reader in [`plan_repo`]);
//! - `plan`: the CI planner, the whole command line of `scripts/affected.mjs` (`<base> [<head>]`, `--files`, `--all`, `--ci
//!   [--app]`, `--commit-range`, `--explain`): the matrix, the base of a run and the commit lint's range in [`plan_ci`], the
//!   command in [`plan_cmd`]. Oracle-tested against the script until it is retired (ADR 0022 R4). `plan --since-run <run>`
//!   reruns only what failed in a finished run ([`plan_since`]).
//! - `ci release-isolation setup|record|verify`: a tag build reads nothing kept ([`release_isolation`], ADR 0085).
//! - `load`: start, hold or stop for new agents, from the load, the free disk and the CI queue ([`load`], ER-24).
//! - `worktree add|claim|reassign|release|drop|list|clean` and `check primary`: agent worktrees locked with an owner note and
//!   bound to their agent, and the probe of the primary checkout ([`worktree`], ADR 0083).

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod check;
pub mod ci_isolation;
pub mod ci_runs;
mod cli;
mod fds;
pub mod gates;
pub mod hook;
pub mod input_hash;
pub mod ipc_endpoints;
pub mod load;
pub mod log_text;
pub mod merge_check;
pub mod nextest_log;
pub mod plan;
pub mod plan_ci;
pub mod plan_cmd;
pub mod plan_gitea;
pub mod plan_repo;
pub mod plan_since;
pub mod release_isolation;
pub mod slots;
pub mod wait;
pub mod worktree;

pub use cli::run;
