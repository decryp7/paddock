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
    o.extra_args.extend(chrome_log_args(
        std::env::var("PADDOCK_TEST_BROWSER_LOG").ok().as_deref(),
        base,
    ));
    o
}

/// Chrome's own verbose log for CI (`PADDOCK_TEST_BROWSER_LOG`, a folder the job keeps as an artifact): why a child
/// process such as the network service went away is only in there (ADR 0030). One file per browser, named after its
/// throwaway base folder (no leading dot, so the artifact keeps it). Nothing when the variable is unset or empty.
fn chrome_log_args(dir: Option<&str>, base: &std::path::Path) -> Vec<String> {
    let Some(dir) = dir.filter(|d| !d.is_empty()) else {
        return Vec::new();
    };
    let dir = std::path::Path::new(dir);
    let _ = std::fs::create_dir_all(dir);
    let name = base
        .file_name()
        .map_or_else(|| "chrome".into(), |n| n.to_string_lossy().into_owned());
    vec![
        "--enable-logging".into(),
        "--v=1".into(),
        format!(
            "--log-file={}",
            dir.join(format!("chrome-{}.log", name.trim_start_matches('.')))
                .display()
        ),
    ]
}

#[test]
fn chrome_logs_only_when_ci_asks_for_them() {
    let base = std::path::Path::new("/t/.tmpAbc");
    assert!(chrome_log_args(None, base).is_empty());
    assert!(chrome_log_args(Some(""), base).is_empty());
    let dir = tempfile::tempdir().unwrap();
    let args = chrome_log_args(dir.path().to_str(), base);
    assert_eq!(&args[..2], ["--enable-logging", "--v=1"]);
    assert_eq!(
        args[2],
        format!(
            "--log-file={}",
            dir.path().join("chrome-tmpAbc.log").display()
        )
    );
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
    let exited = close_by_itself(&mut b);
    assert!(
        exited.0,
        "Browser.close ends it by itself (waited {:?})",
        exited.1
    );
}

/// Chrome cancels every load its network service was carrying when that process crashes or restarts, with
/// `net::ERR_ABORTED` and no other navigation and no download (the trace of Windows CI runs 18449, 19025, 19043 and 19880;
/// the same signature here when the process is ended). The error names that cause, so a Windows failure says whether
/// it was this one. The order is fixed: the first request is held at the server until the network service has been
/// ended, so the load is always in flight when the service goes.
#[test]
fn a_load_the_network_service_lost_names_the_restart() {
    let Some(exe) = require_browser() else { return };
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (arrived, first_request) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let held = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        for s in listener.incoming().map_while(Result::ok) {
            let (arrived, held) = (arrived.clone(), held.clone());
            std::thread::spawn(move || {
                if let Some(s) = answer(s, |_| None) {
                    held.lock().unwrap_or_else(|e| e.into_inner()).push(s);
                    let _ = arrived.send(());
                }
            });
        }
    });
    let base = tempfile::tempdir().unwrap();
    let mut b = Chromium::launch(&exe, &options(base.path())).expect("Chrome for Testing starts");
    let browser = b.pid().expect("the browser's process");
    let page = b.new_page().unwrap();
    let url = format!("http://127.0.0.1:{port}/lost");
    let loaded = std::thread::scope(|s| {
        let load = s.spawn(|| page.navigate(&url, Duration::from_secs(30)));
        first_request
            .recv_timeout(Duration::from_secs(30))
            .expect("the first request reaches the server");
        let pid = page
            .network_service_pid()
            .expect("the process list")
            .expect("the browser names its network service");
        // Only a process of the browser this test launched is ended (rule 7: nothing else on the machine), by identity (rule 53).
        let service = paddock_proc::ProcessId::of(pid).expect("the network service is running");
        let browser_id = paddock_proc::ProcessId::of(browser).expect("the browser is running");
        assert!(
            paddock_proc::descendants_of(browser_id).contains(&service),
            "the network service {pid} is a process of the test's browser {browser}"
        );
        paddock_proc::end(service);
        load.join().unwrap()
    });
    let e = loaded.expect_err("the load was lost").to_string();
    assert!(e.contains("network service restarted"), "{e}");
    let exited = close_by_itself(&mut b);
    assert!(
        exited.0,
        "Browser.close ends it by itself (waited {:?})",
        exited.1
    );
}

