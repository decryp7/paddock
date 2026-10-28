//! The ADR 0026 §6 real-window probe: opens a real dialog window (hidden), lets its page measure itself synchronously and
//! send `dialog_ready`, and checks that the window was shown by that ready and not by the deadline, and that the page's
//! viewport is exactly the measured height after the show (no cut-off footer, no scroll bar: the 1.1-0 spike's two
//! failures). It needs the real webview and a window system, so the ordinary test run skips it: it exits 0 at once unless
//! `PADDOCK_REAL_WINDOWS=1`. The full suite sets that on each OS (macOS, Windows, Linux with a display or Xvfb).
//!
//! Run it: `PADDOCK_REAL_WINDOWS=1 cargo test -p paddock-dialogs --all-features --test show_probe`.
//! A Tauri app owns the main thread, hence `harness = false`; the result is the exit code (0 pass, 1 failed, 2 timed out).

use paddock_dialogs::tauri::WindowHooks;
use paddock_dialogs::{Contracts, DialogRequest, Dialogs, Entry, Shape};
use std::borrow::Cow;
use std::sync::mpsc;
use std::time::Duration;
use tauri::test::mock_context;
use tauri::utils::assets::{AssetKey, AssetsIter, CspHash};
use tauri::{Assets, Manager, Runtime, WebviewUrl, WebviewWindowBuilder, Wry};

/// The measured content height (CSS px) of the probe page's frame.
const CONTENT_HEIGHT: f64 = 240.0;
/// Far longer than the probe waits: only the page's `dialog_ready` can show the window in time.
const DEADLINE: Duration = Duration::from_secs(300);
/// The probe gives up (exit 2) after this: a hung webview must fail the run, not hold it.
const GIVE_UP: Duration = Duration::from_secs(90);

const PAGE: &str = r#"<!doctype html><html><head><meta charset="utf-8"><style>
html,body{margin:0;padding:0}#frame{width:420px;height:240px;background:#888}</style></head>
<body><div id="frame"></div><script>
(async () => {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  await document.fonts.ready;
  // Synchronous: no frame is waited for, since a hidden window runs none on every OS (spike §2).
  const h = document.getElementById('frame').getBoundingClientRect().height;
  await invoke('dialog_ready', { measure: {
    content: { width: 420, height: h }, minContent: { width: 300, height: 100 }, viewportHeight: window.innerHeight } });
  // Shown and sized now: the first frame after that tells what the user sees.
  requestAnimationFrame(() => requestAnimationFrame(() => invoke('probe_report', {
    frame: document.getElementById('frame').getBoundingClientRect().height, inner: window.innerHeight,
    scroll: document.documentElement.scrollHeight })));
})();
</script></body></html>"#;

struct ProbeAssets;

impl<R: Runtime> Assets<R> for ProbeAssets {
    fn get(&self, key: &AssetKey) -> Option<Cow<'_, [u8]>> {
        let path: &str = key.as_ref();
        (path == "/index.html" || path == "index.html").then(|| Cow::Borrowed(PAGE.as_bytes()))
    }
    fn iter(&self) -> Box<AssetsIter<'_>> {
        Box::new(std::iter::empty())
    }
    fn csp_hashes(&self, _: &AssetKey) -> Box<dyn Iterator<Item = CspHash<'_>> + '_> {
        Box::new(std::iter::empty())
    }
}

paddock_commands::command_table! {
    static TABLE, macro handler;
    Mutation [MAIN | DIALOG] limit 9 * paddock_commands::MIB => paddock_dialogs::tauri::dialog_open,
    Query [DIALOG] => paddock_dialogs::tauri::dialog_take_input,
    Mutation [DIALOG] limit 2 * paddock_commands::MIB => paddock_dialogs::tauri::dialog_close,
    Quiet [DIALOG] => paddock_dialogs::tauri::dialog_ready,
    Quiet [DIALOG] => paddock_dialogs::tauri::dialog_fit,
}

#[derive(serde::Deserialize, Debug)]
struct Report {
    frame: f64,
    inner: f64,
    scroll: f64,
}

