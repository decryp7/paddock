//! The Tauri adapter on Tauri's mock runtime: events on `paddock://jobs`, `jobs_snapshot` and `job_cancel` through the
//! guarded invoke path, the routing in front of an app's handler, and the generated TypeScript (plan S9).
#![cfg(feature = "tauri")]

use paddock_commands::testing::{WriteMode, assert_async, sync_generated};
use paddock_commands::{DialogGrant, ts};
use paddock_jobs::tauri::{
    COMMANDS, CommandError, EVENT, Jobs, handler, handler_with_grants, manage, route,
};
use paddock_jobs::{
    CancelReason, EventSink, FakeClock, JobError, JobId, JobKind, JobPool, JobRegistry,
    JobSnapshot, JobState, Phase, Reporter, Workers, assert_nonblocking,
};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;
use tauri::{App, Listener, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

#[tauri::command]
async fn ping() -> String {
    "pong".into()
}

struct Grant(&'static [&'static str]);

impl DialogGrant for Grant {
    fn dialog(&self) -> &str {
        "jobs-viewer"
    }
    fn allows(&self, command: &str) -> bool {
        self.0.contains(&command)
    }
}

fn app_with(
    jobs: impl Fn(tauri::ipc::Invoke<MockRuntime>) -> bool + Send + Sync + 'static,
) -> App<MockRuntime> {
    mock_builder()
        .invoke_handler(route(jobs, tauri::generate_handler![ping]))
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn window(app: &App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .build()
        .unwrap()
}

fn invoke(
    w: &WebviewWindow<MockRuntime>,
    cmd: &str,
    body: serde_json::Value,
) -> Result<serde_json::Value, serde_json::Value> {
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
    get_ipc_response(w, request).map(|b| b.deserialize::<serde_json::Value>().unwrap())
}

/// Every `paddock://jobs` payload the app emitted, decoded.
fn collect_events(app: &App<MockRuntime>) -> Arc<Mutex<Vec<JobSnapshot>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    app.listen_any(EVENT, move |e| {
        sink.lock()
            .unwrap()
            .push(serde_json::from_str::<JobSnapshot>(e.payload()).unwrap());
    });
    seen
}

#[test]
fn the_event_is_named_by_the_shell_scheme() {
    assert_eq!(EVENT, "paddock://jobs");
    assert_eq!(EVENT, paddock_jobs::EVENT_NAME);
}

#[test]
fn a_job_with_phases_reports_through_events_and_is_cancelled_over_ipc() {
    let app = app_with(handler());
    let main = window(&app, "main");
    let events = collect_events(&app);
    let registry = manage(app.handle());
    assert!(
        Arc::ptr_eq(&registry, &manage(app.handle())),
        "a second set-up keeps the first registry"
    );

    let pool = JobPool::new(Workers::new(1));
    let ctx = registry.register("scan", "Scanning the disk", Some("Macintosh HD"));
    let id = ctx.id();
    let (in_phase_two, entered) = mpsc::channel();
    let job = pool
        .spawn_job::<()>(ctx, move |ctx| {
            ctx.phase(Phase::discovering("find", "Finding files"));
            ctx.add(3);
            ctx.set_total(3);
            ctx.phase(Phase::bytes("read", "Reading", Some(1024)));
            ctx.add(512);
            in_phase_two.send(()).unwrap();
            // Runs until cancelled: the command must not wait for it.
            loop {
                ctx.check()?;
                std::thread::yield_now();
            }
        })
        .unwrap();
    entered.recv().unwrap();

    let listed = assert_nonblocking(&pool, || {
        invoke(&main, "jobs_snapshot", serde_json::json!({}))
    })
    .expect("jobs_snapshot returns at once")
    .unwrap();
    let listed: Vec<JobSnapshot> = serde_json::from_value(listed).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id);
    assert_eq!(listed[0].subject.as_deref(), Some("Macintosh HD"));
    let JobState::Running { phase: Some(p) } = &listed[0].state else {
        panic!("running in its second phase: {:?}", listed[0].state)
    };
    assert_eq!((p.phase.id.as_str(), p.done), ("read", 512));

    let cancelled = assert_nonblocking(&pool, || {
        invoke(&main, "job_cancel", serde_json::json!({ "id": id.0 }))
    })
    .expect("job_cancel returns at once, before the job has stopped");
    assert_eq!(cancelled, Ok(serde_json::Value::Bool(true)));
    assert!(matches!(
        job.join(),
        Err(JobError::Cancelled(CancelReason::User))
    ));
    assert_eq!(
        invoke(&main, "job_cancel", serde_json::json!({ "id": id.0 })),
        Ok(false.into()),
        "an ended job"
    );
    assert_eq!(
        invoke(&main, "job_cancel", serde_json::json!({ "id": 999 })),
        Ok(false.into()),
        "an unknown job"
    );

    let events = events.lock().unwrap().clone();
    let states: Vec<String> = events
        .iter()
        .map(|s| match &s.state {
            JobState::Running { phase: Some(p) } => format!("running:{}", p.phase.id),
            other => format!("{other:?}")
                .split([' ', '{'])
                .next()
                .unwrap()
                .to_lowercase(),
        })
        .collect();
    assert_eq!(states.first().map(String::as_str), Some("queued"));
    assert!(
        states.contains(&"running:find".to_string())
            && states.contains(&"running:read".to_string()),
        "{states:?}"
    );
    assert_eq!(
        events.last().unwrap().state,
        JobState::Cancelled {
            reason: CancelReason::User
        }
    );
    assert!(
        events.windows(2).all(|w| w[0].seq < w[1].seq),
        "events arrive in seq order"
    );
}

