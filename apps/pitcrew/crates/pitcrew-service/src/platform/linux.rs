//! Linux: the service is the user's systemd **user** service (ADR 0003, ADR 0009): it runs as the user, in their
//! session manager, with no administrator rights, like the macOS launch agent.
//!
//! `pitcrew-service install` writes `~/.config/systemd/user/pitcrew.service` (`$XDG_CONFIG_HOME`), runs
//! `systemctl --user daemon-reload` and `systemctl --user enable --now pitcrew.service`; `uninstall` disables and
//! stops it, removes the file and reloads. Every `systemctl` call has a timeout and failures say what to do.
//! Running without a log-in session needs lingering, which is the user's decision (`loginctl enable-linger`,
//! docs/linux.md); Pitcrew never runs it.
//!
//! When the program is an AppImage (its files live in a temporary mount that disappears), the service binary is first
//! copied to `<data folder>/bin/pitcrew-service` and the unit points there.

use crate::error::Error;
use crate::process::{quiet_command, run_with_timeout};
use pitcrew_core::service_spec::{systemd_unit, SystemdUnitSpec, SYSTEMD_UNIT};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The systemd user manager runs the service with `run`; there is no separate service entry point.
pub fn dispatch() -> Result<(), Error> {
    Err(Error::Unsupported("The `service` command (Linux uses `run` under systemd)"))
}

/// Where the user's units live: `$XDG_CONFIG_HOME/systemd/user` or `~/.config/systemd/user`.
pub fn unit_dir(env: &dyn Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    env("XDG_CONFIG_HOME")
        .filter(|p| p.is_absolute())
        .or_else(|| env("HOME").map(|h| h.join(".config")))
        .map(|c| c.join("systemd").join("user"))
}

/// Whether `exe` is inside an AppImage's temporary mount (it vanishes when the app closes).
pub fn is_appimage_path(exe: &Path, appimage_env: Option<&str>) -> bool {
    appimage_env.is_some_and(|a| !a.is_empty()) || exe.components().any(|c| c.as_os_str().to_string_lossy().starts_with(".mount_"))
}

/// The unit file's text for `exe` and `data_dir`.
pub fn unit_text(exe: &Path, data_dir: &Path) -> String {
    systemd_unit(&SystemdUnitSpec {
        exec: vec![exe.display().to_string(), "run".into()],
        working_directory: data_dir.display().to_string(),
    })
}

fn env(k: &str) -> Option<PathBuf> {
    std::env::var_os(k).map(PathBuf::from)
}

fn io_error(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.display().to_string(),
        source,
    }
}

fn systemctl(args: &[&str]) -> Result<crate::process::ToolOutput, Error> {
    let mut cmd = quiet_command(Path::new("systemctl"));
    cmd.arg("--user").args(args);
    run_with_timeout(cmd, Duration::from_secs(30)).map_err(|e| match e {
        Error::Os { reason, .. } => Error::Os {
            what: "Running systemctl",
            reason: format!("{reason} (is systemd installed and is this a systemd user session?)"),
        },
        other => other,
    })
}

/// What a failed `systemctl --user` says, with the usual cause spelled out.
pub fn explain_failure(stderr: &str) -> String {
    let first = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("it gave no reason").trim();
    if first.contains("Failed to connect to bus") || first.contains("No medium found") || first.contains("XDG_RUNTIME_DIR") {
        format!("{first}. The systemd user instance is not reachable: log in on the machine (or ssh with a full session) and try again")
    } else {
        first.to_string()
    }
}

fn run_checked(what: &'static str, args: &[&str]) -> Result<(), Error> {
    let out = systemctl(args)?;
    if out.success {
        Ok(())
    } else {
        Err(Error::Os {
            what,
            reason: explain_failure(&out.stderr),
        })
    }
}

/// Copies `exe` to `<data>/bin/pitcrew-service` (atomically, mode 0755) and returns the copy.
fn stable_copy(exe: &Path, data_dir: &Path) -> Result<PathBuf, Error> {
    use std::os::unix::fs::PermissionsExt;
    let bin = data_dir.join("bin");
    std::fs::create_dir_all(&bin).map_err(|e| io_error(&bin, e))?;
    let target = bin.join("pitcrew-service");
    let tmp = bin.join("pitcrew-service.new");
    std::fs::copy(exe, &tmp).map_err(|e| io_error(&tmp, e))?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).map_err(|e| io_error(&tmp, e))?;
    std::fs::rename(&tmp, &target).map_err(|e| io_error(&target, e))?;
    Ok(target)
}

