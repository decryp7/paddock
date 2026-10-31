//! The shell's glue between the agent link and the OS: pushed data becomes Tauri events and tray pixels, and saved
//! settings drive the tray style and the start-at-login registrations.

use crate::dto::AgentStatus;
use crate::health::Hub;
use crate::link::{AgentLink, Sink};
use crate::logging;
use crate::login;
use crate::popover::{Ask, PopoverModel};
use crate::tray_model::{items_from_settings, parse_items, Throttle, TrayItems};
use dyno_core::activity::ActivityEntry;
use dyno_core::alerts::Alert;
use dyno_core::protocol::{LiveUpdate, Reply, Request};
use dyno_core::settings::Settings;
use dyno_core::time::Timestamp;
use dyno_core::widgets::popover_values::{Navigate, PopoverState, PopoverSurfaceValues};
use dyno_core::widgets::{DynoSurface, MenuBarItem, SurfaceView};
use dyno_core::widgets::{MetricReading, ReadingQuery};
use paddock_widgets::Placement;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, Runtime, Wry};

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
    /// `DiskScanStatus`.
    pub const DISK_SCAN: &str = "dyno://disk-scan";
    /// `Vec<Volume>`.
    pub const DISK_VOLUMES: &str = "dyno://disk-volumes";
    /// `SurfaceValues`: the tray popup's widget values, sent only while the popup is visible.
    pub const POPOVER_VALUES: &str = "dyno://popover-values";
    /// `DynoSurface`: a layout was edited; windows read it again.
    pub const WIDGET_LAYOUT: &str = "dyno://widget-layout";
    /// `Navigate`: a widget's button asks the main window to open a page or a Settings section.
    pub const NAVIGATE: &str = "dyno://navigate";
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Wall-clock milliseconds since the epoch (the edge of the app: pure code gets the time as an argument).
fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// The icons are redrawn at most this often (and only when their picture changed).
const TRAY_INTERVAL: Duration = Duration::from_millis(500);
/// One line in the log per this many live updates (the log shows that data flows, without filling up).
const LOG_EVERY: u64 = 60;

/// The notice an alarm becomes in the shell's centre: its severity, title and detail, keyed by the alarm's condition so a
/// repeat updates the message instead of adding another.
pub fn alarm_notice(alarm: &dyno_core::alarms::Alarm) -> paddock_health::notify::centre::NewNotice {
    use dyno_core::health::Severity;
    use paddock_health::notify::centre::{NewNotice, NoticeScope, NoticeSeverity};
    let severity = match alarm.severity {
        Severity::Info => NoticeSeverity::Info,
        Severity::Warning => NoticeSeverity::Warning,
        Severity::Critical => NoticeSeverity::Error,
    };
    let mut notice = NewNotice::new(
        format!("dy.alarm.{}", alarm.key),
        "dy.alarm",
        severity,
        alarm.title.clone(),
        NoticeScope::App,
    );
    notice.body = alarm.detail.clone();
    notice
}

/// The tray items: the menu bar layout the agent keeps, or the comma-separated `DYNO_TRAY_ITEMS` (development and
/// measurements).
pub fn initial_items(placements: &[Placement<MenuBarItem>]) -> Vec<crate::tray_model::ItemSpec> {
    let from_env = std::env::var("DYNO_TRAY_ITEMS").map(|t| parse_items(&t)).unwrap_or_default();
    if from_env.is_empty() {
        items_from_settings(placements)
    } else {
        from_env
    }
}

/// What the shell draws in the tray: the native icons in the app ([`NativePixels`]), nothing in a test ([`NoPixels`]).
pub trait TrayPixels<R: Runtime>: Send + Sync {
    /// The tray items changed.
    fn sync_items(&self, app: &AppHandle<R>, specs: &[crate::tray_model::ItemSpec]) -> Result<(), String>;
    /// Draws one item's frame.
    fn paint(&self, app: &AppHandle<R>, item: &str, frame: &crate::tray_model::Frame);
    /// Shows that the agent is not connected.
    fn show_disconnected(&self, app: &AppHandle<R>);
}

/// The tray icons of the menu bar or notification area.
pub struct NativePixels;

impl TrayPixels<Wry> for NativePixels {
    fn sync_items(&self, app: &AppHandle<Wry>, specs: &[crate::tray_model::ItemSpec]) -> Result<(), String> {
        crate::tray::sync_items(app, specs).map_err(|e| e.to_string())
    }
    fn paint(&self, app: &AppHandle<Wry>, item: &str, frame: &crate::tray_model::Frame) {
        crate::tray::paint(app, item, frame);
    }
    fn show_disconnected(&self, app: &AppHandle<Wry>) {
        crate::tray::show_disconnected(app);
    }
}