#[test]
fn other_commands_reach_the_apps_handler() {
    let app = app_with(handler());
    let main = window(&app, "main");
    manage(app.handle());
    assert_eq!(
        invoke(&main, "ping", serde_json::json!({})),
        Ok("pong".into())
    );
}

#[test]
fn an_app_without_the_jobs_state_gets_a_plain_error() {
    let app = app_with(handler());
    let main = window(&app, "main");
    let e = invoke(&main, "jobs_snapshot", serde_json::json!({})).unwrap_err();
    assert_eq!(
        e,
        serde_json::json!({ "code": "notSetUp", "message": "The jobs service is not set up in this app", "retryable": false })
    );
    assert_eq!(
        invoke(&main, "job_cancel", serde_json::json!({ "id": 1 })).unwrap_err()["code"],
        "notSetUp"
    );
    assert_eq!(CommandError::NotSetUp.code(), "notSetUp");
    let _: &dyn std::error::Error = &CommandError::NotSetUp;
}

#[test]
fn a_malformed_id_is_refused_before_the_registry_is_asked() {
    let app = app_with(handler());
    let main = window(&app, "main");
    manage(app.handle());
    for bad in [
        serde_json::json!({ "id": "1" }),
        serde_json::json!({ "id": -1 }),
        serde_json::json!({}),
    ] {
        let e = invoke(&main, "job_cancel", bad.clone()).unwrap_err();
        assert!(e.as_str().is_some_and(|m| m.contains("id")), "{bad}: {e}");
    }
}

#[test]
fn an_app_may_manage_its_own_registry() {
    struct Quiet;
    impl EventSink for Quiet {
        fn emit(&self, _: &JobSnapshot) {}
    }
    let app = app_with(handler());
    let main = window(&app, "main");
    let registry = Arc::new(JobRegistry::new(Arc::new(Quiet)));
    registry.register("export", "Exporting", None);
    app.manage(Jobs::new(registry.clone()));
    assert!(Arc::ptr_eq(app.state::<Jobs>().registry(), &registry));
    assert!(
        Arc::ptr_eq(&manage(app.handle()), &registry),
        "manage keeps the app's registry"
    );
    let listed = invoke(&main, "jobs_snapshot", serde_json::json!({})).unwrap();
    assert_eq!(listed[0]["state"], "queued");
    assert_eq!(listed[0]["kind"], "export");
}

#[test]
fn dialogs_call_jobs_commands_only_through_their_contract() {
    let plain = app_with(handler());
    manage(plain.handle());
    let dialog = window(&plain, "d-main-1");
    assert_eq!(
        invoke(&dialog, "jobs_snapshot", serde_json::json!({})).unwrap_err()["code"],
        "denied"
    );

    let granted = app_with(handler_with_grants(|_app, label| {
        (label == "d-main-1").then_some(Grant(&["jobs_snapshot"]))
    }));
    manage(granted.handle());
    let dialog = window(&granted, "d-main-1");
    assert_eq!(
        invoke(&dialog, "jobs_snapshot", serde_json::json!({})),
        Ok(serde_json::json!([]))
    );
    let e = invoke(&dialog, "job_cancel", serde_json::json!({ "id": 1 })).unwrap_err();
    assert_eq!(
        (e["code"].as_str(), e["dialog"].as_str()),
        (Some("denied"), Some("jobs-viewer"))
    );
    let popout = window(&granted, "p-main-1");
    assert_eq!(
        invoke(&popout, "job_cancel", serde_json::json!({ "id": 1 })),
        Ok(false.into())
    );
}

