//! A fake DevTools browser for tests (Unix): never a real browser. [`FakeBrowser::install`] writes an executable script that
//! records its command line and re-runs the *test binary* with a marker environment variable; a test function of the
//! caller then calls [`serve_if_requested`], which becomes the fake (a loopback WebSocket server that answers the few
//! commands the driver sends). Unix only: the script is `sh`.
//!
//! The fake exits on `Browser.close` unless its [`Mode`] says to ignore it, so both the graceful close and the
//! kill-after-timeout paths can be tested. A test plays the user with [`FakeBrowser::act`]: closing the window (the
//! page target goes away but the process stays, as Chrome does on macOS), quitting the browser, or losing the
//! DevTools connection.

use serde_json::{Value, json};
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use tungstenite::Message;

const ARGS_ENV: &str = "PADDOCK_FAKE_BROWSER_ARGS_FILE";
/// Set in every fake browser process: an application's `main` refuses to start when it sees it, so a fake that names
/// the wrong binary can never open the app's windows on the desktop.
pub use crate::FAKE_CHILD_ENV as CHILD_ENV;
const MODE_ENV: &str = "PADDOCK_FAKE_BROWSER_MODE";
const TABS_ENV: &str = "PADDOCK_FAKE_BROWSER_TABS";
const CTL_ENV: &str = "PADDOCK_FAKE_BROWSER_CTL";
const URL_ENV: &str = "PADDOCK_FAKE_BROWSER_URL";
const EVAL_ENV: &str = "PADDOCK_FAKE_BROWSER_EVAL";
const CALLS_ENV: &str = "PADDOCK_FAKE_BROWSER_CALLS";
/// The file beside the eval file that holds `Browser.getHistograms`' answer.
const HISTOGRAMS_FILE: &str = "histograms.json";
/// How often the fake looks at its control file and at its sockets.
const TICK: Duration = Duration::from_millis(10);
/// The longest the fake waits for a client's upgrade request.
const HANDSHAKE: Duration = Duration::from_secs(10);

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
    /// The page's own first navigation is still running: `Page.enable` reports it (`Page.frameStartedNavigating`, as
    /// Chrome does), and a `Page.navigate` sent before [`UserAction::FinishStartPage`] is cancelled
    /// (`net::ERR_ABORTED`). Written after the launch and before the page is attached.
    SlowStartPage,
    /// The first navigation of [`UserAction::SlowStartPage`] stops loading (`Page.frameStoppedLoading`).
    FinishStartPage,
    /// `Page.getFrameTree` fails from now on, and the page's main frame is reported under its target id (as Chrome
    /// names a page target's main frame); otherwise the main frame is `F`, unlike any target id.
    NoFrameTree,
    /// The network service restarts: `SystemInfo.getProcessInfo` names another process for it from now on (the fake
    /// starts with process 100).
    RestartNetworkService,
    /// Every `Page.navigate` from now on is cancelled (`net::ERR_ABORTED`, not a download, nothing replaces it).
    AbortLoads,
}

