//! What starts at login on Linux: XDG autostart `.desktop` files (the user's own and the system's) and enabled
//! systemd **user** services. Listing is read-only. Changing an entry touches only the user's own files: a `.desktop`
//! file in `~/.config/autostart` (a system entry is switched off by an override file with `Hidden=true`, which is how
//! desktops do it), or `systemctl --user enable|disable` for user services. System files are never written.

use super::paths::{home_dir, read_text};
use super::tools::Tools;
use crate::parse::desktop_entry::{self, DesktopEntry};
use crate::parse::linux_tools::unit_files;
use dyno_core::ports::{PlatformError, StartupItems};
use dyno_core::startup::{exe_of_command, ImpactRating, StartupEntry, StartupImpact, StartupLocation, StartupState};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Most files read from one autostart folder.
const MAX_FILES: usize = 1_000;
/// Most bytes of the user's own autostart file rewritten (larger ones are not desktop entries).
const MAX_ENTRY_BYTES: usize = 64 * 1024;
/// How long `systemctl` may take.
const SYSTEMCTL_TIMEOUT: Duration = Duration::from_secs(10);

const USER_PREFIX: &str = "autostart:";
const SYSTEM_PREFIX: &str = "autostart-system:";
const UNIT_PREFIX: &str = "systemd-user:";

/// What an entry id refers to, parsed once from the untrusted id.
#[derive(Debug, PartialEq, Eq)]
enum Key {
    /// A file in the user's autostart folder.
    UserFile(String),
    /// A file in a system autostart folder (switched through a user override).
    SystemFile(String),
    /// A systemd user service.
    Unit(String),
}

impl Key {
    fn parse(id: &str) -> Result<Key, PlatformError> {
        let bad = || PlatformError::Failed(format!("\"{id}\" is not a startup entry Dyno can change on Linux."));
        if let Some(f) = id.strip_prefix(USER_PREFIX) {
            return plain_desktop_name(f).map(|f| Key::UserFile(f.to_string())).ok_or_else(bad);
        }
        if let Some(f) = id.strip_prefix(SYSTEM_PREFIX) {
            return plain_desktop_name(f).map(|f| Key::SystemFile(f.to_string())).ok_or_else(bad);
        }
        if let Some(u) = id.strip_prefix(UNIT_PREFIX) {
            return plain_unit_name(u).then(|| Key::Unit(u.to_string())).ok_or_else(bad);
        }
        Err(bad())
    }
}

/// A file name that is a `.desktop` file directly in a folder (no separators, no dot-files, no `..`).
fn plain_desktop_name(name: &str) -> Option<&str> {
    let ok = name.len() <= 255
        && name.ends_with(".desktop")
        && name.len() > ".desktop".len()
        && !name.starts_with('.')
        && !name.contains(['/', '\\', '\0']);
    ok.then_some(name)
}

/// A systemd service name: letters, digits and `:_.@\-`, ending in `.service`, not starting with `-` or `.`.
fn plain_unit_name(name: &str) -> bool {
    name.ends_with(".service")
        && name.len() <= 256
        && !name.starts_with(['-', '.'])
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '.' | '@' | '\\' | '-'))
}

/// Startup items for Linux.
pub struct LinuxStartup {
    user_dir: PathBuf,
    system_dirs: Vec<PathBuf>,
    tools: Arc<dyn Tools>,
}

impl LinuxStartup {
    /// Startup items with explicit folders (tests use temporary ones).
    pub fn new(user_dir: PathBuf, system_dirs: Vec<PathBuf>, tools: Arc<dyn Tools>) -> Self {
        LinuxStartup {
            user_dir,
            system_dirs,
            tools,
        }
    }

