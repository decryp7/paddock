//! The helper's protected folders and the checks before it installs or runs anything as SYSTEM or root (WP-SEC; Dyno ADR 0027
//! §3, paddock-tools ADR 0051 D4).
//!
//! `C:\ProgramData` lets ordinary users create files and folders and subfolders inherit that, so the helper's data root, tools
//! and staging folders are locked when the helper creates them (Windows: inheritance removed, SYSTEM and Administrators full,
//! Users read and execute, owner Administrators; elsewhere root-owned 0755), and **before every install and every run** the
//! chain to the program is verified again from the open objects. A chain that fails is refused with a typed reason and the
//! next step (Repair, which starts the helper again and so locks the folders again); nothing from it is run.
//!
//! The rule itself is `paddock_service::privileged::trust` (shared); this file is the helper's use of it: [`TrustGuard`]
//! and [`GuardedRunner`], a [`ToolRunner`] that spawns only a checked, held program.

use dyno_core::helper::HelperHost;
use dyno_core::ports::PlatformError;
use dyno_platform::runner::{Tool, ToolOutput, ToolRunner};
use paddock_service::privileged::files::FileKind;
use paddock_service::privileged::trust::{is_within, require_same_path, verify_chain, Reason, TrustPort, Untrusted, SID_ADMINISTRATORS};
use paddock_service::privileged::trust_real::{HeldImage, RealTrust};
use paddock_service::privileged::windows_acl::{acl_command, acl_shape_ok};
use std::io;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// What to do with an existing folder that is not protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnUntrusted {
    /// Remove it (links are removed, not followed) and make it again.
    Purge,
    /// Leave it and refuse; Repair removes it.
    Refuse,
}

/// A program held open (see [`HeldImage`]) and where to run it.
pub struct Held {
    /// The path the open object really has.
    pub final_path: String,
    /// What to run.
    pub spawn_path: String,
    _keep: Box<dyn Send>,
}

/// Opens a program for a privileged run. The real one is [`HeldImage`]; tests use a fake with a junction in the path.
pub trait ImageOpener: Send + Sync {
    /// Opens `requested` without following a link.
    ///
    /// # Errors
    /// Missing, a link, or not a regular file.
    fn open(&self, requested: &str) -> io::Result<Held>;
}

/// The real opener.
pub struct RealOpener;

impl ImageOpener for RealOpener {
    fn open(&self, requested: &str) -> io::Result<Held> {
        let h = HeldImage::open(requested)?;
        Ok(Held {
            final_path: h.final_path().to_string(),
            spawn_path: h.spawn_path().to_string(),
            _keep: Box::new(h),
        })
    }
}

/// Locks the helper's folders and checks them (and the programs in them) before use.
pub struct TrustGuard {
    host: HelperHost,
    port: Mutex<Box<dyn TrustPort>>,
    opener: Box<dyn ImageOpener>,
}

fn ms(from: Instant) -> u64 {
    from.elapsed().as_millis() as u64
}

fn text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

impl TrustGuard {
    /// A guard over `port` and `opener`.
    pub fn new(host: HelperHost, port: Box<dyn TrustPort>, opener: Box<dyn ImageOpener>) -> Self {
        TrustGuard {
            host,
            port: Mutex::new(port),
            opener,
        }
    }

    /// The guard of this machine.
    pub fn system(host: HelperHost) -> Self {
        TrustGuard::new(host, Box::new(RealTrust), Box::new(RealOpener))
    }

