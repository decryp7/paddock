use super::*;
use crate::fake::{FakeBrowser, Mode, UserAction};
use crate::page::{MAX_HTML, Outcome};
use crate::ready::{Readiness, Rule};
use paddock_jobs::time::FakeClock;

const FAKE: &str = "launch::tests::fake_browser_process";

/// Not a test: the fake browser process re-runs this binary on this function (see [`crate::fake`]).
#[test]
fn fake_browser_process() {
    crate::fake::serve_if_requested();
}

/// How long a test waits for a closed browser to exit: a hang guard only (the exit is waited for on the condition). A
/// loaded runner can take longer than the product's `CLOSE_GRACE` (5 s) to exit after it answered `Browser.close`
/// (up to 11.7 s with 20 test browsers on the Mac, docs/benchmarks.md), so a test asserting the grace failed on load
/// alone. 40 s: over three times that, and under CI's 60 s test kill (nextest `slow-timeout` 30 s × 2), so a real hang
/// fails with its own message instead of being killed first.
const CLOSE_GUARD: Duration = Duration::from_secs(40);

/// Waits (without sleeping) until `f` holds; a hang guard only.
fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while !f() {
        // conformance: hang-guard — a broken fake must fail the test, not hang it
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::yield_now();
    }
}

fn opts(base: &Path) -> LaunchOptions {
    let mut o = LaunchOptions::new(Profile::Throwaway(base.to_path_buf()));
    o.window = Window::Headless;
    o
}

/// Ten years: limits a test runs out on purpose only (the fake time a launch pumps through never reaches them).
const FAR: Duration = Duration::from_secs(10 * 365 * 24 * 3600);

/// A CPU reading that grows at every look: a browser that is working, whatever the fake time says.
fn busy() -> CpuSampler {
    let used = std::sync::atomic::AtomicU64::new(0);
    Arc::new(move |_| Some(Duration::from_millis(used.fetch_add(1, Ordering::Relaxed))))
}

/// Options whose limits and waits run on `clock`.
fn opts_on(f: &Fixture, clock: &FakeClock) -> LaunchOptions {
    let mut o = opts(&f.base());
    (o.clock, o.max_lifetime, o.cpu_sampler) = (Arc::new(clock.clone()), FAR, Some(busy()));
    o
}

/// Launches `exe` with `o` (whose clock is `clock`) and moves the fake time on whenever the launch waits.
fn launch_on_options(exe: &Browser, clock: &FakeClock, o: LaunchOptions) -> Result<Chromium> {
    std::thread::scope(|s| {
        let h = s.spawn(|| Chromium::launch_with(exe, &o, Sandbox::On));
        while !h.is_finished() {
            if clock.waiters() >= 2 {
                clock.advance(POLL);
            }
            std::thread::yield_now();
        }
        h.join().unwrap()
    })
}

/// Launches with a fake clock: the start-up wait polls on it, so time is moved on whenever the watchdog and the start-up
/// wait both wait (the launch then reads the port file again). The fake time this takes has nothing to do with how long
/// the real fake browser needs, so `opts_on` puts the limits out of reach.
fn launch_on(f: &Fixture, clock: &FakeClock, o: LaunchOptions) -> Result<Chromium> {
    std::thread::scope(|s| {
        let h = s.spawn(|| Chromium::launch_with(&f.exe(), &o, Sandbox::On));
        while !h.is_finished() {
            if clock.waiters() >= 2 {
                clock.advance(POLL);
            }
            std::thread::yield_now();
        }
        h.join().unwrap()
    })
}

struct Fixture {
    dir: tempfile::TempDir,
    fake: FakeBrowser,
}

impl Fixture {
    fn new(mode: Mode, tabs: usize) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let fake = FakeBrowser::install(dir.path(), FAKE, mode, tabs).unwrap();
        Fixture { dir, fake }
    }

    fn exe(&self) -> Browser {
        Browser::chromium(&self.fake.exe)
    }

    fn base(&self) -> PathBuf {
        self.dir.path().join("profiles")
    }

    fn launch(&self) -> Chromium {
        Chromium::launch_with(&self.exe(), &opts(&self.base()), Sandbox::On)
            .expect("the fake starts")
    }
}

// ---- launching (a fake browser; never a real one) ----

#[test]
fn the_launch_command_line_is_the_one_built_and_the_profile_goes_with_the_browser() {
    let f = Fixture::new(Mode::Obey, 1);
    let mut b = f.launch();
    let args = f.fake.args();
    assert!(args.iter().any(|a| a.starts_with("--user-data-dir=")));
    assert_eq!(args.last().map(String::as_str), Some(START_PAGE));
    assert!(!args.iter().any(|a| a == "--no-sandbox"));
    let (dir, pid) = (b.profile_dir().to_path_buf(), b.pid().unwrap());
    assert!(dir.exists() && b.is_running());
    assert_eq!(b.exe(), &f.exe());
    assert_eq!(b.ended_by(), EndedBy::Nothing);
    // conformance: hang-guard — the exit is waited for on the condition; a loaded runner needs more than the grace
    assert!(b.close_gracefully(CLOSE_GUARD), "it exited by itself");
    assert!(!dir.exists(), "the throwaway profile is gone");
    assert!(!paddock_proc::is_alive(pid));
    assert!(f.fake.calls().contains(&"Browser.close".to_string()));
    // conformance: grace-free — the first close already ended the browser, so this one waits out no grace
    assert!(b.close(), "a second close finds it ended");
}

