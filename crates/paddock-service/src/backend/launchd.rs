//! macOS: the launch agent inside the app bundle registered through `SMAppService` ([`LaunchdBundled`]), and a plist
//! file loaded with `launchctl bootstrap` ([`LaunchdFile`], development builds and launch daemons). Both read launchd's
//! own report (`launchctl print`, `print-disabled`) to see a stale registration (R03) and the user's switch.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use super::{
    Backend, Capabilities, DisabledMark, Inspection, Job, LastExit, LoginItemFacts, Mechanism,
    Registration, StartFacts, Target, UnitFile, UnitFileState, UnitOrigin, UnregisterError,
    as_written, with_failure,
};
use crate::launchctl::{bootout, job_report};
use crate::parse::{JobReport, parse_print_disabled, plist_program};
use crate::paths::{LAUNCHCTL, launchd_domain, launchd_target};
use crate::port::{AppService, AppServiceStatus, RegisterError, System};
use crate::render::{LaunchdProgram, launchd_plist};
use crate::spec::ServiceSpec;
use crate::stop::StopMechanism;
use paddock_proc::RunState;
use std::path::PathBuf;
use std::time::Duration;

fn job_of(report: Option<JobReport>) -> Job {
    match report {
        None => Job::Unknown,
        Some(r) if !r.found => Job::Absent,
        Some(r) if r.stale() => Job::Refused,
        Some(r) if r.running => Job::Running,
        Some(r) => with_failure(Job::NotRunning, last_exit_of(&r)),
    }
}

/// What launchd says about a starting job (one `launchctl print`), plus the CPU time and the run state of its process while it
/// runs. A job that stopped with an error reads `NotRunning` here: how it ended is `last_exit`.
fn start_facts(sys: &mut dyn System, target: &str) -> StartFacts {
    let Some(report) = job_report(sys, target) else {
        return StartFacts::unknown();
    };
    let running_pid = report.pid.filter(|_| report.running);
    let job = match job_of(Some(report)) {
        Job::Failed(_) => Job::NotRunning,
        job => job,
    };
    StartFacts {
        job,
        pid: report.pid,
        runs: report.runs,
        last_exit: report.last_exit_code,
        cpu: running_pid.and_then(|pid| sys.cpu_time(pid)),
        run_state: running_pid.map_or(RunState::Unknown, |pid| sys.run_state(pid)),
    }
}

/// How launchd says the job's last process ended: the terminating signal when it names one, else the exit code (`None`
/// before the first exit).
fn last_exit_of(report: &JobReport) -> Option<LastExit> {
    match (report.signal, report.last_exit_code) {
        (Some(signal), _) => Some(LastExit::Signal(signal)),
        (None, Some(code)) => Some(LastExit::Code(code)),
        (None, None) => None,
    }
}

/// Everything the report says: the job, how its last process ended and how often launchd restarted it (`runs` minus
/// the first start).
fn look_at(report: Option<JobReport>) -> (Job, Option<LastExit>, Option<u32>) {
    let last_exit = report.as_ref().and_then(last_exit_of);
    let restarts = report
        .and_then(|r| r.runs)
        .map(|runs| runs.saturating_sub(1));
    (job_of(report), last_exit, restarts)
}

/// What launchd's override database says about the label: listed disabled or enabled, not listed, or could not be read
/// (the tool failed or timed out: that is not "switched on").
fn disabled_mark(sys: &mut dyn System, domain: &str, label: &str) -> DisabledMark {
    let out = match sys.run(LAUNCHCTL, &["print-disabled", domain]) {
        Ok(out) if out.success() => out,
        _ => return DisabledMark::Unreadable,
    };
    match parse_print_disabled(&out.stdout, label) {
        Some(true) => DisabledMark::Disabled,
        Some(false) => DisabledMark::Enabled,
        None => DisabledMark::NotListed,
    }
}

/// Whether launchd's override database marks the label disabled (`None`: not listed or not readable).
fn disabled(sys: &mut dyn System, domain: &str, label: &str) -> Option<bool> {
    disabled_mark(sys, domain, label).as_option()
}

/// The launchd job a stop names, while launchd holds it.
fn stop_of(sys: &mut dyn System, target: String) -> Option<StopMechanism> {
    job_report(sys, &target)
        .is_some_and(|r| r.found)
        .then_some(StopMechanism::Launchd { target })
}

/// The process launchd runs for the job (`pid =` of `launchctl print`), by identity; `None` when none runs or it cannot be read.
fn process_of(sys: &mut dyn System, target: &str) -> Option<paddock_proc::ProcessId> {
    let pid = job_report(sys, target).and_then(|r| r.pid)?;
    sys.process(pid)
}

/// Whether launchd holds the job (a job that could not be asked about is not found).
fn held(sys: &mut dyn System, target: &str) -> bool {
    job_report(sys, target).is_some_and(|r| r.found)
}

fn gone(sys: &mut dyn System, target: &str) -> bool {
    job_report(sys, target).is_some_and(|r| !r.found)
}

