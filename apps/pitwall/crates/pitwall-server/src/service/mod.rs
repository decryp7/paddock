//! Running Pitwall's server as an OS service, on the shared service crate (rule 55, ADR 0056): the verbs the installers and
//! the app call (`install`, `uninstall`, `status`, `print-unit`, `print-plist`, and `--service` for the Windows Service Control
//! Manager), the user's login `PATH` for a supervised server, and the restart after an update. What the service is lives in
//! [`pitwall_service`]; the registration is the shared [`Manager`] over the backend of this OS.
//!
//! - **Windows**: a service (`--service`, LocalSystem) that starts the server in the logged-on user's session
//!   ([`paddock_service::native::host_program`]); `install` and `uninstall` are what the app runs elevated through Windows' own
//!   prompt, and what the installer runs.
//! - **macOS**: the bundled launch agent, registered by the app through `SMAppService`; `install` and `uninstall` say so.
//! - **Linux**: a systemd user unit written by `install`.
//!
//! The decisions are the shared crate's and are tested there; what is tested here is the wiring: which verb asks the manager
//! what, and what is printed.

pub mod watch;

use crate::paths::Dirs;
use paddock_service::backend::Side;
use paddock_service::{Manager, ServiceError, Timings, Verb};
use pitwall_service::{spec, LABEL, NAME, SUPERVISED_ENV};
use std::path::Path;
use std::sync::atomic::AtomicBool;

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
    if let Some(verb) = Verb::parse(&args) {
        return Some(run_verb(verb, &args));
    }
    if let Some(path) = paddock_service::supervised::supervised_path(SUPERVISED_ENV) {
        std::env::set_var("PATH", path);
    }
    if tokio::runtime::Handle::try_current().is_ok() {
        watch::spawn(data_dir_arg(&args).map_or_else(Dirs::for_user, Dirs::at));
    }
    None
}

/// The shared manager over this OS's backend, for the program side. `install` does not wait for the service to answer (the
/// installer and the elevated child must return).
fn manager(program: &Path, data: &Path) -> Result<Manager, ServiceError> {
    let backend = paddock_service::native::host_backend(&spec(), program, Side::Program, data)?;
    Ok(manager_over(backend, data))
}

/// [`manager`] over a backend (the tests pass a fake one).
fn manager_over(backend: Box<dyn paddock_service::Backend>, data: &Path) -> Manager {
    Manager::new(NAME, backend, data, LABEL, env!("CARGO_PKG_VERSION"), || false).with_timings(Timings::default().without_start_wait())
}

/// What a verb prints, or why it failed. Pure over the manager.
pub fn run_with(verb: Verb, m: &mut Manager, program: &Path) -> Result<String, String> {
    let err = |e: ServiceError| e.to_string();
    match verb {
        Verb::Install => m
            .install(&AtomicBool::new(false))
            .map(|_| "Pitwall's service is installed and started".to_string())
            .map_err(err),
        Verb::Uninstall => m.uninstall().map(|()| "Pitwall's service is removed".to_string()).map_err(err),
        Verb::Status => {
            let s = m.status();
            Ok(match (s.installed, s.running) {
                (false, _) => "not installed (run `pitwall-server install`)".to_string(),
                (true, true) => "installed; the server runs".to_string(),
                (true, false) => format!("installed; not running{}", s.message.map(|t| format!(" ({t})")).unwrap_or_default()),
            })
        }
        Verb::PrintUnit => Ok(paddock_service::render::systemd_unit(&spec(), &program.to_string_lossy(), None)),
        Verb::PrintPlist => Ok(paddock_service::render::launchd_plist(
            &spec(),
            paddock_service::render::LaunchdProgram::Bundled,
        )),
        Verb::Stop => Err("stop is not a verb of the Pitwall server; stop the service from Settings or the OS".to_string()),
        Verb::WindowsService => Err("--service is for the Windows Service Control Manager".to_string()),
    }
}

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

fn run_verb(verb: Verb, args: &[String]) -> i32 {
    let program = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => return report(Err(format!("the server program cannot be found: {e}"))),
    };
    if verb == Verb::WindowsService {
        return host(&program);
    }
    // print-unit and print-plist need no registration and no OS access.
    if matches!(verb, Verb::PrintUnit | Verb::PrintPlist | Verb::Stop) {
        return report(run_with_none(verb, &program));
    }
    if cfg!(target_os = "macos") {
        return report(Err(
            "on macOS the Pitwall app registers the launch agent (Settings, Service); this program has nothing to install".to_string(),
        ));
    }
    let data = data_dir_arg(args).map_or_else(Dirs::for_user, Dirs::at);
    match manager(&program, &data.root) {
        Ok(mut m) => report(run_with(verb, &mut m, &program)),
        Err(e) => report(Err(e.to_string())),
    }
}

