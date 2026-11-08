//! Pitwall's own start-at-login entry (Settings > Start at log-in). On Windows it is always the per-user `Run` value
//! ([`paddock_service::login::UserRunEntry`], family ADR 0053 section 6): Pitwall installs once for every user, so each user
//! decides for themselves, and it is never written while the app runs elevated (an elevated process may belong to another
//! account, and `tauri-plugin-autostart` would write the machine-wide key, which starts Pitwall for everyone and which an
//! unelevated "off" cannot remove). Other systems keep the plugin (macOS launch agent, Linux autostart file).

use paddock_service::login::{Elevation, UserRunEntry};
use paddock_service::port::System;
use std::path::PathBuf;

/// The name of the `Run` value: the product name, which the plugin used too, so an upgrade rewrites the value instead of adding
/// a second one.
pub const RUN_VALUE: &str = "Pitwall";

/// The entry that starts `exe` hidden in the tray at login.
pub fn run_entry(exe: PathBuf, sys: Box<dyn System>, elevation: Box<dyn Elevation>) -> UserRunEntry {
    UserRunEntry::new(RUN_VALUE, exe, &[paddock_window::launch::HIDDEN_FLAG], sys, elevation)
}

/// Whether Pitwall starts at log-in for the user running this process.
pub fn is_enabled<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<bool, String> {
    platform::is_enabled(app)
}

/// Turns start at log-in on or off for the user running this process and returns what is set afterwards.
pub fn set_enabled<R: tauri::Runtime>(app: &tauri::AppHandle<R>, enabled: bool) -> Result<bool, String> {
    platform::set_enabled(app, enabled)
}

#[cfg(windows)]
mod platform {
    use paddock_service::login::{Autostart, ProcessElevation};

    fn entry() -> Result<paddock_service::login::UserRunEntry, String> {
        let exe = std::env::current_exe().map_err(|e| format!("the app's folder cannot be found: {e}"))?;
        Ok(super::run_entry(
            exe,
            Box::new(paddock_service::RealSystem::new()),
            Box::new(ProcessElevation),
        ))
    }

    pub fn is_enabled<R: tauri::Runtime>(_app: &tauri::AppHandle<R>) -> Result<bool, String> {
        entry()?.is_on()
    }

    pub fn set_enabled<R: tauri::Runtime>(_app: &tauri::AppHandle<R>, enabled: bool) -> Result<bool, String> {
        let mut entry = entry()?;
        entry.set(enabled)?;
        entry.is_on()
    }
}

#[cfg(not(windows))]
mod platform {
    use tauri_plugin_autostart::ManagerExt;

    pub fn is_enabled<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<bool, String> {
        app.autolaunch().is_enabled().map_err(|e| e.to_string())
    }

    pub fn set_enabled<R: tauri::Runtime>(app: &tauri::AppHandle<R>, enabled: bool) -> Result<bool, String> {
        let launch = app.autolaunch();
        let done = if enabled { launch.enable() } else { launch.disable() };
        done.map_err(|e| e.to_string())?;
        launch.is_enabled().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_service::login::Autostart;
    use paddock_service::testing::FakeSystem;

    const EXE: &str = r"C:\Program Files\Pitwall\pitwall-app.exe";

    struct Elev(bool);
    impl Elevation for Elev {
        fn is_full(&mut self) -> bool {
            self.0
        }
    }

    fn entry(sys: &FakeSystem, elevated: bool) -> UserRunEntry {
        run_entry(EXE.into(), Box::new(sys.clone()), Box::new(Elev(elevated)))
    }

    #[test]
    fn the_run_value_is_pitwalls_name_and_starts_it_hidden() {
        let e = entry(&FakeSystem::new(), false);
        assert_eq!(e.command(), format!(r#""{EXE}" --hidden"#));
        assert_eq!(RUN_VALUE, "Pitwall");
    }

    #[test]
    fn turning_it_on_writes_the_users_value_only_and_reads_back_on() {
        let sys = FakeSystem::new();
        sys.emulate_run_registry();
        let mut e = entry(&sys, false);
        assert_eq!(e.is_on(), Ok(false));
        e.set(true).unwrap();
        assert_eq!(e.is_on(), Ok(true));
        assert!(
            sys.commands().iter().all(|c| !c.contains("HKLM") || !c.contains("add")),
            "{:?}",
            sys.commands()
        );
        e.set(false).unwrap();
        assert_eq!(e.is_on(), Ok(false));
    }

    #[test]
    fn an_elevated_app_never_writes_the_switch() {
        let sys = FakeSystem::new();
        sys.emulate_run_registry();
        let err = entry(&sys, true).set(true).unwrap_err();
        assert!(err.contains("administrator"), "{err}");
        assert!(sys.commands().iter().all(|c| !c.starts_with("reg add")), "{:?}", sys.commands());
    }
}