/// The launch agent inside the app bundle (`Contents/Library/LaunchAgents/<label>.plist`, rendered by
/// [`launchd_plist`] with [`LaunchdProgram::Bundled`]), registered through `SMAppService`; the user approves it in
/// Login Items.
pub struct LaunchdBundled {
    spec: ServiceSpec,
    uid: u32,
    app: Box<dyn AppService>,
    sys: Box<dyn System>,
    /// The program this copy registers, and the program the record says the registration was made for (ADR 0056 section 8:
    /// `SMAppService` does not say which copy it holds, so the identity comes from the record).
    own: Option<String>,
    registered: Option<String>,
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
            own: None,
            registered: None,
        }
    }

    /// Which copy the registration is for: its program, whether that is still there, and whether it is this copy's.
    /// Unknown (`None`) when the record names no program (a Dyno 1.0.0 install) or nothing is registered.
    fn identity(
        &mut self,
        registration: &Registration,
    ) -> (Option<String>, Option<bool>, Option<bool>) {
        if matches!(registration, Registration::Absent) {
            return (None, None, None);
        }
        let Some(registered) = self.registered.clone() else {
            return (None, None, None);
        };
        let exists = self.sys.is_program(std::path::Path::new(&registered));
        let here = self.own.as_ref().map(|own| *own == registered);
        (Some(registered), Some(exists), here)
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
        let mark = disabled_mark(self.sys.as_mut(), &domain, self.spec.label());
        let off = mark == DisabledMark::Disabled;
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
        let (job, last_exit, restarts) = look_at(job_report(self.sys.as_mut(), &target));
        let (program, program_exists, points_here) = self.identity(&registration);
        Inspection {
            registration,
            job,
            program,
            program_exists,
            points_here,
            last_exit,
            restarts,
            login_item: Some(LoginItemFacts { status, mark }),
            ..Inspection::absent()
        }
    }

    fn register(&mut self) -> Result<Inspection, RegisterError> {
        self.app.register()?;
        // What the OS holds now is this copy's: the record learns it with the next look; this read-back must not call it
        // another copy's.
        if self.own.is_some() {
            self.registered.clone_from(&self.own);
        }
        let after = self.inspect();
        log::info!(registration:? = after.registration, job:? = after.job; "launchd: bundled agent registered, read back");
        as_written(after, true)
    }

    fn know_copy(&mut self, own: Option<&str>, registered: Option<&str>) {
        self.own = own.map(str::to_string);
        self.registered = registered.map(str::to_string);
    }

    fn unregister(&mut self) -> Result<Inspection, UnregisterError> {
        self.app.unregister().map_err(UnregisterError::Failed)?;
        Ok(self.inspect())
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

    fn job_process(&mut self) -> Option<paddock_proc::ProcessId> {
        let target = self.target();
        process_of(self.sys.as_mut(), &target)
    }

    fn job_found(&mut self) -> bool {
        let target = self.target();
        held(self.sys.as_mut(), &target)
    }

    fn stop_mechanism(&mut self) -> Option<StopMechanism> {
        let target = self.target();
        stop_of(self.sys.as_mut(), target)
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
        let (job, last_exit, restarts) = look_at(job_report(self.sys.as_mut(), &target));
        let text = match self.sys.read(&self.plist) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Inspection {
                    job,
                    last_exit,
                    restarts,
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
                    last_exit,
                    restarts,
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
            last_exit,
            restarts,
            unit: Some(UnitFile {
                path: self.plist.clone(),
                origin: UnitOrigin::Admin,
                enabled: if off {
                    UnitFileState::Disabled
                } else {
                    UnitFileState::Enabled
                },
            }),
            login_item: None,
        }
    }

    fn register(&mut self) -> Result<Inspection, RegisterError> {
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
            Ok(o) if o.success() => {
                let after = self.inspect();
                log::info!(registration:? = after.registration, job:? = after.job; "launchd: plist bootstrapped, read back");
                as_written(after, true)
            }
            Ok(o) if denied(&o.stderr) => Err(RegisterError::NeedsElevation),
            Ok(o) => Err(RegisterError::Other(format!(
                "launchctl bootstrap: {}",
                o.reason()
            ))),
            Err(e) => Err(RegisterError::Other(format!("launchctl bootstrap {e}"))),
        }
    }

    fn unregister(&mut self) -> Result<Inspection, UnregisterError> {
        match self.sys.remove(&self.plist) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(UnregisterError::Failed(format!(
                    "{} could not be removed: {e}",
                    self.plist.display()
                )));
            }
        }
        Ok(self.inspect())
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

    fn job_process(&mut self) -> Option<paddock_proc::ProcessId> {
        let target = self.launchd_target();
        process_of(self.sys.as_mut(), &target)
    }

    fn job_found(&mut self) -> bool {
        let target = self.launchd_target();
        held(self.sys.as_mut(), &target)
    }

    fn stop_mechanism(&mut self) -> Option<StopMechanism> {
        let target = self.launchd_target();
        stop_of(self.sys.as_mut(), target)
    }
}
