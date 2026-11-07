//! Running Pitwall's server as an OS service, on the shared service crate (rule 55, ADR 0056): the verbs the installers and
//! the app call (`install`, `uninstall`, `status`, `print-unit`, `print-plist`, and `--service` for the Windows Service Control
//! Manager), the user's login `PATH` for a supervised server, and the restart after an update. What the service is lives in
//! [`pitwall_service`]; the registration is the shared [`Manager`] over the backend of this OS.
//!
//! - **Windows**: a service (`--service`, LocalSystem) that starts the server in the logged-on user's session
//!   ([`paddock_service::native::host_program`]); `install` and `uninstall` are what the app runs elevated through Windows' own
//!   prompt, and what the installer runs. `install` registers only a protected copy in Program Files and never moves another
//!   copy's registration ([`placement`], plan B6): the plan comes before the manager, and the registration is read back after
//!   it. `check-install` says the same decision unelevated, so the app and the installer can show its words.
//! - **macOS**: the bundled launch agent, registered by the app through `SMAppService`; `install` and `uninstall` say so.
//! - **Linux**: a systemd user unit written by `install`.
//!
//! The decisions are the shared crate's and are tested there; what is tested here is the wiring: which verb asks the manager
//! what, and what is printed.

pub mod placement;
pub mod watch;
#[cfg(windows)]
mod windows;

use crate::paths::Dirs;
use paddock_service::backend::Side;
use paddock_service::{Manager, ServiceError, Timings, Verb};
use pitwall_service::{spec, LABEL, NAME, SUPERVISED_ENV};
use placement::{Install, Plan, Refusal};
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
    if is_check_install(&args) {
        return Some(report(check_install()));
    }
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

/// The verb of the unelevated dry run of `install` (Windows). It is Pitwall's own, not one of the shared [`Verb`]s.
pub const CHECK_INSTALL: &str = "check-install";

/// Whether the arguments ask for [`CHECK_INSTALL`] (the first argument, as for every verb).
pub fn is_check_install(args: &[String]) -> bool {
    args.first().is_some_and(|a| a == CHECK_INSTALL)
}

/// Says whether `install` would register this program, without changing anything and without elevation: the app and the
/// installer run it first, so the refusal's own words reach the user (the elevated run cannot hand its words back).
#[cfg(windows)]
fn check_install() -> Result<String, String> {
    windows::check_install()
}

#[cfg(not(windows))]
fn check_install() -> Result<String, String> {
    Err("check-install is for Windows".to_string())
}

/// The answer of `check-install` for a plan: the refusal's words, or that `install` may go on. This very program registered
/// already is no refusal here: `install` then repairs that registration in place (see [`install_planned`]).
pub fn check_verdict(plan: Result<Install, Refusal>) -> Result<String, String> {
    match plan {
        Ok(_) | Err(Refusal::AlreadyInstalled) => Ok("Pitwall's service can be registered for this program".to_string()),
        Err(refusal) => Err(refusal.to_string()),
    }
}

/// The service manager's registration of Pitwall's service, read back after `install` (ADR 0054 §1.1). On Windows one handle
/// opened with query and DELETE access serves both calls, so what is removed is what was read.
pub trait Registered {
    /// The program the registration's ImagePath starts (`None`: there is no registration, or it cannot be read).
    fn held(&mut self) -> Option<String>;
    /// Deletes the registration that [`Registered::held`] read.
    ///
    /// # Errors
    /// Why it could not be deleted.
    fn remove(&mut self) -> Result<(), String>;
}

