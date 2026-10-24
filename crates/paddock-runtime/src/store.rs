use crate::catalogue::{Pinned, is_folder_name};
use crate::{Result, RuntimeError};
use paddock_archive::{ArchiveError, Links, Options};
use paddock_download::{Downloader, Integrity, Request, phases};
use paddock_env::{AppDirs, VersionedFile};
use paddock_jobs::time::{Clock, SystemClock};
use paddock_jobs::{JobContext, Phase};
use serde::{Deserialize, Serialize};
use std::fs::TryLockError;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The marker file of a complete release folder.
pub const MARKER: &str = "runtime.json";
/// The store's state file.
pub const STATE_FILE: &str = "state.json";
/// How many quarantined releases are kept on disk (newest first); older ones are deleted.
pub const DEFAULT_KEEP_QUARANTINED: usize = 3;

const STATE_VERSION: u32 = 1;
const MARKER_FORMAT: u32 = 1;
const MAX_MARKER_BYTES: u64 = 64 * 1024;
const DOWNLOADS: &str = "downloads";
const QUARANTINE: &str = ".quarantine";
const STAGING: &str = ".staging-";
const LOCK: &str = ".lock";

/// What differs between runtimes: where the program is inside the unpacked archive and how to check that it runs.
pub trait RuntimeKind: Send + Sync {
    /// The executable inside the unpacked tree `root`, as a path relative to it; `None` when the archive has none.
    fn locate(&self, root: &Path) -> Option<PathBuf>;

    /// Runs the executable once (bounded) and says whether it works: `Ok` with what it reported (a version line), or
    /// `Err` with the reason. `version` is the pinned version, which a good self-test compares with the output.
    fn self_test(&self, executable: &Path, version: &str) -> std::result::Result<String, String>;
}

/// A release that was set aside: it failed its self-test, or the app found it broken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantineRecord {
    pub version: String,
    /// In words, for the user and a diagnostics report.
    pub reason: String,
    /// The folder under `.quarantine/` that holds it.
    pub folder: String,
    /// When, in milliseconds since the Unix epoch.
    pub at_ms: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct State {
    active: Option<String>,
    previous: Option<String>,
    quarantined: Vec<QuarantineRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Marker {
    format: u32,
    name: String,
    version: String,
    sha256: String,
    /// Relative to the release folder, `/`-separated.
    executable: String,
    self_test: String,
}

/// An installed release that passed its self-test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub name: String,
    pub version: String,
    /// The release folder.
    pub dir: PathBuf,
    /// The program to start.
    pub executable: PathBuf,
    /// What its self-test reported.
    pub self_test: String,
    /// The SHA-256 of the archive it came from.
    pub sha256: String,
}

/// The store's answer for a pinned build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Nothing usable is installed.
    NotInstalled,
    /// The pinned version is in use.
    Current(Installed),
    /// Another version is in use; installing the pinned one updates it.
    Outdated {
        installed: Installed,
        pinned: String,
    },
}

/// One runtime's folder: install, use, roll back, quarantine. Cheap to share behind an `Arc`; one install at a time.
pub struct RuntimeStore {
    root: PathBuf,
    name: String,
    state: VersionedFile<State>,
    keep_quarantined: usize,
    archive: Options,
    clock: Arc<dyn Clock>,
    busy: Mutex<()>,
}

impl std::fmt::Debug for RuntimeStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeStore")
            .field("root", &self.root)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// How runtime archives are unpacked: `paddock-archive`'s limits, links kept inside the tree (macOS frameworks link
/// `Versions/Current`), permissions from the archive.
fn default_archive_options() -> Options {
    Options {
        links: Links::Keep,
        ..Options::default()
    }
}

/// A cancel stays a cancel; everything else says what is wrong with the archive.
fn archive_error(e: ArchiveError) -> RuntimeError {
    match e {
        ArchiveError::Cancelled(r) => RuntimeError::Cancelled(r),
        e => RuntimeError::Archive(e.to_string()),
    }
}

