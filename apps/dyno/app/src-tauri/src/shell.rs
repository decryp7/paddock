//! The shell's glue between the agent link and the OS: pushed data becomes Tauri events and tray pixels, and saved
//! settings drive the tray style and the start-at-login registrations.

use crate::dto::AgentStatus;
use crate::health::Hub;
use crate::link::{AgentLink, LiveVotes, Sink};
use crate::logging;
use crate::login;
use crate::popover::{Ask, PopoverModel};
use crate::tray_model::{items_from_settings, parse_items, Throttle, TrayItems};
use dyno_core::activity::ActivityEntry;
use dyno_core::alerts::Alert;
use dyno_core::protocol::{LiveDetail, LiveUpdate, Reply, Request};
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
    /// `PowerView`: the keep-awake state changed.
    pub const POWER: &str = "dyno://power";
    /// `WolView`: the Wake-on-LAN list changed.
    pub const WOL: &str = "dyno://wol";
    /// `WakesView`: the scheduled wakes changed.
    pub const WAKES: &str = "dyno://wakes";
    /// `TrayAvailability`: the desktop started or stopped showing the tray icons.
    pub const TRAY: &str = "dyno://tray";
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
/// The menu bar's own voice in the link's live-detail votes (beside the windows').
pub const MENU_BAR_VOTER: &str = "menu-bar";
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

/// The tray items in the agent's saved settings file: the layout the person made. A missing, unreadable or damaged file gives the
/// defaults, so a start never waits for the agent and a custom layout never flashes the default items first.
pub fn saved_items(settings_file: &std::path::Path) -> Vec<crate::tray_model::ItemSpec> {
    let (settings, _) = dyno_agent::files::load(settings_file, dyno_core::settings::parse);
    initial_items(&settings.unwrap_or_default().menu_bar.layout().placements)
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
    fn sync_items(
        &self,
        app: &AppHandle<R>,
        specs: &[crate::tray_model::ItemSpec],
        initial: &[(String, crate::tray_model::Frame)],
    ) -> Result<(), String>;
    /// Draws one item's frame.
    fn paint(&self, app: &AppHandle<R>, item: &str, frame: &crate::tray_model::Frame);
    /// Shows that the agent is not connected.
    fn show_disconnected(&self, app: &AppHandle<R>, frames: &[(String, crate::tray_model::Frame)]);
    /// The tray menu's Keep awake entries follow `menu` (`None`: the menu has none).
    fn set_power_menu(&self, app: &AppHandle<R>, menu: Option<&crate::power_menu::Menu>);
    /// The tray menu's Wake submenu follows `menu` (`None`: the menu has none).
    fn set_wol_menu(&self, app: &AppHandle<R>, menu: Option<&crate::wol_menu::Menu>);
    /// The tray menus' Quit says whether the agent keeps measuring after it.
    fn set_keeps_measuring(&self, app: &AppHandle<R>, keeps: bool);
}

/// The tray icons of the menu bar or notification area.
pub struct NativePixels;

impl TrayPixels<Wry> for NativePixels {
    fn sync_items(
        &self,
        app: &AppHandle<Wry>,
        specs: &[crate::tray_model::ItemSpec],
        initial: &[(String, crate::tray_model::Frame)],
    ) -> Result<(), String> {
        crate::tray::sync_items(app, specs, initial).map_err(|e| e.to_string())
    }
    fn paint(&self, app: &AppHandle<Wry>, item: &str, frame: &crate::tray_model::Frame) {
        crate::tray::paint(app, item, frame);
    }
    fn show_disconnected(&self, app: &AppHandle<Wry>, frames: &[(String, crate::tray_model::Frame)]) {
        crate::tray::show_disconnected(app, frames);
    }
    fn set_power_menu(&self, app: &AppHandle<Wry>, menu: Option<&crate::power_menu::Menu>) {
        crate::tray::set_power_menu(app, menu);
    }
    fn set_wol_menu(&self, app: &AppHandle<Wry>, menu: Option<&crate::wol_menu::Menu>) {
        crate::tray::set_wol_menu(app, menu);
    }
    fn set_keeps_measuring(&self, app: &AppHandle<Wry>, keeps: bool) {
        crate::tray::set_keeps_measuring(app, keeps);
    }
}

