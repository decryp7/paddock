//! Whether the desktop shows Dyno's tray icons, and what the app does when that changes (ER-27: every look is in the log).
//!
//! The decision is `dyno_core::tray_host` (pure). This module is the thin edge: [`HostSource`] reads the StatusNotifier host from
//! the session bus (Linux only; elsewhere the icons are always shown), [`watch`] looks again every [`POLL_EVERY`] so an extension
//! turned on later, or a watcher that starts late at login, is noticed, and [`HostEffects`] is what a change does: tell the UI
//! and the health chip, tell the shell whether a tray exists (a hidden start opens the window, closing the last window quits,
//! when none does) and make the icons again once a host appears.

use dyno_core::tray_host::{availability, describe, HostChange, HostProbe, HostTracker, TrayAvailability, NO_HOST_ADVICE, NO_HOST_SUMMARY};
use std::time::Duration;

/// How often the host is looked at again. One D-Bus call pair every two seconds costs nothing, and an extension turned on is shown
/// within two seconds.
pub const POLL_EVERY: Duration = Duration::from_secs(2);
/// How long a start waits for a host that is not there yet before it decides the window must open: a panel or extension that
/// starts a few seconds after the app at login should not make a window pop up.
pub const START_GRACE: Duration = Duration::from_secs(3);
/// How long one D-Bus call may take before the host counts as unreadable.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(3);

/// Reads the StatusNotifier host. A fake in the tests.
pub trait HostSource {
    /// One look. Never panics; an unreadable bus is [`HostProbe::CantTell`].
    fn probe(&self) -> HostProbe;
}

/// What a change of the host does.
pub trait HostEffects {
    /// Records the availability for the UI (command and `dyno://tray` event) and the health chip.
    fn publish(&mut self, now: &TrayAvailability);
    /// Tells the shell whether a tray exists.
    fn shell_tray_available(&mut self, available: bool);
    /// Makes the icons again (the host that was missing registered).
    fn rebuild_icons(&mut self);
    /// Writes a line to the log.
    fn log(&mut self, line: &str);
}

/// The first look of a start: waits up to `grace` (in steps of `step`, through `wait`, which says whether to go on) for a host that
/// is not there yet, and returns the last probe.
pub fn probe_at_start(source: &dyn HostSource, grace: Duration, step: Duration, mut wait: impl FnMut(Duration) -> bool) -> HostProbe {
    let mut waited = Duration::ZERO;
    loop {
        let probe = source.probe();
        if availability(&probe).is_shown() || waited >= grace || !wait(step) {
            return probe;
        }
        waited += step;
    }
}

/// Takes one look: records it, and applies what changed. `elapsed` is how long the watch has run (for the log).
pub fn step(tracker: &mut HostTracker, probe: &HostProbe, elapsed: Duration, effects: &mut impl HostEffects) {
    let now = availability(probe);
    match tracker.observe(now.clone()) {
        HostChange::Same => {}
        HostChange::First(_) => {
            effects.log(&format!("tray host: {}", describe(probe)));
            effects.publish(&now);
            effects.shell_tray_available(now.is_shown());
        }
        HostChange::Appeared => {
            effects.log(&format!(
                "tray host: appeared {} s after the start ({}); making the tray icons again",
                elapsed.as_secs(),
                describe(probe)
            ));
            effects.shell_tray_available(true);
            effects.publish(&now);
            effects.rebuild_icons();
        }
        HostChange::Vanished => {
            effects.log(&format!("tray host: gone after {} s ({})", elapsed.as_secs(), describe(probe)));
            effects.shell_tray_available(false);
            effects.publish(&now);
        }
    }
}

/// Looks every [`POLL_EVERY`] until `wait` says stop (it sleeps for the interval and returns whether to go on).
/// `tracker` already holds the start's look.
pub fn watch(source: &dyn HostSource, tracker: &mut HostTracker, effects: &mut impl HostEffects, mut wait: impl FnMut(Duration) -> bool) {
    let mut elapsed = Duration::ZERO;
    while wait(POLL_EVERY) {
        elapsed += POLL_EVERY;
        step(tracker, &source.probe(), elapsed, effects);
    }
}

/// The health chip's words for a missing host, from the engine's text.
pub fn health_words() -> (&'static str, &'static str) {
    (NO_HOST_SUMMARY, NO_HOST_ADVICE)
}

/// The availability the UI reads (`tray_availability`) and the shell's tray is told about; managed by the app.
pub struct TrayHostState(std::sync::Mutex<TrayAvailability>);

impl Default for TrayHostState {
    /// Shown until a look says otherwise (every other desktop, and the time before the first look).
    fn default() -> Self {
        TrayHostState(std::sync::Mutex::new(TrayAvailability::Shown))
    }
}

