//! Windows: the per-user `Run` value ([`WindowsRun`], decision D4's default: the agent runs in the user's session, never
//! session 0) and a Service Control Manager service ([`WindowsService`] inside the elevated `<program> install`,
//! [`ElevatedWindowsService`] from the app through Windows' own prompt). `reg.exe` and `sc.exe` exit with the Win32
//! error and print state numbers that do not depend on the display language, so they are read by number.

use super::{Backend, Capabilities, Inspection, Job, Mechanism, Registration, Target};
use crate::parse::{
    SC_DISABLED, SC_RUNNING, SC_STOPPED, run_value_program, sc_config, sc_exit, sc_privileges,
    sc_state, startup_switched_off,
};
use crate::paths::{REG, SC, WINDOWS_RUN_KEY, WINDOWS_STARTUP_APPROVED_KEY};
use crate::port::{Elevator, RegisterError, System};
use crate::render::{sc_commands, scm_config, windows_command_line};
use crate::spec::ServiceSpec;
use crate::verbs::Verb;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long an elevated `install`/`uninstall` may take (it stops and waits for the old service).
pub const ELEVATED_TIMEOUT: Duration = Duration::from_secs(120);

/// The per-user `Run` value: Windows starts the agent at log-in in the user's session.
pub struct WindowsRun {
    spec: ServiceSpec,
    target: Target,
    sys: Box<dyn System>,
}

impl WindowsRun {
    /// The `Run` value named after the app, starting `target` with the spec's arguments.
    pub fn new(spec: ServiceSpec, target: Target, sys: Box<dyn System>) -> Self {
        Self { spec, target, sys }
    }

    /// The value this copy registers.
    pub fn definition(&self) -> String {
        windows_command_line(&self.target.exe_text(), self.spec.args())
    }

    fn registered_program(&mut self) -> Result<Option<(String, String)>, String> {
        let name = self.spec.name().to_string();
        match self.sys.run(REG, &["query", WINDOWS_RUN_KEY, "/v", &name]) {
            Ok(o) if o.success() => {
                Ok(crate::parse::reg_value(&o.stdout, &name).map(|(_, data)| {
                    (
                        run_value_program(&o.stdout, &name).unwrap_or_default(),
                        data,
                    )
                }))
            }
            Ok(_) => Ok(None),
            Err(e) => Err(format!("reg.exe {e}")),
        }
    }
}

