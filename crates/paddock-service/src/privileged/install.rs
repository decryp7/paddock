//! The install, update and removal of a privileged service as a pure plan ([`plan_install`], [`plan_uninstall`]) and the
//! [`Installer`] that runs it over [`ProtectedFiles`], a [`Backend`] and the OS ([`System`], for `icacls`). The install is
//! idempotent: run again it takes over an older registration (a 0.x helper under the same label, the previous version)
//! and is therefore also the update. Every phase is logged with its duration and ends in the [`Report`] (ER-27).

use super::files::{FileKind, ProtectedFiles, Violation, verify_protected};
use super::layout::{CopyTarget, Layout};
use super::windows_acl::{ICACLS, acl_command, acl_shape_ok};
use crate::backend::{Backend, Job, Registration};
use crate::port::{RegisterError, System};
use paddock_env::Os;
use std::path::Path;
use std::time::{Duration, Instant};

/// One thing the installer does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Stop and unregister what is registered, and wait until the OS has dropped the job.
    StopExisting,
    /// The folder of the protected copy exists and is protected (created when missing).
    EnsureFolder,
    /// Windows: replace the folder's ACL.
    ApplyAcl,
    /// Copy the program into the protected folder and hash it.
    CopyProgram,
    /// The copy (or the packaged program) is root's, not writable by others, and has the hash that was written.
    VerifyProgram,
    /// Register and start.
    Register,
    /// The OS reports the registration, pointing at the protected program, running.
    Confirm,
    /// Remove the protected program.
    RemoveProgram,
    /// Remove the protected folder when it is the install's own and empty.
    RemoveFolder,
    /// The OS holds neither a registration nor a job.
    ConfirmGone,
}

/// The phase names of the log and the progress.
pub type Phase = Step;

/// The install of `layout`; `existing` when the OS holds a registration or job already.
pub fn plan_install(layout: &Layout, existing: bool) -> Vec<Step> {
    let mut steps = Vec::new();
    if existing {
        steps.push(Step::StopExisting);
    }
    if layout.copy.is_some() {
        steps.push(Step::EnsureFolder);
        if layout.os == Os::Windows {
            steps.push(Step::ApplyAcl);
        }
        steps.push(Step::CopyProgram);
    }
    steps.extend([Step::VerifyProgram, Step::Register, Step::Confirm]);
    steps
}

/// The removal of `layout`. A packaged program (Linux) is dpkg's and stays.
pub fn plan_uninstall(layout: &Layout) -> Vec<Step> {
    let mut steps = vec![Step::StopExisting];
    if layout.copy.is_some() {
        steps.extend([Step::RemoveProgram, Step::RemoveFolder]);
    }
    steps.push(Step::ConfirmGone);
    steps
}

/// The removal of the service by the service itself (its own `Uninstall` request): the files go first, the stop and the
/// unregistering last, because stopping the job ends the process that is running these steps.
pub fn plan_uninstall_self(layout: &Layout) -> Vec<Step> {
    let mut steps = Vec::new();
    if layout.copy.is_some() {
        steps.extend([Step::RemoveProgram, Step::RemoveFolder]);
    }
    steps.push(Step::StopExisting);
    steps
}

/// The bounded waits of an install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Waits {
    /// How long the OS gets to drop the old job.
    pub gone: Duration,
    /// How long a registered service gets to run.
    pub start: Duration,
    /// How often the OS is asked while waiting.
    pub poll: Duration,
}

impl Default for Waits {
    fn default() -> Self {
        Self {
            gone: Duration::from_secs(20),
            start: Duration::from_secs(30),
            poll: Duration::from_secs(1),
        }
    }
}

/// One finished phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseRecord {
    /// Which.
    pub phase: Phase,
    /// How long it took.
    pub ms: u64,
    /// What it found or did, without secrets or user content.
    pub detail: String,
}

/// What an install or removal did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    /// The phases that finished, in order.
    pub phases: Vec<PhaseRecord>,
    /// The SHA-256 of the protected copy that was written (for the audit log).
    pub sha256: Option<String>,
    /// An older registration was replaced.
    pub replaced_existing: bool,
}

/// Why an install or removal stopped. A failure while registering or confirming removes the registration it made, so
/// nothing is left that would fail again at every start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// The install would not be safe (a source that is a link or sits inside the target, a folder anyone can change).
    Refused(String),
    /// Administrator rights are missing: run the elevated install.
    NeedsElevation,
    /// The user declined an OS approval.
    Declined,
    /// A phase failed; the text says why and what to do.
    Failed {
        /// The phase.
        phase: Phase,
        /// What happened.
        why: String,
    },
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::Refused(why) => write!(f, "The install was refused: {why}."),
            InstallError::NeedsElevation => f.write_str("The install needs administrator rights; run it through the system's permission prompt."),
            InstallError::Declined => f.write_str("The permission prompt was declined, so nothing was changed."),
            InstallError::Failed { phase, why } => write!(f, "The {phase:?} step failed: {why}. The helper's log has the details."),
        }
    }
}

