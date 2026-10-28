//! The ADR 0026 §6 real-window probe: opens the REAL dialog frame (`DialogShell` of the built shell-demo, route
//! `index.html#/dialog?id=sd.rename&...`, the URL Rust loads) in a real hidden window, lets it measure itself and send
//! `dialog_ready`, and checks that the window was shown by that ready and not by the deadline, that the page's viewport is
//! exactly the measured content height after the show (no cut-off footer, no scroll bar: the 1.1-0 spike's two failures)
//! and that the footer is fully visible. It needs the real webview, a window system and the built UI, so the ordinary
//! test run skips it: it exits 0 at once unless `PADDOCK_REAL_WINDOWS=1`, and with a message unless `PADDOCK_PROBE_DIST`
//! names the built shell-demo (`npm run build -w ui` then `npx ng build shell-demo` in `ui`: `ui/dist/shell-demo/browser`).
//! The workflow job `dialog-window-probe` (manual runs, or opt-in with the variable `PADDOCK_REAL_WINDOW_PROBE`) sets
//! both. The parent is a visible host window and the dialog is modal; the probe also asserts that the dialog window is
//! still hidden when its page's `dialog_ready` reaches Rust (it is shown by that ready, not before).
//!
//! Run it (from `crates`; `--features tauri`, NOT `--all-features`, which pulls in the mock runtime):
//! `PADDOCK_REAL_WINDOWS=1 PADDOCK_PROBE_DIST=../ui/dist/shell-demo/browser cargo test -p paddock-dialogs
//! --features tauri --test show_probe -- --nocapture` (an absolute `PADDOCK_PROBE_DIST` works from any folder). The
//! probe stands in for the app's shell plugin: the page sends the shell's commands (`dialog_take_input`, `dialog_ready`, ...)
//! as `plugin:paddock-shell|<command>`, so the probe registers a plugin of that name and an ACL that lets the `d-*`
//! windows call it (`probe_context`, built from the app's real `paddock-shell:dialog` capability and the plugin's
//! generated permission set); without either Tauri refuses the calls before the probe sees them, the page never
//! gets its input and the run ends "no report within 90s" (2026-10-29: a harness gap, not a product fault). Every call
//! the page makes is printed with its time (`show_probe: <ms> ms  <command> from <label>`). A Tauri app owns the main thread, hence `harness = false`; the result is the exit
//! code (0 pass, 1 failed, 2 timed out).

use paddock_dialogs::tauri::WindowHooks;
use paddock_dialogs::{Contracts, DialogRequest, Dialogs, Entry, Shape, Width};
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;
use tauri::test::mock_context;
use tauri::utils::assets::{AssetKey, AssetsIter, CspHash};
use tauri::{Assets, Listener, Manager, Runtime, WebviewUrl, WebviewWindowBuilder, Wry};

/// Far longer than the probe waits: only the page's `dialog_ready` can show the window in time.
const DEADLINE: Duration = Duration::from_secs(300);
/// The probe gives up (exit 2) after this: a hung webview must fail the run, not hold it.
const GIVE_UP: Duration = Duration::from_secs(90);

/// The hidden host window's page: the dialog's parent needs none of the demo.
const HOST: &str = "<!doctype html><html><head><meta charset=\"utf-8\"></head><body></body></html>";

/// Sent to the shown dialog window: what the user sees, read at once and once more from a short timer. Not from
/// animation frames: a sleeping display runs none, and the report must still come (ER-12: it waits on the event).
const REPORT_SCRIPT: &str = r#"(() => {
  const send = (phase) => {
    const f = document.querySelector('[data-testid=dialog-footer]');
    const r = f ? f.getBoundingClientRect() : null;
    window.__TAURI_INTERNALS__.invoke('probe_report', {
      phase, inner: window.innerHeight, scroll: document.scrollingElement.scrollHeight,
      footerBottom: r ? r.bottom : -1, footerHeight: r ? r.height : 0 });
  };
  send('now');
  setTimeout(() => send('timer'), 50);
})();"#;

/// The built shell-demo, served from disk as the app's assets.
struct DistAssets {
    dist: PathBuf,
}

