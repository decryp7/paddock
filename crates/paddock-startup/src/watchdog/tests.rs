//! The watchdog's decisions with a fake clock (ADR 0058's tests, ported from Debrief, plus the liveness heartbeat of plan
//! §11).

use super::*;
use crate::Choice;
use std::time::Duration;

/// The timings of the first design: a reloaded page is asked about after one more `ready_timeout` and Keep Waiting does not lengthen
/// the wait. The tests below that are not about the longer waits use them, so their numbers stay readable.
fn quick() -> Config {
    Config {
        ask_timeout: Config::default().ready_timeout,
        max_doublings: 0,
        ..Config::default()
    }
}

/// `quick()` with the long-silence question out of reach: for the tests of the mark and of the pings.
fn marks_only() -> Config {
    Config {
        ask_timeout: Duration::from_secs(3_600),
        ..quick()
    }
}

/// The marks and pings only: the long-silence question (`AskHung`) is the subject of its own tests.
fn without_questions(actions: Vec<(u64, Action)>) -> Vec<(u64, Action)> {
    actions
        .into_iter()
        .filter(|(_, a)| !matches!(a, Action::AskHung(_)))
        .collect()
}

fn at(t0: Instant, ms: u64) -> Instant {
    t0 + Duration::from_millis(ms)
}

/// Ticks every 500 ms from `from` to `to` (ms after t0), collecting the actions (pings left out).
fn run(w: &mut Watchdog, t0: Instant, from: u64, to: u64) -> Vec<(u64, Action)> {
    run_all(w, t0, from, to)
        .into_iter()
        .filter(|(_, a)| !matches!(a, Action::Ping { .. }))
        .collect()
}

/// Ticks every 500 ms from `from` to `to`, collecting every action.
fn run_all(w: &mut Watchdog, t0: Instant, from: u64, to: u64) -> Vec<(u64, Action)> {
    let mut out = Vec::new();
    let mut t = from;
    while t <= to {
        out.extend(w.due(at(t0, t)).into_iter().map(|a| (t, a)));
        t += 500;
    }
    out
}

/// Ticks like `run_all` and answers every ping at once (a healthy page).
fn run_answering(w: &mut Watchdog, t0: Instant, from: u64, to: u64) -> Vec<(u64, Action)> {
    let mut out = Vec::new();
    let mut t = from;
    while t <= to {
        for a in w.due(at(t0, t)) {
            if let Action::Ping { label, seq } = &a {
                assert_eq!(w.pong(label, *seq, at(t0, t)), Pong::Alive);
            } else {
                out.push((t, a));
            }
        }
        t += 500;
    }
    out
}

fn ready_at(w: &mut Watchdog, label: &str, t: Instant) {
    w.page_started(label, t);
    assert_eq!(w.ready(label, t), None);
}

// --- Start-up (ADR 0058) ---

#[test]
fn a_window_that_reports_ready_in_time_is_left_alone() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(quick());
    w.page_started("main", t0);
    assert!(!w.is_ready("main"));
    assert!(run(&mut w, t0, 0, 9_500).is_empty());
    assert_eq!(w.ready("main", at(t0, 9_500)), None);
    assert!(w.is_ready("main"));
    assert!(run_answering(&mut w, t0, 10_000, 60_000).is_empty());
}

#[test]
fn a_silent_window_is_reloaded_once_then_the_user_is_asked() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(quick());
    w.page_started("main", t0);
    assert_eq!(
        run(&mut w, t0, 0, 30_000),
        vec![
            (10_000, Action::Reload("main".into())),
            (20_000, Action::AskStartup("main".into()))
        ]
    );
    // Asking: nothing more until the user answers.
    assert!(run_all(&mut w, t0, 30_500, 60_000).is_empty());
}

