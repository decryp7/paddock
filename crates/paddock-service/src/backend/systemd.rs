//! Linux: a systemd user unit (`~/.config/systemd/user`, no administrator rights) or a system unit
//! (`/etc/systemd/system`). Every `systemctl` call has the port's timeout; an unreachable user manager says what to do.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use super::{
    Backend, Capabilities, Inspection, Job, Mechanism, Registration, Target, UnregisterError,
};
use crate::parse::{
    UnitActivity, explain_systemctl, is_switched_off, parse_is_active, systemd_unreachable,
    unit_program,
};
use crate::paths::{SYSTEMCTL, systemctl_scope};
use crate::port::{Output, RegisterError, RunError, System};
use crate::render::systemd_unit;
use crate::spec::{Kind, ServiceSpec};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A systemd unit file.
pub struct Systemd {
    spec: ServiceSpec,
    unit_path: PathBuf,
    target: Target,
    sys: Box<dyn System>,
    packaged: bool,
}

impl Systemd {
    /// The unit `unit_path` (`paths::systemd_unit_path`) starting `target`.
    pub fn new(
        spec: ServiceSpec,
        unit_path: PathBuf,
        target: Target,
        sys: Box<dyn System>,
    ) -> Self {
        Self {
            spec,
            unit_path,
            target,
            sys,
            packaged: false,
        }
    }

    /// The unit is shipped by a package (a `.deb` puts it in `/usr/lib/systemd/system`, disabled): registering only
    /// enables and starts it, unregistering only disables and stops it, and the file is never written or removed
    /// (dpkg owns it).
    #[must_use]
    pub fn packaged(mut self) -> Self {
        self.packaged = true;
        self
    }

    /// The text this copy registers.
    pub fn definition(&self) -> String {
        let wd = self
            .target
            .working_dir
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        systemd_unit(&self.spec, &self.target.exe_text(), wd.as_deref())
    }

    fn ctl(&mut self, args: &[&str]) -> Result<Output, RunError> {
        let mut all: Vec<&str> = systemctl_scope(self.spec.kind()).to_vec();
        all.extend_from_slice(args);
        self.sys.run(SYSTEMCTL, &all)
    }

    fn checked(&mut self, args: &[&str]) -> Result<(), RegisterError> {
        match self.ctl(args) {
            Ok(o) if o.success() => Ok(()),
            Ok(o) if systemd_unreachable(&o.stderr) => {
                Err(RegisterError::Unavailable(explain_systemctl(&o.stderr)))
            }
            Ok(o)
                if o.stderr.contains("Access denied")
                    || o.stderr.contains("Interactive authentication required") =>
            {
                Err(RegisterError::NeedsElevation)
            }
            Ok(o) => Err(RegisterError::Other(format!(
                "systemctl {}: {}",
                args.join(" "),
                explain_systemctl(&o.stderr)
            ))),
            Err(e) => Err(RegisterError::Unavailable(format!(
                "systemctl {e} (is systemd installed and is this a systemd session?)"
            ))),
        }
    }

    fn activity(&mut self) -> Result<UnitActivity, String> {
        let unit = self.spec.unit_file_name();
        match self.ctl(&["is-active", &unit]) {
            // `is-active` exits non-zero for anything but active; its word is on standard output.
            Ok(o) if systemd_unreachable(&o.stderr) => Err(explain_systemctl(&o.stderr)),
            Ok(o) => Ok(parse_is_active(&o.stdout)),
            Err(e) => Err(format!("systemctl {e}")),
        }
    }
}

impl Backend for Systemd {
    fn mechanism(&self) -> Mechanism {
        match self.spec.kind() {
            Kind::UserAgent => Mechanism::SystemdUser,
            Kind::SystemService => Mechanism::SystemdSystem,
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            automatic: self.spec.kind() == Kind::UserAgent,
            pinned_identity: false,
            login_items: false,
        }
    }

