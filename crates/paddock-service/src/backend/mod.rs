//! One [`Backend`] per registration mechanism, all over the [`System`](crate::port::System) port, so the repair
//! sequence and the plan are the same everywhere and every backend is tested with the fake:
//!
//! | Mechanism | Backend | Used for |
//! |---|---|---|
//! | `SMAppService` agent from the bundle | [`LaunchdBundled`] | the macOS app (Pitwall, Pitcrew, Dyno) |
//! | plist file + `launchctl bootstrap` | [`LaunchdFile`] | development builds, launch daemons (Debrief's agent) |
//! | systemd user or system unit | [`Systemd`] | Linux |
//! | `HKCU\…\Run` value | [`WindowsRun`] | per-user agents on Windows (Dyno) |
//! | Service Control Manager | [`WindowsService`] | inside the elevated `<program> install` |
//! | the same through Windows' prompt | [`ElevatedWindowsService`] | the app's Install and Repair buttons |
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

mod launchd;
mod systemd;
mod windows;

pub use launchd::{LaunchdBundled, LaunchdFile};
pub use systemd::Systemd;
pub use windows::{ElevatedWindowsService, WindowsRun, WindowsService};

use crate::error::ServiceError;
use crate::paths::{is_appimage_path, launchd_plist_path, systemd_unit_path};
use crate::port::{AppServiceStatus, RegisterError};
use crate::spec::{Kind, ServiceSpec};
use paddock_env::{Os, Vars};
use paddock_proc::RunState;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How the background process is registered.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mechanism {
    /// A launch agent or daemon inside the app bundle, registered through `SMAppService`.
    LaunchdBundled,
    /// A launchd plist file loaded with `launchctl bootstrap`.
    LaunchdFile,
    /// A systemd user unit.
    SystemdUser,
    /// A systemd system unit.
    SystemdSystem,
    /// The per-user `Run` value.
    WindowsRun,
    /// A Windows service.
    WindowsService,
}

impl Mechanism {
    /// Words for messages.
    pub fn describe(self) -> &'static str {
        match self {
            Mechanism::LaunchdBundled | Mechanism::LaunchdFile => "launch agent",
            Mechanism::SystemdUser => "systemd user service",
            Mechanism::SystemdSystem => "systemd service",
            Mechanism::WindowsRun => "start-up entry",
            Mechanism::WindowsService => "Windows service",
        }
    }
}

/// What the OS holds as the registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Registration {
    /// Nothing is registered.
    Absent,
    /// Registered and allowed.
    Registered,
    /// Registered, waiting for the user's approval (macOS Login Items).
    NeedsApproval,
    /// The user switched it off (Login Items, Task Manager > Startup, Services, `systemctl disable`).
    SwitchedOff,
    /// It cannot be registered from here; the text says why and what to do.
    Unavailable(String),
}

/// What the OS says about the running job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Job {
    /// The OS has no job.
    Absent,
    /// It runs.
    Running,
    /// Known but not running.
    NotRunning,
    /// The OS refuses to start what is registered (macOS: EX_CONFIG, a stale launch constraint).
    Refused,
    /// The OS does not say (a `Run` entry; a helper that did not answer).
    Unknown,
    /// Stopped with an error: the OS keeps how the last process ended (ADR 0054 section 1.1).
    Failed(LastExit),
}

impl Job {
    /// What the OS said, in words for a failure message (what it said, never a guessed reason).
    pub fn describe(self) -> String {
        match self {
            Job::Absent => "the OS does not know the job".into(),
            Job::Running => "running".into(),
            Job::NotRunning => "not running".into(),
            Job::Refused => "not running, the OS refuses to start it (spawn failed, exit code 78 or a stale launch constraint)".into(),
            Job::Unknown => "the OS did not say".into(),
            Job::Failed(exit) => format!("not running, it stopped with {exit}"),
        }
    }
}

