//! How Pitwall's server is registered with each OS, as pure functions of a *target* OS (not the host's): names,
//! the per-OS data folder and unit folder, the systemd user unit, and the restart-after-update decision.
//! Adapted from Pitcrew's `service_spec` (docs/suite-lessons.md: pure functions that take a target OS build paths with
//! that OS's rules). Nothing here touches the disk or the process.

use std::time::Duration;

/// The Windows service's name.
pub const WINDOWS_SERVICE_NAME: &str = "Pitwall";
/// The Windows service's display name.
pub const WINDOWS_DISPLAY_NAME: &str = "Pitwall local LLM server";
/// The Windows service's description.
pub const WINDOWS_DESCRIPTION: &str = "Serves local language models to the apps on this PC (127.0.0.1 only).";
/// The service as paddock-service names it, for its OS log: the Application event log under the source
/// [`WINDOWS_SERVICE_NAME`] (paddock-service `os_log`, ADR 0054 §2.3), the one place a LocalSystem service writes to.
pub fn os_log_spec() -> paddock_service::ServiceSpec {
    paddock_service::ServiceSpec::builder(LAUNCHD_LABEL, WINDOWS_SERVICE_NAME, "pitwall-server")
        .build()
        .unwrap_or_else(|e| unreachable!("Pitwall's own spec is valid: {e}"))
}

/// The launchd label of the macOS launch agent (its plist ships in the app bundle; the app registers it).
pub const LAUNCHD_LABEL: &str = "net.decryptology.pitwall.service";
/// The systemd user unit's name on Linux.
pub const SYSTEMD_UNIT: &str = "pitwall.service";
/// Set by the unit, plist and Windows service wrapper: the process is supervised and is restarted when it exits.
pub const SUPERVISED_ENV: &str = "PITWALL_SERVICE";

/// What the Service Control Manager does when the service fails: restart after these delays (the last repeats).
pub const SCM_RESTART_DELAYS: [Duration; 3] = [Duration::from_secs(10), Duration::from_secs(60), Duration::from_secs(300)];
/// When the SCM forgets earlier failures.
pub const SCM_RESET_AFTER: Duration = Duration::from_secs(24 * 3600);

/// The operating system a path or file is meant for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOs {
    /// Windows.
    Windows,
    /// macOS.
    MacOs,
    /// Linux.
    Linux,
}

impl TargetOs {
    /// The OS this binary was built for.
    pub const fn host() -> TargetOs {
        if cfg!(windows) {
            TargetOs::Windows
        } else if cfg!(target_os = "macos") {
            TargetOs::MacOs
        } else {
            TargetOs::Linux
        }
    }

    /// The path separator.
    pub const fn sep(self) -> char {
        match self {
            TargetOs::Windows => '\\',
            _ => '/',
        }
    }