    /// The real folders: `$XDG_CONFIG_HOME/autostart` (or `~/.config/autostart`) and `/etc/xdg/autostart`.
    pub fn for_this_user(tools: Arc<dyn Tools>) -> Self {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home_dir().map(|h| h.join(".config")))
            .unwrap_or_else(|| std::env::temp_dir().join("dyno-no-home"));
        LinuxStartup::new(config.join("autostart"), vec![PathBuf::from("/etc/xdg/autostart")], tools)
    }

    fn read_entries(dir: &Path) -> Vec<(String, DesktopEntry)> {
        let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
        let mut files: Vec<String> = rd
            .flatten()
            .take(MAX_FILES)
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| plain_desktop_name(n).is_some())
            .collect();
        files.sort();
        files
            .into_iter()
            .filter_map(|n| read_text(&dir.join(&n)).map(|t| (n, desktop_entry::parse(&t))))
            .collect()
    }

    fn entry(id: String, name: String, command: String, location: StartupLocation, enabled: bool) -> StartupEntry {
        StartupEntry {
            id,
            name,
            exe: exe_of_command(&command),
            command,
            location,
            state: if enabled { StartupState::Enabled } else { StartupState::Disabled },
            publisher: None,
            impact: StartupImpact {
                cpu_secs: 0.0,
                disk_bytes: 0,
                rating: ImpactRating::NotMeasured,
            },
        }
    }

    fn list_units(&self) -> Vec<StartupEntry> {
        let args = [
            "--user",
            "list-unit-files",
            "--type=service",
            "--state=enabled",
            "--no-legend",
            "--no-pager",
        ];
        // No user bus (a container, ssh without a session) or no systemd is not an error: there is just nothing to list.
        let Ok(out) = self.tools.run("systemctl", &args, SYSTEMCTL_TIMEOUT) else {
            return Vec::new();
        };
        if !out.ok {
            return Vec::new();
        }
        unit_files(&out.stdout)
            .into_iter()
            .filter(|u| plain_unit_name(&u.name))
            .map(|u| {
                let label = u.name.trim_end_matches(".service").to_string();
                Self::entry(
                    format!("{UNIT_PREFIX}{}", u.name),
                    label,
                    format!("systemd user service {}", u.name),
                    StartupLocation::LoginItem,
                    true,
                )
            })
            .collect()
    }

    fn write_user_file(&self, name: &str, text: &str) -> Result<(), PlatformError> {
        std::fs::create_dir_all(&self.user_dir)
            .map_err(|e| PlatformError::Failed(format!("Could not create {}: {e}", self.user_dir.display())))?;
        let target = self.user_dir.join(name);
        let tmp = self.user_dir.join(format!(".{name}.dyno-tmp"));
        std::fs::write(&tmp, text).map_err(|e| PlatformError::Failed(format!("Could not write {}: {e}", target.display())))?;
        std::fs::rename(&tmp, &target).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            PlatformError::Failed(format!("Could not update {}: {e}", target.display()))
        })
    }

    fn set_file(&self, name: &str, enable: bool) -> Result<(), PlatformError> {
        let user_path = self.user_dir.join(name);
        match read_text(&user_path) {
            Some(text) if text.len() <= MAX_ENTRY_BYTES => {
                if enable && !desktop_entry::parse(&text).has_exec {
                    // A pure override of a system entry: removing it hands control back to the system file.
                    return std::fs::remove_file(&user_path)
                        .map_err(|e| PlatformError::Failed(format!("Could not remove {}: {e}", user_path.display())));
                }
                self.write_user_file(name, &desktop_entry::set_hidden(&text, !enable))
            }
            Some(_) => Err(PlatformError::Failed(format!(
                "{} is too large to be an autostart entry.",
                user_path.display()
            ))),
            None => {
                let system = self
                    .system_dirs
                    .iter()
                    .find_map(|d| read_text(&d.join(name)).map(|t| desktop_entry::parse(&t)));
                match (system, enable) {
                    (None, _) => Err(PlatformError::NotFound(format!("{name} no longer exists."))),
                    // Already on: nothing to write.
                    (Some(_), true) => Ok(()),
                    (Some(e), false) => {
                        let label = if e.name.is_empty() { name.to_string() } else { e.name };
                        self.write_user_file(name, &desktop_entry::override_hidden(&label))
                    }
                }
            }
        }
    }

    fn set_unit(&self, unit: &str, enable: bool) -> Result<(), PlatformError> {
        let verb = if enable { "enable" } else { "disable" };
        let out = self.tools.run("systemctl", &["--user", verb, "--", unit], SYSTEMCTL_TIMEOUT)?;
        if out.ok {
            Ok(())
        } else {
            Err(PlatformError::Failed(format!(
                "systemctl could not {verb} {unit}: {}",
                out.first_error_line()
            )))
        }
    }
}

