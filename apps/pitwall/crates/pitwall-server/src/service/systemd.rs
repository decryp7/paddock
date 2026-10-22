//! Linux: the server is the user's systemd **user** service (docs/linux.md), like Pitcrew's: it runs as the user,
//! with no administrator rights. `pitwall-server install` writes `~/.config/systemd/user/pitwall.service`, runs
//! `systemctl --user daemon-reload` and `enable --now`; `uninstall` disables and stops it and removes the file.
//! Every `systemctl` call has a timeout and failures say what to do. Running without a log-in session needs lingering,
//! which is the user's decision (`loginctl enable-linger`); Pitwall documents it and never runs it.
//!
//! Compiled on every Unix so the unit-writing logic is tested on the Mac too; only Linux dispatches to it.

use super::exec::run_with_timeout;
use super::spec::{is_appimage_path, systemd_unit, systemd_unit_dir, PathEnv, SystemdUnitSpec, SYSTEMD_UNIT};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Runs `systemctl --user <args>` and returns its success or the reason it failed.
pub type Systemctl<'a> = &'a dyn Fn(&[&str]) -> Result<(), String>;

/// What a failed `systemctl --user` says, with the usual cause spelled out.
pub fn explain_failure(stderr: &str) -> String {
    let first = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("it gave no reason").trim();
    if first.contains("Failed to connect to bus") || first.contains("No medium found") || first.contains("XDG_RUNTIME_DIR") {
        format!("{first}. The systemd user instance is not reachable: log in on the machine (or ssh with a full session) and try again")
    } else {
        first.to_string()
    }
}

/// The real `systemctl --user`, 30 s at most per call.
pub fn systemctl(args: &[&str]) -> Result<(), String> {
    let mut full = vec!["--user"];
    full.extend_from_slice(args);
    let out = run_with_timeout(OsStr::new("systemctl"), &full, &[], Duration::from_secs(30))
        .map_err(|e| format!("{e} (is systemd installed and is this a systemd user session?)"))?;
    if out.success {
        Ok(())
    } else {
        Err(explain_failure(&out.stderr))
    }
}

/// The unit file's text for `exe` and `data_dir`.
pub fn unit_text(exe: &Path, data_dir: &Path) -> String {
    systemd_unit(&SystemdUnitSpec {
        exec: vec![exe.display().to_string()],
        working_directory: data_dir.display().to_string(),
    })
}

fn io_error(path: &Path, e: std::io::Error) -> String {
    format!("{}: {e}", path.display())
}

/// Copies `exe` to `<data>/bin/pitwall-server` (atomically, mode 0755) and returns the copy: an AppImage's own files
/// disappear when it closes.
pub fn stable_copy(exe: &Path, data_dir: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;
    let bin = data_dir.join("bin");
    std::fs::create_dir_all(&bin).map_err(|e| io_error(&bin, e))?;
    let target = bin.join("pitwall-server");
    let tmp = bin.join("pitwall-server.new");
    std::fs::copy(exe, &tmp).map_err(|e| io_error(&tmp, e))?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).map_err(|e| io_error(&tmp, e))?;
    std::fs::rename(&tmp, &target).map_err(|e| io_error(&target, e))?;
    Ok(target)
}

/// Writes the unit into `unit_dir` and enables it with `ctl`. Returns the unit file's path.
pub fn install_into(unit_dir: &Path, exe: &Path, data_dir: &Path, ctl: Systemctl<'_>) -> Result<PathBuf, String> {
    std::fs::create_dir_all(unit_dir).map_err(|e| io_error(unit_dir, e))?;
    std::fs::create_dir_all(data_dir).map_err(|e| io_error(data_dir, e))?;
    let file = unit_dir.join(SYSTEMD_UNIT);
    let tmp = unit_dir.join(format!("{SYSTEMD_UNIT}.new"));
    std::fs::write(&tmp, unit_text(exe, data_dir)).map_err(|e| io_error(&tmp, e))?;
    std::fs::rename(&tmp, &file).map_err(|e| io_error(&file, e))?;
    ctl(&["daemon-reload"])?;
    ctl(&["enable", "--now", SYSTEMD_UNIT])?;
    Ok(file)
}

/// Disables and stops the unit, removes the file. Removing a service that is not installed succeeds.
pub fn uninstall_from(unit_dir: &Path, ctl: Systemctl<'_>) -> Result<(), String> {
    let file = unit_dir.join(SYSTEMD_UNIT);
    if !file.exists() {
        return Ok(());
    }
    // A unit that is already stopped or disabled is fine.
    let _ = ctl(&["disable", "--now", SYSTEMD_UNIT]);
    std::fs::remove_file(&file).map_err(|e| io_error(&file, e))?;
    ctl(&["daemon-reload"])
}