    /// Whether `p` is an absolute path by this OS's rules.
    pub fn is_absolute(self, p: &str) -> bool {
        match self {
            TargetOs::Windows => {
                let b = p.as_bytes();
                (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')) || p.starts_with("\\\\")
            }
            _ => p.starts_with('/'),
        }
    }

    /// Joins path parts with this OS's separator.
    pub fn join(self, base: &str, parts: &[&str]) -> String {
        let sep = self.sep();
        let mut out = base.trim_end_matches(['/', '\\']).to_string();
        for p in parts {
            out.push(sep);
            out.push_str(p);
        }
        out
    }
}

/// The environment variables the path rules read (injected so tests need no real environment).
#[derive(Debug, Clone, Default)]
pub struct PathEnv {
    /// `PITWALL_HOME`.
    pub pitwall_home: Option<String>,
    /// `HOME` (`USERPROFILE` on Windows).
    pub home: Option<String>,
    /// `APPDATA` (Windows).
    pub appdata: Option<String>,
    /// `XDG_DATA_HOME` (Linux).
    pub xdg_data_home: Option<String>,
    /// `XDG_CONFIG_HOME` (Linux).
    pub xdg_config_home: Option<String>,
}

impl PathEnv {
    /// Reads the real environment.
    pub fn from_process() -> PathEnv {
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        PathEnv {
            pitwall_home: get("PITWALL_HOME"),
            home: get("HOME").or_else(|| get("USERPROFILE")),
            appdata: get("APPDATA"),
            xdg_data_home: get("XDG_DATA_HOME"),
            xdg_config_home: get("XDG_CONFIG_HOME"),
        }
    }
}

/// Pitwall's data folder on `os` (the same rules as `paths::Dirs::for_user`, which has the host's).
pub fn data_dir(os: TargetOs, env: &PathEnv) -> Option<String> {
    if let Some(h) = &env.pitwall_home {
        return Some(h.clone());
    }
    match os {
        TargetOs::Windows => env
            .appdata
            .as_deref()
            .map(|a| os.join(a, &["Pitwall"]))
            .or_else(|| env.home.as_deref().map(|h| os.join(h, &["AppData", "Roaming", "Pitwall"]))),
        TargetOs::MacOs => env
            .home
            .as_deref()
            .map(|h| os.join(h, &["Library", "Application Support", "Pitwall"])),
        TargetOs::Linux => env
            .xdg_data_home
            .as_deref()
            .filter(|x| os.is_absolute(x))
            .map(|x| os.join(x, &["pitwall"]))
            .or_else(|| env.home.as_deref().map(|h| os.join(h, &[".local", "share", "pitwall"]))),
    }
}

/// Where the user's systemd units live (`$XDG_CONFIG_HOME/systemd/user` or `~/.config/systemd/user`); Linux only.
pub fn systemd_unit_dir(env: &PathEnv) -> Option<String> {
    let os = TargetOs::Linux;
    env.xdg_config_home
        .as_deref()
        .filter(|x| os.is_absolute(x))
        .map(|x| os.join(x, &["systemd", "user"]))
        .or_else(|| env.home.as_deref().map(|h| os.join(h, &[".config", "systemd", "user"])))
}

/// The unit file's full path.
pub fn systemd_unit_file(env: &PathEnv) -> Option<String> {
    systemd_unit_dir(env).map(|d| TargetOs::Linux.join(&d, &[SYSTEMD_UNIT]))
}

/// Where the service log goes when a manager does not capture it: `<data>/logs/service.log`.
pub fn service_log(os: TargetOs, env: &PathEnv) -> Option<String> {
    data_dir(os, env).map(|d| os.join(&d, &["logs", "service.log"]))
}

/// Whether `exe` is inside an AppImage's temporary mount (it vanishes when the app closes), so the unit must point
/// at a stable copy.
pub fn is_appimage_path(exe: &str, appimage_env: Option<&str>) -> bool {
    appimage_env.is_some_and(|a| !a.is_empty()) || exe.split('/').any(|c| c.starts_with(".mount_"))
}

/// Quotes one word of an `ExecStart=` line: systemd splits on whitespace, honours double quotes and backslash escapes,
/// and expands `%` specifiers and `$` variables, so those are doubled.
pub fn systemd_quote(word: &str) -> String {
    let mut out = String::with_capacity(word.len() + 2);
    out.push('"');
    for c in word.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '%' => out.push_str("%%"),
            '$' => out.push_str("$$"),
            '\n' | '\r' => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The Linux systemd user unit that runs the server in the user's session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemdUnitSpec {
    /// The server program and its arguments.
    pub exec: Vec<String>,
    /// The working directory.
    pub working_directory: String,
}

/// Renders the unit: `Restart=always` brings the server back after a crash and after it exits itself following an
/// update, but not after `systemctl stop`. The server gets the user's login `PATH` itself (`PITWALL_SERVICE=1`).
/// Lingering (running without a log-in) is the user's decision: `loginctl enable-linger`.
pub fn systemd_unit(spec: &SystemdUnitSpec) -> String {
    let exec: Vec<String> = spec.exec.iter().map(|a| systemd_quote(a)).collect();
    format!(
        "[Unit]
Description=Pitwall local LLM server
Documentation=https://dev.decryptology.net/decryp7/pitwall
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={exec}
WorkingDirectory={wd}
Environment={env}=1
Restart=always
RestartSec=10
TimeoutStopSec=60

[Install]
WantedBy=default.target
",
        exec = exec.join(" "),
        wd = systemd_quote(&spec.working_directory),
        env = SUPERVISED_ENV,
    )
}

/// A fingerprint of the server's own executable file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinaryStamp {
    /// Size in bytes.
    pub len: u64,
    /// Modification time, milliseconds since the Unix epoch.
    pub modified_ms: u128,
}

