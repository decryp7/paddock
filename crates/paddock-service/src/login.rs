//! Launch at login: one Settings switch that means "the app opens at login (in the tray) and its background service
//! keeps working while the window is closed" (Dyno's design). Two registrations back it, each tried even when the
//! other fails: the app's own login item (an [`Autostart`], e.g. `tauri-plugin-autostart`) and the service
//! ([`Manager::install`] / [`Manager::uninstall`]).
//!
//! The section only stores the switch; applying it registers with the OS, which is slow, so the app runs
//! [`apply_launch_at_login`] on a background thread from its section's `apply` (settings changes must return at once).
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use crate::manager::Manager;
use crate::parse::reg_value;
use crate::paths::{REG, WINDOWS_MACHINE_RUN_KEY, WINDOWS_RUN_KEY};
use crate::port::{AppService, AppServiceStatus, RegisterError, System};
use crate::render::{windows_command_line, windows_program_of};
use crate::status::LOGIN_SECTION;
use paddock_settings::{FieldKind, SectionOwner, SettingField, SettingsSection};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// The switch's key in [`launch_at_login_section`].
pub const LAUNCH_AT_LOGIN: &str = "launchAtLogin";

/// The app's own start-at-login entry.
pub trait Autostart: Send {
    /// Turns it on or off.
    fn set(&mut self, on: bool) -> Result<(), String>;
}

/// Whether this process runs with a full (elevated) token: `TokenElevationType` is `TokenElevationTypeFull`. The plain
/// `TokenElevation` flag is not used: with UAC off, and for the built-in Administrator, every process counts as elevated
/// by it and a per-user write would be refused for no reason.
pub trait Elevation: Send {
    /// Whether the token's elevation type is Full.
    fn is_full(&mut self) -> bool;
}

/// The running process's own token (Windows); other systems have no such elevation, so it is never full.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessElevation;

impl Elevation for ProcessElevation {
    fn is_full(&mut self) -> bool {
        crate::native::process_has_full_token()
    }
}

/// The app's per-user start-at-login entry on Windows: the value `name` in `HKCU\...\Run`, `"exe" args`, written through
/// `reg.exe` on the same backend as the service's `Run` entry ([`crate::backend::WindowsRun`]).
///
/// - It never writes (or deletes) while the process has a full elevated token ([`Elevation`]): an elevated process may
///   belong to a different account than the one logging in, and `tauri-plugin-autostart` would write the machine-wide key.
/// - An `HKLM\...\Run` value of the same name that starts this very program (what an elevated Dyno wrote earlier) is
///   removed, only while elevated (only then can it be).
/// - A write is read back; a value that is not there afterwards is an error.
pub struct UserRunEntry {
    name: String,
    exe: PathBuf,
    args: Vec<String>,
    sys: Box<dyn System>,
    elevation: Box<dyn Elevation>,
}

impl UserRunEntry {
    /// The entry `name` starting `exe args`.
    pub fn new(
        name: &str,
        exe: PathBuf,
        args: &[&str],
        sys: Box<dyn System>,
        elevation: Box<dyn Elevation>,
    ) -> Self {
        Self {
            name: name.to_string(),
            exe,
            args: args.iter().map(|a| (*a).to_string()).collect(),
            sys,
            elevation,
        }
    }

    /// The command the value holds.
    pub fn command(&self) -> String {
        windows_command_line(&self.exe.to_string_lossy(), &self.args)
    }

    fn value_of(&mut self, key: &str) -> Result<Option<String>, String> {
        match self.sys.run(REG, &["query", key, "/v", &self.name]) {
            Ok(o) if o.success() => Ok(reg_value(&o.stdout, &self.name).map(|(_, data)| data)),
            Ok(_) => Ok(None),
            Err(e) => Err(format!("reg.exe {e}")),
        }
    }

