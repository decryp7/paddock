//! ADR 0026 package 1.1-0 spike: does a dialog window that is created hidden run JS, resolve `document.fonts.ready`, lay
//! out and fire `requestAnimationFrame` before it is shown, on WKWebView, WebView2 and WebKitGTK?
//!
//! The app opens a host window and then, one after another, a probe dialog per trial. Each trial picks a show method
//! (`a` hidden then measure after a frame, `b` visible off-screen then move in, `c` hidden then a synchronous layout read
//! after `fonts.ready`) and a scenario (fonts slow, route slow, zoom, second monitor, owned or not). The page
//! (`dist/dialog.html`) reports what it sees; Rust fits the window to the reported content height, shows it and asks the
//! page to measure again. Every observation is one JSON line in `$PROBE_OUT` (default `probe.jsonl`). The app exits when
//! the trials are done. Not product code.

use std::fs::File;
use std::io::Write;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tauri::http::Response;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// The dialog's width (logical px): the `medium` step of ADR 0026.
const WIDTH: f64 = 560.0;
/// The guessed height a window is created at before the content reports (today's 520).
const GUESS: f64 = 520.0;
/// The show deadline under test (ADR 0026 §4.3 says about 1 s); the spike waits longer to see the real time to ready.
const DEADLINE: Duration = Duration::from_millis(3000);
/// The font the probe page uses (the repository's own OFL IBM Plex Sans build), served with an optional delay.
const FONT: &[u8] = include_bytes!("../../../apps/debrief/crates/debrief-pdf/fonts/DebriefReportSans-Regular.ttf");

/// A message from a probe page that the trial runner waits for.
enum Msg {
    Ready { label: String, h: f64, dpr: f64 },
    AfterShow { label: String },
    Settle { label: String },
}

/// The log file, the current trial's start and the channel to the runner.
struct Probe {
    out: Mutex<File>,
    start: Mutex<(Instant, Value)>,
    tx: Mutex<Sender<Msg>>,
}