/// `helper_starts` reads the browser's histograms; a browser that answers without them, or does not know the method,
/// says nothing (`None`) instead of failing.
#[test]
fn helper_starts_come_from_the_browser_or_say_nothing() {
    let f = Fixture::new(Mode::Obey, 1);
    let mut b = f.launch();
    assert!(
        f.fake.args().contains(&NO_TESTING_FIELD_TRIALS.to_string()),
        "a Chromium build starts without its testing config"
    );
    assert_eq!(
        b.helper_starts().unwrap(),
        None,
        "a reply without histograms"
    );
    f.fake.histograms(None).unwrap();
    assert_eq!(
        b.helper_starts().unwrap(),
        None,
        "a browser without the method"
    );
    f.fake
        .histograms(Some(&json!({"histograms": [
            {"name": "ChildProcess.Launched.UtilityProcessHash", "count": 2},
            {"name": "ChildProcess.LaunchFailed.UtilityProcessErrorCode", "count": 1}
        ]})))
        .unwrap();
    assert_eq!(
        b.helper_starts().unwrap(),
        Some(HelperStarts {
            started: 2,
            failed: 1,
            ..HelperStarts::default()
        })
    );
    // conformance: hang-guard — the exit is waited for on the condition; a loaded runner needs more than the grace
    assert!(b.close_gracefully(CLOSE_GUARD));
    assert!(
        matches!(b.helper_starts(), Err(Error::Closed)),
        "a closed browser is an error"
    );
}

/// Regression (Debrief 1.1.1, Linux as root): `--no-sandbox` was appended after the start page.
#[test]
fn without_the_sandbox_the_start_page_is_still_the_last_argument() {
    let f = Fixture::new(Mode::Obey, 1);
    let mut o = opts(&f.base());
    o.disk_cache_bytes = Some(1234);
    let mut b = Chromium::launch_with(&f.exe(), &o, Sandbox::Off).unwrap();
    let args = f.fake.args();
    assert!(
        args.contains(&"--no-sandbox".to_string())
            && args.contains(&"--disk-cache-size=1234".to_string()),
        "{args:?}"
    );
    assert_eq!(
        args.last().map(String::as_str),
        Some(START_PAGE),
        "{args:?}"
    );
    b.kill();
}

#[test]
fn a_kept_profile_is_used_with_its_theme_and_survives_the_close() {
    let f = Fixture::new(Mode::Obey, 1);
    let kept = f.dir.path().join("kept");
    let mut o = opts(&f.base());
    (o.profile, o.accent) = (Profile::Persistent(kept.clone()), Some(0x336699));
    let mut b = Chromium::launch(&f.exe(), &o).unwrap();
    assert!(
        f.fake
            .args()
            .contains(&format!("--user-data-dir={}", kept.display()))
    );
    assert!(
        kept.join("Default/Preferences").is_file(),
        "the frame colour is written"
    );
    // A kept profile gets the long grace: ending Chrome by force in its shutdown can lose cookies or a sign-in.
    assert_eq!(b.close_grace(), CLOSE_GRACE_KEPT);
    // conformance: hang-guard — the exit is waited for on the condition; a loaded runner needs more than the grace
    assert!(b.close_gracefully(CLOSE_GUARD));
    assert!(kept.is_dir(), "a kept profile is not removed");
}

#[test]
fn a_throwaway_profile_keeps_the_short_grace_and_a_kept_one_gets_the_long_one() {
    assert_eq!(
        close_grace_for(&Profile::Throwaway("/t".into())),
        CLOSE_GRACE
    );
    assert_eq!(
        close_grace_for(&Profile::Persistent("/k".into())),
        CLOSE_GRACE_KEPT
    );
    // Above the slowest exit measured on a loaded Mac (11.7 s, docs/benchmarks.md).
    assert!(CLOSE_GRACE_KEPT > Duration::from_millis(11_700));
    let f = Fixture::new(Mode::Obey, 1);
    let mut b = f.launch();
    assert_eq!(b.close_grace(), CLOSE_GRACE);
    // conformance: hang-guard — the exit is waited for on the condition; a loaded runner needs more than the grace
    assert!(b.close_gracefully(CLOSE_GUARD));
}

#[test]
fn a_browser_that_ignores_close_is_ended_after_the_grace() {
    let f = Fixture::new(Mode::IgnoreClose, 1);
    let mut b = f.launch();
    let dir = b.profile_dir().to_path_buf();
    assert_eq!(b.close_answered(), None, "no close sent yet");
    assert!(
        !b.close_gracefully(Duration::from_millis(200)),
        "it had to be ended"
    );
    // It took the request and did not exit: answered, so the slow part was the exit, not the request.
    assert_eq!(b.close_answered(), Some(true));
    assert!(!b.is_running());
    assert!(!dir.exists());
}

#[test]
fn the_start_page_is_driven_so_the_window_has_one_tab() {
    let f = Fixture::new(Mode::Obey, 1);
    let b = f.launch();
    let p = b.start_page().unwrap();
    assert_eq!(b.page_count().unwrap(), 1);
    assert!(!f.fake.calls().contains(&"Target.createTarget".to_string()));
    p.navigate("https://example.test/", Duration::from_secs(10))
        .unwrap();
    assert_eq!(p.info().unwrap().url, "https://example.test/");
    let second = b.new_page().unwrap();
    assert_eq!(b.page_count().unwrap(), 2);
    assert!(second.exists().unwrap());
}

#[test]
fn a_browser_without_a_start_page_gets_a_new_one_after_the_wait_on_its_clock() {
    let f = Fixture::new(Mode::Obey, 0);
    let clock = FakeClock::new();
    let b = launch_on(&f, &clock, opts_on(&f, &clock)).unwrap();
    std::thread::scope(|s| {
        let h = s.spawn(|| b.start_page());
        // The watchdog and the start-page wait both wait on the clock.
        assert!(clock.wait_for_waiters(2, Duration::from_secs(20)));
        assert!(
            !f.fake.calls().contains(&"Target.createTarget".to_string()),
            "no new page before the wait is over"
        );
        clock.advance(START_PAGE_WAIT);
        let p = h.join().unwrap().unwrap();
        assert!(!p.is_closed());
    });
    assert!(f.fake.calls().contains(&"Target.createTarget".to_string()));
}

