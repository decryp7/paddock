//! A fake DevTools browser for tests: never a real browser. [`FakeBrowser::install`] writes an executable script that
//! records its command line and re-runs the *test binary* with a marker environment variable; a test function of the
//! caller then calls [`serve_if_requested`], which becomes the fake (a loopback WebSocket server that answers the few
//! commands the driver sends). Unix only: the script is `sh`.
//!
//! The fake exits on `Browser.close` unless its [`Mode`] says to ignore it, so both the graceful close and the
//! kill-after-timeout paths can be tested. A test plays the user with [`FakeBrowser::act`]: closing the window (the
//! page target goes away but the process stays, as Chrome does on macOS), quitting the browser, or losing the
//! DevTools connection.

use serde_json::{json, Value};
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use tungstenite::Message;

const ARGS_ENV: &str = "FAKE_BROWSER_ARGS_FILE";
/// Set in every fake browser process: an application's `main` refuses to start when it sees it, so a fake that names
/// the wrong binary can never open the app's windows on the desktop.
pub use crate::FAKE_CHILD_ENV as CHILD_ENV;
const MODE_ENV: &str = "FAKE_BROWSER_MODE";
const TABS_ENV: &str = "FAKE_BROWSER_TABS";
const CTL_ENV: &str = "FAKE_BROWSER_CTL";
const CALLS_ENV: &str = "FAKE_BROWSER_CALLS";
/// How often the fake looks at its control file and at its sockets.
const TICK: Duration = Duration::from_millis(10);

/// What the user does to the fake browser ([`FakeBrowser::act`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserAction {
    /// Closes the browser window: the page target is destroyed (its session detaches) and the process keeps running,
    /// as Chrome does on macOS when its last window is closed.
    CloseWindow,
    /// Quits the browser: the process exits.
    Quit,
    /// The DevTools connection drops while the process keeps running.
    DropConnection,
    /// Page loads stop finishing (a page that never fires its load event).
    StallLoads,
    /// A browser that is up but slow to answer (a loaded machine): the answer to `Target.getTargets` is held until
    /// [`UserAction::ReleaseAnswers`] (as in `paddock-chromium`'s fake).
    HoldPageList,
    /// As [`UserAction::HoldPageList`], for `Target.attachToTarget`.
    HoldAttach,
    /// As [`UserAction::HoldPageList`], for `Runtime.evaluate` (a page command after the start).
    HoldEvaluate,
    /// Held answers are sent, and nothing is held from now on.
    ReleaseAnswers,
}

impl UserAction {
    fn word(self) -> &'static str {
        match self {
            UserAction::CloseWindow => "close-window",
            UserAction::Quit => "quit",
            UserAction::DropConnection => "drop-connection",
            UserAction::StallLoads => "stall-loads",
            UserAction::HoldPageList => "hold-page-list",
            UserAction::HoldAttach => "hold-attach",
            UserAction::HoldEvaluate => "hold-evaluate",
            UserAction::ReleaseAnswers => "release-answers",
        }
    }
}

/// How the fake treats `Browser.close`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Answers and exits, like a browser that closes.
    Obey,
    /// Answers and keeps running, like a hung browser.
    IgnoreClose,
}

/// An installed fake: the executable to launch and the files it writes.
pub struct FakeBrowser {
    pub exe: PathBuf,
    /// The command line the fake was started with, one argument per line.
    pub args_file: PathBuf,
    /// The address of the page; the fake writes it on `Page.navigate` and a test may overwrite it (the user navigating).
    pub url_file: PathBuf,
    /// The user's actions, one word per line, read by the running fake.
    pub ctl_file: PathBuf,
    /// The DevTools methods the fake was sent, one per line (`Browser.close`, `Target.createTarget`, ...).
    pub calls_file: PathBuf,
}