impl TrayHostState {
    /// The availability now.
    pub fn get(&self) -> TrayAvailability {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    fn set(&self, now: &TrayAvailability) {
        *self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = now.clone();
    }
}

/// [`HostEffects`] over the running app.
pub struct AppEffects {
    app: tauri::AppHandle<tauri::Wry>,
}

impl AppEffects {
    /// Effects on `app`; also manages the [`TrayHostState`] the first time.
    pub fn new(app: &tauri::AppHandle<tauri::Wry>) -> Self {
        use tauri::Manager;
        if app.try_state::<TrayHostState>().is_none() {
            app.manage(TrayHostState::default());
        }
        AppEffects { app: app.clone() }
    }
}

impl HostEffects for AppEffects {
    fn publish(&mut self, now: &TrayAvailability) {
        use tauri::{Emitter, Manager};
        self.app.state::<TrayHostState>().set(now);
        if let Err(e) = self.app.emit(crate::shell::events::TRAY, now) {
            self.log(&format!("tray host: could not tell the windows ({e})"));
        }
        crate::health::report_tray(&self.app, now);
    }

    fn shell_tray_available(&mut self, available: bool) {
        paddock_window::tauri::set_tray_available(&self.app, available);
    }

    fn rebuild_icons(&mut self) {
        crate::tray::rebuild(&self.app);
    }

    fn log(&mut self, line: &str) {
        crate::logging::info(line);
    }
}

/// The availability now, for the command.
pub fn current(app: &tauri::AppHandle<tauri::Wry>) -> TrayAvailability {
    use tauri::Manager;
    app.try_state::<TrayHostState>().map_or(TrayAvailability::Shown, |s| s.get())
}

/// Linux: looks at the host (waiting [`START_GRACE`] for one that is late), records it, and keeps watching on a thread of its own.
/// Returns whether the desktop shows the icons now. Elsewhere there is no host to wait for.
#[cfg(target_os = "linux")]
pub fn start(app: &tauri::AppHandle<tauri::Wry>) -> bool {
    use std::sync::Arc;
    let source = Arc::new(DbusSource::session());
    let first = probe_at_start(source.as_ref(), START_GRACE, Duration::from_millis(500), |d| {
        std::thread::sleep(d);
        true
    });
    let mut tracker = HostTracker::new();
    let mut effects = AppEffects::new(app);
    step(&mut tracker, &first, Duration::ZERO, &mut effects);
    // The watch lives as long as the process; it ends with it.
    let spawned = std::thread::Builder::new().name("tray-host-watch".into()).spawn(move || {
        watch(source.as_ref(), &mut tracker, &mut effects, |d| {
            std::thread::sleep(d);
            true
        });
    });
    if let Err(e) = spawned {
        crate::logging::info(&format!(
            "tray host: the watch could not start ({e}); a host that appears later is not noticed"
        ));
    }
    availability(&first).is_shown()
}

/// Not Linux: the icons are always shown by the OS's own tray.
#[cfg(not(target_os = "linux"))]
pub fn start(_app: &tauri::AppHandle<tauri::Wry>) -> bool {
    true
}

#[cfg(target_os = "linux")]
pub use dbus::DbusSource;

#[cfg(target_os = "linux")]
mod dbus {
    //! The session bus: `org.kde.StatusNotifierWatcher` and its `IsStatusNotifierHostRegistered` property.
    use super::{HostSource, CALL_TIMEOUT};
    use dyno_core::tray_host::HostProbe;
    use std::sync::Mutex;
    use zbus::blocking::{fdo, Connection};
    use zbus::names::{BusName, InterfaceName};

    /// The well-known name of the watcher.
    pub const WATCHER: &str = "org.kde.StatusNotifierWatcher";
    const PATH: &str = "/StatusNotifierWatcher";
    const PROPERTY: &str = "IsStatusNotifierHostRegistered";

    /// Reads the host from a session bus: the user's, or the one `address` names (the tests' private bus).
    pub struct DbusSource {
        address: Option<String>,
        conn: Mutex<Option<Connection>>,
    }

    impl DbusSource {
        /// The user's session bus.
        pub fn session() -> Self {
            DbusSource {
                address: None,
                conn: Mutex::new(None),
            }
        }

        /// The bus at `address`.
        pub fn at(address: &str) -> Self {
            DbusSource {
                address: Some(address.to_string()),
                conn: Mutex::new(None),
            }
        }

        fn connect(&self) -> zbus::Result<Connection> {
            let builder = match &self.address {
                Some(a) => zbus::blocking::connection::Builder::address(a.as_str())?,
                None => zbus::blocking::connection::Builder::session()?,
            };
            builder.method_timeout(CALL_TIMEOUT).build()
        }

