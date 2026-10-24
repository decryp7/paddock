//! The watchdog's decisions with a fake clock (ADR 0058's tests, ported from Debrief, plus the liveness heartbeat of plan
//! §11).

use super::*;
use std::time::Duration;

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
    let mut w = Watchdog::default();
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
    let mut w = Watchdog::default();
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
    let mut w = Watchdog::default();
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
    let mut w = Watchdog::default();
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
    let mut w = Watchdog::default();
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
    let mut w = Watchdog::default();
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
    let mut w = Watchdog::default();
    w.page_started("main", t0);
    assert!(run(&mut w, t0, 0, 9_000).is_empty());
    assert_eq!(w.ready("main", at(t0, 9_000)), None);
    assert!(run_answering(&mut w, t0, 9_500, 30_000).is_empty());
}

#[test]
fn a_terminated_content_process_is_reported_to_the_reloaded_page() {
    let t0 = Instant::now();
    let mut w = Watchdog::default();
    ready_at(&mut w, "main", t0);
    assert!(w.terminated("main", at(t0, 5_000)));
    assert!(!w.is_ready("main"));
    assert!(run(&mut w, t0, 5_000, 14_500).is_empty());
    assert_eq!(w.ready("main", at(t0, 14_500)), Some(Recovery::Terminated));
}

#[test]
fn terminated_reloads_are_rate_limited_per_window() {
    let t0 = Instant::now();
    let mut w = Watchdog::default();
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
    let mut w = Watchdog::default();
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
    let mut w = Watchdog::default();
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
    let mut w = Watchdog::default();
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
    let mut w = Watchdog::default();
    ready_at(&mut w, "main", t0);
    // Answers the first two pings, then hangs.
    for (t, a) in run_all(&mut w, t0, 500, 4_000) {
        if let Action::Ping { label, seq } = a {
            assert_eq!(w.pong(&label, seq, at(t0, t)), Pong::Alive);
        }
    }
    let actions = run(&mut w, t0, 4_500, 30_000);
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
    let mut w = Watchdog::default();
    ready_at(&mut w, "main", t0);
    assert_eq!(
        run(&mut w, t0, 500, 20_000),
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
    let mut w = Watchdog::default();
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
    let mut w = Watchdog::default();
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
    let mut w = Watchdog::default();
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
    let mut w = Watchdog::default();
    ready_at(&mut w, "main", t0);
    assert!(run(&mut w, t0, 500, 7_000).is_empty());
    assert!(
        w.due(at(t0, 40_000)).is_empty(),
        "the outstanding pings are forgotten on resume"
    );
    assert_eq!(
        run(&mut w, t0, 40_500, 60_000),
        vec![(52_000, Action::Unresponsive("main".into()))]
    );
}

#[test]
fn hidden_windows_are_not_pinged_and_start_again_when_shown() {
    let t0 = Instant::now();
    let mut w = Watchdog::default();
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
    let mut w = Watchdog::default();
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
    let mut w = Watchdog::default();
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
    let mut w = Watchdog::default();
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