fn dirs() -> Result<(PathBuf, PathBuf), String> {
    let env = PathEnv::from_process();
    let unit_dir = systemd_unit_dir(&env).ok_or("HOME is not set, so the systemd user folder cannot be found")?;
    let data_dir = super::spec::data_dir(super::spec::TargetOs::Linux, &env).ok_or("HOME is not set, so the data folder cannot be found")?;
    Ok((PathBuf::from(unit_dir), PathBuf::from(data_dir)))
}

/// `pitwall-server install`.
pub fn install() -> Result<String, String> {
    let (unit_dir, data_dir) = dirs()?;
    let mut exe = std::env::current_exe().map_err(|e| format!("the program's own path cannot be found: {e}"))?;
    if is_appimage_path(&exe.display().to_string(), std::env::var("APPIMAGE").ok().as_deref()) {
        exe = stable_copy(&exe, &data_dir)?;
    }
    let file = install_into(&unit_dir, &exe, &data_dir, &systemctl)?;
    Ok(format!("Pitwall's service is installed ({}) and started", file.display()))
}

/// `pitwall-server uninstall`.
pub fn uninstall() -> Result<String, String> {
    let (unit_dir, _) = dirs()?;
    uninstall_from(&unit_dir, &systemctl)?;
    Ok("Pitwall's service is removed".into())
}

/// `pitwall-server print-unit`: the unit that `install` would write for this program.
pub fn print_unit() -> Result<String, String> {
    let (_, data_dir) = dirs()?;
    let exe = std::env::current_exe().map_err(|e| format!("the program's own path cannot be found: {e}"))?;
    Ok(unit_text(&exe, &data_dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn install_writes_the_unit_then_reloads_and_enables_it() {
        let d = tempfile::tempdir().unwrap();
        let calls = RefCell::new(Vec::<String>::new());
        let ctl = |a: &[&str]| -> Result<(), String> {
            calls.borrow_mut().push(a.join(" "));
            Ok(())
        };
        let exe = Path::new("/usr/bin/pitwall-server");
        let file = install_into(&d.path().join("systemd/user"), exe, &d.path().join("data"), &ctl).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("ExecStart=\"/usr/bin/pitwall-server\"") && text.contains("Restart=always"));
        assert_eq!(*calls.borrow(), vec!["daemon-reload", "enable --now pitwall.service"]);
        assert!(!d.path().join("systemd/user/pitwall.service.new").exists());
        // Reinstalling replaces the file.
        install_into(&d.path().join("systemd/user"), Path::new("/opt/p/pitwall-server"), &d.path().join("data"), &ctl).unwrap();
        assert!(std::fs::read_to_string(&file).unwrap().contains("/opt/p/pitwall-server"));
    }

    #[test]
    fn install_reports_what_systemctl_said() {
        let d = tempfile::tempdir().unwrap();
        let ctl = |a: &[&str]| -> Result<(), String> {
            if a[0] == "enable" {
                Err("boom".into())
            } else {
                Ok(())
            }
        };
        let e = install_into(d.path(), Path::new("/x/pitwall-server"), d.path(), &ctl).unwrap_err();
        assert_eq!(e, "boom");
    }

    #[test]
    fn uninstall_removes_the_unit_and_tolerates_a_missing_one_or_a_stopped_unit() {
        let d = tempfile::tempdir().unwrap();
        let calls = RefCell::new(Vec::<String>::new());
        let ctl = |a: &[&str]| -> Result<(), String> {
            calls.borrow_mut().push(a.join(" "));
            if a[0] == "disable" {
                Err("not loaded".into())
            } else {
                Ok(())
            }
        };
        assert!(uninstall_from(d.path(), &ctl).is_ok(), "nothing installed");
        assert!(calls.borrow().is_empty());
        std::fs::write(d.path().join(SYSTEMD_UNIT), "x").unwrap();
        uninstall_from(d.path(), &ctl).unwrap();
        assert!(!d.path().join(SYSTEMD_UNIT).exists());
        assert_eq!(*calls.borrow(), vec!["disable --now pitwall.service", "daemon-reload"]);
    }

    #[test]
    fn an_appimage_gets_a_stable_copy_that_is_executable() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        std::fs::write(&src, b"binary").unwrap();
        let copy = stable_copy(&src, &d.path().join("data")).unwrap();
        assert_eq!(copy, d.path().join("data/bin/pitwall-server"));
        assert_eq!(std::fs::read(&copy).unwrap(), b"binary");
        assert_eq!(std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777, 0o755);
        assert!(stable_copy(&d.path().join("missing"), &d.path().join("data")).is_err());
    }

    #[test]
    fn failures_name_the_usual_cause() {
        assert!(explain_failure("Failed to connect to bus: No medium found").contains("log in on the machine"));
        assert_eq!(explain_failure("\n  Unit x not found. \n"), "Unit x not found.");
        assert_eq!(explain_failure(""), "it gave no reason");
    }
}