impl std::error::Error for InstallError {}

fn failed(phase: Phase, why: impl std::fmt::Display) -> InstallError {
    InstallError::Failed {
        phase,
        why: why.to_string(),
    }
}

fn violation(what: &str, path: &Path, v: &Violation) -> InstallError {
    InstallError::Refused(format!("{what} {} is not protected: {v}", path.display()))
}

/// Runs the plans.
pub struct Installer<'a> {
    layout: &'a Layout,
    files: &'a mut dyn ProtectedFiles,
    backend: &'a mut dyn Backend,
    sys: &'a mut dyn System,
    waits: Waits,
    report: Report,
    running_here: bool,
}

impl<'a> Installer<'a> {
    /// An installer for `layout` over the OS.
    pub fn new(
        layout: &'a Layout,
        files: &'a mut dyn ProtectedFiles,
        backend: &'a mut dyn Backend,
        sys: &'a mut dyn System,
    ) -> Self {
        Self {
            layout,
            files,
            backend,
            sys,
            waits: Waits::default(),
            report: Report::default(),
            running_here: false,
        }
    }

    /// Other waits (tests).
    #[must_use]
    pub fn with_waits(mut self, waits: Waits) -> Self {
        self.waits = waits;
        self
    }

    /// Installs or updates: copies `source` (the running program) into the protected place and registers it.
    ///
    /// # Errors
    /// An [`InstallError`]; a failure after registering has started removes the registration again.
    pub fn install(mut self, source: &Path) -> Result<Report, InstallError> {
        let existing = self.backend.inspect().present();
        self.report.replaced_existing = existing;
        self.check_source(source)?;
        let plan = plan_install(self.layout, existing);
        log::info!(os:? = self.layout.os, existing = existing, steps = plan.len(); "install starts");
        for step in plan {
            self.run_step(step, Some(source)).inspect_err(|e| {
                log::error!(step:? = step, error:% = e; "install failed");
                if matches!(step, Step::Register | Step::Confirm) {
                    // Never leave a registration that does not work: the next start would fail again and again.
                    let _ = self.backend.evict();
                    let _ = self.backend.unregister();
                }
            })?;
        }
        Ok(self.report)
    }

    /// Stops and removes the service and its protected copy.
    ///
    /// # Errors
    /// An [`InstallError`].
    pub fn uninstall(mut self) -> Result<Report, InstallError> {
        let plan = plan_uninstall(self.layout);
        log::info!(os:? = self.layout.os, steps = plan.len(); "uninstall starts");
        for step in plan {
            self.run_step(step, None)
                .inspect_err(|e| log::error!(step:? = step, error:% = e; "uninstall failed"))?;
        }
        Ok(self.report)
    }

    /// Removes the service from inside the service (see [`plan_uninstall_self`]). A program file the OS will not let go of
    /// while it runs (Windows) is left in place and said so; the registration is removed all the same.
    ///
    /// # Errors
    /// An [`InstallError`].
    pub fn uninstall_self(mut self) -> Result<Report, InstallError> {
        self.running_here = true;
        let plan = plan_uninstall_self(self.layout);
        log::info!(os:? = self.layout.os, steps = plan.len(); "self-uninstall starts");
        for step in plan {
            self.run_step(step, None).inspect_err(
                |e| log::error!(step:? = step, error:% = e; "self-uninstall failed"),
            )?;
        }
        Ok(self.report)
    }

    fn check_source(&mut self, source: &Path) -> Result<(), InstallError> {
        let Some(copy) = &self.layout.copy else {
            return Ok(());
        };
        let facts = self.files.facts(source).map_err(|e| {
            failed(
                Step::CopyProgram,
                format!("{} cannot be read: {e}", source.display()),
            )
        })?;
        match facts {
            Some(f) if f.kind == FileKind::File => {}
            Some(f) => {
                return Err(InstallError::Refused(format!(
                    "the program {} is a {:?}, not a file",
                    source.display(),
                    f.kind
                )));
            }
            None => {
                return Err(InstallError::Refused(format!(
                    "the program {} is not there",
                    source.display()
                )));
            }
        }
        if source == copy.file || source.starts_with(&copy.folder) && !copy.folder_is_system {
            return Err(InstallError::Refused(
                "the program to copy is already inside the protected folder".into(),
            ));
        }
        Ok(())
    }