/// The dynamic-loader variables a test binary may need (cargo and nextest put the native library folders in
/// `DYLD_FALLBACK_LIBRARY_PATH`). macOS strips every `DYLD_*` variable when it runs a system shell (SIP), so the
/// fake's script sets them again for the test binary it starts.
const DYLD_VARS: [&str; 3] = ["DYLD_LIBRARY_PATH", "DYLD_FALLBACK_LIBRARY_PATH", "DYLD_FRAMEWORK_PATH"];

/// The fake as a stand-in program: it writes its command line (one argument per line) to the args file, then runs the test binary
/// in its place, filtered to the one helper test (`--exact`), with the loader variables, the fake's own settings and
/// [`CHILD_ENV`] set. The loader variables are set explicitly because the code under test may start the fake with a cleaned
/// environment.
#[allow(clippy::too_many_arguments)]
fn stand_in(
    dir: &Path,
    dyld: &[(String, String)],
    args: &Path,
    url: &Path,
    ctl: &Path,
    calls: &Path,
    mode: &str,
    tabs: usize,
    me: &Path,
    test_path: &str,
) -> paddock_testkit::StandIn {
    use paddock_testkit::{template_literal, Actions, Exec, StandIn};
    let lit = |p: &Path| template_literal(&p.display().to_string());
    let mut start = Exec::program(&lit(me))
        .args([template_literal(test_path), "--exact".into(), "--nocapture".into()])
        .without_call_arguments();
    for (k, v) in dyld {
        start = start.env(k, &template_literal(v));
    }
    let settings = [
        (CHILD_ENV, "1".to_string()),
        (ARGS_ENV, lit(args)),
        (MODE_ENV, mode.to_string()),
        (TABS_ENV, tabs.to_string()),
        ("FAKE_BROWSER_URL", lit(url)),
        (CTL_ENV, lit(ctl)),
        (CALLS_ENV, lit(calls)),
    ];
    for (k, v) in &settings {
        start = start.env(k, v);
    }
    StandIn::new(dir, "fake-browser").answers(Actions::new().writes_with(&lit(args), "{args_lines}").execs(&start))
}

impl FakeBrowser {
    /// Writes the fake into `dir`. `test_path` is the full path of the caller's test function that calls
    /// [`serve_if_requested`] (for example `browser::renderer::tests::fake_browser_process`); `tabs` is how many pages
    /// the browser has when it starts (its start page, plus any the user opened: 0 for a browser without a window).
    /// As in Chrome, every `Target.createTarget` adds a page to these.
    pub fn install(dir: &Path, test_path: &str, mode: Mode, tabs: usize) -> std::io::Result<FakeBrowser> {
        let me = std::env::current_exe()?;
        let args_file = dir.join("args.txt");
        let url_file = dir.join("url.txt");
        let ctl_file = dir.join("ctl.txt");
        let calls_file = dir.join("calls.txt");
        let mode = if mode == Mode::Obey { "obey" } else { "ignore" };
        let dyld: Vec<(String, String)> = DYLD_VARS
            .iter()
            .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
            .collect();
        let exe = stand_in(
            dir,
            &dyld,
            &args_file,
            &url_file,
            &ctl_file,
            &calls_file,
            mode,
            tabs,
            &me,
            test_path,
        )
        .install()?
        .program()
        .to_path_buf();
        Ok(FakeBrowser {
            exe,
            args_file,
            url_file,
            ctl_file,
            calls_file,
        })
    }

    /// Plays the user: the running fake acts on it within a few milliseconds.
    pub fn act(&self, action: UserAction) -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&self.ctl_file)?;
        writeln!(f, "{}", action.word())
    }

    /// The DevTools methods the fake was sent so far, in order.
    pub fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(&self.calls_file)
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// The arguments the fake was started with (empty until it started).
    pub fn args(&self) -> Vec<String> {
        std::fs::read_to_string(&self.args_file)
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }
}