    /// Removes the machine-wide value if it starts this program. Returns what happened, for the log.
    fn clear_machine_value(&mut self) -> String {
        let found = match self.value_of(WINDOWS_MACHINE_RUN_KEY) {
            Ok(Some(data)) => data,
            Ok(None) => return "no machine-wide Run value".into(),
            Err(e) => return format!("the machine-wide Run value could not be read: {e}"),
        };
        let ours = windows_program_of(&found)
            .is_some_and(|p| p.eq_ignore_ascii_case(&self.exe.to_string_lossy()));
        if !ours {
            return "a machine-wide Run value of another program was left alone".into();
        }
        match self.sys.run(
            REG,
            &["delete", WINDOWS_MACHINE_RUN_KEY, "/v", &self.name, "/f"],
        ) {
            Ok(o) if o.success() => "removed the machine-wide Run value written earlier".into(),
            Ok(o) => format!(
                "the machine-wide Run value could not be removed: {}",
                o.reason()
            ),
            Err(e) => format!("the machine-wide Run value could not be removed: reg.exe {e}"),
        }
    }
}

impl Autostart for UserRunEntry {
    fn set(&mut self, on: bool) -> Result<(), String> {
        if self.elevation.is_full() {
            let note = self.clear_machine_value();
            log::info!("start at login: running elevated, per-user entry not written; {note}");
            return Err(format!(
                "{} is running as administrator, and start at login is set per user. {note}. Open it without \"Run as administrator\" and turn start at login on again.",
                self.name
            ));
        }
        if !on {
            return match self
                .sys
                .run(REG, &["delete", WINDOWS_RUN_KEY, "/v", &self.name, "/f"])
            {
                // Exit 1: the value is not there, which is the goal.
                Ok(o) if o.success() || o.code == Some(1) => {
                    log::info!("start at login: per-user Run value {} removed", self.name);
                    Ok(())
                }
                Ok(o) => Err(format!("reg delete: {}", o.reason())),
                Err(e) => Err(format!("reg.exe {e}")),
            };
        }
        let command = self.command();
        match self.sys.run(
            REG,
            &[
                "add",
                WINDOWS_RUN_KEY,
                "/v",
                &self.name,
                "/t",
                "REG_SZ",
                "/d",
                &command,
                "/f",
            ],
        ) {
            Ok(o) if o.success() => {}
            Ok(o) => return Err(format!("reg add: {}", o.reason())),
            Err(e) => return Err(format!("reg.exe {e}")),
        }
        let back = self.value_of(WINDOWS_RUN_KEY)?;
        if back.as_deref() != Some(command.as_str()) {
            return Err(format!(
                "the Run value {} reads back as {back:?}, not the command written",
                self.name
            ));
        }
        log::info!(
            "start at login: per-user Run value {} = {command}",
            self.name
        );
        Ok(())
    }
}

/// The per-user "start at login was applied once" marker: a small file in the user's own data folder, so another user of the
/// same PC gets their own first run.
#[derive(Debug, Clone)]
pub struct FirstRunMarker {
    path: PathBuf,
}

impl FirstRunMarker {
    /// The marker at `path` (in the per-user data folder).
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Whether the marker is there.
    pub fn is_set(&self, sys: &mut dyn System) -> bool {
        sys.read(&self.path).is_ok()
    }

    /// Writes the marker (`note` says what was decided).
    pub fn record(&self, sys: &mut dyn System, note: &str) -> Result<(), String> {
        sys.write(&self.path, note).map_err(|e| {
            format!(
                "the first-run marker {} could not be written: {e}",
                self.path.display()
            )
        })
    }

