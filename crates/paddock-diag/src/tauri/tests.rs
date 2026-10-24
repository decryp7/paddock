//! The adapter on Tauri's mock runtime: no window ever reaches the desktop, the save picker is a fake, files live in
//! temporary folders, and waiting is event-driven (bounded), never a sleep.

use super::*;
use crate::{Content, Context, SectionFile, SectionOutput};
use ::tauri::ipc::{CallbackFn, InvokeBody};
use ::tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use ::tauri::webview::InvokeRequest;
use ::tauri::{Listener, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use paddock_env::{AppId, MapVars, Os};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc;
use std::time::Duration;

type App = ::tauri::App<MockRuntime>;

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

/// Counts how often it was asked to collect, and contributes one file.
struct Counting(Arc<AtomicUsize>);

impl Section for Counting {
    fn id(&self) -> &str {
        "counting"
    }

    fn collect(&self, _: &Context<'_>) -> SectionOutput {
        self.0.fetch_add(1, Ordering::SeqCst);
        SectionOutput::one(SectionFile::app(
            "extra/info.txt",
            "A file of the app",
            Content::Text("hello from alex".into()),
        ))
    }
}

/// What the fake dialog does.
enum Pick {
    /// Choose this file.
    File(PathBuf),
    /// Close the dialog.
    Dismiss,
    /// Press Cancel on the job while the dialog is open, then choose this file.
    CancelThen(PathBuf, Arc<paddock_jobs::JobRegistry>),
    /// Wait for the test's go, then choose.
    Wait(Mutex<mpsc::Receiver<()>>, PathBuf),
}

struct FakePicker(Pick, Mutex<Vec<(String, String)>>);

impl SavePicker for FakePicker {
    fn pick(&self, window: &str, suggested: &str) -> Option<PathBuf> {
        lock(&self.1).push((window.into(), suggested.into()));
        match &self.0 {
            Pick::File(p) => Some(p.clone()),
            Pick::Dismiss => None,
            Pick::CancelThen(p, jobs) => {
                let id = jobs
                    .snapshots()
                    .last()
                    .map(|s| s.id)
                    .expect("the save job is registered before the dialog");
                assert!(jobs.cancel(id));
                Some(p.clone())
            }
            Pick::Wait(rx, p) => {
                let _ = lock(rx).recv_timeout(Duration::from_secs(20));
                Some(p.clone())
            }
        }
    }
}

struct Rig {
    app: App,
    _root: tempfile::TempDir,
    out: tempfile::TempDir,
    collects: Arc<AtomicUsize>,
    picker: Arc<FakePicker>,
    state: Arc<Diagnostics>,
    jobs: Arc<paddock_jobs::JobRegistry>,
}

fn rig(pick: impl FnOnce(PathBuf, Arc<paddock_jobs::JobRegistry>) -> Pick) -> Rig {
    let root = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let id = AppId::new("demo", "Demo").unwrap();
    let dirs = AppDirs::resolve(
        &id,
        Os::MacOs,
        &MapVars::new().with("DEMO_HOME", root.path()),
        None,
        true,
    )
    .unwrap();
    std::fs::create_dir_all(dirs.logs()).unwrap();
    std::fs::write(
        dirs.logs().join("demo.2026-10-24.log"),
        "{\"@m\":\"line for alex\"}\n",
    )
    .unwrap();
    let app = mock_builder()
        .invoke_handler(invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let jobs = paddock_jobs::tauri::manage(app.handle());
    let collects = Arc::new(AtomicUsize::new(0));
    let picker = Arc::new(FakePicker(
        pick(out.path().join("report.zip"), jobs.clone()),
        Mutex::new(Vec::new()),
    ));
    let redactor = Redactor::new().with_personal(&["alex".into()], &[], &[]);
    let setup = Setup::new(AppInfo::new("demo", "Demo", "1.0.0", ""), dirs)
        .section(Arc::new(Counting(collects.clone())))
        .redactor(redactor)
        .picker(picker.clone());
    let state = install(app.handle(), setup, None);
    Rig {
        app,
        _root: root,
        out,
        collects,
        picker,
        state,
        jobs,
    }
}

fn outcomes(app: &App) -> mpsc::Receiver<Value> {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    app.listen_any(EVENT, move |e| {
        let _ = tx
            .lock()
            .unwrap()
            .send(serde_json::from_str::<Value>(e.payload()).unwrap());
    });
    rx
}

fn next(rx: &mpsc::Receiver<Value>) -> Value {
    rx.recv_timeout(Duration::from_secs(20))
        .expect("an outcome event")
}

fn preview(w: &WebviewWindow<MockRuntime>, token: Option<&str>, description: &str) -> Value {
    invoke(
        w,
        "diagnostics_preview",
        json!({ "token": token, "description": description }),
    )
    .unwrap()
}

#[test]
fn every_command_is_async_and_in_the_table() {
    paddock_commands::testing::assert_async(
        &crate::COMMANDS,
        &[&Path::new(env!("CARGO_MANIFEST_DIR")).join("src")],
    );
}

#[test]
fn a_preview_lists_redacted_files_and_a_later_one_reuses_the_collection() {
    let rig = rig(|_, _| Pick::Dismiss);
    let w = window(&rig.app, "d-main-1");
    let first = preview(&w, None, "");
    let token = first["token"].as_str().unwrap().to_string();
    let names: Vec<&str> = first["preview"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "manifest.json",
            "description.txt",
            "system-info.json",
            "logs/demo.2026-10-24.log",
            "extra/info.txt"
        ]
    );
    assert_eq!(rig.collects.load(Ordering::SeqCst), 1);
    // The user types a description: same token, nothing is collected again, the description file follows.
    let second = preview(&w, Some(&token), "it broke");
    assert_eq!(second["token"], token);
    assert_eq!(rig.collects.load(Ordering::SeqCst), 1);
    let desc = second["preview"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "description.txt")
        .unwrap();
    assert_eq!(desc["bytes"], 8);
    // An unknown token collects anew and gets a new token.
    let third = preview(&w, Some("stale"), "");
    assert_ne!(third["token"], token);
    assert_eq!(rig.collects.load(Ordering::SeqCst), 2);
    assert!(
        third["preview"]["redactions"]["userNames"]
            .as_u64()
            .unwrap()
            >= 2,
        "{third}"
    );
}

#[test]
fn saving_asks_the_user_writes_a_job_and_reports_the_outcome() {
    let rig = rig(|p, _| Pick::File(p));
    let events = outcomes(&rig.app);
    let w = window(&rig.app, "d-main-1");
    let p = preview(&w, None, "");
    let token = p["token"].as_str().unwrap();
    invoke(
        &w,
        "diagnostics_save",
        json!({ "token": token, "description": "it broke" }),
    )
    .unwrap();
    let outcome = next(&events);
    assert_eq!(outcome["state"], "saved", "{outcome}");
    assert_eq!(outcome["files"], 5);
    let zip = std::fs::read(rig.out.path().join("report.zip")).unwrap();
    assert_eq!(zip.len() as u64, outcome["bytes"].as_u64().unwrap());
    // The picker was owned by the asking window and offered a dated name.
    let asked = lock(&rig.picker.1).clone();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].0, "d-main-1");
    assert!(asked[0].1.starts_with("demo-diagnostics-") && asked[0].1.ends_with(".zip"));
    // The written zip is the previewed list (with the description the user typed).
    let archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).unwrap();
    assert_eq!(archive.len(), 5);
    // It ran as a job with the pack phase, and ended Done.
    let snapshot = rig
        .jobs
        .snapshots()
        .into_iter()
        .find(|s| s.kind.0 == JOB_KIND)
        .unwrap();
    assert!(
        matches!(snapshot.state, paddock_jobs::JobState::Done),
        "{:?}",
        snapshot.state
    );
    assert!(!lock(&rig.state.active).saving, "ready for the next save");
}

