//! The driver against a real browser: only the pinned Chrome for Testing named by `CHROMIUM_TEST_PATH` (rule 7), headless
//! (no window on the desktop), closed with `Browser.close` so no code-sign clone or process is left behind. Without the
//! variable the tests pass without doing anything (the fake browser covers the driver everywhere else), unless
//! `PADDOCK_REQUIRE_TEST_BROWSER` is set (Paddock's Linux CI job, whose image has the pinned build): then they fail.

use paddock_chromium::discover::find_for_tests;
use paddock_chromium::{Chromium, LaunchOptions, Outcome, Profile, Window};
use std::time::Duration;

fn options(base: &std::path::Path) -> LaunchOptions {
    let mut o = LaunchOptions::new(Profile::Throwaway(base.to_path_buf()));
    o.window = Window::Headless;
    o
}

#[test]
fn a_real_browser_reads_a_page_and_closes_cleanly() {
    let Some(exe) = find_for_tests() else {
        let required = std::env::var("PADDOCK_REQUIRE_TEST_BROWSER").is_ok_and(|v| !v.is_empty());
        assert!(
            !required,
            "PADDOCK_REQUIRE_TEST_BROWSER is set but CHROMIUM_TEST_PATH names no Chrome for Testing"
        );
        eprintln!("CHROMIUM_TEST_PATH is not set: skipped");
        return;
    };
    // The pinned runtime's self-test against the same build: it names its version.
    use paddock_runtime::RuntimeKind;
    let kind =
        paddock_chromium::runtime::ChromeForTesting::new(paddock_chromium::discover::this_os());
    let pinned = paddock_chromium::runtime::catalogue().unwrap().version;
    match kind.self_test(&exe.path, &pinned) {
        Ok(line) => assert!(line.contains(&pinned), "{line}"),
        Err(why) => assert!(why.contains("another version"), "{why}"),
    }
    let base = tempfile::tempdir().unwrap();
    let mut b = Chromium::launch(&exe, &options(base.path())).expect("Chrome for Testing starts");
    let pid = b.pid().unwrap();
    let page = b.start_page().unwrap();
    page.track_dom_changes().unwrap();
    page.navigate(
        "data:text/html,<title>Hello</title><main id=m>Readable text</main>",
        Duration::from_secs(30),
    )
    .unwrap();
    let rule = paddock_chromium::ready::Rule {
        quiet: Duration::from_millis(100),
        ..Default::default()
    };
    assert_eq!(
        page.wait_ready(Some("#m"), &rule, Duration::from_secs(30))
            .unwrap(),
        Outcome::Ready
    );
    let snap = page.snapshot(Some("#m")).unwrap();
    assert_eq!(snap.title, "Hello");
    assert!(snap.text.contains("Readable text"));
    assert_eq!(
        page.evaluate("navigator.webdriver").unwrap(),
        serde_json::json!(true),
        "no stealth"
    );
    assert!(!page.screenshot_png().unwrap().is_empty());
    assert_eq!(b.page_count().unwrap(), 1, "the start page is the only tab");
    let dir = b.profile_dir().to_path_buf();
    assert!(b.close(), "Browser.close ends it by itself");
    assert!(!dir.exists());
    assert!(!paddock_proc::is_alive(pid), "nothing is left behind");
    assert!(
        paddock_proc::processes_mentioning(&dir.to_string_lossy()).is_empty(),
        "no helper is left behind"
    );
}
