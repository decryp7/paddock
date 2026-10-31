//! Where each OS keeps a registration, as pure functions of a target OS and an injected environment
//! ([`paddock_env::Vars`]), so every layout is tested on every OS without touching the real one.

use crate::error::ServiceError;
use crate::spec::{Kind, ServiceSpec};
use paddock_env::Vars;
use std::path::{Path, PathBuf};

/// The per-user `Run` key (Windows).
pub const WINDOWS_RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
/// Where Task Manager > Startup (and Settings > Apps > Startup) records that the user switched a `Run` entry off.
pub const WINDOWS_STARTUP_APPROVED_KEY: &str =
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
/// `launchctl`.
pub const LAUNCHCTL: &str = "/bin/launchctl";
/// `systemctl` (found on `PATH`: it lives in `/usr/bin` or `/bin`).
pub const SYSTEMCTL: &str = "systemctl";
/// `reg.exe`.
pub const REG: &str = "reg";
/// Windows' system folder when the OS cannot be asked (or says something that is not a full path).
pub const SYSTEM_DIR_FALLBACK: &str = r"C:\Windows\System32";

/// `sc.exe` by full path in `system_dir`, Windows' system folder (`None`, or a value that is not a full path such as
/// `C:\Windows\System32` or `\\?\C:\Windows\System32`, gives [`SYSTEM_DIR_FALLBACK`]). The pure part of
/// [`default_sc_exe`], so it is tested on every OS with an injected folder.
pub fn sc_exe_in(system_dir: Option<&str>) -> String {
    let dir = system_dir
        .map(|v| v.trim_end_matches(['\\', '/']))
        .filter(|v| {
            let b = v.as_bytes();
            (b.len() > 2
                && b[0].is_ascii_alphabetic()
                && b[1] == b':'
                && matches!(b[2], b'\\' | b'/'))
                || v.starts_with(r"\\")
        })
        .unwrap_or(SYSTEM_DIR_FALLBACK);
    format!(r"{dir}\sc.exe")
}

/// `sc.exe` by full path in the folder the OS names (`GetSystemDirectoryW`), never from an environment variable: a bare
/// `sc` is looked up on `PATH` (and `sc` is a PowerShell alias), and `%SystemRoot%` is the user's to change while this
/// runs elevated. Off Windows the fallback folder (the commands only exist there).
pub fn default_sc_exe() -> String {
    sc_exe_in(system_directory().as_deref())
}

#[cfg(windows)]
fn system_directory() -> Option<String> {
    use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
    let mut buf = [0u16; 260];
    // SAFETY: the buffer is valid for its length for the call.
    let n = unsafe { GetSystemDirectoryW(Some(&mut buf)) } as usize;
    (n > 0 && n <= buf.len())
        .then(|| String::from_utf16(&buf[..n]).ok())
        .flatten()
}

#[cfg(not(windows))]
fn system_directory() -> Option<String> {
    None
}

fn home(vars: &dyn Vars) -> Result<PathBuf, ServiceError> {
    vars.get("HOME")
        .map(PathBuf::from)
        // `has_root`, not `is_absolute`: the launchd and systemd paths are computed (and tested) on every OS, and on Windows
        // `/Users/ana` has a root but no drive.
        .filter(|p| p.has_root())
        .ok_or(ServiceError::NoHome)
}

/// The plist file of a launch agent (`~/Library/LaunchAgents/<label>.plist`) or daemon
/// (`/Library/LaunchDaemons/<label>.plist`).
pub fn launchd_plist_path(spec: &ServiceSpec, vars: &dyn Vars) -> Result<PathBuf, ServiceError> {
    let file = format!("{}.plist", spec.label());
    Ok(match spec.kind() {
        Kind::UserAgent => home(vars)?.join("Library").join("LaunchAgents").join(file),
        Kind::SystemService => Path::new("/Library/LaunchDaemons").join(file),
    })
}

/// The launchd domain: `gui/<uid>` for an agent, `system` for a daemon.
pub fn launchd_domain(kind: Kind, uid: u32) -> String {
    match kind {
        Kind::UserAgent => format!("gui/{uid}"),
        Kind::SystemService => "system".to_string(),
    }
}

/// The launchd service target `<domain>/<label>`.
pub fn launchd_target(spec: &ServiceSpec, uid: u32) -> String {
    format!("{}/{}", launchd_domain(spec.kind(), uid), spec.label())
}

/// The folder of systemd units: `$XDG_CONFIG_HOME/systemd/user` (when absolute) or `~/.config/systemd/user` for a user
/// unit, `/etc/systemd/system` for a system unit.
pub fn systemd_unit_dir(kind: Kind, vars: &dyn Vars) -> Result<PathBuf, ServiceError> {
    match kind {
        Kind::SystemService => Ok(PathBuf::from("/etc/systemd/system")),
        Kind::UserAgent => {
            let config = vars
                .get("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|p| p.has_root())
                .map_or_else(|| home(vars).map(|h| h.join(".config")), Ok)?;
            Ok(config.join("systemd").join("user"))
        }
    }
}

/// The unit file.
pub fn systemd_unit_path(spec: &ServiceSpec, vars: &dyn Vars) -> Result<PathBuf, ServiceError> {
    Ok(systemd_unit_dir(spec.kind(), vars)?.join(spec.unit_file_name()))
}

/// The leading `systemctl` arguments for the kind (`--user` for a user unit).
pub fn systemctl_scope(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::UserAgent => &["--user"],
        Kind::SystemService => &[],
    }
}