    fn inspect(&mut self) -> Inspection {
        let text = match self.sys.read(&self.unit_path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Inspection::absent(),
            Err(e) => {
                return Inspection {
                    registration: Registration::Unavailable(format!(
                        "{} cannot be read: {e}",
                        self.unit_path.display()
                    )),
                    job: Job::Unknown,
                    ..Inspection::absent()
                };
            }
        };
        let job = match self.activity() {
            Err(why) => {
                return Inspection {
                    registration: Registration::Unavailable(why),
                    job: Job::Unknown,
                    ..Inspection::absent()
                };
            }
            Ok(UnitActivity::Active) => Job::Running,
            Ok(UnitActivity::Unknown) => Job::Unknown,
            Ok(_) => Job::NotRunning,
        };
        let unit = self.spec.unit_file_name();
        let off = self
            .ctl(&["is-enabled", &unit])
            .is_ok_and(|o| is_switched_off(&o.stdout));
        let program = unit_program(&text);
        let program_exists = program.as_ref().map(|p| self.sys.is_program(Path::new(p)));
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
            definition_current: (!self.packaged).then(|| text == self.definition()),
        }
    }

    fn register(&mut self) -> Result<(), RegisterError> {
        if self.packaged {
            let unit = self.spec.unit_file_name();
            return self.checked(&["enable", "--now", &unit]);
        }
        let text = self.definition();
        if let Err(e) = self.sys.write(&self.unit_path, &text) {
            return Err(if e.kind() == std::io::ErrorKind::PermissionDenied {
                RegisterError::NeedsElevation
            } else {
                RegisterError::Other(format!(
                    "{} could not be written: {e}",
                    self.unit_path.display()
                ))
            });
        }
        let unit = self.spec.unit_file_name();
        self.checked(&["daemon-reload"])?;
        self.checked(&["enable", "--now", &unit])
    }

    fn unregister(&mut self) -> Result<Inspection, UnregisterError> {
        let unit = self.spec.unit_file_name();
        // `disable --now` is checked (ADR 0054 RC-2): a unit that is already stopped, disabled or unknown is fine, anything
        // else is remembered and judged against what systemd says afterwards.
        let tool_failure = match self.ctl(&["disable", "--now", &unit]) {
            Ok(o) if o.success() || unit_unknown(&o.stderr) => None,
            Ok(o) if systemd_unreachable(&o.stderr) => {
                return Err(UnregisterError::Unavailable(explain_systemctl(&o.stderr)));
            }
            Ok(o) => Some(format!(
                "systemctl disable --now {unit}: {}",
                explain_systemctl(&o.stderr)
            )),
            Err(e) => Some(format!("systemctl disable --now {unit} {e}")),
        };
        if let Some(why) = &tool_failure {
            log::warn!(unit = unit.as_str(), why = why.as_str(), packaged = self.packaged; "systemd: disable --now failed");
        }
        if !self.packaged {
            match self.sys.remove(&self.unit_path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(UnregisterError::Failed(format!(
                        "{} could not be removed: {e}",
                        self.unit_path.display()
                    )));
                }
            }
            self.checked(&["daemon-reload"])
                .map_err(|e| UnregisterError::Failed(e.to_string()))?;
        }
        // The read-back decides: a packaged unit stays on disk but must read as disabled and not running.
        let after = self.inspect();
        log::info!(unit = unit.as_str(), registration:? = after.registration, job:? = after.job; "systemd: unregistered, read back");
        match (after.withdrawn(), tool_failure) {
            (true, _) => Ok(after),
            (false, Some(why)) => Err(UnregisterError::Failed(why)),
            (false, None) => Err(UnregisterError::StillPresent(after)),
        }
    }

    fn evict(&mut self) -> Result<(), String> {
        let unit = self.spec.unit_file_name();
        match self.ctl(&["stop", &unit]) {
            Ok(o) if o.success() || o.stderr.contains("not loaded") => Ok(()),
            Ok(o) => Err(format!("systemctl stop: {}", explain_systemctl(&o.stderr))),
            Err(e) => Err(format!("systemctl stop {e}")),
        }
    }

    fn job_gone(&mut self) -> bool {
        matches!(
            self.activity(),
            Ok(UnitActivity::Inactive | UnitActivity::Failed)
        )
    }

    fn pause(&mut self, how_long: Duration) {
        self.sys.pause(how_long);
    }

    fn definition(&self) -> Option<String> {
        Some(Self::definition(self))
    }
}

/// Whether `systemctl`'s refusal only says the unit is not known (nothing to disable, which is the goal).
fn unit_unknown(stderr: &str) -> bool {
    ["does not exist", "not loaded", "No such file", "not found"]
        .iter()
        .any(|w| stderr.contains(w))
}