/// Draws nothing (a test: no icon reaches the desktop).
pub struct NoPixels;

impl<R: Runtime> TrayPixels<R> for NoPixels {
    fn sync_items(&self, _: &AppHandle<R>, _: &[crate::tray_model::ItemSpec]) -> Result<(), String> {
        Ok(())
    }
    fn paint(&self, _: &AppHandle<R>, _: &str, _: &crate::tray_model::Frame) {}
    fn show_disconnected(&self, _: &AppHandle<R>) {}
}

/// The installed app's shell glue (the commands' state).
pub type Shell = ShellOf<Wry>;

/// The shell's state shared by the link, the commands and the tray.
pub struct ShellOf<R: Runtime> {
    app: AppHandle<R>,
    pixels: Arc<dyn TrayPixels<R>>,
    tray: Mutex<TrayItems>,
    throttle: Mutex<Throttle>,
    /// Updates received since the shell started.
    updates: AtomicU64,
    /// `launchAtLogin` as last applied (None until the agent has told us).
    launch_at_login: Mutex<Option<bool>>,
    /// The last agent status, replayed to windows that opened after it was pushed.
    status: Mutex<Option<AgentStatus>>,
    /// `notifications` as last read from the settings (desktop notifications for new alarms).
    notifications: std::sync::atomic::AtomicBool,
    /// The service health of the agent link (the status bar chip).
    hub: Hub<R>,
    /// The tray popup's values (family ADR 0035 §5).
    popover: Mutex<PopoverModel>,
    /// The connection, for the requests the shell itself makes (layouts, popup values).
    link: Mutex<Option<Arc<AgentLink>>>,
    /// A `PopoverValues` request is out (one at a time).
    asking: AtomicBool,
    /// This shell, for the threads that answer later.
    me: Weak<ShellOf<R>>,
    /// The newest live update, whatever the connection (the dashboard's and the previews' readings).
    last_live: Mutex<Option<LiveUpdate>>,
    /// A page or Settings section the main window was asked to open and has not taken yet (a window that is still being
    /// created misses the event, so it asks for this when it starts).
    navigation: Mutex<Option<Navigate>>,
    /// Whether the popup is visible, when not read from the shell's popover state (tests).
    popover_gate: Mutex<Option<Arc<dyn Fn() -> bool + Send + Sync>>>,
}

impl<R: Runtime> ShellOf<R> {
    /// The shell for `app`; `hub` reports the agent link to the shell's service health; `pixels` draws the tray.
    pub fn new(app: AppHandle<R>, hub: Hub<R>, pixels: Arc<dyn TrayPixels<R>>) -> Arc<ShellOf<R>> {
        let size = if cfg!(target_os = "macos") { 44 } else { 32 };
        Arc::new_cyclic(|me| ShellOf {
            app,
            pixels,
            tray: Mutex::new(TrayItems::new(
                initial_items(&Settings::default().menu_bar.layout().placements),
                size,
            )),
            throttle: Mutex::new(Throttle::new(TRAY_INTERVAL)),
            updates: AtomicU64::new(0),
            launch_at_login: Mutex::new(None),
            status: Mutex::new(None),
            notifications: std::sync::atomic::AtomicBool::new(true),
            hub,
            popover: Mutex::new(PopoverModel::default()),
            link: Mutex::new(None),
            asking: AtomicBool::new(false),
            me: me.clone(),
            last_live: Mutex::new(None),
            navigation: Mutex::new(None),
            popover_gate: Mutex::new(None),
        })
    }

    /// The tray items changed (settings, or the development override): the icons follow.
    pub fn set_items(&self, specs: Vec<crate::tray_model::ItemSpec>) {
        let changed = self.tray.lock().unwrap_or_else(|e| e.into_inner()).set_specs(specs.clone());
        if changed {
            if let Err(e) = self.pixels.sync_items(&self.app, &specs) {
                logging::info(&format!("could not update the tray items: {e}"));
            }
        }
    }

    /// How many live updates arrived (for the log and tests).
    pub fn updates(&self) -> u64 {
        self.updates.load(Ordering::SeqCst)
    }

