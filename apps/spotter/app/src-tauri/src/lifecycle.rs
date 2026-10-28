//! Start-up and the long-running parts: opening the engine, the scheduler, pushing state to the windows and the tray,
//! the OS adapters (toast, opener) and the tray menu's actions. `init` runs in Tauri's `setup`, before any window
//! asks for anything; it never waits for the UI.

use crate::dto::{StatePush, WatchSummaryDto};
use crate::health::Hub;
use crate::notifier::{OpenLocation, OsToast, ShellNotifier};
use crate::state::{resolve_data_dir, AppState, Shared, DATA_DIR_ENV};
use crate::{applog, credentials, toast, tray};
use spotter_engine::engine::Change;
use spotter_engine::notifier::NotificationKind;
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

    fn show_for_watch(&self, title: &str, body: &str, kind: NotificationKind, watch_id: &str) {
        let mut toast = self
            .0
            .notification()
            .builder()
            .title(title)
            .body(body)
            .extra(toast::WATCH_KEY, watch_id);
        if let Some(actions) = toast::action_type_for(kind, Some(watch_id)) {
            toast = toast.action_type_id(actions);
        }
        if let Err(e) = toast.show() {
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

/// Tells the clock the machine's current UTC offset and time zone name (schedules follow daylight saving by name).
fn refresh_local_time(clock: &spotter_engine::clock::SystemClock) {
    clock.set_utc_offset_minutes(local_offset_minutes());
    clock.set_local_zone_name(iana_time_zone::get_timezone().ok());
}

/// Opens the engine and starts its background work; returns the shared state.
pub fn init<R: Runtime>(app: &AppHandle<R>, hub: &Hub<R>) -> Result<Shared, String> {
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

    let downloader = paddock_download::tauri::downloader(app).ok_or("The shell's downloader is not set up.")?;
    let registry = crate::jobs::registry(app).ok_or("The shell's jobs are not set up.")?;
    let downloads = crate::jobs::Downloads::new(registry, downloader.clone(), http.clone());
    let notifier = ShellNotifier::new(
        Arc::new(TauriToast(app.clone())),
        opener(app),
        downloads.clone(),
        default_downloads.clone(),
    );
    // Before the first toast: the plugin shows buttons only for toasts that are shown after a handler exists.
    toast::register(app);
    let clock = Arc::new(spotter_engine::clock::SystemClock::new());
    refresh_local_time(&clock);
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
        downloader,
        downloads,
        last_test: std::sync::Mutex::new(None),
        pending_route: std::sync::Mutex::new(None),
        stop,
    });

    // The scheduler, the push of state and the daylight-saving refresh run for as long as the app does.
    // The scheduler runs under the shell's service health: when its task dies the chip says so and Restart runs it again.
    let (scheduled, run_stop) = (engine.clone(), stopped.clone());
    let supervisor = hub.clone();
    tauri::async_runtime::spawn(async move {
        supervisor
            .supervise(move || {
                let (scheduler, stop) = (Scheduler::new(scheduled.clone()), run_stop.clone());
                async move { scheduler.run(stop).await }
            })
            .await;
    });

    let (handle, rx) = (app.clone(), engine.subscribe());
    let engine_for_push = engine.clone();
    let reporter = hub.clone();
    tauri::async_runtime::spawn(async move { forward_changes(handle, engine_for_push, reporter, rx).await });

    let mut tick_stop = stopped;
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(3600)) => refresh_local_time(&clock),
                _ = tick_stop.changed() => break,
            }
        }
    });
    Ok(state)
}

/// Pushes every change of the engine to the windows and the tray.
async fn forward_changes<R: Runtime>(app: AppHandle<R>, engine: Engine, hub: Hub<R>, mut rx: tokio::sync::broadcast::Receiver<Change>) {
    use tokio::sync::broadcast::error::RecvError;
    while let Ok(_) | Err(RecvError::Lagged(_)) = rx.recv().await {
        let watches = push_state(&app, &engine);
        hub.report_sources(&watches);
        hub.report_renderer(&watches);
    }
}

/// Emits the current watches to the windows and refreshes the tray; returns the watches.
pub fn push_state<R: Runtime>(app: &AppHandle<R>, engine: &Engine) -> Vec<spotter_engine::view::WatchSummary> {
    let watches = engine.list_watches();
    let payload = StatePush {
        watches: watches.iter().map(WatchSummaryDto::from).collect(),
    };
    let _ = app.emit(STATE_EVENT, payload);
    tray::update(app);
    watches
}

/// The watches and whether notifications are paused, for the tray's model (empty before the engine is open).
pub fn snapshot<R: Runtime>(app: &AppHandle<R>) -> (Vec<spotter_engine::view::WatchSummary>, bool) {
    match app.try_state::<Shared>() {
        Some(s) => (s.engine.list_watches(), notifications_paused(&s.engine)),
        None => (Vec::new(), false),
    }
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

/// Quit's teardown (a bounded step of the shell's exit): stops the background tasks, asks every running job to stop
/// and ends any browser a render still holds, so a stuck render can neither hang Quit nor outlive the app.
pub fn stop<R: Runtime>(app: &AppHandle<R>) {
    if let Some(s) = app.try_state::<Shared>() {
        let _ = s.stop.send(true);
    }
    if let Some(registry) = crate::jobs::registry(app) {
        crate::jobs::cancel_all(&registry);
    }
    spotter_render::process::end_all();
}

#[cfg(test)]
mod tests {
    use super::*;
    use spotter_engine::clock::{Clock, SystemClock};

    #[test]
    fn the_clock_learns_the_machines_offset_and_zone() {
        let clock = SystemClock::new();
        assert_eq!(clock.utc_offset_minutes(), 0, "UTC until the shell says otherwise");
        refresh_local_time(&clock);
        // Whatever zone this machine is in, the offset is a real one and agrees with the helper the shell uses.
        assert_eq!(clock.utc_offset_minutes(), local_offset_minutes());
        assert!((-14 * 60..=14 * 60).contains(&clock.utc_offset_minutes()));
        if let Some(name) = clock.local_zone_name() {
            assert!(!name.is_empty() && name.len() < 64, "{name}");
        }
    }
}
