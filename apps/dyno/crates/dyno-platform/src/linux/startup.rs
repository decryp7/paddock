//! What starts at login on Linux: XDG autostart `.desktop` files (the user's own and the system's) and enabled
//! systemd **user** services. Read-only: nothing here writes a file or runs `systemctl enable`.

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
/// How long `systemctl` may take.
const SYSTEMCTL_TIMEOUT: Duration = Duration::from_secs(10);

const USER_PREFIX: &str = "autostart:";
const SYSTEM_PREFIX: &str = "autostart-system:";
const UNIT_PREFIX: &str = "systemd-user:";

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
    #[allow(
        clippy::disallowed_methods,
        reason = "KNOWN DEFECT, not a justification: a shared, predictable folder under the OS temp dir that another local user can create first (Dyno security fix, tracked in docs/gates.toml)"
    )]
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
    fn only_plain_file_and_unit_names_are_listed() {
        assert!(plain_desktop_name("a.desktop").is_some());
        for bad in [".desktop", ".hidden.desktop", "a/b.desktop", "a\\b.desktop", "a.txt", "a\0.desktop"] {
            assert!(plain_desktop_name(bad).is_none(), "{bad}");
        }
        assert!(plain_desktop_name(&format!("{}.desktop", "a".repeat(260))).is_none());
        assert!(plain_unit_name("syncthing.service") && plain_unit_name("app@1.service"));
        for bad in ["-x.service", ".x.service", "x.timer", "a b.service", "a/b.service"] {
            assert!(!plain_unit_name(bad), "{bad}");
        }
        assert!(!plain_unit_name(&format!("{}.service", "a".repeat(260))));
    }
}