/// How the job's last process ended, as the OS reports it (typed, never free text).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LastExit {
    /// An exit status (systemd `ExecMainStatus`, launchd `last exit code`).
    Code(i32),
    /// A signal ended it (launchd `last terminating signal`, systemd `ExecMainStatus` with a signal code).
    Signal(i32),
    /// A Win32 error code (`SERVICE_STATUS_PROCESS.dwWin32ExitCode`).
    Win32(u32),
    /// The service's own code (`dwServiceSpecificExitCode`, reported with Win32 error 1066).
    ServiceSpecific(u32),
}

impl LastExit {
    /// Whether the process did not end normally (a non-zero code, a signal other than the ones that ask a service to stop).
    pub fn is_failure(self) -> bool {
        match self {
            LastExit::Code(c) => c != 0,
            // The signals a service is asked to stop with (SIGHUP, SIGINT, SIGPIPE, SIGTERM) are a clean stop.
            LastExit::Signal(s) => !matches!(s, 1 | 2 | 13 | 15),
            LastExit::Win32(c) | LastExit::ServiceSpecific(c) => c != 0,
        }
    }
}

impl std::fmt::Display for LastExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LastExit::Code(c) => write!(f, "exit code {c}"),
            LastExit::Signal(s) => write!(f, "signal {s}"),
            LastExit::Win32(c) => write!(f, "Win32 error {c}"),
            LastExit::ServiceSpecific(c) => write!(f, "service-specific exit code {c}"),
        }
    }
}

/// Where the definition in force comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitOrigin {
    /// The package (`/usr/lib/systemd/system`).
    Packaged,
    /// The administrator or the user (`/etc/systemd/system`, `~/.config/systemd/user`, `~/Library/LaunchAgents`).
    Admin,
    /// Transient (`/run/systemd`).
    Runtime,
    /// The app bundle.
    Bundle,
    /// The Service Control Manager.
    ServiceManager,
}

/// Whether the OS starts the unit by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitFileState {
    /// Started at boot or log-in.
    Enabled,
    /// Present, not started by itself.
    Disabled,
    /// Cannot be enabled; started by something else (systemd `static`, `indirect`), or by hand (SCM demand start).
    Manual,
    /// Switched off so that nothing can start it.
    Masked,
    /// The OS said something this crate does not model.
    Other,
}

/// Which definition is in force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitFile {
    /// The file (systemd `FragmentPath`, the plist) or the program the SCM entry starts.
    pub path: PathBuf,
    /// Where it comes from.
    pub origin: UnitOrigin,
    /// Whether the OS starts it by itself.
    pub enabled: UnitFileState,
}

impl UnitOrigin {
    /// The origin of a systemd unit file by its folder.
    pub fn of_systemd_path(path: &Path) -> UnitOrigin {
        let text = path.to_string_lossy();
        if text.starts_with("/usr/lib/") || text.starts_with("/lib/") {
            UnitOrigin::Packaged
        } else if text.starts_with("/run/") {
            UnitOrigin::Runtime
        } else {
            UnitOrigin::Admin
        }
    }
}

/// What `launchctl print-disabled` said about the item. "Could not read" is not "not listed": a slow or failed look at a
/// switched-off item must not read as switched on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisabledMark {
    /// Listed as disabled (the user's switch is off).
    Disabled,
    /// Listed as enabled.
    Enabled,
    /// The list was read and the item is not in it.
    NotListed,
    /// The list could not be read (the tool failed or timed out).
    Unreadable,
}

impl DisabledMark {
    /// The view the plan takes: `None` for both "not listed" and "unreadable" (a repair after a failed look is the existing
    /// behaviour; only a resume, which would switch the item on, is stricter).
    pub fn as_option(self) -> Option<bool> {
        match self {
            DisabledMark::Disabled => Some(true),
            DisabledMark::Enabled => Some(false),
            DisabledMark::NotListed | DisabledMark::Unreadable => None,
        }
    }
}