/// What the server is doing, as far as restarting is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    /// Requests, queued jobs or downloads are in flight.
    Busy,
    /// Nothing is in flight.
    Idle,
}

/// Whether the server should exit now so its supervisor starts the new binary: the file changed since start, nothing
/// is in flight, and something supervises the process (otherwise exiting would just stop the server). A file that
/// cannot be read now (being replaced) never triggers a restart.
pub fn should_restart(started: Option<BinaryStamp>, now: Option<BinaryStamp>, activity: Activity, supervised: bool) -> bool {
    supervised && activity == Activity::Idle && matches!((started, now), (Some(a), Some(b)) if a != b)
}

/// Merges the login shell's `PATH` in front of the inherited one, dropping duplicates and empty entries. `sep` is the
/// target OS's list separator.
pub fn merge_path(login: &str, inherited: &str, sep: char) -> String {
    let mut seen: Vec<&str> = Vec::new();
    for part in login.split(sep).chain(inherited.split(sep)) {
        if !part.is_empty() && !seen.contains(&part) {
            seen.push(part);
        }
    }
    seen.join(&sep.to_string())
}

/// A logged-on user's session (Windows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserSession {
    /// The session id.
    pub id: u32,
    /// Connected (active) rather than disconnected.
    pub active: bool,
}

/// The session a logged-on user's server runs in. The physical console is only one of them: a PC used over Remote
/// Desktop has its user in another session while the console shows the sign-in screen. Connected sessions come before
/// disconnected ones (a disconnected Remote Desktop session keeps running its programs), the console before other
/// sessions, then the lowest id, so the choice is stable. Session 0 is the services' own and never qualifies.
pub fn pick_user_session(console: Option<u32>, sessions: &[UserSession]) -> Option<u32> {
    sessions
        .iter()
        .filter(|s| s.id != 0)
        .min_by_key(|s| (!s.active, Some(s.id) != console, s.id))
        .map(|s| s.id)
}