/// Whether `exe` is inside an AppImage's temporary mount (it vanishes when the app closes), so a unit must point at a
/// stable copy ([`crate::system::stable_copy`]).
pub fn is_appimage_path(exe: &Path, appimage_env: Option<&str>) -> bool {
    appimage_env.is_some_and(|a| !a.is_empty())
        || exe
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with(".mount_"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_env::MapVars;

    fn spec(kind: Kind) -> ServiceSpec {
        ServiceSpec::builder(
            "net.decryptology.pitwall.service",
            "Pitwall",
            "pitwall-server",
        )
        .kind(kind)
        .build()
        .unwrap()
    }

    #[test]
    fn launchd_files_and_domains() {
        let vars = MapVars::new().with("HOME", "/Users/ana");
        assert_eq!(
            launchd_plist_path(&spec(Kind::UserAgent), &vars).unwrap(),
            PathBuf::from("/Users/ana/Library/LaunchAgents/net.decryptology.pitwall.service.plist")
        );
        assert_eq!(
            launchd_plist_path(&spec(Kind::SystemService), &MapVars::new()).unwrap(),
            PathBuf::from("/Library/LaunchDaemons/net.decryptology.pitwall.service.plist")
        );
        assert!(matches!(
            launchd_plist_path(&spec(Kind::UserAgent), &MapVars::new()),
            Err(ServiceError::NoHome)
        ));
        assert!(matches!(
            launchd_plist_path(&spec(Kind::UserAgent), &MapVars::new().with("HOME", "rel")),
            Err(ServiceError::NoHome)
        ));
        assert_eq!(
            launchd_target(&spec(Kind::UserAgent), 501),
            "gui/501/net.decryptology.pitwall.service"
        );
        assert_eq!(
            launchd_target(&spec(Kind::SystemService), 0),
            "system/net.decryptology.pitwall.service"
        );
    }

    #[test]
    fn systemd_units_go_to_the_users_or_the_systems_folder() {
        let home = MapVars::new().with("HOME", "/home/ana");
        assert_eq!(
            systemd_unit_path(&spec(Kind::UserAgent), &home).unwrap(),
            PathBuf::from("/home/ana/.config/systemd/user/pitwall.service")
        );
        let xdg = MapVars::new()
            .with("HOME", "/home/ana")
            .with("XDG_CONFIG_HOME", "/cfg");
        assert_eq!(
            systemd_unit_dir(Kind::UserAgent, &xdg).unwrap(),
            PathBuf::from("/cfg/systemd/user")
        );
        let rel = MapVars::new()
            .with("HOME", "/home/ana")
            .with("XDG_CONFIG_HOME", "cfg");
        assert_eq!(
            systemd_unit_dir(Kind::UserAgent, &rel).unwrap(),
            PathBuf::from("/home/ana/.config/systemd/user")
        );
        assert!(systemd_unit_dir(Kind::UserAgent, &MapVars::new()).is_err());
        assert_eq!(
            systemd_unit_path(&spec(Kind::SystemService), &MapVars::new()).unwrap(),
            PathBuf::from("/etc/systemd/system/pitwall.service")
        );
        assert_eq!(systemctl_scope(Kind::UserAgent), ["--user"]);
        assert!(systemctl_scope(Kind::SystemService).is_empty());
    }

    #[test]
    fn sc_is_run_by_full_path_from_the_system_directory_with_a_fallback() {
        let want = r"C:\Windows\System32\sc.exe";
        assert_eq!(sc_exe_in(None), want);
        assert_eq!(
            sc_exe_in(Some(r"D:\WINNT\system32")),
            r"D:\WINNT\system32\sc.exe"
        );
        assert_eq!(
            sc_exe_in(Some(r"E:\Windows\System32\")),
            r"E:\Windows\System32\sc.exe"
        );
        assert_eq!(
            sc_exe_in(Some(r"\\?\C:\Windows\System32")),
            r"\\?\C:\Windows\System32\sc.exe"
        );
        // Not a full path (a planted relative folder, empty, a bare drive): never used.
        for bad in [
            "", "Windows", r".\evil", "C:", r"C:evil", "/usr", r"ab\x", r"1:\x",
        ] {
            assert_eq!(sc_exe_in(Some(bad)), want, "{bad:?}");
        }
        assert!(!sc_exe_in(None).eq_ignore_ascii_case("sc"));
        // The real lookup answers a full path ending in sc.exe on any OS.
        let real = default_sc_exe();
        assert!(
            real.to_ascii_lowercase().ends_with(r"\sc.exe") && real.contains('\\'),
            "{real}"
        );
    }

    #[test]
    fn appimage_paths_are_recognised() {
        assert!(is_appimage_path(
            Path::new("/tmp/.mount_PitwalXYZ/usr/bin/pitwall-server"),
            None
        ));
        assert!(is_appimage_path(
            Path::new("/usr/bin/x"),
            Some("/home/a/Pitwall.AppImage")
        ));
        assert!(!is_appimage_path(
            Path::new("/usr/bin/pitwall-server"),
            Some("")
        ));
        assert!(!is_appimage_path(
            Path::new("/usr/bin/pitwall-server"),
            None
        ));
    }
}