    /// Where it is.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// The app itself as a login item through `SMAppService.mainApp` (macOS 13+), behind the [`AppService`] port
/// ([`crate::native::SmAppService::main_app`] is the real one).
///
/// Registering while approval in Login Items is pending is not a failure (the user has to allow it there); a status that
/// is already `Enabled` is left alone. The outcome is logged.
pub struct AppServiceAutostart {
    app: Box<dyn AppService>,
}

impl AppServiceAutostart {
    /// The login item behind `app`.
    pub fn new(app: Box<dyn AppService>) -> Self {
        Self { app }
    }
}

impl Autostart for AppServiceAutostart {
    fn set(&mut self, on: bool) -> Result<(), String> {
        let status = self.app.status();
        if on {
            if status == AppServiceStatus::Enabled {
                log::info!("start at login: the app is already a login item");
                return Ok(());
            }
            return match self.app.register() {
                Ok(()) => {
                    log::info!("start at login: the app was registered as a login item");
                    Ok(())
                }
                Err(RegisterError::NeedsApproval) => {
                    log::info!(
                        "start at login: registered; waiting for the user to allow it in Login Items"
                    );
                    Ok(())
                }
                Err(e) => Err(e.to_string()),
            };
        }
        if matches!(
            status,
            AppServiceStatus::NotRegistered | AppServiceStatus::NotFound
        ) {
            return Ok(());
        }
        self.app.unregister()?;
        log::info!("start at login: the app was removed from the login items");
        Ok(())
    }
}

/// The app's start-at-login entry on Linux: an XDG autostart file `<config>/autostart/<stem>.desktop` that starts
/// `exe args` (with the hidden flag, so it opens in the tray), written through the [`System`] port. The desktop session
/// reads it at login (GNOME, KDE, Xfce and the others). A write is read back; the outcome is logged.
pub struct XdgAutostart {
    file: PathBuf,
    name: String,
    exe: PathBuf,
    args: Vec<String>,
    sys: Box<dyn System>,
}

impl XdgAutostart {
    /// The entry `stem`.desktop named `name` in `config_dir/autostart` (`config_dir` is `$XDG_CONFIG_HOME` or `~/.config`).
    pub fn new(
        config_dir: &Path,
        stem: &str,
        name: &str,
        exe: PathBuf,
        args: &[&str],
        sys: Box<dyn System>,
    ) -> Self {
        Self {
            file: config_dir.join("autostart").join(format!("{stem}.desktop")),
            name: name.to_string(),
            exe,
            args: args.iter().map(|a| (*a).to_string()).collect(),
            sys,
        }
    }

    /// The file's text.
    pub fn definition(&self) -> String {
        let quote = |s: &str| {
            let mut q = String::from("\"");
            for c in s.chars() {
                if matches!(c, '"' | '\\' | '$' | '`') {
                    q.push('\\');
                }
                q.push(c);
            }
            q.push('"');
            q
        };
        let mut exec = quote(&self.exe.to_string_lossy());
        for a in &self.args {
            exec.push(' ');
            exec.push_str(&quote(a));
        }
        format!(
            "[Desktop Entry]\nType=Application\nName={}\nExec={exec}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
            self.name
        )
    }
}

impl Autostart for XdgAutostart {
    fn set(&mut self, on: bool) -> Result<(), String> {
        if !on {
            return match self.sys.remove(&self.file) {
                Ok(()) => {
                    log::info!("start at login: removed {}", self.file.display());
                    Ok(())
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(format!("{} could not be removed: {e}", self.file.display())),
            };
        }
        let text = self.definition();
        self.sys
            .write(&self.file, &text)
            .map_err(|e| format!("{} could not be written: {e}", self.file.display()))?;
        match self.sys.read(&self.file) {
            Ok(back) if back == text => {
                log::info!("start at login: wrote {}", self.file.display());
                Ok(())
            }
            other => Err(format!(
                "{} does not read back as written ({other:?})",
                self.file.display()
            )),
        }
    }
}

/// Tells a change of the switch from the value the section is applied with at start: the first value is only
/// remembered (a stored "off" at start must never remove a service the user installed with Install).
#[derive(Debug, Default)]
pub struct LoginSwitch {
    last: std::sync::Mutex<Option<bool>>,
}

impl LoginSwitch {
    /// Whether `on` is a change by the user (not the first value, not the same value again).
    pub fn changed(&self, on: bool) -> bool {
        let mut last = self
            .last
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let changed = last.is_some_and(|l| l != on);
        *last = Some(on);
        changed
    }
}

/// An app without a login item of its own (only the service follows the switch).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoAutostart;

impl Autostart for NoAutostart {
    fn set(&mut self, _on: bool) -> Result<(), String> {
        Ok(())
    }
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

