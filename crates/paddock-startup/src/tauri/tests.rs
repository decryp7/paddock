//! The adapter on a mock Tauri app (`tauri::test`): the commands on the real invoke path behind the command guard, the
//! ticker's actions with a fake clock, the hooks and the question.

use super::*;
use ::tauri::ipc::{CallbackFn, InvokeBody};
use ::tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use ::tauri::webview::InvokeRequest;
use ::tauri::{Listener, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use paddock_commands::tauri::Guard;
use serde_json::{Value, json};
use std::time::Duration;

type App = ::tauri::App<MockRuntime>;

/// Every log record of this crate, as (level, window, message).
static LOGGED: Mutex<Vec<(log::Level, String, String)>> = Mutex::new(Vec::new());

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        if !record.target().starts_with("paddock_startup") {
            return;
        }
        let window = record
            .key_values()
            .get("Window".into())
            .map(|v| v.to_string())
            .unwrap_or_default();
        lock(&LOGGED).push((record.level(), window, record.args().to_string()));
    }
    fn flush(&self) {}
}

/// The records logged for window `label` (tests use their own labels: the logger is process-wide).
/// Installs the capturing logger. Every test that reads `log_of` calls it (directly or through `app_with`): under nextest
/// each test is its own process, so a test cannot rely on another one having installed it first.
fn capture() {
    let _ = log::set_logger(&Capture);
    log::set_max_level(log::LevelFilter::Trace);
}

fn log_of(label: &str) -> Vec<(log::Level, String)> {
    lock(&LOGGED)
        .iter()
        .filter(|(_, w, _)| w == label)
        .map(|(l, _, m)| (*l, m.clone()))
        .collect()
}

/// What the hooks and the asker saw.
#[derive(Default)]
struct Seen {
    unresponsive: Vec<(String, bool)>,
    ready: Vec<(String, Option<Recovery>)>,
    safe_mode: Vec<String>,
    relaunch: Vec<String>,
    asked: Vec<(String, Prompt)>,
    answers: Vec<Box<dyn FnOnce(Choice) + Send>>,
}

type Shared = Arc<Mutex<Seen>>;

struct FakeAsker(Shared);

impl Asker<MockRuntime> for FakeAsker {
    fn ask(
        &self,
        _app: &AppHandle<MockRuntime>,
        label: &str,
        prompt: Prompt,
        answer: Box<dyn FnOnce(Choice) + Send>,
    ) {
        let mut seen = lock(&self.0);
        seen.asked.push((label.to_string(), prompt));
        seen.answers.push(answer);
    }
}

/// The first design's timings (a reloaded page is asked about after one more `ready_timeout`, Keep Waiting does not lengthen the wait),
/// so the flows below read in short times; the longer waits are tested on the watchdog itself.
fn quick() -> Config {
    Config {
        ask_timeout: Config::default().ready_timeout,
        max_doublings: 0,
        ..Config::default()
    }
}

fn builder(seen: &Shared) -> StartupBuilder<MockRuntime> {
    let (a, b, c, d) = (seen.clone(), seen.clone(), seen.clone(), seen.clone());
    Startup::builder()
        .config(quick())
        .on_unresponsive(move |_app, label, on| lock(&a).unresponsive.push((label.to_string(), on)))
        .on_ready(move |webview, why| lock(&b).ready.push((webview.label().to_string(), why)))
        .safe_mode(move |_app, label| {
            lock(&c).safe_mode.push(label.to_string());
            if label == "w-safe-fails" {
                Err("no relaunch here".into())
            } else {
                Ok(())
            }
        })
        .relaunch(move |app| {
            let label = app
                .webview_windows()
                .keys()
                .min()
                .cloned()
                .unwrap_or_default();
            lock(&d).relaunch.push(label.clone());
            if label == "w-2" {
                Err("no restart here".into())
            } else {
                Ok(())
            }
        })
        .asker(FakeAsker(seen.clone()))
}

