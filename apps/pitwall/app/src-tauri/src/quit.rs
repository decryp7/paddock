//! Quitting Pitwall (ADR 0021). The service keeps serving unless the user chose otherwise: the setting "When you quit
//! Pitwall" is keep serving (default), stop serving, or ask each time. Stopping serving asks the server to unload every
//! model, end every engine process (gracefully, then killed) and pause until Pitwall is opened again or serving is
//! resumed; the service itself stays registered. The decisions are pure; Tauri glue is at the bottom.

use crate::commands::{forward, SharedClient};
use crate::server_client::ServerClient;
use pitwall_core::config::OnQuit;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// The event that asks the open window to show the "When you quit" dialog.
pub const ASK_EVENT: &str = "pitwall://quit-ask";

/// What quitting does about serving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitChoice {
    /// Leave the service serving.
    KeepServing,
    /// Stop serving first.
    StopServing,
}

/// What to do when the user quits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// Quit now, doing this about serving.
    Quit(QuitChoice),
    /// Ask the user first.
    Ask,
}

/// The decision for a setting.
pub fn plan(on_quit: OnQuit) -> Plan {
    match on_quit {
        OnQuit::KeepServing => Plan::Quit(QuitChoice::KeepServing),
        OnQuit::StopServing => Plan::Quit(QuitChoice::StopServing),
        OnQuit::Ask => Plan::Ask,
    }
}

