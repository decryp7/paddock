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
use paddock_service::privileged::trust::{
    between, is_within, require_same_path, verify_chain, verify_container, verify_tree, Reason, TrustPort, Untrusted,
};
use paddock_service::privileged::trust_real::{HeldImage, RealTrust};
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

/// Why a folder or a program cannot be used (ADR 0051 D4.4's `Unusable`). Each says what to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unusable {
    /// A folder of the helper (or the way to it) is not protected, and the lock was not applied: Repair does that.
    HelperFolderNotProtected(Untrusted),
    /// A program is not in a protected place; nothing was run.
    ProgramNotProtected(Untrusted),
    /// The helper could not lock or make a folder; Repair already ran, so the next step is by hand.
    CouldNotLock {
        /// The folder.
        path: String,
        /// What went wrong.
        why: String,
    },
}

impl std::fmt::Display for Unusable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unusable::HelperFolderNotProtected(u) | Unusable::ProgramNotProtected(u) => u.fmt(f),
            Unusable::CouldNotLock { path, why } => write!(
                f,
                "{path} could not be locked: {why}. Remove or fix that folder as an administrator, then try again."
            ),
        }
    }
}

impl std::error::Error for Unusable {}

impl From<Unusable> for PlatformError {
    fn from(u: Unusable) -> Self {
        PlatformError::Failed(u.to_string())
    }
}

/// Locks the helper's folders and checks them (and the programs in them) before use.
pub struct TrustGuard {
    host: HelperHost,
    port: Mutex<Box<dyn TrustPort>>,
    opener: Box<dyn ImageOpener>,
    /// The last system folder on the way to the helper's: it and everything above it must already be trusted, and the
    /// helper never changes anything at or above it. What lies between it and a folder the helper locks is the helper's own.
    base: String,
}

fn ms(from: Instant) -> u64 {
    from.elapsed().as_millis() as u64
}

fn text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

impl TrustGuard {
    /// A guard over `port` and `opener`; `base` is the system folder the helper's vendor folder is made in
    /// (`C:\ProgramData`, `/Library/Application Support`, `/var/lib`).
    pub fn new(host: HelperHost, port: Box<dyn TrustPort>, opener: Box<dyn ImageOpener>, base: &Path) -> Self {
        TrustGuard {
            host,
            port: Mutex::new(port),
            opener,
            base: text(base),
        }
    }

    /// The guard of this machine for the data folder `data_dir`: its base is the folder that holds the vendor folder
    /// (`C:\\ProgramData\\Dyno\\Helper` has `C:\\ProgramData`).
    pub fn system(host: HelperHost, data_dir: &Path) -> Self {
        let base = data_dir.parent().and_then(Path::parent).unwrap_or(data_dir);
        TrustGuard::new(host, Box::new(RealTrust), Box::new(RealOpener), base)
    }

    fn port(&self) -> std::sync::MutexGuard<'_, Box<dyn TrustPort>> {
        self.port.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Makes `dir` the helper's protected folder: checks the way to it, makes what is missing, locks it, verifies it.
    ///
    /// 1. **Nothing changes unless the way is trusted.** `base` and everything above it must pass the ancestor rule, or this
    ///    returns at once: no folder is made, no ACL set, nothing removed.
    /// 2. Each folder between `base` and `dir` is made with its protected descriptor given at creation (create-new); one that
    ///    is already there and fails the ancestor rule is moved aside (never followed or read) and made again.
    /// 3. A `dir` that exists and is not protected (a user made it, or it is loose, or anything in it is not the helper's)
    ///    holds nothing the helper can trust: with [`OnUntrusted::Purge`] it is removed (links inside are removed, never
    ///    followed) and made again; with [`OnUntrusted::Refuse`] it is left and refused.
    /// 4. The owner and DACL are applied to the folder by a handle (never by path), and the result is verified.
    ///
    /// # Errors
    /// Why the folder could not be locked or does not pass the check afterwards.
    pub fn ensure_root(&self, dir: &Path, on_untrusted: OnUntrusted) -> Result<(), Unusable> {
        let started = Instant::now();
        let path = text(dir);
        let result = self.ensure(&path, on_untrusted);
        match &result {
            Ok(()) => log::info!(Dir = path.as_str(), Ms = ms(started); "Helper: protected folder locked and checked"),
            Err(e) => {
                log::error!(Dir = path.as_str(), Error = e.to_string().as_str(), Ms = ms(started); "Helper: protected folder could not be locked")
            }
        }
        result
    }