impl Backend for WindowsRun {
    fn mechanism(&self) -> Mechanism {
        Mechanism::WindowsRun
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            automatic: true,
            pinned_identity: false,
            login_items: false,
        }
    }

    fn inspect(&mut self) -> Inspection {
        let (program, data) = match self.registered_program() {
            Ok(Some(found)) => found,
            Ok(None) => return Inspection::absent(),
            Err(why) => {
                return Inspection {
                    registration: Registration::Unavailable(why),
                    job: Job::Unknown,
                    ..Inspection::absent()
                };
            }
        };
        let name = self.spec.name().to_string();
        let off = self
            .sys
            .run(REG, &["query", WINDOWS_STARTUP_APPROVED_KEY, "/v", &name])
            .is_ok_and(|o| o.success() && startup_switched_off(&o.stdout, &name));
        let program = (!program.is_empty()).then_some(program);
        let program_exists = program.as_ref().map(|p| self.sys.is_program(Path::new(p)));
        Inspection {
            registration: if off {
                Registration::SwitchedOff
            } else {
                Registration::Registered
            },
            job: Job::Unknown,
            points_here: program
                .as_ref()
                .map(|p| p.eq_ignore_ascii_case(&self.target.exe_text())),
            program,
            program_exists,
            definition_current: Some(data == self.definition()),
        }
    }

    fn register(&mut self) -> Result<(), RegisterError> {
        let name = self.spec.name().to_string();
        let value = self.definition();
        match self.sys.run(
            REG,
            &[
                "add",
                WINDOWS_RUN_KEY,
                "/v",
                &name,
                "/t",
                "REG_SZ",
                "/d",
                &value,
                "/f",
            ],
        ) {
            Ok(o) if o.success() => {}
            Ok(o) => return Err(RegisterError::Other(format!("reg add: {}", o.reason()))),
            Err(e) => return Err(RegisterError::Other(format!("reg.exe {e}"))),
        }
        // Windows starts it at the next log-in; start it now too.
        let exe = self.target.exe.clone();
        let args = self.spec.args().to_vec();
        self.sys.spawn(&exe, &args).map_err(RegisterError::Other)
    }

    fn unregister(&mut self) -> Result<(), String> {
        let name = self.spec.name().to_string();
        match self
            .sys
            .run(REG, &["delete", WINDOWS_RUN_KEY, "/v", &name, "/f"])
        {
            // Exit 1: the value is not there, which is the goal.
            Ok(o) if o.success() || o.code == Some(1) => Ok(()),
            Ok(o) => Err(format!("reg delete: {}", o.reason())),
            Err(e) => Err(format!("reg.exe {e}")),
        }
    }

    fn evict(&mut self) -> Result<(), String> {
        // The running agent is asked to stop; the registered program may be another copy's.
        let program = match self.registered_program() {
            Ok(Some((p, _))) if !p.is_empty() => PathBuf::from(p),
            _ => self.target.exe.clone(),
        };
        if !self.sys.is_program(&program) {
            return Ok(());
        }
        let exe = program.to_string_lossy().into_owned();
        match self.sys.run(&exe, &[Verb::Stop.word()]) {
            Ok(o) if o.success() => Ok(()),
            Ok(o) => Err(format!("{} stop: {}", program.display(), o.reason())),
            Err(e) => Err(format!("{} stop {e}", program.display())),
        }
    }

    fn job_gone(&mut self) -> bool {
        true
    }

    fn pause(&mut self, how_long: Duration) {
        self.sys.pause(how_long);
    }

    fn definition(&self) -> Option<String> {
        Some(Self::definition(self))
    }
}

/// A Service Control Manager service (LocalSystem, delayed automatic start, restart on failure), used inside the
/// elevated `<program> install` and `uninstall` (and, unelevated, to read the state).
pub struct WindowsService {
    spec: ServiceSpec,
    target: Target,
    sys: Box<dyn System>,
}

fn sc_failure(what: &str, o: &crate::port::Output) -> RegisterError {
    match o.code {
        Some(sc_exit::ACCESS_DENIED) => RegisterError::NeedsElevation,
        Some(sc_exit::MARKED_FOR_DELETE) => RegisterError::Other(format!(
            "{what}: the old service is marked for deletion; close the Services window and try again"
        )),
        _ => RegisterError::Other(format!("{what}: {}", o.reason())),
    }
}

impl WindowsService {
    /// The service named after the app, starting `target --service`.
    pub fn new(spec: ServiceSpec, target: Target, sys: Box<dyn System>) -> Self {
        Self { spec, target, sys }
    }

    fn state(&mut self) -> Result<Option<u32>, String> {
        let name = self.spec.name().to_string();
        match self.sys.run(SC, &["query", &name]) {
            Ok(o) if o.code == Some(sc_exit::DOES_NOT_EXIST) => Ok(None),
            Ok(o) if o.success() => Ok(Some(sc_state(&o.stdout).unwrap_or(0))),
            Ok(o) => Err(format!("sc query: {}", o.reason())),
            Err(e) => Err(format!("sc.exe {e}")),
        }
    }
}