    fn run_step(&mut self, step: Step, source: Option<&Path>) -> Result<(), InstallError> {
        let started = Instant::now();
        let detail = match step {
            Step::StopExisting => self.stop_existing()?,
            Step::EnsureFolder => self.ensure_folder()?,
            Step::ApplyAcl => self.apply_acl()?,
            Step::CopyProgram => self.copy_program(source)?,
            Step::VerifyProgram => self.verify_program()?,
            Step::Register => self.register()?,
            Step::Confirm => self.confirm()?,
            Step::RemoveProgram => self.remove_program()?,
            Step::RemoveFolder => self.remove_folder()?,
            Step::ConfirmGone => self.confirm_gone()?,
        };
        let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        log::info!(step:? = step, ms = ms, detail = detail.as_str(); "install phase finished");
        self.report.phases.push(PhaseRecord {
            phase: step,
            ms,
            detail,
        });
        Ok(())
    }

    fn copy(&self) -> Result<&CopyTarget, InstallError> {
        self.layout
            .copy
            .as_ref()
            .ok_or_else(|| InstallError::Refused("this OS has no protected copy to make".into()))
    }

    fn stop_existing(&mut self) -> Result<String, InstallError> {
        let mut notes = Vec::new();
        // Read before the stop: afterwards the OS may not name the process any more.
        let running = self.backend.job_process();
        if self.running_here {
            // The stop ends the process that is running these steps: take the registration away first.
            self.backend
                .unregister()
                .map_err(|e| failed(Step::StopExisting, e))?;
            if let Err(e) = self.backend.evict() {
                notes.push(format!("stop: {e}"));
            }
        } else {
            if let Err(e) = self.backend.evict() {
                notes.push(format!("stop: {e}"));
            }
            self.backend
                .unregister()
                .map_err(|e| failed(Step::StopExisting, e))?;
        }
        let mut waited = Duration::ZERO;
        while !self.backend.job_gone() {
            if waited >= self.waits.gone {
                notes.push("the OS still holds the job".into());
                break;
            }
            self.backend.pause(self.waits.poll);
            waited += self.waits.poll;
        }
        // The entry being gone is not the process being gone: wait for that process itself, by identity, before anything
        // is removed (Windows cannot delete a program a process still holds).
        if let Some(process) = running.filter(|_| !self.running_here) {
            let mut waited = Duration::ZERO;
            while self.backend.process_alive(process) {
                if waited >= self.waits.gone {
                    return Err(failed(
                        Step::StopExisting,
                        format!(
                            "the old process (pid {}) is still running {} s after it was asked to stop; close it, or restart the computer, and try again",
                            process.pid(),
                            self.waits.gone.as_secs()
                        ),
                    ));
                }
                self.backend.pause(self.waits.poll);
                waited += self.waits.poll;
            }
        }
        Ok(if notes.is_empty() {
            "removed".into()
        } else {
            notes.join("; ")
        })
    }

    fn ensure_folder(&mut self) -> Result<String, InstallError> {
        let copy = self.copy()?.clone();
        let facts = self
            .files
            .facts(&copy.folder)
            .map_err(|e| failed(Step::EnsureFolder, e))?;
        let created = facts.is_none();
        if created {
            self.files
                .create_dir(&copy.folder, copy.folder_mode)
                .map_err(|e| {
                    failed(
                        Step::EnsureFolder,
                        format!("{}: {e}", copy.folder.display()),
                    )
                })?;
        }
        let facts = self
            .files
            .facts(&copy.folder)
            .map_err(|e| failed(Step::EnsureFolder, e))?;
        verify_protected(facts.as_ref(), FileKind::Dir)
            .map_err(|v| violation("the folder", &copy.folder, &v))?;
        Ok(if created {
            "created".into()
        } else {
            "checked".into()
        })
    }

    fn apply_acl(&mut self) -> Result<String, InstallError> {
        let folder = self.copy()?.folder.clone();
        let args = acl_command(&folder);
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        match self.sys.run(ICACLS, &refs) {
            Ok(o) if o.success() => {}
            Ok(o) => return Err(failed(Step::ApplyAcl, format!("icacls: {}", o.reason()))),
            Err(e) => return Err(failed(Step::ApplyAcl, format!("icacls {e}"))),
        }
        let folder_text = folder.to_string_lossy().into_owned();
        let listing = self
            .sys
            .run(ICACLS, &[&folder_text])
            .map_err(|e| failed(Step::ApplyAcl, format!("icacls {e}")))?;
        acl_shape_ok(&listing.stdout).map_err(|why| failed(Step::ApplyAcl, why))?;
        Ok("set and checked".into())
    }