/// `pitwall-server install` on Windows, after [`placement::plan_install`] decided over this program and what the service
/// manager holds: a refusal changes nothing and says what to do; otherwise the shared manager registers this program
/// (replacing an unsafe registration of another copy, or repairing this program's own), and the registration is read back:
/// one that does not start the checked program is deleted again. `log` is the service's OS log (Event Viewer).
///
/// # Errors
/// The words for the user.
pub fn install_planned(
    plan: Result<Install, Refusal>,
    program: &str,
    m: &mut Manager,
    reg: &mut dyn Registered,
    log: &mut dyn FnMut(&str),
) -> Result<String, String> {
    match &plan {
        Err(Refusal::AlreadyInstalled) => {
            log(&format!(
                "install checked: {program} is registered already; the registration is repaired in place"
            ));
        }
        Err(refusal) => {
            log(&refusal.log_line());
            return Err(refusal.to_string());
        }
        Ok(install) => {
            log(&format!(
                "install checked: {program} is in its protected folder under Program Files"
            ));
            if let Plan::ReplaceUnsafe { .. } = install.plan {
                // Another copy's registration that is not safe to run as the system goes; nothing of that copy is kept.
                log("install: an unsafe registration of another program is replaced");
            }
        }
    }
    let installed = m.install(&AtomicBool::new(false));
    let held = reg.held();
    let mismatched = held.as_deref().is_some_and(|h| !placement::same_path(h, program));
    if installed.is_ok() || mismatched {
        placement::read_back(held.as_deref(), program, || {
            let removed = reg.remove();
            log(&format!(
                "install: the registration read back does not start the checked program; removing it: {}",
                removed.as_ref().map_or_else(|e| format!("failed ({e})"), |()| "done".into())
            ));
            removed
        })?;
    }
    installed
        .map(|_| "Pitwall's service is installed and started".to_string())
        .map_err(|e| e.to_string())
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
    #[cfg(windows)]
    if verb == Verb::Install {
        return report(windows::install(&program, &data.root));
    }
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
    windows::host(program)
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

    #[test]
    fn check_install_is_pitwalls_own_verb_and_only_first() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // The unelevated dry run of install (the app shows its words before Windows asks for permission).
        assert!(is_check_install(&a(&["check-install"])));
        assert!(!is_check_install(&a(&["--port", "1", "check-install"])));
        assert!(!is_check_install(&a(&[])));
        assert_eq!(Verb::parse(&a(&["check-install"])), None, "not one of the shared verbs");
        #[cfg(not(windows))]
        assert!(check_install().unwrap_err().contains("for Windows"));
    }

    const EXE: &str = "C:\\Program Files\\Pitwall\\pitwall-server.exe";

    fn planned(plan: Plan) -> Result<Install, Refusal> {
        Ok(Install {
            program: EXE.to_string(),
            image_path: placement::windows_command_line_for(EXE),
            plan,
        })
    }

    /// The service manager's registration as a test says: what it holds, and whether removing it works.
    struct FakeRegistered {
        held: Option<String>,
        remove: Result<(), String>,
        removed: bool,
    }

    impl Registered for FakeRegistered {
        fn held(&mut self) -> Option<String> {
            self.held.clone()
        }
        fn remove(&mut self) -> Result<(), String> {
            self.removed = true;
            self.remove.clone()
        }
    }

    fn holding(held: Option<&str>) -> FakeRegistered {
        FakeRegistered {
            held: held.map(str::to_string),
            remove: Ok(()),
            removed: false,
        }
    }

    fn over_fake_scm(dir: &Path) -> (Manager, FakeSystem) {
        let sys = FakeSystem::new();
        let backend = paddock_service::backend::WindowsService::new(
            pitwall_service::spec_of(true),
            paddock_service::backend::Target::exe(EXE),
            Box::new(sys.clone()),
        );
        (manager_over(Box::new(backend), dir), sys)
    }

    /// B6 merged into B1b: the shared manager replaces whatever is registered, so the Program Files plan must come first. A
    /// refusal reaches the service manager not at all, and says what to do; the OS log gets the refusal's own line.
    #[test]
    fn a_refused_plan_never_reaches_the_service_manager() {
        let refusals = [
            Refusal::OutsideProgramFiles {
                program: "D:\\x\\pitwall-server.exe".into(),
                program_files: "C:\\Program Files".into(),
            },
            Refusal::NotProtected {
                why: "account Users may write".into(),
            },
            Refusal::OddPath("\\\\?\\C:\\x".into()),
            Refusal::NoProgramFiles("FOLDERID_ProgramFiles failed".into()),
            Refusal::OtherCopy {
                registered: "C:\\Program Files\\Pitwall Old\\pitwall-server.exe".into(),
                program: EXE.into(),
            },
        ];
        for refusal in refusals {
            let dir = tempfile::tempdir().unwrap();
            let (mut m, sys) = over_fake_scm(dir.path());
            let mut reg = holding(None);
            let mut lines = vec![];
            let said = install_planned(Err(refusal.clone()), EXE, &mut m, &mut reg, &mut |l| lines.push(l.to_string()));
            assert_eq!(said, Err(refusal.to_string()));
            assert!(sys.commands().is_empty(), "{refusal:?} ran {:?}", sys.commands());
            assert!(!reg.removed);
            assert_eq!(lines, vec![refusal.log_line()]);
            assert!(check_verdict(Err(refusal.clone())).is_err());
        }
    }

    /// A plan that lets the install go on registers exactly the checked program with the ImagePath the plan names.
    #[test]
    fn an_accepted_plan_registers_the_checked_program_through_the_manager() {
        for plan in [
            Plan::Create,
            Plan::ReplaceUnsafe {
                old: "D:\\x\\pitwall-server.exe".into(),
            },
        ] {
            let dir = tempfile::tempdir().unwrap();
            let (mut m, sys) = over_fake_scm(dir.path());
            let mut reg = holding(Some(EXE));
            let mut lines = vec![];
            let _ = install_planned(planned(plan.clone()), EXE, &mut m, &mut reg, &mut |l| lines.push(l.to_string()));
            let create = sys.commands().into_iter().find(|c| c.contains("create Pitwall"));
            let create = create.unwrap_or_else(|| panic!("{plan:?}: no sc create in {:?}", sys.commands()));
            assert!(create.contains(EXE) && create.contains("--service"), "{create}");
            assert!(!reg.removed, "the registration reads back as this program");
            assert!(lines[0].starts_with("install checked:"), "{lines:?}");
            assert!(check_verdict(planned(plan)).is_ok());
        }
        // This very program registered already: repaired in place, and check-install does not refuse it.
        assert!(check_verdict(Err(Refusal::AlreadyInstalled)).is_ok());
        let dir = tempfile::tempdir().unwrap();
        let (mut m, sys) = over_fake_scm(dir.path());
        let _ = install_planned(Err(Refusal::AlreadyInstalled), EXE, &mut m, &mut holding(Some(EXE)), &mut |_| {});
        assert!(!sys.commands().is_empty(), "the manager looks at the registration");
    }

    /// The read-back (ADR 0054 §1.1, B6 re-review P2-1): a registration that does not start the checked program is removed,
    /// whether the manager said it worked or not, and the words say what the removal did.
    #[test]
    fn a_registration_read_back_as_another_program_is_removed() {
        for remove in [Ok(()), Err("access denied".to_string())] {
            let dir = tempfile::tempdir().unwrap();
            let (mut m, _) = over_fake_scm(dir.path());
            let mut reg = FakeRegistered {
                held: Some("D:\\Elsewhere\\pitwall-server.exe".into()),
                remove: remove.clone(),
                removed: false,
            };
            let mut lines = vec![];
            let said = install_planned(planned(Plan::Create), EXE, &mut m, &mut reg, &mut |l| lines.push(l.to_string())).unwrap_err();
            assert!(reg.removed, "{said}");
            match remove {
                Ok(()) => assert!(said.contains("so it was removed again"), "{said}"),
                Err(_) => assert!(said.contains("could not be removed (access denied)"), "{said}"),
            }
            assert!(lines.iter().any(|l| l.contains("removing it")), "{lines:?}");
        }
    }

    /// The plan's ImagePath is the one the shared manager registers (one command-line writer, ADR 0054 §1.1).
    #[test]
    fn the_plans_image_path_is_what_the_manager_registers() {
        let config = paddock_service::render::scm_config(&pitwall_service::spec_of(true), EXE);
        assert_eq!(placement::windows_command_line_for(EXE), config.command_line);
    }

    /// Regression (B6 security review P1-1, P1-2): the LocalSystem service logged to `%ProgramData%\Pitwall\logs\service.log`,
    /// with the folder taken from the environment, made with `create_dir_all`, measured through links and rotated with
    /// `remove_file` by path, so a standard user who made that folder first (a junction to an object-manager link) could have
    /// SYSTEM delete any file. The service writes to the OS log instead (the Application event log under its own source,
    /// paddock-service's `os_log`, ADR 0054 §2.3): no file and no folder of its own outside the protected install. Since B1b the
    /// service code is Pitwall's `windows.rs` and the shared host it calls (the SCM host and event log in `native.rs`,
    /// `service_main.rs`, `scm_host.rs`), so all of it is held to the rule.
    #[test]
    fn the_system_service_writes_no_file_in_a_folder_users_can_make() {
        fn code_of(text: &str) -> String {
            // B6 re-review P2-2: no file system call and no environment read at all, comments aside (Clippy's
            // disallowed-methods is per crate, and the server uses the file system elsewhere).
            text.lines()
                .map(|l| l.split("//").next().unwrap_or(""))
                .collect::<Vec<_>>()
                .join("\n")
        }
        fn between<'a>(text: &'a str, from: &str, to: &str) -> &'a str {
            let start = text.find(from).unwrap_or_else(|| panic!("{from} is gone: re-point this test"));
            let rest = &text[start..];
            &rest[..rest.find(to).unwrap_or_else(|| panic!("{to} is gone: re-point this test"))]
        }
        let native = include_str!("../../../../../../crates/paddock-service/src/native.rs");
        let parts = [
            ("pitwall windows.rs", code_of(include_str!("windows.rs"))),
            (
                "paddock-service service_main.rs",
                code_of(include_str!("../../../../../../crates/paddock-service/src/service_main.rs")),
            ),
            (
                "paddock-service scm_host.rs",
                code_of(include_str!("../../../../../../crates/paddock-service/src/scm_host.rs")),
            ),
            (
                "paddock-service native.rs mod eventlog",
                code_of(between(native, "mod eventlog {", "mod syslog {")),
            ),
            (
                "paddock-service native.rs mod scm",
                code_of(between(native, "mod scm {", "pub fn process_has_full_token")),
            ),
        ];
        for (name, code) in &parts {
            for banned in ["fs::", "File::", "OpenOptions", "var_os(", "env::var(", "service.log", "create_dir"] {
                assert!(!code.contains(banned), "{name} still uses {banned}");
            }
        }
        assert!(
            parts[0].1.contains("paddock_service::os_log::open"),
            "the service writes to the OS log"
        );
        assert!(parts[0].1.contains("native::host_program"), "the service runs on the shared host");
        assert!(parts[4].1.contains("os_log.write"), "the shared host writes to the OS log");
    }
}