/// Registers and starts the user service.
pub fn install() -> Result<(), Error> {
    let dir = unit_dir(&env).ok_or(Error::Os {
        what: "Finding the systemd user folder",
        reason: "HOME is not set".into(),
    })?;
    let data_dir = crate::paths::data_dir().ok_or(Error::Os {
        what: "Finding the data folder",
        reason: "HOME is not set".into(),
    })?;
    let mut exe = std::env::current_exe().map_err(|source| Error::Io {
        path: "the service executable".into(),
        source,
    })?;
    if is_appimage_path(&exe, std::env::var("APPIMAGE").ok().as_deref()) {
        exe = stable_copy(&exe, &data_dir)?;
    }
    std::fs::create_dir_all(&data_dir).map_err(|e| io_error(&data_dir, e))?;
    std::fs::create_dir_all(&dir).map_err(|e| io_error(&dir, e))?;
    crate::store::write_atomic(&dir.join(SYSTEMD_UNIT), &unit_text(&exe, &data_dir))?;
    run_checked("Reloading systemd", &["daemon-reload"])?;
    run_checked("Enabling the Pitcrew service", &["enable", "--now", SYSTEMD_UNIT])
}

/// Stops and removes the user service. Removing one that is not installed succeeds.
pub fn uninstall() -> Result<(), Error> {
    let dir = unit_dir(&env).ok_or(Error::Os {
        what: "Finding the systemd user folder",
        reason: "HOME is not set".into(),
    })?;
    // Not installed or not loaded is fine: the point is that it is gone afterwards.
    let _ = systemctl(&["disable", "--now", SYSTEMD_UNIT]);
    match std::fs::remove_file(dir.join(SYSTEMD_UNIT)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_error(&dir.join(SYSTEMD_UNIT), e)),
    }
    run_checked("Reloading systemd", &["daemon-reload"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_live_in_the_users_config_folder() {
        let e = |k: &str| match k {
            "HOME" => Some(PathBuf::from("/home/ci")),
            "XDG_CONFIG_HOME" => Some(PathBuf::from("/cfg")),
            _ => None,
        };
        assert_eq!(unit_dir(&e), Some(PathBuf::from("/cfg/systemd/user")));
        let only_home = |k: &str| (k == "HOME").then(|| PathBuf::from("/home/ci"));
        assert_eq!(unit_dir(&only_home), Some(PathBuf::from("/home/ci/.config/systemd/user")));
        let relative = |k: &str| match k {
            "HOME" => Some(PathBuf::from("/home/ci")),
            _ => Some(PathBuf::from("rel")),
        };
        assert_eq!(unit_dir(&relative), Some(PathBuf::from("/home/ci/.config/systemd/user")));
        assert_eq!(unit_dir(&|_| None), None);
    }

    #[test]
    fn an_appimage_is_copied_somewhere_stable() {
        assert!(is_appimage_path(Path::new("/tmp/.mount_PitcrewAbC/usr/bin/pitcrew-service"), None));
        assert!(is_appimage_path(
            Path::new("/usr/bin/pitcrew-service"),
            Some("/home/u/Pitcrew.AppImage")
        ));
        assert!(!is_appimage_path(Path::new("/usr/bin/pitcrew-service"), None));
        assert!(!is_appimage_path(Path::new("/usr/bin/pitcrew-service"), Some("")));
    }

    #[test]
    fn the_unit_runs_this_executable() {
        let t = unit_text(Path::new("/usr/bin/pitcrew-service"), Path::new("/home/ci/.local/share/Pitcrew"));
        assert!(t.contains("ExecStart=\"/usr/bin/pitcrew-service\" \"run\""));
    }

    #[test]
    fn failures_say_what_to_do() {
        let bus = explain_failure("\nFailed to connect to bus: No medium found\n");
        assert!(bus.contains("systemd user instance is not reachable"), "{bus}");
        assert_eq!(
            explain_failure("Unit pitcrew.service not found.\nmore"),
            "Unit pitcrew.service not found."
        );
        assert_eq!(explain_failure(""), "it gave no reason");
    }

    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "a test builds its scratch path from the OS temp folder and removes it"
    )]
    fn a_copied_service_is_executable_and_replaces_the_old_one() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("pitcrew-it-linux-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let src = d.join("src");
        std::fs::write(&src, "one").unwrap();
        let copy = stable_copy(&src, &d.join("data")).unwrap();
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), "one");
        assert_eq!(std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777, 0o755);
        std::fs::write(&src, "two").unwrap();
        stable_copy(&src, &d.join("data")).unwrap();
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), "two");
        assert!(stable_copy(&d.join("missing"), &d.join("data")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