/// The two raw readings behind a macOS `SMAppService` registration: its status and launchd's override database. Decisions
/// that must tell "switched off", "never registered" and "could not be read" apart (a resume after a stop) read these;
/// [`Registration`] is their summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginItemFacts {
    /// `SMAppService.status`.
    pub status: AppServiceStatus,
    /// `launchctl print-disabled`.
    pub mark: DisabledMark,
}

impl LoginItemFacts {
    /// Whether the user turned the item off in System Settings > Login Items: `RequiresApproval` is how macOS reports an
    /// item the user switched off (or never allowed); `NotRegistered` alone is also a fresh install, so it only counts when
    /// launchd has the label marked disabled; `Enabled` with launchd saying disabled is the two views disagreeing and the
    /// user's switch wins; `NotFound` is not a switch.
    pub fn switched_off(&self) -> bool {
        match self.status {
            AppServiceStatus::RequiresApproval => true,
            AppServiceStatus::NotRegistered | AppServiceStatus::Enabled => {
                self.mark.as_option() == Some(true)
            }
            AppServiceStatus::NotFound => false,
        }
    }
}

/// One look at the registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inspection {
    /// The registration.
    pub registration: Registration,
    /// The job.
    pub job: Job,
    /// The executable the registration starts, when the OS says (not for `SMAppService`).
    pub program: Option<String>,
    /// Whether that executable is still there (`None`: not known).
    pub program_exists: Option<bool>,
    /// Whether it is this copy's executable (`None`: not known).
    pub points_here: Option<bool>,
    /// Whether the registered definition equals what this copy would register (`None`: not known).
    pub definition_current: Option<bool>,
    /// How the job's last process ended (`SERVICE_STATUS_PROCESS`, systemd `ExecMainStatus`, launchd `last exit code`).
    pub last_exit: Option<LastExit>,
    /// How often the OS restarted the job after it ended (systemd `NRestarts`, launchd `runs` minus one); `None` on
    /// Windows, where the event log says (ADR 0054 section 1.3), and when the OS does not say.
    pub restarts: Option<u32>,
    /// Which definition is in force (`None`: the OS does not say).
    pub unit: Option<UnitFile>,
    /// The raw macOS readings behind `registration` (`None` for the other mechanisms).
    pub login_item: Option<LoginItemFacts>,
}

impl Inspection {
    /// The registration is gone or switched off and the OS no longer runs the job: what a removal has to leave. A packaged
    /// unit stays on disk, so "switched off" counts; a job that is still running or that the OS keeps refusing does not.
    pub fn withdrawn(&self) -> bool {
        matches!(
            self.registration,
            Registration::Absent | Registration::SwitchedOff
        ) && !matches!(self.job, Job::Running | Job::Refused)
    }

    /// An inspection of nothing.
    pub fn absent() -> Inspection {
        Inspection {
            registration: Registration::Absent,
            job: Job::Absent,
            program: None,
            program_exists: None,
            points_here: None,
            definition_current: None,
            last_exit: None,
            restarts: None,
            unit: None,
            login_item: None,
        }
    }

    /// Something is registered or the OS still holds a job.
    pub fn present(&self) -> bool {
        !matches!(self.registration, Registration::Absent) || !matches!(self.job, Job::Absent)
    }
}

/// Why removing a registration failed (ADR 0054 section 1.1). A removal whose read-back disagrees is never `Ok`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnregisterError {
    /// Administrator rights are needed.
    NeedsElevation,
    /// It cannot be reached from here (no systemd manager); the text says what to do.
    Unavailable(String),
    /// The tool or the file operation failed; the text is the tool's own words.
    Failed(String),
    /// The call reported success, but looking again shows the registration or the job is still there.
    StillPresent(Inspection),
}

impl std::fmt::Display for UnregisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UnregisterError::NeedsElevation => f.write_str("administrator rights are needed"),
            UnregisterError::Unavailable(why) | UnregisterError::Failed(why) => f.write_str(why),
            UnregisterError::StillPresent(look) => write!(
                f,
                "the OS still holds the registration or the job after it was removed (registration {:?}, job {:?})",
                look.registration, look.job
            ),
        }
    }
}

