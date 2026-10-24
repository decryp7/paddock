//! The adapter on Tauri's mock runtime: no window reaches the desktop, the picker and launcher are fakes, files live in
//! temporary folders, and events are awaited through a channel (bounded), never a sleep.

use super::*;
use crate::files::fakes::{Choosing, Recording};
use crate::open::Os;
use crate::recent::{FILES, PROJECTS, Recents};
use ::tauri::ipc::{CallbackFn, InvokeBody};
use ::tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use ::tauri::webview::InvokeRequest;
use ::tauri::{Listener, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use serde_json::{Value, json};
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

type App = ::tauri::App<MockRuntime>;

#[::tauri::command]
async fn read_it(grants: WindowGrants, path: String) -> Result<String, CommandError> {
    Ok(grants.read_file(&path, &["log"])?.text())
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

fn s(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}

struct Rig {
    app: App,
    dir: tempfile::TempDir,
    launched: Arc<Recording>,
    files: Arc<Files>,
}

fn rig(chosen: Vec<PathBuf>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let launched = Arc::new(Recording::default());
    let app = mock_builder()
        .invoke_handler(::tauri::generate_handler![
            files_pick,
            files_recent_list,
            files_recent_remove,
            files_recent_clear,
            files_recent_grant,
            files_reveal,
            files_open,
            files_open_app_folder,
            files_open_url,
            read_it
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let files = Files::new(Recents::open(Some(dir.path()), &[FILES, PROJECTS]))
        .picker(Arc::new(Choosing(chosen, Mutex::default())))
        .launcher(launched.clone())
        .os(Os::Mac)
        .folder(AppFolder::Logs, dir.path());
    let files = install(app.handle(), files);
    Rig {
        app,
        dir,
        launched,
        files,
    }
}

fn expect_event<T: serde::de::DeserializeOwned + Send + 'static>(
    on: &impl Listener<MockRuntime>,
    name: &str,
) -> mpsc::Receiver<T> {
    let (tx, rx) = mpsc::channel();
    on.listen(name, move |e| {
        let _ = tx.send(serde_json::from_str::<T>(e.payload()).unwrap());
    });
    rx
}

#[test]
fn a_dialog_window_picks_for_its_host_and_the_app_reads_with_the_grant() {
    let t = tempfile::tempdir().unwrap();
    let log = t.path().join("a.log");
    std::fs::write(&log, "x").unwrap();
    let r = rig(vec![log.clone()]);
    let main = window(&r.app, "main");
    let dialog = window(&r.app, "d-main-1");
    let other = window(&r.app, "w-2");
    // Nothing was chosen yet: the app's own command refuses.
    let e = invoke(&main, "read_it", json!({ "path": s(&log) })).unwrap_err();
    assert_eq!(e["code"], "notGranted");
    let picked = invoke(
        &dialog,
        "files_pick",
        json!({ "request": { "kind": "openFile", "title": "Open" } }),
    )
    .unwrap();
    assert_eq!(picked, json!([s(&std::fs::canonicalize(&log).unwrap())]));
    assert!(
        invoke(&main, "read_it", json!({ "path": s(&log) })).is_ok(),
        "the dialog's grant belongs to its host"
    );
    assert_eq!(
        invoke(&other, "read_it", json!({ "path": s(&log) })).unwrap_err()["code"],
        "notGranted"
    );
    // The extension check is the app's.
    r.files
        .grants()
        .grant("main", t.path(), Access::Read, GrantSource::Output)
        .unwrap();
    assert_eq!(
        invoke(&main, "read_it", json!({ "path": s(t.path()) })).unwrap_err()["code"],
        "invalid"
    );
    // A bad request is refused through the wire error.
    let bad = invoke(
        &main,
        "files_pick",
        json!({ "request": { "kind": "save", "defaultName": "../x" } }),
    )
    .unwrap_err();
    assert_eq!(bad["code"], "invalid");
}

#[test]
fn recent_lists_are_recorded_listed_regranted_and_announced() {
    let t = tempfile::tempdir().unwrap();
    let file = t.path().join("p.proj");
    std::fs::write(&file, "x").unwrap();
    let r = rig(vec![]);
    let main = window(&r.app, "main");
    let changed = expect_event::<RecentLists>(&r.app, RECENT_EVENT);
    let granted = r
        .files
        .grants()
        .grant("main", &file, Access::Read, GrantSource::Dialog)
        .unwrap();
    opened(r.app.handle(), "projects", &granted, Some("Alpha"));
    let lists = changed.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(lists.entries("projects")[0].name.as_deref(), Some("Alpha"));
    // Opening the same path again changes nothing... except the time, which the lists record: still announced.
    let listed = invoke(&main, "files_recent_list", json!({})).unwrap();
    assert_eq!(listed["lists"][1]["entries"][0]["path"], granted.text());
    // A later run: grants gone, the list remains, the regrant works for the window.
    r.files.grants().forget("main");
    let path = invoke(
        &main,
        "files_recent_grant",
        json!({ "list": "projects", "path": granted.text() }),
    )
    .unwrap();
    assert_eq!(path, json!(granted.text()));
    assert!(
        invoke(
            &main,
            "files_recent_grant",
            json!({ "list": "files", "path": granted.text() })
        )
        .is_err()
    );
    invoke(
        &main,
        "files_recent_remove",
        json!({ "list": "projects", "path": granted.text() }),
    )
    .unwrap();
    assert!(changed.recv_timeout(Duration::from_secs(10)).is_ok());
    invoke(
        &main,
        "files_recent_remove",
        json!({ "list": "projects", "path": granted.text() }),
    )
    .unwrap();
    opened(r.app.handle(), "projects", &granted, None);
    assert!(changed.recv_timeout(Duration::from_secs(10)).is_ok());
    invoke(&main, "files_recent_clear", json!({ "list": "projects" })).unwrap();
    assert!(changed.recv_timeout(Duration::from_secs(10)).is_ok());
    invoke(&main, "files_recent_clear", json!({ "list": "projects" })).unwrap();
    assert!(
        changed.try_recv().is_err(),
        "an unchanged list is not announced"
    );
    // An app without the service ignores `opened`.
    let bare = mock_builder().build(mock_context(noop_assets())).unwrap();
    opened(bare.handle(), "projects", &granted, None);
    assert!(grant_command_line(bare.handle(), "main", &[s(&file)]).is_empty());
}

#[test]
fn reveal_open_folders_and_urls_go_through_the_launcher() {
    let t = tempfile::tempdir().unwrap();
    let log = t.path().join("a.log");
    std::fs::write(&log, "x").unwrap();
    let r = rig(vec![]);
    let main = window(&r.app, "main");
    assert_eq!(
        invoke(&main, "files_reveal", json!({ "path": s(&log) })).unwrap_err()["code"],
        "notGranted"
    );
    r.files
        .grants()
        .grant("main", &log, Access::Read, GrantSource::Dialog)
        .unwrap();
    invoke(&main, "files_reveal", json!({ "path": s(&log) })).unwrap();
    invoke(&main, "files_open", json!({ "path": s(&log) })).unwrap();
    invoke(&main, "files_open_app_folder", json!({ "folder": "logs" })).unwrap();
    assert_eq!(
        invoke(&main, "files_open_app_folder", json!({ "folder": "cache" })).unwrap_err()["code"],
        "notFound"
    );
    invoke(
        &main,
        "files_open_url",
        json!({ "url": "https://example.org" }),
    )
    .unwrap();
    assert_eq!(
        invoke(
            &main,
            "files_open_url",
            json!({ "url": "file:///etc/passwd" })
        )
        .unwrap_err()["code"],
        "invalid"
    );
    assert_eq!(r.launched.started.lock().unwrap().len(), 4);
    let _ = &r.dir;
}

#[test]
fn dropped_files_are_granted_to_the_window_and_announced_and_closing_forgets() {
    let t = tempfile::tempdir().unwrap();
    let log = t.path().join("a.log");
    std::fs::write(&log, "x").unwrap();
    let r = rig(vec![]);
    let w2 = window(&r.app, "w-2");
    let pop = window(&r.app, "p-w-2-1");
    let dropped = expect_event::<DroppedFiles>(&w2, DROPPED_EVENT);
    let drop = |paths: Vec<PathBuf>| {
        WindowEvent::DragDrop(::tauri::DragDropEvent::Drop {
            paths,
            position: ::tauri::PhysicalPosition::new(0.0, 0.0),
        })
    };
    on_window_event(
        &w2.as_ref().window(),
        &drop(vec![log.clone(), t.path().join("missing")]),
    );
    let got = dropped.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(got.paths, vec![s(&std::fs::canonicalize(&log).unwrap())]);
    assert!(invoke(&w2, "read_it", json!({ "path": s(&log) })).is_ok());
    assert!(
        invoke(&pop, "read_it", json!({ "path": s(&log) })).is_ok(),
        "a pop-out shares its host's grants"
    );
    // Nothing granted, nothing announced.
    on_window_event(&w2.as_ref().window(), &drop(vec![t.path().join("missing")]));
    assert!(dropped.try_recv().is_err());
    // Other events are ignored; a pop-out closing keeps its host's grants; the host closing drops them.
    on_window_event(&w2.as_ref().window(), &WindowEvent::Focused(true));
    on_window_event(&pop.as_ref().window(), &WindowEvent::Destroyed);
    assert!(invoke(&w2, "read_it", json!({ "path": s(&log) })).is_ok());
    on_window_event(&w2.as_ref().window(), &WindowEvent::Destroyed);
    assert!(invoke(&w2, "read_it", json!({ "path": s(&log) })).is_err());
    // The command line grants like a drop.
    let granted = grant_command_line(r.app.handle(), "p-w-2-1", &[s(&log), "not-a-path".into()]);
    assert_eq!(granted.len(), 1);
    assert!(invoke(&w2, "read_it", json!({ "path": s(&log) })).is_ok());
}

#[test]
fn window_grants_cover_reads_writes_dirs_and_the_missing_service() {
    let t = tempfile::tempdir().unwrap();
    let log = t.path().join("a.log");
    std::fs::write(&log, "x").unwrap();
    let r = rig(vec![]);
    let g = WindowGrants::of(r.files.clone(), "d-main-3");
    g.grant(t.path(), Access::Read, GrantSource::Dialog)
        .unwrap();
    assert!(g.read_file(&s(&log), &[]).is_ok());
    assert!(g.read_dir(&s(t.path())).is_ok());
    assert!(
        g.read_dir(&s(&log))
            .unwrap_err()
            .message
            .contains("not a folder")
    );
    assert!(
        g.read_file(&s(t.path()), &[])
            .unwrap_err()
            .message
            .contains("not a file")
    );
    let out = t.path().join("out.csv");
    assert!(g.write(&s(&out), &[]).is_err());
    assert!(g.read_or_write(&s(&out), &[]).is_err());
    g.grant(&out, Access::Write, GrantSource::Dialog).unwrap();
    assert!(g.write(&s(&out), &["csv"]).is_ok());
    assert!(g.read_or_write(&s(&out), &[]).is_ok());
    assert!(files(r.app.handle()).is_some());
    assert!(
        install(r.app.handle(), Files::new(Recents::open(None, &[FILES]))).can_pick(),
        "a second install keeps the first"
    );
    // An app that did not set up files: the argument says so.
    let bare = mock_builder()
        .invoke_handler(::tauri::generate_handler![read_it])
        .build(mock_context(noop_assets()))
        .unwrap();
    let w = window(&bare, "main");
    assert_eq!(
        invoke(&w, "read_it", json!({ "path": s(&log) })).unwrap_err()["code"],
        "unavailable"
    );
    on_window_event(&w.as_ref().window(), &WindowEvent::Destroyed);
}