fn app_with(startup: Startup<MockRuntime>) -> App {
    capture();
    install(mock_builder(), startup)
        .invoke_handler(Guard::new(&crate::COMMANDS).wrap(invoke_handler()))
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn app() -> (App, Shared) {
    let seen = Shared::default();
    (app_with(builder(&seen).build().unwrap()), seen)
}

fn window(app: &App, label: &str) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .build()
        .unwrap()
}

fn invoke(w: &WebviewWindow<MockRuntime>, cmd: &str, body: Value) -> Result<Value, Value> {
    let request = InvokeRequest {
        cmd: cmd.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        }
        .parse()
        .unwrap(),
        body: InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(w, request).map(|b| b.deserialize::<Value>().unwrap())
}

fn startup(app: &App) -> ::tauri::State<'_, Startup<MockRuntime>> {
    app.state::<Startup<MockRuntime>>()
}

/// A page starts loading in `w` (what `on_page_load` does), without starting the real ticker thread.
fn page_started(app: &App, w: &WebviewWindow<MockRuntime>, at: Instant) {
    startup(app).with(|d| d.page_started(w.label(), at));
}

/// Ticks every 500 ms from `from` to `to` ms after `t0`.
fn tick(app: &App, t0: Instant, from: u64, to: u64) {
    let mut t = from;
    while t <= to {
        startup(app).tick(app.handle(), t0 + Duration::from_millis(t));
        t += 500;
    }
}

/// The pings sent, by listening to every target.
fn pings(app: &App) -> Arc<Mutex<Vec<u64>>> {
    let got = Arc::new(Mutex::new(Vec::new()));
    let sink = got.clone();
    app.listen_any(PING_EVENT, move |event| {
        let seq = serde_json::from_str::<Value>(event.payload())
            .ok()
            .and_then(|v| v["seq"].as_u64());
        if let Some(seq) = seq {
            lock(&sink).push(seq);
        }
    });
    got
}

#[test]
fn a_page_that_reports_ready_gets_a_plain_reply_and_the_hooks_hear_it() {
    let (app, seen) = app();
    let main = window(&app, "main");
    page_load(main.as_ref(), PageLoadEvent::Started);
    page_load(main.as_ref(), PageLoadEvent::Finished);
    assert_eq!(startup(&app).with(|d| d.labels()), ["main"]);
    let reply = invoke(&main, "window_ready", json!({})).unwrap();
    assert_eq!(reply, json!({"recovered": null, "ui": null}));
    let seen = lock(&seen);
    assert_eq!(seen.ready, [("main".to_string(), None)]);
    assert_eq!(
        seen.unresponsive,
        [("main".to_string(), false)],
        "a ready page answers: a dialog keeps its own close rules"
    );
    assert!(
        log_of("main")
            .iter()
            .any(|(l, m)| *l == log::Level::Info && m == "Window main ready")
    );
    startup(&app).stop();
}

#[test]
fn a_paused_process_is_said_in_the_log_and_does_not_start_a_question() {
    let (app, seen) = app();
    let w = window(&app, "w-paused");
    let t0 = Instant::now();
    page_started(&app, &w, t0);
    tick(&app, t0, 0, 1_000);
    // No tick for a minute: the process was suspended. The page's clock restarts instead of timing out at once.
    startup(&app).tick(app.handle(), t0 + Duration::from_secs(60));
    assert!(lock(&seen).asked.is_empty() && lock(&seen).unresponsive.is_empty());
    assert!(
        lock(&LOGGED)
            .iter()
            .any(|(l, _, m)| *l == log::Level::Warn && m.starts_with("process.paused")),
        "the pause is in the log"
    );
    startup(&app).stop();
}

