//! The push gate's budget counts RUN time, not time queued for the machine-wide heavy slot (ADR 0102). Pure: instants and text in,
//! numbers out, so every rule is tested without a clock or a process.
//!
//! - [`BudgetClock`]: the 15-minute budget with credit for time queued behind OTHER agents' steps. Time a step queued while a
//!   sibling gate of the same push held the slot is run time of the push, not waiting for others, and earns no credit.
//! - [`parse_heavy_timing`]: the `<agent> timing queued Q s, ran R s` line `heavy` prints for every run.
//! - [`is_landing_ref`]: the refs whose pre-push gate takes the priority lane (`train/*`, `main`, a release tag).
//! - [`with_heavy_flags`]: puts the lane and the two limits into the `heavy run ... --` prefix.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Queue and run seconds of one step, as `heavy` measured them.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Timing {
    /// Seconds from the start until the command began (slot queue and load gate).
    pub queued: f64,
    /// Seconds the command ran.
    pub ran: f64,
}

/// How far the heavy step in flight has got, as the executor sees it live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Slot {
    /// Nothing is known (the executor does not look).
    #[default]
    Unknown,
    /// Waiting for the machine-wide slot since `since` (when the step was handed to `heavy`: the build of `heavy` before it is
    /// not waiting for others): every second since is waiting for others.
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

/// The heavy step of the push in flight, as the clock knows it: which one (the instant it was launched), the most its wait may
/// count (its own `--queue-max`) and how far it has got.
struct Live {
    token: Instant,
    max: Duration,
    slot: Slot,
}

#[derive(Default)]
struct Inner {
    credit: Duration,
    /// Spans in which a gate of this push held the slot.
    held: Vec<(Instant, Instant)>,
    /// The heavy step in flight, if any: its live wait counts in [`BudgetClock::left_live`] until [`BudgetClock::record`] or
    /// [`BudgetClock::end_step`] clears it, under the same lock as the credit, so a wait is never counted twice.
    live: Option<Live>,
}

/// The budget of one push gate: `total` of run time from `start`, plus the time its steps queued for others.
pub struct BudgetClock {
    start: Instant,
    total: Duration,
    inner: Mutex<Inner>,
}

/// The length of `[a, b]` covered by `spans` (which may overlap each other).
fn covered(a: Instant, b: Instant, spans: &[(Instant, Instant)]) -> Duration {
    let mut cut: Vec<(Instant, Instant)> = spans
        .iter()
        .filter_map(|&(s, e)| {
            let (s, e) = (s.max(a), e.min(b));
            (s < e).then_some((s, e))
        })
        .collect();
    cut.sort();
    let mut total = Duration::ZERO;
    let mut upto: Option<Instant> = None;
    for (s, e) in cut {
        let s = upto.map_or(s, |u| s.max(u));
        if e > s {
            total += e - s;
            upto = Some(e);
        }
    }
    total
}

impl BudgetClock {
    /// A budget of `total` run time starting at `start`.
    pub fn new(start: Instant, total: Duration) -> BudgetClock {
        BudgetClock {
            start,
            total,
            inner: Mutex::new(Inner::default()),
        }
    }

    /// The run time left at `now`: the budget, plus the credit, minus the time since the start.
    pub fn left(&self, now: Instant) -> Duration {
        let credit = self.inner.lock().map_or(Duration::ZERO, |i| i.credit);
        (self.start + self.total + credit).saturating_duration_since(now)
    }

    /// A heavy step of this push is launched at `token` and may wait `max` at most (its own queue limit).
    pub fn begin_step(&self, token: Instant, max: Duration) {
        if let Ok(mut i) = self.inner.lock() {
            i.live = Some(Live {
                token,
                max,
                slot: Slot::Unknown,
            });
        }
    }

    /// What the executor says about the step launched at `token`; ignored when that step has already ended (a late answer about a
    /// step whose credit is on the clock must not count again).
    pub fn observe(&self, token: Instant, slot: Slot) {
        if let Ok(mut i) = self.inner.lock()
            && let Some(l) = i.live.as_mut()
            && l.token == token
        {
            l.slot = slot;
        }
    }

    /// The step in flight is over without a timing to record (nothing was measured): its live wait stops counting.
    pub fn end_step(&self) {
        if let Ok(mut i) = self.inner.lock() {
            i.live = None;
        }
    }

