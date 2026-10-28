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
    o.extra_args = paddock_chromium::discover::TEST_ISOLATION_FLAGS
        .iter()
        .map(|f| f.to_string())
        .collect();
    o
}

/// Answers one connection like a well-behaved server: reads the whole request head, answers with `answer(path)`, stops
/// writing and reads until the browser closes its side (closing a socket that still holds unread bytes makes Windows send
/// a reset instead of an orderly close, which can throw away an answer the browser has not read yet). `None` holds the
/// connection open instead (returned so the caller keeps it).
fn answer(
    mut s: std::net::TcpStream,
    reply: impl Fn(&str) -> Option<String>,
) -> Option<std::net::TcpStream> {
    use std::io::{Read, Write};
    let mut head = Vec::new();
    let mut buf = [0u8; 4096];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < 16 << 10 {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => return None,
            Ok(n) => head.extend_from_slice(&buf[..n]),
        }
    }
    let req = String::from_utf8_lossy(&head).to_string();
    let path = req.split_whitespace().nth(1).unwrap_or("/");
    let Some(body) = reply(path) else {
        return Some(s);
    };
    let _ = write!(
        s,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = s.shutdown(std::net::Shutdown::Write);
    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
    while matches!(s.read(&mut buf), Ok(n) if n > 0) {}
    None
}

fn require_browser() -> Option<paddock_chromium::discover::Browser> {
    let exe = find_for_tests();
    if exe.is_none() {
        let required = std::env::var("PADDOCK_REQUIRE_TEST_BROWSER").is_ok_and(|v| !v.is_empty());
        assert!(
            !required,
            "PADDOCK_REQUIRE_TEST_BROWSER is set but CHROMIUM_TEST_PATH names no Chrome for Testing"
        );
    }
    exe
}

/// A new tab is loaded at once, with no wait of the caller's (the tab's own first `about:blank` navigation may still
/// run; the page waits for it instead of having the load cancelled, Spotter's Windows CI runs 6473 and 6563).
#[test]
fn a_new_tab_loads_a_page_at_once() {
    let Some(exe) = require_browser() else { return };
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in listener.incoming().flatten() {
            std::thread::spawn(move || {
                answer(s, |_| Some("<title>Tab</title><p id=p>new tab</p>".into()))
            });
        }
    });
    let base = tempfile::tempdir().unwrap();
    let mut b = Chromium::launch(&exe, &options(base.path())).expect("Chrome for Testing starts");
    for _ in 0..3 {
        let page = b.new_page().unwrap();
        page.navigate(
            &format!("http://127.0.0.1:{port}/"),
            Duration::from_secs(30),
        )
        .unwrap();
        assert_eq!(
            page.info().unwrap().url,
            format!("http://127.0.0.1:{port}/")
        );
    }
    assert!(b.close(), "Browser.close ends it by itself");
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
    // Helpers end a moment after the browser; a bounded wait, not an immediate check (rule 40).
    let end = std::time::Instant::now() + Duration::from_secs(20);
    while !paddock_proc::processes_mentioning(&dir.to_string_lossy()).is_empty() {
        // conformance: hang-guard — a helper that never ends fails the test instead of hanging it
        assert!(std::time::Instant::now() < end, "no helper is left behind");
        std::thread::yield_now();
    }
}

/// Chrome answers `Page.navigate` with `net::ERR_ABORTED` for every load it cancels before the page commits; the driver
/// says which kind it was. A download is one; a load replaced by another navigation is the other, made on purpose here: the
/// first request is held at a local server until the second navigation starts, so the order is fixed.
#[test]
fn a_cancelled_load_says_whether_it_was_a_download_or_replaced() {
    let Some(exe) = require_browser() else { return };
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (arrived, first_request) = std::sync::mpsc::channel();
    // Holds `/held` (the connection stays open until the test ends) and answers everything else with a page.
    std::thread::spawn(move || {
        let held = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        for s in listener.incoming().flatten() {
            let (arrived, held) = (arrived.clone(), held.clone());
            std::thread::spawn(move || {
                let kept = answer(s, |p| {
                    if p == "/held" {
                        let _ = arrived.send(());
                        None
                    } else {
                        Some("<p>other</p>\n".into())
                    }
                });
                held.lock().unwrap_or_else(|e| e.into_inner()).extend(kept);
            });
        }
    });
    let base = tempfile::tempdir().unwrap();
    let mut b = Chromium::launch(&exe, &options(base.path())).expect("Chrome for Testing starts");
    let page = b.start_page().unwrap();
    let e = page
        .navigate(
            "data:application/zip;base64,UEsFBg==",
            Duration::from_secs(30),
        )
        .unwrap_err()
        .to_string();
    assert!(e.contains("file download"), "{e}");
    let e = std::thread::scope(|s| {
        let first = s.spawn(|| {
            page.navigate(
                &format!("http://127.0.0.1:{port}/held"),
                Duration::from_secs(30),
            )
        });
        first_request
            .recv_timeout(Duration::from_secs(30))
            .expect("the first load reached the server");
        page.navigate(
            &format!("http://127.0.0.1:{port}/other"),
            Duration::from_secs(30),
        )
        .unwrap();
        first.join().unwrap().unwrap_err().to_string()
    });
    assert!(
        e.contains("replaced") && e.contains(&format!("http://127.0.0.1:{port}/other")),
        "{e}"
    );
    assert!(b.close(), "Browser.close ends it by itself");
}