#[test]
fn the_reloaded_page_starting_restarts_its_clock_and_hears_why_once_ready() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(quick());
    w.page_started("main", t0);
    assert_eq!(run(&mut w, t0, 0, 10_000).len(), 1);
    w.page_started("main", at(t0, 10_200));
    assert!(
        run(&mut w, t0, 10_500, 20_000).is_empty(),
        "the reload's own page gets a full timeout"
    );
    assert_eq!(
        w.ready("main", at(t0, 20_000)),
        Some(Recovery::Unresponsive)
    );
    assert_eq!(w.ready("main", at(t0, 20_000)), None, "told once");
    // A later reload (by the user) is a fresh start again: reload first, not the question.
    w.page_started("main", at(t0, 40_000));
    assert_eq!(
        run(&mut w, t0, 40_000, 50_000),
        vec![(50_000, Action::Reload("main".into()))]
    );
}

#[test]
fn keep_waiting_gives_another_timeout_before_asking_again() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(quick());
    w.page_started("d-main-1", t0);
    run(&mut w, t0, 0, 20_000);
    w.keep_waiting("d-main-1", at(t0, 25_000));
    assert_eq!(
        run(&mut w, t0, 25_000, 40_000),
        vec![(35_000, Action::AskStartup("d-main-1".into()))]
    );
}

#[test]
fn keep_waiting_does_nothing_for_a_window_that_is_not_asked_about() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(quick());
    w.page_started("main", t0);
    w.keep_waiting("main", at(t0, 9_000));
    assert_eq!(
        run(&mut w, t0, 0, 10_000),
        vec![(10_000, Action::Reload("main".into()))],
        "the clock was not reset"
    );
    ready_at(&mut w, "w-2", t0);
    w.keep_waiting("w-2", t0);
    assert!(w.is_ready("w-2"));
}

#[test]
fn a_long_pause_of_the_app_itself_restarts_the_clocks_instead_of_firing() {
    // The app process stopped for 20 s (CPU limiter) right after the window started loading: on resume the window
    // must get its full timeout, not be reloaded at once.
    let t0 = Instant::now();
    let mut w = Watchdog::new(quick());
    w.page_started("main", t0);
    assert!(run(&mut w, t0, 0, 2_000).is_empty());
    assert!(
        w.due(at(t0, 22_000)).is_empty(),
        "resumed after a pause: no action"
    );
    assert!(run(&mut w, t0, 22_500, 31_500).is_empty());
    assert_eq!(w.due(at(t0, 32_000)), vec![Action::Reload("main".into())]);
}

#[test]
fn a_paused_webview_that_resumes_before_the_deadline_is_not_reloaded() {
    // Only the content process is paused (the app keeps ticking): if it resumes and reports ready within the timeout,
    // nothing happens; the watchdog is in the app process, which the webview's pause cannot stop.
    let t0 = Instant::now();
    let mut w = Watchdog::new(quick());
    w.page_started("main", t0);
    assert!(run(&mut w, t0, 0, 9_000).is_empty());
    assert_eq!(w.ready("main", at(t0, 9_000)), None);
    assert!(run_answering(&mut w, t0, 9_500, 30_000).is_empty());
}

#[test]
fn a_terminated_content_process_is_reported_to_the_reloaded_page() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(quick());
    ready_at(&mut w, "main", t0);
    assert!(w.terminated("main", at(t0, 5_000)));
    assert!(!w.is_ready("main"));
    assert!(run(&mut w, t0, 5_000, 14_500).is_empty());
    assert_eq!(w.ready("main", at(t0, 14_500)), Some(Recovery::Terminated));
}

#[test]
fn terminated_reloads_are_rate_limited_per_window() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(quick());
    let allowed: Vec<bool> = (0..5)
        .map(|i| w.terminated("w-crashy", at(t0, i * 1_000)))
        .collect();
    assert_eq!(allowed, [true, true, true, false, false]);
    assert!(w.terminated("w-other", t0), "per window");
    assert!(
        w.terminated("w-crashy", at(t0, 61_000)),
        "the window moves on"
    );
    w.closed("w-crashy");
    assert!(
        w.terminated("w-crashy", at(t0, 61_500)),
        "a closed window's history is forgotten"
    );
}

#[test]
fn closed_windows_and_unknown_labels_are_ignored() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(quick());
    w.page_started("w-2", t0);
    w.closed("w-2");
    assert!(run_all(&mut w, t0, 0, 30_000).is_empty());
    assert_eq!(w.ready("nobody", t0), None);
    w.keep_waiting("nobody", t0);
    w.set_visible("nobody", false, t0);
    assert_eq!(w.pong("nobody", 1, t0), Pong::Alive);
    assert!(!w.is_ready("nobody"));
    assert!(!w.is_unresponsive("nobody"));
    assert!(w.labels().is_empty());
}