/// Draws nothing (a test: no icon reaches the desktop).
pub struct NoPixels;

impl<R: Runtime> TrayPixels<R> for NoPixels {
    fn sync_items(
        &self,
        _: &AppHandle<R>,
        _: &[crate::tray_model::ItemSpec],
        _: &[(String, crate::tray_model::Frame)],
    ) -> Result<(), String> {
        Ok(())
    }
    fn paint(&self, _: &AppHandle<R>, _: &str, _: &crate::tray_model::Frame) {}
    fn show_disconnected(&self, _: &AppHandle<R>, _: &[(String, crate::tray_model::Frame)]) {}
    fn set_power_menu(&self, _: &AppHandle<R>, _: Option<&crate::power_menu::Menu>) {}
    fn set_wol_menu(&self, _: &AppHandle<R>, _: Option<&crate::wol_menu::Menu>) {}
    fn set_keeps_measuring(&self, _: &AppHandle<R>, _: bool) {}
}

/// The installed app's shell glue (the commands' state).
pub type Shell = ShellOf<Wry>;

/// The first id of the app's own jobs (the Start-at-login switch). The agent's jobs, which the Jobs view lists with them,
/// count from 1, so the two never share an id.
const APP_JOB_ID_BASE: u64 = 0x4c4f_4749_4e00;

/// The shell's state shared by the link, the commands and the tray.
pub struct ShellOf<R: Runtime> {
    app: AppHandle<R>,
    pixels: Arc<dyn TrayPixels<R>>,
    tray: Mutex<TrayItems>,
    throttle: Mutex<Throttle>,
    /// Updates received since the shell started.
    updates: AtomicU64,
    /// `launchAtLogin` as last applied (None until the agent has told us).
    launch_at_login: Arc<Mutex<Option<bool>>>,
    /// Applies the Start-at-login switch off the calling thread.
    login_switch: login::SwitchWorker,
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
    /// Where the menu bar's live-detail wish goes (the link once connected).
    votes: Mutex<Option<Arc<dyn LiveVotes>>>,
    /// A `PopoverValues` request is out (one at a time).
    asking: AtomicBool,
    /// This shell, for the threads that answer later.
    me: Weak<ShellOf<R>>,
    /// The newest live update, whatever the connection (the dashboard's and the previews' readings).
    last_live: Mutex<Option<LiveUpdate>>,
    /// When `last_live` arrived, on `clock` (milliseconds); `u64::MAX` before the first.
    last_live_at: std::sync::atomic::AtomicU64,
    /// The injected clock (the tray's away delay and how old a reading served to an MCP client may be).
    clock: Arc<dyn paddock_jobs::Clock>,
    /// A page or Settings section the main window was asked to open and has not taken yet (a window that is still being
    /// created misses the event, so it asks for this when it starts).
    navigation: Mutex<Option<Navigate>>,
    /// Whether the popup is visible, when not read from the shell's popover state (tests).
    popover_gate: Mutex<Option<Arc<dyn Fn() -> bool + Send + Sync>>>,
    /// Connection to first live update, for the login-start log.
    first_data: crate::first_data::FirstData,
    /// The open alarms that name a metric, by alarm id: a tray item whose metric an alarm rule covers shows its alert exactly
    /// while one of these is open.
    open_alarms: Mutex<crate::tray_look::OpenAlarms>,
    /// The delay before the tray items show the agent is away (on the injected clock).
    away: crate::tray_away::AwayTimer,
    /// The agent's last power view (the tray's Keep awake entries follow it).
    power: Mutex<Option<dyno_core::power::PowerView>>,
    /// `power.keepAwake.trayItem` and `.display` as last read from the settings.
    power_tray: AtomicBool,
    power_display: AtomicBool,
    power_keep: Mutex<dyno_core::power::settings::KeepAwakeSettings>,
    /// When the tray menu's time left was last drawn (it is drawn again after a minute, while a hold runs).
    power_drawn_at: std::sync::atomic::AtomicI64,
}

