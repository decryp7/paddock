//! The event stream of one shim run. The run emits events; sinks (the run log, the stall report, later output capture and Pitcrew)
//! each take the ones they care about. A sink never changes the run: it cannot fail it, and a panic-free `event` is its whole contract.

use std::time::Duration;

/// What happened in one run.
#[derive(Debug, Clone, PartialEq)]
pub enum RunEvent<'a> {
    /// The real tool has started (after the slot was taken).
    Started {
        /// The tool, as called (`cargo`).
        tool: &'a str,
        /// The project the run belongs to ([`crate::project`]).
        project: &'a str,
        /// Epoch microseconds.
        start_us: u64,
    },
    /// The tool's process tree used no CPU for `quiet` (the stall limit); `ends` says whether the shim ends it.
    Stalled {
        /// The tool, as called.
        tool: &'a str,
        /// The project.
        project: &'a str,
        /// How long nothing moved.
        quiet: Duration,
        /// The tool's pid.
        pid: u32,
        /// Whether the shim ends the tool now (`PADDOCK_SHIM_STALL_ACTION=end`).
        ends: bool,
    },
    /// The tool ended.
    Ended {
        /// The tool, as called.
        tool: &'a str,
        /// The project.
        project: &'a str,
        /// Epoch microseconds of the start.
        start_us: u64,
        /// How long it ran (the queue is not counted).
        duration: Duration,
        /// The exit code the shim leaves with.
        code: i32,
    },
}

/// A consumer of [`RunEvent`]s.
pub trait Sink {
    /// Takes one event. Must not panic and must not block for long.
    fn event(&mut self, e: &RunEvent<'_>);
}

/// The sinks of one run, in order.
#[derive(Default)]
pub struct Sinks {
    list: Vec<Box<dyn Sink>>,
}

impl Sinks {
    /// No sinks.
    pub fn new() -> Sinks {
        Sinks::default()
    }

    /// Adds a sink.
    pub fn add(&mut self, s: Box<dyn Sink>) {
        self.list.push(s);
    }

    /// Hands the event to every sink.
    pub fn emit(&mut self, e: &RunEvent<'_>) {
        for s in &mut self.list {
            s.event(e);
        }
    }
}