#[test]
fn several_windows_time_out_in_label_order() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(quick());
    for l in ["w-3", "main", "d-main-1"] {
        w.page_started(l, t0);
    }
    assert_eq!(w.labels(), ["d-main-1", "main", "w-3"]);
    assert!(run(&mut w, t0, 0, 9_500).is_empty());
    let fired: Vec<String> = w
        .due(at(t0, 10_000))
        .iter()
        .map(|a| a.label().to_string())
        .collect();
    assert_eq!(fired, ["d-main-1", "main", "w-3"]);
}

#[test]
fn boot_errors_are_capped_per_page_load() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(Config {
        max_boot_errors: 2,
        ..Config::default()
    });
    w.page_started("main", t0);
    let admitted: Vec<bool> = (0..4).map(|_| w.admit_boot_error("main")).collect();
    assert_eq!(admitted, [true, true, false, false]);
    assert!(w.admit_boot_error("w-2"), "per window");
    w.page_started("main", at(t0, 1_000));
    assert!(
        w.admit_boot_error("main"),
        "a new page load starts a new count"
    );
    w.closed("main");
    assert!(w.admit_boot_error("main"));
    assert_eq!(w.config().max_boot_errors, 2);
}

// --- Liveness after start-up (plan §11) ---

#[test]
fn a_ready_window_is_pinged_every_interval_with_rising_numbers() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(quick());
    ready_at(&mut w, "main", t0);
    let pings: Vec<(u64, Action)> = run_all(&mut w, t0, 500, 6_000)
        .into_iter()
        .inspect(|(t, a)| {
            if let Action::Ping { label, seq } = a {
                w.pong(label, *seq, at(t0, *t));
            }
        })
        .collect();
    assert_eq!(
        pings,
        vec![
            (
                2_000,
                Action::Ping {
                    label: "main".into(),
                    seq: 1
                }
            ),
            (
                4_000,
                Action::Ping {
                    label: "main".into(),
                    seq: 2
                }
            ),
            (
                6_000,
                Action::Ping {
                    label: "main".into(),
                    seq: 3
                }
            ),
        ]
    );
}

#[test]
fn a_page_that_stops_answering_after_ready_is_marked_unresponsive() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(quick());
    ready_at(&mut w, "main", t0);
    // Answers the first two pings, then hangs.
    for (t, a) in run_all(&mut w, t0, 500, 4_000) {
        if let Action::Ping { label, seq } = a {
            assert_eq!(w.pong(&label, seq, at(t0, t)), Pong::Alive);
        }
    }
    let actions = without_questions(run(&mut w, t0, 4_500, 30_000));
    // Pings 3 to 7 go unanswered: the fifth miss is noticed when ping 8 is due, 10 s after the first unanswered ping.
    assert_eq!(actions, vec![(16_000, Action::Unresponsive("main".into()))]);
    assert!(w.is_unresponsive("main"));
    assert!(
        w.is_ready("main"),
        "it did start; it is not waiting for ready"
    );
    // Pings go on while it is marked, so a page that resumes is noticed.
    let pings = run_all(&mut w, t0, 30_500, 34_000);
    assert_eq!(pings.len(), 2);
    let Some((t, Action::Ping { seq, .. })) = pings.last().cloned() else {
        panic!("a ping")
    };
    assert_eq!(
        w.pong("main", seq - 1, at(t0, t)),
        Pong::Recovered,
        "a late answer to an earlier ping counts"
    );
    assert!(!w.is_unresponsive("main"));
    assert_eq!(
        w.ready("main", at(t0, t)),
        None,
        "a page that recovered by itself was not reloaded"
    );
}

