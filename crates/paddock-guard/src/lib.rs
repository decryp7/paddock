//! Crash containment for code the app does not control (plugins, workers, anything behind a trait object that may panic or
//! hang), from Debrief's `debrief_core::guard` without its plugin types.
//!
//! - [`catch`] turns a panic into a typed [`Crashed`] naming what crashed.
//! - [`Quarantine`] counts [`Failure`]s per owner (panic, hang, garbled reply) on an injected
//!   [`Clock`](paddock_jobs::time::Clock); [`QuarantinePolicy`] (3 in 10 minutes by default) decides when an owner is no longer
//!   called. A typed error the owner returns on purpose is not a failure. The quarantined owners are kept in a versioned state
//!   file ([`paddock_env::VersionedFile`]) that tolerates a missing or damaged file.
//! - [`Guard::call`] is the building block of guarded adapters: it refuses a quarantined or hung owner, runs the function
//!   under a time budget (on a helper thread that is abandoned when it overruns), catches panics and records the failures.
//!
//! In-process guarding cannot stop a running call, survive a stack overflow or an abort, or free memory; a guarded call is
//! meant to wrap the *remote* call to an out-of-process owner, where the process boundary does that.

mod crash;
mod guard;
mod quarantine;

pub use crash::{Crashed, catch, panic_message};
pub use guard::{CallError, Guard};
pub use quarantine::{
    Change, Failure, MAX_ERROR_CHARS, OwnerStatus, Quarantine, QuarantinePolicy, Refusal,
};

/// Log target of guard events (contained crashes, quarantines, overruns).
pub const LOG_TARGET: &str = "paddock_guard";
