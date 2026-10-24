//! Launch at login: one Settings switch that means "the app opens at login (in the tray) and its background service
//! keeps working while the window is closed" (Dyno's design). Two registrations back it, each tried even when the
//! other fails: the app's own login item (an [`Autostart`], e.g. `tauri-plugin-autostart`) and the service
//! ([`Manager::install`] / [`Manager::uninstall`]).
//!
//! The section only stores the switch; applying it registers with the OS, which is slow, so the app runs
//! [`apply_launch_at_login`] on a background thread from its section's `apply` (settings changes must return at once).

use crate::manager::Manager;
use crate::status::LOGIN_SECTION;
use paddock_settings::{FieldKind, SectionOwner, SettingField, SettingsSection};
use std::sync::atomic::AtomicBool;

/// The switch's key in [`launch_at_login_section`].
pub const LAUNCH_AT_LOGIN: &str = "launchAtLogin";

/// The app's own start-at-login entry.
pub trait Autostart: Send {
    /// Turns it on or off.
    fn set(&mut self, on: bool) -> Result<(), String>;
}

/// The settings section (`pd.login`): one switch, off by default (nothing starts at login unasked). `with_service`
/// says whether the app has a background service the switch also covers.
pub fn launch_at_login_section(app: &str, with_service: bool) -> SettingsSection {
    let what = if with_service {
        format!(
            "{app} opens in the menu bar or tray when you log in, and its background service keeps working while the window is closed."
        )
    } else {
        format!("{app} opens in the menu bar or tray when you log in.")
    };
    SettingsSection::new(LOGIN_SECTION, "Start at login", SectionOwner::Shell)
        .describe("What starts when you log in to this computer.")
        .field(
            SettingField::new(LAUNCH_AT_LOGIN, "Open at login", FieldKind::Bool, false)
                .describe(what),
        )
}

/// Applies the switch to both registrations. Each is attempted even if the other fails; the error names what did not
/// work and what to do (the setting itself stays saved).
pub fn apply_launch_at_login(
    on: bool,
    service: Option<&mut Manager>,
    autostart: &mut dyn Autostart,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut problems = Vec::new();
    if let Some(m) = service {
        let result = if on {
            m.install(cancel).map(drop)
        } else {
            m.uninstall()
        };
        if let Err(e) = result {
            problems.push(format!(
                "the background service could not be {}: {e}",
                if on { "registered" } else { "removed" }
            ));
        }
    }
    if let Err(e) = autostart.set(on) {
        problems.push(format!(
            "the app could not be {} the login items: {e}",
            if on { "added to" } else { "removed from" }
        ));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Your setting is saved, but {}.",
            problems.join("; and ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{Target, WindowsRun};
    use crate::spec::ServiceSpec;
    use crate::testing::FakeSystem;

    struct Auto(Vec<bool>, bool);
    impl Autostart for Auto {
        fn set(&mut self, on: bool) -> Result<(), String> {
            self.0.push(on);
            if self.1 { Err("denied".into()) } else { Ok(()) }
        }
    }

    fn manager(sys: &FakeSystem, dir: &std::path::Path) -> Manager {
        let spec = ServiceSpec::builder("net.decryptology.dyno.agent", "Dyno", "dyno-agent")
            .args(["run"])
            .build()
            .unwrap();
        sys.put_program("C:\\Dyno\\dyno-agent.exe");
        let backend = WindowsRun::new(
            spec,
            Target::exe("C:\\Dyno\\dyno-agent.exe"),
            Box::new(sys.clone()),
        );
        Manager::new(
            "Dyno",
            Box::new(backend),
            dir,
            "net.decryptology.dyno.agent",
            "1.0.0",
            || true,
        )
    }

    #[test]
    fn the_section_is_one_switch_off_by_default() {
        let s = launch_at_login_section("Dyno", true);
        assert!(s.check().is_ok());
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["id"], LOGIN_SECTION);
        assert!(json.to_string().contains("background service"));
        assert!(
            !serde_json::to_string(&launch_at_login_section("Spotter", false))
                .unwrap()
                .contains("background service")
        );
    }

    #[test]
    fn the_switch_registers_or_removes_both_and_says_what_failed() {
        let d = tempfile::tempdir().unwrap();
        let sys = FakeSystem::new();
        let mut m = manager(&sys, d.path());
        let mut a = Auto(Vec::new(), false);
        let cancel = AtomicBool::new(false);
        apply_launch_at_login(true, Some(&mut m), &mut a, &cancel).unwrap();
        assert!(sys.commands().iter().any(|c| c.starts_with("reg add")));
        assert_eq!(m.recorded().registered_version.as_deref(), Some("1.0.0"));
        apply_launch_at_login(false, Some(&mut m), &mut a, &cancel).unwrap();
        assert!(sys.commands().iter().any(|c| c.starts_with("reg delete")));
        assert_eq!(a.0, [true, false]);
        // The app's own item fails: the service is still registered and the message says so.
        let mut failing = Auto(Vec::new(), true);
        let e = apply_launch_at_login(true, None, &mut failing, &cancel).unwrap_err();
        assert!(
            e.starts_with("Your setting is saved")
                && e.contains("added to the login items: denied"),
            "{e}"
        );
        // Both fail.
        sys.on("reg add", |_, _| Ok(crate::port::Output::code(1)));
        let e = apply_launch_at_login(true, Some(&mut m), &mut failing, &cancel).unwrap_err();
        assert!(
            e.contains("could not be registered") && e.contains("; and "),
            "{e}"
        );
    }
}
