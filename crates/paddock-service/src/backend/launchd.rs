//! macOS: the launch agent inside the app bundle registered through `SMAppService` ([`LaunchdBundled`]), and a plist
//! file loaded with `launchctl bootstrap` ([`LaunchdFile`], development builds and launch daemons). Both read launchd's
//! own report (`launchctl print`, `print-disabled`) to see a stale registration (R03) and the user's switch.

use super::{Backend, Capabilities, Inspection, Job, Mechanism, Registration, StartFacts, Target};
use crate::launchctl::{bootout, job_report};
use crate::parse::{JobReport, parse_print_disabled, plist_program};
use crate::paths::{LAUNCHCTL, launchd_domain, launchd_target};
use crate::port::{AppService, AppServiceStatus, RegisterError, System};
use crate::render::{LaunchdProgram, launchd_plist};
use crate::spec::ServiceSpec;
use paddock_proc::RunState;
use std::path::PathBuf;
use std::time::Duration;

fn job_of(report: Option<JobReport>) -> Job {
    match report {
        None => Job::Unknown,
        Some(r) if !r.found => Job::Absent,
        Some(r) if r.stale() => Job::Refused,
        Some(r) if r.running => Job::Running,
        Some(_) => Job::NotRunning,
    }
}

/// Whether launchd's override database marks the label disabled (`None`: not listed or not readable).
fn disabled(sys: &mut dyn System, domain: &str, label: &str) -> Option<bool> {
    let out = sys.run(LAUNCHCTL, &["print-disabled", domain]).ok()?;
    if !out.success() {
        return None;
    }
    parse_print_disabled(&out.stdout, label)
}

fn gone(sys: &mut dyn System, target: &str) -> bool {
    job_report(sys, target).is_some_and(|r| !r.found)
}

/// What launchd says about a starting job (one `launchctl print`), plus the CPU time of its process while it runs.
fn start_facts(sys: &mut dyn System, target: &str) -> StartFacts {
    let Some(report) = job_report(sys, target) else {
        return StartFacts::unknown();
    };
    let running_pid = report.pid.filter(|_| report.running);
    StartFacts {
        job: job_of(Some(report)),
        pid: report.pid,
        runs: report.runs,
        last_exit: report.last_exit_code,
        cpu: running_pid.and_then(|pid| sys.cpu_time(pid)),
        run_state: running_pid.map_or(RunState::Unknown, |pid| sys.run_state(pid)),
    }
}

/// The launch agent inside the app bundle (`Contents/Library/LaunchAgents/<label>.plist`, rendered by
/// [`launchd_plist`] with [`LaunchdProgram::Bundled`]), registered through `SMAppService`; the user approves it in
/// Login Items.
pub struct LaunchdBundled {
    spec: ServiceSpec,
    uid: u32,
    app: Box<dyn AppService>,
    sys: Box<dyn System>,
}

impl LaunchdBundled {
    /// The bundled agent of `spec` for the user `uid` (`paddock_service::native::current_uid()`).
    pub fn new(
        spec: ServiceSpec,
        uid: u32,
        app: Box<dyn AppService>,
        sys: Box<dyn System>,
    ) -> Self {
        Self {
            spec,
            uid,
            app,
            sys,
        }
    }

    fn target(&self) -> String {
        launchd_target(&self.spec, self.uid)
    }
}

impl Backend for LaunchdBundled {
    fn mechanism(&self) -> Mechanism {
        Mechanism::LaunchdBundled
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            automatic: true,
            pinned_identity: true,
            login_items: true,
        }
    }

    fn inspect(&mut self) -> Inspection {
        let status = self.app.status();
        let domain = launchd_domain(self.spec.kind(), self.uid);
        let off = disabled(self.sys.as_mut(), &domain, self.spec.label()) == Some(true);
        let registration = match status {
            AppServiceStatus::RequiresApproval => Registration::NeedsApproval,
            _ if off => Registration::SwitchedOff,
            // NotFound is what macOS (13 to 26) reports for an app that never registered: Background Task Management has
            // no record of it until the first `register`. That is "not installed", and Install must go ahead; when the
            // app really cannot register from where it is, `register` itself says so.
            AppServiceStatus::NotRegistered | AppServiceStatus::NotFound => Registration::Absent,
            AppServiceStatus::Enabled => Registration::Registered,
        };
        let target = self.target();
        Inspection {
            registration,
            job: job_of(job_report(self.sys.as_mut(), &target)),
            ..Inspection::absent()
        }
    }

    fn register(&mut self) -> Result<(), RegisterError> {
        self.app.register()
    }

    fn unregister(&mut self) -> Result<(), String> {
        self.app.unregister()
    }

    fn evict(&mut self) -> Result<(), String> {
        let target = self.target();
        bootout(self.sys.as_mut(), &target)
    }

    fn job_gone(&mut self) -> bool {
        let target = self.target();
        gone(self.sys.as_mut(), &target)
    }

    fn observe_start(&mut self) -> Option<StartFacts> {
        let target = self.target();
        Some(start_facts(self.sys.as_mut(), &target))
    }

    fn pause(&mut self, how_long: Duration) {
        self.sys.pause(how_long);
    }

    fn open_login_items(&mut self) -> bool {
        self.app.open_login_items();
        true
    }
}

