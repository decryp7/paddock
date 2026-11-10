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
//! - [`StartJudge`], [`StartWatch`]: a start judged on whether the process is stopped ([`run_state`]), its CPU time, progress and a
//!   ready handshake, never on a fixed wait (ADR 0031); a stopped process is waited for and said so
//!   ([`StallCause::Paused`]); [`StallReport`] says why a wait looks stuck. [`InstanceLock`]: one instance per folder by a
//!   held OS lock; [`hint`]: a discovery file is only a hint (rule 53).
//!
//! - [`console`]: a GUI-subsystem program (no console window at login) that still prints when run from a terminal.
//! - [`self_test`]: the `--self-test` every sidecar answers, started from the built app by CI (rule 23).
//!
//! Processes are named by identity ([`ProcessId`]: pid plus start time), never by a bare pid, because the OS hands a freed
//! pid on (Windows at once): anything ended is first checked to still be the process that was meant (ADR 0033).
//!
//! Unix children lead their own process group (killed with `killpg`); every supervised Windows child runs in a kill-on-close
//! job object, which ends the whole tree by handle, so its helpers end with it even when they left its tree or the app
//! crashed. Both are backed by a walk of the process table, by identity, for helpers that left the group or the job.

pub mod cleanup;
mod command;
pub mod console;
pub mod guardian;
pub mod hint;
mod identity;
pub mod installed;
mod instance;
#[cfg(windows)]
mod job;
mod login;
mod run;
mod runstate;
pub mod self_test;
mod stall;
mod start;
mod supervised;
mod sys;
mod tree;
mod usage;
mod watch;

#[cfg(test)]
mod testchild;

/// The effective user id of this process (Unix only), for checks that a file belongs to whoever runs the app.
#[cfg(unix)]
pub fn effective_uid() -> u32 {
    sys::effective_uid()
}

#[cfg(all(test, unix))]
mod uid_tests {
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn the_effective_uid_owns_the_files_this_process_creates() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mine");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(
            std::fs::metadata(&file).unwrap().uid(),
            super::effective_uid()
        );
    }
}

pub use command::{CREATE_NO_WINDOW, KEPT_ENV, clean_env, command, lower_priority};
pub use identity::{Ending, Holder, ProcessId, StartTime, holder};
pub use instance::{
    Claim, HeldState, InstanceError, InstanceLock, held_state, is_held_now, is_held_settled,
};
pub use login::{
    MAX_PATH_LEN, SHELL_TIMEOUT, ask_shell, login_path, merge_path, parse_probe_output,
    probe_script, shell_candidates,
};
pub use run::{DEFAULT_MAX_OUTPUT, Limits, Outcome, Output, RunError, run_bounded};
pub use runstate::{RunState, run_state};
pub use stall::{StallAction, StallCause, StallReport};
pub use start::{
    Cpu, ExitInfo, Liveness, Pace, Patience, Reading, StartJudge, StartProbe, StartState,
    StartWatch, Watched, liveness_of, tail_of, tree_cpu,
};
pub use supervised::{MemoryCap, Registry, Sampler, Supervised, end_all, global_registry};
pub use tree::{
    Presence, Terminated, TreeEnding, ask, descendants_of, end, is_alive, kill_tree, may_be_alive,
    presence, processes_mentioning, terminate,
};
pub use usage::{
    CpuSample, MAX_TREE, OsSource, ProcEntry, ProcessSource, Row, Table, descendants, identity,
    parse_pss, process_memory, read_children, read_pss, snapshot, table_from, tree_cpu_time,
    tree_cpu_time_from, tree_cpu_time_of, tree_memory, tree_memory_from, tree_memory_of,
    tree_memory_with,
};
pub use watch::{
    MemoryEvent, MemoryListener, MemoryUse, MemoryWatch, REARM_PERCENT, Verdict, WARN_PERCENT,
};