/// Another holder is [`RuntimeError::Busy`]; anything else is an I/O error.
fn lock_error(e: TryLockError, path: &Path) -> RuntimeError {
    match e {
        TryLockError::WouldBlock => RuntimeError::Busy,
        TryLockError::Error(e) => RuntimeError::io("lock", path, e),
    }
}

/// Joins a `/`-separated relative path below `base`, refusing anything that is not plain names.
fn below(base: &Path, rel: &str) -> Option<PathBuf> {
    let mut out = base.to_path_buf();
    for part in rel.split('/') {
        let p = Path::new(part);
        match p.components().next() {
            Some(Component::Normal(_)) if p.components().count() == 1 => out.push(part),
            _ => return None,
        }
    }
    Some(out)
}

/// `rel` as a `/`-separated string of plain names.
fn slashed(rel: &Path) -> Option<String> {
    let parts: Option<Vec<&str>> = rel
        .components()
        .map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    parts.filter(|p| !p.is_empty()).map(|p| p.join("/"))
}

fn remove_dir(path: &Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(RuntimeError::io("remove", path, e)),
    }
}

impl RuntimeStore {
    /// The store of runtime `name` in `root`. Nothing is created until an install.
    pub fn new(root: impl Into<PathBuf>, name: &str) -> Result<RuntimeStore> {
        if !is_folder_name(name) {
            return Err(RuntimeError::Catalogue(format!(
                "the name '{name}' is not a valid folder name"
            )));
        }
        let root = root.into();
        Ok(RuntimeStore {
            state: VersionedFile::new(root.join(STATE_FILE), STATE_VERSION),
            root,
            name: name.to_string(),
            keep_quarantined: DEFAULT_KEEP_QUARANTINED,
            archive: default_archive_options(),
            clock: Arc::new(SystemClock),
            busy: Mutex::new(()),
        })
    }

    /// The store of runtime `name` in the app's data folder: `<data>/runtimes/<name>`.
    pub fn in_app_data(dirs: &AppDirs, name: &str) -> Result<RuntimeStore> {
        RuntimeStore::new(dirs.data().join("runtimes").join(name), name)
    }

    /// Uses `clock` for the quarantine times (tests pass a fake clock).
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// How archives are unpacked (limits, links, permissions); see `paddock_archive::Options`.
    #[must_use]
    pub fn with_archive_options(mut self, options: Options) -> Self {
        self.archive = options;
        self
    }

    /// How many quarantined releases stay on disk (at least 1).
    #[must_use]
    pub fn keep_quarantined(mut self, n: usize) -> Self {
        self.keep_quarantined = n.max(1);
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    fn load(&self) -> State {
        let loaded = self.state.load();
        if let Some(p) = &loaded.problem {
            log::warn!(
                "The {} runtime state was not usable ({p:?}); what is on disk is used",
                self.name
            );
        }
        loaded.value
    }

    fn save(&self, state: &State) -> Result<()> {
        self.state.save(state).map_err(|e| RuntimeError::Io {
            op: "save",
            path: self.state.path().to_path_buf(),
            source: std::io::Error::other(e.to_string()),
        })
    }

    fn release_dir(&self, version: &str) -> PathBuf {
        self.root.join(version)
    }

    /// The release `version` when its folder is complete (marker present, matching, executable there).
    fn read_release(&self, version: &str) -> Option<Installed> {
        if !is_folder_name(version) {
            return None;
        }
        let dir = self.release_dir(version);
        let file = std::fs::File::open(dir.join(MARKER)).ok()?;
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::Read::take(file, MAX_MARKER_BYTES), &mut text)
            .ok()?;
        let m: Marker = serde_json::from_str(&text).ok()?;
        if m.format != MARKER_FORMAT || m.name != self.name || m.version != version {
            return None;
        }
        let executable = below(&dir, &m.executable).filter(|p| p.is_file())?;
        Some(Installed {
            name: m.name,
            version: m.version,
            dir,
            executable,
            self_test: m.self_test,
            sha256: m.sha256,
        })
    }