    fn ensure(&self, path: &str, on_untrusted: OnUntrusted) -> Result<(), Unusable> {
        let could_not = |what: &str, e: &dyn std::fmt::Display| Unusable::CouldNotLock {
            path: path.to_string(),
            why: format!("{what}: {e}"),
        };
        let mut port = self.port();
        let style = port.style();
        let base = self.base.clone();
        let between = between(&base, path, style).ok_or_else(|| could_not("it is not below the system folder", &base))?;
        // 1. The way to the helper's folders is a system place that must already be trusted; if it is not, nothing is touched.
        verify_container(port.as_mut(), &base).map_err(|u| match on_untrusted {
            OnUntrusted::Refuse => Unusable::HelperFolderNotProtected(u),
            // A repair cannot fix a system folder, so it does not send the user back to Repair.
            OnUntrusted::Purge => could_not("the system folder above it", &u.describe()),
        })?;
        // 2. Each folder between the base and `dir`.
        for folder in &between {
            match port.node(folder).map_err(|e| could_not("examine", &e))? {
                None => port.create_dir(folder).map_err(|e| could_not("create", &e))?,
                Some(_) => {
                    if let Err(found) = verify_chain(port.as_mut(), folder, FileKind::Dir, None, false) {
                        log::warn!(Dir = folder.as_str(), Found = found.to_string().as_str(); "Helper: a folder on the way is not trusted; moved aside");
                        let aside = port.rename_aside(folder).map_err(|e| could_not("move aside", &e))?;
                        log::warn!(Dir = folder.as_str(), Aside = aside.as_str(); "Helper: the untrusted folder was moved aside and is never read");
                        port.create_dir(folder).map_err(|e| could_not("create", &e))?;
                    }
                }
            }
        }
        // 3. The folder itself.
        match port.node(path).map_err(|e| could_not("examine", &e))? {
            None => port.create_dir(path).map_err(|e| could_not("create", &e))?,
            Some(_) => {
                let found = verify_chain(port.as_mut(), path, FileKind::Dir, Some(path), false)
                    .and_then(|()| verify_tree(port.as_mut(), path, MAX_TREE))
                    .err();
                if let Some(found) = found.filter(|f| is_within(&f.path, path, style)) {
                    log::warn!(Dir = path, Found = found.to_string().as_str(), Purge = on_untrusted == OnUntrusted::Purge; "Helper: an existing folder is not protected");
                    match on_untrusted {
                        OnUntrusted::Refuse => return Err(Unusable::HelperFolderNotProtected(found)),
                        OnUntrusted::Purge => {
                            port.remove_tree(path).map_err(|e| could_not("remove", &e))?;
                            log::warn!(Dir = path; "Helper: an unprotected folder was removed and is made again as SYSTEM");
                            port.create_dir(path).map_err(|e| could_not("create", &e))?;
                        }
                    }
                }
            }
        }
        // 4. Owner and DACL on the folder itself, then the whole check.
        port.lock_dir(path).map_err(|e| could_not("lock", &e))?;
        verify_chain(port.as_mut(), path, FileKind::Dir, Some(path), false)
            .and_then(|()| verify_tree(port.as_mut(), path, MAX_TREE))
            .map_err(|u| match on_untrusted {
                OnUntrusted::Refuse => Unusable::HelperFolderNotProtected(u),
                OnUntrusted::Purge => could_not("verify after the lock", &u.describe()),
            })
    }