#[test]
fn an_unresponsive_page_the_user_reloads_hears_why() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(quick());
    ready_at(&mut w, "main", t0);
    assert_eq!(
        without_questions(run(&mut w, t0, 500, 20_000)),
        vec![(12_000, Action::Unresponsive("main".into()))]
    );
    // The user chose Reload: the page starts loading again and its start-up clock applies.
    w.page_started("main", at(t0, 21_000));
    assert!(!w.is_unresponsive("main"));
    assert!(
        run_all(&mut w, t0, 21_000, 25_000).is_empty(),
        "no pings while it is starting"
    );
    assert_eq!(w.ready("main", at(t0, 25_000)), Some(Recovery::Hung));
}

#[test]
fn keep_waiting_on_an_unresponsive_page_gives_it_as_many_pings_again() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(marks_only());
    ready_at(&mut w, "main", t0);
    assert_eq!(run(&mut w, t0, 500, 12_000).len(), 1);
    w.keep_waiting("main", at(t0, 13_000));
    assert!(!w.is_unresponsive("main"));
    assert_eq!(
        run(&mut w, t0, 13_000, 30_000),
        vec![(25_000, Action::Unresponsive("main".into()))]
    );
}

#[test]
fn stale_and_unknown_pongs_change_nothing() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(quick());
    ready_at(&mut w, "main", t0);
    assert_eq!(w.pong("main", 1, t0), Pong::Alive, "not sent yet");
    let pings = run_all(&mut w, t0, 500, 2_000);
    assert_eq!(pings.len(), 1);
    assert_eq!(w.pong("main", 1, at(t0, 2_000)), Pong::Alive);
    assert_eq!(
        w.pong("main", 1, at(t0, 2_000)),
        Pong::Alive,
        "answered twice"
    );
    assert_eq!(w.pong("main", 0, at(t0, 2_000)), Pong::Alive);
}

#[test]
fn a_paused_app_process_does_not_mark_its_windows_unresponsive() {
    // The app process (the one that pings) is stopped for 30 s: on resume no window has "missed" pings; they start
    // again one interval later.
    let t0 = Instant::now();
    let mut w = Watchdog::new(quick());
    ready_at(&mut w, "main", t0);
    ready_at(&mut w, "w-2", t0);
    assert!(run_answering(&mut w, t0, 500, 3_000).is_empty());
    // The process stops at 3 s and resumes at 33 s.
    assert!(
        w.due(at(t0, 33_000)).is_empty(),
        "the resume itself: no ping, no mark"
    );
    assert!(run_answering(&mut w, t0, 33_500, 60_000).is_empty());
    assert!(!w.is_unresponsive("main") && !w.is_unresponsive("w-2"));
}

#[test]
fn a_paused_app_process_with_a_hung_page_still_notices_the_hang_after_resume() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(quick());
    ready_at(&mut w, "main", t0);
    assert!(run(&mut w, t0, 500, 7_000).is_empty());
    assert!(
        w.due(at(t0, 40_000)).is_empty(),
        "the outstanding pings are forgotten on resume"
    );
    assert_eq!(
        without_questions(run(&mut w, t0, 40_500, 60_000)),
        vec![(52_000, Action::Unresponsive("main".into()))]
    );
}

#[test]
fn hidden_windows_are_not_pinged_and_start_again_when_shown() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(marks_only());
    ready_at(&mut w, "main", t0);
    assert_eq!(
        run_all(&mut w, t0, 500, 3_000).len(),
        1,
        "one unanswered ping"
    );
    w.set_visible("main", false, at(t0, 3_000));
    w.set_visible("main", false, at(t0, 3_200));
    assert!(
        run_all(&mut w, t0, 3_500, 60_000).is_empty(),
        "minimised for a minute: no pings, no mark"
    );
    w.set_visible("main", true, at(t0, 60_000));
    let after = run_all(&mut w, t0, 60_500, 62_000);
    assert_eq!(
        after,
        vec![(
            62_000,
            Action::Ping {
                label: "main".into(),
                seq: 2
            }
        )]
    );
    assert_eq!(
        run(&mut w, t0, 62_500, 80_000),
        vec![(72_000, Action::Unresponsive("main".into()))]
    );
}

