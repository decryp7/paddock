//! The shell's glue between the agent link and the OS: pushed data becomes Tauri events and tray pixels, and saved
//! settings drive the tray style and the start-at-login registrations.

use crate::dto::AgentStatus;
use crate::link::{AgentLink, Sink};
use crate::logging;
use crate::login::{self, AppAutostart};
use crate::tray_model::{items_from_settings, parse_items, Throttle, TrayItems};
use dyno_core::activity::ActivityEntry;
use dyno_core::alerts::Alert;
use dyno_core::protocol::{LiveUpdate, Reply, Request};
use dyno_core::settings::Settings;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, Wry};

/// The push channels of `tauri-backend.ts`.
pub mod events {
    /// `LiveUpdate`.
    pub const LIVE: &str = "dyno://live";
    /// `Alert`.
    pub const ALERT: &str = "dyno://alert";
    /// `ActivityEntry`.
    pub const ACTIVITY: &str = "dyno://activity";
    /// `AgentStatus`.
    pub const AGENT: &str = "dyno://agent";
    /// `AlarmChange`.
    pub const ALARM: &str = "dyno://alarm";
}

/// The icons are redrawn at most this often (and only when their picture changed).
const TRAY_INTERVAL: Duration = Duration::from_millis(500);
/// One line in the log per this many live updates (the log shows that data flows, without filling up).
const LOG_EVERY: u64 = 60;

/// The tray items: the user's settings, or the comma-separated `DYNO_TRAY_ITEMS` (development and measurements).
pub fn initial_items(items: &dyno_core::widgets::TrayItems) -> Vec<crate::tray_model::ItemSpec> {
    let from_env = std::env::var("DYNO_TRAY_ITEMS").map(|t| parse_items(&t)).unwrap_or_default();
    if from_env.is_empty() {
        items_from_settings(items)
    } else {
        from_env
    }
}

/// The shell's state shared by the link, the commands and the tray.
pub struct Shell {
    app: AppHandle<Wry>,
    tray: Mutex<TrayItems>,
    throttle: Mutex<Throttle>,
    /// Updates received since the shell started.
    updates: AtomicU64,
    /// `launchAtLogin` as last applied (None until the agent has told us).
    launch_at_login: Mutex<Option<bool>>,
    paused: Mutex<bool>,
    /// The last agent status, replayed to windows that opened after it was pushed.
    status: Mutex<Option<AgentStatus>>,
    /// `notifications` as last read from the settings (desktop notifications for new alarms).
    notifications: std::sync::atomic::AtomicBool,
}

impl Shell {
    /// The shell for `app`.
    pub fn new(app: AppHandle<Wry>) -> Arc<Shell> {
        let size = if cfg!(target_os = "macos") { 44 } else { 32 };
        Arc::new(Shell {
            app,
            tray: Mutex::new(TrayItems::new(initial_items(&Settings::default().tray_items), size)),
            throttle: Mutex::new(Throttle::new(TRAY_INTERVAL)),
            updates: AtomicU64::new(0),
            launch_at_login: Mutex::new(None),
            paused: Mutex::new(false),
            status: Mutex::new(None),
            notifications: std::sync::atomic::AtomicBool::new(true),
        })
    }

    /// The tray items changed (settings, or the development override): the icons follow.
    pub fn set_items(&self, specs: Vec<crate::tray_model::ItemSpec>) {
        let changed = self.tray.lock().unwrap_or_else(|e| e.into_inner()).set_specs(specs.clone());
        if changed {
            if let Err(e) = crate::tray::sync_items(&self.app, &specs) {
                logging::info(&format!("could not update the tray items: {e}"));
            }
        }
    }

    /// How many live updates arrived (for the log and tests).
    pub fn updates(&self) -> u64 {
        self.updates.load(Ordering::SeqCst)
    }