impl WindowsService {
    /// Reads the privileges the SCM now keeps (`sc qprivs`) and refuses to start a service that holds any other than the
    /// declared ones (ADR 0027: `SeChangeNotifyPrivilege` only); nothing is checked when the spec declares none.
    fn check_privileges(&mut self, name: &str, declared: &[String]) -> Result<(), RegisterError> {
        if declared.is_empty() {
            return Ok(());
        }
        let out = match self.sys.run(SC, &["qprivs", name]) {
            Ok(o) if o.success() => o,
            Ok(o) => return Err(sc_failure("sc qprivs", &o)),
            Err(e) => return Err(RegisterError::Other(format!("sc.exe {e}"))),
        };
        let mut want = declared.to_vec();
        want.sort();
        want.dedup();
        let held = sc_privileges(&out.stdout);
        if held == want {
            return Ok(());
        }
        Err(RegisterError::Other(format!(
            "the service {name} holds the privileges [{}] and must hold exactly [{}]; it was not started (remove it and install again)",
            held.join(", "),
            want.join(", ")
        )))
    }
}

/// The first few frames of the call stack that led here, for the log line before a service is deleted.
fn caller_frames() -> String {
    let text = std::backtrace::Backtrace::force_capture().to_string();
    let frames: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| {
            l.contains("::")
                && !l.starts_with("at ")
                && !l.contains("caller_frames")
                && !l.contains("std::backtrace")
        })
        .take(8)
        .collect();
    frames.join(" < ")
}

impl Backend for WindowsService {
    fn mechanism(&self) -> Mechanism {
        Mechanism::WindowsService
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            automatic: true,
            pinned_identity: false,
            login_items: false,
        }
    }

    fn inspect(&mut self) -> Inspection {
        let state = match self.state() {
            Ok(None) => return Inspection::absent(),
            Ok(Some(s)) => s,
            Err(why) => {
                return Inspection {
                    registration: Registration::Unavailable(why),
                    job: Job::Unknown,
                    ..Inspection::absent()
                };
            }
        };
        let name = self.spec.name().to_string();
        let (program, start) = match self.sys.run(SC, &["qc", &name]) {
            Ok(o) if o.success() => sc_config(&o.stdout),
            _ => (None, None),
        };
        let program_exists = program.as_ref().map(|p| self.sys.is_program(Path::new(p)));
        let points_here = program
            .as_ref()
            .map(|p| p.eq_ignore_ascii_case(&self.target.exe_text()));
        Inspection {
            registration: if start == Some(SC_DISABLED) {
                Registration::SwitchedOff
            } else {
                Registration::Registered
            },
            job: match state {
                SC_RUNNING => Job::Running,
                0 => Job::Unknown,
                _ => Job::NotRunning,
            },
            program,
            program_exists,
            points_here,
            definition_current: points_here,
        }
    }

    fn register(&mut self) -> Result<(), RegisterError> {
        let config = scm_config(&self.spec, &self.target.exe_text());
        for call in sc_commands(&config) {
            let args: Vec<&str> = call.iter().map(String::as_str).collect();
            match self.sys.run(SC, &args) {
                Ok(o) if o.success() => {}
                Ok(o) => return Err(sc_failure(&format!("sc {}", args[0]), &o)),
                Err(e) => return Err(RegisterError::Other(format!("sc.exe {e}"))),
            }
        }
        self.check_privileges(&config.name, &config.privileges)?;
        match self.sys.run(SC, &["start", &config.name]) {
            // 1056: already running.
            Ok(o) if o.success() || o.code == Some(1056) => Ok(()),
            Ok(o) => Err(sc_failure("sc start", &o)),
            Err(e) => Err(RegisterError::Other(format!("sc.exe {e}"))),
        }
    }

    fn unregister(&mut self) -> Result<(), String> {
        let name = self.spec.name().to_string();
        // A deleted service cannot be started again, and one deleted while a handle is open only goes at the next restart: say
        // who asked, before it happens (ER-27).
        log::warn!(service = name.as_str(), caller = caller_frames().as_str(); "deleting the Windows service (sc stop, sc delete)");
        let _ = self.sys.run(SC, &["stop", &name]);
        match self.sys.run(SC, &["delete", &name]) {
            Ok(o)
                if o.success()
                    || o.code == Some(sc_exit::DOES_NOT_EXIST)
                    || o.code == Some(sc_exit::MARKED_FOR_DELETE) =>
            {
                Ok(())
            }
            Ok(o) => Err(sc_failure("sc delete", &o).to_string()),
            Err(e) => Err(format!("sc.exe {e}")),
        }
    }

    fn evict(&mut self) -> Result<(), String> {
        let name = self.spec.name().to_string();
        match self.sys.run(SC, &["stop", &name]) {
            Ok(o)
                if o.success()
                    || o.code == Some(sc_exit::NOT_ACTIVE)
                    || o.code == Some(sc_exit::DOES_NOT_EXIST) =>
            {
                Ok(())
            }
            Ok(o) => Err(format!("sc stop: {}", o.reason())),
            Err(e) => Err(format!("sc.exe {e}")),
        }
    }

    fn job_gone(&mut self) -> bool {
        matches!(self.state(), Ok(None | Some(SC_STOPPED)))
    }

    fn job_process(&mut self) -> Option<paddock_proc::ProcessId> {
        let name = self.spec.name().to_string();
        match self.sys.run(SC, &["queryex", &name]) {
            Ok(o) if o.success() => {
                crate::parse::sc_pid(&o.stdout).and_then(paddock_proc::ProcessId::of)
            }
            _ => None,
        }
    }

    fn pause(&mut self, how_long: Duration) {
        self.sys.pause(how_long);
    }
}

