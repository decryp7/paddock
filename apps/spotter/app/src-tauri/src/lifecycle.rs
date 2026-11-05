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

/// What [`init`] takes from the machine: the installed app's folders, toasts and log; a test's keep to its folder, show
/// no toast and write nothing to the user's log (rule 7).
#[derive(Debug, Clone)]
pub struct Seams {
    /// The data folder; `None`: `SPOTTER_DATA_DIR` or the app's data folder.
    pub data_dir: Option<std::path::PathBuf>,
    /// Whether toasts go to the OS (its notification plugin).
    pub os_toasts: bool,
    /// Where the start-up notes go (Spotter's log when installed).
    pub note: fn(&str),
    /// Where the watches' secrets live.
    pub secrets: SecretsIn,
}

/// Where the watches' secrets live: the OS credential store (the installed app) or memory (a test: never the user's
/// keychain).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretsIn {
    /// The OS credential store.
    Os,
    /// Memory, for the run only.
    Memory,
}

impl SecretsIn {
    fn store(self) -> Arc<dyn spotter_engine::secrets::SecretStore> {
        match self {
            SecretsIn::Os => Arc::new(credentials::KeyringSecrets),
            SecretsIn::Memory => Arc::new(spotter_engine::secrets::MemorySecrets::new()),
        }
    }
}

impl Seams {
    /// The installed app's.
    pub fn installed() -> Seams {
        Seams {
            data_dir: None,
            os_toasts: true,
            note: applog::write,
            secrets: SecretsIn::Os,
        }
    }

    /// A test's: data in `dir`, no OS toasts, notes to the `log` facade only, secrets in memory.
    pub fn test(dir: &std::path::Path) -> Seams {
        Seams {
            data_dir: Some(dir.join("data")),
            os_toasts: false,
            note: |m| log::info!("{m}"),
            secrets: SecretsIn::Memory,
        }
    }
}

/// Toasts that show nothing (a test).
struct NoToast;

impl OsToast for NoToast {
    fn show(&self, _: &str, _: &str) {}
}

/// Opens the engine and starts its background work; returns the shared state.
pub fn init<R: Runtime>(app: &AppHandle<R>, hub: &Hub<R>, seams: &Seams) -> Result<Shared, String> {
    let started = std::time::Instant::now();
    let data_dir = match &seams.data_dir {
        Some(dir) => dir.clone(),
        None => {
            let default_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("The data folder is not known: {e}"))?;
            resolve_data_dir(std::env::var(DATA_DIR_ENV).ok().as_deref(), default_dir)
        }
    };
    // A test's downloads stay in its folder; the installed app's go to the user's Downloads.
    let default_downloads = match &seams.data_dir {
        Some(_) => data_dir.join("downloads"),
        None => app
            .path()
            .download_dir()
            .map(|d| d.join("Spotter"))
            .unwrap_or_else(|_| data_dir.join("downloads")),
    };
    let version = app.package_info().version.to_string();
    // The metadata client takes the proxy decision the shell's downloader was built with.
    let http = match app.try_state::<crate::ProxyChoice>() {
        Some(choice) => Http::with_proxy(&version, choice.0.clone())?,
        None => Http::new(&version)?,
    };

    let downloader = paddock_download::tauri::downloader(app).ok_or("The shell's downloader is not set up.")?;
    let registry = crate::jobs::registry(app).ok_or("The shell's jobs are not set up.")?;
    let downloads = crate::jobs::Downloads::new(registry, downloader.clone(), http.clone());
    let notifier = ShellNotifier::new(
        if seams.os_toasts {
            Arc::new(TauriToast(app.clone())) as Arc<dyn OsToast>
        } else {
            Arc::new(NoToast)
        },
        opener(app),
        downloads.clone(),
        default_downloads.clone(),
    );
    // Before the first toast: the plugin shows buttons only for toasts that are shown after a handler exists.
    if seams.os_toasts {
        toast::register(app);
    }
    let clock = Arc::new(spotter_engine::clock::SystemClock::new());
    refresh_local_time(&clock);
    let mut services = Services::new(seams.secrets.store());
    services.clock = clock.clone();
    services.notifier = Arc::new(notifier.clone());
    services.renderer = Some(Arc::new(ChromiumRenderer::new(&data_dir, data_dir.join("cache").join("render"))));
    let opened = std::time::Instant::now();
    let engine = Engine::open(EngineConfig::new(&data_dir, &version), services).map_err(|e| {
        log::error!(
            "start: the engine could not open its data after {} ms: {e}; next: check that the data folder is writable, or report a problem",
            opened.elapsed().as_millis()
        );
        e.to_string()
    })?;
    log::info!(
        "start: engine opened in {} ms ({} watches, {} warnings)",
        opened.elapsed().as_millis(),
        engine.list_watches().len(),
        engine.warnings().len()
    );
    notifier.attach(engine.clone());
    for w in engine.warnings() {
        (seams.note)(&format!("start-up: {w}"));
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
    log::info!(
        "start: background work running (scheduler, change push) after {} ms",
        started.elapsed().as_millis()
    );
    Ok(state)
}