#[test]
fn a_window_hidden_while_starting_is_pinged_only_once_shown() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(quick());
    w.page_started("w-2", t0);
    w.set_visible("w-2", false, t0);
    assert_eq!(w.ready("w-2", at(t0, 1_000)), None);
    assert!(run_all(&mut w, t0, 1_000, 10_000).is_empty());
    w.set_visible("w-2", true, at(t0, 10_000));
    assert_eq!(
        run_all(&mut w, t0, 10_500, 12_000),
        vec![(
            12_000,
            Action::Ping {
                label: "w-2".into(),
                seq: 1
            }
        )]
    );
}

#[test]
fn a_window_shown_again_while_still_starting_keeps_its_start_up_clock() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(quick());
    w.page_started("w-2", t0);
    w.set_visible("w-2", false, at(t0, 1_000));
    w.set_visible("w-2", true, at(t0, 2_000));
    assert_eq!(
        run(&mut w, t0, 0, 10_000),
        vec![(10_000, Action::Reload("w-2".into()))]
    );
}

#[test]
fn several_unresponsive_windows_come_in_label_order_each_before_its_ping() {
    let t0 = Instant::now();
    let mut w = Watchdog::new(marks_only());
    ready_at(&mut w, "w-2", t0);
    ready_at(&mut w, "main", t0);
    let at12 = run_all(&mut w, t0, 500, 12_000)
        .into_iter()
        .filter(|(t, _)| *t == 12_000)
        .map(|(_, a)| a)
        .collect::<Vec<_>>();
    assert_eq!(
        at12,
        vec![
            Action::Unresponsive("main".into()),
            Action::Ping {
                label: "main".into(),
                seq: 6
            },
            Action::Unresponsive("w-2".into()),
            Action::Ping {
                label: "w-2".into(),
                seq: 6
            },
        ]
    );
}

fn visible_default() -> Watchdog {
    Watchdog::default()
}

#[test]
fn the_default_asks_a_visible_window_no_earlier_than_seventy_seconds_after_its_page_began_loading()
{
    let t0 = Instant::now();
    let mut w = visible_default();
    w.page_started("main", t0);
    // Reloaded at 10 s; then 60 s more before the question.
    assert_eq!(
        run(&mut w, t0, 0, 80_000),
        vec![
            (10_000, Action::Reload("main".into())),
            (70_000, Action::AskStartup("main".into()))
        ]
    );
}

#[test]
fn a_hidden_window_is_never_asked_about_and_gets_a_fresh_wait_when_it_is_shown() {
    let t0 = Instant::now();
    let mut w = visible_default();
    w.page_started("main", t0);
    w.set_visible("main", false, t0);
    // Ten minutes hidden (a login start): reloaded once, never asked.
    let acts = run(&mut w, t0, 0, 600_000);
    assert_eq!(acts, vec![(10_000, Action::Reload("main".into()))]);
    // Shown at 600 s: the wait starts then, not at once.
    w.set_visible("main", true, at(t0, 600_000));
    assert!(run(&mut w, t0, 600_500, 659_000).is_empty());
    assert_eq!(
        run(&mut w, t0, 659_500, 661_000),
        vec![(660_000, Action::AskStartup("main".into()))]
    );
}

#[test]
fn each_keep_waiting_doubles_the_next_wait_up_to_the_limit() {
    let t0 = Instant::now();
    let mut w = visible_default();
    w.page_started("main", t0);
    let mut asked_at = at(t0, 0);
    let mut waits = Vec::new();
    let mut now_ms = 0;
    // Reload at 10 s, then four questions, each answered with Keep Waiting.
    for _ in 0..5 {
        loop {
            now_ms += 500;
            let now = at(t0, now_ms);
            let acts = w.due(now);
            if acts.iter().any(|a| matches!(a, Action::AskStartup(_))) {
                waits.push(now.duration_since(asked_at).as_secs());
                asked_at = now;
                w.keep_waiting("main", now);
                break;
            }
            assert!(now_ms < 3_000_000, "never asked");
        }
    }
    // 70 s from the start (10 s to the reload counted in the first), then 120, 240, 480 and 480 (three doublings at most).
    assert_eq!(waits, [70, 120, 240, 480, 480], "{waits:?}");
}

