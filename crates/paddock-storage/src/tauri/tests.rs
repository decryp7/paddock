//! The adapter on Tauri's mock runtime: no window reaches the desktop, files live in temporary folders, and waiting is
//! event-driven (a channel with a bound), never a sleep. A job is held still with a gate the test opens.

use super::*;
use crate::walk::tests::write;
use crate::{Category, ClearChoice, Item, Kind, Source};
use ::tauri::ipc::{CallbackFn, InvokeBody};
use ::tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use ::tauri::webview::InvokeRequest;
use ::tauri::{Listener, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use paddock_files::open::{Invocation, Launcher};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Parks the thread that asks for the categories (the job) until the test lets it go.
struct Gated {
    armed: AtomicBool,
    parked: mpsc::SyncSender<()>,
    go: Mutex<mpsc::Receiver<()>>,
    categories: Vec<Category>,
    panics: AtomicBool,
}

impl Source for Gated {
    fn categories(&self) -> Vec<Category> {
        assert!(
            !self.panics.load(Ordering::SeqCst),
            "a bug in the app's categories"
        );
        if self.armed.swap(false, Ordering::SeqCst) {
            self.parked.send(()).unwrap();
            self.go
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(30))
                .unwrap();
        }
        self.categories.clone()
    }
}

struct Seen(Mutex<Vec<Invocation>>);

impl Launcher for Seen {
    fn start(&self, i: &Invocation) -> std::io::Result<()> {
        self.0.lock().unwrap().push(i.clone());
        Ok(())
    }
}

struct Rig {
    app: App,
    root: PathBuf,
    _dir: tempfile::TempDir,
    source: Arc<Gated>,
    parked: mpsc::Receiver<()>,
    go: mpsc::SyncSender<()>,
    shown: Arc<Seen>,
    events: mpsc::Receiver<StorageEvent>,
}

fn rig(
    with_files: bool,
    with_jobs: bool,
    extra: impl FnOnce(&std::path::Path, Vec<Category>) -> Vec<Category>,
) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = paddock_files::path::canonical(dir.path()).unwrap();
    write(&root.join("cache/a.bin"), 100);
    write(&root.join("cache/b.bin"), 50);
    let categories = extra(
        &root,
        vec![
            Category::new("cache", "Cache", Kind::Cache)
                .folder(root.join("cache"))
                .entries(|_| None),
        ],
    );
    let (parked_tx, parked) = mpsc::sync_channel(1);
    let (go, go_rx) = mpsc::sync_channel(1);
    let source = Arc::new(Gated {
        armed: AtomicBool::new(false),
        parked: parked_tx,
        go: Mutex::new(go_rx),
        categories,
        panics: AtomicBool::new(false),
    });
    let app = mock_builder()
        .invoke_handler(invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    if with_jobs {
        paddock_jobs::tauri::manage(app.handle());
    }
    let shown = Arc::new(Seen(Mutex::new(Vec::new())));
    if with_files {
        paddock_files::tauri::install(
            app.handle(),
            paddock_files::Files::new(paddock_files::Recents::open(None, &[]))
                .launcher(shown.clone()),
        );
    }
    install(
        app.handle(),
        Storage::new(source.clone()).also_within(&root),
    );
    let (tx, events) = mpsc::channel();
    app.listen(EVENT, move |e| {
        let _ = tx.send(serde_json::from_str::<StorageEvent>(e.payload()).unwrap());
    });
    Rig {
        app,
        root,
        _dir: dir,
        source,
        parked,
        go,
        shown,
        events,
    }
}

fn event(r: &Rig) -> StorageEvent {
    r.events
        .recv_timeout(Duration::from_secs(30))
        .expect("the job ends with an event")
}

#[test]
fn a_scan_is_a_job_whose_outcome_is_an_event_and_the_last_usage_is_kept() {
    let r = rig(true, true, |_, c| c);
    let main = window(&r.app, "main");
    assert_eq!(
        invoke(&main, "storage_last", json!({})).unwrap(),
        Value::Null
    );
    invoke(&main, "storage_scan", json!({})).unwrap();
    let StorageEvent::Usage { usage } = event(&r) else {
        panic!("a usage")
    };
    assert_eq!(usage.total_bytes, 150);
    let last = invoke(&main, "storage_last", json!({})).unwrap();
    assert_eq!(last["totalBytes"], 150);
    assert_eq!(last["categories"][0]["id"], "cache");
    // The job is finished and shown in the jobs list.
    let jobs = r
        .app
        .state::<paddock_jobs::tauri::Jobs>()
        .registry()
        .snapshots();
    assert!(jobs.iter().any(|j| j.kind.0 == SCAN_KIND), "{jobs:?}");
}

