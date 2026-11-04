//! The install, update and removal of a privileged service as a pure plan ([`plan_install`], [`plan_uninstall`]) and the
//! [`Installer`] that runs it over [`ProtectedFiles`], a [`Backend`] and the OS ([`System`], for `icacls`). The install is
//! idempotent: run again it takes over an older registration (a 0.x helper under the same label, the previous version)
//! and is therefore also the update. Every phase is logged with its duration and ends in the [`Report`] (ER-27).

use super::files::{FileKind, HeldSource, ProtectedFiles, Violation, hash_bytes, verify_protected};
use super::layout::{CopyTarget, Layout};
use super::trust::{self, Style, TrustPort, Untrusted};
use super::windows_acl::{ICACLS, acl_command, acl_shape_ok};
use crate::backend::{Backend, Job, Registration};
use crate::port::{RegisterError, System};
use paddock_env::Os;
use std::path::Path;
use std::time::{Duration, Instant};

/// The largest program the installer copies (it is read whole through one handle, so what is checked is what is written).
pub const MAX_PROGRAM: u64 = 256 * 1024 * 1024;
/// The largest signature file read.
pub const MAX_SIGNATURE: u64 = 16 * 1024;

/// Checks the bytes of the program that is about to be copied against a signature (macOS: a minisign signature of the family
/// update key, because the bundle is user-owned and no ownership check can vouch for it).
pub trait SourceVerifier: Send + Sync {
    /// `Ok` with a short verdict for the log, or the reason in words (with the next step) the program is not accepted.
    ///
    /// # Errors
    /// Why the bytes are not accepted.
    fn verify(&self, program: &[u8], signature: &[u8]) -> Result<String, String>;
}