    fn port(&self) -> std::sync::MutexGuard<'_, Box<dyn TrustPort>> {
        self.port.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Creates `dir` if it is absent and locks it (Windows: a protected DACL; elsewhere root-owned 0755), then verifies it.
    /// Run at every start of the helper and before every install, so a folder somebody loosened is locked again.
    ///
    /// A folder that already exists and is not protected (a user made it before the fix or before the first install, so its
    /// owner or rights are not SYSTEM's) holds nothing the helper can trust: with [`OnUntrusted::Purge`] it is removed (links
    /// inside are removed, never followed) and made again; with [`OnUntrusted::Refuse`] it is left and refused.
    ///
    /// # Errors
    /// Why the folder could not be locked or does not pass the check afterwards.
    pub fn ensure_root(&self, dir: &Path, on_untrusted: OnUntrusted) -> Result<(), PlatformError> {
        let started = Instant::now();
        let path = text(dir);
        let result = self
            .purge_if_untrusted(dir, on_untrusted)
            .and_then(|()| self.lock(&path))
            .and_then(|()| self.verify_dir(dir, dir).map_err(|u| PlatformError::Failed(u.to_string())));
        match &result {
            Ok(()) => log::info!(Dir = path.as_str(), Ms = ms(started); "Helper: protected folder locked and checked"),
            Err(e) => {
                log::error!(Dir = path.as_str(), Error = e.to_string().as_str(), Ms = ms(started); "Helper: protected folder could not be locked")
            }
        }
        result
    }

    /// Empties `dir` (links removed, never followed) and locks it again: the staging folder of one install, so what an earlier
    /// run left (a half-written package, a file its installer still held) cannot collide with this one.
    ///
    /// # Errors
    /// The folder could not be emptied or locked.
    pub fn reset_root(&self, dir: &Path) -> Result<(), PlatformError> {
        let path = text(dir);
        self.port().remove_tree(&path).map_err(|e| {
            PlatformError::Failed(format!(
                "{path} could not be emptied: {e}. Repair the helper (Settings > Helper > Repair)."
            ))
        })?;
        self.ensure_root(dir, OnUntrusted::Purge)
    }

    fn purge_if_untrusted(&self, dir: &Path, on_untrusted: OnUntrusted) -> Result<(), PlatformError> {
        let path = text(dir);
        let mut port = self.port();
        let style = port.style();
        match port.node(&path) {
            Ok(None) => return Ok(()),
            Ok(Some(_)) => {}
            Err(e) => {
                return Err(PlatformError::Failed(format!(
                    "{path} could not be examined: {e}. Repair the helper (Settings > Helper > Repair)."
                )))
            }
        }
        let Err(found) = verify_chain(port.as_mut(), &path, FileKind::Dir, Some(&path), false) else {
            return Ok(());
        };
        log::warn!(Dir = path.as_str(), Found = found.to_string().as_str(), Purge = on_untrusted == OnUntrusted::Purge; "Helper: an existing folder is not protected");
        // Only a fault in the folder itself (or below it) is cured by emptying it; a fault in an ancestor stays for the
        // verification after the lock to report.
        if !is_within(&found.path, &path, style) {
            return Ok(());
        }
        match on_untrusted {
            OnUntrusted::Refuse => Err(PlatformError::Failed(found.to_string())),
            OnUntrusted::Purge => {
                port.remove_tree(&path).map_err(|e| {
                    PlatformError::Failed(format!(
                        "{path} is not protected ({}) and could not be removed: {e}. Repair the helper (Settings > Helper > Repair).",
                        found.reason
                    ))
                })?;
                log::warn!(Dir = path.as_str(); "Helper: an unprotected folder was removed and is made again as SYSTEM");
                Ok(())
            }
        }
    }

    /// Locks the data root, then `tools` and `staging` in it (see [`ensure_root`](Self::ensure_root)). The data root holds
    /// `config.json`, which the helper takes its orders from, so it is never purged at start (`data`).
    ///
    /// # Errors
    /// The first folder that could not be locked.
    pub fn prepare_folders(&self, data_dir: &Path, data: OnUntrusted) -> Result<(), PlatformError> {
        self.ensure_root(data_dir, data)?;
        self.ensure_root(&data_dir.join("tools"), OnUntrusted::Purge)?;
        self.ensure_root(&data_dir.join("staging"), OnUntrusted::Purge)
    }

    fn lock(&self, path: &str) -> Result<(), PlatformError> {
        let failed = |what: &str, e: &dyn std::fmt::Display| {
            PlatformError::Failed(format!(
                "{path} could not be locked ({what}: {e}). Repair the helper (Settings > Helper > Repair)."
            ))
        };
        let mut port = self.port();
        port.create_dir_all(path).map_err(|e| failed("create", &e))?;
        match self.host {
            HelperHost::Windows => {
                port.icacls(&[path.to_string(), "/setowner".into(), format!("*{SID_ADMINISTRATORS}")])
                    .map_err(|e| failed("owner", &e))?;
                port.icacls(&acl_command(Path::new(path))).map_err(|e| failed("icacls", &e))?;
                let listing = port.icacls(&[path.to_string()]).map_err(|e| failed("icacls listing", &e))?;
                acl_shape_ok(&listing).map_err(|why| failed("acl", &why))
            }
            HelperHost::Macos | HelperHost::Linux => port.chmod(path, 0o755).map_err(|e| failed("chmod", &e)),
        }
    }