fn display_name(entry: &DesktopEntry, file: &str) -> String {
    if entry.name.is_empty() {
        file.trim_end_matches(".desktop").to_string()
    } else {
        entry.name.clone()
    }
}

impl StartupItems for LinuxStartup {
    fn list(&mut self) -> Result<Vec<StartupEntry>, PlatformError> {
        let mut out = Vec::new();
        let user = Self::read_entries(&self.user_dir);
        for (file, e) in user.iter().filter(|(_, e)| e.has_exec) {
            out.push(Self::entry(
                format!("{USER_PREFIX}{file}"),
                display_name(e, file),
                e.exec.clone(),
                StartupLocation::UserStartupFolder,
                e.enabled(),
            ));
        }
        let mut listed: Vec<String> = Vec::new();
        for dir in &self.system_dirs {
            for (file, e) in Self::read_entries(dir) {
                let user_file = user.iter().find(|(f, _)| *f == file).map(|(_, u)| u);
                // The same file name with an Exec in the user's folder replaces the system entry entirely.
                if !e.has_exec || listed.contains(&file) || user_file.is_some_and(|u| u.has_exec) {
                    continue;
                }
                listed.push(file.clone());
                // A user file without Exec is an override: it can only switch the entry off.
                let enabled = e.enabled() && user_file.is_none_or(DesktopEntry::enabled);
                out.push(Self::entry(
                    format!("{SYSTEM_PREFIX}{file}"),
                    display_name(&e, &file),
                    e.exec.clone(),
                    StartupLocation::CommonStartupFolder,
                    enabled,
                ));
            }
        }
        out.extend(self.list_units());
        Ok(out)
    }

    fn set_state(&mut self, entry: &StartupEntry, state: StartupState) -> Result<(), PlatformError> {
        let enable = match state {
            StartupState::Enabled => true,
            StartupState::Disabled => false,
            StartupState::Delayed(_) => {
                return Err(PlatformError::Unsupported(
                    "Linux has no delayed start for login entries; turn the entry off or on.".into(),
                ))
            }
        };
        match Key::parse(&entry.id)? {
            Key::UserFile(name) | Key::SystemFile(name) => self.set_file(&name, enable),
            Key::Unit(unit) => self.set_unit(&unit, enable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tools::fake::FakeTools;
    use super::super::tools::ToolOutput;
    use super::*;

    fn w(dir: &Path, name: &str, text: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), text).unwrap();
    }

    struct Env {
        _dir: tempfile::TempDir,
        user: PathBuf,
        system: PathBuf,
        tools: Arc<FakeTools>,
        startup: LinuxStartup,
    }

    fn env() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("home/.config/autostart");
        let system = dir.path().join("etc/xdg/autostart");
        let tools = Arc::new(FakeTools::default());
        let startup = LinuxStartup::new(user.clone(), vec![system.clone()], tools.clone());
        Env {
            _dir: dir,
            user,
            system,
            tools,
            startup,
        }
    }