#[test]
fn every_command_is_async_and_in_the_table() {
    assert_async(
        &COMMANDS,
        &[&Path::new(env!("CARGO_MANIFEST_DIR")).join("src")],
    );
    assert_eq!(
        COMMANDS.kind_of("jobs_snapshot"),
        paddock_commands::Kind::Query
    );
    assert_eq!(
        COMMANDS.kind_of("job_cancel"),
        paddock_commands::Kind::Mutation
    );
}

const REGENERATE: &str =
    "PADDOCK_WRITE_TS=1 cargo test -p paddock-jobs --features tauri --test tauri generated";

fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/projects/paddock-ui/shell/jobs/generated")
}

/// One snapshot of every shape the UI decodes, produced by the real reporter on a manual clock (lesson C19b: mocks and
/// specs decode values Rust wrote, so a wire change fails both sides).
fn fixture_snapshots() -> Vec<JobSnapshot> {
    struct Keep(Mutex<Vec<JobSnapshot>>);
    impl EventSink for Keep {
        fn emit(&self, s: &JobSnapshot) {
            self.0.lock().unwrap().push(s.clone());
        }
    }
    let keep = Arc::new(Keep(Mutex::new(Vec::new())));
    let clock = Arc::new(FakeClock::new());
    let reporter = |id: u64, kind: &str, title: &str, subject: Option<&str>| {
        Reporter::with_clock(
            JobId(id),
            JobKind::new(kind),
            title,
            subject,
            keep.clone(),
            clock.clone(),
            0,
        )
    };
    let queued = reporter(1, "import", "Importing logs", Some("3 folders"));
    let finding = reporter(2, "import", "Importing logs", None);
    finding.begin_phase(Phase::discovering("find", "Finding files"));
    clock.advance_ms(1_200);
    finding.add(12_345);
    let download = reporter(3, "download", "Downloading the model", Some("model.gguf"));
    download.begin_phase(Phase::bytes(
        "download",
        "Downloading",
        Some(118 * 1024 * 1024),
    ));
    for _ in 0..10 {
        clock.advance_ms(500);
        download.add(4 * 1024 * 1024);
    }
    let writing = reporter(4, "save", "Saving the project", None);
    writing.begin_phase(Phase::items("write", "Writing", 40));
    writing.add(3);
    let guard = writing.uncancellable("Writing the database");
    let done = reporter(5, "export", "Exporting report.html", None);
    done.start();
    done.finish(JobState::Done);
    let failed = reporter(6, "scan", "Scanning", None);
    failed.start();
    failed.finish(JobState::Failed {
        message: "The folder is not readable".into(),
    });
    let user = reporter(7, "scan", "Scanning", None);
    user.finish(JobState::Cancelled {
        reason: CancelReason::User,
    });
    let deadline = reporter(8, "research", "Researching", None);
    deadline.start();
    deadline.finish(JobState::Cancelled {
        reason: CancelReason::Deadline,
    });
    let all = vec![
        queued.snapshot(),
        finding.snapshot(),
        download.snapshot(),
        writing.snapshot(),
        done.snapshot(),
        failed.snapshot(),
        user.snapshot(),
        deadline.snapshot(),
    ];
    drop(guard);
    all
}

#[test]
fn generated_typescript_is_current() {
    let names = ts::names_module(&COMMANDS, "JOBS_COMMANDS", "JobsCommand", REGENERATE).unwrap();
    let fixtures = serde_json::to_string_pretty(&fixture_snapshots()).unwrap();
    let fixtures = format!(
        "// Generated from paddock-jobs' wire model by its tests (Rust round trip). Do not edit: {REGENERATE}.\n\n\
         /** One `paddock://jobs` snapshot of every shape, as Rust serialises them: queued, discovering, determinate bytes,\n \
         * not cancellable, done, failed, cancelled by the user and by a deadline. */\n\
         export const JOB_SNAPSHOT_FIXTURES: readonly unknown[] = {fixtures};\n"
    );
    let mode = WriteMode::from_env();
    let stale: Vec<String> = [("jobs-commands.ts", names), ("jobs-fixtures.ts", fixtures)]
        .into_iter()
        .filter_map(|(name, text)| sync_generated(&generated_dir().join(name), &text, mode).err())
        .map(|e| e.to_string())
        .collect();
    assert!(stale.is_empty(), "{}", stale.join("\n"));
}