/// A plist file (`~/Library/LaunchAgents` or `/Library/LaunchDaemons`) loaded with `launchctl bootstrap`.
pub struct LaunchdFile {
    spec: ServiceSpec,
    uid: u32,
    plist: PathBuf,
    target: Target,
    sys: Box<dyn System>,
}

impl LaunchdFile {
    /// The plist file `plist` (`paths::launchd_plist_path`) starting `target`, for the user `uid`.
    pub fn new(
        spec: ServiceSpec,
        uid: u32,
        plist: PathBuf,
        target: Target,
        sys: Box<dyn System>,
    ) -> Self {
        Self {
            spec,
            uid,
            plist,
            target,
            sys,
        }
    }

    /// The text this copy registers.
    pub fn definition(&self) -> String {
        let wd = self
            .target
            .working_dir
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        let log = self
            .target
            .log
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        launchd_plist(
            &self.spec,
            LaunchdProgram::File {
                exe: &self.target.exe_text(),
                working_dir: wd.as_deref(),
                log: log.as_deref(),
            },
        )
    }

    fn launchd_target(&self) -> String {
        launchd_target(&self.spec, self.uid)
    }
}

fn denied(text: &str) -> bool {
    text.contains("Operation not permitted") || text.contains("Permission denied")
}

impl Backend for LaunchdFile {
    fn mechanism(&self) -> Mechanism {
        Mechanism::LaunchdFile
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            automatic: true,
            pinned_identity: true,
            login_items: false,
        }
    }

    fn inspect(&mut self) -> Inspection {
        let target = self.launchd_target();
        let job = job_of(job_report(self.sys.as_mut(), &target));
        let text = match self.sys.read(&self.plist) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Inspection {
                    job,
                    ..Inspection::absent()
                };
            }
            Err(e) => {
                return Inspection {
                    registration: Registration::Unavailable(format!(
                        "{} cannot be read: {e}",
                        self.plist.display()
                    )),
                    job,
                    ..Inspection::absent()
                };
            }
        };
        let domain = launchd_domain(self.spec.kind(), self.uid);
        let off = disabled(self.sys.as_mut(), &domain, self.spec.label()) == Some(true);
        let program = plist_program(&text);
        let program_exists = program
            .as_ref()
            .map(|p| self.sys.is_program(std::path::Path::new(p)));
        Inspection {
            registration: if off {
                Registration::SwitchedOff
            } else {
                Registration::Registered
            },
            job,
            points_here: program.as_ref().map(|p| *p == self.target.exe_text()),
            program,
            program_exists,
            definition_current: Some(text == self.definition()),
        }
    }

    fn register(&mut self) -> Result<(), RegisterError> {
        let text = self.definition();
        if let Err(e) = self.sys.write(&self.plist, &text) {
            return Err(if e.kind() == std::io::ErrorKind::PermissionDenied {
                RegisterError::NeedsElevation
            } else {
                RegisterError::Other(format!(
                    "{} could not be written: {e}",
                    self.plist.display()
                ))
            });
        }
        let domain = launchd_domain(self.spec.kind(), self.uid);
        let plist = self.plist.to_string_lossy().into_owned();
        match self.sys.run(LAUNCHCTL, &["bootstrap", &domain, &plist]) {
            Ok(o) if o.success() => Ok(()),
            Ok(o) if denied(&o.stderr) => Err(RegisterError::NeedsElevation),
            Ok(o) => Err(RegisterError::Other(format!(
                "launchctl bootstrap: {}",
                o.reason()
            ))),
            Err(e) => Err(RegisterError::Other(format!("launchctl bootstrap {e}"))),
        }
    }

    fn unregister(&mut self) -> Result<(), String> {
        match self.sys.remove(&self.plist) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!(
                "{} could not be removed: {e}",
                self.plist.display()
            )),
        }
    }

    fn evict(&mut self) -> Result<(), String> {
        let target = self.launchd_target();
        bootout(self.sys.as_mut(), &target)
    }

    fn job_gone(&mut self) -> bool {
        let target = self.launchd_target();
        gone(self.sys.as_mut(), &target)
    }

    fn observe_start(&mut self) -> Option<StartFacts> {
        let target = self.launchd_target();
        Some(start_facts(self.sys.as_mut(), &target))
    }

    fn pause(&mut self, how_long: Duration) {
        self.sys.pause(how_long);
    }

    fn definition(&self) -> Option<String> {
        Some(Self::definition(self))
    }
}