/// The signature check of an install: the verifier and where the signature file may be (the first that exists is read).
pub struct SignatureCheck<'a> {
    /// What judges the bytes.
    pub verifier: &'a dyn SourceVerifier,
    /// The places of the signature file, in order.
    pub locations: Vec<std::path::PathBuf>,
}

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
/// unregistering last, because stopping the job ends the process that is running these steps. If this process is still
/// alive afterwards, [`Step::ConfirmGone`] says whether the OS really dropped the registration (ADR 0054 RC-2).
pub fn plan_uninstall_self(layout: &Layout) -> Vec<Step> {
    let mut steps = Vec::new();
    if layout.copy.is_some() {
        steps.extend([Step::RemoveProgram, Step::RemoveFolder]);
    }
    steps.extend([Step::StopExisting, Step::ConfirmGone]);
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
    /// The signature verdict on the source program, when a signature check ran.
    pub signature: Option<String>,
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

/// `\\?\C:\x` as `C:\x` (what `canonicalize` returns on Windows is the verbatim form, which neither the trust rule's path
/// splitting nor a comparison with the layout's plain paths understands). `\\?\UNC\` paths are left as they are.
pub fn without_verbatim_prefix(path: &str) -> &str {
    match path.strip_prefix("\\\\?\\") {
        Some(rest) if !rest.starts_with("UNC\\") => rest,
        _ => path,
    }
}

/// A Windows path that failed the trust rule, in words with the next step.
fn untrusted(what: &str, e: &Untrusted) -> InstallError {
    log::error!(what = what, path = e.path.as_str(), reason:% = e.reason; "install: not a protected place");
    InstallError::Refused(format!("{what}: {}", e.describe()))
}

/// Runs the plans.
pub struct Installer<'a> {
    layout: &'a Layout,
    files: &'a mut dyn ProtectedFiles,
    backend: &'a mut dyn Backend,
    sys: &'a mut dyn System,
    trust: Option<&'a mut dyn TrustPort>,
    signature: Option<SignatureCheck<'a>>,
    held: Option<HeldSource>,
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
            trust: None,
            signature: None,
            held: None,
            waits: Waits::default(),
            report: Report::default(),
            running_here: false,
        }
    }

    /// The trust rule of the machine (Windows: who may change Program Files and the folder the program is copied into, and the
    /// source of the copy). A Windows install without it is refused: nothing else can say that the copy is protected.
    #[must_use]
    pub fn with_trust(mut self, trust: &'a mut dyn TrustPort) -> Self {
        self.trust = Some(trust);
        self
    }

    /// Requires the program to carry a valid signature (see [`SignatureCheck`]); without one the install is refused.
    #[must_use]
    pub fn with_signature(mut self, check: SignatureCheck<'a>) -> Self {
        self.signature = Some(check);
        self
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
                    if let Err(why) = self.backend.evict() {
                        log::warn!(step:? = step, error:% = why; "install clean-up: the job could not be stopped");
                    }
                    if let Err(why) = self.backend.unregister() {
                        log::warn!(step:? = step, error:% = why; "install clean-up: the registration could not be removed");
                    }
                }
            })?;
        }
        Ok(self.report)
    }

    /// Checks the program that would be installed (its place, its signature) and nothing else: no folder is made, nothing is
    /// written or registered. The caller runs it before it prepares anything of its own, so a refused source leaves no trace.
    ///
    /// # Errors
    /// An [`InstallError`] when the source would be refused by [`install`](Self::install).
    pub fn preflight(mut self, source: &Path) -> Result<Report, InstallError> {
        self.check_source(source)?;
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

    /// The trust port of a Windows install; its absence refuses the install.
    fn trust_port(&mut self) -> Result<&mut (dyn TrustPort + 'a), InstallError> {
        self.trust.as_deref_mut().ok_or_else(|| {
            InstallError::Refused("no trust check is configured for this install, so nothing can say the copy is protected".into())
        })
    }

    /// Opens the source once, checks it through that handle and keeps its bytes: what is checked here is what is written.
    fn check_source(&mut self, source: &Path) -> Result<(), InstallError> {
        let Some(copy) = &self.layout.copy else {
            return Ok(());
        };
        let copy = copy.clone();
        if self.layout.os == Os::Windows {
            // The chain to the source must be one only administrators can change: a per-user copy is refused (a user could
            // swap it between this check and the copy), the per-machine one in Program Files passes.
            let text = without_verbatim_prefix(&source.to_string_lossy()).to_string();
            let port = self.trust_port()?;
            trust::verify_chain(port, &text, FileKind::File, None, false).map_err(|e| {
                log::error!(source = "checked", verdict = "refused"; "install: the program to copy is not in a protected place");
                InstallError::Refused(format!(
                    "the program to copy is not in a place only administrators can change ({}); reinstall Dyno for all users (per-machine) and turn the helper on again",
                    e.describe()
                ))
            })?;
        }
        let held = self.files.open_source(source, MAX_PROGRAM).map_err(|e| {
            failed(
                Step::CopyProgram,
                format!("{} cannot be read: {e}", source.display()),
            )
        })?;
        let Some(held) = held else {
            return Err(InstallError::Refused(format!(
                "the program {} is not there",
                source.display()
            )));
        };
        if held.facts.kind != FileKind::File {
            return Err(InstallError::Refused(format!(
                "the program {} is a {:?}, not a file",
                source.display(),
                held.facts.kind
            )));
        }
        let inside = if self.layout.os == Os::Windows {
            // Windows paths are compared as text with Windows rules, whatever the host.
            let plain = without_verbatim_prefix(&source.to_string_lossy()).to_string();
            let (file, folder) = (copy.file.to_string_lossy(), copy.folder.to_string_lossy());
            trust::is_within(&plain, &file, Style::Windows)
                || (trust::is_within(&plain, &folder, Style::Windows) && !copy.folder_is_system)
        } else {
            source == copy.file || source.starts_with(&copy.folder) && !copy.folder_is_system
        };
        if inside {
            return Err(InstallError::Refused(
                "the program to copy is already inside the protected folder".into(),
            ));
        }
        let sha = hash_bytes(&held.bytes);
        let verdict = self.check_signature(&held.bytes)?;
        log::info!(
            sha256 = sha.as_str(),
            bytes = held.bytes.len(),
            signature = verdict.as_deref().unwrap_or("not required");
            "install: the source program was read through one handle and checked"
        );
        self.report.signature = verdict;
        self.held = Some(held);
        Ok(())
    }

    /// The signature verdict on `program`, or a refusal (no signature file, a bad one, none configured is no check).
    fn check_signature(&mut self, program: &[u8]) -> Result<Option<String>, InstallError> {
        let Some(check) = &self.signature else {
            return Ok(None);
        };
        let mut found = None;
        for location in &check.locations {
            match self.files.open_source(location, MAX_SIGNATURE) {
                Ok(Some(h)) if h.facts.kind == FileKind::File => {
                    found = Some(h.bytes);
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    log::warn!(signature_file = "unreadable", error:% = e; "install: a signature location could not be read")
                }
            }
        }
        let Some(signature) = found else {
            log::error!(verdict = "missing"; "install: the program has no signature file");
            return Err(InstallError::Refused(
                "the helper has no signature file next to it, so it cannot be told from a replaced copy; reinstall Dyno from the official download and try again".into(),
            ));
        };
        match check.verifier.verify(program, &signature) {
            Ok(verdict) => Ok(Some(verdict)),
            Err(why) => {
                log::error!(verdict = "refused", reason = why.as_str(); "install: the program's signature was refused");
                Err(InstallError::Refused(why))
            }
        }
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
            let after = self
                .backend
                .unregister()
                .map_err(|e| failed(Step::StopExisting, e))?;
            log::info!(registration:? = after.registration, job:? = after.job; "stop: unregistered, read back");
            if let Err(e) = self.backend.evict() {
                notes.push(format!("stop: {e}"));
            }
        } else {
            if let Err(e) = self.backend.evict() {
                notes.push(format!("stop: {e}"));
            }
            let after = self
                .backend
                .unregister()
                .map_err(|e| failed(Step::StopExisting, e))?;
            log::info!(registration:? = after.registration, job:? = after.job; "stop: unregistered, read back");
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
        if self.layout.os == Os::Windows {
            // Program Files itself: nobody but administrators may replace what the install makes in it.
            let folder = copy.folder.to_string_lossy().into_owned();
            let parent = trust::parent_of(&folder, Style::Windows)
                .ok_or_else(|| InstallError::Refused(format!("{folder} has no parent folder")))?;
            let port = self.trust_port()?;
            trust::verify_container(port, &parent)
                .map_err(|e| untrusted("the Program Files folder", &e))?;
            log::info!(root = parent.as_str(); "install: the Program Files folder passed the trust check");
        }
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
        // The listing says what was set; the trust rule reads the real security descriptor from the open folder (owner,
        // inheritance, every ancestor) and is what proves nobody else can change it.
        let text = folder.to_string_lossy().into_owned();
        let port = self.trust_port()?;
        trust::verify_chain(port, &text, FileKind::Dir, Some(&text), false)
            .map_err(|e| untrusted("the protected folder", &e))?;
        Ok("set and checked".into())
    }

    fn copy_program(&mut self, source: Option<&Path>) -> Result<String, InstallError> {
        let copy = self.copy()?.clone();
        let source = source.ok_or_else(|| InstallError::Refused("no program to copy".into()))?;
        // The bytes that were checked through the one open handle; the path is not opened again.
        let held = self.held.take().ok_or_else(|| {
            InstallError::Refused(format!(
                "{} was not checked before the copy",
                source.display()
            ))
        })?;
        let hash = self
            .files
            .write_program(&held.bytes, &copy.file, copy.file_mode)
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
        if self.layout.os == Os::Windows {
            // Windows reports no owner or mode on a file, so the line above only proved its kind: the trust rule reads the
            // descriptor of the file and of every folder above it.
            let text = program.to_string_lossy().into_owned();
            let root = self.copy()?.folder.to_string_lossy().into_owned();
            let port = self.trust_port()?;
            trust::verify_chain(port, &text, FileKind::File, Some(&root), false)
                .map_err(|e| untrusted("the installed program", &e))?;
        }
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
        log::info!(registration:? = look.registration, job:? = look.job; "confirm gone: read back");
        // A packaged unit stays on disk, switched off; anything registered or running is not gone. A service removing
        // itself is still the job, whose stop is in flight: only its registration is judged.
        let gone = if self.running_here {
            matches!(
                look.registration,
                Registration::Absent | Registration::SwitchedOff
            )
        } else {
            look.withdrawn()
        };
        if !gone {
            return Err(failed(
                Step::ConfirmGone,
                format!(
                    "the OS still holds the registration or the job (registration {:?}, job {:?})",
                    look.registration, look.job
                ),
            ));
        }
        Ok("gone".into())
    }
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
