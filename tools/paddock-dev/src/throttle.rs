//! How a heavy step gets the machine-wide slot, as pure data and text (ER-22, ER-24): the command that goes before it (`throttle run
//! pre-push --`, or the one `PADDOCK_THROTTLE` names), the flags and agent name `throttle` is given, the limits a step and its wait
//! are bound by, and what `throttle`'s own log lines say about the wait. No process is started and no file read here: the push gate
//! (`pre_push`), `./dev gen check`, `./dev ci run` and `./dev wait` share these, so all of them queue for the same slot. The part that
//! builds the `throttle` binary when none is on PATH is [`crate::heavy_tool`].

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// The whole capture's bound of the stack samples the push gate writes before it ends a hung step (`pre_push::stacks`): the kill is
/// never held longer than this by the looks. It is here because [`CAP_GRACE`] is counted from it.
pub const STACK_CAPTURE_LIMIT: Duration = Duration::from_secs(60);

/// What one row may run on its own, counted from the moment its command starts (from the heavy slot taken, never from the wait for it;
/// no total is shared by the rows of a push): the backstop, GitLab's default job timeout. Measured cause
/// (docs/research/2026-11-19-gate-hang-detection.md section 0, heavy's activity log 2026-11-14 to 19): the slowest healthy row known is
/// a cold app doctest build, 1122 s (docs/benchmarks.md, "Doctest row of the push gate"); 60 min is over three times it; no healthy
/// row ran longer. Before the stall judgement (WP-2) it is the only thing that ends a hung row. Queue time is not in it (a test reads
/// this constant: with no cap, the over-cap test is red).
pub const ROW_CAP: Duration = Duration::from_secs(60 * 60);

/// How long after [`ROW_CAP`] the outer limits of a step (heavy's `--run-max`, the process timeout) still wait: the cap is enforced by
/// the executor's monitor, which logs the stacks before it ends the tree, so these are only the backstop if the monitor is gone.
///
/// At least the stack capture's own bound plus 15 s ([`STACK_CAPTURE_LIMIT`]): the monitor ends the tree after the capture, and these
/// outer limits must not end it first, mid-capture (review d).
pub const CAP_GRACE: Duration = Duration::from_secs(STACK_CAPTURE_LIMIT.as_secs() + 15);

/// How long a step's wait for the machine-wide heavy slot may go without the queue making progress (`PADDOCK_QUEUE_STALL` seconds
/// overrides). The wait itself is not bounded by its length and is never run time: while slots are taken and released or the step's place
/// in the queue moves, it keeps waiting; a queue that shows the same picture for this long ends the wait as "not measured: queue
/// stalled, held by <who>", never a pass (ADR 0102, ER-33). Measured cause (ER-12): the gaps between two queue events (a lock taken or
/// released) while somebody waited, heavy's activity log of 2026-11-14 13:02Z to 2026-11-21 10:12Z on this Mac: 1,386 gaps, p50 81 s,
/// p90 314 s, p95 453 s, p99 813 s, max 1,446 s (docs/benchmarks.md, "Pre-push heavy queue progress gaps"). 30 minutes is over the
/// longest gap seen; a holder that really runs longer is ended by its own row cap, which is the owner's guard, not this wait's.
pub const QUEUE_STALL: Duration = Duration::from_secs(30 * 60);

/// The most steps seen waiting in heavy's queue at once: the longest queue in heavy's activity log, 2026-11-14 to 2026-11-21 ("position 7
/// of 13"; docs/benchmarks.md, "Pre-push heavy queue progress gaps").
const QUEUE_LENGTH_SEEN: u64 = 13;

/// The slowest healthy row measured, in seconds: a cold doctest build, 1,122 s (docs/benchmarks.md, "Doctest row of the push gate").
const SLOWEST_HEALTHY_ROW: u64 = 1122;

/// The outer process limit on one step's whole wait for the heavy slot, so that no call runs unbounded (ER-32). It is a backstop and not
/// the judgement: a queue that keeps moving is waited for and `throttle`'s stall judgement ends one that does not, so this only ends a
/// `throttle` client that is itself hung. Derived from data: the longest queue seen ([`QUEUE_LENGTH_SEEN`], 13) times the slowest healthy
/// row ([`SLOWEST_HEALTHY_ROW`]), 4 h 5 min, doubled for a queue longer than any seen: 8 h 10 min.
pub const WAIT_BACKSTOP: Duration = Duration::from_secs(2 * QUEUE_LENGTH_SEEN * SLOWEST_HEALTHY_ROW);