impl<R: Runtime> Assets<R> for DistAssets {
    fn get(&self, key: &AssetKey) -> Option<Cow<'_, [u8]>> {
        let path: &str = key.as_ref();
        let path = path.trim_start_matches('/');
        if path == "host.html" {
            return Some(Cow::Borrowed(HOST.as_bytes()));
        }
        let path = if path.is_empty() { "index.html" } else { path };
        if path.split('/').any(|c| c == ".." || c.contains('\\')) {
            return None;
        }
        std::fs::read(self.dist.join(path)).ok().map(Cow::Owned)
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
#[serde(rename_all = "camelCase")]
struct Report {
    phase: String,
    inner: f64,
    scroll: f64,
    footer_bottom: f64,
    footer_height: f64,
}

/// The shell plugin's name, as the page's `shellInvoke` addresses it.
const SHELL_PLUGIN: &str = "paddock-shell";

/// The app's dialog-window capability and the shell plugin's generated permission sets: the probe grants exactly what they
/// grant, so a missing or wrong capability (a command left out of the `dialog` set, the set left out of the capability, a
/// window pattern that does not match `d-*`) fails the probe the way it would fail an app's dialogs.
const DIALOG_CAPABILITY: &str =
    include_str!("../../../apps/shell-demo/src-tauri/capabilities/dialog.json");
const WINDOW_CLASSES: &str = include_str!("../../paddock-shell/permissions/window-classes.toml");

/// The commands of the shell plugin's `dialog` permission set (`allow-dialog-take-input` is `dialog_take_input`).
fn dialog_set_commands() -> Vec<String> {
    let mut in_set = false;
    let mut in_list = false;
    let mut commands = Vec::new();
    for line in WINDOW_CLASSES.lines().map(str::trim) {
        if line == "[[set]]" {
            in_set = false;
            in_list = false;
        } else if line == "identifier = \"dialog\"" {
            in_set = true;
        } else if in_set && line.starts_with("permissions = [") {
            in_list = true;
        } else if in_set && in_list && line == "]" {
            break;
        } else if in_set && in_list {
            let id = line.trim_end_matches(',').trim_matches('"');
            commands.push(
                id.strip_prefix("allow-")
                    .expect("an allow- permission")
                    .replace('-', "_"),
            );
        }
    }
    assert!(
        !commands.is_empty(),
        "the shell plugin has no `dialog` permission set"
    );
    commands
}

/// The mock context with an ACL built from the real `paddock-shell:dialog` capability. Without a plugin and an ACL Tauri
/// refuses every `plugin:paddock-shell|...` call ("Plugin not found") before the probe's handler sees it, and the page
/// never gets its input or sends its ready. (`core:event:default`, which the capability also grants, is not modelled: the
/// page's event listening is not part of what the probe asserts.)
fn probe_context<A: Assets<Wry>>(assets: A) -> tauri::Context<Wry> {
    use tauri::utils::acl::manifest::Manifest;
    use tauri::utils::acl::resolved::{Resolved, ResolvedCommand};
    let capability: serde_json::Value =
        serde_json::from_str(DIALOG_CAPABILITY).expect("the dialog capability is JSON");
    let grants_set = capability["permissions"]
        .as_array()
        .is_some_and(|p| p.iter().any(|x| x == "paddock-shell:dialog"));
    assert!(
        grants_set,
        "the dialog capability does not grant paddock-shell:dialog"
    );
    let windows: Vec<glob::Pattern> = capability["windows"]
        .as_array()
        .expect("the capability names its windows")
        .iter()
        .map(|w| {
            glob::Pattern::new(w.as_str().expect("a window pattern"))
                .expect("a valid label pattern")
        })
        .collect();
    let allowed = dialog_set_commands()
        .into_iter()
        .map(|c| {
            let cmd = ResolvedCommand {
                windows: windows.clone(),
                webviews: windows.clone(),
                ..Default::default()
            };
            (format!("plugin:{SHELL_PLUGIN}|{c}"), vec![cmd])
        })
        .collect();
    let manifest = Manifest {
        default_permission: None,
        permissions: Default::default(),
        permission_sets: Default::default(),
        global_scope_schema: None,
    };
    let mut context = mock_context(assets);
    *context.runtime_authority_mut() = tauri::ipc::RuntimeAuthority::new(
        [(SHELL_PLUGIN.to_string(), manifest)].into_iter().collect(),
        Resolved {
            allowed_commands: allowed,
            ..Default::default()
        },
    );
    context
}

/// The calls the page made, with the milliseconds since the probe started: printed as it goes (the per-step timings of a
/// good run) and again when the probe gives up, so a page whose calls are refused or never made shows it.
struct Trace {
    start: std::time::Instant,
    calls: std::sync::Mutex<Vec<String>>,
}

impl Default for Trace {
    fn default() -> Self {
        Self {
            start: std::time::Instant::now(),
            calls: Default::default(),
        }
    }
}

impl Trace {
    fn note(&self, what: &str) {
        let line = format!("{:>5} ms  {what}", self.start.elapsed().as_millis());
        eprintln!("show_probe: {line}");
        self.calls.lock().unwrap().push(line);
    }
    fn seen(&self) -> String {
        let calls = self.calls.lock().unwrap();
        if calls.is_empty() {
            "none".into()
        } else {
            calls.join("; ")
        }
    }
}

/// The built shell-demo's folder, or why the probe does not run.
fn dist() -> Result<PathBuf, String> {
    let dist = std::env::var_os("PADDOCK_PROBE_DIST").map(PathBuf::from);
    match dist {
        Some(d) if d.join("index.html").is_file() => Ok(d),
        Some(d) => Err(format!("PADDOCK_PROBE_DIST={} has no index.html; build the shell-demo first", d.display())),
        None => Err("PADDOCK_PROBE_DIST is not set; point it at the built shell-demo (ui/dist/shell-demo/browser)".into()),
    }
}

fn main() {
    if std::env::var("PADDOCK_REAL_WINDOWS").as_deref() != Ok("1") {
        eprintln!("show_probe: skipped (set PADDOCK_REAL_WINDOWS=1 to open a real window)");
        return;
    }
    let dist = match dist() {
        Ok(d) => d,
        Err(why) => {
            eprintln!("show_probe: skipped: {why}");
            return;
        }
    };
    let (report_tx, report_rx) = mpsc::channel::<Report>();
    let guard =
        paddock_commands::tauri::Guard::new(&TABLE).dialog_grants(paddock_dialogs::tauri::grant_of);
    let guarded = guard.wrap(handler!());
    let report_tx = std::sync::Mutex::new(report_tx);
    // Whether the dialog window was still hidden when its page's `dialog_ready` arrived (checked before it is handled).
    let hidden_at_ready = Arc::new(AtomicBool::new(false));
    let saw_ready = Arc::new(AtomicBool::new(false));
    let (hidden_flag, ready_flag) = (hidden_at_ready.clone(), saw_ready.clone());
    let trace = Arc::new(Trace::default());
    let watchdog_trace = trace.clone();
    let on_invoke = Arc::new(move |invoke: tauri::ipc::Invoke<Wry>| -> bool {
        trace.note(&format!(
            "{} from {}",
            invoke.message.command(),
            invoke.message.webview().label()
        ));
        match invoke.message.command() {
            "probe_report" => {
                if let tauri::ipc::InvokeBody::Json(v) = invoke.message.payload()
                    && let Ok(r) = serde_json::from_value::<Report>(v.clone())
                {
                    let _ = report_tx.lock().unwrap().send(r);
                }
                invoke.resolver.resolve(());
                true
            }
            // The shell plugin's start-up handshake (every dialog window's base commands): nothing to answer here.
            "window_pong" | "window_ready" => {
                invoke.resolver.resolve(());
                true
            }
            "dialog_ready" => {
                let hidden = invoke
                    .message
                    .webview()
                    .window()
                    .is_visible()
                    .map(|v| !v)
                    .unwrap_or(false);
                hidden_flag.store(hidden, Ordering::SeqCst);
                ready_flag.store(true, Ordering::SeqCst);
                guarded(invoke)
            }
            _ => guarded(invoke),
        }
    });
    // The page sends the shell's commands (`dialog_*` among them) as `plugin:paddock-shell|<command>` (`shellInvoke`),
    // which only a plugin of that name receives; the probe's own `probe_*` calls go to the app.
    let plugin_invoke = on_invoke.clone();
    let app_invoke = on_invoke.clone();
    let app = tauri::Builder::<Wry>::new()
        .plugin(
            tauri::plugin::Builder::<Wry, ()>::new("paddock-shell")
                .invoke_handler(move |invoke| plugin_invoke(invoke))
                .build(),
        )
        .invoke_handler(move |invoke| app_invoke(invoke))
        .build(probe_context(DistAssets { dist }))
        .expect("the real-runtime app builds");
    let rename = Entry::new("sd.rename", Shape::Form(paddock_dialogs::Modality::Modal));
    let rename = Entry {
        width: Some(Width::Narrow),
        ..rename
    };
    let contracts = Contracts::from_entries(&[rename], &[]).expect("contract");
    app.manage(Dialogs::new(std::sync::Arc::new(contracts)).with_show_deadline(DEADLINE));
    app.manage(WindowHooks::<Wry>::new());
    // A visible host window: the modal's parent, as in an app.
    WebviewWindowBuilder::new(&app, "main", WebviewUrl::App("host.html".into()))
        .visible(true)
        .build()
        .expect("host window");

    // When the dialog is shown, ask its page what it looks like after two frames.
    let handle = app.handle().clone();
    app.listen_any(paddock_dialogs::SHOWN_EVENT, {
        let handle = handle.clone();
        move |_| {
            let label = handle.state::<Dialogs>().of_host("main").into_iter().next();
            if let Some(w) = label.and_then(|l| handle.get_webview_window(&l)) {
                let _ = w.eval(REPORT_SCRIPT);
            }
        }
    });

    let (done_tx, done_rx) = mpsc::channel::<()>();
    // Only a failure path: the probe is event-driven, this just stops a hung webview from holding the run.
    let watchdog_handle = handle.clone();
    std::thread::spawn(move || {
        if done_rx.recv_timeout(GIVE_UP).is_err() {
            eprintln!(
                "show_probe: no report within {GIVE_UP:?}: the dialog page never became ready; calls seen: {}",
                watchdog_trace.seen()
            );
            watchdog_handle.exit(2);
        }
    });
    std::thread::spawn({
        let handle = handle.clone();
        move || {
            // The first report (read at once) decides when it holds; if not, the second (from the timer) gets a say:
            // the layout of a just-shown window can take a moment, and neither is awaited by sleeping.
            let code = match report_rx.recv() {
                Ok(first) => {
                    let mut failures = check(&handle, &first, &hidden_at_ready, &saw_ready);
                    if !failures.is_empty()
                        && let Ok(second) = report_rx.recv()
                    {
                        failures = check(&handle, &second, &hidden_at_ready, &saw_ready);
                    }
                    finish(&failures)
                }
                Err(_) => 1,
            };
            let _ = done_tx.send(());
            handle.exit(code);
        }
    });

    app.run(move |app, event| {
        if let tauri::RunEvent::Ready = event {
            let request = DialogRequest {
                dialog_id: "sd.rename".into(),
                instance: None,
                title: "Rename".into(),
                input: Some(serde_json::json!({ "current": "Shell demo" })),
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

/// What the report shows is wrong (empty: the probe's expectations hold).
fn check(
    app: &tauri::AppHandle<Wry>,
    r: &Report,
    hidden_at_ready: &AtomicBool,
    saw_ready: &AtomicBool,
) -> Vec<String> {
    let dialogs = app.state::<Dialogs>();
    let Some(label) = dialogs.of_host("main").into_iter().next() else {
        return vec!["the dialog is gone".into()];
    };
    let entry = dialogs.get(&label).expect("open dialog");
    let mut failures = Vec::new();
    if !saw_ready.load(Ordering::SeqCst) {
        failures.push("the page never sent dialog_ready".to_string());
    } else if !hidden_at_ready.load(Ordering::SeqCst) {
        failures
            .push("the dialog window was already visible when dialog_ready arrived".to_string());
    }
    // Shown by the page's ready (the deadline is minutes away), and ready recorded the frame's measure.
    if !entry.shown {
        failures.push("the window was not shown".to_string());
    }
    let Some(measured) = entry.measure.map(|m| m.content.height) else {
        failures.push("ready did not record a measure".to_string());
        return failures;
    };
    // Exact after the show: the viewport is the measured content height (nothing cut off, no scroll) and the whole
    // footer is inside it.
    if (r.inner - measured).abs() >= 1.0 {
        failures.push(format!(
            "the viewport is {} px for a measured frame of {measured} px (a viewport delta)",
            r.inner
        ));
    }
    if r.scroll - r.inner > 1.0 {
        failures.push(format!(
            "the page scrolls: {} px of content in {} px",
            r.scroll, r.inner
        ));
    }
    if r.footer_height <= 0.0 || r.footer_bottom > r.inner + 0.5 {
        failures.push(format!(
            "the footer is cut off: it is {} px tall and ends at {} px of a {} px viewport",
            r.footer_height, r.footer_bottom, r.inner
        ));
    }
    if failures.is_empty() {
        eprintln!(
            "show_probe: ok on the {} report ({r:?}, measured {measured} px)",
            r.phase
        );
    }
    failures
}

/// The exit code for the failures found.
fn finish(failures: &[String]) -> i32 {
    for f in failures {
        eprintln!("show_probe: FAILED: {f}");
    }
    i32::from(!failures.is_empty())
}