/// The oldest live update an MCP client may be given: three times the slowest pace the agent keeps with no window open (the
/// unwatched process interval, which the live update carries), so a stopped agent is never read as the current state.
pub const MAX_LIVE_AGE_MS: u64 = 3 * dyno_core::sampling::UNWATCHED_PROCESS_MS as u64;

/// What a reading tool gets: the newest live update when the agent is connected and the update is recent enough (`age_ms` is how
/// long ago it arrived, `None` before the first), else the words for the client. Pure, so the age bound is tested directly.
pub fn fresh_live(connected: bool, last: Option<LiveUpdate>, age_ms: Option<u64>) -> Result<LiveUpdate, String> {
    if !connected {
        return Err("Dyno is not connected to its agent, so it has no current readings; try again in a moment.".into());
    }
    match (last, age_ms) {
        (Some(update), Some(age)) if age <= MAX_LIVE_AGE_MS => Ok(update),
        (Some(_), Some(age)) => Err(format!(
            "Dyno's newest readings are {} seconds old, so they are not shown as current; try again in a moment.",
            age / 1000
        )),
        _ => Err("Dyno has not received readings from its agent yet; try again in a moment.".into()),
    }
}

impl<R: Runtime> ShellOf<R> {
    /// The shell for `app`; `hub` reports the agent link to the shell's service health; `pixels` draws the tray.
    pub fn new(
        app: AppHandle<R>,
        hub: Hub<R>,
        pixels: Arc<dyn TrayPixels<R>>,
        items: Vec<crate::tray_model::ItemSpec>,
        clock: Arc<dyn paddock_jobs::Clock>,
    ) -> Arc<ShellOf<R>> {
        if let Some(jobs) = app.try_state::<paddock_jobs::tauri::Jobs>() {
            jobs.registry().start_ids_at(APP_JOB_ID_BASE);
        }
        Arc::new_cyclic(|me| ShellOf {
            app,
            pixels,
            tray: Mutex::new(TrayItems::new(items, crate::tray_model::Env::native())),
            open_alarms: Mutex::new(Default::default()),
            away: crate::tray_away::AwayTimer::new(clock.clone()),
            clock,
            last_live_at: std::sync::atomic::AtomicU64::new(u64::MAX),
            throttle: Mutex::new(Throttle::new(TRAY_INTERVAL)),
            updates: AtomicU64::new(0),
            launch_at_login: Arc::new(Mutex::new(None)),
            login_switch: login::SwitchWorker::default(),
            status: Mutex::new(None),
            notifications: std::sync::atomic::AtomicBool::new(true),
            hub,
            popover: Mutex::new(PopoverModel::default()),
            link: Mutex::new(None),
            votes: Mutex::new(None),
            asking: AtomicBool::new(false),
            me: me.clone(),
            last_live: Mutex::new(None),
            navigation: Mutex::new(None),
            popover_gate: Mutex::new(None),
            first_data: crate::first_data::FirstData::default(),
            power: Mutex::new(None),
            power_tray: AtomicBool::new(false),
            power_display: AtomicBool::new(false),
            power_keep: Mutex::new(Default::default()),
            power_drawn_at: std::sync::atomic::AtomicI64::new(0),
        })
    }

    /// Where the menu bar's live-detail wish goes from now on (the link at connect; a fake in tests).
    pub fn use_votes(&self, votes: Arc<dyn LiveVotes>) {
        *lock(&self.votes) = Some(votes);
    }