/// The line `throttle` writes when it holds the slot for a step (`<agent> holds the lock (pid N) at ...`): before it, a step is queued.
pub const LOCK_TAKEN: &str = " holds the lock (pid ";

/// The variable that, with `PADDOCK_THROTTLE=off`, lets a test run a heavy gate without the slot.
pub const TEST_THROTTLE_OFF: &str = "PADDOCK_TEST_THROTTLE_OFF";

/// The value of `PADDOCK_THROTTLE`, else of the old name `PADDOCK_HEAVY` (accepted until the settings that set it are changed).
pub fn throttle_override(env: &HashMap<String, String>) -> Option<&String> {
    env.get("PADDOCK_THROTTLE").or_else(|| env.get("PADDOCK_HEAVY"))
}

/// The command that puts a heavy step through the machine-wide slot (ER-24): `PADDOCK_THROTTLE` as given, else `throttle` on PATH (the
/// shell `heavy.sh` is retired: engineering-playbook's script is a thin wrapper of `throttle`). Empty when there is none (the caller says so).
pub fn throttle_prefix(env: &HashMap<String, String>, has: &dyn Fn(&str) -> bool) -> Vec<String> {
    if let Some(h) = throttle_override(env).filter(|h| !h.trim().is_empty()) {
        if h.trim() == "off" {
            return Vec::new();
        }
        return h.split_whitespace().map(str::to_string).collect();
    }
    if has("throttle") {
        return ["throttle", "run", "pre-push", "--"].map(String::from).to_vec();
    }
    Vec::new()
}

/// Whether `program` is a file in a folder of `env`'s `PATH`.
pub fn on_path(env: &HashMap<String, String>, program: &str) -> bool {
    let path = env.get("PATH").cloned().unwrap_or_default();
    std::env::split_paths(&path).any(|d| exists(&d, program))
}

fn exists(dir: &Path, program: &str) -> bool {
    dir.join(program).is_file() || (cfg!(windows) && dir.join(format!("{program}.exe")).is_file())
}

/// Whether `prefix` is our own `throttle run <agent> --` (and not someone else's wrapper, which takes none of our flags).
pub fn is_ours(prefix: &[String]) -> bool {
    prefix.len() >= 3 && prefix[1] == "run" && prefix.last().is_some_and(|l| l == "--")
}

/// `prefix` with only the priority lane (`--high`) when `train`: for a caller that sets no limits of its own (`./dev gen check`). Anything
/// that is not our `throttle run <agent> --` is returned unchanged.
pub fn with_priority(prefix: &[String], train: bool) -> Vec<String> {
    let ours = is_ours(prefix);
    if !ours || !train {
        return prefix.to_vec();
    }
    let mut out = prefix[..2].to_vec();
    out.push("--high".into());
    out.extend_from_slice(&prefix[2..]);
    out
}

/// `prefix` (our `throttle run <agent> --`) with `name` as the agent: a step's own name, so its lines in a log are told from any other
/// step's (`<name> holds the lock (pid`). Anything else (not our `throttle`) is returned unchanged.
pub fn with_agent_name(prefix: &[String], name: &str) -> Vec<String> {
    let ours = is_ours(prefix);
    let mut out = prefix.to_vec();
    if ours {
        let at = out.len() - 2;
        out[at] = name.to_string();
    }
    out
}

/// Who held the slot when a queue stalled, from heavy's line `<agent> queue stalled: ... (queued Q s, held by <who>), so the command
/// never ran` (the failure line after it, which carries the same words without the closing phrase, is skipped).
pub fn stall_line_holder(log: &str) -> Option<String> {
    log.lines().rev().find_map(|l| {
        let rest = l.split(" queue stalled: ").nth(1)?;
        let held = rest.split(", held by ").nth(1)?;
        let (who, _) = held.split_once(", so the command never ran")?;
        Some(who.strip_suffix(')').unwrap_or(who).to_string())
    })
}

/// What heavy last said about the wait, from its own `<agent> waiting 126s: <status>` lines in the step's log: `queued, position 7 of 8,
/// behind <agent>` while in the ticket queue, `the lock is held by <agent> (pid N) for 3 min` at the head. `None` before the first line.
pub fn wait_status(log: &str) -> Option<String> {
    log.lines().rev().find_map(|l| {
        let rest = l.split(" waiting ").nth(1)?;
        let (secs, status) = rest.split_once("s: ")?;
        secs.chars().all(|c| c.is_ascii_digit()).then(|| status.trim().to_string())
    })
}