impl std::error::Error for UnregisterError {}

/// What a backend sees of a starting job on one poll (see [`Backend::observe_start`]): the facts a
/// [`StartWatch`](crate::StartWatch) judges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartFacts {
    /// The job. A job that is not running is `NotRunning` here, also when it stopped with an error: how it ended is
    /// `last_exit`.
    pub job: Job,
    /// The job's process while it runs, by identity (pid and start time, rule 53) so a restarted job is told from the last
    /// one even when the pid is handed on; `None` when it does not run or the OS did not name it.
    pub pid: Option<paddock_proc::ProcessId>,
    /// How often the OS has started the job.
    pub runs: Option<u32>,
    /// The exit code the job last ended with (`None`: never, a signal ended it, or not known).
    pub last_exit: Option<i32>,
    /// The job process's CPU time so far, when it could be read.
    pub cpu: Option<Duration>,
    /// Whether the OS has stopped the job's process ([`RunState::Stopped`]: App Tamer, a signal); `Unknown` when the OS
    /// does not say (Windows, no pid, a process of another user).
    pub run_state: RunState,
}

impl StartFacts {
    /// Nothing known: the OS did not say.
    pub fn unknown() -> StartFacts {
        StartFacts {
            job: Job::Unknown,
            pid: None,
            runs: None,
            last_exit: None,
            cpu: None,
            run_state: RunState::Unknown,
        }
    }
}

/// What a backend allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Repairs may run without the user asking (no OS prompt involved).
    pub automatic: bool,
    /// The OS pins the registered build's code identity, so every update needs a fresh registration (macOS, rule 24).
    pub pinned_identity: bool,
    /// The user approves it in Login Items (macOS `SMAppService`).
    pub login_items: bool,
}

/// One registration mechanism. Every method makes bounded OS calls through the [`System`](crate::port::System) port
/// and returns; none panics. Failures of `unregister` and `evict` are reported and the sequence goes on (the job may
/// already be gone).
pub trait Backend: Send {
    /// Which mechanism.
    fn mechanism(&self) -> Mechanism;
    /// What it allows.
    fn capabilities(&self) -> Capabilities;
    /// Looks at the registration and the job.
    fn inspect(&mut self) -> Inspection;
    /// Registers (and starts) it and returns what the OS holds afterwards (the post-write inspection).
    ///
    /// # Errors
    /// [`RegisterError`]; a write whose read-back disagrees is [`RegisterError::NotAsWritten`], never `Ok`.
    fn register(&mut self) -> Result<Inspection, RegisterError>;
    /// Removes the registration and returns what the OS holds afterwards (the post-inspection).
    ///
    /// # Errors
    /// [`UnregisterError`]; a removal whose read-back disagrees is [`UnregisterError::StillPresent`], never `Ok`.
    fn unregister(&mut self) -> Result<Inspection, UnregisterError>;
    /// Removes the live job (`launchctl bootout`, `systemctl stop`, `sc stop`, the agent's `stop`).
    fn evict(&mut self) -> Result<(), String>;
    /// Whether the OS no longer holds the job.
    fn job_gone(&mut self) -> bool;
    /// The process of the running job (pid and start time, read now), so a stop can wait for that process to end: the OS may
    /// drop the job's entry while the process still holds its program open (Windows: the program cannot then be deleted).
    fn job_process(&mut self) -> Option<paddock_proc::ProcessId> {
        None
    }
    /// Whether `process` still runs (by identity, never by pid alone: rule 53).
    fn process_alive(&mut self, process: paddock_proc::ProcessId) -> bool {
        process.is_alive()
    }
    /// Waits (through the port: the fake only advances its clock).
    fn pause(&mut self, how_long: Duration);
    /// Replaces the registration in one step when the mechanism can only do it that way (one elevation prompt instead
    /// of two); `None` means "use the sequence".
    fn replace_in_one_step(&mut self) -> Option<Result<Inspection, RegisterError>> {
        None
    }
    /// Opens the OS page where the user allows or switches it; false when there is none.
    fn open_login_items(&mut self) -> bool {
        false
    }
    /// One cheap look at a service that is starting, for a [`StartWatch`](crate::StartWatch): the job's state, pid, run
    /// count, last exit and CPU time. `None` (the default) when the mechanism cannot say more than [`inspect`](Self::inspect)
    /// does (systemd, Windows): its repair then keeps the fixed wait of [`Timings::start_wait`](crate::Timings).
    fn observe_start(&mut self) -> Option<StartFacts> {
        None
    }
    /// The definition this copy registers (plist, unit, `Run` value), when the mechanism has one.
    fn definition(&self) -> Option<String> {
        None
    }
    /// Tells the backend which program this copy would register and which the record says was registered, before each look
    /// (ADR 0056 section 8: the identity of a copy). A mechanism that cannot read the registered program from the OS
    /// (`SMAppService`) compares the two itself; the others ignore it.
    fn know_copy(&mut self, _own: Option<&str>, _registered: Option<&str>) {}
    /// Whether the OS holds the job now. A job the OS could not be asked about is not found (a wait for the job must not
    /// end on a failed look). The default reads a full [`inspect`](Backend::inspect); a backend with a cheaper look overrides it.
    fn job_found(&mut self) -> bool {
        !matches!(self.inspect().job, Job::Absent | Job::Unknown)
    }
    /// The job a stop can name while the OS holds it (launchd job, systemd user unit); `None` where there is none to stop.
    /// A stop keeps the registration ([`crate::stop`]).
    fn stop_mechanism(&mut self) -> Option<crate::stop::StopMechanism> {
        None
    }
}

