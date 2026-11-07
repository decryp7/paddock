//! Running Pitwall's server as an OS service (docs/suite-lessons.md, adapted from Pitcrew): the verbs the installers and
//! the app call (`install`, `uninstall`, `status`, `print-unit`, and `--service` for the Windows Service Control
//! Manager), the user's login `PATH` for a supervised server, and the restart after an update.
//!
//! - **Windows**: a service (`--service`, LocalSystem) that starts the server in the logged-on user's session, picked
//!   with `WTSEnumerateSessionsW` (active first, then the console, then the lowest id); `windows.rs`.
//! - **macOS**: the bundled launch agent, registered by the app through `SMAppService`; nothing here (the plist is in
//!   the app bundle, `app/src-tauri/macos`).
//! - **Linux**: a systemd user unit written by `install` ([`systemd`]).
//!
//! Everything that is a decision (names, per-OS paths, the unit text, session choice, restart-on-update) is a pure
//! function in [`spec`]; the rest is thin.

pub mod exec;
pub mod login_path;
pub mod placement;
pub mod spec;
#[cfg(unix)]
pub mod systemd;
pub mod watch;
#[cfg(windows)]
mod windows;

use crate::paths::Dirs;

/// What `pitwall-server <first argument>` asks of the service integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// Register the service with the OS and start it.
    Install,
    /// Stop and remove it.
    Uninstall,
    /// Say whether it is installed and running.
    Status,
    /// Print the systemd unit `install` would write (Linux).
    PrintUnit,
    /// Run under the Windows Service Control Manager.
    WindowsService,
    /// Say whether `install` would register this program, without changing anything and without elevation (Windows): the
    /// app runs it first, so the refusal's own words reach the user before Windows asks for permission.
    CheckInstall,
}

/// The verb in the arguments (after the program name), if the first one is a service verb.
pub fn parse_verb(args: &[String]) -> Option<Verb> {
    match args.first().map(String::as_str) {
        Some("install") => Some(Verb::Install),
        Some("uninstall") => Some(Verb::Uninstall),
        Some("status") => Some(Verb::Status),
        Some("print-unit") => Some(Verb::PrintUnit),
        Some("--service") => Some(Verb::WindowsService),
        Some("check-install") => Some(Verb::CheckInstall),
        _ => None,
    }
}

/// The value of `--data-dir`, if given.
pub fn data_dir_arg(args: &[String]) -> Option<String> {
    args.iter().position(|a| a == "--data-dir").and_then(|i| args.get(i + 1)).cloned()
}

/// The one call `main` makes before parsing its own options. A service verb is carried out and its exit code returned
/// (`main` exits with it). Otherwise this prepares a *supervised* server (started by launchd, systemd or the Windows
/// service: `PITWALL_SERVICE=1`): the user's login `PATH`, and the watcher that restarts it after an update once idle.
/// A server started by hand is left alone. Returns `None` when `main` should go on.
pub fn dispatch() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(verb) = parse_verb(&args) {
        return Some(run_verb(verb));
    }
    login_path::apply_to_process();
    if tokio::runtime::Handle::try_current().is_ok() {
        watch::spawn(data_dir_arg(&args).map_or_else(Dirs::for_user, Dirs::at));
    }
    None
}

/// Prints a verb's outcome and gives its exit code (Windows' own verb runner reports for itself).
#[cfg_attr(windows, allow(dead_code))]
fn report(result: Result<String, String>) -> i32 {
    match result {
        Ok(m) => {
            println!("{m}");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn run_verb(verb: Verb) -> i32 {
    #[cfg(windows)]
    return windows::run_verb(verb);
    #[cfg(target_os = "linux")]
    return report(match verb {
        Verb::Install => systemd::install(),
        Verb::Uninstall => systemd::uninstall(),
        Verb::Status => linux_status(),
        Verb::PrintUnit => systemd::print_unit(),
        Verb::WindowsService => Err("--service is for the Windows Service Control Manager".into()),
        Verb::CheckInstall => Err("check-install is for Windows".into()),
    });
    #[cfg(not(any(windows, target_os = "linux")))]
    return report(match verb {
        Verb::PrintUnit => Err("print-unit is for Linux (systemd)".into()),
        Verb::WindowsService => Err("--service is for the Windows Service Control Manager".into()),
        Verb::CheckInstall => Err("check-install is for Windows".into()),
        _ => Err("on macOS the Pitwall app registers the launch agent (Settings, Service); this program has nothing to install".into()),
    });
}

#[cfg(target_os = "linux")]
fn linux_status() -> Result<String, String> {
    let env = spec::PathEnv::from_process();
    let file = spec::systemd_unit_file(&env).ok_or("HOME is not set")?;
    if !std::path::Path::new(&file).exists() {
        return Ok("not installed (run `pitwall-server install`)".into());
    }
    let out = exec::run_with_timeout(
        std::ffi::OsStr::new("systemctl"),
        &["--user", "is-active", spec::SYSTEMD_UNIT],
        &[],
        std::time::Duration::from_secs(10),
    )?;
    Ok(format!("installed ({file}); state: {}", out.stdout.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn only_a_leading_service_verb_is_taken() {
        assert_eq!(parse_verb(&a(&["install"])), Some(Verb::Install));
        assert_eq!(parse_verb(&a(&["uninstall"])), Some(Verb::Uninstall));
        assert_eq!(parse_verb(&a(&["status"])), Some(Verb::Status));
        assert_eq!(parse_verb(&a(&["print-unit"])), Some(Verb::PrintUnit));
        assert_eq!(parse_verb(&a(&["--service"])), Some(Verb::WindowsService));
        // The unelevated dry run of install (the app shows its words before Windows asks for permission).
        assert_eq!(parse_verb(&a(&["check-install"])), Some(Verb::CheckInstall));
        assert_eq!(parse_verb(&a(&["--port", "1", "install"])), None);
        assert_eq!(parse_verb(&a(&[])), None);
    }

    #[test]
    fn the_data_dir_argument_is_found() {
        assert_eq!(data_dir_arg(&a(&["--port", "1", "--data-dir", "/x"])).as_deref(), Some("/x"));
        assert_eq!(data_dir_arg(&a(&["--data-dir"])), None);
        assert_eq!(data_dir_arg(&a(&[])), None);
    }

    #[test]
    fn reports_map_to_exit_codes() {
        assert_eq!(report(Ok("fine".into())), 0);
        assert_eq!(report(Err("bad".into())), 1);
    }

    /// Regression (B6 security review P1-1, P1-2): the LocalSystem service logged to `%ProgramData%\Pitwall\logs\service.log`,
    /// with the folder taken from the environment, made with `create_dir_all`, measured through links and rotated with
    /// `remove_file` by path, so a standard user who made that folder first (a junction to an object-manager link) could have
    /// SYSTEM delete any file. The service writes to the OS log instead (the Application event log under its own source,
    /// paddock-service's `os_log`, ADR 0054 §2.3): no file and no folder of its own outside the protected install.
    #[test]
    fn the_system_service_writes_no_file_in_a_folder_users_can_make() {
        // B6 re-review P2-2: no file system call and no environment read at all, comments aside (Clippy's
        // disallowed-methods is per crate, and the server uses the file system elsewhere).
        let code: String = include_str!("windows.rs")
            .lines()
            .map(|l| l.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        for banned in ["fs::", "File::", "OpenOptions", "var_os(", "env::var(", "service.log"] {
            assert!(!code.contains(banned), "the service code still uses {banned}");
        }
        assert!(code.contains("paddock_service::os_log::open"), "the service writes to the OS log");
    }
}