/// The verbs that read nothing of the OS.
fn run_with_none(verb: Verb, program: &Path) -> Result<String, String> {
    match verb {
        Verb::PrintUnit => Ok(paddock_service::render::systemd_unit(&spec(), &program.to_string_lossy(), None)),
        Verb::PrintPlist => Ok(paddock_service::render::launchd_plist(
            &spec(),
            paddock_service::render::LaunchdProgram::Bundled,
        )),
        _ => Err("stop is not a verb of the Pitwall server; stop the service from Settings or the OS".to_string()),
    }
}

/// Runs under the Windows Service Control Manager: the shared host starts this program in the logged-on user's session.
#[cfg(windows)]
fn host(program: &Path) -> i32 {
    match paddock_service::native::host_program(&spec(), program.to_path_buf()) {
        Ok(code) => i32::try_from(code).unwrap_or(1),
        Err(e) => report(Err(e.to_string())),
    }
}

#[cfg(not(windows))]
fn host(_program: &Path) -> i32 {
    report(Err("--service is for the Windows Service Control Manager".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_service::backend::{Systemd, Target};
    use paddock_service::testing::FakeSystem;

    fn over_fake_systemd(dir: &Path) -> (Manager, FakeSystem) {
        let sys = FakeSystem::new();
        let backend = Systemd::new(
            pitwall_service::spec_of(false),
            dir.join("pitwall.service"),
            Target::exe("/opt/pitwall/pitwall-server"),
            Box::new(sys.clone()),
        );
        (manager_over(Box::new(backend), dir), sys)
    }

    #[test]
    fn the_data_dir_argument_is_found() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(data_dir_arg(&a(&["--port", "1", "--data-dir", "/x"])).as_deref(), Some("/x"));
        assert_eq!(data_dir_arg(&a(&["--data-dir"])), None);
        assert_eq!(data_dir_arg(&a(&[])), None);
    }

    #[test]
    fn install_and_uninstall_go_through_the_manager_and_say_so() {
        let dir = tempfile::tempdir().unwrap();
        let (mut m, sys) = over_fake_systemd(dir.path());
        let program = Path::new("/opt/pitwall/pitwall-server");
        assert_eq!(
            run_with(Verb::Status, &mut m, program).unwrap(),
            "not installed (run `pitwall-server install`)"
        );
        let said = run_with(Verb::Install, &mut m, program).unwrap();
        assert!(said.contains("installed and started"), "{said}");
        assert!(
            sys.commands().iter().any(|c| c.contains("systemctl") && c.contains("enable")),
            "the unit is enabled through systemctl: {:?}",
            sys.commands()
        );
        assert!(sys
            .file(&dir.path().join("pitwall.service"))
            .is_some_and(|u| u.contains("PITWALL_SERVICE=1")));
        let gone = run_with(Verb::Uninstall, &mut m, program).unwrap();
        assert!(gone.contains("removed"), "{gone}");
        assert!(sys.file(&dir.path().join("pitwall.service")).is_none());
    }

    #[test]
    fn print_unit_and_print_plist_render_the_one_spec_and_the_other_verbs_refuse_with_a_reason() {
        let program = Path::new("/opt/pitwall/pitwall-server");
        let unit = run_with_none(Verb::PrintUnit, program).unwrap();
        assert!(unit.contains("ExecStart=") && unit.contains("PITWALL_SERVICE=1"), "{unit}");
        let plist = run_with_none(Verb::PrintPlist, program).unwrap();
        assert!(plist.contains("net.decryptology.pitwall.service"), "{plist}");
        assert!(run_with_none(Verb::Stop, program).unwrap_err().contains("not a verb"));
        let dir = tempfile::tempdir().unwrap();
        let (mut m, _) = over_fake_systemd(dir.path());
        assert!(run_with(Verb::WindowsService, &mut m, program)
            .unwrap_err()
            .contains("Service Control Manager"));
    }

    #[test]
    fn reports_map_to_exit_codes() {
        assert_eq!(report(Ok("fine".into())), 0);
        assert_eq!(report(Err("bad".into())), 1);
    }
}