#[test]
fn a_page_that_becomes_ready_forgets_the_doubling() {
    let t0 = Instant::now();
    let mut w = visible_default();
    w.page_started("main", t0);
    run(&mut w, t0, 0, 70_500);
    w.keep_waiting("main", at(t0, 70_500));
    assert_eq!(
        w.ready("main", at(t0, 71_000)),
        Some(Recovery::Unresponsive)
    );
    w.page_started("main", at(t0, 100_000));
    // A fresh page load starts from the first wait again.
    let acts = run(&mut w, t0, 100_000, 180_000);
    assert_eq!(
        acts.iter()
            .filter(|(_, a)| matches!(a, Action::AskStartup(_)))
            .count(),
        1,
        "{acts:?}"
    );
    assert!(
        acts.iter()
            .any(|(t, a)| *t == 170_000 && matches!(a, Action::AskStartup(_))),
        "{acts:?}"
    );
}

#[test]
fn inconsistent_ask_timeouts_are_refused() {
    let bad = Config {
        ask_timeout: Duration::from_secs(1),
        ..Config::default()
    };
    assert_eq!(bad.checked(), Err(crate::ConfigError::AskBeforeReload));
    assert_eq!(Config::default().ask_wait(0), Duration::from_secs(60));
    assert_eq!(
        Config::default().ask_wait(9),
        Duration::from_secs(480),
        "the doubling stops at the limit"
    );
}

/// Ticks every 500 ms from `from` to `to`, answering every ping when `alive` says so at that time; returns every action by time.
fn run_with(
    w: &mut Watchdog,
    t0: Instant,
    from: u64,
    to: u64,
    alive: impl Fn(u64) -> bool,
) -> Vec<(u64, Action)> {
    let mut out = Vec::new();
    let mut ms = from;
    while ms <= to {
        let now = at(t0, ms);
        for a in w.due(now) {
            if let (Action::Ping { label, seq }, true) = (&a, alive(ms)) {
                w.pong(label, *seq, now);
            }
            out.push((ms, a));
        }
        ms += 500;
    }
    out
}

fn hung_questions(acts: &[(u64, Action)]) -> Vec<u64> {
    acts.iter()
        .filter(|(_, a)| matches!(a, Action::AskHung(_)))
        .map(|(t, _)| *t)
        .collect()
}

#[test]
fn a_ready_page_stopped_by_a_limiter_is_marked_at_once_but_asked_about_only_after_a_minute() {
    let t0 = Instant::now();
    let mut w = Watchdog::default();
    w.page_started("main", t0);
    assert_eq!(w.ready("main", at(t0, 100)), None);
    let acts = run_with(&mut w, t0, 0, 130_000, |_| false);
    let marked = acts
        .iter()
        .find(|(_, a)| matches!(a, Action::Unresponsive(_)))
        .map(|(t, _)| *t)
        .expect("marked");
    assert!(
        marked < 15_000,
        "the mark (for a dialog's OS close) stays quick: {marked}"
    );
    let asked = hung_questions(&acts);
    assert_eq!(asked.len(), 1, "one question, not a stream: {asked:?}");
    assert!(
        asked[0] >= 60_000 && asked[0] <= 70_000,
        "the first question comes after the long wait: {asked:?}"
    );
}

#[test]
fn a_page_that_comes_back_and_stops_again_does_not_raise_a_second_question_while_the_first_is_open_and_waits_longer_after()
 {
    let t0 = Instant::now();
    let mut w = Watchdog::default();
    w.page_started("main", t0);
    w.ready("main", at(t0, 100));
    // Stopped, asked at about 62 s.
    let first = run_with(&mut w, t0, 0, 70_000, |_| false);
    let q1 = hung_questions(&first);
    assert_eq!(q1.len(), 1, "{q1:?}");
    // The limiter lets the page run for a few seconds (the question is still open on screen), then stops it again.
    let alive_between = |ms: u64| (70_500..80_000).contains(&ms);
    let second = run_with(&mut w, t0, 70_500, 400_000, alive_between);
    assert!(
        hung_questions(&second).is_empty(),
        "no second dialog on top of the open one: {:?}",
        hung_questions(&second)
    );
    // The person clicks Keep Waiting (the page was running at that moment).
    w.question_answered("main");
    w.keep_waiting("main", at(t0, 400_500));
    let third = run_with(&mut w, t0, 400_500, 800_000, |_| false);
    let q3 = hung_questions(&third);
    assert_eq!(q3.len(), 1, "{q3:?}");
    // The first wait was about 60 s; after one question the next is about 120 s, not 12 s: the answer was not forgotten when the page recovered.
    assert!(
        q3[0] - 400_500 >= 110_000,
        "asked again after only {} ms",
        q3[0] - 400_500
    );
}

