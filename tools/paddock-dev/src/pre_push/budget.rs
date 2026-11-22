//! The push gate's timing vocabulary. A row's run time is its own and is bounded by its own cap ([`super::exec::ROW_CAP`]); the time
//! it waits for the machine-wide heavy slot is never run time and has its own bound, the queue's progress ([`super::exec::QUEUE_STALL`]).
//! There is no total budget shared by the rows of a push (2026-11-21, ADR 0102 amendment). Pure: instants and text in, numbers out.
//!
//! - [`parse_heavy_timing`]: the `<agent> timing queued Q s, ran R s` line `heavy` prints for every run.
//! - [`is_landing_ref`]: the refs whose pre-push gate takes the priority lane (`train/*`, `main`, a release tag).
//! - [`with_heavy_flags`]: puts the lane and the two limits into the `heavy run ... --` prefix.

use std::time::{Duration, Instant};

/// Queue and run seconds of one step, as `heavy` measured them.
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
    /// Waiting for the machine-wide slot since `since` (when the step was handed to `heavy`): every second since is queue time,
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

/// `heavy`'s `<agent> timing queued 12.3 s, ran 45.6 s` line, the last one in `log`.
pub fn parse_heavy_timing(log: &str) -> Option<Timing> {
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

/// Seconds, as `heavy` takes them.
fn secs(d: Duration) -> String {
    format!("{}", d.as_secs().max(1))
}

/// `prefix` (the `heavy run <agent> --` command) with the priority lane and the limits. Only our own `heavy` has them: a
/// `PADDOCK_HEAVY` of someone else's, or `heavy.sh`, is returned unchanged.
pub fn with_heavy_flags(prefix: &[String], priority: bool, queue_stall: Duration, run_max: Duration) -> Vec<String> {
    let ours = prefix.len() >= 3 && prefix[1] == "run" && prefix.last().is_some_and(|l| l == "--");
    if !ours {
        return prefix.to_vec();
    }
    let mut flags: Vec<String> = Vec::new();
    if priority {
        flags.push("--high".into());
    }
    flags.extend(["--queue-stall".into(), secs(queue_stall), "--run-max".into(), secs(run_max)]);
    let mut out = prefix[..2].to_vec();
    out.extend(flags);
    out.extend_from_slice(&prefix[2..]);
    out
}

/// `prefix` (our `heavy run <agent> --`) with `name` as the agent: a step's own name, so its lines in a log are told from any other
/// step's (`<name> holds the lock (pid`). Anything else (not our `heavy`) is returned unchanged.
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
        let log = "heavy: X timing queued 1.0 s, ran 2.0 s\nnoise\nheavy: pre-push timing queued 12.3 s, ran 45.6 s\n";
        assert_eq!(parse_heavy_timing(log), Some(Timing { queued: 12.3, ran: 45.6 }));
        assert_eq!(parse_heavy_timing("heavy: nothing here\n"), None);
        assert_eq!(parse_heavy_timing("a timing queued x s, ran 1.0 s"), None);
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
        let ours: Vec<String> = ["heavy", "run", "--high", "--queue-stall", "1", "pre-push", "--"]
            .map(String::from)
            .to_vec();
        assert_eq!(
            with_agent_name(&ours, "pre-push-7-1").join(" "),
            "heavy run --high --queue-stall 1 pre-push-7-1 --"
        );
        let other = vec!["heavy.sh".to_string()];
        assert_eq!(with_agent_name(&other, "x"), other);
    }

    #[test]
    fn the_flags_go_into_our_heavy_only() {
        let ours: Vec<String> = ["heavy", "run", "pre-push", "--"].map(String::from).to_vec();
        let got = with_heavy_flags(&ours, true, Duration::from_secs(1800), Duration::from_secs(900));
        assert_eq!(got.join(" "), "heavy run --high --queue-stall 1800 --run-max 900 pre-push --");
        let got = with_heavy_flags(&ours, false, Duration::from_secs(1800), Duration::from_millis(100));
        assert_eq!(got.join(" "), "heavy run --queue-stall 1800 --run-max 1 pre-push --");
        for other in [vec!["heavy.sh".to_string()], vec!["my-heavy".into(), "--x".into()], vec![]] {
            assert_eq!(with_heavy_flags(&other, true, MIN15, MIN15), other);
        }
    }
}