#[test]
fn a_start_that_does_nothing_is_stuck_on_the_clock_not_after_a_fixed_wait() {
    let d = tempfile::tempdir().unwrap();
    let silent = script(d.path(), "silent.sh", "exec sleep 30");
    let clock = FakeClock::new();
    let mut o = opts(&d.path().join("p"));
    // No CPU at all: idle for the patience's 2 s of ticks, counted on the fake clock.
    o.cpu_sampler = Some(Arc::new(|_| Some(Duration::from_millis(5))));
    o.patience = Patience {
        idle: Duration::from_secs(2),
        cpu_every: 1,
        ..o.patience
    };
    (o.clock, o.max_lifetime) = (Arc::new(clock.clone()), FAR);
    let e = launch_on_options(&silent, &clock, o).err().unwrap();
    assert!(
        matches!(&e, Error::Launch(m) if m.contains("the browser")),
        "{e}"
    );
}

#[test]
fn a_busy_start_is_waited_for_and_a_spinning_one_is_given_up_on() {
    let d = tempfile::tempdir().unwrap();
    let silent = script(d.path(), "silent.sh", "exec sleep 30");
    let clock = FakeClock::new();
    let mut o = opts(&d.path().join("p"));
    // CPU time grows: not idle; at 3 CPU seconds without a port it is spinning.
    let used = std::sync::atomic::AtomicU64::new(0);
    o.cpu_sampler = Some(Arc::new(move |_| {
        Some(Duration::from_millis(used.fetch_add(50, Ordering::Relaxed)))
    }));
    o.patience = Patience {
        spin: Duration::from_secs(3),
        cpu_every: 1,
        ..o.patience
    };
    (o.clock, o.max_lifetime) = (Arc::new(clock.clone()), FAR);
    let e = launch_on_options(&silent, &clock, o).err().unwrap();
    assert!(
        matches!(&e, Error::Launch(m) if m.contains("the browser")),
        "{e}"
    );
}

#[test]
fn a_start_that_cannot_be_observed_is_bounded_by_the_hang_guard() {
    let d = tempfile::tempdir().unwrap();
    let silent = script(d.path(), "silent.sh", "exec sleep 30");
    let clock = FakeClock::new();
    let mut o = opts(&d.path().join("p"));
    // The OS gives no CPU time (no identity, no reading): the judge could never conclude, so the guard ends it.
    o.cpu_sampler = Some(Arc::new(|_| None));
    o.patience = Patience {
        hang_guard: Duration::from_secs(2),
        cpu_every: 1,
        ..o.patience
    };
    (o.clock, o.max_lifetime) = (Arc::new(clock.clone()), FAR);
    let e = launch_on_options(&silent, &clock, o).err().unwrap();
    assert!(
        matches!(&e, Error::Launch(m) if m.contains("the browser")),
        "{e}"
    );
}

#[test]
fn a_browser_stopped_at_start_is_paused_and_waited_for_not_called_idle() {
    let d = tempfile::tempdir().unwrap();
    let pid_file = d.path().join("pid");
    let stopped = script(
        d.path(),
        "stopped.sh",
        &format!("echo $$ > {}\nkill -STOP $$\nsleep 30", pid_file.display()),
    );
    let clock = FakeClock::new();
    let mut o = opts(&d.path().join("p"));
    // A constant CPU time: only the pause keeps the idle count from ending the start.
    (o.clock, o.max_lifetime, o.cpu_sampler) = (
        Arc::new(clock.clone()),
        Duration::from_secs(30),
        Some(Arc::new(|_| Some(Duration::from_millis(5))) as CpuSampler),
    );
    o.patience = Patience {
        idle: Duration::from_secs(2),
        cpu_every: 1,
        ..o.patience
    };
    let e = std::thread::scope(|s| {
        let h = s.spawn(|| Chromium::launch_with(&stopped, &o, Sandbox::On));
        assert!(clock.wait_for_waiters(2, Duration::from_secs(20)));
        // Time does not move until the browser is really stopped (the pid file is written just before).
        wait_until("the browser to stop", || {
            std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|t| t.trim().parse::<u32>().ok())
                .and_then(paddock_proc::ProcessId::of)
                .is_some_and(|id| paddock_proc::run_state(id) == paddock_proc::RunState::Stopped)
        });
        while !h.is_finished() {
            if clock.waiters() >= 1 {
                clock.advance(POLL);
            }
            std::thread::yield_now();
        }
        h.join().unwrap().err().unwrap()
    });
    // Only the caller's cap ended it; the idle stall (which would say "has done nothing") never fired.
    assert!(
        matches!(&e, Error::Launch(m) if m.contains("did not start within 30 s")),
        "{e}"
    );
}

#[test]
fn a_start_ended_by_the_callers_cap_says_it_was_the_time_limit() {
    let d = tempfile::tempdir().unwrap();
    let silent = script(d.path(), "silent.sh", "exec sleep 300");
    let clock = FakeClock::new();
    let mut o = opts(&d.path().join("p"));
    (o.clock, o.max_lifetime, o.cpu_sampler) = (
        Arc::new(clock.clone()),
        Duration::from_secs(2),
        Some(busy()),
    );
    let e = std::thread::scope(|s| {
        let h = s.spawn(|| Chromium::launch_with(&silent, &o, Sandbox::On));
        // Both waits are registered (the watchdog's 2 s and the start loop's tick) before time moves. From then on the
        // clock is moved on whenever anything waits: once the cap has fired the watchdog is gone and only the start loop
        // waits, and it must still be given the time to look again and find the browser ended (rule 49).
        assert!(
            clock.wait_for_waiters(2, Duration::from_secs(20)),
            "the watchdog and the start-up wait"
        );
        while !h.is_finished() {
            if clock.waiters() >= 1 {
                clock.advance(POLL);
            }
            std::thread::yield_now();
        }
        h.join().unwrap().err().unwrap()
    });
    assert!(
        matches!(&e, Error::Launch(m) if m.contains("did not start within 2 s")),
        "{e}"
    );
    assert!(!e.to_string().contains("exited"), "{e}");
    assert!(
        paddock_proc::processes_mentioning(&d.path().join("silent.sh").to_string_lossy())
            .is_empty(),
        "the browser was ended"
    );
}