/// Becomes the fake browser when this process was started by [`FakeBrowser::install`]'s script; returns at once otherwise
/// (so the caller's marker test is harmless in a normal run). Never returns when it serves.
pub fn serve_if_requested() {
    let Ok(args_file) = std::env::var(ARGS_ENV) else { return };
    let ignore = std::env::var(MODE_ENV).as_deref() == Ok("ignore");
    let tabs: usize = std::env::var(TABS_ENV).ok().and_then(|t| t.parse().ok()).unwrap_or(1);
    let url_file = std::env::var("FAKE_BROWSER_URL").unwrap_or_default();
    apply_control();
    // An idle fake still quits when told to.
    std::thread::spawn(|| loop {
        apply_control();
        std::thread::sleep(TICK);
    });
    let dir = std::fs::read_to_string(&args_file)
        .ok()
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("--user-data-dir=").map(PathBuf::from)));
    let (Some(dir), Ok(listener)) = (dir, TcpListener::bind("127.0.0.1:0")) else {
        std::process::exit(2)
    };
    let Ok(addr) = listener.local_addr() else { std::process::exit(2) };
    if std::fs::write(dir.join("DevToolsActivePort"), format!("{}\n/devtools/browser/fake\n", addr.port())).is_err() {
        std::process::exit(2);
    }
    for stream in listener.incoming().flatten() {
        let url_file = url_file.clone();
        std::thread::spawn(move || connection(stream, ignore, tabs, &url_file));
    }
    std::process::exit(0);
}

/// The user's actions so far (the fake is its own process, so process-wide flags are its state).
static WINDOW_CLOSED: AtomicBool = AtomicBool::new(false);
static CONNECTION_DROPPED: AtomicBool = AtomicBool::new(false);
static LOADS_STALLED: AtomicBool = AtomicBool::new(false);
static PAGE_LIST_HELD: AtomicBool = AtomicBool::new(false);
static ATTACH_HELD: AtomicBool = AtomicBool::new(false);
static EVALUATE_HELD: AtomicBool = AtomicBool::new(false);
static ANSWERS_RELEASED: AtomicBool = AtomicBool::new(false);
/// Pages opened over DevTools (`Target.createTarget`), besides the ones the browser started with.
static CREATED: AtomicUsize = AtomicUsize::new(0);

/// Notes a method the fake was sent (best effort: a test that reads it waits for what it expects).
fn record_call(method: &str) {
    let Ok(path) = std::env::var(CALLS_ENV) else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{method}");
    }
}

/// How much of the control file was there when this fake started: earlier actions were for an earlier browser.
static CONTROL_START: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

/// Applies the user's actions written since this fake started. Called before every answer too, so an action written
/// before a request is always seen by it.
fn apply_control() {
    let Ok(ctl) = std::env::var(CTL_ENV) else { return };
    let words = std::fs::read_to_string(ctl).unwrap_or_default();
    let start = *CONTROL_START.get_or_init(|| words.len());
    for w in words.get(start..).unwrap_or("").lines() {
        match w.trim() {
            "close-window" => WINDOW_CLOSED.store(true, Ordering::SeqCst),
            "drop-connection" => CONNECTION_DROPPED.store(true, Ordering::SeqCst),
            "stall-loads" => LOADS_STALLED.store(true, Ordering::SeqCst),
            "hold-page-list" => PAGE_LIST_HELD.store(true, Ordering::SeqCst),
            "hold-attach" => ATTACH_HELD.store(true, Ordering::SeqCst),
            "hold-evaluate" => EVALUATE_HELD.store(true, Ordering::SeqCst),
            "release-answers" => ANSWERS_RELEASED.store(true, Ordering::SeqCst),
            "quit" => std::process::exit(0),
            _ => {}
        }
    }
}

