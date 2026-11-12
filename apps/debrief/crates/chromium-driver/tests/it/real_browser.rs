//! Launches a real Chromium-based browser when one is installed (skipped otherwise) against a local server that
//! serves a page rendered by script.
//!
//! The waits are ADR 0071's: every answer and page load while the browser is there ([`Answers::Judged`] from the kit), the
//! page's content until it appears or the browser ends, and the browser's end until it is gone; never a fixed count or
//! deadline. On macOS the first run of the browser is paid once per process before the first launch (`warm_first_start`).

use chromium_driver::{find_for_tests as find, Answers, Chromium, LaunchOptions, Profile};
use paddock_testkit::start::{wait_exit, wait_ready_or_exit};
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

/// The pinned test browser, or `None` (the test then skips). With `DEBRIEF_REQUIRE_TEST_BROWSER` set (CI) a missing
/// browser fails the test instead: a silent skip would hide a broken browser install (Paddock rule T02).
fn browser_or_skip() -> Option<chromium_driver::BrowserExe> {
    let found = find();
    if found.is_none() && std::env::var_os("DEBRIEF_REQUIRE_TEST_BROWSER").is_some_and(|v| !v.is_empty() && v != "0") {
        panic!("DEBRIEF_REQUIRE_TEST_BROWSER is set but no pinned test browser was found: install the pin with conformance/ci/install-test-browser.mjs --rust-source app/src-tauri/src/browser/runtime.rs and set CHROMIUM_TEST_PATH");
    }
    // The OS holds the first run of a browser bundle on macOS with no CPU and no output (ADR 0071): paid here, once per
    // process and file, before the launch's own judge looks. Not on Windows, where `--version` starts the browser.
    #[cfg(target_os = "macos")]
    if let Some(b) = &found {
        paddock_testkit::start::warm_first_start(&b.path, &["--version"]);
    }
    found
}

/// Launch options for a test: throwaway profiles below `base`, and the test form of the answers (ADR 0071).
fn test_options(base: &std::path::Path) -> LaunchOptions {
    LaunchOptions {
        profile: Profile::Throwaway(base.join("profiles")),
        answers: Answers::Judged {
            patience: paddock_testkit::start::PATIENCE,
            guard: paddock_testkit::HANG_GUARD,
        },
        ..LaunchOptions::default()
    }
}

/// The browser by identity, for the kit's waits.
fn identity(b: &Chromium) -> paddock_proc::ProcessId {
    paddock_proc::ProcessId::of(b.pid().expect("the browser's pid")).expect("the browser's identity")
}

const PAGE: &str = "<!doctype html><html><head><title>Rendered</title></head><body><ul id=r></ul>\
<script>setTimeout(()=>{for(let i=1;i<=3;i++){const l=document.createElement('li');l.textContent='item '+i;r.appendChild(l);}},150)</script></body></html>";

fn serve() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let mut s = s;
            let mut b = [0u8; 2048];
            let _ = s.read(&mut b);
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{PAGE}",
                PAGE.len()
            );
        }
    });
    port
}

#[test]
fn renders_a_scripted_page_and_leaves_nothing_behind() {
    let Some(exe) = browser_or_skip() else {
        eprintln!("skipped: set CHROMIUM_TEST_PATH to a pinned test Chromium (installed browsers are never launched by tests)");
        return;
    };
    let port = serve();
    let base = tempfile::tempdir().unwrap();
    let opts = LaunchOptions {
        accent: Some(0x2a7de1),
        ..test_options(base.path())
    };
    let mut b = Chromium::launch(&exe, &opts).expect("launch");
    let dir = b.profile_dir().to_path_buf();
    let browser = identity(&b);
    // The theme was written into the profile before launch.
    let prefs = std::fs::read_to_string(dir.join("Default/Preferences")).unwrap();
    assert!(prefs.contains("user_color2"));

    let page = b.start_page().expect("page");
    assert_eq!(b.page_count().unwrap(), 1, "the start page is driven; no second tab");
    page.block_heavy(true).unwrap();
    // The load is waited for while the browser is there (the test form ignores the product's timeout given here).
    page.navigate(&format!("http://127.0.0.1:{port}/"), Duration::from_secs(15))
        .expect("navigate");
    // The page's script fills the list after 150 ms: waited for until it has, or the browser ended.
    let n = wait_ready_or_exit(browser, "the browser rendering the scripted list", || {
        page.evaluate("document.querySelectorAll('#r li').length")
            .ok()
            .and_then(|v| v.as_u64())
            .filter(|n| *n == 3)
    })
    .expect_ready("the browser");
    assert_eq!(n, 3, "script-rendered content appears");
    assert_eq!(page.info().unwrap().title, "Rendered");
    assert!(page.load_finished().unwrap());
    // The real automation signals stay on (no stealth) ...
    assert_eq!(page.evaluate("navigator.webdriver").unwrap(), json!(true));
    // ... and nothing of ours is in the page: no injected nodes, attributes or script, only what the server sent.
    assert_eq!(
        page.evaluate(
            "document.documentElement.outerHTML.includes('debrief') || document.documentElement.outerHTML.includes('AI is browsing')"
        )
        .unwrap(),
        json!(false)
    );
    assert_eq!(page.evaluate("document.querySelectorAll('script').length").unwrap(), json!(1));
    assert_eq!(page.text_excerpt().unwrap().trim(), "item 1\nitem 2\nitem 3");
    assert!(page.challenge().unwrap().is_none());
    // The browser closes the way a user would close it, and its throwaway profile goes with it.

    // Stop ends the process; the throwaway profile is removed on drop.
    b.kill();
    assert!(!b.is_running());
    wait_exit(browser, "the stopped browser");
    drop(b);
    assert!(!dir.exists(), "throwaway profile left behind");
}

#[test]
fn dropping_the_browser_ends_the_process() {
    let Some(exe) = browser_or_skip() else { return };
    let base = tempfile::tempdir().unwrap();
    let b = Chromium::launch(&exe, &test_options(base.path())).expect("launch");
    let browser = identity(&b);
    drop(b);
    // The process ends: waited for until it is gone (a leaked browser fails at the hang guard, naming it).
    wait_exit(browser, "the dropped browser");
}