#[test]
fn waiting_for_a_page_polls_and_times_out_on_the_clock() {
    let f = Fixture::new(Mode::Obey, 1);
    let clock = FakeClock::new();
    let b = launch_on(&f, &clock, opts_on(&f, &clock)).unwrap();
    let p = b.start_page().unwrap();
    let rule = Rule::default();
    f.fake.page_shows(&json!({"state": "loading"})).unwrap();
    std::thread::scope(|s| {
        let h = s.spawn(|| p.wait_ready(None, &rule, Duration::from_secs(10)));
        assert!(
            clock.wait_for_waiters(2, Duration::from_secs(20)),
            "a poll waits on the clock"
        );
        f.fake
            .page_shows(&json!({"state": "complete", "quiet": 900.0}))
            .unwrap();
        clock.advance(Duration::from_millis(100));
        assert_eq!(
            h.join().unwrap().unwrap(),
            Outcome::Ready,
            "ready on the next poll"
        );
    });
    f.fake.page_shows(&json!({"state": "loading"})).unwrap();
    std::thread::scope(|s| {
        let h = s.spawn(|| p.wait_ready(None, &rule, Duration::from_secs(10)));
        assert!(clock.wait_for_waiters(2, Duration::from_secs(20)));
        clock.advance(Duration::from_secs(10));
        // The poll after the deadline sees the time is up.
        while !h.is_finished() {
            if clock.waiters() >= 2 {
                clock.advance(Duration::from_millis(100));
            }
            std::thread::yield_now();
        }
        assert_eq!(
            h.join().unwrap().unwrap(),
            Outcome::TimedOut(Readiness::Loading)
        );
    });
}

#[test]
fn the_user_closing_the_window_ends_the_page_while_the_process_runs() {
    let f = Fixture::new(Mode::Obey, 1);
    let mut b = f.launch();
    let p = b.start_page().unwrap();
    assert_eq!(b.gone(&p), None);
    f.fake.act(UserAction::CloseWindow).unwrap();
    // The fake reports the close on its next message: any call triggers it.
    let _ = b.page_count();
    wait_until("the page to be gone", || p.is_closed());
    assert_eq!(b.gone(&p), Some(Gone::PageClosed));
    assert!(matches!(p.evaluate("1"), Err(Error::Closed)));
    assert!(matches!(
        p.navigate("https://x.test/", Duration::from_secs(5)),
        Err(Error::Closed)
    ));
    assert!(matches!(p.info(), Err(Error::Closed)));
    assert!(!p.exists().unwrap());
}

/// Regression (Spotter, Windows CI runs 6473 and 6563): a `Page.navigate` sent right after the tab was created, while its
/// own first navigation (`about:blank`) still ran, came back `net::ERR_ABORTED`. The fake keeps that navigation running
/// until told otherwise and cancels any load sent meanwhile, as Chrome can; the first load now waits for it instead.
#[test]
fn the_first_load_waits_until_the_tabs_own_first_navigation_has_settled() {
    let f = Fixture::new(Mode::Obey, 1);
    let b = f.launch();
    f.fake.act(UserAction::SlowStartPage).unwrap();
    let p = b.new_page().unwrap();
    // While it runs, the load is not even sent (the driver before the fix sent it and got ERR_ABORTED back at once).
    let e = p.navigate("https://a.test/", Duration::from_millis(200));
    assert!(matches!(e, Err(Error::Timeout(_))), "{e:?}");
    assert!(!f.fake.calls().iter().any(|c| c == "Page.navigate"));
    std::thread::scope(|s| {
        let load = s.spawn(|| p.navigate("https://a.test/", Duration::from_secs(20)));
        f.fake.act(UserAction::FinishStartPage).unwrap();
        load.join().unwrap().unwrap();
    });
    assert!(f.fake.calls().iter().any(|c| c == "Page.navigate"));
    // Settled once is settled: later loads do not wait for it again.
    p.navigate("https://b.test/", Duration::from_secs(10))
        .unwrap();
}

/// Regression (review P2-16): a network service that restarted while the page sat idle, between two loads, is not
/// blamed on a later abort: the process is looked up right before each load, so the evidence for "restarted" is always
/// about the load that was cancelled.
#[test]
fn a_restart_between_loads_is_not_blamed_on_the_next_abort() {
    let f = Fixture::new(Mode::Obey, 1);
    let b = f.launch();
    let p = b.new_page().unwrap();
    p.navigate("https://a.test/", Duration::from_secs(10))
        .unwrap();
    f.fake.act(UserAction::RestartNetworkService).unwrap();
    f.fake.act(UserAction::AbortLoads).unwrap();
    let e = p
        .navigate("https://b.test/", Duration::from_secs(10))
        .unwrap_err()
        .to_string();
    assert!(e.contains("network service unchanged (process 101)"), "{e}");
}

/// The events that would show the first navigation settled can leave the bounded event log (a busy page sends more than
/// it keeps); the page then counts it as settled instead of waiting out the caller's whole timeout for an event it can no
/// longer see.
#[test]
fn a_first_navigation_whose_events_left_the_log_counts_as_settled() {
    let f = Fixture::new(Mode::Obey, 1);
    let b = f.launch();
    f.fake.act(UserAction::SlowStartPage).unwrap();
    let p = b.new_page().unwrap();
    p.call("Debug.flood", json!({})).unwrap();
    // The load is sent (the fake, still on its first navigation, cancels it), not held until the 20 s run out.
    let e = p.navigate("https://a.test/", Duration::from_secs(20));
    assert!(
        matches!(&e, Err(Error::Protocol(m)) if m.contains("net::ERR_ABORTED")),
        "{e:?}"
    );
    assert!(f.fake.calls().iter().any(|c| c == "Page.navigate"));
}