    /// Verifies `dir` (inside `root`) from the open objects: every component, the stricter rule inside `root`.
    ///
    /// # Errors
    /// The first component that is not protected.
    pub fn verify_dir(&self, dir: &Path, root: &Path) -> Result<(), Untrusted> {
        let r = verify_chain(self.port().as_mut(), &text(dir), FileKind::Dir, Some(&text(root)), false);
        if let Err(u) = &r {
            log::warn!(Path = u.path.as_str(), Reason = u.reason.to_string().as_str(); "Helper: a folder is not protected; refused");
        }
        r
    }

    /// Opens `program` for a run and verifies the chain to the file the open object really is. `root` is the helper's
    /// protected folder the program sits in (`None` for a program the OS ships, like `/usr/sbin/smartctl`, whose links are
    /// resolved first on Linux).
    ///
    /// # Errors
    /// Why the program is not run.
    pub fn open_checked(&self, program: &str, root: Option<&Path>) -> Result<Held, PlatformError> {
        let refuse = |u: Untrusted| {
            log::warn!(Program = program, Path = u.path.as_str(), Reason = u.reason.to_string().as_str(); "Helper: a program is not protected; not run");
            PlatformError::Failed(u.to_string())
        };
        let held = self.opener.open(program).map_err(|e| {
            log::warn!(Program = program, Error = e.to_string().as_str(); "Helper: a program could not be opened for a checked run; not run");
            let reason = match e.kind() {
                io::ErrorKind::NotFound => Reason::Missing,
                _ => Reason::Unreadable(e.to_string()),
            };
            PlatformError::Failed(
                Untrusted {
                    path: program.to_string(),
                    reason,
                }
                .to_string(),
            )
        })?;
        let mut port = self.port();
        let style = port.style();
        // Only a program the OS ships may sit behind links (merged /usr on Linux); everything else must be where it says.
        let may_resolve = root.is_none() && self.host == HelperHost::Linux;
        if !may_resolve {
            require_same_path(program, &held.final_path, style).map_err(refuse)?;
        }
        let checked = held.final_path.strip_prefix("\\\\?\\").unwrap_or(&held.final_path).to_string();
        verify_chain(port.as_mut(), &checked, FileKind::File, root.map(text).as_deref(), false).map_err(refuse)?;
        Ok(held)
    }
}

/// A [`ToolRunner`] that runs a program only after [`TrustGuard::open_checked`], by the checked, held path.
pub struct GuardedRunner {
    guard: std::sync::Arc<TrustGuard>,
    root: Option<std::path::PathBuf>,
    inner: Box<dyn ToolRunner>,
}

impl GuardedRunner {
    /// Runs through `inner` what the guard allows.
    pub fn new(guard: std::sync::Arc<TrustGuard>, root: Option<std::path::PathBuf>, inner: Box<dyn ToolRunner>) -> Self {
        GuardedRunner { guard, root, inner }
    }
}

impl ToolRunner for GuardedRunner {
    fn run_tool(&self, tool: &Tool, timeout: Duration) -> Result<ToolOutput, PlatformError> {
        let started = Instant::now();
        let held = self.guard.open_checked(&tool.program, self.root.as_deref())?;
        let checked = Tool {
            program: held.spawn_path.clone(),
            args: tool.args.clone(),
            env: tool.env.clone(),
        };
        // `held` stays open until the process has run: nobody can swap or delete the checked file meanwhile.
        let out = self.inner.run_tool(&checked, timeout);
        log::info!(Program = tool.program.as_str(), Ok = out.is_ok(), Ms = ms(started); "Helper: checked program ran");
        drop(held);
        out
    }
}

#[cfg(test)]
pub(crate) mod tests;