#[test]
fn the_question_offers_save_report_and_stays_closed_while_the_report_is_written() {
    let seen = Shared::default();
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let (tx, released) = (Mutex::new(tx), Mutex::new(released));
    let configured = Startup::builder()
        .config(quick())
        .asker(FakeAsker(seen.clone()))
        .report(move |_app, label| {
            let _ = lock(&tx).send(label.to_string());
            // The save dialog is open until the test lets go.
            let _ = lock(&released).recv();
            Ok(())
        })
        .build()
        .unwrap();
    let app = app_with(configured);
    let w = window(&app, "w-report");
    let t0 = Instant::now();
    page_started(&app, &w, t0);
    tick(&app, t0, 0, 10_000);
    tick(&app, t0, 10_500, 20_000);
    let buttons = lock(&seen).asked[0].1.buttons.clone();
    assert!(
        buttons.contains(&crate::ask::SAVE_REPORT_LABEL),
        "{buttons:?}"
    );
    let answer = lock(&seen).answers.pop().unwrap();
    answer(Choice::SaveReport);
    // The hook runs on its own thread: waited on by its signal (bounded), never by a sleep.
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(10)).unwrap(),
        "w-report"
    );
    // The dialog is still open for minutes: the window is not asked again meanwhile.
    tick(&app, t0, 20_500, 200_000);
    assert_eq!(
        lock(&seen).asked.len(),
        1,
        "no second question while the report is being saved"
    );
    assert!(startup(&app).with(|w| w.is_asking("w-report")));
    release.send(()).unwrap();
    let mut tries = 0;
    while startup(&app).with(|w| w.is_asking("w-report")) {
        tries += 1;
        assert!(tries < 5_000_000, "the window never went back to waiting");
        std::thread::yield_now();
    }
    startup(&app).stop();
}

#[test]
fn a_silent_window_is_reloaded_once_then_the_user_is_asked_and_each_answer_is_applied() {
    let (app, seen) = app();
    let w = window(&app, "w-2");
    let t0 = Instant::now();
    page_started(&app, &w, t0);
    tick(&app, t0, 0, 10_000);
    assert!(log_of("w-2").iter().any(|(l, m)| *l == log::Level::Error
        && m.contains("did not finish starting in time; reloading")));
    tick(&app, t0, 10_500, 20_000);
    {
        let s = lock(&seen);
        assert_eq!(
            s.unresponsive,
            [("w-2".to_string(), true)],
            "a dialog may now be closed by the OS close"
        );
        assert_eq!(s.asked.len(), 1);
        assert_eq!(s.asked[0].0, "w-2");
        assert_eq!(s.asked[0].1.question, Question::Startup { safe_mode: true });
    }
    let answer = lock(&seen).answers.pop().unwrap();
    answer(Choice::Wait);
    tick(&app, t0, 20_500, 30_000);
    assert_eq!(
        lock(&seen).asked.len(),
        2,
        "Keep Waiting: asked again after another full timeout"
    );
    let answer = lock(&seen).answers.pop().unwrap();
    answer(Choice::SafeMode);
    assert_eq!(lock(&seen).safe_mode, ["w-2"]);
    // Reload: waiting again (if the reload never starts a page, it is still watched).
    startup(&app).with(|d| d.keep_waiting("w-2", t0));
    let mut t = 30_500;
    while lock(&seen).asked.len() < 3 {
        startup(&app).tick(app.handle(), t0 + Duration::from_millis(t));
        t += 500;
    }
    let answer = lock(&seen).answers.pop().unwrap();
    answer(Choice::Reload);
    assert!(!startup(&app).with(|d| d.is_ready("w-2")));
    // The reloaded page starts loading (on_page_load) and gets a full timeout.
    page_started(&app, &w, t0 + Duration::from_millis(t));
    tick(&app, t0, t + 500, t + 9_000);
    assert_eq!(lock(&seen).asked.len(), 3, "Reload restarted the clock");
    assert!(
        log_of("w-2")
            .iter()
            .any(|(_, m)| m == "Not-responding question answered")
    );
    startup(&app).stop();
}