/// Without a frame tree the main frame is the target id (as in Chrome): the running navigation is still found and
/// waited for. (With a frame tree the fake's main frame `F` differs from every target id, so the other tests show the
/// frame tree's id is the one used.)
#[test]
fn without_a_frame_tree_the_target_id_names_the_main_frame() {
    let f = Fixture::new(Mode::Obey, 1);
    let b = f.launch();
    f.fake.act(UserAction::SlowStartPage).unwrap();
    f.fake.act(UserAction::NoFrameTree).unwrap();
    let p = b.new_page().unwrap();
    let e = p.navigate("https://a.test/", Duration::from_millis(200));
    assert!(matches!(e, Err(Error::Timeout(_))), "{e:?}");
    assert!(!f.fake.calls().iter().any(|c| c == "Page.navigate"));
    f.fake.act(UserAction::FinishStartPage).unwrap();
    p.navigate("https://a.test/", Duration::from_secs(20))
        .unwrap();
}

#[test]
fn closing_the_window_ends_the_wait_for_the_tabs_first_navigation() {
    let f = Fixture::new(Mode::Obey, 1);
    let b = f.launch();
    f.fake.act(UserAction::SlowStartPage).unwrap();
    let p = b.new_page().unwrap();
    std::thread::scope(|s| {
        let load = s.spawn(|| p.navigate("https://a.test/", Duration::from_secs(20)));
        f.fake.act(UserAction::CloseWindow).unwrap();
        let _ = b.page_count();
        assert!(matches!(load.join().unwrap(), Err(Error::Closed)));
    });
}

#[test]
fn a_load_in_progress_ends_at_once_when_the_window_is_closed() {
    let f = Fixture::new(Mode::Obey, 1);
    let b = f.launch();
    let p = b.start_page().unwrap();
    f.fake.act(UserAction::StallLoads).unwrap();
    let since = p.conn.mark();
    p.call("Page.navigate", json!({"url": "https://slow.test/"}))
        .unwrap();
    f.fake.act(UserAction::CloseWindow).unwrap();
    let _ = b.page_count();
    assert!(matches!(
        p.wait_load(since, Duration::from_secs(20)),
        Err(Error::Closed)
    ));
}

#[test]
fn a_page_that_never_loads_times_out_and_back_is_not_an_error() {
    let f = Fixture::new(Mode::Obey, 1);
    let b = f.launch();
    let p = b.start_page().unwrap();
    p.back(Duration::from_secs(10)).unwrap();
    f.fake.act(UserAction::StallLoads).unwrap();
    let _ = b.page_count();
    assert!(matches!(
        p.navigate("https://slow.test/", Duration::ZERO),
        Err(Error::Timeout(_))
    ));
    p.back(Duration::ZERO).unwrap();
}

#[test]
fn quitting_and_a_lost_connection_are_told_apart() {
    let f = Fixture::new(Mode::Obey, 1);
    let mut b = f.launch();
    let p = b.start_page().unwrap();
    f.fake.act(UserAction::DropConnection).unwrap();
    let _ = b.page_count();
    wait_until("the connection to drop", || b.conn.is_closed());
    assert_eq!(b.gone(&p), Some(Gone::Disconnected));
    assert!(matches!(b.page_count(), Err(Error::Closed)));
    f.fake.act(UserAction::Quit).unwrap();
    wait_until("the process to exit", || !b.proc().is_running());
    assert_eq!(b.gone(&p), Some(Gone::Exited));
    // conformance: grace-free — the process exited before this close, so it waits out no grace
    assert!(b.close(), "it had ended already");
}

#[test]
fn the_time_limit_runs_on_the_injected_clock() {
    let f = Fixture::new(Mode::IgnoreClose, 1);
    let clock = FakeClock::new();
    let mut b = launch_on(&f, &clock, opts_on(&f, &clock)).unwrap();
    assert!(
        clock.wait_for_waiters(1, Duration::from_secs(20)),
        "the watchdog waits on the clock"
    );
    assert_eq!(b.ended_by(), EndedBy::Nothing);
    clock.advance(FAR);
    wait_until("the browser to be ended", || !b.proc().is_running());
    assert_eq!(b.ended_by(), EndedBy::TimeLimit);
    b.kill();
}

#[test]
fn the_memory_cap_ends_the_tree() {
    let f = Fixture::new(Mode::IgnoreClose, 1);
    let grow = Arc::new(AtomicBool::new(false));
    let g = grow.clone();
    let mut o = opts(&f.base());
    o.memory_cap = Some(1 << 20);
    o.memory_sampler = Some(Arc::new(
        move |_| if g.load(Ordering::SeqCst) { 1 << 30 } else { 0 },
    ));
    let mut b = Chromium::launch_with(&f.exe(), &o, Sandbox::On).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(b.ended_by(), EndedBy::Nothing);
    grow.store(true, Ordering::SeqCst);
    wait_until("the cap to end the browser", || {
        b.ended_by() == EndedBy::MemoryCap
    });
    wait_until("the process to end", || !b.proc().is_running());
    b.kill();
}

#[test]
fn a_browser_over_the_cap_while_it_starts_fails_to_launch_and_says_why() {
    // A browser that never gets as far as its port file, so the cap (not the start) decides, whatever the machine's speed.
    let d = tempfile::tempdir().unwrap();
    let starting = script(d.path(), "starting.sh", "exec sleep 30");
    let mut o = opts(&d.path().join("p"));
    o.memory_cap = Some(1);
    o.memory_sampler = Some(Arc::new(|_| u64::MAX));
    let e = Chromium::launch_with(&starting, &o, Sandbox::On)
        .err()
        .unwrap()
        .to_string();
    assert!(e.contains("more memory than allowed"), "{e}");
}

#[test]
fn a_quick_browser_over_the_cap_is_caught_when_it_connects() {
    // The fake may connect before the watcher's first look; the launch looks once more then, so the outcome is the same.
    let f = Fixture::new(Mode::IgnoreClose, 1);
    let mut o = opts(&f.base());
    o.memory_cap = Some(1);
    o.memory_sampler = Some(Arc::new(|_| u64::MAX));
    let e = Chromium::launch_with(&f.exe(), &o, Sandbox::On)
        .err()
        .unwrap()
        .to_string();
    assert!(e.contains("more memory than allowed"), "{e}");
}

