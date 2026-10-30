//! Child processes for the Paddock apps (plan X6). Every app forbids `std::process::Command::new` and points at this
//! crate instead, so the same rules hold everywhere:
//!
//! - [`command`]: no console window on Windows (R06), optionally a cleared environment ([`clean_env`]) and lower
//!   priority ([`lower_priority`], never through `nice(1)`, which strips `DYLD_*` on macOS).
//! - [`run_bounded`]: run to completion with a timeout and an output cap; on a timeout or the cap the whole process tree
//!   is ended (T03).
//! - [`Supervised`]: a long-running child whose whole tree ends on drop, on [`Supervised::terminate`] and when it exceeds
//!   a memory cap ([`MemoryCap`]); a [`Registry`] ends every supervised tree at shutdown ([`end_all`]).
//! - [`process_memory`], [`tree_memory`]: real memory, never the resident size: PSS (`smaps_rollup`, thread entries
//!   skipped) on Linux, the physical footprint on macOS, the private working set on Windows (R07, rule 26).
//! - [`login_path`]: the `PATH` of the user's login shell, for a service started by launchd or systemd.
//!
//! - [`self_test`]: the `--self-test` every sidecar answers, started from the built app by CI (rule 23).
//!
//! Processes are named by identity ([`ProcessId`]: pid plus start time), never by a bare pid, because the OS hands a freed
//! pid on (Windows at once): anything ended is first checked to still be the process that was meant (ADR 0033).
//!
//! Unix children lead their own process group (killed with `killpg`); every supervised Windows child runs in a kill-on-close
//! job object, which ends the whole tree by handle, so its helpers end with it even when they left its tree or the app
//! crashed. Both are backed by a walk of the process table, by identity, for helpers that left the group or the job.

mod command;
mod identity;
#[cfg(windows)]
mod job;
mod login;
mod run;
pub mod self_test;
mod supervised;
mod sys;
mod tree;
mod usage;
mod watch;

#[cfg(test)]
mod testchild;

pub use command::{CREATE_NO_WINDOW, KEPT_ENV, clean_env, command, lower_priority};
pub use identity::{Ending, Holder, ProcessId, StartTime, holder};
pub use login::{
    MAX_PATH_LEN, SHELL_TIMEOUT, ask_shell, login_path, merge_path, parse_probe_output,
    probe_script, shell_candidates,
};
pub use run::{DEFAULT_MAX_OUTPUT, Limits, Outcome, Output, RunError, run_bounded};
pub use supervised::{MemoryCap, Registry, Sampler, Supervised, end_all, global_registry};
pub use tree::{
    Terminated, TreeEnding, ask, descendants_of, end, is_alive, kill_tree, processes_mentioning,
    terminate,
};
pub use usage::{
    MAX_TREE, OsSource, ProcEntry, ProcessSource, Row, Table, descendants, identity, parse_pss,
    process_memory, read_children, read_pss, snapshot, table_from, tree_memory, tree_memory_from,
    tree_memory_of, tree_memory_with,
};
pub use watch::{
    MemoryEvent, MemoryListener, MemoryUse, MemoryWatch, REARM_PERCENT, Verdict, WARN_PERCENT,
};