impl Probe {
    fn log(&self, ev: &str, label: &str, data: Value) {
        let (t0, trial) = match self.start.lock() {
            Ok(g) => (g.0, g.1.clone()),
            Err(p) => (p.get_ref().0, p.get_ref().1.clone()),
        };
        let line = json!({ "t_ms": t0.elapsed().as_secs_f64() * 1000.0, "ev": ev, "label": label, "trial": trial, "data": data });
        if let Ok(mut f) = self.out.lock() {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// The page's report: every observation is logged; ready, after-show and settle also go to the runner.
#[tauri::command]
fn probe(window: WebviewWindow, state: tauri::State<'_, Probe>, ev: String, data: Value) {
    let label = window.label().to_string();
    state.log(&format!("js:{ev}"), &label, data.clone());
    let msg = match ev.as_str() {
        "ready" => Some(Msg::Ready {
            label,
            h: data.get("h").and_then(Value::as_f64).unwrap_or(GUESS),
            dpr: data.get("dpr").and_then(Value::as_f64).unwrap_or(1.0),
        }),
        "after_show" => Some(Msg::AfterShow { label }),
        "settle" => Some(Msg::Settle { label }),
        _ => None,
    };
    if let (Some(msg), Ok(tx)) = (msg, state.tx.lock()) {
        let _ = tx.send(msg);
    }
}

/// One trial: a show method and a scenario.
#[derive(Clone)]
struct Trial {
    method: &'static str,
    scenario: &'static str,
    rep: u32,
}

impl Trial {
    fn zoom(&self) -> f64 {
        match self.scenario {
            "zoom125" => 1.25,
            "zoom150" => 1.5,
            _ => 1.0,
        }
    }
    fn owned(&self) -> bool {
        self.scenario != "unowned"
    }
    fn info(&self) -> Value {
        json!({ "method": self.method, "scenario": self.scenario, "rep": self.rep })
    }
}

fn trials(monitors: usize) -> Vec<Trial> {
    let reps: u32 = std::env::var("PROBE_REPS").ok().and_then(|s| s.parse().ok()).unwrap_or(3);
    let mut scenarios = vec!["normal", "font_slow", "route_slow", "zoom125", "zoom150", "unowned"];
    if monitors > 1 {
        scenarios.push("monitor2");
    }
    let mut out = Vec::new();
    for rep in 0..reps {
        for scenario in &scenarios {
            for method in ["a", "b", "c"] {
                out.push(Trial { method, scenario, rep });
            }
        }
    }
    out
}

/// Waits for the runner message that matches `want` from window `label`, until `timeout`.
/// Ready carries the content height in CSS px and the page's devicePixelRatio.
fn wait_for(rx: &Receiver<Msg>, label: &str, want: &str, timeout: Duration) -> Option<(f64, f64)> {
    let until = Instant::now() + timeout;
    loop {
        let left = until.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(Msg::Ready { label: l, h, dpr }) if l == label && want == "ready" => return Some((h, dpr)),
            Ok(Msg::AfterShow { label: l }) if l == label && want == "after_show" => return Some((0.0, 1.0)),
            Ok(Msg::Settle { label: l }) if l == label && want == "settle" => return Some((0.0, 1.0)),
            Ok(_) => continue,
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => return None,
        }
    }
}

/// The logical centre position of a `WIDTH` x `h` window on monitor `index` (the last one if out of range).
fn centred(monitors: &[tauri::Monitor], index: usize, h: f64) -> (f64, f64) {
    let Some(m) = monitors.get(index.min(monitors.len().saturating_sub(1))) else {
        return (100.0, 100.0);
    };
    let s = m.scale_factor();
    let pos = m.work_area().position.to_logical::<f64>(s);
    let size = m.work_area().size.to_logical::<f64>(s);
    (pos.x + (size.width - WIDTH) / 2.0, pos.y + (size.height - h) / 2.0)
}

fn window_facts(w: &WebviewWindow) -> Value {
    let scale = w.scale_factor().unwrap_or(1.0);
    let inner = w.inner_size().map(|s| s.to_logical::<f64>(scale)).ok();
    let outer = w.outer_size().map(|s| s.to_logical::<f64>(scale)).ok();
    let pos = w.outer_position().map(|p| p.to_logical::<f64>(scale)).ok();
    json!({
        "scale": scale,
        "inner": inner.map(|s| [s.width, s.height]),
        "inner_phys": w.inner_size().ok().map(|s| [s.width, s.height]),
        "outer": outer.map(|s| [s.width, s.height]),
        "pos": pos.map(|p| [p.x, p.y]),
        "visible": w.is_visible().ok(),
        "monitor": w.current_monitor().ok().flatten().and_then(|m| m.name().cloned()),
    })
}

/// `monitors` is read on the main thread: on macOS `available_monitors` from another thread returns none.
fn run_trials(app: AppHandle, rx: Receiver<Msg>, monitors: Vec<tauri::Monitor>) {
    let probe = app.state::<Probe>();
    let list: Vec<Value> = monitors
        .iter()
        .map(|m| json!({ "name": m.name(), "scale": m.scale_factor(), "pos": [m.position().x, m.position().y], "size": [m.size().width, m.size().height], "work": [m.work_area().size.width, m.work_area().size.height] }))
        .collect();
    probe.log("monitors", "", json!(list));
    let Some(host) = app.get_webview_window("main") else { return };
    for (n, trial) in trials(monitors.len()).into_iter().enumerate() {
        if let Ok(mut g) = probe.start.lock() {
            *g = (Instant::now(), trial.info());
        }
        let label = format!("d-main-{n}");
        let monitor = if trial.scenario == "monitor2" {
            monitors.len().saturating_sub(1)
        } else {
            0
        };
        let (x, y) = centred(&monitors, monitor, GUESS);
        let url = format!("dialog.html?method={}&scenario={}", trial.method, trial.scenario);
        let mut b = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(url.clone().into()))
            .title("Probe dialog")
            .inner_size(WIDTH, GUESS)
            .resizable(true)
            .focused(false)
            .background_color(tauri::window::Color(27, 30, 35, 255));
        b = if trial.method == "b" {
            b.position(-30000.0, -30000.0).visible(true)
        } else {
            b.position(x, y).visible(false)
        };
        if trial.owned() {
            b = match b.parent(&host) {
                Ok(b) => b,
                Err(e) => {
                    probe.log("parent_error", &label, json!(e.to_string()));
                    return;
                }
            };
        }
        probe.log("create", &label, json!({ "url": url }));
        let w = match b.build() {
            Ok(w) => w,
            Err(e) => {
                probe.log("build_error", &label, json!(e.to_string()));
                continue;
            }
        };
        probe.log("built", &label, window_facts(&w));
        if trial.zoom() != 1.0 {
            let _ = w.set_zoom(trial.zoom());
        }
        let mut ready = wait_for(&rx, &label, "ready", DEADLINE);
        if ready.is_none() {
            probe.log("deadline", &label, window_facts(&w));
            let _ = w.show();
            let _ = w.set_focus();
            probe.log("shown_at_deadline", &label, window_facts(&w));
            ready = wait_for(&rx, &label, "ready", DEADLINE);
            probe.log(if ready.is_some() { "late_ready" } else { "never_ready" }, &label, json!(null));
        }
        if let Some((css_h, dpr)) = ready {
            probe.log("pre_show", &label, window_facts(&w));
            // CSS px are logical px times the webview zoom (dpr / scale); a zoomed page reports a smaller CSS height.
            let scale = w.scale_factor().unwrap_or(1.0);
            let h = (css_h * dpr / scale).ceil();
            let _ = w.set_size(LogicalSize::new(WIDTH, h));
            probe.log("sized", &label, window_facts(&w));
            let (x, y) = centred(&monitors, monitor, h);
            if trial.method == "b" {
                let _ = w.set_position(LogicalPosition::new(x, y));
            }
            let _ = w.show();
            // A dialog opens frontmost with focus, as the product's does (an occluded WKWebView pauses frames).
            let _ = w.set_focus();
            probe.log("shown", &label, window_facts(&w));
            let _ = app.emit("probe-shown", label.clone());
            if wait_for(&rx, &label, "after_show", DEADLINE).is_none() {
                probe.log("no_after_show", &label, json!(null));
            }
            if wait_for(&rx, &label, "settle", DEADLINE).is_none() {
                probe.log("no_settle", &label, json!(null));
            }
            probe.log("settled", &label, window_facts(&w));
        }
        let _ = w.destroy();
        let closed = Instant::now();
        while app.get_webview_window(&label).is_some() && closed.elapsed() < Duration::from_secs(3) {
            std::thread::yield_now();
        }
    }
    probe.log("done", "", json!(null));
    app.exit(0);
}

fn main() {
    let path = std::env::var("PROBE_OUT").unwrap_or_else(|_| "probe.jsonl".into());
    let out = match File::create(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cannot write {path}: {e}");
            std::process::exit(2);
        }
    };
    let (tx, rx) = mpsc::channel::<Msg>();
    let result = tauri::Builder::default()
        .manage(Probe {
            out: Mutex::new(out),
            start: Mutex::new((Instant::now(), json!(null))),
            tx: Mutex::new(tx),
        })
        // `slow://localhost/font?d=<ms>` (Windows: `http://slow.localhost/font?d=<ms>`): the probe font after `d` ms.
        .register_asynchronous_uri_scheme_protocol("slow", |_ctx, req, responder| {
            let delay = req
                .uri()
                .query()
                .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("d=")))
                .and_then(|d| d.parse::<u64>().ok())
                .unwrap_or(0);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(delay));
                let resp = Response::builder()
                    .header("Content-Type", "font/ttf")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(FONT.to_vec());
                if let Ok(resp) = resp {
                    responder.respond(resp);
                }
            });
        })
        .invoke_handler(tauri::generate_handler![probe])
        .setup(move |app| {
            let host = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Probe host")
                .inner_size(480.0, 320.0)
                .position(40.0, 60.0)
                .focused(false)
                .build()?;
            app.state::<Probe>().log("host", "main", window_facts(&host));
            let handle = app.handle().clone();
            let monitors = app.available_monitors().unwrap_or_default();
            std::thread::spawn(move || run_trials(handle, rx, monitors));
            Ok(())
        })
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("spike failed: {e}");
        std::process::exit(1);
    }
}