    struct Elev(bool);
    impl Elevation for Elev {
        fn is_full(&mut self) -> bool {
            self.0
        }
    }

    const APP: &str = r"C:\Program Files\Dyno\dyno-app.exe";
    const RUN_Q: &str = "reg query HKCU";
    const HKLM_Q: &str = "reg query HKLM";

    fn entry(sys: &FakeSystem, full: bool) -> UserRunEntry {
        UserRunEntry::new(
            "Dyno",
            APP.into(),
            &["--hidden"],
            Box::new(sys.clone()),
            Box::new(Elev(full)),
        )
    }

    /// A fake registry that keeps the HKCU value `reg add` writes, so the read-back sees it.
    fn registry(sys: &FakeSystem, drop_writes: bool) {
        let cell = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let w = cell.clone();
        sys.on("reg add HKCU", move |a, _| {
            if !drop_writes {
                *w.lock().unwrap() = a[a.iter().position(|x| x == "/d").unwrap() + 1].clone();
            }
            Ok(crate::port::Output::code(0))
        });
        sys.on(RUN_Q, move |_, _| {
            let v = cell.lock().unwrap().clone();
            Ok(if v.is_empty() {
                crate::port::Output::code(1)
            } else {
                crate::port::Output::ok(&format!("\n    Dyno    REG_SZ    {v}\n"))
            })
        });
    }