/// The setting in the server's `settings` answer; anything unreadable is the default (keep serving).
pub fn on_quit_of(settings: &Value) -> OnQuit {
    settings
        .pointer("/general/onQuit")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// The settings with "remember my choice" applied.
pub fn remember(mut settings: Value, choice: QuitChoice) -> Value {
    let value = match choice {
        QuitChoice::KeepServing => OnQuit::KeepServing,
        QuitChoice::StopServing => OnQuit::StopServing,
    };
    if let Some(general) = settings.get_mut("general").and_then(Value::as_object_mut) {
        general.insert("onQuit".into(), json!(value));
    }
    settings
}

/// Reads the setting (an unreachable server reads as the default, so quitting always works).
pub async fn read_plan(client: &dyn ServerClient) -> Plan {
    match forward(client, "settings", Value::Null).await {
        Ok(s) => plan(on_quit_of(&s)),
        Err(_) => Plan::Quit(QuitChoice::KeepServing),
    }
}

/// Carries out a choice on the server: optionally remembers it, and stops serving when asked. Failures never keep the
/// app from quitting (an unreachable server has nothing to stop); they are returned for the log.
pub async fn apply(client: &dyn ServerClient, choice: QuitChoice, remember_choice: bool) -> Vec<String> {
    let mut problems = vec![];
    if remember_choice {
        match forward(client, "settings", Value::Null).await {
            Ok(s) => {
                if let Err(e) = forward(client, "save_settings", json!({ "settings": remember(s, choice) })).await {
                    problems.push(format!("could not remember the choice: {}", e.message));
                }
            }
            Err(e) => problems.push(format!("could not remember the choice: {}", e.message)),
        }
    }
    if choice == QuitChoice::StopServing {
        if let Err(e) = forward(client, "stop_serving", Value::Null).await {
            problems.push(format!("could not stop serving: {}", e.message));
        }
    }
    problems
}

/// Whether the window should show the dialog as soon as it can (the quit was asked while no window was open).
#[derive(Default)]
pub struct PendingAsk(AtomicBool);

impl PendingAsk {
    /// Records that the dialog is wanted.
    pub fn set(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    /// Takes the request (once).
    pub fn take(&self) -> bool {
        self.0.swap(false, Ordering::SeqCst)
    }
}

/// How long a quit may take before the watchdog ends the process: the server's own bound for stopping serving
/// (`stop_serving` times out at 30 s in `commands::timeout_for`) plus time to close the windows.
pub const EXIT_STALL_LIMIT: std::time::Duration = std::time::Duration::from_secs(40);

/// Ends a quit that stalls (an engine that never stops, a webview that never closes): after `limit` without the
/// process having exited, calls `end(code)`. The engines belong to the service, which keeps running, so ending the
/// process loses nothing. Starts a thread; returns whether it was started.
fn spawn_exit_watchdog(limit: std::time::Duration, code: i32, end: impl FnOnce(i32) + Send + 'static) -> bool {
    let spawned = std::thread::Builder::new().name("pitwall-exit-watchdog".into()).spawn(move || {
        std::thread::sleep(limit);
        eprintln!("Pitwall: quitting stalled for {} s; ending the process", limit.as_secs());
        end(code);
    });
    spawned.is_ok()
}

/// Arms the exit watchdog once per process, at the start of a quit.
pub fn arm_exit_watchdog(code: i32) {
    static ARMED: AtomicBool = AtomicBool::new(false);
    if !ARMED.swap(true, Ordering::SeqCst) {
        spawn_exit_watchdog(EXIT_STALL_LIMIT, code, |code| std::process::exit(code));
    }
}

/// The user asked to quit with `forced` (the "Quit and stop serving" items) or by the setting (`None`).
pub async fn request<R: Runtime>(app: &AppHandle<R>, forced: Option<QuitChoice>) {
    let client: SharedClient = app.state::<SharedClient>().inner().clone();
    let plan = match forced {
        Some(c) => Plan::Quit(c),
        None => read_plan(client.as_ref()).await,
    };
    match plan {
        Plan::Quit(choice) => {
            arm_exit_watchdog(0);
            for p in apply(client.as_ref(), choice, false).await {
                eprintln!("Pitwall: {p}");
            }
            app.exit(0);
        }
        Plan::Ask => {
            // The dialog lives in the window: open (or focus) it, and tell it, now and when it loads.
            crate::shell::handle(app, crate::window::Lifecycle::TrayOpen);
            app.state::<PendingAsk>().set();
            let _ = app.emit(ASK_EVENT, ());
        }
    }
}

/// The user's answer in the "When you quit" dialog: quit now, stopping serving or not, optionally remembering it.
#[tauri::command]
pub async fn quit_app<R: Runtime>(
    app: AppHandle<R>,
    client: tauri::State<'_, SharedClient>,
    stop_serving: bool,
    remember_choice: bool,
) -> Result<(), pitwall_core::mgmt::CommandError> {
    let choice = if stop_serving {
        QuitChoice::StopServing
    } else {
        QuitChoice::KeepServing
    };
    arm_exit_watchdog(0);
    for p in apply(client.as_ref(), choice, remember_choice).await {
        eprintln!("Pitwall: {p}");
    }
    app.exit(0);
    Ok(())
}

/// Whether a quit is waiting for the dialog (it was asked while no window was open): true once.
#[tauri::command]
pub async fn take_quit_ask(pending: tauri::State<'_, PendingAsk>) -> Result<bool, pitwall_core::mgmt::CommandError> {
    Ok(pending.take())
}

/// After Pitwall was quit with "stop serving", opening it again resumes serving. Called when the app starts.
pub async fn resume_if_stopped(client: &dyn ServerClient) {
    if let Ok(o) = forward(client, "overview", Value::Null).await {
        if o.pointer("/server/serving").and_then(Value::as_str) == Some("stopped") {
            let _ = forward(client, "set_serving", json!({ "paused": false })).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server_client::fake::{FakeServer, Reply};
    use pitwall_core::mgmt::{CommandError, ErrorCode};

    #[tokio::test]
    async fn a_quit_whose_engine_never_stops_is_ended_by_the_watchdog() {
        use crate::server_client::BoxFuture;
        // A server whose stop never answers (an engine that will not die); the call's own timeout is far away.
        struct NeverStops;
        impl ServerClient for NeverStops {
            fn call<'a>(&'a self, _: &'a str, _: Value, _: std::time::Duration) -> BoxFuture<'a, Result<Value, CommandError>> {
                Box::pin(std::future::pending())
            }
            fn open_stream<'a>(
                &'a self,
                _: &'a str,
                _: Option<Value>,
                _: std::time::Duration,
            ) -> BoxFuture<'a, Result<Box<dyn crate::server_client::FrameStream>, CommandError>> {
                Box::pin(std::future::pending())
            }
        }
        let (tx, rx) = std::sync::mpsc::channel();
        assert!(spawn_exit_watchdog(std::time::Duration::from_millis(20), 7, move |code| {
            let _ = tx.send(code);
        }));
        // The quit itself hangs in the stop, as it did before the watchdog existed...
        let quit = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            apply(&NeverStops, QuitChoice::StopServing, false),
        )
        .await;
        assert!(quit.is_err(), "the stop never finishes");
        // ...and the watchdog has ended the process with the quit's exit code.
        assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(10)), Ok(7));
    }

    #[test]
    fn the_setting_decides_and_defaults_to_keep_serving() {
        assert_eq!(plan(OnQuit::KeepServing), Plan::Quit(QuitChoice::KeepServing));
        assert_eq!(plan(OnQuit::StopServing), Plan::Quit(QuitChoice::StopServing));
        assert_eq!(plan(OnQuit::Ask), Plan::Ask);
        assert_eq!(on_quit_of(&json!({"general": {"onQuit": "stopServing"}})), OnQuit::StopServing);
        assert_eq!(on_quit_of(&json!({"general": {"onQuit": "ask"}})), OnQuit::Ask);
        for missing in [
            json!({}),
            json!({"general": {}}),
            json!({"general": {"onQuit": "explode"}}),
            json!(null),
        ] {
            assert_eq!(on_quit_of(&missing), OnQuit::KeepServing, "{missing}");
        }
    }

    #[test]
    fn remembering_changes_only_the_quit_setting() {
        let s = json!({"general": {"theme": "dark", "onQuit": "ask"}, "server": {"port": 1}});
        let r = remember(s, QuitChoice::StopServing);
        assert_eq!(
            r,
            json!({"general": {"theme": "dark", "onQuit": "stopServing"}, "server": {"port": 1}})
        );
        assert_eq!(remember(json!({}), QuitChoice::KeepServing), json!({}));
    }

    #[tokio::test]
    async fn stopping_asks_the_server_and_remembering_saves_the_setting() {
        let f = FakeServer::new();
        f.reply("settings", Reply::Ok(json!({"general": {"onQuit": "ask"}})));
        assert!(apply(&f, QuitChoice::StopServing, true).await.is_empty());
        let calls = f.calls();
        let names: Vec<&str> = calls.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(names, ["settings", "save_settings", "stop_serving"], "remember first, then stop");
        assert_eq!(calls[1].1["settings"]["general"]["onQuit"], "stopServing");

        let f = FakeServer::new();
        assert!(apply(&f, QuitChoice::KeepServing, false).await.is_empty());
        assert!(f.calls().is_empty(), "keeping serving touches nothing");
    }

    #[tokio::test]
    async fn an_unreachable_server_never_keeps_the_app_from_quitting() {
        let down = CommandError::new(ErrorCode::ServerUnavailable, "not running");
        let f = FakeServer::new();
        f.reply("settings", Reply::Err(down.clone()))
            .reply("stop_serving", Reply::Err(down));
        assert_eq!(read_plan(&f).await, Plan::Quit(QuitChoice::KeepServing));
        let problems = apply(&f, QuitChoice::StopServing, true).await;
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems.iter().all(|p| p.contains("not running")));
    }

    #[tokio::test]
    async fn opening_pitwall_resumes_only_a_stopped_server() {
        let f = FakeServer::new();
        f.reply("overview", Reply::Ok(json!({"server": {"serving": "stopped"}})));
        resume_if_stopped(&f).await;
        assert_eq!(f.calls().last().unwrap(), &("set_serving".to_string(), json!({"paused": false})));

        let f = FakeServer::new();
        f.reply("overview", Reply::Ok(json!({"server": {"serving": "paused"}})));
        resume_if_stopped(&f).await;
        assert_eq!(f.calls().len(), 1, "a pause the user chose stays");
    }

    #[test]
    fn the_ask_request_is_taken_once() {
        let p = PendingAsk::default();
        assert!(!p.take());
        p.set();
        assert!(p.take());
        assert!(!p.take());
    }
}