#[test]
fn a_port_that_refuses_is_tried_again_until_the_start_is_judged_stuck() {
    let d = tempfile::tempdir().unwrap();
    // A port file that names a closed port (1), then the browser waits.
    let refusing = script(
        d.path(),
        "refusing.sh",
        "for a in \"$@\"; do case $a in --user-data-dir=*) p=${a#--user-data-dir=};; esac; done\nprintf '1\\n/devtools/browser/x\\n' > \"$p/DevToolsActivePort\"\nexec sleep 30",
    );
    let clock = FakeClock::new();
    let mut o = opts(&d.path().join("p"));
    (o.clock, o.max_lifetime) = (Arc::new(clock.clone()), Duration::from_secs(3600));
    // No CPU: idle after 2 s of ticks, which ends the retries of the refused port.
    o.cpu_sampler = Some(Arc::new(|_| Some(Duration::from_millis(5))));
    o.patience = Patience {
        idle: Duration::from_secs(2),
        cpu_every: 1,
        ..o.patience
    };
    std::thread::scope(|s| {
        let h = s.spawn(|| Chromium::launch_with(&refusing, &o, Sandbox::On));
        let written = || {
            std::fs::read_dir(d.path().join("p"))
                .map(|rd| {
                    rd.flatten().any(|e| {
                        std::fs::read_to_string(e.path().join("DevToolsActivePort"))
                            .is_ok_and(|t| t.ends_with("/devtools/browser/x\n"))
                    })
                })
                .unwrap_or(false)
        };
        wait_until("the whole port file", written);
        // Polls after the file is there: each is refused and tried again while time is left.
        for _ in 0..3 {
            assert!(
                clock.wait_for_waiters(2, Duration::from_secs(20)),
                "the watchdog and the start-up wait"
            );
            clock.advance(POLL);
        }
        while !h.is_finished() {
            if clock.waiters() >= 2 {
                clock.advance(POLL);
            }
            std::thread::yield_now();
        }
        let e = h.join().unwrap().err().unwrap();
        assert!(matches!(e, Error::Connection(_)), "{e}");
    });
}

#[test]
fn a_launch_without_a_memory_cap_uses_a_plain_child() {
    let f = Fixture::new(Mode::Obey, 1);
    let mut o = opts(&f.base());
    o.memory_cap = None;
    let mut b = Chromium::launch_with(&f.exe(), &o, Sandbox::On).unwrap();
    // conformance: hang-guard — the exit is waited for on the condition; a loaded runner needs more than the grace
    assert!(b.close_gracefully(CLOSE_GUARD));
}

#[test]
fn a_failed_start_reports_the_browsers_error_output_and_leaves_no_profile() {
    use std::os::unix::fs::PermissionsExt;
    let base = tempfile::tempdir().unwrap();
    let exe = base.path().join("fake-chrome.sh");
    std::fs::write(
        &exe,
        "#!/bin/sh\n# paddock-fake-browser\necho 'Running as root without --no-sandbox is not supported' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let profiles = base.path().join("profiles");
    let e = Chromium::launch(&Browser::chromium(&exe), &opts(&profiles))
        .err()
        .unwrap()
        .to_string();
    assert!(
        e.contains("exited right after it started") && e.contains("without --no-sandbox"),
        "{e}"
    );
    assert_eq!(std::fs::read_dir(&profiles).unwrap().count(), 0);
    let quiet = base.path().join("quiet.sh");
    std::fs::write(&quiet, "#!/bin/sh\n# paddock-fake-browser\nexit 0\n").unwrap();
    std::fs::set_permissions(&quiet, std::fs::Permissions::from_mode(0o755)).unwrap();
    let e = Chromium::launch(&Browser::chromium(&quiet), &opts(&profiles))
        .err()
        .unwrap()
        .to_string();
    assert!(e.ends_with("exited right after it started"), "{e}");
}

#[test]
fn missing_executable_or_profile_folder_is_a_launch_error() {
    let base = tempfile::tempdir().unwrap();
    let e = Chromium::launch(
        &Browser::chromium("/nonexistent/chrome"),
        &opts(base.path()),
    )
    .err()
    .unwrap();
    assert!(matches!(e, Error::Launch(_)));
    assert_eq!(std::fs::read_dir(base.path()).unwrap().count(), 0);
    let file = base.path().join("file");
    std::fs::write(&file, b"x").unwrap();
    let e = Chromium::launch(&Browser::chromium("/nonexistent/chrome"), &opts(&file))
        .err()
        .unwrap();
    assert!(e.to_string().contains("profile folder"), "{e}");
}

// ---- pages ----

#[test]
fn a_page_is_read_snapshot_by_snapshot() {
    let f = Fixture::new(Mode::Obey, 1);
    let b = f.launch();
    let p = b.start_page().unwrap();
    p.track_dom_changes().unwrap();
    p.block_heavy(true).unwrap();
    p.block_heavy(false).unwrap();
    assert_eq!(p.screenshot_png().unwrap(), "iVBORw0KGgo=");
    assert!(p.load_finished().unwrap());
    assert_eq!(p.evaluate("x").unwrap(), Value::Null);
    f.fake.page_shows(&json!("plain text")).unwrap();
    assert_eq!(p.text_excerpt().unwrap(), "plain text");
    f.fake
        .page_shows(&json!({"state": "complete", "present": true, "quiet": 900.0, "pw": false, "url": "https://a.test/",
            "title": "A", "text": "content", "html": "<html></html>"}))
        .unwrap();
    let s = p.snapshot(Some("#main")).unwrap();
    assert_eq!(
        (
            s.url.as_str(),
            s.title.as_str(),
            s.quiet_ms,
            s.element_present
        ),
        ("https://a.test/", "A", 900, Some(true))
    );
    assert!(s.load_finished && !s.has_password_field);
    assert_eq!(
        p.wait_ready(Some("#main"), &Rule::default(), Duration::from_secs(10))
            .unwrap(),
        Outcome::Ready
    );
    assert_eq!(p.challenge().unwrap(), None);
    assert_eq!(
        p.capture().unwrap(),
        ("<html></html>".to_string(), "https://a.test/".to_string())
    );
    f.fake
        .page_shows(&json!({"state": "complete", "quiet": 10.0, "text": "Verify you are human"}))
        .unwrap();
    assert_eq!(
        p.wait_ready(None, &Rule::default(), Duration::from_secs(10))
            .unwrap(),
        Outcome::Challenge(crate::Challenge::HumanCheck)
    );
    f.fake.page_shows(&json!({"state": "loading"})).unwrap();
    assert_eq!(
        p.wait_ready(None, &Rule::default(), Duration::ZERO)
            .unwrap(),
        Outcome::TimedOut(Readiness::Loading)
    );
    f.fake
        .page_shows(&json!({"exception": "ReferenceError: x"}))
        .unwrap();
    assert!(matches!(p.evaluate("x"), Err(Error::Protocol(m)) if m.contains("ReferenceError")));
    // The cap is proved with a few bytes: `MAX_HTML` itself is 16 MiB, whose transfer through the fake browser in a debug
    // build outlasted the call timeout on a loaded CI runner (macOS run 6726).
    f.fake.page_shows(&json!({"html": "x".repeat(9)})).unwrap();
    assert!(
        p.capture_within(8)
            .unwrap_err()
            .to_string()
            .contains("larger")
    );
    assert_eq!(p.capture_within(9).unwrap().0, "x".repeat(9));
}

#[test]
fn a_helper_that_left_the_process_group_is_ended_with_the_browser() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("chromium-helper-test");
    std::fs::create_dir_all(&dir).unwrap();
    // A "helper" outside the browser's tree whose command line names the profile (one process, no children).
    let log = dir.join("helper.log");
    std::fs::write(&log, b"").unwrap();
    let mut cmd = paddock_proc::command("tail");
    cmd.arg("-f").arg(&log);
    #[allow(clippy::zombie_processes)]
    let mut helper = cmd.spawn().unwrap();
    wait_until("the helper to show up", || {
        !paddock_proc::processes_mentioning(&dir.to_string_lossy()).is_empty()
    });
    assert_eq!(end_leftovers(&dir), 1);
    helper.wait().unwrap();
    assert_eq!(end_leftovers(&dir), 0, "nothing is left");
}

