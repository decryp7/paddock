//! Long operations for the Paddock apps (ADR 0008).
//!
//! The crate is pure Rust with no UI or Tauri dependency. An app wires one [`EventSink`] (for Tauri: the `tauri` feature's `tauri::Emit`, on [`EVENT_NAME`])
//! and everything else is shared:
//!
//! * [`JobSnapshot`] is the wire model (camelCase JSON): id, kind, title, subject, a [`JobState`] state machine and a [`PhaseProgress`]
//!   with its own scale ([`Unit`], known or unknown total).
//! * [`Reporter`] throttles and coalesces updates; [`JobRegistry`] and [`JobContext`] own ids, cancel tokens and terminal states.
//! * [`JobPool`] runs job bodies on a bounded worker pool so command handlers return at once ([`assert_nonblocking`] tests that).
//! * [`journal`] is the activity journal (a bounded, persisted log of what the app did, with undo states).
//! * [`schedule`] is the family's schedules (every N, times of day, cron; zones, DST, catch-up) and pause windows.
//! * [`time`] is the family's clock port ([`Clock`], [`SystemClock`], [`FakeClock`]), deadlines and restart back-off.
//! * [`CancelToken`] is checked cooperatively; [`par_walk`], [`par_map_ordered`], [`pipeline_ordered`] and [`HashingWriter`] honour it
//!   within [`CANCEL_CHECK_ITEMS`] items or [`CHUNK`] bytes.

mod cancel;
mod error;
mod eta;
mod hash;
mod job;
pub mod journal;
mod map;
mod model;
mod pipeline;
mod pool;
mod reporter;
pub mod schedule;
#[cfg(feature = "tauri")]
pub mod tauri;
#[cfg(feature = "tauri")]
pub mod tauri_activity;
pub mod time;
mod walk;
mod workers;

pub use cancel::{CancelReason, CancelToken, Cancelled, OnCancel};
pub use error::JobError;
pub use eta::EtaEstimator;
pub use hash::{CHUNK, HashingWriter, copy_hashed, hex};
pub use job::{JobContext, JobRegistry};
pub use map::par_map_ordered;
pub use model::{
    IllegalTransition, JobId, JobKind, JobSnapshot, JobState, Phase, PhaseProgress, Unit,
};
pub use pipeline::pipeline_ordered;
pub use pool::{
    BLOCKED_AFTER, Blocked, JobHandle, JobPool, Offload, PoolClosed, PoolHold, assert_nonblocking,
    assert_nonblocking_within,
};
pub use reporter::{Counter, DEFAULT_MIN_INTERVAL_MS, EventSink, Reporter, UncancellableGuard};
pub use time::{Clock, FakeClock, SystemClock};
pub use walk::{
    Decision, EntryKind, Filter, SymlinkPolicy, WalkEntry, WalkIssue, WalkOptions, WalkResult,
    par_walk,
};
pub use workers::Workers;

/// Parallel helpers look at the cancel token at least this often (items), so a cancel is seen within a few milliseconds of work.
pub const CANCEL_CHECK_ITEMS: usize = 256;
/// The documented cancel latency target for helpers in this crate; longer, non-interruptible steps declare themselves
/// with [`JobContext::uncancellable`].
pub const CANCEL_LATENCY_TARGET_MS: u64 = 250;
/// The Tauri event name apps emit [`JobSnapshot`]s on (ADR 0012 naming: `paddock://<area>`).
pub const EVENT_NAME: &str = "paddock://jobs";
