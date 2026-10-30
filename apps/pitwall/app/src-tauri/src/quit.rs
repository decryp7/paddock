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

/// Starts quitting: arms the shell's exit watchdog first (an engine that never stops would otherwise hang the quit in
/// `apply`; the watchdog ends the process after `crate::EXIT_STALL_LIMIT`, and the engines belong to the service, which
/// keeps running, so nothing is lost), applies the choice about serving, then quits through the window layer.
async fn quit_now<R: Runtime>(app: &AppHandle<R>, client: &dyn ServerClient, choice: QuitChoice, remember_choice: bool) {
    paddock_window::tauri::arm_exit_watchdog(app, 0);
    for p in apply(client, choice, remember_choice).await {
        eprintln!("Pitwall: {p}");
    }
    paddock_window::tauri::quit(app);
}

/// The user asked to quit with `forced` (the "Quit and stop serving" items) or by the setting (`None`).
pub async fn request<R: Runtime>(app: &AppHandle<R>, forced: Option<QuitChoice>) {
    let client: SharedClient = app.state::<SharedClient>().inner().clone();
    let plan = match forced {
        Some(c) => Plan::Quit(c),
        None => read_plan(client.as_ref()).await,
    };
    match plan {
        Plan::Quit(choice) => quit_now(app, client.as_ref(), choice, false).await,
        Plan::Ask => {
            // The dialog lives in the window: open (or focus) it, and tell it, now and when it loads. A cancelled question
            // never reaches the window layer, so nothing is left half-quitting.
            if let Err(e) = paddock_window::tauri::reveal(app, "main") {
                eprintln!("Pitwall: the window could not be shown to ask about quitting ({})", e.message);
            }
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
    quit_now(&app, client.as_ref(), choice, remember_choice).await;
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