    /// The tray items changed (settings, or the development override): the icons follow.
    pub fn set_items(&self, specs: Vec<crate::tray_model::ItemSpec>) {
        // With no window open the link asks for the tray level (no process table at the user's pace); an item that shows
        // one app needs the top lists at that pace, so the menu bar asks for the summary while it has one.
        if let Some(votes) = lock(&self.votes).clone() {
            let wish = crate::tray_model::shows_apps(&specs).then_some(LiveDetail::Summary);
            if let Err(e) = votes.set_vote(MENU_BAR_VOTER, wish) {
                logging::warn(&format!("could not update the menu bar's live detail: {}", e.message));
            }
        }
        let changed = self.tray.lock().unwrap_or_else(|e| e.into_inner()).set_specs(specs.clone());
        if changed {
            let initial = self
                .tray
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .unavailable_frames(crate::tray::dark_taskbar());
            if let Err(e) = self.pixels.sync_items(&self.app, &specs, &initial) {
                logging::warn(&format!("could not update the tray items: {e}"));
            }
        }
    }

    /// Draws the tray menu's Keep awake entries from the last power view (none when the setting is off).
    fn draw_power_menu(&self) {
        let now = now_millis();
        self.power_drawn_at.store(now, Ordering::Relaxed);
        let show_keep_awake = self.power_tray.load(Ordering::Relaxed);
        let menu = lock(&self.power).as_ref().and_then(|v| {
            let programs = if show_keep_awake { self.busiest_programs() } else { Vec::new() };
            let mut m = crate::power_menu::menu(v, now, &programs, self.power_display.load(Ordering::Relaxed));
            if show_keep_awake {
                return Some(m);
            }
            // The Keep awake submenu is off, but a countdown to sleep or shut down always shows its line and Cancel (safety).
            m.entries.clear();
            m.header = None;
            m.countdown_header.is_some().then_some(m)
        });
        self.pixels.set_power_menu(&self.app, menu.as_ref());
    }