#[test]
fn a_failing_safe_mode_restart_is_logged_and_without_one_the_question_has_no_safe_mode() {
    let (app, seen) = app();
    let w = window(&app, "w-safe-fails");
    let t0 = Instant::now();
    page_started(&app, &w, t0);
    tick(&app, t0, 0, 20_000);
    let answer = lock(&seen).answers.pop().unwrap();
    answer(Choice::SafeMode);
    assert!(
        log_of("w-safe-fails")
            .iter()
            .any(|(_, m)| m == "Could not restart in Safe Mode: no relaunch here")
    );

    let seen2 = Shared::default();
    let app2 = app_with(
        Startup::builder()
            .config(quick())
            .asker(FakeAsker(seen2.clone()))
            .build()
            .unwrap(),
    );
    let w = window(&app2, "w-nosafe");
    page_started(&app2, &w, t0);
    tick(&app2, t0, 0, 20_000);
    assert_eq!(
        lock(&seen2).asked[0].1.question,
        Question::Startup { safe_mode: false }
    );
    let answer = lock(&seen2).answers.pop().unwrap();
    answer(Choice::SafeMode);
    tick(&app2, t0, 20_500, 30_000);
    assert_eq!(lock(&seen2).asked.len(), 2, "an unoffered Safe Mode waits");
}

#[test]
fn without_an_asker_the_question_is_logged_and_the_window_keeps_waiting() {
    let app = app_with(Startup::builder().config(quick()).build().unwrap());
    let w = window(&app, "w-noask");
    let t0 = Instant::now();
    page_started(&app, &w, t0);
    tick(&app, t0, 0, 20_000);
    assert!(
        log_of("w-noask")
            .iter()
            .any(|(l, m)| *l == log::Level::Warn && m.contains("No asker is installed"))
    );
}

#[test]
fn a_page_that_stops_answering_after_ready_is_marked_asked_about_and_recovers_on_its_next_answer() {
    let (app, seen) = app();
    let got = pings(&app);
    let w = window(&app, "w-3");
    page_load(w.as_ref(), PageLoadEvent::Started);
    invoke(&w, "window_ready", json!({})).unwrap();
    let t0 = Instant::now();
    // A healthy page answers each ping.
    let mut t = 500;
    while t <= 6_000 {
        startup(&app).tick(app.handle(), t0 + Duration::from_millis(t));
        let last = lock(&got).last().copied();
        if let Some(seq) = last {
            invoke(&w, "window_pong", json!({ "seq": seq })).unwrap();
        }
        t += 500;
    }
    assert!(lock(&got).len() >= 3);
    assert_eq!(lock(&seen).asked.len(), 0);
    // Then it hangs.
    tick(&app, t0, 6_500, 20_000);
    {
        let s = lock(&seen);
        assert_eq!(s.asked.len(), 1);
        assert_eq!(s.asked[0].1.question, Question::Unresponsive);
        assert_eq!(s.unresponsive.last(), Some(&("w-3".to_string(), true)));
    }
    assert!(
        log_of("w-3").iter().any(|(l, m)| *l == log::Level::Error
            && m == "Window w-3 stopped responding; asking the user")
    );
    // It answers again (a late answer to its latest ping).
    let seq = *lock(&got).last().unwrap();
    invoke(&w, "window_pong", json!({ "seq": seq })).unwrap();
    assert_eq!(
        lock(&seen).unresponsive.last(),
        Some(&("w-3".to_string(), false))
    );
    assert!(
        log_of("w-3")
            .iter()
            .any(|(_, m)| m == "Window w-3 responds again")
    );
    // The open question's Keep Waiting is harmless now.
    let answer = lock(&seen).answers.pop().unwrap();
    answer(Choice::Wait);
    assert!(!startup(&app).with(|d| d.is_unresponsive("w-3")));
}