    /// The agent's settings are known (at connect, and after every save): the tray follows them, and a changed
    /// `launchAtLogin` registers or removes the login items. The first sight only records the value: Dyno never
    /// registers itself unasked.
    pub fn settings_changed(&self, settings: &Settings, link: &AgentLink) -> Result<(), String> {
        self.set_items(initial_items(&settings.menu_bar.layout().placements));
        lock(&self.popover).set_layout(settings.popover.layout().placements.clone());
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
                let mut autostart = login::platform_autostart(&self.app);
                let result = login::apply_launch_at_login(wanted, login::current_location(), service.as_mut(), autostart.as_mut());
                // What was just applied is recorded first (it decides what a quit does), then the agent is handed over to
                // the service just registered, or taken back from the one just removed.
                link.set_service_installed(service.installed());
                link.refresh_service();
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

    /// Whether the popup is visible: the shell's popover state, or the gate a test set.
    fn popover_visible(&self) -> bool {
        if let Some(gate) = lock(&self.popover_gate).as_ref() {
            return gate();
        }
        self.app
            .try_state::<paddock_window::tauri::Popovers>()
            .is_some_and(|p| p.is_open(paddock_window::popover::label_for(None).unwrap_or_default().as_str()))
    }

    /// Replaces how the shell learns that the popup is visible (a test has no popover window).
    pub fn set_popover_gate(&self, gate: Arc<dyn Fn() -> bool + Send + Sync>) {
        *lock(&self.popover_gate) = Some(gate);
    }

    /// The popup's values now, for a popup that asks for its first paint.
    pub fn popover_snapshot(&self) -> PopoverSurfaceValues {
        lock(&self.popover).values(Timestamp(now_millis()), &dyno_platform::zone::SystemZone)
    }

    /// The popup has opened and asks for its first paint: it counts as visible from now (the next tick need not notice
    /// first), the slow widgets are asked for at once, and the values known now are returned.
    pub fn popover_opened(&self) -> PopoverSurfaceValues {
        let now = Timestamp(now_millis());
        let ask = {
            let mut m = lock(&self.popover);
            m.set_visible(true);
            m.due(now)
        };
        if let Some(ask) = ask {
            self.ask_slow(ask);
        }
        self.popover_snapshot()
    }

    /// Reads metrics from the newest live update, for the dashboard and the previews.
    pub fn metric_readings(&self, queries: &[ReadingQuery]) -> Vec<MetricReading> {
        dyno_core::widgets::read_all(queries, lock(&self.last_live).as_ref())
    }

    /// Asks the main window to open a page or a Settings section. It is kept until the window takes it
    /// ([`ShellOf::take_navigation`]), because a window that is being created does not see the event; the event goes to the
    /// main window only.
    pub fn navigate(&self, to: Navigate) {
        *lock(&self.navigation) = Some(to);
        if let Err(e) = self.app.emit_to(paddock_commands::labels::MAIN_LABEL, events::NAVIGATE, to) {
            logging::info(&format!("could not emit {}: {e}", events::NAVIGATE));
        }
    }

    /// The navigation waiting for the main window, once.
    pub fn take_navigation(&self) -> Option<Navigate> {
        lock(&self.navigation).take()
    }

    /// Something the popup's slow widgets show may have changed: they are asked for again on the next tick.
    pub fn popover_changed(&self) {
        lock(&self.popover).force_refresh();
    }

    /// Sends the popup its values, only while it is visible.
    fn emit_popover(&self, now: Timestamp) {
        let values = lock(&self.popover).emission(now, &dyno_platform::zone::SystemZone);
        if let Some(values) = values {
            self.emit(events::POPOVER_VALUES, values);
        }
    }

    /// The live tick of the popup: remember the sample, notice a show or hide, ask the agent for the slow kinds that are due
    /// and send the values.
    fn popover_tick(&self, update: &LiveUpdate) {
        let now = update.system.at;
        let visible = self.popover_visible();
        let ask = {
            let mut m = lock(&self.popover);
            m.on_live(update);
            m.set_visible(visible);
            m.due(now)
        };
        if let Some(ask) = ask {
            self.ask_slow(ask);
        }
        self.emit_popover(now);
    }

    /// Asks the agent for the slow kinds on a thread of its own (a request must never block the link's reader), then
    /// applies and sends the answer. One request at a time: when one is out, the next tick asks again.
    fn ask_slow(&self, ask: Ask) {
        let Some(link) = lock(&self.link).clone() else { return };
        let Some(me) = self.me.upgrade() else { return };
        if self.asking.swap(true, Ordering::SeqCst) {
            lock(&self.popover).force_refresh();
            return;
        }
        let spawned = std::thread::Builder::new().name("dyno-popover".into()).spawn(move || {
            let answer = link.call(Request::PopoverValues { placements: ask.ids });
            me.asking.store(false, Ordering::SeqCst);
            match answer {
                Ok(Reply::PopoverValues(values)) => me.popover_answered(ask.generation, values.0),
                Ok(_) => {}
                Err(e) => {
                    logging::info(&format!("could not read the popup values: {}", e.message));
                    lock(&me.popover).force_refresh();
                }
            }
        });
        if spawned.is_err() {
            self.asking.store(false, Ordering::SeqCst);
        }
    }

    fn popover_answered(&self, generation: u64, values: Vec<PopoverState>) {
        if lock(&self.popover).on_slow(generation, values) {
            self.emit_popover(Timestamp(now_millis()));
        }
    }

    /// A surface's layout changed in the agent (an edit from any window): the app reads what it draws itself (the menu bar
    /// items, the popup's layout) and tells the windows to read the rest.
    fn layout_changed(&self, surface: DynoSurface) {
        self.emit(events::WIDGET_LAYOUT, surface);
        if surface == DynoSurface::Dashboard {
            return;
        }
        let Some(link) = lock(&self.link).clone() else { return };
        let Some(me) = self.me.upgrade() else { return };
        let _ = std::thread::Builder::new()
            .name("dyno-layout".into())
            .spawn(move || match link.call(Request::WidgetLayout { surface }) {
                Ok(Reply::WidgetLayout(view)) => me.layout_read(*view),
                Ok(_) => {}
                Err(e) => logging::info(&format!("could not read the {surface:?} layout: {}", e.message)),
            });
    }

    fn layout_read(&self, view: SurfaceView) {
        match view {
            SurfaceView::MenuBar(v) => self.set_items(initial_items(&v.layout.placements)),
            SurfaceView::Popover(v) => {
                lock(&self.popover).set_layout(v.layout.placements);
                self.emit_popover(Timestamp(now_millis()));
            }
            SurfaceView::Dashboard(_) => {}
        }
    }

    fn emit<S: serde::Serialize + Clone>(&self, event: &str, payload: S) {
        if let Err(e) = self.app.emit(event, payload) {
            logging::info(&format!("could not emit {event}: {e}"));
        }
    }
}

impl<R: Runtime> Sink for ShellOf<R> {
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
        if self.throttle.lock().unwrap_or_else(|e| e.into_inner()).allow(Instant::now()) {
            let frames = self
                .tray
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .frames(&update, crate::tray::dark_taskbar());
            for (id, frame) in frames {
                self.pixels.paint(&self.app, &id, &frame);
            }
        }
        *lock(&self.last_live) = Some(update.clone());
        self.popover_tick(&update);
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
        if change.notify {
            // The shell's notification centre: one message per alarm, updated in place while it repeats (an in-app toast
            // when a window is open, the list in the centre otherwise).
            let _ = paddock_health::tauri::post(&self.app, None, alarm_notice(&change.alarm), false);
            if self.notifications.load(Ordering::Relaxed) {
                use tauri_plugin_notification::NotificationExt;
                let a = &change.alarm;
                if let Err(e) = self.app.notification().builder().title(&a.title).body(&a.detail).show() {
                    logging::info(&format!("could not show a notification: {e}"));
                }
            }
        }
        self.popover_changed();
        self.emit(events::ALARM, change);
    }