/// Adds `NAME=value` to a Windows environment block (UTF-16 `A=1\0B=2\0\0`) without disturbing the rest.
pub fn env_block_with(block: &[u16], entry: &str) -> Vec<u16> {
    let mut out = Vec::with_capacity(block.len() + entry.len() + 2);
    let mut i = 0;
    while i < block.len() && block[i] != 0 {
        let end = block[i..].iter().position(|&c| c == 0).map_or(block.len(), |p| i + p);
        out.extend_from_slice(&block[i..end]);
        out.push(0);
        i = end + 1;
    }
    out.extend(entry.encode_utf16());
    out.push(0);
    out.push(0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> PathEnv {
        PathEnv {
            home: Some("/home/ana".into()),
            ..PathEnv::default()
        }
    }

    #[test]
    fn data_dirs_follow_the_target_os_not_the_host() {
        let win = PathEnv {
            appdata: Some(r"C:\Users\Ana\AppData\Roaming".into()),
            ..PathEnv::default()
        };
        assert_eq!(data_dir(TargetOs::Windows, &win).unwrap(), r"C:\Users\Ana\AppData\Roaming\Pitwall");
        assert_eq!(
            service_log(TargetOs::Windows, &win).unwrap(),
            r"C:\Users\Ana\AppData\Roaming\Pitwall\logs\service.log"
        );
        let mac = PathEnv {
            home: Some("/Users/ana".into()),
            ..PathEnv::default()
        };
        assert_eq!(
            data_dir(TargetOs::MacOs, &mac).unwrap(),
            "/Users/ana/Library/Application Support/Pitwall"
        );
        assert_eq!(data_dir(TargetOs::Linux, &env()).unwrap(), "/home/ana/.local/share/pitwall");
        let xdg = PathEnv {
            xdg_data_home: Some("/data".into()),
            ..env()
        };
        assert_eq!(data_dir(TargetOs::Linux, &xdg).unwrap(), "/data/pitwall");
        // A relative XDG_DATA_HOME is ignored (the spec says so).
        let rel = PathEnv {
            xdg_data_home: Some("data".into()),
            ..env()
        };
        assert_eq!(data_dir(TargetOs::Linux, &rel).unwrap(), "/home/ana/.local/share/pitwall");
        let over = PathEnv {
            pitwall_home: Some("/tmp/pw".into()),
            ..env()
        };
        assert_eq!(data_dir(TargetOs::Windows, &over).unwrap(), "/tmp/pw");
        assert_eq!(data_dir(TargetOs::Linux, &PathEnv::default()), None);
        let profile = PathEnv {
            home: Some(r"C:\Users\Ana".into()),
            ..PathEnv::default()
        };
        assert_eq!(
            data_dir(TargetOs::Windows, &profile).unwrap(),
            r"C:\Users\Ana\AppData\Roaming\Pitwall"
        );
    }

    #[test]
    fn absolute_paths_use_the_target_os_rules() {
        assert!(TargetOs::Windows.is_absolute(r"C:\x"));
        assert!(TargetOs::Windows.is_absolute("D:/x"));
        assert!(TargetOs::Windows.is_absolute(r"\\server\share"));
        assert!(!TargetOs::Windows.is_absolute("/x"));
        assert!(!TargetOs::Linux.is_absolute(r"C:\x"));
        assert!(TargetOs::Linux.is_absolute("/x"));
        assert!(!TargetOs::MacOs.is_absolute("x/y"));
        assert_eq!(TargetOs::Linux.sep(), '/');
        assert_eq!(TargetOs::Windows.join(r"C:\a\", &["b", "c"]), r"C:\a\b\c");
        assert_eq!(TargetOs::host(), TargetOs::host());
    }

    #[test]
    fn the_unit_goes_to_the_users_systemd_folder() {
        assert_eq!(systemd_unit_file(&env()).unwrap(), "/home/ana/.config/systemd/user/pitwall.service");
        let xdg = PathEnv {
            xdg_config_home: Some("/cfg".into()),
            ..env()
        };
        assert_eq!(systemd_unit_dir(&xdg).unwrap(), "/cfg/systemd/user");
        let rel = PathEnv {
            xdg_config_home: Some("cfg".into()),
            ..env()
        };
        assert_eq!(systemd_unit_dir(&rel).unwrap(), "/home/ana/.config/systemd/user");
        assert_eq!(systemd_unit_dir(&PathEnv::default()), None);
        assert_eq!(systemd_unit_file(&PathEnv::default()), None);
    }

    #[test]
    fn the_systemd_unit_runs_the_server_in_the_users_session() {
        let unit = systemd_unit(&SystemdUnitSpec {
            exec: vec!["/home/ana/.local/share/pitwall/bin/pitwall-server".into()],
            working_directory: "/home/ana/.local/share/pitwall".into(),
        });
        assert!(unit.contains("ExecStart=\"/home/ana/.local/share/pitwall/bin/pitwall-server\"\n"));
        assert!(unit.contains("WantedBy=default.target") && unit.contains("Restart=always"));
        assert!(unit.contains("Environment=PITWALL_SERVICE=1"));
        assert!(unit.contains("WorkingDirectory=\"/home/ana/.local/share/pitwall\""));
        assert_eq!(SYSTEMD_UNIT, "pitwall.service");
    }

    #[test]
    fn systemd_words_are_quoted_so_nothing_is_expanded_or_split() {
        assert_eq!(systemd_quote("/opt/my apps/x"), "\"/opt/my apps/x\"");
        assert_eq!(systemd_quote("50%$HOME"), "\"50%%$$HOME\"");
        assert_eq!(systemd_quote("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(systemd_quote("a\nExecStart=/bin/evil"), "\"a ExecStart=/bin/evil\"");
    }

    #[test]
    fn appimage_paths_are_recognised() {
        assert!(is_appimage_path("/tmp/.mount_PitwalXYZ/usr/bin/pitwall-server", None));
        assert!(is_appimage_path("/usr/bin/x", Some("/home/a/Pitwall.AppImage")));
        assert!(!is_appimage_path("/usr/bin/pitwall-server", Some("")));
    }

    #[test]
    fn a_changed_binary_restarts_only_an_idle_supervised_server() {
        let a = BinaryStamp { len: 10, modified_ms: 1 };
        let b = BinaryStamp { len: 11, modified_ms: 2 };
        assert!(should_restart(Some(a), Some(b), Activity::Idle, true));
        // Busy: wait until idle.
        assert!(!should_restart(Some(a), Some(b), Activity::Busy, true));
        // Nobody would start it again.
        assert!(!should_restart(Some(a), Some(b), Activity::Idle, false));
        // Unchanged, or unreadable while an installer replaces it.
        assert!(!should_restart(Some(a), Some(a), Activity::Idle, true));
        assert!(!should_restart(Some(a), None, Activity::Idle, true));
        assert!(!should_restart(None, Some(b), Activity::Idle, true));
    }

    #[test]
    fn the_login_path_comes_first_without_duplicates() {
        assert_eq!(
            merge_path("/opt/homebrew/bin:/usr/bin::", "/usr/bin:/bin", ':'),
            "/opt/homebrew/bin:/usr/bin:/bin"
        );
        assert_eq!(merge_path("C:\\a;C:\\b", "C:\\b;C:\\c", ';'), "C:\\a;C:\\b;C:\\c");
        assert_eq!(merge_path("", "", ':'), "");
    }

    #[test]
    fn recovery_delays_grow() {
        assert!(SCM_RESTART_DELAYS.windows(2).all(|w| w[0] < w[1]));
        assert!(SCM_RESET_AFTER > SCM_RESTART_DELAYS[2]);
        assert!(!WINDOWS_SERVICE_NAME.contains(' '));
        assert!(!WINDOWS_DESCRIPTION.is_empty() && !WINDOWS_DISPLAY_NAME.is_empty());
        assert_eq!(LAUNCHD_LABEL, "net.decryptology.pitwall.service");
    }

    #[test]
    fn a_remote_desktop_user_is_found_when_the_console_shows_the_sign_in_screen() {
        let s = |id, active| UserSession { id, active };
        assert_eq!(pick_user_session(Some(1), &[s(2, true)]), Some(2));
        assert_eq!(pick_user_session(None, &[s(2, true)]), Some(2));
        assert_eq!(pick_user_session(Some(1), &[]), None);
        assert_eq!(pick_user_session(Some(5), &[s(3, false), s(4, true)]), Some(4));
        assert_eq!(pick_user_session(Some(5), &[s(4, true), s(5, true)]), Some(5));
        assert_eq!(pick_user_session(Some(9), &[s(7, true), s(6, true)]), Some(6));
        assert_eq!(pick_user_session(None, &[s(8, false)]), Some(8));
        assert_eq!(pick_user_session(Some(0), &[s(0, true)]), None);
    }

    #[test]
    fn an_environment_block_gains_one_entry() {
        let block: Vec<u16> = "A=1\0BB=22\0\0".encode_utf16().collect();
        let with = env_block_with(&block, "PITWALL_SERVICE=1");
        let text = String::from_utf16_lossy(&with);
        assert_eq!(text, "A=1\0BB=22\0PITWALL_SERVICE=1\0\0");
        let empty = env_block_with(&[0, 0], "X=1");
        assert_eq!(String::from_utf16_lossy(&empty), "X=1\0\0");
        assert_eq!(String::from_utf16_lossy(&env_block_with(&[], "X=1")), "X=1\0\0");
    }
}