#[test]
fn a_clear_needs_the_preview_the_confirmation_and_ends_with_what_it_did() {
    let r = rig(true, true, |root, mut c| {
        write(&root.join("logs/old.log"), 10);
        c.push(
            Category::new("logs", "Logs", Kind::Data)
                .folder(root.join("logs"))
                .entries(|_| None),
        );
        c
    });
    let main = window(&r.app, "main");
    let dialog = window(&r.app, "d-main-1");
    let ask = |confirmed: bool| json!({ "request": { "categories": [{ "id": "cache" }, { "id": "logs", "confirmed": confirmed }] } });
    // No preview yet: refused at once, nothing starts.
    let refused = invoke(&main, "storage_clear", ask(true)).unwrap_err();
    assert_eq!(
        (refused["code"].as_str(), refused["kind"].as_str()),
        (Some("invalid"), Some("noPreview"))
    );
    invoke(&dialog, "storage_scan", json!({})).unwrap();
    assert!(matches!(event(&r), StorageEvent::Usage { .. }));
    // Unknown categories are refused before a job exists.
    let unknown = invoke(
        &main,
        "storage_clear",
        json!({ "request": { "categories": [{ "id": "nope" }] } }),
    )
    .unwrap_err();
    assert_eq!(unknown["code"], "notFound");
    invoke(&dialog, "storage_clear", ask(false)).unwrap();
    let StorageEvent::Cleared { result } = event(&r) else {
        panic!("a result")
    };
    assert_eq!(
        result.freed_bytes, 150,
        "the cache went, the unconfirmed data stayed"
    );
    assert!(
        result.categories[1]
            .error
            .as_deref()
            .unwrap()
            .contains("confirm")
    );
    assert!(r.root.join("logs/old.log").exists() && !r.root.join("cache/a.bin").exists());
    assert_eq!(
        invoke(&main, "storage_last", json!({})).unwrap(),
        Value::Null,
        "a clear forgets the old figures"
    );
    let again = invoke(&main, "storage_clear", ask(true)).unwrap_err();
    assert_eq!(again["kind"], "noPreview", "the preview was used up");
}

#[test]
fn only_one_job_runs_at_a_time_and_cancel_ends_a_scan_without_changes() {
    let r = rig(true, true, |_, c| c);
    let main = window(&r.app, "main");
    assert_eq!(
        invoke(&main, "storage_cancel", json!({})).unwrap(),
        json!(false),
        "nothing to cancel"
    );
    r.source.armed.store(true, Ordering::SeqCst);
    invoke(&main, "storage_scan", json!({})).unwrap();
    r.parked.recv_timeout(Duration::from_secs(30)).unwrap();
    let busy = invoke(&main, "storage_scan", json!({})).unwrap_err();
    assert_eq!(busy["code"], "busy");
    assert_eq!(
        invoke(&main, "storage_cancel", json!({})).unwrap(),
        json!(true)
    );
    r.go.send(()).unwrap();
    assert_eq!(event(&r), StorageEvent::Cancelled);
    assert_eq!(
        invoke(&main, "storage_last", json!({})).unwrap(),
        Value::Null
    );
    // The job slot is free again.
    invoke(&main, "storage_scan", json!({})).unwrap();
    assert!(matches!(event(&r), StorageEvent::Usage { .. }));
}

#[test]
fn cancel_during_a_clear_stops_between_items_and_reports_a_partial_result() {
    let (hold_tx, hold_rx) = mpsc::sync_channel::<()>(1);
    let (release_tx, release_rx) = mpsc::sync_channel::<()>(1);
    let hold_tx = Arc::new(Mutex::new(hold_tx));
    let release_rx = Arc::new(Mutex::new(release_rx));
    let r = rig(true, true, move |root, _| {
        write(&root.join("m/one"), 1);
        write(&root.join("m/two"), 2);
        let (hold, release) = (hold_tx.clone(), release_rx.clone());
        vec![
            Category::new("m", "Models", Kind::Cache)
                .folder(root.join("m"))
                .item(Item::path(root.join("m/one")).removed_by(move |p| {
                    hold.lock().unwrap().send(()).unwrap();
                    release
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(30))
                        .unwrap();
                    std::fs::remove_file(p).map_err(|e| e.to_string())
                }))
                .item(Item::path(root.join("m/two"))),
        ]
    });
    let main = window(&r.app, "main");
    invoke(&main, "storage_scan", json!({})).unwrap();
    assert!(matches!(event(&r), StorageEvent::Usage { .. }));
    invoke(
        &main,
        "storage_clear",
        json!({ "request": { "categories": [{ "id": "m" }] } }),
    )
    .unwrap();
    hold_rx.recv_timeout(Duration::from_secs(30)).unwrap();
    assert_eq!(
        invoke(&main, "storage_cancel", json!({})).unwrap(),
        json!(true)
    );
    release_tx.send(()).unwrap();
    let StorageEvent::Cleared { result } = event(&r) else {
        panic!("a partial result")
    };
    assert!(result.cancelled);
    assert!(!r.root.join("m/one").exists() && r.root.join("m/two").exists());
    let jobs = r
        .app
        .state::<paddock_jobs::tauri::Jobs>()
        .registry()
        .snapshots();
    assert!(
        jobs.iter().any(|j| j.kind.0 == CLEAR_KIND
            && matches!(j.state, paddock_jobs::JobState::Cancelled { .. }))
    );
}