    fn disk_scan(&self, status: dyno_core::disk::scan::DiskScanStatus) {
        self.emit(events::DISK_SCAN, status);
    }

    fn job(&self, status: paddock_jobs::wire::WireSnapshot) {
        self.popover_changed();
        // The shell's job event: its Jobs view, status item and notices read `paddock://jobs`.
        self.emit(paddock_jobs::EVENT_NAME, paddock_jobs::JobSnapshot::from(status));
    }

    fn disk_volumes(&self, volumes: Vec<dyno_core::disk::volume::Volume>) {
        self.popover_changed();
        self.emit(events::DISK_VOLUMES, volumes);
    }

    fn agent(&self, status: AgentStatus) {
        logging::info(&format!(
            "agent: {:?}{}",
            status.state,
            status.message.as_deref().map(|m| format!(" ({m})")).unwrap_or_default()
        ));
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = Some(status.clone());
        self.hub.report(&status);
        if matches!(status.state, crate::dto::AgentState::Running) {
            lock(&self.popover).agent_back();
        } else {
            self.pixels.show_disconnected(&self.app);
            lock(&self.popover).agent_lost(Timestamp(now_millis()));
            self.emit_popover(Timestamp(now_millis()));
        }
        self.emit(events::AGENT, status);
    }

    fn stalled(&self, stall: Option<crate::link::Stall>) {
        self.hub.report_stall(stall);
    }

    fn widget_layout_changed(&self, surface: DynoSurface) {
        self.layout_changed(surface);
    }

    fn connected(&self, link: &Arc<AgentLink>) {
        *lock(&self.link) = Some(link.clone());
        // Read what the shell itself needs: the tray's metrics and style, and whether launching at login is on.
        match link.call(Request::Settings) {
            Ok(Reply::Settings(s)) => {
                let _ = self.settings_changed(&s, link);
            }
            Ok(_) => {}
            Err(e) => logging::info(&format!("could not read the settings: {}", e.message)),
        }
    }
}