    /// The versions with a complete release folder, sorted.
    pub fn installed_versions(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(&self.root)
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| e.file_name().to_str().map(str::to_string))
                    .filter(|n| self.read_release(n).is_some())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    /// The release in use: the one the state names, else the one before it, else (a lost or damaged state file) the
    /// newest complete folder. `None` when nothing usable is installed.
    pub fn active(&self) -> Option<Installed> {
        let s = self.load();
        s.active
            .as_deref()
            .and_then(|v| self.read_release(v))
            .or_else(|| s.previous.as_deref().and_then(|v| self.read_release(v)))
            .or_else(|| {
                let quarantined: Vec<&str> =
                    s.quarantined.iter().map(|q| q.version.as_str()).collect();
                self.installed_versions()
                    .into_iter()
                    .rev()
                    .find(|v| !quarantined.contains(&v.as_str()))
                    .and_then(|v| self.read_release(&v))
            })
    }

    /// The release kept for [`rollback`](Self::rollback), when it is complete.
    pub fn previous(&self) -> Option<Installed> {
        self.load()
            .previous
            .as_deref()
            .and_then(|v| self.read_release(v))
    }

    /// The releases set aside, newest last.
    pub fn quarantined(&self) -> Vec<QuarantineRecord> {
        self.load().quarantined
    }

    /// Whether `pinned` is in use, another version is, or nothing is.
    pub fn status(&self, pinned: &Pinned) -> Status {
        match self.active() {
            None => Status::NotInstalled,
            Some(i) if i.version == pinned.version => Status::Current(i),
            Some(installed) => Status::Outdated {
                installed,
                pinned: pinned.version.clone(),
            },
        }
    }