#[test]
fn a_bug_in_the_apps_categories_fails_the_job_not_the_app() {
    let r = rig(true, true, |_, c| c);
    let main = window(&r.app, "main");
    r.source.panics.store(true, Ordering::SeqCst);
    invoke(&main, "storage_scan", json!({})).unwrap();
    let StorageEvent::Failed { message } = event(&r) else {
        panic!("a failure")
    };
    assert!(message.contains("internal error"), "{message}");
    r.source.panics.store(false, Ordering::SeqCst);
    invoke(&main, "storage_scan", json!({})).unwrap();
    assert!(
        matches!(event(&r), StorageEvent::Usage { .. }),
        "later work is not poisoned"
    );
}

#[test]
fn a_size_tree_is_a_job_refused_up_front_for_a_folder_it_may_not_walk() {
    let r = rig(true, true, |root, mut c| {
        c.push(Category::new("gone", "Gone", Kind::Cache).folder(root.join("gone")));
        c
    });
    let main = window(&r.app, "main");
    invoke(&main, "storage_tree", json!({ "id": "cache" })).unwrap();
    let StorageEvent::Tree { tree } = event(&r) else {
        panic!("a tree")
    };
    assert_eq!(
        (tree.root.bytes, tree.capped, tree.root.children.len()),
        (150, false, 2)
    );
    assert_eq!(
        invoke(&main, "storage_tree", json!({ "id": "nope" })).unwrap_err()["code"],
        "notFound"
    );
    assert_eq!(
        invoke(&main, "storage_tree", json!({ "id": "gone" })).unwrap_err()["code"],
        "unavailable"
    );
    let jobs = r
        .app
        .state::<paddock_jobs::tauri::Jobs>()
        .registry()
        .snapshots();
    assert!(jobs.iter().any(|j| j.kind.0 == TREE_KIND));
}

#[test]
fn a_category_folder_is_shown_by_id_only() {
    let r = rig(true, true, |_, c| c);
    let main = window(&r.app, "main");
    invoke(&main, "storage_reveal", json!({ "id": "cache" })).unwrap();
    assert_eq!(r.shown.0.lock().unwrap().len(), 1);
    assert_eq!(
        invoke(&main, "storage_reveal", json!({ "id": "/etc" })).unwrap_err()["code"],
        "notFound"
    );
    let bare = rig(false, true, |_, c| c);
    let w = window(&bare.app, "main");
    assert_eq!(
        invoke(&w, "storage_reveal", json!({ "id": "cache" })).unwrap_err()["code"],
        "unavailable"
    );
}

#[test]
fn without_jobs_the_commands_say_so() {
    let r = rig(true, false, |_, c| c);
    let main = window(&r.app, "main");
    assert_eq!(
        invoke(&main, "storage_scan", json!({})).unwrap_err()["code"],
        "unavailable"
    );
    assert_eq!(
        invoke(&main, "storage_cancel", json!({})).unwrap(),
        json!(false)
    );
    let again = install(
        r.app.handle(),
        Storage::new(Arc::new(Vec::<Category>::new())),
    );
    assert!(
        Arc::ptr_eq(&again, &again.clone()) && again.storage().folder("cache").is_ok(),
        "a second install keeps the first"
    );
}

#[test]
fn the_events_have_a_stable_wire_form() {
    let v = serde_json::to_value(StorageEvent::Failed {
        message: "x".into(),
    })
    .unwrap();
    assert_eq!(v, json!({ "state": "failed", "message": "x" }));
    assert_eq!(
        serde_json::to_value(StorageEvent::Cancelled).unwrap(),
        json!({ "state": "cancelled" })
    );
    let _ = ClearChoice {
        id: "a".into(),
        confirmed: false,
    };
}

#[test]
fn a_phase_without_a_known_total_is_a_counting_phase() {
    use paddock_jobs::{EventSink, JobRegistry, JobSnapshot};
    struct Collect(Mutex<Vec<JobSnapshot>>);
    impl EventSink for Collect {
        fn emit(&self, s: &JobSnapshot) {
            self.0.lock().unwrap().push(s.clone());
        }
    }
    let sink = Arc::new(Collect(Mutex::new(Vec::new())));
    let registry = JobRegistry::new(sink.clone());
    let ctx = registry.register("t", "Test", None);
    let _ = ctx.run(|ctx| {
        let run = JobRun(ctx);
        run.phase("a", "Counting", None);
        run.add(3);
        run.phase("b", "Known", Some(5));
        assert!(!run.cancelled());
        Ok(())
    });
    let totals: Vec<(String, Option<u64>)> = sink
        .0
        .lock()
        .unwrap()
        .iter()
        .filter_map(|s| match &s.state {
            paddock_jobs::JobState::Running { phase: Some(p) } => {
                Some((p.phase.label.clone(), p.phase.total))
            }
            _ => None,
        })
        .collect();
    assert!(
        totals.contains(&("Counting".to_string(), None)),
        "{totals:?}"
    );
    assert!(
        totals.contains(&("Known".to_string(), Some(5))),
        "{totals:?}"
    );
}
