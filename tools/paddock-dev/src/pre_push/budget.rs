//! The push gate's timing vocabulary. A row's run time is its own and is bounded by its own cap ([`super::exec::ROW_CAP`]); the time
//! it waits for the machine-wide heavy slot is never run time and has its own bound, the queue's progress ([`super::exec::QUEUE_STALL`]).
//! There is no total budget shared by the rows of a push (2026-11-21, ADR 0102 amendment). Pure: instants and text in, numbers out.
//!
//! - [`parse_throttle_timing`]: the `<agent> timing queued Q s, ran R s` line `throttle` prints for every run.
//! - [`is_landing_ref`]: the refs whose pre-push gate takes the priority lane (`train/*`, `main`, a release tag).
//! - [`with_throttle_flags`]: puts the lane and the two limits into the `throttle run ... --` prefix.

use std::time::{Duration, Instant};

/// Queue and run seconds of one step, as `throttle` measured them.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Timing {
    /// Seconds from the start until the command began (slot queue and load gate).
    pub queued: f64,
    /// Seconds the command ran.
    pub ran: f64,
}

/// How far the heavy step in flight has got, as the executor sees it live: the row cap counts only from [`Slot::Taken`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Slot {
    /// Nothing is known (the executor does not look).
    #[default]
    Unknown,
    /// Waiting for the machine-wide slot since `since` (when the step was handed to `throttle`): every second since is queue time,
    /// never run time.
    Queued {
        /// When the wait began.
        since: Instant,
    },
    /// The slot was taken at `at` after waiting since `since`; the command has been running since `at`.
    Taken {
        /// When the wait began.
        since: Instant,
        /// When the slot was taken.
        at: Instant,
    },
}

/// `throttle`'s `<agent> timing queued 12.3 s, ran 45.6 s` line, the last one in `log`.
pub fn parse_throttle_timing(log: &str) -> Option<Timing> {
    log.lines().rev().find_map(|l| {
        let rest = l.split(" timing queued ").nth(1)?;
        let (q, rest) = rest.split_once(" s, ran ")?;
        let (r, _) = rest.split_once(" s")?;
        Some(Timing {
            queued: q.trim().parse().ok()?,
            ran: r.trim().parse().ok()?,
        })
    })
}

/// Whether a push to `remote_ref` is a landing: a `train/*` branch, `main`, or a release tag (`<app>-vX.Y.Z` or `vX.Y.Z`).
pub fn is_landing_ref(remote_ref: &str) -> bool {
    if remote_ref == "refs/heads/main" {
        return true;
    }
    if let Some(rest) = remote_ref.strip_prefix("refs/heads/train/") {
        return !rest.is_empty();
    }
    let Some(tag) = remote_ref.strip_prefix("refs/tags/") else {
        return false;
    };
    // `v1.2.3` or `<app>-v1.2.3` (a pre-release suffix is fine): digits.digits.digits right after the `v`.
    let version = |s: &str| {
        let mut p = s.splitn(3, '.');
        let digits = |x: Option<&str>| x.is_some_and(|x| x.starts_with(|c: char| c.is_ascii_digit()));
        digits(p.next()) && digits(p.next()) && digits(p.next())
    };
    match tag.strip_prefix('v') {
        Some(rest) => version(rest),
        None => tag.split_once("-v").is_some_and(|(app, rest)| !app.is_empty() && version(rest)),
    }
}

/// Seconds, as `throttle` takes them.
fn secs(d: Duration) -> String {
    format!("{}", d.as_secs().max(1))
}

/// The longest quiet gap of a passed step, in seconds, per long row kind: the `max_gap_secs` of `step-history.jsonl` over every passed
/// step on record (this Mac, 2026-11-14 to 2026-11-27: clippy 844 steps, clippy-windows 352, conformance-tests 592, member-audits 201,
/// tests 123). It is the longest time between two progress events, so an upper bound of the time with no output and no CPU that
/// `throttle --stall` counts. The limit of each kind is [`HANG_LIMITS`].
pub const HANG_MEASURED_GAP_SECS: [(&str, f64); 5] = [
    ("clippy", 230.1),
    ("clippy-windows", 291.0),
    ("conformance-tests", 294.8),
    ("member-audits", 113.1),
    ("tests", 291.5),
];

/// The time a long row may show no output and use no CPU before `throttle --stall` ends it (exit 81). A hang detector, never a speed
/// limit (ER-12): at least three times the longest gap measured for the kind ([`HANG_MEASURED_GAP_SECS`]), in whole 5-minute steps, so
/// a healthy row never gets near it, and a hung one (the local sccache server, 2026-11-27) costs 10 to 15 minutes, not the 60-minute row cap.
pub const HANG_LIMITS: [(&str, Duration); 5] = [
    ("clippy", Duration::from_secs(900)),
    ("clippy-windows", Duration::from_secs(900)),
    ("conformance-tests", Duration::from_secs(900)),
    ("member-audits", Duration::from_secs(600)),
    ("tests", Duration::from_secs(900)),
];

/// The hang limit of a row kind (the first word of its id), or `None` for a short row or one with no measured gap.
pub fn hang_limit(kind: &str) -> Option<Duration> {
    HANG_LIMITS.iter().find(|(k, _)| *k == kind).map(|(_, d)| *d)
}