    #[test]
    fn turning_it_on_writes_the_users_value_hidden_and_reads_it_back() {
        let sys = FakeSystem::new();
        registry(&sys, false);
        entry(&sys, false).set(true).unwrap();
        let cmds = sys.commands();
        assert!(
            cmds.iter().any(|c| c.starts_with("reg add HKCU")
                && c.contains(r#""C:\Program Files\Dyno\dyno-app.exe" --hidden"#)),
            "{cmds:?}"
        );
        assert!(!cmds.iter().any(|c| c.contains("HKLM") && c.contains("add")));
    }

    #[test]
    fn a_write_that_is_not_there_afterwards_is_an_error() {
        let sys = FakeSystem::new();
        registry(&sys, true);
        let e = entry(&sys, false).set(true).unwrap_err();
        assert!(e.contains("reads back"), "{e}");
    }

    #[test]
    fn a_full_token_never_writes_the_per_user_value_but_removes_its_own_machine_value() {
        let sys = FakeSystem::new();
        sys.on(HKLM_Q, |_, _| {
            Ok(crate::port::Output::ok(&format!(
                "\n    Dyno    REG_SZ    \"{APP}\" --hidden\n"
            )))
        });
        for on in [true, false] {
            sys.clear_calls();
            let e = entry(&sys, true).set(on).unwrap_err();
            assert!(e.contains("administrator"), "{e}");
            let cmds = sys.commands();
            assert!(
                cmds.iter().any(|c| c.starts_with("reg delete HKLM")),
                "{cmds:?}"
            );
            assert!(
                !cmds
                    .iter()
                    .any(|c| c.starts_with("reg add") || c.starts_with("reg delete HKCU")),
                "{cmds:?}"
            );
        }
    }

    #[test]
    fn another_programs_machine_value_is_left_alone() {
        let sys = FakeSystem::new();
        sys.on(HKLM_Q, |_, _| {
            Ok(crate::port::Output::ok(
                "\n    Dyno    REG_SZ    \"C:\\Tools\\other.exe\" --hidden\n",
            ))
        });
        entry(&sys, true).set(true).unwrap_err();
        assert!(!sys.commands().iter().any(|c| c.starts_with("reg delete")));
    }

    #[test]
    fn turning_it_off_deletes_the_users_value_and_a_missing_one_is_fine() {
        let sys = FakeSystem::new();
        sys.on("reg delete HKCU", |_, _| Ok(crate::port::Output::code(1)));
        entry(&sys, false).set(false).unwrap();
        assert!(
            sys.commands()
                .iter()
                .any(|c| c.starts_with("reg delete HKCU"))
        );
    }

    #[test]
    fn the_first_run_marker_is_per_user_file_and_survives() {
        let sys = FakeSystem::new();
        let m = FirstRunMarker::new(r"C:\Users\a\AppData\Roaming\Dyno\login-default-applied");
        let mut s = sys.clone();
        assert!(!m.is_set(&mut s));
        m.record(&mut s, "applied").unwrap();
        assert!(m.is_set(&mut s));
        let other = FirstRunMarker::new(r"C:\Users\b\AppData\Roaming\Dyno\login-default-applied");
        assert!(
            !other.is_set(&mut s),
            "another user has their own first run"
        );
    }

    #[test]
    fn the_main_app_login_item_registers_once_and_waits_for_approval() {
        use crate::testing::FakeAppService;
        let sys = FakeSystem::new();
        let app = FakeAppService::new(&sys);
        let mut a = AppServiceAutostart::new(Box::new(app.clone()));
        a.set(true).unwrap();
        assert!(
            sys.calls()
                .iter()
                .any(|c| format!("{c:?}").contains("register"))
        );
        // Already enabled: nothing more is registered.
        sys.clear_calls();
        a.set(true).unwrap();
        assert!(
            !sys.calls()
                .iter()
                .any(|c| format!("{c:?}").contains("\"register"))
        );
        // Approval pending in Login Items is not a failure.
        let pending = FakeAppService::new(&sys);
        pending.register_results([Err(RegisterError::NeedsApproval)]);
        AppServiceAutostart::new(Box::new(pending))
            .set(true)
            .unwrap();
        // A real failure is reported.
        let broken = FakeAppService::new(&sys);
        broken.register_results([Err(RegisterError::Other("boom".into()))]);
        assert!(
            AppServiceAutostart::new(Box::new(broken))
                .set(true)
                .unwrap_err()
                .contains("boom")
        );
        // Off: removed when registered, left alone when not.
        sys.clear_calls();
        a.set(false).unwrap();
        assert!(
            sys.calls()
                .iter()
                .any(|c| format!("{c:?}").contains("unregister"))
        );
        sys.clear_calls();
        a.set(false).unwrap();
        assert!(
            !sys.calls()
                .iter()
                .any(|c| format!("{c:?}").contains("unregister"))
        );
    }

    fn xdg(sys: &FakeSystem) -> XdgAutostart {
        XdgAutostart::new(
            Path::new("/home/a/.config"),
            "dyno",
            "Dyno",
            "/usr/bin/dyno-app".into(),
            &["--hidden"],
            Box::new(sys.clone()),
        )
    }

    #[test]
    fn linux_writes_an_autostart_file_that_starts_the_app_hidden_and_removes_it() {
        let sys = FakeSystem::new();
        xdg(&sys).set(true).unwrap();
        let text = sys
            .file(Path::new("/home/a/.config/autostart/dyno.desktop"))
            .expect("the file");
        assert!(
            text.contains("Exec=\"/usr/bin/dyno-app\" \"--hidden\"\n"),
            "{text}"
        );
        assert!(text.starts_with("[Desktop Entry]\nType=Application\nName=Dyno\n"));
        xdg(&sys).set(false).unwrap();
        assert!(
            sys.file(Path::new("/home/a/.config/autostart/dyno.desktop"))
                .is_none()
        );
        xdg(&sys).set(false).unwrap();
    }

    #[test]
    fn linux_says_when_the_file_cannot_be_written() {
        let sys = FakeSystem::new();
        sys.fail_writes(Some(std::io::ErrorKind::PermissionDenied));
        let e = xdg(&sys).set(true).unwrap_err();
        assert!(e.contains("could not be written"), "{e}");
    }

    #[test]
    fn only_a_change_of_the_switch_acts() {
        let s = LoginSwitch::default();
        assert!(!s.changed(false), "the value at start");
        assert!(!s.changed(false));
        assert!(s.changed(true));
        assert!(!s.changed(true));
        assert!(s.changed(false));
        assert!(NoAutostart.set(true).is_ok());
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