    /// Holds the store for one install, rollback or clean-up: in this process and, through `.lock`, in others.
    fn hold(&self) -> Result<(std::sync::MutexGuard<'_, ()>, std::fs::File)> {
        let guard = match self.busy.try_lock() {
            Ok(g) => g,
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return Err(RuntimeError::Busy),
        };
        std::fs::create_dir_all(&self.root)
            .map_err(|e| RuntimeError::io("create", &self.root, e))?;
        let path = self.root.join(LOCK);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| RuntimeError::io("open", &path, e))?;
        file.try_lock().map_err(|e| lock_error(e, &path))?;
        Ok((guard, file))
    }

    /// Downloads, unpacks, tests and installs `pinned` as the job `ctx` (phases: the download's own, then `unpack`,
    /// `self-test`, `install`). The release in use stays until the new one passed its self-test; it then becomes the
    /// rollback target and older releases are removed. A release that fails is moved to the quarantine folder. Cancel
    /// works in every phase up to the final swap and keeps the partial download for the next run. Installing the version
    /// already in use returns it at once; a complete folder of it that is not in use is tested and taken without a download.
    pub fn install(
        &self,
        ctx: &JobContext,
        downloader: &Downloader,
        pinned: &Pinned,
        kind: &dyn RuntimeKind,
    ) -> Result<Installed> {
        if pinned.name != self.name {
            return Err(RuntimeError::Catalogue(format!(
                "the build is for {}, not for {}",
                pinned.name, self.name
            )));
        }
        let _hold = self.hold()?;
        let state = self.load();
        if state.active.as_deref() == Some(pinned.version.as_str())
            && let Some(i) = self.read_release(&pinned.version)
        {
            return Ok(i);
        }
        if let Some(existing) = self.read_release(&pinned.version) {
            let exe = existing.executable.clone();
            let text = self.self_test_phase(ctx, kind, &exe, &pinned.version);
            return match text {
                Ok(_) => {
                    self.activate(ctx, &pinned.version)?;
                    Ok(self.read_release(&pinned.version).unwrap_or(existing))
                }
                Err(why) => Err(self.set_aside(&existing.dir, &pinned.version, why)?),
            };
        }
        let downloads = self.root.join(DOWNLOADS);
        std::fs::create_dir_all(&downloads)
            .map_err(|e| RuntimeError::io("create", &downloads, e))?;
        let archive = downloads.join(&pinned.file_name);
        let req = Request::new(&pinned.url, &archive, Integrity::Sha256(pinned.sha256))?
            .size(pinned.size)
            .max_bytes(pinned.size);
        let got = downloader.download(ctx, &req)?;
        let staging = self.root.join(format!("{STAGING}{}", pinned.version));
        remove_dir(&staging)?;
        let result =
            self.unpack_test_install(ctx, kind, pinned, &got.path, &staging, &got.sha256.to_hex());
        if !matches!(result, Err(RuntimeError::SelfTestFailed { .. })) {
            // A failed self-test moved the folder away already; anything else leaves no staging behind.
            let _ = remove_dir(&staging);
        }
        if result.is_ok()
            || matches!(
                result,
                Err(RuntimeError::SelfTestFailed { .. } | RuntimeError::NoExecutable)
            )
        {
            // The archive was used (or can never work): it is not kept.
            let _ = std::fs::remove_file(&got.path);
        }
        result
    }

    fn unpack_test_install(
        &self,
        ctx: &JobContext,
        kind: &dyn RuntimeKind,
        pinned: &Pinned,
        archive: &Path,
        staging: &Path,
        sha256: &str,
    ) -> Result<Installed> {
        paddock_archive::extract(archive, staging, &self.archive, ctx).map_err(archive_error)?;
        let rel = kind.locate(staging).ok_or(RuntimeError::NoExecutable)?;
        let rel_text = slashed(&rel).ok_or(RuntimeError::NoExecutable)?;
        let exe = staging.join(&rel);
        if !exe.is_file() {
            return Err(RuntimeError::NoExecutable);
        }
        let text = match self.self_test_phase(ctx, kind, &exe, &pinned.version) {
            Ok(t) => t,
            Err(reason) => return Err(self.set_aside(staging, &pinned.version, reason)?),
        };
        ctx.cancel_token().check()?;
        let marker = Marker {
            format: MARKER_FORMAT,
            name: self.name.clone(),
            version: pinned.version.clone(),
            sha256: sha256.to_string(),
            executable: rel_text,
            self_test: text,
        };
        let json = serde_json::to_vec_pretty(&marker)
            .map_err(|e| RuntimeError::io("write", staging, std::io::Error::other(e)))?;
        let marker_path = staging.join(MARKER);
        std::fs::write(&marker_path, json)
            .map_err(|e| RuntimeError::io("write", &marker_path, e))?;
        let dir = self.release_dir(&pinned.version);
        // From here the swap is not interrupted: the new folder is complete before the state names it.
        ctx.phase(Phase::items(phases::INSTALL, phases::INSTALL_LABEL, 1));
        let _guard = ctx.uncancellable("the new version is being put in place");
        remove_dir(&dir)?;
        std::fs::rename(staging, &dir).map_err(|e| RuntimeError::io("move into place", &dir, e))?;
        self.activate_now(&pinned.version)?;
        ctx.set(1);
        self.read_release(&pinned.version)
            .ok_or(RuntimeError::NoExecutable)
    }

    /// The `self-test` phase: one item.
    fn self_test_phase(
        &self,
        ctx: &JobContext,
        kind: &dyn RuntimeKind,
        exe: &Path,
        version: &str,
    ) -> std::result::Result<String, String> {
        ctx.phase(Phase::items(phases::SELF_TEST, phases::SELF_TEST_LABEL, 1));
        let r = kind.self_test(exe, version);
        ctx.set(1);
        r
    }

    fn activate(&self, ctx: &JobContext, version: &str) -> Result<()> {
        ctx.phase(Phase::items(phases::INSTALL, phases::INSTALL_LABEL, 1));
        self.activate_now(version)?;
        ctx.set(1);
        Ok(())
    }

    /// Makes `version` the one in use, keeps the one before it for a rollback and removes older releases.
    fn activate_now(&self, version: &str) -> Result<()> {
        let mut s = self.load();
        let old = s
            .active
            .take()
            .filter(|a| a != version && self.read_release(a).is_some());
        s.previous = old.or(s
            .previous
            .take()
            .filter(|p| p != version && self.read_release(p).is_some()));
        s.active = Some(version.to_string());
        self.save(&s)?;
        for v in self.installed_versions() {
            if Some(&v) != s.active.as_ref() && Some(&v) != s.previous.as_ref() {
                let _ = remove_dir(&self.release_dir(&v));
            }
        }
        Ok(())
    }

    /// Moves `folder` (a staging folder or a release) to the quarantine and records it; returns the error to report.
    fn set_aside(&self, folder: &Path, version: &str, reason: String) -> Result<RuntimeError> {
        self.move_to_quarantine(folder, version, &reason)?;
        log::warn!(
            "The {} runtime {version} was set aside: {reason}",
            self.name
        );
        Ok(RuntimeError::SelfTestFailed {
            version: version.to_string(),
            reason,
        })
    }

    fn move_to_quarantine(&self, folder: &Path, version: &str, reason: &str) -> Result<()> {
        let q = self.root.join(QUARANTINE);
        std::fs::create_dir_all(&q).map_err(|e| RuntimeError::io("create", &q, e))?;
        let at_ms = self.clock.wall_ms();
        let mut name = format!("{version}-{at_ms}");
        let mut n = 1;
        while q.join(&name).exists() {
            name = format!("{version}-{at_ms}-{n}");
            n += 1;
        }
        let to = q.join(&name);
        std::fs::rename(folder, &to).map_err(|e| RuntimeError::io("set aside", folder, e))?;
        let mut s = self.load();
        if s.active.as_deref() == Some(version) {
            s.active = s.previous.take().filter(|p| self.read_release(p).is_some());
        }
        if s.previous.as_deref() == Some(version) {
            s.previous = None;
        }
        s.quarantined.push(QuarantineRecord {
            version: version.to_string(),
            reason: reason.to_string(),
            folder: name,
            at_ms,
        });
        let drop = s.quarantined.len().saturating_sub(self.keep_quarantined);
        for old in s.quarantined.drain(..drop) {
            let _ = remove_dir(&q.join(&old.folder));
        }
        self.save(&s)
    }

    /// Sets the release `version` aside because the app found it broken (it crashed at start, say). When it was in use,
    /// the previous release takes over and is returned. The release is not used again until it is installed afresh.
    pub fn quarantine(&self, version: &str, reason: &str) -> Result<Option<Installed>> {
        let _hold = self.hold()?;
        let dir = self.release_dir(version);
        if !is_folder_name(version) || !dir.is_dir() {
            return Err(RuntimeError::NotInstalled(format!(
                "{} {version}",
                self.name
            )));
        }
        self.move_to_quarantine(&dir, version, reason)?;
        log::warn!(
            "The {} runtime {version} was set aside: {reason}",
            self.name
        );
        Ok(self.active())
    }

    /// Goes back to the release before the one in use, after it passed its self-test again; the one in use becomes the
    /// rollback target. A previous release that fails is set aside.
    pub fn rollback(&self, kind: &dyn RuntimeKind) -> Result<Installed> {
        let _hold = self.hold()?;
        let s = self.load();
        let prev = s
            .previous
            .as_deref()
            .and_then(|v| self.read_release(v))
            .ok_or_else(|| {
                RuntimeError::NotInstalled(format!("a previous {} version", self.name))
            })?;
        if let Err(reason) = kind.self_test(&prev.executable, &prev.version) {
            return Err(self.set_aside(&prev.dir, &prev.version, reason)?);
        }
        let mut s = s;
        s.previous = s.active.take();
        s.active = Some(prev.version.clone());
        self.save(&s)?;
        Ok(prev)
    }

    /// Removes the release `version`; the one in use is refused.
    pub fn remove(&self, version: &str) -> Result<()> {
        let _hold = self.hold()?;
        if self.active().is_some_and(|a| a.version == version) {
            return Err(RuntimeError::InUse(version.to_string()));
        }
        let dir = self.release_dir(version);
        if !is_folder_name(version) || !dir.is_dir() {
            return Err(RuntimeError::NotInstalled(format!(
                "{} {version}",
                self.name
            )));
        }
        remove_dir(&dir)?;
        let mut s = self.load();
        if s.previous.as_deref() == Some(version) {
            s.previous = None;
            self.save(&s)?;
        }
        Ok(())
    }

    /// Clean-up at start-up: staging folders of an interrupted install, incomplete release folders, and downloads of
    /// other archives than `keep`'s (whose partial download is kept for a resume). Skipped (0) while an install runs.
    /// Returns how many entries were removed.
    pub fn sweep(&self, keep: Option<&Pinned>) -> Result<usize> {
        let Ok(_hold) = self.hold() else { return Ok(0) };
        let s = self.load();
        let mut removed = 0;
        let entries: Vec<std::fs::DirEntry> = std::fs::read_dir(&self.root)
            .map(|rd| rd.flatten().collect())
            .unwrap_or_default();
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            let stale_staging = name.starts_with(STAGING);
            let incomplete = is_folder_name(&name)
                && path.is_dir()
                && name != DOWNLOADS
                && self.read_release(&name).is_none()
                && s.active.as_deref() != Some(name.as_str());
            if stale_staging || incomplete {
                remove_dir(&path)?;
                removed += 1;
            }
        }
        let downloads = self.root.join(DOWNLOADS);
        let files: Vec<std::fs::DirEntry> = std::fs::read_dir(&downloads)
            .map(|rd| rd.flatten().collect())
            .unwrap_or_default();
        for f in files {
            let name = f.file_name().to_string_lossy().into_owned();
            if keep.is_some_and(|k| name.starts_with(&k.file_name)) {
                continue;
            }
            std::fs::remove_file(f.path()).map_err(|e| RuntimeError::io("remove", f.path(), e))?;
            removed += 1;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_store_is_held_once_and_survives_a_poisoned_lock() {
        let d = tempfile::tempdir().unwrap();
        let s = RuntimeStore::new(d.path().join("r"), "tool").unwrap();
        let h = s.hold().unwrap();
        assert!(matches!(s.hold(), Err(RuntimeError::Busy)));
        drop(h);
        let _ = std::thread::scope(|sc| {
            sc.spawn(|| {
                let _g = s.busy.lock();
                panic!("poison the lock");
            })
            .join()
        });
        assert!(s.hold().is_ok(), "a poisoned lock is recovered");
        assert!(matches!(
            lock_error(TryLockError::WouldBlock, d.path()),
            RuntimeError::Busy
        ));
        let e = lock_error(TryLockError::Error(std::io::Error::other("x")), d.path());
        assert!(matches!(e, RuntimeError::Io { op: "lock", .. }));
        assert_eq!(s.read_release(".."), None);
        let file = d.path().join("f");
        std::fs::write(&file, b"x").unwrap();
        assert!(remove_dir(&file).is_err(), "a file is not a folder");
        assert!(remove_dir(&d.path().join("none")).is_ok());
        let cancelled = archive_error(ArchiveError::Cancelled(paddock_jobs::CancelReason::User));
        assert!(matches!(cancelled, RuntimeError::Cancelled(_)));
        assert!(matches!(
            archive_error(ArchiveError::Duplicate("a".into())),
            RuntimeError::Archive(_)
        ));
    }

    #[test]
    fn relative_paths_are_plain_names_only() {
        let base = Path::new("/r");
        assert_eq!(below(base, "a/b"), Some(PathBuf::from("/r/a/b")));
        for bad in ["", "a//b", "../x", "/x", "a/./b", "a/.."] {
            assert_eq!(below(base, bad), None, "{bad}");
        }
        assert_eq!(slashed(Path::new("a/b")), Some("a/b".into()));
        assert_eq!(slashed(Path::new("")), None);
        assert_eq!(slashed(Path::new("../a")), None);
    }
}