#[test]
fn a_paused_app_process_does_not_mark_a_ready_window_unresponsive() {
    let (app, seen) = app();
    let w = window(&app, "w-4");
    page_started(&app, &w, Instant::now());
    invoke(&w, "window_ready", json!({})).unwrap();
    let t0 = Instant::now();
    tick(&app, t0, 500, 3_000);
    // Stopped for a minute, then a tick: the clocks restart.
    tick(&app, t0, 63_000, 63_000);
    tick(&app, t0, 63_500, 70_000);
    assert!(lock(&seen).asked.is_empty());
    assert!(!startup(&app).with(|d| d.is_unresponsive("w-4")));
}

#[test]
fn a_terminated_content_process_reloads_the_page_which_gets_its_checkpoint_back() {
    let (app, _seen) = app();
    let w = window(&app, "w-5");
    page_load(w.as_ref(), PageLoadEvent::Started);
    invoke(&w, "window_ready", json!({})).unwrap();
    invoke(&w, "ui_checkpoint", json!({ "ui": { "query": "timeout" } })).unwrap();
    web_content_terminated(w.as_ref());
    assert!(
        log_of("w-5").iter().any(|(l, m)| *l == log::Level::Error
            && m == "The web content process of window w-5 terminated")
    );
    let reply = invoke(&w, "window_ready", json!({})).unwrap();
    assert_eq!(
        reply,
        json!({"recovered": "terminated", "ui": {"query": "timeout"}})
    );
    // A page that crashes on load is not reloaded in a loop.
    for _ in 0..5 {
        web_content_terminated(w.as_ref());
    }
    let reloads = log_of("w-5")
        .iter()
        .filter(|(_, m)| m.ends_with("terminated"))
        .count();
    assert_eq!(reloads, 6);
    assert!(
        !startup(&app).with(|d| d.terminated("w-5", Instant::now())),
        "the budget is spent"
    );
}

#[test]
fn checkpoints_must_be_objects_and_a_user_reload_restores_nothing() {
    let (app, _seen) = app();
    let w = window(&app, "main");
    page_started(&app, &w, Instant::now());
    let e = invoke(&w, "ui_checkpoint", json!({ "ui": [1, 2] })).unwrap_err();
    assert_eq!(
        e,
        json!({"code": "invalid", "message": "the UI state must be an object"})
    );
    invoke(&w, "ui_checkpoint", json!({ "ui": { "a": 1 } })).unwrap();
    assert_eq!(
        invoke(&w, "window_ready", json!({})).unwrap(),
        json!({"recovered": null, "ui": null})
    );
}

#[test]
fn boot_errors_are_logged_bounded_and_capped() {
    let app = app_with(
        Startup::builder()
            .config(Config {
                max_boot_errors: 2,
                ..Config::default()
            })
            .build()
            .unwrap(),
    );
    let w = window(&app, "p-main-7");
    page_load(w.as_ref(), PageLoadEvent::Started);
    let long = format!("TypeError: x\n{}", "y".repeat(600));
    for kind in ["error", "<img>", "bootstrap"] {
        invoke(
            &w,
            "window_boot_error",
            json!({ "kind": kind, "message": long }),
        )
        .unwrap();
    }
    let lines = log_of("p-main-7");
    assert_eq!(lines.len(), 2, "at most two per page load");
    assert!(
        lines[0]
            .1
            .starts_with("Uncaught error in window p-main-7: TypeError: x y")
    );
    assert!(lines[0].1.ends_with('…') && !lines[0].1.contains('\n'));
    assert!(
        lines[1].1.starts_with("Uncaught other in window p-main-7"),
        "the kind is not trusted"
    );
}

#[test]
fn the_guard_keeps_the_checkpoint_to_main_windows() {
    let (app, _seen) = app();
    let popout = window(&app, "p-main-1");
    let e = invoke(&popout, "ui_checkpoint", json!({ "ui": {} })).unwrap_err();
    assert_eq!(e["code"], "denied");
    let outside = window(&app, "browser-1");
    assert_eq!(
        invoke(&outside, "window_ready", json!({})).unwrap_err()["code"],
        "denied"
    );
}