    /// [`BudgetClock::left`] while a heavy step of this push is in flight: its wait so far is not run time either, but it is
    /// credited only when the step ends ([`BudgetClock::record`]), so a row that starts meanwhile would see the budget spent by
    /// waiting for others. The wait is what [`Slot`] says (queued: since it began; taken: until then), at most the step's queue
    /// limit; no step in flight, or nothing known (no live credit: the end-of-step credit is all there is, as for a `heavy.sh`
    /// of someone else's): none. Credit and live wait are read under one lock.
    pub fn left_live(&self, now: Instant) -> Duration {
        let Ok(i) = self.inner.lock() else { return Duration::ZERO };
        let waited = i.live.as_ref().map_or(Duration::ZERO, |l| {
            let w = match l.slot {
                Slot::Unknown => Duration::ZERO,
                Slot::Queued { since } => now.saturating_duration_since(since),
                Slot::Taken { since, at } => at.min(now).saturating_duration_since(since),
            };
            w.min(l.max)
        });
        (self.start + self.total + i.credit + waited).saturating_duration_since(now)
    }

    /// The total credit so far.
    pub fn credit(&self) -> Duration {
        self.inner.lock().map_or(Duration::ZERO, |i| i.credit)
    }

    /// A step that began at `started`, ended at `ended` and queued for `queued` before its command ran (heavy's own measure,
    /// clamped to the wall time it was seen to take). The part of the queue time in which no gate of this push held the slot is
    /// credited: that was waiting for others.
    pub fn record(&self, started: Instant, ended: Instant, queued: Duration) {
        let queued = queued.min(ended.saturating_duration_since(started));
        let held_at = started + queued;
        let Ok(mut i) = self.inner.lock() else { return };
        let own = covered(started, held_at, &i.held);
        i.credit += queued.saturating_sub(own);
        // Even a step that did not queue held the slot from `started`: a sibling that queued behind it is not waiting for others.
        i.held.push((held_at, ended));
        i.live = None;
    }
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
pub fn with_heavy_flags(prefix: &[String], priority: bool, queue_max: Duration, run_max: Duration) -> Vec<String> {
    let ours = prefix.len() >= 3 && prefix[1] == "run" && prefix.last().is_some_and(|l| l == "--");
    if !ours {
        return prefix.to_vec();
    }
    let mut flags: Vec<String> = Vec::new();
    if priority {
        flags.push("--high".into());
    }
    flags.extend(["--queue-max".into(), secs(queue_max), "--run-max".into(), secs(run_max)]);
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

    fn at(base: Instant, s: u64) -> Instant {
        base + Duration::from_secs(s)
    }
    const MIN15: Duration = Duration::from_secs(900);

    #[test]
    fn without_queueing_the_budget_is_the_wall_clock() {
        let b = Instant::now();
        let c = BudgetClock::new(b, MIN15);
        assert_eq!(c.left(at(b, 100)), Duration::from_secs(800));
        c.record(at(b, 0), at(b, 100), Duration::ZERO);
        assert_eq!(c.left(at(b, 100)), Duration::from_secs(800));
        assert_eq!(c.left(at(b, 2000)), Duration::ZERO);
    }

    #[test]
    fn time_queued_behind_other_agents_is_not_budget() {
        let b = Instant::now();
        let c = BudgetClock::new(b, MIN15);
        // queued 20 min behind someone else, then ran 5 min: 25 min on the wall, 5 min of budget spent
        c.record(at(b, 0), at(b, 1500), Duration::from_secs(1200));
        assert_eq!(c.credit(), Duration::from_secs(1200));
        assert_eq!(c.left(at(b, 1500)), Duration::from_secs(600));
    }

    #[test]
    fn a_step_still_queued_is_not_run_time_and_one_that_got_the_slot_counts_only_from_then() {
        let b = Instant::now();
        let c = BudgetClock::new(b, MIN15);
        let max = Duration::from_secs(1800);
        c.begin_step(b, max);
        // queued the whole 20 minutes: all run time is left
        c.observe(b, Slot::Queued { since: b });
        assert_eq!(c.left_live(at(b, 1200)), MIN15);
        // the slot taken at 300 s: at 700 s, 400 s have run
        c.observe(b, Slot::Taken { since: b, at: at(b, 300) });
        assert_eq!(c.left_live(at(b, 700)), Duration::from_secs(500));
        // the build of heavy before the wait began (queued since 100 s) is not waiting for others
        c.observe(b, Slot::Queued { since: at(b, 100) });
        assert_eq!(c.left_live(at(b, 700)), Duration::from_secs(800));
        // the wait counts at most the step's own queue limit
        c.begin_step(b, Duration::from_secs(60));
        c.observe(b, Slot::Queued { since: b });
        assert_eq!(c.left_live(at(b, 700)), Duration::from_secs(260));
        // nothing known, or no step in flight: the wall clock
        c.observe(b, Slot::Unknown);
        assert_eq!(c.left_live(at(b, 700)), Duration::from_secs(200));
        c.end_step();
        c.observe(b, Slot::Queued { since: b });
        assert_eq!(c.left_live(at(b, 700)), Duration::from_secs(200), "no step in flight");
    }

    #[test]
    fn a_late_answer_about_a_step_whose_credit_is_recorded_is_not_counted_twice() {
        let b = Instant::now();
        let c = BudgetClock::new(b, MIN15);
        c.begin_step(b, Duration::from_secs(1800));
        c.observe(b, Slot::Taken { since: b, at: at(b, 300) });
        // the step ends: its 300 s are credit now ...
        c.record(b, at(b, 400), Duration::from_secs(300));
        // ... and a stale answer of the executor about it does not add them again
        c.observe(b, Slot::Taken { since: b, at: at(b, 300) });
        assert_eq!(c.left_live(at(b, 400)), Duration::from_secs(800));
    }

    #[test]
    fn time_queued_behind_a_sibling_gate_of_the_same_push_is_run_time() {
        let b = Instant::now();
        let c = BudgetClock::new(b, MIN15);
        // A holds the slot 0..300; B started at 0, queued 300 behind A, ran until 400: nothing to credit
        c.record(at(b, 0), at(b, 300), Duration::ZERO);
        c.record(at(b, 0), at(b, 400), Duration::from_secs(300));
        assert_eq!(c.credit(), Duration::ZERO);
        assert_eq!(c.left(at(b, 400)), Duration::from_secs(500));
    }

    #[test]
    fn only_the_part_not_covered_by_a_sibling_is_credited() {
        let b = Instant::now();
        let c = BudgetClock::new(b, MIN15);
        c.record(at(b, 0), at(b, 300), Duration::ZERO); // sibling holds 0..300
        c.record(at(b, 0), at(b, 600), Duration::from_secs(500)); // queued 500, 300 of it behind the sibling
        assert_eq!(c.credit(), Duration::from_secs(200));
    }

    #[test]
    fn a_queue_time_longer_than_the_step_was_seen_to_take_is_clamped() {
        let b = Instant::now();
        let c = BudgetClock::new(b, MIN15);
        c.record(at(b, 0), at(b, 10), Duration::from_secs(10_000));
        assert_eq!(c.credit(), Duration::from_secs(10));
    }

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
        let ours: Vec<String> = ["heavy", "run", "--high", "--queue-max", "1", "pre-push", "--"]
            .map(String::from)
            .to_vec();
        assert_eq!(
            with_agent_name(&ours, "pre-push-7-1").join(" "),
            "heavy run --high --queue-max 1 pre-push-7-1 --"
        );
        let other = vec!["heavy.sh".to_string()];
        assert_eq!(with_agent_name(&other, "x"), other);
    }

    #[test]
    fn the_flags_go_into_our_heavy_only() {
        let ours: Vec<String> = ["heavy", "run", "pre-push", "--"].map(String::from).to_vec();
        let got = with_heavy_flags(&ours, true, Duration::from_secs(1800), Duration::from_secs(900));
        assert_eq!(got.join(" "), "heavy run --high --queue-max 1800 --run-max 900 pre-push --");
        let got = with_heavy_flags(&ours, false, Duration::from_secs(1800), Duration::from_millis(100));
        assert_eq!(got.join(" "), "heavy run --queue-max 1800 --run-max 1 pre-push --");
        for other in [vec!["heavy.sh".to_string()], vec!["my-heavy".into(), "--x".into()], vec![]] {
            assert_eq!(with_heavy_flags(&other, true, MIN15, MIN15), other);
        }
    }
}