/// Judges what the OS holds after a registration was written (ADR 0054 section 1.1): the post-write inspection is returned
/// when it shows the registration as written, [`RegisterError::NotAsWritten`] otherwise. `user_switch_is_not_ours` is true
/// for mechanisms where the user's own switch (Task Manager > Startup, Login Items) legitimately reads back "switched off".
pub(crate) fn as_written(
    after: Inspection,
    user_switch_is_not_ours: bool,
) -> Result<Inspection, RegisterError> {
    match &after.registration {
        Registration::NeedsApproval => return Err(RegisterError::NeedsApproval),
        Registration::Absent | Registration::Unavailable(_) => {
            return Err(RegisterError::NotAsWritten(after));
        }
        Registration::SwitchedOff if !user_switch_is_not_ours => {
            return Err(RegisterError::NotAsWritten(after));
        }
        Registration::Registered | Registration::SwitchedOff => {}
    }
    if after.definition_current == Some(false) || after.points_here == Some(false) {
        return Err(RegisterError::NotAsWritten(after));
    }
    Ok(after)
}

/// The job as the OS reports it: stopped with a failing last exit is [`Job::Failed`], anything else keeps `job`.
pub(crate) fn with_failure(job: Job, last_exit: Option<LastExit>) -> Job {
    match (job, last_exit) {
        (Job::NotRunning, Some(exit)) if exit.is_failure() => Job::Failed(exit),
        _ => job,
    }
}

/// The program a file-based registration points at: this copy's executable (after any AppImage stable copy), its
/// working folder and where the OS writes start-up output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The executable.
    pub exe: PathBuf,
    /// The working folder.
    pub working_dir: Option<PathBuf>,
    /// The launchd output file.
    pub log: Option<PathBuf>,
}

impl Target {
    /// Just the executable.
    pub fn exe(exe: impl Into<PathBuf>) -> Target {
        Target {
            exe: exe.into(),
            working_dir: None,
            log: None,
        }
    }

    /// With a working folder.
    #[must_use]
    pub fn working_dir(mut self, dir: impl Into<PathBuf>) -> Target {
        self.working_dir = Some(dir.into());
        self
    }

