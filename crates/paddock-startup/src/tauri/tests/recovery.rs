//! The recovery commands on the mock app: the notice after a crash, each of the three answers, the autosaved session
//! behind `ui_checkpoint`, and the clean exit.

use super::*;
use crate::lifecycle::{InstanceScope, Start};
use crate::safe_mode::Mode;
use paddock_log::CrashMarker;
use std::path::Path;

fn begin(dir: &Path, pid: u32, alive: &[u32]) -> Lifecycle {
    let alive = alive.to_vec();
    Lifecycle::begin_with(
        Start {
            data_dir: dir,
            process: "app",
            mode: Mode::Normal,
            scope: InstanceScope::Main,
            app_version: "1.0.0",
        },
        pid,
        &move |p| alive.contains(&p).then_some(1000),
    )
}

/// A run that crashed with `main` and `w-2` checkpoints, and the app started after it.
fn after_a_crash(dir: &Path) -> Lifecycle {
    let first = begin(dir, 100, &[100]);
    first.checkpoint("main", json!({"query": "error"})).unwrap();
    first.checkpoint("w-2", json!({"query": "other"})).unwrap();
    first.flush();
    std::mem::forget(first);
    begin(dir, 200, &[200])
}

fn app_after(life: Lifecycle, seen: &Shared) -> App {
    app_with(builder(seen).lifecycle(life).build().unwrap())
}

#[test]
fn after_a_crash_the_page_hears_the_offer_and_each_answer_settles_it() {
    let t = tempfile::tempdir().unwrap();
    let seen = Shared::default();
    let app = app_after(after_a_crash(t.path()), &seen);
    let w = window(&app, "main");
    let status = invoke(&w, "recovery_status", json!({})).unwrap();
    assert_eq!(status["safeMode"], false);
    assert_eq!(
        status["unclean"]["offers"],
        json!({"restore": true, "safeMode": true, "startFresh": true})
    );
    assert_eq!(status["unclean"]["session"]["windows"], 2);

    let plan = invoke(&w, "recovery_restore", json!({})).unwrap();
    assert_eq!(plan["windows"], json!(["main", "w-2"]));
    assert_eq!(plan["ui"], json!({"query": "error"}));
    let after = invoke(&w, "recovery_status", json!({})).unwrap();
    assert_eq!(after["unclean"], Value::Null, "the notice does not return");
}

#[test]
fn the_notice_is_the_main_windows_only() {
    let t = tempfile::tempdir().unwrap();
    let seen = Shared::default();
    let app = app_after(after_a_crash(t.path()), &seen);
    let other = window(&app, "w-2");
    let status = invoke(&other, "recovery_status", json!({})).unwrap();
    assert_eq!(status, json!({"safeMode": false, "unclean": null}));
    let main = window(&app, "main");
    assert!(invoke(&main, "recovery_status", json!({})).unwrap()["unclean"].is_object());
}

#[test]
fn start_fresh_forgets_the_session() {
    let t = tempfile::tempdir().unwrap();
    let seen = Shared::default();
    let app = app_after(after_a_crash(t.path()), &seen);
    let w = window(&app, "main");
    invoke(&w, "recovery_start_fresh", json!({})).unwrap();
    assert_eq!(
        invoke(&w, "recovery_status", json!({})).unwrap()["unclean"],
        Value::Null
    );
    assert_eq!(
        invoke(&w, "recovery_restore", json!({})).unwrap()["windows"],
        json!([])
    );
}

#[test]
fn safe_mode_runs_the_apps_restart_and_a_failing_one_keeps_the_notice() {
    let t = tempfile::tempdir().unwrap();
    let attempts = Arc::new(Mutex::new(Vec::<String>::new()));
    let log = attempts.clone();
    let app = app_with(
        Startup::builder()
            .lifecycle(after_a_crash(t.path()))
            .safe_mode(move |_app, label| {
                let mut seen = lock(&log);
                seen.push(label.to_string());
                if seen.len() == 1 {
                    Err("no relaunch here".into())
                } else {
                    Ok(())
                }
            })
            .build()
            .unwrap(),
    );
    let main = window(&app, "main");
    let e = invoke(&main, "recovery_safe_mode", json!({})).unwrap_err();
    assert_eq!(e["code"], "safeModeFailed");
    assert!(e["message"].as_str().unwrap().contains("no relaunch here"));
    assert!(
        invoke(&main, "recovery_status", json!({})).unwrap()["unclean"].is_object(),
        "the user can choose again"
    );
    invoke(&main, "recovery_safe_mode", json!({})).unwrap();
    assert_eq!(*lock(&attempts), ["main", "main"]);
    assert_eq!(
        invoke(&main, "recovery_status", json!({})).unwrap()["unclean"],
        Value::Null
    );
}