#[test]
fn a_profile_that_cannot_be_removed_is_given_up_without_failing() {
    let d = tempfile::tempdir().unwrap();
    assert!(!cleanup_with(d.path(), &|_| Err(std::io::Error::other(
        "busy"
    ))));
    assert!(
        cleanup_with(&d.path().join("gone"), &|_| Err(std::io::Error::other(
            "busy"
        ))),
        "nothing left to remove"
    );
    cleanup(&d.path().join("kept"), false);
}

#[test]
fn a_theme_that_cannot_be_written_does_not_stop_the_launch() {
    let f = Fixture::new(Mode::Obey, 1);
    let kept = f.dir.path().join("kept");
    std::fs::create_dir_all(&kept).unwrap();
    std::fs::write(kept.join("Default"), b"a file, not a folder").unwrap();
    let mut o = opts(&f.base());
    (o.profile, o.accent) = (Profile::Persistent(kept), Some(0x336699));
    let mut b = Chromium::launch_with(&f.exe(), &o, Sandbox::On).unwrap();
    // conformance: hang-guard — the exit is waited for on the condition; a loaded runner needs more than the grace
    assert!(b.close_gracefully(CLOSE_GUARD));
}

fn script(dir: &Path, name: &str, body: &str) -> Browser {
    use std::os::unix::fs::PermissionsExt;
    let exe = dir.join(name);
    std::fs::write(&exe, format!("#!/bin/sh\n# paddock-fake-browser\n{body}\n")).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    Browser::chromium(exe)
}

#[test]
fn a_garbled_port_file_or_a_silent_browser_fails_the_launch() {
    let d = tempfile::tempdir().unwrap();
    let garbled = script(
        d.path(),
        "garbled.sh",
        "for a in \"$@\"; do case $a in --user-data-dir=*) p=${a#--user-data-dir=};; esac; done\necho nonsense > \"$p/DevToolsActivePort\"",
    );
    let e = Chromium::launch_with(&garbled, &opts(&d.path().join("p")), Sandbox::On)
        .err()
        .unwrap();
    assert!(matches!(e, Error::Launch(_)), "{e}");
    let silent = script(d.path(), "silent.sh", "exec sleep 30");
    let clock = FakeClock::new();
    let mut o = opts(&d.path().join("p"));
    o.cpu_sampler = Some(Arc::new(|_| Some(Duration::ZERO)));
    o.patience = Patience {
        idle: Duration::ZERO,
        cpu_every: 1,
        ..o.patience
    };
    o.clock = Arc::new(clock.clone());
    let e = launch_on_options(&silent, &clock, o).err().unwrap();
    assert!(
        matches!(&e, Error::Launch(m) if m.contains("the browser")),
        "{e}"
    );
    assert!(
        paddock_proc::processes_mentioning(&d.path().join("silent.sh").to_string_lossy())
            .is_empty(),
        "the silent browser was ended"
    );
}

#[test]
fn the_connection_bounds_waits_and_keeps_only_recent_events() {
    let f = Fixture::new(Mode::Obey, 1);
    let mut b = f.launch();
    assert!(matches!(
        b.conn.call(None, "Debug.hang", json!({}), Duration::ZERO),
        Err(Error::Timeout("a command"))
    ));
    b.conn
        .call(None, "Debug.flood", json!({}), Duration::from_secs(20))
        .unwrap();
    assert!(b.conn.mark() >= 1100);
    assert!(
        b.conn
            .wait_event(0, Duration::ZERO, |e| e.method == "Debug.event")
            .is_some(),
        "the newest events are kept"
    );
    let p = b.start_page().unwrap();
    assert!(
        matches!(p.navigate("https://unreachable.test/", Duration::from_secs(10)), Err(Error::Protocol(m)) if m.contains("did not load"))
    );
    f.fake.act(UserAction::DropConnection).unwrap();
    assert_eq!(
        b.conn
            .wait_event(u64::MAX, Duration::from_secs(20), |_| false)
            .map(|e| e.method),
        None
    );
    assert!(
        b.conn.is_closed(),
        "the wait ended because the connection closed"
    );
    assert!(matches!(
        b.conn
            .call(None, "Target.getTargets", json!({}), Duration::from_secs(5)),
        Err(Error::Closed)
    ));
    b.kill();
}

