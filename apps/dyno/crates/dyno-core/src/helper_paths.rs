//! Where the privileged helper lives on each system (ADR 0016), as pure functions of the host and the folders the OS
//! names. Shared by the helper (which serves there) and the agent (which connects there).

use crate::helper::HelperHost;
use std::path::{Path, PathBuf};

/// The launchd label and the file stem of the macOS pieces.
pub const MAC_LABEL: &str = "net.decryptology.dyno.helper";
/// The systemd unit.
pub const LINUX_UNIT: &str = "dyno-helper.service";
/// The Windows service name.
pub const WINDOWS_SERVICE: &str = "DynoHelper";
/// The Windows pipe the daemon listens on (named after the launchd label, ADR 0027 §1).
pub const WINDOWS_PIPE: &str = r"\\.\pipe\net.decryptology.dyno.helper";

/// The folders Windows names (read from the environment at run time; tests pass their own).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsRoots {
    /// `%ProgramFiles%`.
    pub program_files: PathBuf,
    /// `%ProgramData%`.
    pub program_data: PathBuf,
    /// `%SystemRoot%` (where `sc.exe` and `icacls.exe` live, in `System32`).
    pub system_root: PathBuf,
}

/// `base` and `tail` joined with a backslash, whatever system this runs on (the paths of another system are built by
/// pure functions and checked in tests anywhere).
pub fn join_windows(base: &Path, tail: &str) -> PathBuf {
    PathBuf::from(format!("{}\\{tail}", base.to_string_lossy().trim_end_matches(['\\', '/'])))
}

/// `base` and `tail` joined with a slash, whatever system this runs on (`Path::join` would use a backslash on Windows).
pub fn join_unix(base: &Path, tail: &str) -> PathBuf {
    PathBuf::from(format!("{}/{tail}", base.to_string_lossy().trim_end_matches('/')))
}

/// Whether `path` is absolute by the rules of `host` (a drive letter or UNC share on Windows, a slash elsewhere).
pub fn is_absolute_on(host: HelperHost, path: &Path) -> bool {
    let s = path.to_string_lossy();
    match host {
        HelperHost::Windows => {
            let b = s.as_bytes();
            (b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')) || s.starts_with(r"\\")
        }
        _ => s.starts_with('/'),
    }
}

/// Every place the helper touches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperPaths {
    /// The system the paths are for.
    pub host: HelperHost,
    /// The installed program (the daemon runs from here, never from the app's folder).
    pub binary: PathBuf,
    /// The helper's data folder (root-owned): configuration, audit log.
    pub data_dir: PathBuf,
    /// `config.json`.
    pub config: PathBuf,
    /// `audit.jsonl`.
    pub audit: PathBuf,
    /// The socket path or pipe name clients connect to.
    pub endpoint: String,
    /// The folder the endpoint's socket lives in (Unix), created by the service manager or the installer.
    pub endpoint_dir: Option<PathBuf>,
    /// The file that registers the daemon with the service manager (plist or unit); none on Windows (a service).
    pub registration: Option<PathBuf>,
}