    fn copy_program(&mut self, source: Option<&Path>) -> Result<String, InstallError> {
        let copy = self.copy()?.clone();
        let source = source.ok_or_else(|| InstallError::Refused("no program to copy".into()))?;
        let hash = self
            .files
            .copy_program(source, &copy.file, copy.file_mode)
            .map_err(|e| failed(Step::CopyProgram, format!("{}: {e}", copy.file.display())))?;
        let short = hash.chars().take(12).collect::<String>();
        self.report.sha256 = Some(hash);
        Ok(format!("sha256 {short}"))
    }

    fn verify_program(&mut self) -> Result<String, InstallError> {
        let program = self.layout.program.clone();
        let facts = self
            .files
            .facts(&program)
            .map_err(|e| failed(Step::VerifyProgram, e))?;
        verify_protected(facts.as_ref(), FileKind::File)
            .map_err(|v| violation("the program", &program, &v))?;
        if let Some(written) = self.report.sha256.clone() {
            let now = self
                .files
                .sha256(&program)
                .map_err(|e| failed(Step::VerifyProgram, e))?;
            if now != written {
                return Err(failed(
                    Step::VerifyProgram,
                    "the copy differs from what was written",
                ));
            }
        }
        Ok("owner, mode and hash checked".into())
    }

    fn register(&mut self) -> Result<String, InstallError> {
        match self.backend.register() {
            Ok(()) => Ok("registered".into()),
            Err(RegisterError::NeedsElevation) => Err(InstallError::NeedsElevation),
            Err(RegisterError::Declined) => Err(InstallError::Declined),
            Err(e) => Err(failed(Step::Register, e)),
        }
    }

    fn confirm(&mut self) -> Result<String, InstallError> {
        let mut waited = Duration::ZERO;
        loop {
            let look = self.backend.inspect();
            match look.registration {
                Registration::Registered => {}
                Registration::NeedsApproval => {
                    return Err(failed(
                        Step::Confirm,
                        "macOS needs your approval in System Settings > General > Login Items",
                    ));
                }
                Registration::SwitchedOff => {
                    return Err(failed(
                        Step::Confirm,
                        "the service is switched off in the system settings",
                    ));
                }
                Registration::Unavailable(why) => return Err(failed(Step::Confirm, why)),
                Registration::Absent => {
                    return Err(failed(
                        Step::Confirm,
                        "the OS holds no registration after registering",
                    ));
                }
            }
            if look.points_here == Some(false) {
                return Err(failed(
                    Step::Confirm,
                    "the registration points at another program",
                ));
            }
            match look.job {
                Job::Running => return Ok("running".into()),
                Job::Refused => {
                    return Err(failed(
                        Step::Confirm,
                        "the OS refuses to start the registered program",
                    ));
                }
                _ => {}
            }
            if waited >= self.waits.start {
                return Err(failed(
                    Step::Confirm,
                    "it was registered but did not start in time",
                ));
            }
            self.backend.pause(self.waits.poll);
            waited += self.waits.poll;
        }
    }

    fn remove_program(&mut self) -> Result<String, InstallError> {
        let file = self.copy()?.file.clone();
        match self.files.remove_file(&file) {
            Ok(()) => Ok("removed".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("was not there".into()),
            Err(e) if self.running_here => Ok(format!("left in place while it runs: {e}")),
            Err(e) => Err(failed(
                Step::RemoveProgram,
                format!("{}: {e}", file.display()),
            )),
        }
    }

    fn remove_folder(&mut self) -> Result<String, InstallError> {
        let copy = self.copy()?.clone();
        if copy.folder_is_system {
            return Ok("the OS's folder stays".into());
        }
        match self.files.remove_dir(&copy.folder) {
            Ok(()) => Ok("removed".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("was not there".into()),
            // Something else lives there (the data folder of an older layout): not ours to delete.
            Err(e) => Ok(format!("left in place: {e}")),
        }
    }

    fn confirm_gone(&mut self) -> Result<String, InstallError> {
        let look = self.backend.inspect();
        // A packaged unit stays on disk, switched off; anything registered or running is not gone.
        let registered = !matches!(
            look.registration,
            Registration::Absent | Registration::SwitchedOff
        );
        let running = matches!(look.job, Job::Running | Job::Refused);
        if registered || running {
            return Err(failed(
                Step::ConfirmGone,
                "the OS still holds the registration or the job",
            ));
        }
        Ok("gone".into())
    }
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