        fn read(&self, conn: &Connection) -> zbus::Result<HostProbe> {
            let bus = fdo::DBusProxy::new(conn)?;
            let name = BusName::try_from(WATCHER)?;
            if !bus.name_has_owner(name)? {
                return Ok(HostProbe::WatcherAbsent);
            }
            let props = fdo::PropertiesProxy::builder(conn).destination(WATCHER)?.path(PATH)?.build()?;
            let value = props.get(InterfaceName::try_from(WATCHER)?, PROPERTY)?;
            let registered = bool::try_from(value).map_err(|e| zbus::Error::Failure(format!("{PROPERTY} is not a boolean: {e}")))?;
            Ok(if registered {
                HostProbe::Present
            } else {
                HostProbe::NoHostRegistered
            })
        }
    }

    impl HostSource for DbusSource {
        fn probe(&self) -> HostProbe {
            let mut slot = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if slot.is_none() {
                match self.connect() {
                    Ok(c) => *slot = Some(c),
                    Err(e) => return HostProbe::CantTell(format!("no session bus: {e}")),
                }
            }
            let Some(conn) = slot.as_ref() else {
                return HostProbe::CantTell("no session bus".to_string());
            };
            match self.read(conn) {
                Ok(p) => p,
                Err(e) => {
                    // The bus may have restarted: connect again on the next look.
                    *slot = None;
                    HostProbe::CantTell(format!("the StatusNotifier host could not be read: {e}"))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    /// Answers the scripted probes in order, the last one for ever.
    struct Script(RefCell<VecDeque<HostProbe>>);

    impl Script {
        fn new(probes: &[HostProbe]) -> Self {
            Script(RefCell::new(probes.iter().cloned().collect()))
        }
    }

    impl HostSource for Script {
        fn probe(&self) -> HostProbe {
            let mut q = self.0.borrow_mut();
            if q.len() > 1 {
                q.pop_front().unwrap()
            } else {
                q.front().cloned().unwrap()
            }
        }
    }

    #[derive(Default)]
    struct Spy {
        published: Vec<TrayAvailability>,
        shell: Vec<bool>,
        rebuilt: usize,
        log: Vec<String>,
    }

    impl HostEffects for Spy {
        fn publish(&mut self, now: &TrayAvailability) {
            self.published.push(now.clone());
        }
        fn shell_tray_available(&mut self, available: bool) {
            self.shell.push(available);
        }
        fn rebuild_icons(&mut self) {
            self.rebuilt += 1;
        }
        fn log(&mut self, line: &str) {
            self.log.push(line.to_string());
        }
    }

    use HostProbe::{NoHostRegistered, Present, WatcherAbsent};

    #[test]
    fn an_absent_watcher_is_published_as_no_host_and_the_shell_is_told_there_is_no_tray() {
        let mut t = HostTracker::new();
        let mut spy = Spy::default();
        step(&mut t, &WatcherAbsent, Duration::ZERO, &mut spy);
        assert_eq!(spy.published, vec![TrayAvailability::no_host()]);
        assert_eq!(spy.shell, vec![false]);
        assert_eq!(spy.rebuilt, 0);
        assert!(spy.log[0].contains("nothing owns org.kde.StatusNotifierWatcher"), "{:?}", spy.log);
    }

    #[test]
    fn a_present_host_is_shown_and_nothing_is_rebuilt_at_the_start() {
        let mut t = HostTracker::new();
        let mut spy = Spy::default();
        step(&mut t, &Present, Duration::ZERO, &mut spy);
        assert_eq!(spy.published, vec![TrayAvailability::Shown]);
        assert_eq!(spy.shell, vec![true]);
        assert_eq!(spy.rebuilt, 0);
    }

    #[test]
    fn a_host_that_appears_later_makes_the_icons_again_once_and_is_logged_with_when() {
        let source = Script::new(&[WatcherAbsent, WatcherAbsent, NoHostRegistered, Present, Present, Present]);
        let mut t = HostTracker::new();
        let mut spy = Spy::default();
        step(&mut t, &source.probe(), Duration::ZERO, &mut spy);
        let mut left = 5;
        watch(&source, &mut t, &mut spy, |_| {
            left -= 1;
            left > 0
        });
        assert_eq!(spy.rebuilt, 1, "rebuilt once, not on every look");
        assert_eq!(spy.shell, vec![false, true]);
        assert_eq!(spy.published, vec![TrayAvailability::no_host(), TrayAvailability::Shown]);
        let appeared = spy.log.iter().find(|l| l.contains("appeared")).expect("the appearance is logged");
        assert!(appeared.contains("6 s after the start"), "{appeared}");
    }

    #[test]
    fn a_host_that_goes_away_is_published_and_the_shell_loses_its_tray() {
        let source = Script::new(&[Present, WatcherAbsent]);
        let mut t = HostTracker::new();
        let mut spy = Spy::default();
        step(&mut t, &source.probe(), Duration::ZERO, &mut spy);
        let mut left = 2;
        watch(&source, &mut t, &mut spy, |_| {
            left -= 1;
            left > 0
        });
        assert_eq!(spy.shell, vec![true, false]);
        assert_eq!(spy.published.last(), Some(&TrayAvailability::no_host()));
        assert_eq!(spy.rebuilt, 0);
        assert!(spy.log.iter().any(|l| l.contains("gone")));
    }

    #[test]
    fn the_start_waits_the_grace_for_a_late_host_and_stops_as_soon_as_it_is_there() {
        let source = Script::new(&[WatcherAbsent, NoHostRegistered, Present]);
        let mut slept = Vec::new();
        let probe = probe_at_start(&source, START_GRACE, Duration::from_secs(1), |d| {
            slept.push(d);
            true
        });
        assert_eq!(probe, Present);
        assert_eq!(slept.len(), 2);
    }

    #[test]
    fn the_start_gives_up_after_the_grace_with_the_last_look() {
        let source = Script::new(&[WatcherAbsent]);
        let mut slept = Duration::ZERO;
        let probe = probe_at_start(&source, START_GRACE, Duration::from_secs(1), |d| {
            slept += d;
            true
        });
        assert_eq!(probe, WatcherAbsent);
        assert_eq!(slept, START_GRACE);
    }

    #[test]
    fn an_unreadable_bus_raises_no_alarm_and_keeps_the_tray() {
        let mut t = HostTracker::new();
        let mut spy = Spy::default();
        step(
            &mut t,
            &HostProbe::CantTell("no session bus: refused".into()),
            Duration::ZERO,
            &mut spy,
        );
        assert_eq!(spy.published, vec![TrayAvailability::Shown]);
        assert_eq!(spy.shell, vec![true]);
        assert!(spy.log[0].contains("refused"));
    }

    #[test]
    fn the_health_words_are_the_engines() {
        let (head, advice) = health_words();
        assert!(head.starts_with("Your desktop"));
        assert!(advice.contains("gnome-shell-extension-appindicator"));
    }
}

#[cfg(all(test, target_os = "linux"))]
mod bus_tests {
    //! The adapter against a real private session bus (`dbus-daemon`) with a fake watcher, so the property read is proven, not
    //! assumed. A missing `dbus-daemon` fails the test with the install step.
    use super::dbus::{DbusSource, WATCHER};
    use super::{HostProbe, HostSource};
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Stdio};

    struct Bus {
        child: Child,
        address: String,
    }

    impl Bus {
        fn start() -> Bus {
            let mut child = dyno_platform::process::command("dbus-daemon")
                .args(["--session", "--nofork", "--print-address"])
                .stdout(Stdio::piped())
                .spawn()
                .expect("dbus-daemon is needed for this test (apt install dbus-daemon)");
            let out = child.stdout.take().expect("piped");
            let mut address = String::new();
            BufReader::new(out).read_line(&mut address).expect("the bus prints its address");
            Bus {
                child,
                address: address.trim().to_string(),
            }
        }
    }

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    struct Watcher {
        registered: bool,
    }

    #[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
    impl Watcher {
        #[zbus(property)]
        fn is_status_notifier_host_registered(&self) -> bool {
            self.registered
        }
    }

    fn own_watcher(address: &str, registered: bool) -> zbus::blocking::Connection {
        zbus::blocking::connection::Builder::address(address)
            .expect("address")
            .name(WATCHER)
            .expect("name")
            .serve_at("/StatusNotifierWatcher", Watcher { registered })
            .expect("serve")
            .build()
            .expect("the fake watcher connects")
    }

    #[test]
    fn the_adapter_reads_absent_unregistered_and_present_from_a_real_bus() {
        let bus = Bus::start();
        let source = DbusSource::at(&bus.address);
        assert_eq!(source.probe(), HostProbe::WatcherAbsent);
        let watcher = own_watcher(&bus.address, false);
        assert_eq!(source.probe(), HostProbe::NoHostRegistered);
        drop(watcher);
        let _present = own_watcher(&bus.address, true);
        assert_eq!(source.probe(), HostProbe::Present);
    }

    #[test]
    fn a_bus_that_is_not_there_is_cant_tell_with_the_reason() {
        let source = DbusSource::at("unix:path=/nonexistent/dyno-test-bus");
        match source.probe() {
            HostProbe::CantTell(why) => assert!(why.contains("no session bus"), "{why}"),
            other => panic!("{other:?}"),
        }
    }
}