    /// With a launchd output file.
    #[must_use]
    pub fn log(mut self, file: impl Into<PathBuf>) -> Target {
        self.log = Some(file.into());
        self
    }

    pub(crate) fn exe_text(&self) -> String {
        self.exe.to_string_lossy().into_owned()
    }
}

/// Who registers: the app (its Install and Repair buttons) or the background program itself (its `install` verb, run by
/// an installer or an administrator, already elevated where that is needed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The app.
    App,
    /// The background program.
    Program,
}

/// Which backend to build for a target OS, and with what (pure; `native::host_backend` builds it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// [`LaunchdBundled`] (the program is inside an app bundle).
    Bundled,
    /// [`LaunchdFile`].
    LaunchdFile {
        /// The plist file.
        plist: PathBuf,
        /// What it starts.
        target: Target,
    },
    /// [`Systemd`].
    Systemd {
        /// The unit file.
        unit: PathBuf,
        /// What it starts.
        target: Target,
        /// Copy this AppImage program to the data folder first and start the copy.
        copy_from: Option<PathBuf>,
    },
    /// [`WindowsRun`].
    WindowsRun {
        /// What it starts.
        target: Target,
    },
    /// [`WindowsService`], or [`ElevatedWindowsService`] when `elevated`.
    WindowsService {
        /// Changed through Windows' prompt (the app's side).
        elevated: bool,
        /// What it starts.
        target: Target,
    },
}

/// Whether `program` is inside a macOS app bundle (`X.app/Contents/MacOS/program`).
pub fn in_app_bundle(program: &Path) -> bool {
    let parts: Vec<String> = program
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts
        .windows(3)
        .any(|w| w[0].ends_with(".app") && w[1] == "Contents" && w[2] == "MacOS")
}