fn connection(stream: std::net::TcpStream, ignore: bool, tabs: usize, url_file: &str) {
    // The handshake waits for the whole upgrade request however slow the client is; only the message loop ticks (a
    // read timeout during the handshake abandoned it, "Handshake not finished", on a loaded machine).
    let Ok(mut ws) = tungstenite::accept(stream) else { return };
    let _ = ws.get_ref().set_read_timeout(Some(TICK));
    let mut told_closed = false;
    // Applies the user's actions; false when the connection is to end. The browser reports a closed window once, as
    // Chrome does: the page's session detaches and its target is destroyed.
    let mut user_acts = |ws: &mut tungstenite::WebSocket<std::net::TcpStream>| -> bool {
        apply_control();
        if CONNECTION_DROPPED.load(Ordering::SeqCst) {
            return false;
        }
        if WINDOW_CLOSED.load(Ordering::SeqCst) && !told_closed {
            told_closed = true;
            for ev in [
                json!({"method": "Target.detachedFromTarget", "params": {"sessionId": "S1", "targetId": "T1"}}),
                json!({"method": "Target.targetDestroyed", "params": {"targetId": "T1"}}),
            ] {
                if ws.send(Message::text(ev.to_string())).is_err() {
                    return false;
                }
            }
        }
        true
    };
    loop {
        if !user_acts(&mut ws) {
            return;
        }
        let msg = match ws.read() {
            Ok(m) => m,
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
            Err(_) => return,
        };
        let Message::Text(t) = msg else { continue };
        if !user_acts(&mut ws) {
            return;
        }
        let closed = WINDOW_CLOSED.load(Ordering::SeqCst);
        let Ok(req) = serde_json::from_str::<Value>(t.as_str()) else {
            continue;
        };
        let method = req["method"].as_str().unwrap_or("");
        record_call(method);
        let url = || std::fs::read_to_string(url_file).unwrap_or_else(|_| "about:blank".into());
        let on_page = req.get("sessionId").is_some() || method == "Target.getTargetInfo";
        let mut reply = if closed && on_page {
            // The window is gone: its page answers no more, as Chrome answers for a closed target.
            json!({"id": req["id"], "error": {"code": -32001, "message": "No session or target with given id found"}})
        } else {
            let result = match method {
                "Target.createTarget" => {
                    let n = tabs + CREATED.fetch_add(1, Ordering::SeqCst) + 1;
                    json!({"targetId": format!("T{n}")})
                }
                "Target.attachToTarget" => json!({"sessionId": "S1"}),
                "Browser.getWindowForTarget" => json!({"windowId": 1}),
                "Target.getTargets" => {
                    // The start page(s) first, then the pages opened over DevTools; a closed window has none.
                    let open = if closed { 0 } else { tabs + CREATED.load(Ordering::SeqCst) };
                    json!({"targetInfos": (0..open).map(|i| json!({"targetId": format!("T{}", i + 1), "type": "page", "url": "about:blank"})).collect::<Vec<_>>()})
                }
                "Target.getTargetInfo" => json!({"targetInfo": {"url": url(), "title": "fake"}}),
                "Runtime.evaluate" if req["params"]["expression"].as_str() == Some("document.readyState") => {
                    json!({"result": {"value": "complete"}})
                }
                "Page.navigate" => {
                    if let Some(u) = req["params"]["url"].as_str() {
                        let _ = std::fs::write(url_file, u);
                    }
                    json!({"frameId": "F"})
                }
                _ => json!({}),
            };
            json!({"id": req["id"], "result": result})
        };
        if let Some(s) = req.get("sessionId") {
            reply["sessionId"] = s.clone();
        }
        let held = match method {
            "Target.getTargets" => &PAGE_LIST_HELD,
            "Target.attachToTarget" => &ATTACH_HELD,
            "Runtime.evaluate" => &EVALUATE_HELD,
            _ => &ANSWERS_RELEASED,
        };
        if !std::ptr::eq(held, &ANSWERS_RELEASED) && held.load(Ordering::SeqCst) && !ANSWERS_RELEASED.load(Ordering::SeqCst) {
            // A slow browser: the answer waits for the test's release (the control thread still quits the fake).
            record_call(&format!("held {method}"));
            while !ANSWERS_RELEASED.load(Ordering::SeqCst) {
                apply_control();
                std::thread::sleep(TICK);
            }
        }
        if ws.send(Message::text(reply.to_string())).is_err() {
            return;
        }
        if method == "Page.navigate" && !closed && !LOADS_STALLED.load(Ordering::SeqCst) {
            let ev = json!({"method": "Page.loadEventFired", "sessionId": "S1", "params": {}});
            let _ = ws.send(Message::text(ev.to_string()));
        }
        if method == "Browser.close" && !ignore {
            let _ = std::io::stdout().flush();
            std::process::exit(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression (a loaded machine): the 10 ms read timeout of the fake's message loop was set before the WebSocket
    /// handshake, so a client whose upgrade request came in slower than that saw the connection close ("WebSocket
    /// protocol error: Handshake not finished"). Half a request, then a wait on our side: the fake must still be there.
    #[test]
    fn a_slow_handshake_is_waited_for() {
        use std::io::Read;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Some(Ok(stream)) = listener.incoming().next() {
                connection(stream, false, 1, "");
            }
        });
        let mut c = std::net::TcpStream::connect(addr).unwrap();
        c.write_all(b"GET /devtools/browser/fake HTTP/1.1\r\nHost: 127.0.0.1\r\n").unwrap();
        // Twenty of the fake's ticks without the rest of the request: a fake that gave up has closed by then.
        c.set_read_timeout(Some(TICK * 20)).unwrap();
        let mut b = [0u8; 64];
        match c.read(&mut b) {
            Ok(0) => panic!("the fake closed the connection during the handshake"),
            Ok(n) => panic!(
                "unexpected answer before the request was complete: {:?}",
                String::from_utf8_lossy(&b[..n])
            ),
            Err(e) => assert!(
                matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut),
                "{e}"
            ),
        }
        c.write_all(
            b"Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
        )
        .unwrap();
        c.set_read_timeout(None).unwrap();
        let n = c.read(&mut b).unwrap();
        assert!(
            String::from_utf8_lossy(&b[..n]).starts_with("HTTP/1.1 101"),
            "{:?}",
            String::from_utf8_lossy(&b[..n])
        );
    }

    /// Regression (macOS CI, nextest): the test binary finds libduckdb through `DYLD_FALLBACK_LIBRARY_PATH`, which the
    /// shell running the fake dropped, so every fake browser exited at once ("exited right after it started"). The fake is run
    /// for real, with a stand-in in place of the test binary that writes the environment and the arguments it was given.
    #[test]
    fn the_fake_restores_every_loader_variable_and_marks_the_child() {
        let d = tempfile::tempdir().unwrap();
        let seen = d.path().join("seen");
        let me = paddock_testkit::StandIn::new(d.path(), "test-binary")
            .answers(paddock_testkit::Actions::new().writes_with(
                &paddock_testkit::template_literal(&seen.display().to_string()),
                "{environ}\nARGS={args}",
            ))
            .install()
            .unwrap();
        let dyld = vec![
            ("DYLD_LIBRARY_PATH".to_string(), "/a".to_string()),
            ("DYLD_FALLBACK_LIBRARY_PATH".to_string(), "/b:/c".to_string()),
        ];
        let p = |n: &str| d.path().join(n);
        let exe = stand_in(
            d.path(),
            &dyld,
            &p("args"),
            &p("url"),
            &p("ctl"),
            &p("calls"),
            "obey",
            1,
            me.program(),
            "x::fake",
        )
        .install()
        .unwrap();
        let o = paddock_testkit::TestCommand::new(exe.program())
            .args(["--headless", "a b"])
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let text = std::fs::read_to_string(&seen).unwrap();
        for want in [
            "DYLD_LIBRARY_PATH=/a".to_string(),
            "DYLD_FALLBACK_LIBRARY_PATH=/b:/c".to_string(),
            format!("{CHILD_ENV}=1"),
            "ARGS=x::fake --exact --nocapture".to_string(),
        ] {
            assert!(text.lines().any(|l| l == want), "{want} missing in {text}");
        }
        assert_eq!(
            std::fs::read_to_string(p("args")).unwrap(),
            "--headless\na b\n",
            "the command line, one argument per line"
        );
        assert!(DYLD_VARS.contains(&"DYLD_FALLBACK_LIBRARY_PATH"));
    }
}
