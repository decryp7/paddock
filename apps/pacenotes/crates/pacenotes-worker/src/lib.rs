#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! The parser worker and its host (ADR 0006).
//!
//! - [`protocol`]: the two messages and their version.
//! - [`serve`]: the worker's side (the `pacenotes-parse-worker` program calls it).
//! - [`host`]: the app's side: a pool of workers behind the index's `SymbolSource`, a worker that dies or is judged stuck
//!   quarantines the file and the run goes on.
//! - [`stall`] and [`probe`]: ADR 0031's stall judgement (no wall clocks) as a pure state machine, and the per-process probe it
//!   reads.

#[cfg(feature = "test-hooks")]
mod hooks;
pub mod host;
pub mod probe;
pub mod protocol;
pub mod serve;
pub mod stall;

pub use host::{ParseWorkers, WorkerSettings};
pub use probe::{OsProbe, ProcSample, ProcessProbe};
pub use protocol::{PROTOCOL, ParseReply, ParseRequest};
pub use stall::{RunState, StallJudge, StallLimits, Tick, Verdict};