fn main() {
    if std::env::var("PADDOCK_REAL_WINDOWS").as_deref() != Ok("1") {
        eprintln!("show_probe: skipped (set PADDOCK_REAL_WINDOWS=1 to open a real window)");
        return;
    }
    let (report_tx, report_rx) = mpsc::channel::<Report>();
    let guard =
        paddock_commands::tauri::Guard::new(&TABLE).dialog_grants(paddock_dialogs::tauri::grant_of);
    let guarded = guard.wrap(handler!());
    let report_tx = std::sync::Mutex::new(report_tx);
    let app = tauri::Builder::<Wry>::new()
        .invoke_handler(move |invoke| {
            if invoke.message.command() == "probe_report" {
                if let tauri::ipc::InvokeBody::Json(v) = invoke.message.payload()
                    && let Ok(r) = serde_json::from_value::<Report>(v.clone())
                {
                    let _ = report_tx.lock().unwrap().send(r);
                }
                invoke.resolver.resolve(());
                true
            } else {
                guarded(invoke)
            }
        })
        .build(mock_context(ProbeAssets))
        .expect("the real-runtime app builds");
    let form = Shape::Form(paddock_dialogs::Modality::Modal);
    let contracts = Contracts::from_entries(&[Entry::new("probe", form)], &[]).expect("contract");
    app.manage(Dialogs::new(std::sync::Arc::new(contracts)).with_show_deadline(DEADLINE));
    app.manage(WindowHooks::<Wry>::new());
    // The host window stays hidden: the probe disturbs nobody's desktop more than the dialog itself does.
    WebviewWindowBuilder::new(&app, "main", WebviewUrl::App("index.html".into()))
        .visible(false)
        .build()
        .expect("host window");

    let (done_tx, done_rx) = mpsc::channel::<()>();
    let handle = app.handle().clone();
    // Only a failure path: the probe is event-driven, this just stops a hung webview from holding the run.
    let watchdog_handle = handle.clone();
    std::thread::spawn(move || {
        if done_rx.recv_timeout(GIVE_UP).is_err() {
            eprintln!(
                "show_probe: no report within {GIVE_UP:?}: the dialog page never became ready"
            );
            watchdog_handle.exit(2);
        }
    });
    std::thread::spawn({
        let handle = handle.clone();
        move || {
            let code = match report_rx.recv() {
                Ok(r) => check(&handle, &r),
                Err(_) => 1,
            };
            let _ = done_tx.send(());
            handle.exit(code);
        }
    });

    app.run(move |app, event| {
        if let tauri::RunEvent::Ready = event {
            let request = DialogRequest {
                dialog_id: "probe".into(),
                instance: None,
                title: "Probe".into(),
                input: None,
                remembered: None,
                theme: Some(paddock_dialogs::Theme::Dark),
            };
            if let Err(e) = paddock_dialogs::tauri::open(app, "main", request) {
                eprintln!("show_probe: the dialog did not open: {e:?}");
                app.exit(1);
            }
        }
    });
}

/// 0 when the probe's expectations hold.
fn check(app: &tauri::AppHandle<Wry>, r: &Report) -> i32 {
    let dialogs = app.state::<Dialogs>();
    let Some(label) = dialogs.of_host("main").into_iter().next() else {
        eprintln!("show_probe: the dialog is gone");
        return 1;
    };
    let entry = dialogs.get(&label).expect("open dialog");
    let mut failures = Vec::new();
    // Shown by the page's ready (the deadline is minutes away), and ready measured the frame.
    if !entry.shown {
        failures.push("the window was not shown".to_string());
    }
    match entry.measure {
        Some(m) if (m.content.height - CONTENT_HEIGHT).abs() < 0.5 => {}
        other => failures.push(format!(
            "ready did not record the {CONTENT_HEIGHT} px measure: {other:?}"
        )),
    }
    // Exact after the show: the frame is its measured height and the viewport equals it (nothing cut off, no scroll).
    if (r.frame - CONTENT_HEIGHT).abs() >= 0.5 {
        failures.push(format!(
            "the frame is {} px after the show, measured {CONTENT_HEIGHT}",
            r.frame
        ));
    }
    if (r.inner - r.frame).abs() >= 1.0 {
        failures.push(format!(
            "the viewport is {} px for a frame of {} px (a viewport delta)",
            r.inner, r.frame
        ));
    }
    if r.scroll - r.inner > 1.0 {
        failures.push(format!(
            "the page scrolls: {} px of content in {} px",
            r.scroll, r.inner
        ));
    }
    if failures.is_empty() {
        eprintln!("show_probe: ok ({r:?})");
        0
    } else {
        for f in &failures {
            eprintln!("show_probe: FAILED: {f}");
        }
        1
    }
}
