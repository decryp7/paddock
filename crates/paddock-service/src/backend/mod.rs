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

mod launchd;
mod systemd;
mod windows;

pub use launchd::{LaunchdBundled, LaunchdFile};
pub use systemd::Systemd;
pub use windows::{ElevatedWindowsService, WindowsRun, WindowsService};

use crate::error::ServiceError;
use crate::paths::{is_appimage_path, launchd_plist_path, systemd_unit_path};
use crate::port::RegisterError;
use crate::spec::{Kind, ServiceSpec};
use paddock_env::{Os, Vars};
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

impl Inspection {
    /// An inspection of nothing.
    pub fn absent() -> Inspection {
        Inspection {
            registration: Registration::Absent,
            job: Job::Absent,
            program: None,
            program_exists: None,
            points_here: None,
            definition_current: None,
        }
    }

    /// Something is registered or the OS still holds a job.
    pub fn present(&self) -> bool {
        !matches!(self.registration, Registration::Absent) || !matches!(self.job, Job::Absent)
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
    /// Registers (and starts) it.
    fn register(&mut self) -> Result<(), RegisterError>;
    /// Removes the registration.
    fn unregister(&mut self) -> Result<(), String>;
    /// Removes the live job (`launchctl bootout`, `systemctl stop`, `sc stop`, the agent's `stop`).
    fn evict(&mut self) -> Result<(), String>;
    /// Whether the OS no longer holds the job.
    fn job_gone(&mut self) -> bool;
    /// Waits (through the port: the fake only advances its clock).
    fn pause(&mut self, how_long: Duration);
    /// Replaces the registration in one step when the mechanism can only do it that way (one elevation prompt instead
    /// of two); `None` means "use the sequence".
    fn replace_in_one_step(&mut self) -> Option<Result<(), RegisterError>> {
        None
    }
    /// Opens the OS page where the user allows or switches it; false when there is none.
    fn open_login_items(&mut self) -> bool {
        false
    }
    /// The definition this copy registers (plist, unit, `Run` value), when the mechanism has one.
    fn definition(&self) -> Option<String> {
        None
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
}