/// Chooses the mechanism for `spec` on `os`: the bundled agent when the program is in an app bundle (a plist file
/// otherwise, for development builds and daemons), a systemd unit (an AppImage's program copied first), the `Run`
/// value for a per-user agent on Windows, a service otherwise (through Windows' prompt from the app).
pub fn choose(
    os: Os,
    spec: &ServiceSpec,
    program: &Path,
    side: Side,
    appimage: Option<&str>,
    vars: &dyn Vars,
) -> Result<Choice, ServiceError> {
    let target = Target::exe(program);
    Ok(match os {
        Os::MacOs if in_app_bundle(program) && side == Side::App => Choice::Bundled,
        Os::MacOs => Choice::LaunchdFile {
            plist: launchd_plist_path(spec, vars)?,
            target,
        },
        Os::Linux => Choice::Systemd {
            unit: systemd_unit_path(spec, vars)?,
            copy_from: is_appimage_path(program, appimage).then(|| program.to_path_buf()),
            target,
        },
        Os::Windows => match spec.kind() {
            Kind::UserAgent => Choice::WindowsRun { target },
            Kind::SystemService => Choice::WindowsService {
                elevated: side == Side::App,
                target,
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mechanisms_have_words_and_an_inspection_knows_presence() {
        for m in [
            Mechanism::LaunchdBundled,
            Mechanism::LaunchdFile,
            Mechanism::SystemdUser,
            Mechanism::SystemdSystem,
            Mechanism::WindowsRun,
            Mechanism::WindowsService,
        ] {
            assert!(!m.describe().is_empty());
        }
        let mut i = Inspection::absent();
        assert!(!i.present());
        i.job = Job::Refused;
        assert!(i.present(), "a job launchd still holds counts");
        let mut i = Inspection::absent();
        i.registration = Registration::SwitchedOff;
        assert!(i.present());
        let t = Target::exe("/a").working_dir("/w").log("/l");
        assert_eq!(
            (t.exe_text(), t.working_dir.unwrap(), t.log.unwrap()),
            ("/a".into(), "/w".into(), "/l".into())
        );
    }

    #[test]
    fn the_mechanism_follows_the_os_the_kind_and_where_the_program_is() {
        use paddock_env::MapVars;
        let agent = ServiceSpec::builder("net.decryptology.dyno.agent", "Dyno", "dyno-agent")
            .build()
            .unwrap();
        let svc = ServiceSpec::builder(
            "net.decryptology.pitwall.service",
            "Pitwall",
            "pitwall-server",
        )
        .kind(Kind::SystemService)
        .build()
        .unwrap();
        let vars = MapVars::new().with("HOME", "/home/ana");
        let bundled = Path::new("/Applications/Dyno.app/Contents/MacOS/dyno-agent");
        assert!(in_app_bundle(bundled) && !in_app_bundle(Path::new("/usr/local/bin/dyno-agent")));
        assert_eq!(
            choose(Os::MacOs, &agent, bundled, Side::App, None, &vars).unwrap(),
            Choice::Bundled
        );
        let Choice::LaunchdFile { plist, target } = choose(
            Os::MacOs,
            &agent,
            Path::new("/dev/target/dyno-agent"),
            Side::App,
            None,
            &vars,
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(
            plist,
            PathBuf::from("/home/ana/Library/LaunchAgents/net.decryptology.dyno.agent.plist")
        );
        assert_eq!(target.exe, PathBuf::from("/dev/target/dyno-agent"));
        assert!(matches!(
            choose(Os::MacOs, &agent, bundled, Side::Program, None, &vars).unwrap(),
            Choice::LaunchdFile { .. }
        ));
        let Choice::Systemd {
            unit, copy_from, ..
        } = choose(
            Os::Linux,
            &agent,
            Path::new("/tmp/.mount_Dyno/usr/bin/dyno-agent"),
            Side::App,
            None,
            &vars,
        )
        .unwrap()
        else {
            panic!()
        };
        assert_eq!(
            unit,
            PathBuf::from("/home/ana/.config/systemd/user/dyno.service")
        );
        assert!(copy_from.is_some());
        assert!(matches!(
            choose(
                Os::Linux,
                &agent,
                Path::new("/usr/bin/dyno-agent"),
                Side::App,
                None,
                &vars
            )
            .unwrap(),
            Choice::Systemd {
                copy_from: None,
                ..
            }
        ));
        assert!(matches!(
            choose(
                Os::Windows,
                &agent,
                Path::new("C:\\D\\dyno-agent.exe"),
                Side::App,
                None,
                &vars
            )
            .unwrap(),
            Choice::WindowsRun { .. }
        ));
        assert!(matches!(
            choose(
                Os::Windows,
                &svc,
                Path::new("C:\\P\\pitwall-server.exe"),
                Side::App,
                None,
                &vars
            )
            .unwrap(),
            Choice::WindowsService { elevated: true, .. }
        ));
        assert!(matches!(
            choose(
                Os::Windows,
                &svc,
                Path::new("C:\\P\\pitwall-server.exe"),
                Side::Program,
                None,
                &vars
            )
            .unwrap(),
            Choice::WindowsService {
                elevated: false,
                ..
            }
        ));
        assert!(
            choose(
                Os::Linux,
                &agent,
                Path::new("/x"),
                Side::App,
                None,
                &MapVars::new()
            )
            .is_err()
        );
    }

    #[test]
    fn every_job_state_is_described_in_the_oss_words() {
        let all = [
            (Job::Absent, "does not know the job"),
            (Job::Running, "running"),
            (Job::NotRunning, "not running"),
            (Job::Refused, "refuses to start it"),
            (Job::Unknown, "did not say"),
            (Job::Failed(LastExit::Code(78)), "stopped with exit code 78"),
            (Job::Failed(LastExit::Signal(9)), "stopped with signal 9"),
            (
                Job::Failed(LastExit::Win32(1053)),
                "stopped with Win32 error 1053",
            ),
            (
                Job::Failed(LastExit::ServiceSpecific(7)),
                "stopped with service-specific exit code 7",
            ),
        ];
        for (job, words) in all {
            let text = job.describe();
            assert!(text.contains(words), "{job:?}: {text}");
        }
    }
}