impl UserAction {
    fn word(self) -> &'static str {
        match self {
            UserAction::CloseWindow => "close-window",
            UserAction::Quit => "quit",
            UserAction::DropConnection => "drop-connection",
            UserAction::StallLoads => "stall-loads",
            UserAction::SlowStartPage => "slow-start-page",
            UserAction::FinishStartPage => "finish-start-page",
            UserAction::NoFrameTree => "no-frame-tree",
            UserAction::RestartNetworkService => "restart-network-service",
            UserAction::AbortLoads => "abort-loads",
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
    /// The value every `Runtime.evaluate` returns (JSON), except `document.readyState` ("complete"); a test writes it
    /// with [`FakeBrowser::page_shows`]. Without the file the result is `null`. A JSON object with an `"exception"` key
    /// makes the script fail with that text.
    pub eval_file: PathBuf,
}

/// The dynamic-loader variables a test binary may need (cargo and nextest put the native library folders in
/// `DYLD_FALLBACK_LIBRARY_PATH`). macOS strips every `DYLD_*` variable when it runs a system shell (SIP), so the
/// fake's script sets them again for the test binary it starts.
const DYLD_VARS: [&str; 3] = [
    "DYLD_LIBRARY_PATH",
    "DYLD_FALLBACK_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
];

/// The fake's start script: the loader variables, the fake's own settings and [`CHILD_ENV`], then the test binary
/// filtered to the one helper test (`--exact`).
#[allow(clippy::too_many_arguments)]
fn script(
    dyld: &[(String, String)],
    f: &FakeBrowser,
    mode: &str,
    tabs: usize,
    me: &Path,
    test_path: &str,
) -> String {
    let dyld: String = dyld
        .iter()
        .map(|(k, v)| format!("export {k}='{v}'\n"))
        .collect();
    format!(
        "#!/bin/sh\n{dyld}printf '%s\\n' \"$@\" > '{args}'\nexport {CHILD_ENV}=1 {ARGS_ENV}='{args}' {MODE_ENV}={mode} {TABS_ENV}={tabs} {URL_ENV}='{url}' {CTL_ENV}='{ctl}' {CALLS_ENV}='{calls}' {EVAL_ENV}='{eval}'\nexec '{me}' '{test_path}' --exact --nocapture\n",
        args = f.args_file.display(),
        url = f.url_file.display(),
        ctl = f.ctl_file.display(),
        calls = f.calls_file.display(),
        eval = f.eval_file.display(),
        me = me.display(),
    )
}

impl FakeBrowser {
    /// Writes the fake into `dir`. `test_path` is the full path of the caller's test function that calls
    /// [`serve_if_requested`] (for example `browser::renderer::tests::fake_browser_process`); `tabs` is how many pages
    /// the browser has when it starts (its start page, plus any the user opened: 0 for a browser without a window).
    /// As in Chrome, every `Target.createTarget` adds a page to these.
    pub fn install(
        dir: &Path,
        test_path: &str,
        mode: Mode,
        tabs: usize,
    ) -> std::io::Result<FakeBrowser> {
        let me = std::env::current_exe()?;
        let f = FakeBrowser {
            exe: dir.join("fake-browser.sh"),
            args_file: dir.join("args.txt"),
            url_file: dir.join("url.txt"),
            ctl_file: dir.join("ctl.txt"),
            calls_file: dir.join("calls.txt"),
            eval_file: dir.join("eval.json"),
        };
        let mode = if mode == Mode::Obey { "obey" } else { "ignore" };
        let dyld: Vec<(String, String)> = DYLD_VARS
            .iter()
            .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
            .collect();
        std::fs::write(&f.exe, script(&dyld, &f, mode, tabs, &me, test_path))?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&f.exe, std::fs::Permissions::from_mode(0o755))?;
        Ok(f)
    }