    /// Whether the rules are paused, as of the last update.
    pub fn rules_paused(&self) -> bool {
        *self.paused.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The agent's settings are known (at connect, and after every save): the tray follows them, and a changed
    /// `launchAtLogin` registers or removes the login items. The first sight only records the value: Dyno never
    /// registers itself unasked.
    pub fn settings_changed(&self, settings: &Settings, link: &AgentLink) -> Result<(), String> {
        self.set_items(initial_items(&settings.tray_items));
        self.notifications.store(settings.notifications, Ordering::Relaxed);
        let previous = *self.launch_at_login.lock().unwrap_or_else(|e| e.into_inner());
        let wanted = settings.launch_at_login;
        match previous {
            None => {
                *self.launch_at_login.lock().unwrap_or_else(|e| e.into_inner()) = Some(wanted);
                Ok(())
            }
            Some(p) if p == wanted => Ok(()),
            Some(_) => {
                let mut service = login::platform_service();
                let result = login::apply_launch_at_login(wanted, service.as_mut(), &mut AppAutostart(self.app.clone()));
                link.set_service_installed(service.installed());
                if result.is_ok() {
                    *self.launch_at_login.lock().unwrap_or_else(|e| e.into_inner()) = Some(wanted);
                    logging::info(&format!("start at login is now {}", if wanted { "on" } else { "off" }));
                }
                result
            }
        }
    }

    /// Pushes the current agent status again. A window that opens later (the tray dropdown, a reopened main window)
    /// never saw the push that announced it, so a slow beat repeats it while any window is open.
    pub fn replay_status(&self) {
        if self.app.webview_windows().is_empty() {
            return;
        }
        let status = self.status.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(s) = status {
            self.emit(events::AGENT, s);
        }
    }

    fn emit<S: serde::Serialize + Clone>(&self, event: &str, payload: S) {
        if let Err(e) = self.app.emit(event, payload) {
            logging::info(&format!("could not emit {event}: {e}"));
        }
    }
}

impl Sink for Shell {
    fn live(&self, update: LiveUpdate) {
        let n = self.updates.fetch_add(1, Ordering::SeqCst) + 1;
        if n == 1 || n.is_multiple_of(LOG_EVERY) {
            logging::info(&format!(
                "live update #{n}: cpu {:.0}%, memory {:.0}%, {} top apps, {} processes",
                update.system.cpu.total,
                update.system.memory_percent(),
                update.top_cpu.len(),
                update.processes.as_ref().map_or(0, Vec::len)
            ));
        }
        *self.paused.lock().unwrap_or_else(|e| e.into_inner()) = update.rules_paused;
        if self.throttle.lock().unwrap_or_else(|e| e.into_inner()).allow(Instant::now()) {
            let frames = self
                .tray
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .frames(&update, crate::tray::dark_taskbar());
            for (id, frame) in frames {
                crate::tray::paint(&self.app, &id, &frame, update.rules_paused);
            }
        }
        // With no window open nothing listens: skip serialising a (large) update for nobody.
        if !self.app.webview_windows().is_empty() {
            self.emit(events::LIVE, update);
        }
    }

    fn alert(&self, alert: Alert) {
        self.emit(events::ALERT, alert);
    }

    fn activity(&self, entry: ActivityEntry) {
        self.emit(events::ACTIVITY, entry);
    }

    fn alarm(&self, change: dyno_core::alarms::AlarmChange) {
        if change.notify && self.notifications.load(Ordering::Relaxed) {
            use tauri_plugin_notification::NotificationExt;
            let a = &change.alarm;
            if let Err(e) = self.app.notification().builder().title(&a.title).body(&a.detail).show() {
                logging::info(&format!("could not show a notification: {e}"));
            }
        }
        self.emit(events::ALARM, change);
    }

    fn agent(&self, status: AgentStatus) {
        logging::info(&format!(
            "agent: {:?}{}",
            status.state,
            status.message.as_deref().map(|m| format!(" ({m})")).unwrap_or_default()
        ));
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = Some(status.clone());
        if !matches!(status.state, crate::dto::AgentState::Running) {
            crate::tray::show_disconnected(&self.app);
        }
        self.emit(events::AGENT, status);
    }

    fn connected(&self, link: &Arc<AgentLink>) {
        // Read what the shell itself needs: the tray's metrics and style, and whether launching at login is on.
        match link.call(Request::Settings) {
            Ok(Reply::Settings(s)) => {
                let _ = self.settings_changed(&s, link);
            }
            Ok(_) => {}
            Err(e) => logging::info(&format!("could not read the settings: {}", e.message)),
        }
        // Is a service keeping the agent alive? (decides what Quit does). Off this thread: the OS may be slow.
        let link = link.clone();
        std::thread::spawn(move || {
            let mut service = login::platform_service();
            link.set_service_installed(service.installed());
        });
    }
}