impl HelperPaths {
    /// The paths for `host`. `windows` is read only for Windows.
    pub fn for_host(host: HelperHost, windows: &WindowsRoots) -> HelperPaths {
        let in_dir = |dir: PathBuf| {
            if host == HelperHost::Windows {
                (join_windows(&dir, "config.json"), join_windows(&dir, "audit.jsonl"))
            } else {
                (join_unix(&dir, "config.json"), join_unix(&dir, "audit.jsonl"))
            }
        };
        match host {
            HelperHost::Macos => {
                let data_dir = PathBuf::from("/Library/Application Support/Dyno/Helper");
                let (config, audit) = in_dir(data_dir.clone());
                HelperPaths {
                    host,
                    binary: join_unix(Path::new("/Library/PrivilegedHelperTools"), MAC_LABEL),
                    data_dir,
                    config,
                    audit,
                    endpoint: "/var/run/dyno-helper/helper.sock".into(),
                    endpoint_dir: Some(PathBuf::from("/var/run/dyno-helper")),
                    registration: Some(PathBuf::from(format!("/Library/LaunchDaemons/{MAC_LABEL}.plist"))),
                }
            }
            HelperHost::Linux => {
                let data_dir = PathBuf::from("/var/lib/dyno-helper");
                let (config, audit) = in_dir(data_dir.clone());
                HelperPaths {
                    host,
                    binary: PathBuf::from("/usr/bin/dyno-helper"),
                    data_dir,
                    config,
                    audit,
                    endpoint: "/run/dyno-helper/helper.sock".into(),
                    endpoint_dir: Some(PathBuf::from("/run/dyno-helper")),
                    registration: Some(PathBuf::from(format!("/usr/lib/systemd/system/{LINUX_UNIT}"))),
                }
            }
            HelperHost::Windows => {
                let data_dir = join_windows(&join_windows(&windows.program_data, "Dyno"), "Helper");
                let (config, audit) = in_dir(data_dir.clone());
                HelperPaths {
                    host,
                    binary: join_windows(&join_windows(&windows.program_files, "Dyno Helper"), "dyno-helper.exe"),
                    data_dir,
                    config,
                    audit,
                    endpoint: WINDOWS_PIPE.into(),
                    endpoint_dir: None,
                    registration: None,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> WindowsRoots {
        WindowsRoots {
            program_files: PathBuf::from(r"C:\Program Files"),
            program_data: PathBuf::from(r"C:\ProgramData"),
            system_root: PathBuf::from(r"C:\Windows"),
        }
    }

    #[test]
    fn each_system_has_its_own_system_owned_places() {
        let mac = HelperPaths::for_host(HelperHost::Macos, &roots());
        assert_eq!(
            mac.binary,
            PathBuf::from("/Library/PrivilegedHelperTools/net.decryptology.dyno.helper")
        );
        assert_eq!(
            mac.registration,
            Some(PathBuf::from("/Library/LaunchDaemons/net.decryptology.dyno.helper.plist"))
        );
        assert_eq!(mac.endpoint, "/var/run/dyno-helper/helper.sock");
        assert_eq!(mac.endpoint_dir, Some(PathBuf::from("/var/run/dyno-helper")));
        let lin = HelperPaths::for_host(HelperHost::Linux, &roots());
        assert_eq!(lin.binary, PathBuf::from("/usr/bin/dyno-helper"));
        assert_eq!(lin.endpoint_dir, Some(PathBuf::from("/run/dyno-helper")));
        assert!(lin.registration.unwrap().ends_with("dyno-helper.service"));
        let win = HelperPaths::for_host(HelperHost::Windows, &roots());
        assert_eq!(win.binary, PathBuf::from(r"C:\Program Files\Dyno Helper\dyno-helper.exe"));
        assert_eq!(win.config, PathBuf::from(r"C:\ProgramData\Dyno\Helper\config.json"));
        assert_eq!(win.endpoint, r"\\.\pipe\net.decryptology.dyno.helper");
        assert!(win.registration.is_none());
        assert_eq!(win.audit, PathBuf::from(r"C:\ProgramData\Dyno\Helper\audit.jsonl"));
    }

    #[test]
    fn absolute_means_what_the_target_system_says() {
        assert!(is_absolute_on(
            HelperHost::Windows,
            Path::new(r"C:\Program Files\Dyno\dyno-agent.exe")
        ));
        assert!(is_absolute_on(HelperHost::Windows, Path::new(r"\\server\share\x")));
        assert!(!is_absolute_on(HelperHost::Windows, Path::new("/usr/bin/x")));
        assert!(!is_absolute_on(HelperHost::Windows, Path::new(r"dyno-agent.exe")));
        assert!(is_absolute_on(HelperHost::Macos, Path::new("/Applications/Dyno.app")));
        assert!(!is_absolute_on(HelperHost::Linux, Path::new("dyno-agent")));
        assert_eq!(
            join_windows(Path::new(r"C:\Windows\"), "System32"),
            PathBuf::from(r"C:\Windows\System32")
        );
        assert_eq!(join_unix(Path::new("/var/lib/"), "token"), PathBuf::from("/var/lib/token"));
        // The paths of the Unix systems use slashes only, even when computed on Windows.
        for host in [HelperHost::Macos, HelperHost::Linux] {
            let p = HelperPaths::for_host(host, &roots());
            for path in [&p.binary, &p.data_dir, &p.config, &p.audit] {
                assert!(!path.to_string_lossy().contains('\\'), "{path:?}");
            }
        }
    }
}