#[test]
fn a_page_that_stays_healthy_for_ten_minutes_forgets_the_earlier_questions() {
    let t0 = Instant::now();
    let mut w = Watchdog::default();
    w.page_started("main", t0);
    w.ready("main", at(t0, 100));
    let first = run_with(&mut w, t0, 0, 70_000, |_| false);
    assert_eq!(hung_questions(&first).len(), 1);
    w.question_answered("main");
    // Healthy for eleven minutes.
    run_with(&mut w, t0, 70_500, 740_000, |_| true);
    let again = run_with(&mut w, t0, 740_500, 820_000, |_| false);
    let q = hung_questions(&again);
    assert_eq!(q.len(), 1, "{q:?}");
    assert!(
        q[0] - 740_500 <= 75_000,
        "back to the first wait after a long healthy time: {}",
        q[0] - 740_500
    );
}

#[test]
fn hiding_and_showing_a_window_over_and_over_does_not_keep_a_hung_start_from_being_asked() {
    let t0 = Instant::now();
    let mut w = Watchdog::default();
    w.page_started("main", t0);
    let mut asked = Vec::new();
    let mut ms = 0;
    let mut visible = true;
    while ms <= 600_000 {
        let now = at(t0, ms);
        // Hidden and shown every 50 s (a window that comes and goes).
        if ms % 50_000 == 0 && ms > 0 {
            visible = !visible;
            w.set_visible("main", visible, now);
        }
        for a in w.due(now) {
            if matches!(a, Action::AskStartup(_)) {
                asked.push(ms);
            }
        }
        ms += 500;
    }
    assert_eq!(
        asked.len(),
        1,
        "a hung start is asked about once its visible time is long enough: {asked:?}"
    );
    assert!(asked[0] < 300_000, "{asked:?}");
}

#[test]
fn a_start_up_question_and_a_hung_page_question_are_never_open_together() {
    let t0 = Instant::now();
    let mut w = Watchdog::default();
    w.page_started("main", t0);
    let first = run_with(&mut w, t0, 0, 75_000, |_| false);
    assert_eq!(
        first
            .iter()
            .filter(|(_, a)| matches!(a, Action::AskStartup(_)))
            .count(),
        1,
        "{first:?}"
    );
    // The page finally becomes ready while that question is still on screen, then stops answering.
    w.ready("main", at(t0, 76_000));
    let later = run_with(&mut w, t0, 76_000, 400_000, |_| false);
    assert!(
        hung_questions(&later).is_empty(),
        "a second dialog on top of the open one: {:?}",
        hung_questions(&later)
    );
    assert!(w.is_question_open("main"));
    // Answering the one question frees the window for the other.
    w.question_answered("main");
    assert!(!w.is_question_open("main"));
    let after = run_with(&mut w, t0, 400_500, 600_000, |_| false);
    assert_eq!(hung_questions(&after).len(), 1, "{after:?}");
}

#[test]
fn a_hung_page_question_on_screen_holds_back_the_start_up_question_of_a_reload() {
    let t0 = Instant::now();
    let mut w = Watchdog::default();
    w.page_started("main", t0);
    w.ready("main", at(t0, 100));
    let hung = run_with(&mut w, t0, 0, 70_000, |_| false);
    assert_eq!(hung_questions(&hung).len(), 1);
    // The page is reloaded (by anything but the answer) and never gets ready: the open question is not joined by a second.
    w.page_started("main", at(t0, 71_000));
    let acts = run_with(&mut w, t0, 71_000, 300_000, |_| false);
    assert!(
        !acts.iter().any(|(_, a)| matches!(a, Action::AskStartup(_))),
        "{acts:?}"
    );
}