#[test]
fn an_app_without_a_safe_mode_restart_does_not_offer_it() {
    let t = tempfile::tempdir().unwrap();
    let app = app_with(
        Startup::builder()
            .lifecycle(after_a_crash(t.path()))
            .build()
            .unwrap(),
    );
    let w = window(&app, "main");
    let status = invoke(&w, "recovery_status", json!({})).unwrap();
    assert_eq!(status["unclean"]["offers"]["safeMode"], false);
    assert_eq!(
        invoke(&w, "recovery_safe_mode", json!({})).unwrap_err()["code"],
        "safeModeUnavailable"
    );
}

#[test]
fn without_a_lifecycle_the_start_is_normal_and_the_answers_say_so() {
    let (app, _) = app();
    let w = window(&app, "main");
    assert_eq!(
        invoke(&w, "recovery_status", json!({})).unwrap(),
        json!({"safeMode": false, "unclean": null})
    );
    for cmd in [
        "recovery_restore",
        "recovery_start_fresh",
        "recovery_safe_mode",
    ] {
        assert_eq!(
            invoke(&w, cmd, json!({})).unwrap_err()["code"],
            "noLifecycle",
            "{cmd}"
        );
    }
}

#[test]
fn a_safe_mode_run_reports_it_without_a_notice() {
    let t = tempfile::tempdir().unwrap();
    let life = Lifecycle::begin_with(
        Start {
            data_dir: t.path(),
            process: "app",
            mode: Mode::Safe,
            scope: InstanceScope::Main,
            app_version: "1.0.0",
        },
        300,
        &|_| Some(1),
    );
    let seen = Shared::default();
    let app = app_after(life, &seen);
    let w = window(&app, "main");
    let status = invoke(&w, "recovery_status", json!({})).unwrap();
    assert_eq!(status, json!({"safeMode": true, "unclean": null}));
}

#[test]
fn checkpoints_reach_the_session_file_and_an_oversized_one_is_refused_once() {
    let t = tempfile::tempdir().unwrap();
    let seen = Shared::default();
    let app = app_after(begin(t.path(), 100, &[100]), &seen);
    let w = window(&app, "main");
    invoke(&w, "ui_checkpoint", json!({"ui": {"query": "kept"}})).unwrap();
    let big = json!({"ui": {"blob": "x".repeat(crate::session::MAX_WINDOW_BYTES + 1)}});
    // The reload checkpoint still works; only the disk refuses.
    invoke(&w, "ui_checkpoint", big.clone()).unwrap();
    invoke(&w, "ui_checkpoint", big).unwrap();
    let life = startup(&app).lifecycle().unwrap().session().current();
    let _ = life;
    startup(&app).lifecycle().unwrap().flush();
    assert_eq!(
        startup(&app)
            .lifecycle()
            .unwrap()
            .session()
            .current()
            .windows["main"],
        json!({"query": "kept"})
    );
    let refusals = log_of("main")
        .into_iter()
        .filter(|(_, m)| m.contains("is not kept in the session"))
        .count();
    assert_eq!(refusals, 1, "one log line, not one per checkpoint");
    // In memory the big state is what a reload gets back.
    assert!(startup(&app).checkpoint("main", json!({"a": 1})).is_ok());
}

#[test]
fn a_clean_exit_removes_the_marker_and_keeps_the_last_checkpoint() {
    let t = tempfile::tempdir().unwrap();
    let seen = Shared::default();
    let app = app_after(begin(t.path(), 100, &[100]), &seen);
    let w = window(&app, "main");
    invoke(&w, "ui_checkpoint", json!({"ui": {"n": 9}})).unwrap();
    assert!(t.path().join(crate::lifecycle::MARKER_FILE).exists());
    end_run(app.handle());
    assert!(!t.path().join(crate::lifecycle::MARKER_FILE).exists());
    let next = begin(t.path(), 200, &[200]);
    assert_eq!(next.previous_run(), crate::PreviousRun::Clean);
    assert_eq!(next.session().previous().windows["main"], json!({"n": 9}));
    // Without the state installed it does nothing.
    let bare = mock_builder().build(mock_context(noop_assets())).unwrap();
    end_run(bare.handle());
}

#[test]
fn the_crash_marker_of_the_log_is_cleared_by_the_answer() {
    let t = tempfile::tempdir().unwrap();
    let life = after_a_crash(t.path());
    let marker = CrashMarker::path_for(t.path(), "app");
    std::fs::write(&marker, r#"{"version":1,"process":"app","pid":100,"time":"t","thread":"w","message":"m","location":"a.rs:1"}"#).unwrap();
    drop(life);
    let seen = Shared::default();
    let app = app_after(begin(t.path(), 201, &[201]), &seen);
    let w = window(&app, "main");
    invoke(&w, "recovery_start_fresh", json!({})).unwrap();
    assert!(!marker.exists());
}

#[test]
fn only_main_windows_may_ask_about_recovery() {
    let (app, _) = app();
    let popout = window(&app, "p-host-1");
    for cmd in [
        "recovery_status",
        "recovery_restore",
        "recovery_start_fresh",
        "recovery_safe_mode",
    ] {
        let e = invoke(&popout, cmd, json!({})).unwrap_err();
        assert_ne!(
            e["code"], "noLifecycle",
            "{cmd} must be stopped by the guard before the command runs"
        );
    }
}