    /// The busiest programs in the newest live update (root process of each app), for the "until a program quits" entries.
    fn busiest_programs(&self) -> Vec<(u32, String)> {
        lock(&self.last_live)
            .as_ref()
            .map(|l| {
                l.top_cpu
                    .iter()
                    .filter_map(|g| g.pids.first().map(|p| (*p, g.name.clone())))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Asks the agent for an MCP tool: its answer, or the words to give the client.
    pub fn call_agent(&self, request: Request) -> Result<Reply, String> {
        let Some(link) = lock(&self.link).clone().filter(|l| l.connected()) else {
            return Err("Dyno is not connected to its agent; try again in a moment.".into());
        };
        // Inside the MCP server's own call limit, so a slow agent ends this call before the server abandons it.
        link.call_within(request, crate::mcp::AGENT_CALL_LIMIT).map_err(|e| e.message)
    }

    /// The newest live update the agent pushed, for an MCP reading tool (the same values the window and the tray draw), or the
    /// words for the client when the agent is not connected or has sent none yet.
    pub fn newest_live(&self) -> Result<LiveUpdate, String> {
        let connected = lock(&self.link).as_ref().is_some_and(|l| l.connected());
        let at = self.last_live_at.load(Ordering::SeqCst);
        let age_ms = (at != u64::MAX).then(|| self.clock.now_ms().saturating_sub(at));
        fresh_live(connected, lock(&self.last_live).clone(), age_ms)
    }

    /// A Wake entry of the tray menu was chosen: wake the computer (a job the agent runs) or open the Power page.
    pub fn wol_menu_chosen(&self, id: &str) {
        let Some(action) = crate::wol_menu::action_of(id) else {
            return;
        };
        let device = match action {
            crate::wol_menu::Action::Manage => {
                self.navigate(Navigate::Page(dyno_core::widgets::popover_values::Page::Power));
                return;
            }
            crate::wol_menu::Action::Wake(device) => device,
        };
        let Some(link) = lock(&self.link).clone() else {
            logging::info("wake from the tray: the agent is not connected");
            return;
        };
        let _ = std::thread::Builder::new().name("dyno-wol-menu".into()).spawn(move || {
            match link.call(Request::WolWake { id: device, test: false }) {
                Ok(_) => logging::info(&format!("wake from the tray: device {device} started")),
                Err(e) => logging::warn(&format!("wake from the tray: device {device} failed: {}", e.message)),
            }
        });
    }

    /// Whether the agent keeps measuring after a plain Quit changed: the tray menus' Quit says so.
    pub fn set_keeps_measuring(&self, keeps: bool) {
        self.pixels.set_keeps_measuring(&self.app, keeps);
    }

    /// A Keep awake entry of the tray menu was chosen: ask the agent (it answers with the new view, which is pushed back).
    pub fn power_menu_chosen(&self, id: &str) {
        let keep = lock(&self.power_keep).clone();
        let Some(action) = crate::power_menu::action_of(id, self.power_display.load(Ordering::Relaxed), &keep) else {
            return;
        };
        if action == crate::power_menu::Action::ToggleDisplay {
            self.power_display.fetch_xor(true, Ordering::Relaxed);
            self.draw_power_menu();
            return;
        }
        let Some(link) = lock(&self.link).clone() else {
            logging::info("keep awake from the tray: the agent is not connected");
            return;
        };
        let request = match action {
            crate::power_menu::Action::Start(spec) => Request::PowerKeepAwake { spec },
            crate::power_menu::Action::Stop => Request::PowerStopKeepAwake,
            crate::power_menu::Action::ToggleDisplay => return,
            crate::power_menu::Action::CancelIdle => Request::PowerIdleCancel,
            crate::power_menu::Action::SnoozeIdle => Request::PowerIdleSnooze { minutes: 30 },
        };
        let id = id.to_string();
        let _ = std::thread::Builder::new()
            .name("dyno-power-menu".into())
            .spawn(move || match link.call(request) {
                Ok(_) => logging::info(&format!("keep awake from the tray: {id} done")),
                Err(e) => logging::warn(&format!("keep awake from the tray: {id} failed: {}", e.message)),
            });
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
        self.tray
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_policy(crate::tray_look::AlertPolicy::from_settings(settings));
        lock(&self.popover).set_layout(settings.popover.layout().placements.clone());
        self.notifications.store(settings.notifications, Ordering::Relaxed);
        self.power_tray.store(
            settings.power.keep_awake.tray_item && settings.power.keep_awake.enabled,
            Ordering::Relaxed,
        );
        self.power_display.store(settings.power.keep_awake.display, Ordering::Relaxed);
        *lock(&self.power_keep) = settings.power.keep_awake.clone();
        self.draw_power_menu();
        let previous = *self.launch_at_login.lock().unwrap_or_else(|e| e.into_inner());
        let wanted = settings.launch_at_login;
        match previous {
            None => {
                *self.launch_at_login.lock().unwrap_or_else(|e| e.into_inner()) = Some(wanted);
                // Windows, once per start: the entry Dyno 1.0.0 left in place of the app's own is repaired with the switch.
                #[cfg(windows)]
                {
                    let mut service = login::platform_service();
                    let mut autostart = login::platform_autostart(&self.app);
                    match login::ensure_app_entry(wanted, autostart.as_mut()) {
                        Ok(_) => {}
                        Err(e) => {
                            log::warn!(target: "dyno", Error = e.as_str(); "Start at login is on but the app's logon entry could not be written. Turn Start at login off and on in Settings.")
                        }
                    }
                    match login::repair_stale_agent_entry(wanted, service.as_mut(), autostart.as_mut()) {
                        Ok(true) => logging::info(&format!(
                            "removed the stale agent logon entry that Dyno 1.0.0 wrote; the app's own entry is {}",
                            if wanted {
                                "registered again"
                            } else {
                                "not needed (start at login is off)"
                            }
                        )),
                        Ok(false) => {}
                        Err(e) => {
                            log::warn!(target: "dyno", Error = e.as_str(); "The stale agent logon entry of Dyno 1.0.0 could not be repaired; start at login may start only the agent. Turn Start at login off and on in Settings.")
                        }
                    }
                }
                // Start at login is on by default: the first run registers it once. The start already tried it before the
                // agent started; this is the same function, once per process, so a failed try is not repeated here.
                login::run_first_run(&self.app, link, true);
                Ok(())
            }
            Some(p) if p == wanted => Ok(()),
            Some(_) => {
                // The save returns at once; one worker applies it and reports as a job (ER-17). What was just applied is
                // recorded first (it decides what a quit does), then the agent is handed over to the service just registered,
                // or taken back from the one just removed; a death in between is put right at the next start (`wanted`).
                let (app, value) = (self.app.clone(), self.launch_at_login.clone());
                // The shell plugin manages the app's jobs; its Jobs view lists this one and its Cancel stops it.
        let jobs = self.app.state::<paddock_jobs::tauri::Jobs>().registry().clone();
        let register = move || jobs.register("start-at-login", "Start Dyno at login", None);
        self.login_switch.start(register, move |_cancel| {
                    let mut service = login::platform_service();
                    let mut autostart = login::platform_autostart(&app);
                    let result = login::apply_launch_at_login(wanted, login::current_location(), service.as_mut(), autostart.as_mut());
                    // The link is the one the app manages (the save command holds only a reference to it).
                    if let Some(link) = app.try_state::<Arc<crate::link::AgentLink>>() {
                        link.set_service_installed(service.installed());
                        link.refresh_service();
                    }
                    if result.is_ok() {
                        *value.lock().unwrap_or_else(|e| e.into_inner()) = Some(wanted);
                        logging::info(&format!("start at login is now {}", if wanted { "on" } else { "off" }));
                    }
                    result
                })
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

    /// Whether the popup's model wants values (it was opened or seen visible). A preview's snapshot never sets this.
    pub fn popover_wants_values(&self) -> bool {
        lock(&self.popover).visible()
    }

    /// The Settings preview's values: the slow widgets are asked of the agent now and the answer is kept, but the popup is
    /// never marked visible and nothing is broadcast. Blocking (it waits for the agent): call it off the async runtime.
    pub fn popover_preview(&self) -> PopoverSurfaceValues {
        let ask = lock(&self.popover).preview_ask();
        let link = lock(&self.link).clone();
        if let (Some(ask), Some(link)) = (ask, link) {
            match link.call(Request::PopoverValues { placements: ask.ids }) {
                Ok(Reply::PopoverValues(values)) => {
                    lock(&self.popover).on_slow(ask.generation, values.0);
                }
                Ok(_) => {}
                Err(e) => logging::warn(&format!("could not read the popup preview: {}", e.message)),
            }
        }
        self.popover_snapshot()
    }

    /// Whether the slow kinds are due to be asked again at the next tick.
    pub fn popover_refresh_wanted(&self) -> bool {
        lock(&self.popover).refresh_wanted()
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
            logging::warn(&format!("could not emit {}: {e}", events::NAVIGATE));
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
        let (Some(link), Some(me)) = (lock(&self.link).clone(), self.me.upgrade()) else {
            // Nobody to ask yet: keep the wish, so the on-change kinds are asked once there is a link.
            lock(&self.popover).force_refresh();
            return;
        };
        if self.asking.swap(true, Ordering::SeqCst) {
            lock(&self.popover).force_refresh();
            return;
        }
        let spawned = std::thread::Builder::new().name("dyno-popover".into()).spawn(move || {
            let answer = link.call(Request::PopoverValues { placements: ask.ids });
            me.asking.store(false, Ordering::SeqCst);
            match answer {
                Ok(Reply::PopoverValues(values)) => me.popover_answered(ask.generation, values.0),
                Ok(_) => lock(&me.popover).force_refresh(),
                Err(e) => {
                    logging::warn(&format!("could not read the popup values: {}", e.message));
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
                Err(e) => logging::warn(&format!("could not read the {surface:?} layout: {}", e.message)),
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

    /// The agent is not running: after [`crate::tray_away::AWAY_DELAY`], if it is still away, every item shows its unavailable frame.
    fn agent_away(&self) {
        let me = self.me.clone();
        self.away.lost(move || {
            let Some(shell) = me.upgrade() else { return };
            let frames = shell
                .tray
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .unavailable_frames(crate::tray::dark_taskbar());
            logging::info(&format!(
                "tray: the agent is still away after {} s, showing {} item(s) as unavailable",
                crate::tray_away::AWAY_DELAY.as_secs(),
                frames.len()
            ));
            shell.pixels.show_disconnected(&shell.app, &frames);
        });
    }

    fn emit<S: serde::Serialize + Clone>(&self, event: &str, payload: S) {
        if let Err(e) = self.app.emit(event, payload) {
            logging::warn(&format!("could not emit {event}: {e}"));
        }
    }
}

impl<R: Runtime> Sink for ShellOf<R> {
    fn live(&self, update: LiveUpdate) {
        // The tray's "N min left" is drawn again after a minute while a hold with an end runs.
        let (counting, holding) = lock(&self.power).as_ref().map_or((false, false), |v| {
            (
                matches!(v.idle, dyno_core::power::idle::IdleView::Countdown { .. }),
                matches!(v.awake, dyno_core::power::awake::AwakeView::Holding { ends: Some(_), .. }),
            )
        });
        // A countdown's seconds are drawn again every ten.
        if now_millis() - self.power_drawn_at.load(Ordering::Relaxed) >= if counting { 10_000 } else { 60_000 } && (counting || holding) {
            self.draw_power_menu();
        }
        let n = self.updates.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(ms) = self.first_data.first_update(Instant::now()) {
            logging::info(&format!("first data {ms} ms after the connection"));
        }
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
            let mut tray = self.tray.lock().unwrap_or_else(|e| e.into_inner());
            // The taskbar's DPI may have changed: every item is drawn again at the new size.
            #[cfg(not(target_os = "macos"))]
            if tray.set_px(crate::tray::native_icon_px()) {
                logging::info("tray: the icon size changed, drawing every item again");
            }
            let frames = tray.frames(&update, crate::tray::dark_taskbar());
            drop(tray);
            for (id, frame) in frames {
                self.pixels.paint(&self.app, &id, &frame);
            }
        }
        *lock(&self.last_live) = Some(update.clone());
        self.last_live_at.store(self.clock.now_ms(), Ordering::SeqCst);
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
        {
            // The tray items whose metric an alarm rule covers show their alert while an alarm for it is open.
            let metrics = lock(&self.open_alarms).update(change.alarm.id, change.alarm.metric.as_ref(), change.alarm.state.is_open());
            self.tray.lock().unwrap_or_else(|e| e.into_inner()).set_alarmed(metrics);
        }
        if change.notify {
            // The shell's notification centre: one message per alarm, updated in place while it repeats (an in-app toast
            // when a window is open, the list in the centre otherwise).
            let _ = paddock_health::tauri::post(&self.app, None, alarm_notice(&change.alarm), false);
            if self.notifications.load(Ordering::Relaxed) {
                use tauri_plugin_notification::NotificationExt;
                let a = &change.alarm;
                if let Err(e) = self.app.notification().builder().title(&a.title).body(&a.detail).show() {
                    logging::warn(&format!("could not show a notification: {e}"));
                }
            }
        }
        self.popover_changed();
        self.emit(events::ALARM, change);
    }

    fn wakes_changed(&self, view: dyno_core::power::schedule::WakesView) {
        self.emit(events::WAKES, view);
    }

    fn wol_changed(&self, view: dyno_core::wol::WolView) {
        self.pixels.set_wol_menu(&self.app, crate::wol_menu::menu(&view).as_ref());
        self.emit(events::WOL, view);
    }

    fn power_changed(&self, view: dyno_core::power::PowerView) {
        let was_counting = lock(&self.power)
            .as_ref()
            .is_some_and(|v| matches!(v.idle, dyno_core::power::idle::IdleView::Countdown { .. }));
        if let (false, dyno_core::power::idle::IdleView::Countdown { action, deadline, .. }) = (was_counting, &view.idle) {
            // The warning before anything happens: what, how long, and where to cancel.
            use tauri_plugin_notification::NotificationExt;
            let secs = (deadline.since(Timestamp(now_millis())) / 1000).max(0);
            let verb = action.verb();
            let body = format!("Dyno will {verb} this computer in {secs} seconds. Open Dyno's menu and choose Cancel {verb} to stop it.");
            if let Err(e) = self
                .app
                .notification()
                .builder()
                .title(format!("Dyno is about to {verb}"))
                .body(body)
                .show()
            {
                logging::warn(&format!("could not show the countdown notification: {e}"));
            }
        }
        *lock(&self.power) = Some(view.clone());
        self.draw_power_menu();
        self.emit(events::POWER, view);
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
            self.away.back();
            lock(&self.popover).agent_back();
        } else {
            self.agent_away();
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

    fn connection_made(&self) {
        self.first_data.connected(Instant::now());
    }

    fn connected(&self, link: &Arc<AgentLink>) {
        *lock(&self.link) = Some(link.clone());
        self.use_votes(link.clone());
        match link.call(Request::PowerState) {
            Ok(Reply::Power(v)) => self.power_changed(*v),
            Ok(_) => {}
            Err(e) => logging::warn(&format!("could not read the power state: {}", e.message)),
        }
        match link.call(Request::WolDevices) {
            Ok(Reply::Wol(v)) => self.wol_changed(*v),
            Ok(_) => {}
            Err(e) => logging::warn(&format!("could not read the Wake-on-LAN list: {}", e.message)),
        }
        // Read what the shell itself needs: the tray's metrics and style, and whether launching at login is on.
        match link.call(Request::Settings) {
            Ok(Reply::Settings(s)) => {
                let _ = self.settings_changed(&s, link);
            }
            Ok(_) => {}
            Err(e) => logging::warn(&format!("could not read the settings: {}", e.message)),
        }
    }
}

#[cfg(test)]
mod fresh_live_tests {
    use super::*;

    fn update() -> LiveUpdate {
        let mut system = dyno_platform::fake::quiet_sample(dyno_core::time::Timestamp(1));
        system.cpu.total = 7.0;
        LiveUpdate {
            system,
            pressure: dyno_core::metrics::MemoryPressure::Normal,
            top_cpu: vec![],
            top_memory: vec![],
            overhead: Default::default(),
            processes: None,
            groups: None,
            other_users: None,
            via: Default::default(),
        }
    }

    #[test]
    fn readings_are_current_only_while_the_agent_is_connected_and_the_update_is_recent() {
        assert!(fresh_live(true, Some(update()), Some(0)).is_ok());
        assert!(fresh_live(true, Some(update()), Some(MAX_LIVE_AGE_MS)).is_ok());
        let old = fresh_live(true, Some(update()), Some(MAX_LIVE_AGE_MS + 1_000)).unwrap_err();
        assert!(old.contains("not shown as current"), "{old}");
        let gone = fresh_live(false, Some(update()), Some(0)).unwrap_err();
        assert!(
            gone.contains("not connected"),
            "an agent that crashed leaves old values behind: {gone}"
        );
        assert!(fresh_live(true, None, None).unwrap_err().contains("not received"));
    }
}

#[cfg(test)]
mod saved_items_tests {
    use super::*;
    use dyno_core::widgets::{edit_layout, SurfaceEdit};
    use paddock_widgets::{Edit, PlacementId};

    fn write(dir: &std::path::Path, text: &str) -> std::path::PathBuf {
        let path = dir.join("settings.json");
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn a_saved_custom_layout_is_what_the_tray_starts_with() {
        let mut s = Settings::default();
        edit_layout(
            &mut s,
            SurfaceEdit::MenuBar(Edit::Remove {
                id: PlacementId::new("number-2").unwrap(),
            }),
            None,
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), &serde_json::to_string(&s).unwrap());
        let items = saved_items(&path);
        let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["number", "number-3", "number-4"], "not the four default items");
    }

    #[test]
    fn no_file_or_a_damaged_one_gives_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let defaults = initial_items(&Settings::default().menu_bar.layout().placements);
        assert_eq!(saved_items(&dir.path().join("missing.json")), defaults);
        let path = write(dir.path(), "{ not json");
        assert_eq!(saved_items(&path), defaults);
    }
}