/// Opens the question about a hung page "main" and returns its identity; the watchdog is `ready` at 100 ms and silent until 70 s.
fn hung_with_question(w: &mut Watchdog, t0: Instant) -> u64 {
    w.page_started("main", t0);
    w.ready("main", at(t0, 100));
    let acts = run_with(w, t0, 0, 70_000, |_| false);
    assert_eq!(hung_questions(&acts).len(), 1, "{acts:?}");
    w.question_seq("main").expect("the question is on screen")
}

#[test]
fn a_question_on_screen_is_closed_when_the_page_answers_again_and_its_late_answer_is_stale() {
    let t0 = Instant::now();
    let mut w = Watchdog::default();
    let seq = hung_with_question(&mut w, t0);
    assert!(w.is_question_open("main"));
    // The page answers its latest ping: it recovered (the dialog was put up at about 62 s and is still there).
    let sent = run_all(&mut w, t0, 70_500, 71_000)
        .into_iter()
        .find_map(|(_, a)| match a {
            Action::Ping { seq, .. } => Some(seq),
            _ => None,
        })
        .expect("a ping");
    assert_eq!(w.pong("main", sent, at(t0, 71_000)), Pong::Recovered);
    assert!(
        w.close_question("main"),
        "the dialog is still up: it must be taken down"
    );
    assert!(!w.is_question_open("main"));
    assert_eq!(w.question_seq("main"), None);
    assert!(!w.close_question("main"), "taken down once");
    // The person's click raced the recovery: Reload now would reload a healthy page by surprise.
    assert!(
        !w.accept_answer("main", seq, Choice::Reload),
        "a stale answer is not carried out"
    );
}

#[test]
fn closing_the_question_on_recovery_keeps_the_doubling_of_the_next_wait() {
    let t0 = Instant::now();
    let mut w = Watchdog::default();
    hung_with_question(&mut w, t0);
    // Healthy for a short while (the question was closed by the recovery), then stopped again.
    run_with(&mut w, t0, 70_500, 80_000, |_| true);
    assert!(w.close_question("main"));
    let again = run_with(&mut w, t0, 80_500, 400_000, |_| false);
    let q = hung_questions(&again);
    assert_eq!(q.len(), 1, "{q:?}");
    assert!(
        q[0] - 80_500 >= 110_000,
        "the earlier question still counts: asked again after only {} ms",
        q[0] - 80_500
    );
}

#[test]
fn only_the_current_question_takes_an_answer_and_a_report_in_progress_is_not_closed_by_recovery() {
    let t0 = Instant::now();
    let mut w = Watchdog::default();
    let seq = hung_with_question(&mut w, t0);
    assert!(!w.accept_answer("main", seq + 1, Choice::Wait), "another question's answer");
    assert!(w.is_question_open("main"), "a stale answer changes nothing");
    assert!(w.accept_answer("main", seq, Choice::SaveReport));
    assert!(!w.accept_answer("main", seq, Choice::Wait), "answered once");
    // The question is answered (the report's own save dialog is up): there is no question dialog to take down.
    assert!(w.is_question_open("main"), "no second question while the report is written");
    assert_eq!(w.question_seq("main"), None);
    assert!(!w.close_question("main"));
    w.question_answered("main");
    assert!(!w.is_question_open("main"));
    // A plain answer closes the question.
    let mut v = Watchdog::default();
    let seq = hung_with_question(&mut v, t0);
    assert!(v.accept_answer("main", seq, Choice::Wait));
    assert!(!v.is_question_open("main"));
}

#[test]
fn each_question_has_its_own_identity() {
    let t0 = Instant::now();
    let mut w = Watchdog::default();
    let first = hung_with_question(&mut w, t0);
    assert!(w.accept_answer("main", first, Choice::Wait));
    w.keep_waiting("main", at(t0, 70_500));
    let acts = run_with(&mut w, t0, 70_500, 400_000, |_| false);
    assert_eq!(hung_questions(&acts).len(), 1, "{acts:?}");
    let second = w.question_seq("main").unwrap();
    assert_ne!(first, second);
    assert!(!w.accept_answer("main", first, Choice::Reload));
}