/// The Windows service as the app sees it: read unelevated, changed by running `<program> install` or `uninstall`
/// through Windows' own prompt (UAC). It never repairs by itself: a prompt only appears when the user asks.
pub struct ElevatedWindowsService {
    inner: WindowsService,
    program: PathBuf,
    elevator: Box<dyn Elevator>,
}

impl ElevatedWindowsService {
    /// The service of `spec` whose program is `program` (next to the app), changed through `elevator`.
    pub fn new(
        spec: ServiceSpec,
        program: PathBuf,
        sys: Box<dyn System>,
        elevator: Box<dyn Elevator>,
    ) -> Self {
        Self {
            inner: WindowsService::new(spec, Target::exe(program.clone()), sys),
            program,
            elevator,
        }
    }

    fn elevated(&mut self, verb: Verb) -> Result<(), RegisterError> {
        match self
            .elevator
            .run_elevated(&self.program, &[verb.word()], ELEVATED_TIMEOUT)?
        {
            0 => Ok(()),
            code => Err(RegisterError::Other(format!(
                "{} {} failed (exit code {code}); the service log has details",
                self.program.display(),
                verb.word()
            ))),
        }
    }
}

impl Backend for ElevatedWindowsService {
    fn mechanism(&self) -> Mechanism {
        Mechanism::WindowsService
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            automatic: false,
            pinned_identity: false,
            login_items: false,
        }
    }

    fn inspect(&mut self) -> Inspection {
        self.inner.inspect()
    }

    fn register(&mut self) -> Result<(), RegisterError> {
        self.elevated(Verb::Install)
    }

    fn unregister(&mut self) -> Result<(), String> {
        self.elevated(Verb::Uninstall).map_err(|e| e.to_string())
    }

    fn evict(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn job_gone(&mut self) -> bool {
        self.inner.job_gone()
    }

    fn pause(&mut self, how_long: Duration) {
        self.inner.pause(how_long);
    }

    /// `<program> install` replaces an existing service itself (it runs the whole sequence elevated), so one prompt.
    fn replace_in_one_step(&mut self) -> Option<Result<(), RegisterError>> {
        Some(self.elevated(Verb::Install))
    }
}

#[cfg(test)]
mod tests {
    use super::caller_frames;

    #[test]
    fn the_line_before_a_service_is_deleted_names_who_asked() {
        let frames = caller_frames();
        assert!(
            frames.contains("the_line_before_a_service_is_deleted_names_who_asked"),
            "{frames}"
        );
        assert!(!frames.contains("caller_frames"), "{frames}");
    }
}
