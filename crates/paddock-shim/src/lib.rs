//! The shim: interception and monitoring of the heavy tools (user decision 77, 2026-11-26).
//!
//! A program called `cargo`, `npm`, `npx`, `node`, `playwright` or `ng` (a link to the `throttle` binary, written by
//! `throttle shims install <dir>`) is a shim: it finds the real tool further along `PATH` (skipping itself, [`resolve`]), takes a slot
//! from `paddock-throttle` (normal priority, or the train lane when `THROTTLE_PRIORITY=train`), runs the real tool with the same
//! arguments, standard streams and exit code ([`shim::run`]), and reports what happened as an event stream ([`event`]) to sinks
//! ([`sinks`]): the run log line (tool, project, start, duration, exit code, never the command line, ER-27) and the stall report.
//! Output capture and Pitcrew are later sinks of the same stream.
//!
//! - Nested: when `THROTTLE_HELD` names a live holder, the tool runs at once in that slot (ER-43, the same rule as `throttle run`).
//! - Opt out: `PADDOCK_SHIM=off`, or a file `.paddock-shim-off` in the project folder or above ([`project`]): the real tool is run
//!   with no slot and no record.
//! - Degrade: when no slot can be taken (an unwritable lock folder), the tool still runs and the cause is logged.
//! - Hang detection is observe-by-default: a run with no CPU for the limit (`PADDOCK_SHIM_STALL`, seconds, default 600, 0 off) is
//!   reported and logged; the tool is ended only when `PADDOCK_SHIM_STALL_ACTION=end`. The standard streams belong to the real
//!   tool (they are not piped), so the output cannot be counted: the detector watches CPU time of the tool's process tree alone.

pub mod event;
pub mod install;
pub mod project;
pub mod resolve;
pub mod shim;
pub mod sinks;

pub use shim::{SHIMMED, called_as, run};