/// `prefix` (the `throttle run <agent> --` command) with the priority lane and the limits (`hang`: throttle's `--stall`, for a long row).
/// Only our own `throttle` has them: a `PADDOCK_THROTTLE` of someone else's, or `heavy.sh`, is returned unchanged.
pub fn with_throttle_flags(
    prefix: &[String],
    priority: bool,
    queue_stall: Duration,
    run_max: Duration,
    hang: Option<Duration>,
) -> Vec<String> {
    let ours = prefix.len() >= 3 && prefix[1] == "run" && prefix.last().is_some_and(|l| l == "--");
    if !ours {
        return prefix.to_vec();
    }
    let mut flags: Vec<String> = Vec::new();
    if priority {
        flags.push("--high".into());
    }
    flags.extend(["--queue-stall".into(), secs(queue_stall), "--run-max".into(), secs(run_max)]);
    if let Some(h) = hang {
        flags.extend(["--stall".into(), secs(h)]);
    }
    let mut out = prefix[..2].to_vec();
    out.extend(flags);
    out.extend_from_slice(&prefix[2..]);
    out
}

/// `prefix` (our `throttle run <agent> --`) with `name` as the agent: a step's own name, so its lines in a log are told from any other
/// step's (`<name> holds the lock (pid`). Anything else (not our `throttle`) is returned unchanged.
pub fn with_agent_name(prefix: &[String], name: &str) -> Vec<String> {
    let ours = prefix.len() >= 3 && prefix[1] == "run" && prefix.last().is_some_and(|l| l == "--");
    let mut out = prefix.to_vec();
    if ours {
        let at = out.len() - 2;
        out[at] = name.to_string();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN15: Duration = Duration::from_secs(900);

    #[test]
    fn heavys_timing_line_is_read_and_the_last_one_wins() {
        let log = "throttle: X timing queued 1.0 s, ran 2.0 s\nnoise\nthrottle: pre-push timing queued 12.3 s, ran 45.6 s\n";
        assert_eq!(parse_throttle_timing(log), Some(Timing { queued: 12.3, ran: 45.6 }));
        assert_eq!(parse_throttle_timing("throttle: nothing here\n"), None);
        assert_eq!(parse_throttle_timing("a timing queued x s, ran 1.0 s"), None);
    }

    #[test]
    fn landings_are_trains_main_and_release_tags() {
        for yes in [
            "refs/heads/main",
            "refs/heads/train/2026-11-13-9",
            "refs/tags/v1.2.3",
            "refs/tags/dyno-v1.1.0",
            "refs/tags/pitwall-v1.0.4-rc.1",
        ] {
            assert!(is_landing_ref(yes), "{yes}");
        }
        for no in [
            "refs/heads/feat/x",
            "refs/heads/train/",
            "refs/heads/trainx/a",
            "refs/heads/main2",
            "refs/heads/fix/main",
            "refs/tags/v1",
            "refs/tags/nightly",
            "refs/tags/-v1.2.3",
            "refs/remotes/origin/main",
            "",
        ] {
            assert!(!is_landing_ref(no), "{no}");
        }
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

    #[test]
    fn the_flags_go_into_our_heavy_only() {
        let ours: Vec<String> = ["throttle", "run", "pre-push", "--"].map(String::from).to_vec();
        let got = with_throttle_flags(&ours, true, Duration::from_secs(1800), Duration::from_secs(900), None);
        assert_eq!(got.join(" "), "throttle run --high --queue-stall 1800 --run-max 900 pre-push --");
        let got = with_throttle_flags(&ours, false, Duration::from_secs(1800), Duration::from_millis(100), None);
        assert_eq!(got.join(" "), "throttle run --queue-stall 1800 --run-max 1 pre-push --");
        let got = with_throttle_flags(&ours, false, Duration::from_secs(1800), Duration::from_secs(900), Some(MIN15));
        assert_eq!(
            got.join(" "),
            "throttle run --queue-stall 1800 --run-max 900 --stall 900 pre-push --"
        );
        for other in [vec!["heavy.sh".to_string()], vec!["my-throttle".into(), "--x".into()], vec![]] {
            assert_eq!(with_throttle_flags(&other, true, MIN15, MIN15, Some(MIN15)), other);
        }
    }

    /// Every long heavy row kind has a hang limit that is a wide multiple of the longest quiet gap measured for it; a short row has none.
    #[test]
    fn the_long_rows_have_a_hang_limit_wider_than_their_measured_gap() {
        for (kind, longest) in HANG_MEASURED_GAP_SECS {
            let limit = hang_limit(kind).unwrap_or_else(|| panic!("{kind} has no hang limit"));
            assert!(
                limit.as_secs_f64() >= 3.0 * longest,
                "{kind}: {limit:?} is not a wide margin over {longest} s"
            );
        }
        for kind in ["tests", "clippy", "clippy-windows", "conformance-tests", "member-audits"] {
            assert!(hang_limit(kind).is_some(), "{kind}");
        }
        for kind in ["fmt", "lockfile", "commit-lint", "gates-register", "doctest"] {
            assert_eq!(hang_limit(kind), None, "{kind} is short or has no measured gap: no hang limit");
        }
    }
}