    fn find<'a>(l: &'a [StartupEntry], id: &str) -> &'a StartupEntry {
        l.iter()
            .find(|e| e.id == id)
            .unwrap_or_else(|| panic!("{id} not in {:?}", l.iter().map(|e| &e.id).collect::<Vec<_>>()))
    }

    fn ok(stdout: &str) -> Result<ToolOutput, PlatformError> {
        Ok(ToolOutput {
            ok: true,
            stdout: stdout.into(),
            stderr: String::new(),
        })
    }

    #[test]
    fn lists_user_system_and_unit_entries_with_overrides() {
        let mut e = env();
        w(
            &e.user,
            "mine.desktop",
            "[Desktop Entry]\nName=Mine\nExec=\"/opt/mine/run\" --tray\n",
        );
        w(
            &e.user,
            "paused.desktop",
            "[Desktop Entry]\nName=Paused\nExec=paused\nHidden=true\n",
        );
        w(
            &e.user,
            "gnome.desktop",
            "[Desktop Entry]\nExec=gnome-thing\nX-GNOME-Autostart-enabled=false\n",
        );
        w(&e.user, "cloud.desktop", "[Desktop Entry]\nName=Cloud (mine)\nExec=my-cloud\n");
        w(&e.user, "notes.txt", "ignored");
        w(&e.system, "cloud.desktop", "[Desktop Entry]\nName=Cloud\nExec=cloud-sync\n");
        w(
            &e.system,
            "print.desktop",
            "[Desktop Entry]\nName=Print applet\nExec=print-applet\n",
        );
        w(&e.system, "update.desktop", "[Desktop Entry]\nName=Updater\nExec=updater\n");
        w(
            &e.user,
            "update.desktop",
            "[Desktop Entry]\nType=Application\nName=Updater\nHidden=true\n",
        );
        e.tools
            .answer("systemctl", ok("pipewire.service enabled enabled\nsyncthing.service enabled -\n"));
        let list = e.startup.list().unwrap();
        let mine = find(&list, "autostart:mine.desktop");
        assert_eq!(
            (mine.name.as_str(), mine.state, mine.location.clone()),
            ("Mine", StartupState::Enabled, StartupLocation::UserStartupFolder)
        );
        assert_eq!(mine.exe.as_deref(), Some("/opt/mine/run"));
        assert_eq!(find(&list, "autostart:paused.desktop").state, StartupState::Disabled);
        let gnome = find(&list, "autostart:gnome.desktop");
        assert_eq!((gnome.name.as_str(), gnome.state), ("gnome", StartupState::Disabled));
        assert!(
            list.iter().all(|e| e.id != "autostart-system:cloud.desktop"),
            "the user's file replaces the system one"
        );
        assert_eq!(find(&list, "autostart-system:print.desktop").state, StartupState::Enabled);
        assert_eq!(
            find(&list, "autostart-system:update.desktop").state,
            StartupState::Disabled,
            "disabled by a user override"
        );
        assert!(
            list.iter().all(|e| e.id != "autostart:update.desktop"),
            "an override is not an entry of its own"
        );
        let unit = find(&list, "systemd-user:syncthing.service");
        assert_eq!((unit.name.as_str(), unit.state), ("syncthing", StartupState::Enabled));
        assert!(!unit.location.needs_admin());
        assert_eq!(
            e.tools.calls(),
            ["systemctl --user list-unit-files --type=service --state=enabled --no-legend --no-pager"]
        );
    }

    #[test]
    fn listing_survives_missing_folders_and_systemd() {
        let mut e = env();
        assert_eq!(e.startup.list().unwrap(), vec![]);
        e.tools.answer(
            "systemctl",
            Ok(ToolOutput {
                ok: false,
                stdout: String::new(),
                stderr: "Failed to connect to bus".into(),
            }),
        );
        assert_eq!(e.startup.list().unwrap(), vec![]);
    }

    #[test]
    fn turning_a_user_entry_off_and_on_edits_only_the_flag() {
        let mut e = env();
        w(&e.user, "mine.desktop", "[Desktop Entry]\nName=Mine\nExec=mine\nComment=keep me\n");
        let entry = find(&e.startup.list().unwrap(), "autostart:mine.desktop").clone();
        e.startup.set_state(&entry, StartupState::Disabled).unwrap();
        let text = std::fs::read_to_string(e.user.join("mine.desktop")).unwrap();
        assert!(text.contains("Hidden=true") && text.contains("Comment=keep me"), "{text}");
        assert_eq!(
            find(&e.startup.list().unwrap(), "autostart:mine.desktop").state,
            StartupState::Disabled
        );
        e.startup.set_state(&entry, StartupState::Enabled).unwrap();
        assert_eq!(
            find(&e.startup.list().unwrap(), "autostart:mine.desktop").state,
            StartupState::Enabled
        );
        assert!(!std::fs::read_dir(&e.user)
            .unwrap()
            .flatten()
            .any(|f| f.file_name().to_string_lossy().contains("dyno-tmp")));
    }

    #[test]
    fn a_system_entry_is_switched_by_a_user_override_and_the_system_file_is_untouched() {
        let mut e = env();
        let sys = "[Desktop Entry]\nName=Print applet\nExec=print-applet\n";
        w(&e.system, "print.desktop", sys);
        let entry = find(&e.startup.list().unwrap(), "autostart-system:print.desktop").clone();
        e.startup.set_state(&entry, StartupState::Disabled).unwrap();
        assert_eq!(std::fs::read_to_string(e.system.join("print.desktop")).unwrap(), sys);
        let ov = std::fs::read_to_string(e.user.join("print.desktop")).unwrap();
        assert!(ov.contains("Hidden=true") && ov.contains("Name=Print applet"), "{ov}");
        assert_eq!(
            find(&e.startup.list().unwrap(), "autostart-system:print.desktop").state,
            StartupState::Disabled
        );
        e.startup.set_state(&entry, StartupState::Enabled).unwrap();
        assert!(
            !e.user.join("print.desktop").exists(),
            "the override is removed, not left half-edited"
        );
        assert_eq!(
            find(&e.startup.list().unwrap(), "autostart-system:print.desktop").state,
            StartupState::Enabled
        );
        // Enabling what is already on writes nothing.
        e.startup.set_state(&entry, StartupState::Enabled).unwrap();
        assert!(!e.user.join("print.desktop").exists());
    }

    fn unit_entry(unit: &str) -> StartupEntry {
        LinuxStartup::entry(
            format!("{UNIT_PREFIX}{unit}"),
            unit.into(),
            unit.into(),
            StartupLocation::LoginItem,
            true,
        )
    }

    #[test]
    fn systemd_user_units_use_systemctl_and_report_failures() {
        let mut e = env();
        e.tools.answer("systemctl", ok(""));
        let entry = unit_entry("syncthing.service");
        e.startup.set_state(&entry, StartupState::Disabled).unwrap();
        e.startup.set_state(&entry, StartupState::Enabled).unwrap();
        assert_eq!(
            e.tools.calls(),
            [
                "systemctl --user disable -- syncthing.service",
                "systemctl --user enable -- syncthing.service"
            ]
        );
        e.tools.answer(
            "systemctl",
            Ok(ToolOutput {
                ok: false,
                stdout: String::new(),
                stderr: "Unit file x does not exist.\n".into(),
            }),
        );
        assert!(
            matches!(e.startup.set_state(&entry, StartupState::Disabled), Err(PlatformError::Failed(m)) if m.contains("does not exist"))
        );
        e.tools.answer(
            "systemctl",
            Err(PlatformError::Timeout("systemctl did not finish within 10 s.".into())),
        );
        assert!(matches!(
            e.startup.set_state(&entry, StartupState::Disabled),
            Err(PlatformError::Timeout(_))
        ));
    }

    #[test]
    fn hostile_ids_and_unsupported_states_are_refused_before_touching_anything() {
        let mut e = env();
        for id in [
            "autostart:../../../etc/passwd.desktop",
            "autostart:a/b.desktop",
            "autostart:.hidden.desktop",
            "autostart:.desktop",
            "autostart:x.txt",
            "autostart-system:/etc/x.desktop",
            "systemd-user:--now.service",
            "systemd-user:a b.service",
            "systemd-user:x.timer",
            "random",
            "",
        ] {
            let entry = LinuxStartup::entry(id.into(), "x".into(), "x".into(), StartupLocation::UserStartupFolder, true);
            assert!(
                matches!(e.startup.set_state(&entry, StartupState::Disabled), Err(PlatformError::Failed(_))),
                "{id}"
            );
        }
        assert!(e.tools.calls().is_empty());
        assert!(!e.user.exists());
        let entry = LinuxStartup::entry(
            "autostart:x.desktop".into(),
            "x".into(),
            "x".into(),
            StartupLocation::UserStartupFolder,
            true,
        );
        assert!(matches!(
            e.startup.set_state(&entry, StartupState::Delayed(30)),
            Err(PlatformError::Unsupported(_))
        ));
        assert!(
            matches!(e.startup.set_state(&entry, StartupState::Disabled), Err(PlatformError::NotFound(_))),
            "an entry that vanished"
        );
        assert!(!e.user.exists());
    }

    #[test]
    fn oversized_files_are_not_rewritten() {
        let mut e = env();
        w(
            &e.user,
            "big.desktop",
            &format!("[Desktop Entry]\nExec=x\n#{}\n", "a".repeat(MAX_ENTRY_BYTES)),
        );
        let entry = LinuxStartup::entry(
            "autostart:big.desktop".into(),
            "big".into(),
            "x".into(),
            StartupLocation::UserStartupFolder,
            true,
        );
        assert!(matches!(e.startup.set_state(&entry, StartupState::Disabled), Err(PlatformError::Failed(m)) if m.contains("too large")));
    }
}
