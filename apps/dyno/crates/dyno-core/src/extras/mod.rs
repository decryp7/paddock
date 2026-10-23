//! The extras (WP-13): small, independent, read-only features. Everything here is a pure function over text or values:
//! the agent runs the OS tools (with timeouts, behind a `ToolRunner`) and hands their output to these parsers.
//!
//! * [`changes`]: a timeline of installs, updates, new startup items and setting changes.
//! * [`sleep`]: what keeps the computer awake and what woke it.
//! * [`boot`]: how long start-up took and what slowed it.
//! * [`usage`]: network and energy per app.
//! * [`gpu`]: GPU use per process.
//! * [`software`]: the installed-software inventory and the inventory report.
//! * [`files`]: the large and duplicate file finder (grouping only; the agent walks and hashes).
//! * [`maintenance`]: when Dyno's heavier background work may run.

pub mod boot;
pub mod changes;
pub mod files;
pub mod gpu;
pub mod maintenance;
pub mod sleep;
pub mod software;
pub mod usage;

/// Whether a line of tool output is worth parsing at all (bounded, printable): used by every parser to drop noise.
pub(crate) fn clean(line: &str) -> &str {
    let t = line.trim();
    if t.len() > 2_000 {
        ""
    } else {
        t
    }
}

/// At most `max` characters of `s`.
pub(crate) fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}