#[test]
fn closing_the_dialog_writes_nothing_and_says_so() {
    let rig = rig(|_, _| Pick::Dismiss);
    let events = outcomes(&rig.app);
    let w = window(&rig.app, "d-main-1");
    let token = preview(&w, None, "")["token"].as_str().unwrap().to_string();
    invoke(
        &w,
        "diagnostics_save",
        json!({ "token": token, "description": "" }),
    )
    .unwrap();
    assert_eq!(next(&events), json!({ "state": "dismissed" }));
    assert_eq!(std::fs::read_dir(rig.out.path()).unwrap().count(), 0);
    // The user can try again.
    invoke(
        &w,
        "diagnostics_save",
        json!({ "token": token, "description": "" }),
    )
    .unwrap();
    assert_eq!(next(&events)["state"], "dismissed");
}

#[test]
fn cancel_during_the_dialog_leaves_no_file() {
    let rig = rig(Pick::CancelThen);
    let events = outcomes(&rig.app);
    let w = window(&rig.app, "d-main-1");
    let token = preview(&w, None, "")["token"].as_str().unwrap().to_string();
    invoke(
        &w,
        "diagnostics_save",
        json!({ "token": token, "description": "" }),
    )
    .unwrap();
    assert_eq!(next(&events), json!({ "state": "cancelled" }));
    assert_eq!(
        std::fs::read_dir(rig.out.path()).unwrap().count(),
        0,
        "no zip, no partial file"
    );
    let snapshot = rig
        .jobs
        .snapshots()
        .into_iter()
        .find(|s| s.kind.0 == JOB_KIND)
        .unwrap();
    assert!(matches!(
        snapshot.state,
        paddock_jobs::JobState::Cancelled { .. }
    ));
}