    /// Empties `dir` (links removed, never followed) and locks it again: the staging folder of one install, so what an earlier
    /// run left (a half-written package, a file its installer still held) cannot collide with this one. The way to it is
    /// checked first, like [`ensure_root`](Self::ensure_root).
    ///
    /// # Errors
    /// The folder could not be emptied or locked.
    pub fn reset_root(&self, dir: &Path) -> Result<(), Unusable> {
        let path = text(dir);
        // Check and make the way first: nothing is removed under a way that is not trusted.
        self.ensure_root(dir, OnUntrusted::Purge)?;
        self.port().remove_tree(&path).map_err(|e| Unusable::CouldNotLock {
            path: path.clone(),
            why: format!("emptying: {e}"),
        })?;
        self.ensure_root(dir, OnUntrusted::Purge)
    }

    /// Locks the data root, then `tools` and `staging` in it (see [`ensure_root`](Self::ensure_root)). The data root holds
    /// `config.json`, which the helper takes its orders from, so it is never purged at start (`data`).
    ///
    /// # Errors
    /// The first folder that could not be locked.
    pub fn prepare_folders(&self, data_dir: &Path, data: OnUntrusted) -> Result<(), Unusable> {
        self.ensure_root(data_dir, data)?;
        self.ensure_root(&data_dir.join("tools"), OnUntrusted::Purge)?;
        self.ensure_root(&data_dir.join("staging"), OnUntrusted::Purge)
    }

    /// Verifies `dir` (inside `root`) from the open objects: every component, the stricter rule inside `root`.
    ///
    /// # Errors
    /// The first component that is not protected.
    pub fn verify_dir(&self, dir: &Path, root: &Path) -> Result<(), Unusable> {
        let r = verify_chain(self.port().as_mut(), &text(dir), FileKind::Dir, Some(&text(root)), false);
        if let Err(u) = &r {
            log::warn!(Path = u.path.as_str(), Reason = u.reason.to_string().as_str(); "Helper: a folder is not protected; refused");
        }
        r.map_err(Unusable::HelperFolderNotProtected)
    }

    /// Verifies that an existing file of the data root (`audit.jsonl`, `policy.toml`, a pending `config.json.new`) is a
    /// protected regular file inside the verified root.
    ///
    /// # Errors
    /// The first component that is not protected.
    pub fn verify_file(&self, file: &Path, root: &Path) -> Result<(), Unusable> {
        verify_chain(self.port().as_mut(), &text(file), FileKind::File, Some(&text(root)), false)
            .map_err(Unusable::HelperFolderNotProtected)
    }

    /// Opens `program` for a run and verifies the chain to the file the open object really is. `root` is the helper's
    /// protected folder the program sits in (`None` for a program the OS ships, like `/usr/sbin/smartctl`, whose links are
    /// resolved first on Linux).
    ///
    /// # Errors
    /// Why the program is not run.
    pub fn open_checked(&self, program: &str, root: Option<&Path>) -> Result<Held, Unusable> {
        let refuse = |u: Untrusted| {
            log::warn!(Program = program, Path = u.path.as_str(), Reason = u.reason.to_string().as_str(); "Helper: a program is not protected; not run");
            Unusable::ProgramNotProtected(u)
        };
        let held = self.opener.open(program).map_err(|e| {
            log::warn!(Program = program, Error = e.to_string().as_str(); "Helper: a program could not be opened for a checked run; not run");
            let reason = match e.kind() {
                io::ErrorKind::NotFound => Reason::Missing,
                _ => Reason::Unreadable(e.to_string()),
            };
            Unusable::ProgramNotProtected(Untrusted {
                path: program.to_string(),
                reason,
            })
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

/// The most entries below a helper folder that are examined (a folder with more is untrusted).
const MAX_TREE: usize = 10_000;

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
        let held = self
            .guard
            .open_checked(&tool.program, self.root.as_deref())
            .map_err(PlatformError::from)?;
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