/// Pushes every change of the engine to the windows and the tray.
async fn forward_changes<R: Runtime>(app: AppHandle<R>, engine: Engine, hub: Hub<R>, mut rx: tokio::sync::broadcast::Receiver<Change>) {
    use tokio::sync::broadcast::error::RecvError;
    while let Ok(_) | Err(RecvError::Lagged(_)) = rx.recv().await {
        let watches = push_state(&app, &engine);
        hub.report_sources(&watches);
        hub.report_renderer();
    }
}

/// Emits the current watches to the windows and refreshes the tray; returns the watches.
pub fn push_state<R: Runtime>(app: &AppHandle<R>, engine: &Engine) -> Vec<spotter_engine::view::WatchSummary> {
    let watches = engine.list_watches();
    let payload = StatePush {
        watches: watches.iter().map(WatchSummaryDto::from).collect(),
        popover: engine.popover_view(),
        popover_values: engine.popover_values(),
        popover_status: engine.popover_status(),
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
    log::info!("tray menu: {}", menu_item_name(id));
    let Some(state) = app.try_state::<Shared>() else { return };
    let engine = state.engine.clone();
    match id {
        "check-all" => match crate::jobs::registry(app) {
            // A job with progress and Cancel, like the command (rule 18).
            Some(registry) => crate::jobs::spawn_check_all(&registry, engine),
            None => {
                tauri::async_runtime::spawn(async move {
                    let reports = engine.check_now(None).await;
                    note_result("tray: check all", reports.map(|r| r.len()));
                });
            }
        },
        "pause" => note_result("tray: pause notifications for 60 minutes", engine.pause_notifications(Some(60))),
        "resume" => note_result("tray: resume notifications", engine.pause_notifications(None)),
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

/// The tray menu item as the log names it: the fixed items by name, a watch's item by kind only (never its id or name).
fn menu_item_name(id: &str) -> &str {
    match id {
        "check-all" | "pause" | "resume" => id,
        _ => "watch",
    }
}

/// What the log says about how an action ended: a failure says why and what to do, a success only that it is done (the
/// value is never logged).
pub(crate) fn describe_result<T, E: std::fmt::Display>(action: &str, result: &Result<T, E>) -> (log::Level, String) {
    match result {
        Ok(_) => (log::Level::Info, format!("{action}: done")),
        Err(e) => (
            log::Level::Warn,
            format!(
                "{action}: failed: {}; next: try again from the window, or report a problem",
                applog::one_line(&e.to_string())
            ),
        ),
    }
}

/// Logs how an action ended, so a click or a background step that did nothing leaves a reason.
pub(crate) fn note_result<T, E: std::fmt::Display>(action: &str, result: Result<T, E>) {
    let (level, text) = describe_result(action, &result);
    log::log!(level, "{text}");
}

/// Quit's teardown (a bounded step of the shell's exit): stops the background tasks, asks every running job to stop
/// and ends any browser a render still holds, so a stuck render can neither hang Quit nor outlive the app.
pub fn stop<R: Runtime>(app: &AppHandle<R>) {
    let started = std::time::Instant::now();
    log::info!("exit: stopping background work, jobs and browsers");
    if let Some(s) = app.try_state::<Shared>() {
        let _ = s.stop.send(true);
    }
    if let Some(registry) = crate::jobs::registry(app) {
        crate::jobs::cancel_all(&registry);
    }
    spotter_render::process::end_all();
    log::info!("exit: stopped in {} ms", started.elapsed().as_millis());
}

#[cfg(test)]
mod tests {

    /// A test's seams never reach the user's keychain: secrets stay in memory (rule 7); the installed app's use the OS
    /// store.
    #[test]
    fn a_tests_secrets_stay_in_memory() {
        let dir = std::path::Path::new("/nonexistent-test-dir");
        assert_eq!(Seams::test(dir).secrets, SecretsIn::Memory);
        assert_eq!(Seams::installed().secrets, SecretsIn::Os);
    }

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
    #[test]
    fn a_finished_action_is_logged_with_its_outcome_and_a_failure_with_the_next_step() {
        let ok: Result<usize, String> = Ok(3);
        assert_eq!(
            describe_result("tray: check all", &ok),
            (log::Level::Info, "tray: check all: done".to_string())
        );
        let failed: Result<(), String> = Err("the settings file is read-only\nsecond line".into());
        let (level, text) = describe_result("tray: pause notifications for 60 minutes", &failed);
        assert_eq!(level, log::Level::Warn);
        assert!(
            text.starts_with("tray: pause notifications for 60 minutes: failed: the settings file is read-only"),
            "{text}"
        );
        assert!(text.contains("next: "), "{text}");
        assert!(!text.contains('\n'), "{text}");
    }

    #[test]
    fn the_tray_menu_is_logged_by_item_kind_never_by_a_watch() {
        assert_eq!(menu_item_name("check-all"), "check-all");
        assert_eq!(menu_item_name("pause"), "pause");
        assert_eq!(menu_item_name("watch:abc-123"), "watch");
    }
}
