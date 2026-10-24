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
    assert!(
        b.close_gracefully(Duration::from_secs(10)),
        "it exited by itself"
    );
    assert!(!dir.exists(), "the throwaway profile is gone");
    assert!(!paddock_proc::is_alive(pid));
    assert!(f.fake.calls().contains(&"Browser.close".to_string()));
    assert!(b.close(), "a second close finds it ended");
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
    assert!(b.close());
    assert!(kept.is_dir(), "a kept profile is not removed");
}

#[test]
fn a_browser_that_ignores_close_is_ended_after_the_grace() {
    let f = Fixture::new(Mode::IgnoreClose, 1);
    let mut b = f.launch();
    let dir = b.profile_dir().to_path_buf();
    assert!(
        !b.close_gracefully(Duration::from_millis(200)),
        "it had to be ended"
    );
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
fn a_browser_without_a_start_page_gets_a_new_one() {
    let f = Fixture::new(Mode::Obey, 0);
    let b = f.launch();
    let p = b.start_page().unwrap();
    assert!(!p.is_closed());
    assert!(f.fake.calls().contains(&"Target.createTarget".to_string()));
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
    wait_until("the process to exit", || !b.process.is_running());
    assert_eq!(b.gone(&p), Some(Gone::Exited));
    assert!(b.close(), "it had ended already");
}

#[test]
fn the_time_limit_runs_on_the_injected_clock() {
    let f = Fixture::new(Mode::IgnoreClose, 1);
    let clock = FakeClock::new();
    let mut o = opts(&f.base());
    (o.clock, o.max_lifetime) = (Arc::new(clock.clone()), Duration::from_secs(60));
    let mut b = Chromium::launch_with(&f.exe(), &o, Sandbox::On).unwrap();
    assert!(
        clock.wait_for_waiters(1, Duration::from_secs(20)),
        "the watchdog waits on the clock"
    );
    assert_eq!(b.ended_by(), EndedBy::Nothing);
    clock.advance(Duration::from_secs(60));
    wait_until("the browser to be ended", || !b.process.is_running());
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
    wait_until("the process to end", || !b.process.is_running());
    b.kill();
}

#[test]
fn a_browser_over_the_cap_while_it_starts_fails_to_launch_and_says_why() {
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
fn a_launch_without_a_memory_cap_uses_a_plain_child() {
    let f = Fixture::new(Mode::Obey, 1);
    let mut o = opts(&f.base());
    o.memory_cap = None;
    let mut b = Chromium::launch_with(&f.exe(), &o, Sandbox::On).unwrap();
    assert!(b.close());
}

#[test]
fn a_failed_start_reports_the_browsers_error_output_and_leaves_no_profile() {
    use std::os::unix::fs::PermissionsExt;
    let base = tempfile::tempdir().unwrap();
    let exe = base.path().join("fake-chrome.sh");
    std::fs::write(
        &exe,
        "#!/bin/sh\necho 'Running as root without --no-sandbox is not supported' >&2\nexit 1\n",
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
    std::fs::write(&quiet, "#!/bin/sh\nexit 0\n").unwrap();
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
    f.fake
        .page_shows(&json!({"html": "x".repeat(MAX_HTML + 1)}))
        .unwrap();
    assert!(p.capture().unwrap_err().to_string().contains("larger"));
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
    assert!(b.close());
}

fn script(dir: &Path, name: &str, body: &str) -> Browser {
    use std::os::unix::fs::PermissionsExt;
    let exe = dir.join(name);
    std::fs::write(&exe, format!("#!/bin/sh\n{body}\n")).unwrap();
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
    let mut o = opts(&d.path().join("p"));
    o.start_limit = Duration::ZERO;
    let e = Chromium::launch_with(&silent, &o, Sandbox::On)
        .err()
        .unwrap();
    assert!(matches!(e, Error::Timeout("the browser start")), "{e}");
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