#[test]
fn going_back_ends_at_once_when_the_window_is_closed_meanwhile() {
    let f = Fixture::new(Mode::Obey, 1);
    let b = f.launch();
    let p = b.start_page().unwrap();
    f.fake.act(UserAction::StallLoads).unwrap();
    let _ = b.page_count();
    std::thread::scope(|s| {
        s.spawn(|| {
            wait_until("the history step", || {
                f.fake
                    .calls()
                    .iter()
                    .any(|c| c == "Page.navigateToHistoryEntry")
            });
            f.fake.act(UserAction::CloseWindow).unwrap();
        });
        assert!(matches!(
            p.back(Duration::from_secs(20)),
            Err(Error::Closed)
        ));
    });
}

/// `capture` applies [`MAX_HTML`] (`capture_within` takes it as a parameter so tests need not move that much); the cap is
/// part of the contract with the apps, so a change to it is a deliberate one.
#[test]
fn the_page_size_cap_is_sixteen_mebibytes() {
    assert_eq!(MAX_HTML, 16 << 20);
}

/// Ends the stopped-and-continued child when dropped, also on a panic of the parent. It continues the child and tells it
/// `go`, so the child finishes its checks and exits and its own drops end the fake browser and remove its folder (the
/// fake runs in a process group of its own, so no signal to the child's group reaches it); only a child that does not
/// exit within a bound is killed (with its group) and reaped.
#[cfg(target_os = "linux")]
struct ChildGuard {
    child: std::process::Child,
    go: PathBuf,
}

#[cfg(target_os = "linux")]
impl Drop for ChildGuard {
    fn drop(&mut self) {
        // Signal only a child that is still unreaped: once it was waited for, its pid may belong to another process.
        if let Ok(None) = self.child.try_wait() {
            let pid = self.child.id() as i32;
            // SAFETY: a signal to the still-unreaped child of this guard.
            unsafe { libc::kill(pid, libc::SIGCONT) };
            let _ = std::fs::write(&self.go, "");
            let end = Instant::now() + Duration::from_secs(15);
            // conformance: hang-guard — polls another process's state, which has no event to wait on
            while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(5));
            }
            if let Ok(None) = self.child.try_wait() {
                // SAFETY: a signal to the process group this guard's still-unreaped child leads (set at spawn).
                unsafe { libc::killpg(pid, libc::SIGKILL) };
            }
        }
        let _ = self.child.wait();
    }
}

/// Regression: Linux ends a socket read that has a timeout with EINTR when the process is stopped and continued
/// (signal(7)), and the DevTools reader took that for the end of the connection, so every page said closed. The test
/// runs itself again as a child process (so no other test is stopped), stops and continues the child a few times while
/// it holds a connection to the fake browser, then lets it check the connection. They talk through files in a
/// temporary folder: `ready` (the child's connection is up) and `go` (the stops are over).
#[cfg(target_os = "linux")]
#[test]
fn a_stopped_and_continued_process_keeps_its_browser_connection() {
    const CHILD: &str = "PADDOCK_CHROMIUM_STOP_CHILD";
    // The wait is on the condition; the short sleep only keeps the poll from spinning (a hang guard bounds it).
    let wait_for = |what: &str, cond: &mut dyn FnMut() -> bool| {
        let end = Instant::now() + Duration::from_secs(20);
        while !cond() {
            // conformance: hang-guard — a stuck child must fail the test, not hang it
            assert!(Instant::now() < end, "timed out waiting for {what}");
            // conformance: hang-guard — polls another process's state, which has no event to wait on
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    if let Some(dir) = std::env::var_os(CHILD).map(PathBuf::from) {
        let f = Fixture::new(Mode::Obey, 1);
        let mut b = f.launch();
        let p = b.start_page().unwrap();
        std::fs::write(dir.join("ready"), "").unwrap();
        wait_for("the parent said go", &mut || dir.join("go").exists());
        assert_eq!(b.gone(&p), None, "the connection outlived the stops");
        assert!(p.exists().unwrap());
        return;
    }
    use std::os::unix::process::CommandExt;
    let dir = tempfile::tempdir().unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "launch::tests::a_stopped_and_continued_process_keeps_its_browser_connection",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, dir.path())
        .stdin(std::process::Stdio::null())
        .process_group(0)
        .spawn()
        .unwrap();
    let pid = child.id();
    let mut child = ChildGuard {
        child,
        go: dir.path().join("go"),
    };
    wait_for("the child to be ready", &mut || {
        assert!(
            child.child.try_wait().unwrap().is_none(),
            "the child ended before it was ready"
        );
        dir.path().join("ready").exists()
    });
    // The child's state in /proc: `T` once the stop took effect.
    let stopped = || {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|s| s.rsplit_once(") ").map(|(_, rest)| rest.starts_with('T')))
            .unwrap_or(false)
    };
    // SAFETY: a signal to the child this test spawned and has not waited for (it is reaped only below).
    let kill = |sig: i32| assert_eq!(unsafe { libc::kill(pid as i32, sig) }, 0);
    for _ in 0..5 {
        kill(libc::SIGSTOP);
        wait_for("the child to stop", &mut || stopped());
        kill(libc::SIGCONT);
        wait_for("the child to run again", &mut || !stopped());
    }
    std::fs::write(dir.path().join("go"), "").unwrap();
    assert!(
        child.child.wait().unwrap().success(),
        "the child's checks failed (its output is above)"
    );
}