#[test]
fn the_cancel_command_stops_the_running_save_and_is_a_no_op_otherwise() {
    let (go, wait) = mpsc::channel();
    let rig = rig(|p, _| Pick::Wait(Mutex::new(wait), p));
    let events = outcomes(&rig.app);
    let w = window(&rig.app, "d-main-1");
    assert_eq!(
        invoke(&w, "diagnostics_cancel", json!({})).unwrap(),
        json!(false),
        "nothing to cancel"
    );
    let token = preview(&w, None, "")["token"].as_str().unwrap().to_string();
    invoke(
        &w,
        "diagnostics_save",
        json!({ "token": token, "description": "" }),
    )
    .unwrap();
    // A second save while the first is waiting for the user is refused.
    let busy = invoke(
        &w,
        "diagnostics_save",
        json!({ "token": token, "description": "" }),
    )
    .unwrap_err();
    assert_eq!(busy["code"], "busy");
    // The job exists from the moment the dialog opens, so Cancel works in that phase too.
    assert_eq!(
        invoke(&w, "diagnostics_cancel", json!({})).unwrap(),
        json!(true)
    );
    go.send(()).unwrap();
    assert_eq!(next(&events), json!({ "state": "cancelled" }));
    assert_eq!(std::fs::read_dir(rig.out.path()).unwrap().count(), 0);
}

