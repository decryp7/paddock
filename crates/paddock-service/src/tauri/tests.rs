//! The adapter on Tauri's mock runtime: no window reaches the desktop, the OS is the recording fake, waiting is
//! event-driven (bounded), never a sleep.

use super::*;
use crate::backend::{Target, WindowsRun};
use crate::port::Output;
use crate::spec::ServiceSpec;
use crate::testing::FakeSystem;
use ::tauri::ipc::{CallbackFn, InvokeBody};
use ::tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use ::tauri::webview::InvokeRequest;
use ::tauri::{Listener, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use paddock_health::{ServiceSpec as HealthSpec, ServiceState};
use serde_json::{Value, json};
use std::sync::mpsc;

type App = ::tauri::App<MockRuntime>;

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
    app: App,
    window: WebviewWindow<MockRuntime>,
    sys: FakeSystem,
    _dir: tempfile::TempDir,
}

fn rig(health: Option<&str>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let sys = FakeSystem::new();
    sys.put_program("C:\\D\\dyno-agent.exe");
    sys.answer("reg query", Output::code(1));
    let spec = ServiceSpec::builder("net.decryptology.dyno.agent", "Dyno", "dyno-agent")
        .args(["run"])
        .build()
        .unwrap();
    let b = WindowsRun::new(
        spec,
        Target::exe("C:\\D\\dyno-agent.exe"),
        Box::new(sys.clone()),
    );
    let m = Manager::new(
        "Dyno",
        Box::new(b),
        dir.path(),
        "net.decryptology.dyno.agent",
        "1.0.0",
        || false,
    );
    let app = mock_builder()
        .invoke_handler(invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let h = Health::new(
        vec![HealthSpec::new("agent", "the agent")],
        Box::new(|_, _| {}),
    )
    .unwrap();
    app.manage(h);
    let mut setup = Setup::new(m).checks(Duration::ZERO, Duration::ZERO, 0);
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
        sys,
        _dir: dir,
    }
}

#[test]
fn status_install_and_the_event_reach_every_window_and_health() {
    let r = rig(Some("agent"));
    let (tx, rx) = mpsc::channel::<Value>();
    r.app.listen_any(EVENT, move |e| {
        let _ = tx.send(serde_json::from_str(e.payload()).unwrap());
    });
    let s = invoke(&r.window, "service_status").unwrap();
    assert_eq!(s["condition"], "notInstalled");
    assert_eq!(s["mechanism"], "windowsRun");
    let health = r.app.state::<Arc<Health>>();
    assert_eq!(health.state("agent"), Some(ServiceState::Down));
    assert_eq!(invoke(&r.window, "service_install").unwrap(), json!(true));
    // The install's own update (after the status look's).
    let mut last = Value::Null;
    while last["status"]["repair"]["state"] != json!("succeeded") {
        last = rx
            .recv_timeout(Duration::from_secs(30))
            .expect("the install's update");
    }
    assert!(r.sys.commands().iter().any(|c| c.starts_with("reg add")));
    assert_eq!(
        invoke(&r.window, "open_login_items").unwrap(),
        json!(false),
        "Windows has no Login Items"
    );
    assert_eq!(invoke(&r.window, "service_repair").unwrap(), Value::Null);
    let state = r.app.state::<Arc<Service>>();
    assert!(
        Arc::ptr_eq(
            state.control(),
            install(r.app.handle(), Setup::new(rig_manager())).control()
        ),
        "installed once"
    );
    state.shutdown();
}

fn rig_manager() -> Manager {
    let sys = FakeSystem::new();
    let spec = ServiceSpec::builder("net.a.b", "A", "a").build().unwrap();
    let b = WindowsRun::new(spec, Target::exe("C:\\a.exe"), Box::new(sys));
    Manager::new(
        "A",
        Box::new(b),
        std::path::Path::new("/nonexistent-paddock-test"),
        "net.a.b",
        "1",
        || false,
    )
}

#[test]
fn without_a_declared_health_entry_the_service_still_works() {
    for id in [Some("undeclared"), None] {
        let r = rig(id);
        assert_eq!(
            invoke(&r.window, "service_status").unwrap()["condition"],
            "notInstalled"
        );
        r.app.state::<Arc<Service>>().shutdown();
    }
    // A health id with no registry managed at all.
    let app = mock_builder()
        .invoke_handler(invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let s = install(
        app.handle(),
        Setup::new(rig_manager())
            .health("x")
            .checks(Duration::from_secs(3600), Duration::ZERO, 1),
    );
    s.shutdown();
}

#[test]
fn a_command_without_the_state_is_refused_with_a_code() {
    let app = mock_builder()
        .invoke_handler(invoke_handler())
        .build(mock_context(noop_assets()))
        .unwrap();
    let w = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
        .build()
        .unwrap();
    assert!(invoke(&w, "service_status").is_err());
    assert_eq!(not_set_up().code, ServiceErrorCode::NotSetUp);
}
