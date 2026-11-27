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
//! - `gen check`: the checks of the generated and pinned files a change touches, from `docs/generators.toml`
//!   ([`generators`], ER-40); `merge check` runs the ones a merge makes due;
//! - `plan`: the CI planner, the whole command line of the retired `scripts/affected.mjs` (`<base> [<head>]`, `--files`, `--all`, `--ci
//!   [--app]`, `--explain`): the affected closure of a change set, the first consumer of `paddock-repo-graph` ([`plan`], with the
//!   `.gitea` narrowing in [`plan_gitea`] and the git reader in [`plan_repo`]), the matrix and the base of a run in [`plan_ci`], the
//!   command in [`plan_cmd`]. Tested against recorded answers (the script was retired 2026-11-18, ADR 0022 R4). `plan --since-run <run>`
//!   reruns only what failed in a finished run ([`plan_since`]).
//! - `ci setup-rust`: the compile cache and the Tauri Linux libraries of a CI job, the port of the setup-rust action's shell steps
//!   ([`ci_setup_rust`]), over [`ci_tool`], the wrapper contract for third-party programs a CI step runs (docs/plans/ci-in-rust.md);
//! - `ci build-jobs [<split>]`: CARGO_BUILD_JOBS and NEXTEST_TEST_THREADS for a job, the port of `conformance/ci/build-jobs.sh`
//!   ([`ci_build_jobs`]).
//! - `audit nightly|compare`: advisory audits of every lockfile, main's nightly and a change's new findings only ([`audit`], ER-41).
//! - `locks sync`: the command that regenerates every lockfile and the workspace hack ([`locks`]).
//! - `locks guard [--write]`: a dependency added to a shared crate changes neither the workspace hack, the features of a watched crate
//!   nor a lockfile without that change in the same commit ([`locks_guard`], report-only).
//! - `ci stable-workspace|node-caches|host-tools|sccache-stats|build-folder|prune-build-folder|prune-folder|prune-agent-builds|
//!   host-cleanup`: the host-runner steps of the kit, the ports of the actions of the same names and of `build-folder.sh`, the prune
//!   scripts and `host-cleanup.sh` ([`ci_host`], docs/plans/ci-in-rust.md WP-4). Every deletion goes through its guarded remove.
//! - `ci release-isolation setup|record|verify`: a tag build reads nothing kept ([`release_isolation`], ADR 0085).
//! - `disk check|report|scan|clean|adopt|record`: free-space check, usage tracking (owner records and a SQLite index) and clean-up
//!   of merged and stale-pushed folders ([`disk`]).
//! - `load`: start, hold or stop for new agents, from the load, the free disk and the CI queue ([`load`], ER-24).
//! - `places`: the machine's dev folders, derived from `paddock.worktreeRoot` for the scripts, `--pinned` for the VM guards ([`places`]).
//! - `ci triage <run>`: NEW, ALREADY FAILING ON MAIN or FLAKY for each failed test of a run ([`ci_runs::triage`]).
//! - `worktree add|claim|reassign|release|drop|list|clean`: agent worktrees locked with an owner note and
//!   bound to their agent ([`worktree`], ADR 0083).
//! - `pr open|label|list|sync|plan-status`: the branch ledger on Gitea draft pull requests with labels, and plan rows checked
//!   against it ([`pr`], docs/plans/merge-queue.md).

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod active_apps;
pub mod audit;
pub mod cargo_target;
pub mod check;
pub mod check_ids;
pub mod child_env;
pub mod ci_build_env;
pub mod ci_build_jobs;
pub mod ci_dyno_linux_units;
pub mod ci_host;
pub mod ci_isolation;
pub mod ci_job_model;
pub mod ci_run;
pub mod ci_runs;
pub mod ci_rust_pin;
pub mod ci_s3_guard;
pub mod ci_setup_rust;
pub mod ci_tool;
mod cli;
pub mod deps_cache;
pub mod disk;
pub mod escape;
pub mod full_tree;
pub mod gates;
pub mod generators;
pub mod git_env;
pub mod health;
pub mod heavy_cmd;
pub mod heavy_install;
pub mod hook;
pub mod host_tests;
pub mod input_hash;
pub mod ipc_endpoints;
pub mod job_outcome;
pub mod lessons_driver;
pub mod load;
pub mod local_sccache;
pub mod locks;
pub mod locks_guard;
pub mod log_text;
pub mod merge_check;
pub mod nextest_log;
pub mod os_placement;
pub mod pathfind;
pub mod places;
pub mod plan;
pub mod plan_cargo_lock;
pub mod plan_ci;
pub mod plan_cmd;
pub mod plan_gitea;
pub mod plan_graph;
pub mod plan_repo;
pub mod plan_since;
pub mod pr;
pub mod pre_push;
pub mod priority;
pub mod process_env;
pub mod queue;
pub mod release_isolation;
pub mod repo_inputs;
pub mod repo_model;
pub mod run_scope;
pub mod safe_remove;
pub mod slots;
pub mod test_cache;
pub mod test_cost;
pub mod test_features;
// Unix only: it starts the helper through a POSIX shell, and every test that uses it is `#[cfg(unix)]`.
#[cfg(all(test, unix))]
mod test_sleeper;
pub mod tool_cache;
pub mod wait;
pub mod wait_lock;
pub mod worktree;

pub use cli::run;
