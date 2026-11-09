//! The orphan guardian (ADR 0076 amendment 2026-11-09): when a process dies without ending its supervised children (a
//! SIGKILL, a crash, a test runner's kill), their process groups are ended and the folders they were given removed.
//! Without it a child's tree runs on forever: the owner's own ends (the `Drop` of [`crate::Supervised`], a time-limit
//! watchdog thread) die with the owner.
//!
//! **Unix.** One guardian per owner process, started on the first guarded child: this executable run again with
//! [`GUARDIAN_ENV`] set, which [`run_if_requested`] at the top of `main` turns into the guardian (a binary that does not
//! call it never starts one, and says so once in the log). It runs in a group of its own and reads a pipe whose write end
//! only the owner holds (close-on-exec): one line per guarded child (`add`), and one when its owner ended that child
//! (`del`). End of file means the owner is gone: every group still registered is ended, then its folder removed, and each
//! step is one line in the guardian's log ([`LOG_ENV`]).
//!
//! One keeper per guarded child: `/bin/cat`, started by the owner into the child's process group, reading a pipe whose
//! write end only the guardian holds. While it is there (alive, or a zombie not yet reaped) the group's id cannot be handed
//! to another group, so the guardian signals a group only after checking that its keeper, by identity, is still in it
//! (rule 53, ADR 0033). It ends when the guardian ends, or when its owner ends the child. A keeper only has to block
//! on a pipe: `cat` is the cheapest process that does; the owner's executable run again would map megabytes for nothing.
//!
//! The guardian obeys only its owner: its parent, by pid and start time, running the same executable, on a pipe (on Linux
//! one the owner holds). It removes only a canonical folder holding the owner's marker, takes only whole lines, logs only
//! into a private folder of the user's (`O_NOFOLLOW`, mode 0600), and is off for root (amendment §1a).
//!
//! N guarded children cost N + 1 processes. Not done: `PR_SET_PDEATHSIG` (Linux) needs a `pre_exec` hook, which the family
//! bans (lint/clippy.toml), and it fires when the *thread* that started the child ends (prctl(2)).
//!
//! Limits: a child that leaves its process group (`setsid`) is outside the guard, as it is outside the owner's tree kill.
//! An owner killed between the child's start and its registration is not covered; a caller's start-up sweep is the
//! backstop for its folder.
//!
//! **Windows** needs no guardian: each child's job object is kill-on-close, and the job handle closes when the owner dies.
//! The functions below do nothing there, so every `main` calls them the same way.

/// Reserved: set to `1` in the environment of the guardian process this executable starts.
pub const GUARDIAN_ENV: &str = "PADDOCK_PROC_GUARDIAN";
/// The file the guardian appends its log lines to: from the owner's environment, else [`set_log`], else
/// `paddock-proc-guardian.log` in the temporary folder.
pub const LOG_ENV: &str = "PADDOCK_PROC_GUARDIAN_LOG";

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{Registration, register};
#[cfg(unix)]
pub use unix::{enable_in_test_binary, run_if_requested, set_log};

/// Call first thing in `main`. In the guardian process this serves the owner and never returns; elsewhere it returns at
/// once and lets this executable start a guardian for its supervised children. Windows: nothing to do.
#[cfg(not(unix))]
pub fn run_if_requested() {}

/// For a test binary (whose `main` is the test harness): `entry` is the full path of a test function of this binary that
/// calls [`run_if_requested`]. Windows: nothing to do.
#[cfg(not(unix))]
pub fn enable_in_test_binary(_entry: &str) {}

/// Where the guardian writes its log lines. Windows: nothing to do.
#[cfg(not(unix))]
pub fn set_log(_path: &std::path::Path) {}