/// Whether the output of a gate run through `throttle` shows that it never got the slot: it printed who holds the lock
/// ("the lock is held by <name> (pid <n>) ...") and never "<name> holds the lock". Returns the phrase for the verdict.
pub fn slot_wait(log: &str) -> Option<String> {
    if log.lines().any(|l| l.contains(LOCK_TAKEN)) {
        return None;
    }
    let held = log.lines().rev().find_map(|l| l.split("the lock is held by ").nth(1))?;
    let who = held.split(" for ").next().unwrap_or(held).trim();
    Some(format!("waiting for the heavy slot, held by {who}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heavy_is_found_when_the_variable_is_not_set() {
        let env = HashMap::new();
        assert!(throttle_prefix(&env, &|_| false).is_empty());
        assert!(
            throttle_prefix(&env, &|p| p == "heavy.sh").is_empty(),
            "the shell heavy.sh is retired: it is no slot tool of ours"
        );
        assert_eq!(throttle_prefix(&env, &|p| p == "throttle"), ["throttle", "run", "pre-push", "--"]);
        let set = HashMap::from([("PADDOCK_THROTTLE".to_string(), "my-heavy --x".to_string())]);
        assert_eq!(throttle_prefix(&set, &|_| true), ["my-heavy", "--x"]);
    }

    #[test]
    fn a_log_that_only_waited_names_the_holder() {
        let waited =
            "throttle: the lock is held by Bob (pid 12) for 3m, running: cargo clippy\nthrottle: the lock is held by Bob (pid 12) for 4m\n";
        assert_eq!(
            slot_wait(waited).as_deref(),
            Some("waiting for the heavy slot, held by Bob (pid 12)")
        );
        let ran = format!("{waited}throttle: pre-push holds the lock (pid 9) at 10:00:00Z, waited 5s\n");
        assert_eq!(slot_wait(&ran), None);
        assert_eq!(slot_wait("error: boom\n"), None);
    }

    #[test]
    fn heavy_off_runs_without_the_slot_and_is_the_only_way_to() {
        let off = HashMap::from([("PADDOCK_THROTTLE".to_string(), "off".to_string())]);
        assert!(throttle_prefix(&off, &|_| true).is_empty());
    }

    /// Heavy's own line names the holder of a stalled queue; the failure line after it (the same words without the closing phrase) is
    /// not taken for it, and a log without either names nobody.
    #[test]
    fn the_holder_of_a_stalled_queue_is_read_from_heavys_line() {
        let log = "pre-push-7-1 queue stalled: the queue made no progress for 1800 s (queued 1801.2 s, held by agent-7 (pid 41), other (pid 9)), so the command never ran\n\
                   heavy 0.1 (macos) exit 0x0000004F: queue stalled: no progress for 1800 s, held by agent-7 (pid 41), other (pid 9) (queued 1801.2 s); the command never ran. Next: x\n";
        assert_eq!(stall_line_holder(log).as_deref(), Some("agent-7 (pid 41), other (pid 9)"));
        assert_eq!(stall_line_holder("the lock is held by agent-7 (pid 41) for 3 min\n"), None);
        assert_eq!(stall_line_holder(""), None);
    }

    #[test]
    fn train_work_takes_the_priority_lane_in_our_throttle_only() {
        let ours: Vec<String> = ["throttle", "run", "pre-push", "--"].map(String::from).to_vec();
        assert_eq!(with_priority(&ours, true).join(" "), "throttle run --high pre-push --");
        assert_eq!(with_priority(&ours, false), ours, "normal work is unchanged");
        let other = vec!["heavy.sh".to_string()];
        assert_eq!(with_priority(&other, true), other, "someone else's wrapper has no such flag");
    }

    #[test]
    fn a_step_gets_its_own_agent_name_in_our_heavy_only() {
        let ours: Vec<String> = ["throttle", "run", "--high", "--queue-stall", "1", "pre-push", "--"]
            .map(String::from)
            .to_vec();
        assert_eq!(
            with_agent_name(&ours, "pre-push-7-1").join(" "),
            "throttle run --high --queue-stall 1 pre-push-7-1 --"
        );
        let other = vec!["heavy.sh".to_string()];
        assert_eq!(with_agent_name(&other, "x"), other);
    }
}
