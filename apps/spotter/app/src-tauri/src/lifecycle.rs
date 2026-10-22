//! Start-up and the long-running parts: opening the engine, the scheduler, pushing state to the windows and the tray,
//! the OS adapters (toast, opener) and the tray menu's actions. `init` runs in Tauri's `setup`, before any window
//! asks for anything; it never waits for the UI.

use crate::dto::{StatePush, WatchSummaryDto};
use crate::notifier::{OpenLocation, OsToast, ShellNotifier};
use crate::state::{resolve_data_dir, AppState, Shared, DATA_DIR_ENV};
use crate::{applog, credentials, tray};
use spotter_engine::engine::Change;
use spotter_engine::{Engine, EngineConfig, Scheduler, Services};
use spotter_render::ChromiumRenderer;
use spotter_sources::http::Http;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

/// The event the UI listens to for the watch list.
pub const STATE_EVENT: &str = "spotter://state";

/// Notifications through the OS.
struct TauriToast<R: Runtime>(AppHandle<R>);

impl<R: Runtime> OsToast for TauriToast<R> {
    fn show(&self, title: &str, body: &str) {
        if let Err(e) = self.0.notification().builder().title(title).body(body).show() {
            applog::write(&format!("a notification could not be shown: {e}"));
        }
    }
}

/// Opening locations with the system's handler.
struct TauriOpen<R: Runtime>(AppHandle<R>);

impl<R: Runtime> OpenLocation for TauriOpen<R> {
    fn open(&self, location: &str) -> Result<(), String> {
        if !crate::notifier::openable(location) {
            return Err("This release's location is not something Spotter can open.".into());
        }
        let result = if location.starts_with("http://") || location.starts_with("https://") {
            self.0.opener().open_url(location, None::<&str>)
        } else {
            self.0.opener().open_path(location, None::<&str>)
        };
        result.map_err(|e| format!("The location could not be opened: {e}"))
    }
}

/// Opens the location of a release (shared by the command and the toast actions).
pub fn opener<R: Runtime>(app: &AppHandle<R>) -> Arc<dyn OpenLocation> {
    Arc::new(TauriOpen(app.clone()))
}

fn local_offset_minutes() -> i32 {
    use chrono::Offset;
    chrono::Local::now().offset().fix().local_minus_utc() / 60
}

/// Opens the engine and starts its background work; returns the shared state.
pub fn init<R: Runtime>(app: &AppHandle<R>) -> Result<Shared, String> {
    let default_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("The data folder is not known: {e}"))?;
    let data_dir = resolve_data_dir(std::env::var(DATA_DIR_ENV).ok().as_deref(), default_dir);
    let default_downloads = app
        .path()
        .download_dir()
        .map(|d| d.join("Spotter"))
        .unwrap_or_else(|_| data_dir.join("downloads"));
    let version = app.package_info().version.to_string();
    let http = Http::new(&version)?;

    let notifier = ShellNotifier::new(
        Arc::new(TauriToast(app.clone())),
        opener(app),
        http.clone(),
        default_downloads.clone(),
    );
    let clock = Arc::new(spotter_engine::clock::SystemClock::new());
    clock.set_utc_offset_minutes(local_offset_minutes());
    let mut services = Services::new(Arc::new(credentials::KeyringSecrets));
    services.clock = clock.clone();
    services.notifier = Arc::new(notifier.clone());
    services.renderer = Some(Arc::new(ChromiumRenderer::new(&data_dir, data_dir.join("cache").join("render"))));
    let engine = Engine::open(EngineConfig::new(&data_dir, &version), services).map_err(|e| e.to_string())?;
    notifier.attach(engine.clone());
    for w in engine.warnings() {
        applog::write(&format!("start-up: {w}"));
    }

    let (stop, stopped) = tokio::sync::watch::channel(false);
    let state: Shared = Arc::new(AppState {
        engine: engine.clone(),
        data_dir,
        default_downloads,
        http,
        stop,
    });

    // The scheduler, the push of state and the daylight-saving refresh run for as long as the app does.
    let scheduler = Scheduler::new(engine.clone());
    let run_stop = stopped.clone();
    tauri::async_runtime::spawn(async move { scheduler.run(run_stop).await });

    let (handle, rx) = (app.clone(), engine.subscribe());
    let engine_for_push = engine.clone();
    tauri::async_runtime::spawn(async move { forward_changes(handle, engine_for_push, rx).await });

    let mut tick_stop = stopped;
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(3600)) => clock.set_utc_offset_minutes(local_offset_minutes()),
                _ = tick_stop.changed() => break,
            }
        }
    });
    Ok(state)
}

/// Pushes every change of the engine to the windows and the tray.
async fn forward_changes<R: Runtime>(app: AppHandle<R>, engine: Engine, mut rx: tokio::sync::broadcast::Receiver<Change>) {
    use tokio::sync::broadcast::error::RecvError;
    while let Ok(_) | Err(RecvError::Lagged(_)) = rx.recv().await {
        push_state(&app, &engine);
    }
}

/// Emits the current watches to the windows and refreshes the tray.
pub fn push_state<R: Runtime>(app: &AppHandle<R>, engine: &Engine) {
    let watches = engine.list_watches();
    let payload = StatePush {
        watches: watches.iter().map(WatchSummaryDto::from).collect(),
    };
    let _ = app.emit(STATE_EVENT, payload);
    tray::update(app, &watches, notifications_paused(engine));
}

/// Whether notifications are paused right now.
pub fn notifications_paused(engine: &Engine) -> bool {
    let until = engine.settings().notifications_paused_until;
    until.is_some_and(|ms| ms > now_ms())
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// A click on a tray menu item.
pub fn on_menu<R: Runtime>(app: &AppHandle<R>, id: &str) {
    let Some(state) = app.try_state::<Shared>() else { return };
    let engine = state.engine.clone();
    match id {
        "open" => tray::show_main(app, None),
        "quit" => app.exit(0),
        "check-all" => {
            tauri::async_runtime::spawn(async move {
                let _ = engine.check_now(None).await;
            });
        }
        "pause" => {
            let _ = engine.pause_notifications(Some(60));
        }
        "resume" => {
            let _ = engine.pause_notifications(None);
        }
        other => {
            if let Some(watch) = tray::watch_of_item(other) {
                let route = format!("/watches/{watch}");
                if tray::valid_route(&route) {
                    tray::show_main(app, Some(&route));
                }
            }
        }
    }
}

/// Stops the background tasks (at exit).
pub fn stop(app: &AppHandle<impl Runtime>) {
    if let Some(s) = app.try_state::<Shared>() {
        let _ = s.stop.send(true);
    }
}