    /// Plays the user: the running fake acts on it within a few milliseconds.
    pub fn act(&self, action: UserAction) -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.ctl_file)?;
        writeln!(f, "{}", action.word())
    }

    /// The DevTools methods the fake was sent so far, in order.
    pub fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(&self.calls_file)
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// What every page script returns from now on (see [`FakeBrowser::eval_file`]).
    pub fn page_shows(&self, value: &Value) -> std::io::Result<()> {
        std::fs::write(&self.eval_file, value.to_string())
    }

    /// What `Browser.getHistograms` answers from now on: `Some(result)`, or a protocol error for `None`.
    pub fn histograms(&self, result: Option<&Value>) -> std::io::Result<()> {
        let path = self.eval_file.with_file_name(HISTOGRAMS_FILE);
        std::fs::write(
            path,
            result.map_or_else(|| "error".to_string(), Value::to_string),
        )
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
    let Ok(args_file) = std::env::var(ARGS_ENV) else {
        return;
    };
    let ignore = std::env::var(MODE_ENV).as_deref() == Ok("ignore");
    let tabs: usize = std::env::var(TABS_ENV)
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or(1);
    let url_file = std::env::var(URL_ENV).unwrap_or_default();
    apply_control();
    // An idle fake still quits when told to.
    std::thread::spawn(|| {
        loop {
            apply_control();
            std::thread::sleep(TICK);
        }
    });
    let dir = std::fs::read_to_string(&args_file).ok().and_then(|s| {
        s.lines()
            .find_map(|l| l.strip_prefix("--user-data-dir=").map(PathBuf::from))
    });
    let (Some(dir), Ok(listener)) = (dir, TcpListener::bind("127.0.0.1:0")) else {
        std::process::exit(2)
    };
    let Ok(addr) = listener.local_addr() else {
        std::process::exit(2)
    };
    if std::fs::write(
        dir.join("DevToolsActivePort"),
        format!("{}\n/devtools/browser/fake\n", addr.port()),
    )
    .is_err()
    {
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
static START_PAGE_SLOW: AtomicBool = AtomicBool::new(false);
static START_PAGE_FINISHED: AtomicBool = AtomicBool::new(false);
static NO_FRAME_TREE: AtomicBool = AtomicBool::new(false);
static NETWORK_SERVICE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(100);
static LOADS_ABORTED: AtomicBool = AtomicBool::new(false);
/// The number of the last page opened over DevTools (its target id is `T<n>`).
static LAST_TARGET: AtomicUsize = AtomicUsize::new(0);

/// The id of the page's main frame (see [`UserAction::NoFrameTree`]).
fn main_frame() -> String {
    if NO_FRAME_TREE.load(Ordering::SeqCst) {
        format!("T{}", LAST_TARGET.load(Ordering::SeqCst))
    } else {
        "F".into()
    }
}
/// Pages opened over DevTools (`Target.createTarget`), besides the ones the browser started with.
static CREATED: AtomicUsize = AtomicUsize::new(0);

/// Notes a method the fake was sent (best effort: a test that reads it waits for what it expects).
fn record_call(method: &str) {
    let Ok(path) = std::env::var(CALLS_ENV) else {
        return;
    };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{method}");
    }
}

/// How much of the control file was there when this fake started: earlier actions were for an earlier browser.
static CONTROL_START: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

/// Applies the user's actions written since this fake started. Called before every answer too, so an action written
/// before a request is always seen by it.
fn apply_control() {
    let Ok(ctl) = std::env::var(CTL_ENV) else {
        return;
    };
    let words = std::fs::read_to_string(ctl).unwrap_or_default();
    let start = *CONTROL_START.get_or_init(|| words.len());
    // Each restart written so far is one more process (the file is read again before every answer, so it is a count).
    let restarts = words
        .get(start..)
        .unwrap_or("")
        .lines()
        .filter(|w| w.trim() == "restart-network-service")
        .count();
    NETWORK_SERVICE.store(
        100 + u32::try_from(restarts).unwrap_or(u32::MAX - 100),
        Ordering::SeqCst,
    );
    for w in words.get(start..).unwrap_or("").lines() {
        match w.trim() {
            "close-window" => WINDOW_CLOSED.store(true, Ordering::SeqCst),
            "drop-connection" => CONNECTION_DROPPED.store(true, Ordering::SeqCst),
            "stall-loads" => LOADS_STALLED.store(true, Ordering::SeqCst),
            "slow-start-page" => START_PAGE_SLOW.store(true, Ordering::SeqCst),
            "finish-start-page" => START_PAGE_FINISHED.store(true, Ordering::SeqCst),
            "no-frame-tree" => NO_FRAME_TREE.store(true, Ordering::SeqCst),
            "abort-loads" => LOADS_ABORTED.store(true, Ordering::SeqCst),
            "quit" => std::process::exit(0),
            _ => {}
        }
    }
}

/// The fake's side of the WebSocket upgrade (bounded by [`HANDSHAKE`]), then the connection's tick: the tick is set only
/// after the handshake. A tick during the handshake gave up on a client whose upgrade request came later than one tick (a loaded
/// runner) and dropped the connection (Spotter's copy of this fake, Windows CI runs 19116 and 19657, os error 10053;
/// Debrief's driver had the same, "Handshake not finished").
fn handshake(stream: std::net::TcpStream) -> Option<tungstenite::WebSocket<std::net::TcpStream>> {
    // Bounded far above any scheduling delay, so a client that never sends its upgrade request cannot hold the thread.
    let _ = stream.set_read_timeout(Some(HANDSHAKE));
    let ws = tungstenite::accept(stream).ok()?;
    let _ = ws.get_ref().set_read_timeout(Some(TICK));
    Some(ws)
}

fn connection(stream: std::net::TcpStream, ignore: bool, tabs: usize, url_file: &str) {
    let Some(mut ws) = handshake(stream) else {
        return;
    };
    // What a real browser may also send: a ping, and a text the client does not understand.
    let _ = ws.send(Message::Ping(Vec::new().into()));
    let _ = ws.send(Message::text("not json"));
    let _ = ws.send(Message::text(r#"{"neither": "a reply nor an event"}"#));
    let mut told_closed = false;
    let mut told_finished = false;
    // Applies the user's actions; false when the connection is to end. The browser reports a closed window once, as
    // Chrome does: the page's session detaches and its target is destroyed.
    let mut user_acts = |ws: &mut tungstenite::WebSocket<std::net::TcpStream>| -> bool {
        apply_control();
        if CONNECTION_DROPPED.load(Ordering::SeqCst) {
            let _ = ws.close(None);
            let _ = ws.flush();
            return false;
        }
        if START_PAGE_SLOW.load(Ordering::SeqCst)
            && START_PAGE_FINISHED.load(Ordering::SeqCst)
            && !told_finished
        {
            told_finished = true;
            let ev = json!({"method": "Page.frameStoppedLoading", "sessionId": "S1", "params": {"frameId": main_frame()}});
            if ws.send(Message::text(ev.to_string())).is_err() {
                return false;
            }
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
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
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
        if method == "Debug.hang" {
            // Never answered (a reply nobody waits for is sent instead): the caller's time limit ends the wait.
            let _ = ws.send(Message::text(r#"{"id": 4294967295, "result": {}}"#));
            continue;
        }
        if method == "Debug.flood" {
            // More events than the client keeps.
            for i in 0..1100 {
                let ev = json!({"method": "Debug.event", "params": {"n": i}});
                if ws.send(Message::text(ev.to_string())).is_err() {
                    return;
                }
            }
        }
        // The first navigation still running (`UserAction::SlowStartPage`): reported before the reply to `Page.enable`.
        let starting =
            START_PAGE_SLOW.load(Ordering::SeqCst) && !START_PAGE_FINISHED.load(Ordering::SeqCst);
        if method == "Page.enable" && starting {
            // A child frame's navigation first: only the main frame's (frame `F`, see `Page.getFrameTree`) is waited for.
            for (frame, loader) in [("C".to_string(), "LC"), (main_frame(), "L0")] {
                let ev = json!({"method": "Page.frameStartedNavigating", "sessionId": "S1", "params": {
                    "frameId": frame, "loaderId": loader, "url": "about:blank", "navigationType": "differentDocument"}});
                if ws.send(Message::text(ev.to_string())).is_err() {
                    return;
                }
            }
        }
        let url = || std::fs::read_to_string(url_file).unwrap_or_else(|_| "about:blank".into());
        let on_page = req.get("sessionId").is_some() || method == "Target.getTargetInfo";
        // `Browser.getHistograms` answers from `histograms.json` beside the eval file ([`FakeBrowser::histograms`]):
        // `error` is a protocol error, JSON is the result, no file is a reply without histograms.
        let histograms = (method == "Browser.getHistograms")
            .then(|| {
                std::env::var(EVAL_ENV).ok().and_then(|p| {
                    std::fs::read_to_string(Path::new(&p).with_file_name(HISTOGRAMS_FILE)).ok()
                })
            })
            .flatten();
        let mut reply = if method == "Page.getFrameTree" && NO_FRAME_TREE.load(Ordering::SeqCst) {
            json!({"id": req["id"], "error": {"code": -32000, "message": "no frame tree"}})
        } else if histograms.as_deref() == Some("error") {
            json!({"id": req["id"], "error": {"code": -32601, "message": "'Browser.getHistograms' wasn't found"}})
        } else if let Some(h) = histograms
            .as_deref()
            .and_then(|t| serde_json::from_str::<Value>(t).ok())
        {
            json!({"id": req["id"], "result": h})
        } else if closed && on_page {
            // The window is gone: its page answers no more, as Chrome answers for a closed target.
            json!({"id": req["id"], "error": {"code": -32001, "message": "No session or target with given id found"}})
        } else {
            let result = match method {
                "Target.createTarget" => {
                    let n = tabs + CREATED.fetch_add(1, Ordering::SeqCst) + 1;
                    LAST_TARGET.store(n, Ordering::SeqCst);
                    json!({"targetId": format!("T{n}")})
                }
                "Target.attachToTarget" => json!({"sessionId": "S1"}),
                "Browser.getWindowForTarget" => json!({"windowId": 1}),
                "Target.getTargets" => {
                    // The start page(s) first, then the pages opened over DevTools; a closed window has none.
                    let open = if closed {
                        0
                    } else {
                        tabs + CREATED.load(Ordering::SeqCst)
                    };
                    json!({"targetInfos": (0..open).map(|i| json!({"targetId": format!("T{}", i + 1), "type": "page", "url": "about:blank"})).collect::<Vec<_>>()})
                }
                "Target.getTargetInfo" => json!({"targetInfo": {"url": url(), "title": "fake"}}),
                "Runtime.evaluate"
                    if req["params"]["expression"].as_str() == Some("document.readyState") =>
                {
                    json!({"result": {"value": "complete"}})
                }
                "Runtime.evaluate" => {
                    let v: Value = std::env::var(EVAL_ENV)
                        .ok()
                        .and_then(|p| std::fs::read_to_string(p).ok())
                        .and_then(|t| serde_json::from_str(&t).ok())
                        .unwrap_or(Value::Null);
                    match v.get("exception").and_then(Value::as_str) {
                        Some(e) => json!({"result": {}, "exceptionDetails": {"text": e}}),
                        None => json!({"result": {"value": v}}),
                    }
                }
                "Page.captureScreenshot" => json!({"data": "iVBORw0KGgo="}),
                "Page.getFrameTree" => json!({"frameTree": {"frame": {"id": "F"}}}),
                "Page.getNavigationHistory" => {
                    json!({"currentIndex": 1, "entries": [{"id": 1}, {"id": 2}]})
                }
                "SystemInfo.getProcessInfo" => json!({"processInfo": [
                    {"type": "browser", "id": 1},
                    {"type": "network.mojom.NetworkService", "id": NETWORK_SERVICE.load(Ordering::SeqCst)},
                ]}),
                "Page.navigate" if starting || LOADS_ABORTED.load(Ordering::SeqCst) => {
                    json!({"frameId": main_frame(), "loaderId": "L1", "errorText": "net::ERR_ABORTED", "isDownload": false})
                }
                "Page.navigate"
                    if req["params"]["url"]
                        .as_str()
                        .is_some_and(|u| u.contains("unreachable")) =>
                {
                    json!({"frameId": "F", "errorText": "net::ERR_NAME_NOT_RESOLVED"})
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
        if ws.send(Message::text(reply.to_string())).is_err() {
            return;
        }
        let loads = matches!(method, "Page.navigate" | "Page.navigateToHistoryEntry")
            && !((starting || LOADS_ABORTED.load(Ordering::SeqCst)) && method == "Page.navigate");
        if loads && !closed && !LOADS_STALLED.load(Ordering::SeqCst) {
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

    /// Regression: a client that sends its upgrade request many ticks after connecting still gets its connection. The
    /// quiet spell makes the old failure certain (the fake gave up after one tick and closed the connection); the
    /// client's read timeout only ends the quiet spell, it is not a wait for the server.
    #[test]
    fn the_fake_waits_for_a_late_handshake_and_then_ticks() {
        use std::io::Read;
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let mut raw = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (server, _) = l.accept().unwrap();
        let served = std::thread::spawn(move || {
            handshake(server).map(|ws| ws.get_ref().read_timeout().ok().flatten())
        });
        raw.set_read_timeout(Some(TICK * 50)).unwrap();
        let quiet = raw.read(&mut [0u8; 1]);
        assert!(
            matches!(&quiet, Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)),
            "the fake ended the connection before the client's handshake: {quiet:?}"
        );
        raw.set_read_timeout(Some(TICK * 500)).unwrap(); // hang guard only
        let url = format!("ws://127.0.0.1:{port}/devtools/browser/fake");
        tungstenite::client(url.as_str(), raw).expect("the late handshake completes");
        assert_eq!(
            served.join().unwrap(),
            Some(Some(TICK)),
            "the connection ticks once the handshake is done"
        );
    }

    /// Regression (macOS CI, nextest): the test binary finds its native libraries through `DYLD_FALLBACK_LIBRARY_PATH`,
    /// which the shell running the fake dropped, so every fake browser exited at once.
    #[test]
    fn the_script_restores_every_loader_variable_and_marks_the_child() {
        let dyld = vec![
            ("DYLD_LIBRARY_PATH".to_string(), "/a".to_string()),
            (
                "DYLD_FALLBACK_LIBRARY_PATH".to_string(),
                "/b:/c".to_string(),
            ),
        ];
        let f = FakeBrowser {
            exe: "/t/x".into(),
            args_file: "/t/args".into(),
            url_file: "/t/url".into(),
            ctl_file: "/t/ctl".into(),
            calls_file: "/t/calls".into(),
            eval_file: "/t/eval".into(),
        };
        let s = script(&dyld, &f, "obey", 1, Path::new("/bin/test"), "x::fake");
        assert!(s.contains("export DYLD_LIBRARY_PATH='/a'\n"), "{s}");
        assert!(
            s.contains("export DYLD_FALLBACK_LIBRARY_PATH='/b:/c'\n"),
            "{s}"
        );
        assert!(s.contains(&format!("export {CHILD_ENV}=1 ")), "{s}");
        assert!(s.contains(&format!("{EVAL_ENV}='/t/eval'")), "{s}");
        assert!(
            s.ends_with("exec '/bin/test' 'x::fake' --exact --nocapture\n"),
            "{s}"
        );
    }
}