#[test]
fn closed_windows_are_forgotten_on_the_next_tick() {
    let (app, _seen) = app();
    let w = window(&app, "w-6");
    page_started(&app, &w, Instant::now());
    startup(&app).with(|d| d.page_started("w-gone", Instant::now()));
    startup(&app).checkpoint("w-gone", json!({})).unwrap();
    startup(&app).tick(app.handle(), Instant::now());
    assert_eq!(startup(&app).with(|d| d.labels()), ["w-6"]);
}

#[test]
fn only_the_apps_own_pages_are_watched() {
    assert!(watched_by_default("main", "main"));
    assert!(watched_by_default("d-main-3", "d-main-3"));
    assert!(watched_by_default("p-w-2-1", "p-w-2-1"));
    assert!(!watched_by_default("tray", "tray"), "a tray popover");
    assert!(
        !watched_by_default("browser-main-1", "main"),
        "a second webview in a window"
    );
    assert!(
        !watched_by_default("ghost", "ghost"),
        "outside the label scheme"
    );
    let app = app_with(
        Startup::builder()
            .watch(|webview, _| webview == "w-7")
            .build()
            .unwrap(),
    );
    let other = window(&app, "w-8");
    page_load(other.as_ref(), PageLoadEvent::Started);
    web_content_terminated(other.as_ref());
    assert!(startup(&app).with(|d| d.labels()).is_empty());
    assert!(log_of("w-8").is_empty());
}

#[test]
fn commands_fail_plainly_when_the_contract_is_not_installed() {
    capture();
    let app = mock_builder()
        .invoke_handler(invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let w = window(&app, "main");
    for (cmd, body) in [
        ("window_ready", json!({})),
        ("window_pong", json!({ "seq": 1 })),
        ("ui_checkpoint", json!({ "ui": {} })),
        (
            "window_boot_error",
            json!({ "kind": "error", "message": "x" }),
        ),
        ("recovery_status", json!({})),
        ("recovery_restore", json!({})),
        ("recovery_start_fresh", json!({})),
        ("recovery_safe_mode", json!({})),
        ("app_relaunch", json!({})),
    ] {
        assert_eq!(
            invoke(&w, cmd, body).unwrap_err()["code"],
            "notInstalled",
            "{cmd}"
        );
    }
    // Hooks without the state do nothing.
    page_load(w.as_ref(), PageLoadEvent::Started);
    web_content_terminated(w.as_ref());
    assert!(start(app.handle()).is_none());
    answered(app.handle(), "main", Choice::Reload);
}

#[test]
fn the_ticker_runs_on_its_own_thread_once_and_stops() {
    let seen = Shared::default();
    let fast = Config {
        tick: Duration::from_millis(1),
        ..Config::default()
    };
    let app = app_with(builder(&seen).config(fast).build().unwrap());
    let w = window(&app, "w-9");
    // A window that is gone: the ticker forgets it, which shows that it ticks.
    startup(&app).with(|d| d.page_started("w-ticked-away", Instant::now()));
    let handle = start(app.handle()).expect("started");
    page_load(w.as_ref(), PageLoadEvent::Started);
    assert!(start(app.handle()).is_none(), "once");
    while startup(&app)
        .with(|d| d.labels())
        .contains(&"w-ticked-away".to_string())
    {
        std::thread::yield_now();
    }
    startup(&app).stop();
    handle.join().unwrap();
}

#[test]
fn errors_on_the_edges_are_logged_with_their_window() {
    capture();
    assert_eq!(super::logged::<u8, &str>(Ok(1), "w-10", "never"), Some(1));
    assert_eq!(
        super::logged::<u8, &str>(Err("gone"), "w-10", "Could not reload the window"),
        None
    );
    assert_eq!(
        log_of("w-10"),
        [(
            log::Level::Error,
            "Could not reload the window: gone".to_string()
        )]
    );
}

#[test]
fn malformed_requests_are_refused_by_the_commands_arguments() {
    let (app, _seen) = app();
    let w = window(&app, "w-11");
    assert!(invoke(&w, "window_pong", json!({})).is_err());
    assert!(invoke(&w, "window_pong", json!({ "seq": -1 })).is_err());
    assert!(invoke(&w, "window_boot_error", json!({ "kind": 1 })).is_err());
    assert!(invoke(&w, "window_boot_error", json!({ "kind": "error" })).is_err());
    assert!(invoke(&w, "ui_checkpoint", json!({})).is_err());
}

#[cfg(feature = "dialog")]
#[test]
fn the_builder_takes_the_native_dialogs() {
    assert!(
        Startup::<MockRuntime>::builder()
            .native_dialogs()
            .build()
            .is_ok()
    );
}

#[test]
fn inconsistent_timings_are_refused_when_building() {
    let bad = Config {
        tick: Duration::ZERO,
        ..Config::default()
    };
    assert!(matches!(
        Startup::<MockRuntime>::builder().config(bad).build(),
        Err(ConfigError::ZeroTick)
    ));
}

#[test]
fn every_command_is_async_and_handled() {
    paddock_commands::testing::assert_async(
        &crate::COMMANDS,
        &[std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src"
        ))],
    );
    assert_eq!(
        DIALOG_BASE
            .iter()
            .filter(|c| crate::COMMANDS.spec(c).is_some())
            .count(),
        3
    );
}