#[test]
fn a_stale_preview_a_missing_picker_and_a_bad_answer_are_refused_with_words() {
    let rig = rig(|_, _| Pick::Dismiss);
    let w = window(&rig.app, "d-main-1");
    let e = invoke(
        &w,
        "diagnostics_save",
        json!({ "token": "nope", "description": "" }),
    )
    .unwrap_err();
    assert_eq!(
        (e["code"].as_str(), e["retryable"].as_bool()),
        (Some("expired"), Some(true))
    );
    assert!(e["message"].as_str().unwrap().contains("out of date"));

    // The dialog answers with a path that is not a .zip: the save fails, naming what to change, and writes nothing.
    let rig = self::rig(|p, _| Pick::File(p.with_file_name("report.txt")));
    let events = outcomes(&rig.app);
    let w = window(&rig.app, "d-main-1");
    let token = preview(&w, None, "")["token"].as_str().unwrap().to_string();
    invoke(
        &w,
        "diagnostics_save",
        json!({ "token": token, "description": "" }),
    )
    .unwrap();
    let out = next(&events);
    assert_eq!(out["state"], "failed");
    assert!(out["message"].as_str().unwrap().contains(".zip"), "{out}");
    assert_eq!(std::fs::read_dir(rig.out.path()).unwrap().count(), 0);

    // A build with no way to ask where to save says so.
    let app = mock_builder()
        .invoke_handler(invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    paddock_jobs::tauri::manage(app.handle());
    let root = tempfile::tempdir().unwrap();
    let id = AppId::new("demo", "Demo").unwrap();
    let dirs = AppDirs::resolve(
        &id,
        Os::MacOs,
        &MapVars::new().with("DEMO_HOME", root.path()),
        None,
        true,
    )
    .unwrap();
    install(
        app.handle(),
        Setup::new(AppInfo::new("demo", "Demo", "1", ""), dirs),
        None,
    );
    let w = window(&app, "d-main-1");
    let token = preview(&w, None, "")["token"].as_str().unwrap().to_string();
    let e = invoke(
        &w,
        "diagnostics_save",
        json!({ "token": token, "description": "" }),
    )
    .unwrap_err();
    assert_eq!(e["code"], "noPicker");
}

#[test]
fn without_the_state_the_report_commands_are_refused_and_log_lines_still_flow() {
    let app = mock_builder()
        .invoke_handler(invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let w = window(&app, "main");
    for cmd in [
        "diagnostics_preview",
        "diagnostics_save",
        "diagnostics_cancel",
    ] {
        let e = invoke(&w, cmd, json!({ "token": "t", "description": "" })).unwrap_err();
        assert!(
            e.to_string().contains("not managed") || e.to_string().contains("state"),
            "{cmd}: {e}"
        );
    }
    // The webview's lines go to the process's log; this one is a no-op without a subscriber, but must not fail.
    assert_eq!(
        invoke(
            &w,
            "log_from_webview",
            json!({ "level": "warn", "message": "ui problem" })
        )
        .unwrap(),
        Value::Null
    );
}

#[test]
fn the_log_handle_is_kept_for_the_logging_section_and_flushed_before_collecting() {
    let dir = tempfile::tempdir().unwrap();
    let id = AppId::new("demo", "Demo").unwrap();
    let config = paddock_log::LogConfig::new(&id, "app", "1.0.0")
        .with_log_dir(dir.path())
        .with_stdout(false)
        .with_panic_hook(false);
    paddock_log::Logging::new(config).scoped(|handle| {
        let dirs = AppDirs::resolve(
            &id,
            Os::MacOs,
            &MapVars::new().with("DEMO_HOME", dir.path()),
            None,
            true,
        )
        .unwrap();
        let state = Diagnostics::new(
            Setup::new(AppInfo::new("demo", "Demo", "1", ""), dirs).log(handle.clone()),
            None,
        );
        assert_eq!(
            state.log_handle().map(|h| h.level()),
            Some(paddock_log::LogLevel::Info)
        );
        assert!(!state.preview(None, "").token.is_empty());
    });
}

#[test]
fn the_setups_clock_names_the_file_and_install_keeps_the_first_state() {
    use chrono::TimeZone;
    let rig = rig(|_, _| Pick::Dismiss);
    let events = outcomes(&rig.app);
    let w = window(&rig.app, "d-main-1");
    // A second install keeps the first state (the shell may be built twice in a test).
    let id = AppId::new("demo", "Demo").unwrap();
    let dirs = AppDirs::resolve(
        &id,
        Os::MacOs,
        &MapVars::new().with("DEMO_HOME", rig._root.path()),
        None,
        true,
    )
    .unwrap();
    let again = install(
        rig.app.handle(),
        Setup::new(AppInfo::new("other", "Other", "9", ""), dirs),
        None,
    );
    assert!(Arc::ptr_eq(&again, &rig.state));
    let _ = (events, w);
    // A fixed clock gives a fixed suggested name and a fixed manifest time.
    let fixed = Arc::new(paddock_log::FakeClock::at_wall(
        chrono::Utc
            .with_ymd_and_hms(2026, 10, 24, 10, 30, 0)
            .unwrap()
            .into(),
    ));
    let root = tempfile::tempdir().unwrap();
    let dirs = AppDirs::resolve(
        &id,
        Os::MacOs,
        &MapVars::new().with("DEMO_HOME", root.path()),
        None,
        true,
    )
    .unwrap();
    let app = mock_builder()
        .invoke_handler(invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    paddock_jobs::tauri::manage(app.handle());
    let picker = Arc::new(FakePicker(Pick::Dismiss, Mutex::new(Vec::new())));
    install(
        app.handle(),
        Setup::new(AppInfo::new("demo", "Demo", "1", ""), dirs)
            .clock(fixed)
            .picker(picker.clone()),
        None,
    );
    let events = outcomes(&app);
    let w = window(&app, "d-main-1");
    let token = preview(&w, None, "")["token"].as_str().unwrap().to_string();
    invoke(
        &w,
        "diagnostics_save",
        json!({ "token": token, "description": "" }),
    )
    .unwrap();
    assert_eq!(next(&events)["state"], "dismissed");
    assert_eq!(lock(&picker.1)[0].1, "demo-diagnostics-20261024-103000.zip");
}

#[test]
fn a_save_without_the_jobs_state_is_refused_and_a_panicking_picker_fails_the_save_not_the_app() {
    // No jobs state: a save is a job, so it says the app did not set reports up.
    let app = mock_builder()
        .invoke_handler(invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let id = AppId::new("demo", "Demo").unwrap();
    let dirs = AppDirs::resolve(
        &id,
        Os::MacOs,
        &MapVars::new().with("DEMO_HOME", root.path()),
        None,
        true,
    )
    .unwrap();
    let picker = Arc::new(FakePicker(Pick::Dismiss, Mutex::new(Vec::new())));
    install(
        app.handle(),
        Setup::new(AppInfo::new("demo", "Demo", "1", ""), dirs).picker(picker),
        None,
    );
    let w = window(&app, "d-main-1");
    let token = preview(&w, None, "")["token"].as_str().unwrap().to_string();
    let e = invoke(
        &w,
        "diagnostics_save",
        json!({ "token": token, "description": "" }),
    )
    .unwrap_err();
    assert_eq!(e["code"], "notSetUp");
    assert_eq!(
        invoke(&w, "diagnostics_cancel", json!({})).unwrap(),
        json!(false)
    );
    assert!(
        !lock(
            &install(
                app.handle(),
                Setup::new(
                    AppInfo::new("a", "A", "1", ""),
                    AppDirs::resolve(
                        &id,
                        Os::MacOs,
                        &MapVars::new().with("DEMO_HOME", root.path()),
                        None,
                        true
                    )
                    .unwrap()
                ),
                None
            )
            .active
        )
        .saving
    );

    // A picker that panics: the job contains it, the save ends as failed with words, and the next save can start.
    struct Panics;
    impl SavePicker for Panics {
        fn pick(&self, _: &str, _: &str) -> Option<PathBuf> {
            panic!("the dialog plugin crashed")
        }
    }
    let rig = rig(|_, _| Pick::Dismiss);
    let panicking = mock_builder()
        .invoke_handler(invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    paddock_jobs::tauri::manage(panicking.handle());
    let dirs = AppDirs::resolve(
        &id,
        Os::MacOs,
        &MapVars::new().with("DEMO_HOME", rig._root.path()),
        None,
        true,
    )
    .unwrap();
    install(
        panicking.handle(),
        Setup::new(AppInfo::new("demo", "Demo", "1", ""), dirs).picker(Arc::new(Panics)),
        None,
    );
    let events = outcomes(&panicking);
    let w = window(&panicking, "d-main-1");
    let token = preview(&w, None, "")["token"].as_str().unwrap().to_string();
    invoke(
        &w,
        "diagnostics_save",
        json!({ "token": token, "description": "" }),
    )
    .unwrap();
    let out = next(&events);
    assert_eq!(out["state"], "failed");
    assert!(
        out["message"].as_str().unwrap().contains("internal error"),
        "{out}"
    );
    invoke(
        &w,
        "diagnostics_save",
        json!({ "token": token, "description": "" }),
    )
    .unwrap();
    assert_eq!(
        next(&events)["state"],
        "failed",
        "the failed save did not leave the state stuck"
    );
}

#[test]
fn requests_with_missing_or_mistyped_arguments_are_refused_before_they_run() {
    let rig = rig(|_, _| Pick::Dismiss);
    let w = window(&rig.app, "d-main-1");
    for (cmd, body) in [
        ("diagnostics_preview", json!({ "token": null })),
        ("diagnostics_preview", json!({ "description": 3 })),
        ("diagnostics_save", json!({ "token": "t" })),
        ("diagnostics_save", json!({ "token": 1, "description": "" })),
        ("log_from_webview", json!({ "level": "info" })),
        ("log_from_webview", json!({ "level": 1, "message": "m" })),
    ] {
        assert!(invoke(&w, cmd, body).is_err(), "{cmd}");
    }
    assert_eq!(
        rig.collects.load(Ordering::SeqCst),
        0,
        "nothing was collected for a refused request"
    );
}
