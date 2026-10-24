//! The adapter on Tauri's mock runtime: no window reaches the desktop, waiting is event-driven (bounded), never a sleep.

use super::*;
use crate::view::kind;
use crate::{BuiltinPack, Limits, Origin, PackError, PackKind, PackLoader, PackSource, PackStore};
use ::tauri::ipc::{CallbackFn, InvokeBody};
use ::tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use ::tauri::webview::InvokeRequest;
use ::tauri::{Listener, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use paddock_health::{ServiceSpec as HealthSpec, ServiceState};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io;
use std::sync::mpsc;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

struct Lines;
#[derive(serde::Deserialize)]
struct LinesRaw {
    #[serde(default)]
    line: Vec<String>,
}
impl PackKind for Lines {
    type Raw = LinesRaw;
    type Pack = Vec<String>;
    const FORMAT: u32 = 1;
    fn convert(raw: LinesRaw, _: Origin) -> Result<Vec<String>, PackError> {
        Ok(raw.line)
    }
}

#[derive(Clone, Default)]
struct Mem(std::sync::Arc<Mutex<BTreeMap<String, String>>>);
impl Mem {
    fn put(&self, name: &str, text: &str) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(name.into(), text.into());
    }
}
impl PackSource for Mem {
    fn list(&self) -> io::Result<Vec<String>> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .keys()
            .cloned()
            .collect())
    }
    fn read(&self, name: &str, _max: u64) -> io::Result<Vec<u8>> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(name)
            .map(|s| s.as_bytes().to_vec())
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }
}

fn invoke(w: &WebviewWindow<MockRuntime>, cmd: &str) -> Result<Value, Value> {
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
        body: InvokeBody::Json(json!({})),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(w, request).map(|b| b.deserialize::<Value>().unwrap())
}

struct Rig {
    app: ::tauri::App<MockRuntime>,
    window: WebviewWindow<MockRuntime>,
    src: Mem,
    store: Arc<PackStore<Lines>>,
}

fn rig(health: Option<&str>) -> Rig {
    let src = Mem::default();
    src.put("good.toml", "format = 1\nline = ['a']\n");
    let loader = PackLoader::<Lines>::new(
        vec![BuiltinPack {
            name: "builtin.toml",
            text: "format = 1\nline = ['b']\n",
        }],
        Box::new(src.clone()),
        Limits::default(),
    );
    let store = Arc::new(PackStore::new(loader));
    store.reload();
    let app = mock_builder()
        .invoke_handler(invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let h = Registry::new(
        vec![HealthSpec::new("packs", "the knowledge packs")],
        Box::new(|_, _| {}),
    )
    .unwrap();
    app.manage(h);
    let mut setup = Setup::new().kind(kind("lines", "Line packs", store.clone()));
    if let Some(id) = health {
        setup = setup.health(id);
    }
    install(app.handle(), setup);
    let window = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
        .build()
        .unwrap();
    Rig {
        app,
        window,
        src,
        store,
    }
}

#[test]
fn status_lists_every_kind_with_its_packs_and_health() {
    let r = rig(Some("packs"));
    let v = invoke(&r.window, "packs_status").unwrap();
    let k = &v["kinds"][0];
    assert_eq!(
        (k["id"].clone(), k["label"].clone()),
        (json!("lines"), json!("Line packs"))
    );
    assert_eq!(
        k["packs"],
        json!([{"name": "builtin.toml", "origin": "builtin"}, {"name": "good.toml", "origin": "user"}])
    );
    assert_eq!(k["health"], json!({"level": "ok"}));
    assert_eq!(
        r.app.state::<Arc<Registry>>().state("packs"),
        Some(ServiceState::Ok)
    );
}

#[test]
fn a_change_goes_to_every_window_and_health_goes_down_then_the_repair_reloads() {
    let r = rig(Some("packs"));
    let (tx, rx) = mpsc::channel::<Value>();
    r.app.listen_any(EVENT, move |e| {
        let _ = tx.send(serde_json::from_str(e.payload()).unwrap());
    });
    r.src.put("bad.toml", "format = 1\nline = 5\n");
    assert_eq!(
        invoke(&r.window, "packs_reload").unwrap()["kinds"][0]["problems"][0]["file"],
        json!("bad.toml")
    );
    let ev = rx
        .recv_timeout(Duration::from_secs(30))
        .expect("the change event");
    assert_eq!(ev["kinds"][0]["health"]["level"], json!("degraded"));
    let health = r.app.state::<Arc<Registry>>();
    assert_eq!(health.state("packs"), Some(ServiceState::Down));
    // The repair is a reload: still broken, so still down with the cause.
    health.begin_repair("packs").unwrap();
    health.finish_repair("packs");
    // Fixing the file, then the repair (run by the shell, not through the pack) clears it.
    r.src.put("bad.toml", "format = 1\nline = ['ok']\n");
    let packs = r.app.state::<Arc<Packs>>();
    assert!(packs.reload());
    assert_eq!(health.state("packs"), Some(ServiceState::Ok));
    assert_eq!(r.store.current().problems.len(), 0);
}

fn settle(health: &Registry) -> Option<ServiceState> {
    // The repair runs on its own thread: wait for it to leave `repairing` (bounded by yields, not time).
    for _ in 0..2_000_000 {
        let s = health.state("packs");
        if s != Some(ServiceState::Repairing) {
            return s;
        }
        std::thread::yield_now();
    }
    health.state("packs")
}

#[test]
fn the_repair_closure_reloads_and_reports_what_is_still_wrong() {
    let r = rig(Some("packs"));
    let health = r.app.state::<Arc<Registry>>().inner().clone();
    r.src.put("bad.toml", "nope");
    assert!(r.app.state::<Arc<Packs>>().reload());
    assert_eq!(health.state("packs"), Some(ServiceState::Down));
    // The repair reads the folder again (there is no watcher here), finds the pack still bad and stays down.
    health.repair("packs").unwrap();
    assert_eq!(settle(&health), Some(ServiceState::Down));
    // The user fixes the file; the repair finds nothing wrong any more.
    r.src.put("bad.toml", "format = 1\n");
    health.repair("packs").unwrap();
    assert_eq!(settle(&health), Some(ServiceState::Ok));
}

#[test]
fn without_a_health_entry_or_registry_the_packs_still_work_and_install_is_once() {
    let r = rig(None);
    assert_eq!(
        invoke(&r.window, "packs_status").unwrap()["kinds"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let again = install(r.app.handle(), Setup::new());
    assert_eq!(again.view().kinds.len(), 1, "installed once");

    // An undeclared id and no registry at all are logged, not fatal.
    for (id, with_registry) in [("missing", true), ("packs", false)] {
        let app = mock_builder().build(mock_context(noop_assets())).unwrap();
        if with_registry {
            app.manage(
                Registry::new(vec![HealthSpec::new("packs", "p")], Box::new(|_, _| {})).unwrap(),
            );
        }
        let p = install(app.handle(), Setup::new().health(id));
        assert!(p.view().kinds.is_empty());
        assert!(!p.reload());
    }
}

#[test]
fn health_only_goes_down_for_a_refused_pack_not_for_notes() {
    let r = rig(Some("packs"));
    r.src.put("noted.toml", "format = 1\nextra = 1\n");
    let v = invoke(&r.window, "packs_reload").unwrap();
    assert_eq!(v["kinds"][0]["health"]["level"], json!("notes"));
    assert_eq!(
        r.app.state::<Arc<Registry>>().state("packs"),
        Some(ServiceState::Ok)
    );
}