#[cfg(feature = "dialog")]
#[test]
fn the_native_asker_without_the_dialog_plugin_waits() {
    let app = app_with(Startup::builder().build().unwrap());
    let got = Arc::new(Mutex::new(None));
    let sink = got.clone();
    NativeAsker.ask(
        app.handle(),
        "w-native",
        Prompt::new(Question::Unresponsive, "App"),
        Box::new(move |c| *lock(&sink) = Some(c)),
    );
    assert_eq!(*lock(&got), Some(Choice::Wait));
    assert!(
        log_of("w-native")
            .iter()
            .any(|(_, m)| m.contains("tauri-plugin-dialog is not registered"))
    );
    assert!(
        native::message(
            app.handle(),
            "w-native",
            &Prompt::new(Question::Unresponsive, "App")
        )
        .is_none()
    );
    // With the plugin the dialog is built, owned by the window when it exists (not shown: no windows on the desktop).
    let with_plugin = mock_builder()
        .plugin(tauri_plugin_dialog::init())
        .build(mock_context(noop_assets()))
        .unwrap();
    let w = window(&with_plugin, "w-native-2");
    let prompt = Prompt::new(Question::Startup { safe_mode: false }, "App");
    assert!(native::message(with_plugin.handle(), w.label(), &prompt).is_some());
    assert!(native::message(with_plugin.handle(), "w-none", &prompt).is_some());
}

mod recovery;

#[test]
fn a_window_can_ask_for_the_whole_app_to_restart_and_is_told_when_it_cannot() {
    let (first, seen) = app();
    let w = window(&first, "w-1");
    assert_eq!(invoke(&w, "app_relaunch", json!({})).unwrap(), Value::Null);
    assert_eq!(lock(&seen).relaunch, ["w-1"]);
    assert!(
        log_of("w-1")
            .iter()
            .any(|(_, m)| m == "Restarting the app on request")
    );

    let (second, seen) = app();
    let w = window(&second, "w-2");
    let err = invoke(&w, "app_relaunch", json!({})).unwrap_err();
    assert_eq!(err["code"], "relaunchFailed");
    assert_eq!(err["message"], "no restart here");
    assert_eq!(lock(&seen).relaunch.len(), 1, "the app keeps running");
    assert!(
        log_of("w-2")
            .iter()
            .any(|(_, m)| m == "Could not restart the app: no restart here")
    );

    let bare = app_with(Startup::builder().build().unwrap());
    let w = window(&bare, "w-1");
    assert_eq!(
        invoke(&w, "app_relaunch", json!({})).unwrap_err()["code"],
        "relaunchUnavailable"
    );
}