/// A document the server answers with no content (204) is cancelled by Chrome with `net::ERR_ABORTED` too, but it was
/// answered: the error never blames the network service.
#[test]
fn an_answered_abort_never_blames_the_network_service() {
    let Some(exe) = require_browser() else { return };
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in listener.incoming().map_while(Result::ok) {
            std::thread::spawn(move || {
                let mut s = s;
                let mut head = Vec::new();
                let mut buf = [0u8; 4096];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < 16 << 10 {
                    match std::io::Read::read(&mut s, &mut buf) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => head.extend_from_slice(&buf[..n]),
                    }
                }
                let _ = std::io::Write::write_all(
                    &mut s,
                    b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n",
                );
                let _ = s.shutdown(std::net::Shutdown::Write);
                let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
                while matches!(std::io::Read::read(&mut s, &mut buf), Ok(n) if n > 0) {}
            });
        }
    });
    let base = tempfile::tempdir().unwrap();
    let mut b = Chromium::launch(&exe, &options(base.path())).expect("Chrome for Testing starts");
    let page = b.new_page().unwrap();
    let e = page
        .navigate(
            &format!("http://127.0.0.1:{port}/empty"),
            Duration::from_secs(30),
        )
        .expect_err("a 204 is not a page")
        .to_string();
    assert!(
        e.contains("net::ERR_ABORTED") && !e.contains("network service"),
        "{e}"
    );
    let exited = close_by_itself(&mut b);
    assert!(
        exited.0,
        "Browser.close ends it by itself (waited {:?})",
        exited.1
    );
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
    let exited = close_by_itself(&mut b);
    assert!(
        exited.0,
        "Browser.close ends it by itself (waited {:?})",
        exited.1
    );
    assert_nothing_left(pid, &dir);
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
    // What the server saw, for the failure message: each connection and the first line of its request. Chrome opens
    // sockets that send nothing (preconnects, backup connects), so every connection has a thread of its own and one idle
    // socket can never keep the real request from being accepted.
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen_by_server = seen.clone();
    // Holds `/held` (the connection stays open until the test ends) and answers everything else with a page.
    std::thread::spawn(move || {
        let held = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        for (n, s) in listener.incoming().map_while(Result::ok).enumerate() {
            let (arrived, seen, held) = (arrived.clone(), seen_by_server.clone(), held.clone());
            seen.lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(format!("connection {n} opened"));
            std::thread::spawn(move || {
                let mut s = s;
                // A socket that never sends gives up after a minute instead of holding its thread for the whole run.
                let _ = s.set_read_timeout(Some(Duration::from_secs(60)));
                let mut request = Vec::new();
                let mut buf = [0u8; 4096];
                while !request.contains(&b'\n') && request.len() < 16 * 1024 {
                    match std::io::Read::read(&mut s, &mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => request.extend_from_slice(&buf[..n]),
                    }
                }
                let line = String::from_utf8_lossy(&request)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                seen.lock().unwrap_or_else(|e| e.into_inner()).push(format!(
                    "connection {n}: {}",
                    if line.is_empty() {
                        "nothing sent"
                    } else {
                        &line
                    }
                ));
                if line.starts_with("GET /held") {
                    let _ = arrived.send(());
                    held.lock().unwrap_or_else(|e| e.into_inner()).push(s);
                } else if !line.is_empty() {
                    let _ = std::io::Write::write_all(
                        &mut s,
                        b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 13\r\nConnection: close\r\n\r\n<p>other</p>\n",
                    );
                }
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
        if first_request.recv_timeout(Duration::from_secs(30)).is_err() {
            // Windows runs 6696 and 6697 never saw the request. Say what the load itself reported (its own wait ends after
            // 30 s), so the next failure names the cause instead of only the symptom.
            panic!(
                "the first load did not reach the server in 30 s; the load itself ended with {:?}; the server saw {:?}",
                first.join(),
                seen.lock().unwrap_or_else(|e| e.into_inner())
            );
        }
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
    let exited = close_by_itself(&mut b);
    assert!(
        exited.0,
        "Browser.close ends it by itself (waited {:?})",
        exited.1
    );
}

/// How long a loaded CI runner may take to let Chrome exit after `Browser.close` is a property of the machine, not of the
/// driver (macOS 6662 and 6726 and Windows 6726 took more than `CLOSE_GRACE`, 5 s, with the test otherwise fine). The test
/// asserts the mechanism, that the browser exits by itself and is not killed, and waits for that exit on the condition; the
/// bound only keeps a browser that never exits from hanging the test (rule 40). Returns whether it exited and the wait.
fn close_by_itself(b: &mut Chromium) -> (bool, Duration) {
    // Over three times the slowest exit measured on a loaded Mac (11.7 s, docs/benchmarks.md) and under CI's 60 s test
    // kill (nextest slow-timeout 30 s × 2), so a browser that never exits fails here, with the message below.
    const HANG_GUARD: Duration = Duration::from_secs(40);
    let dir = b.profile_dir().to_string_lossy().into_owned();
    let started = std::time::Instant::now();
    let exited = b.close_gracefully(HANG_GUARD);
    let waited = started.elapsed();
    // Always printed (nextest shows it for a failed test): how long a runner needs is the evidence for the next change;
    // whether the browser answered Browser.close, and for one that did not exit, which of its processes were still there.
    let left = if exited {
        Vec::new()
    } else {
        paddock_proc::processes_mentioning(&dir)
    };
    eprintln!(
        "Browser.close: answered = {:?}, exited by itself = {exited}, waited {waited:?}, left before the forced end: {left:?}",
        b.close_answered()
    );
    (exited, waited)
}

/// The browser, its profile and its helpers are gone. Helpers end a moment after the browser, so they get a bounded wait
/// on the condition, not an immediate check (rule 40).
fn assert_nothing_left(pid: u32, dir: &std::path::Path) {
    assert!(!dir.exists(), "the profile folder is removed");
    assert!(!paddock_proc::is_alive(pid), "nothing is left behind");
    let end = std::time::Instant::now() + Duration::from_secs(20);
    while !paddock_proc::processes_mentioning(&dir.to_string_lossy()).is_empty() {
        // conformance: hang-guard — a helper that never ends fails the test instead of hanging it
        assert!(std::time::Instant::now() < end, "no helper is left behind");
        std::thread::yield_now();
    }
}

/// The other end of `close_by_itself`: with no grace at all the browser cannot exit by itself first, so it is ended by
/// force, and the profile, the process and its helpers (on macOS the code-sign clone too) must be gone all the same.
/// Together the two cover both outcomes of a close with no timing in either.
#[test]
fn a_browser_ended_by_force_leaves_nothing_behind() {
    let Some(exe) = find_for_tests() else {
        let required = std::env::var("PADDOCK_REQUIRE_TEST_BROWSER").is_ok_and(|v| !v.is_empty());
        assert!(
            !required,
            "PADDOCK_REQUIRE_TEST_BROWSER is set but CHROMIUM_TEST_PATH names no Chrome for Testing"
        );
        return;
    };
    let base = tempfile::tempdir().unwrap();
    let mut b = Chromium::launch(&exe, &options(base.path())).expect("Chrome for Testing starts");
    let pid = b.pid().unwrap();
    b.start_page().unwrap();
    let dir = b.profile_dir().to_path_buf();
    assert!(
        !b.close_gracefully(Duration::ZERO),
        "with no grace the browser is ended by force"
    );
    assert_nothing_left(pid, &dir);
}
