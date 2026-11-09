//! The app-wide updater: the [`UpdatePhase`] machine with the work around it. Every step that touches the network or the
//! disk is a job body ([`Updater::run_check`], [`Updater::run_download`]) that the adapter submits to a job pool; the
//! `begin_*` methods only change the phase and return at once (rule 10). Results of work the user has moved on from (a
//! cancel, a newer check) are recognised by their [`Ticket`] and dropped.

use crate::error::{DEFAULT_MAX_DOWNLOAD_BYTES, UpdateError};
use crate::install::{InstallInfo, InstallScope, InstallerLock};
use crate::manifest::{self, MAX_MANIFEST_BYTES, Manifest, Platform, Target};
use crate::record::{FailedInstall, Handoff, RecordStore};
use crate::signature::{self, UpdateKey, VerifyError};
use crate::state::{self, Event, UpdateInfo, UpdatePhase};
use paddock_download::{Downloader, Get, Integrity, Request};
use paddock_jobs::{CancelReason, CancelToken, JobContext, JobError, Phase};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// The event every [`UpdateStatus`] is sent on, to every window (the updater is app-wide).
pub const STATUS_EVENT: &str = "paddock://updates";
/// The job phase of the signature check (bytes, total = the file's size).
pub const SIGNATURE_PHASE: &str = "signature";
/// The job phase of a check (items, total 1).
pub const CHECK_PHASE: &str = "check";
/// Longest wait for the release information's body.
const MANIFEST_TIMEOUT: Duration = Duration::from_secs(60);
/// Why an install is refused in a build without an [`Installer`].
const NO_INSTALLER: &str = "this build has no installer";
fn no_installer() -> UpdateError {
    UpdateError::InstallFailed {
        detail: NO_INSTALLER.into(),
    }
}

/// How often a blocking request is looked at for a cancel.
const TICK: Duration = Duration::from_millis(10);

/// A release channel: its id (`stable`, `beta`), its label and the address of its `latest.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    pub id: String,
    pub label: String,
    pub endpoint: String,
}

impl Channel {
    pub fn new(id: &str, label: &str, endpoint: &str) -> Channel {
        Channel {
            id: id.into(),
            label: label.into(),
            endpoint: endpoint.into(),
        }
    }
}

/// A channel on the wire (its address is not the UI's business; the host is in [`UpdateStatus::endpoint_host`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ChannelInfo {
    pub id: String,
    pub label: String,
}

/// Where the update preferences are kept: the shell's `pd.updates` settings section (`paddock-shell`), so the
/// settings are their one writer and source (rule 40). [`save`](PreferenceStore::save) stores them and puts them into
/// effect through [`Updater::apply_preferences`] (the store's own change path, which also serves a change made in the
/// Settings window). Without a store the preferences live in memory for the run.
pub trait PreferenceStore: Send + Sync {
    /// Stores `prefs` and applies them; an error refuses the change.
    fn save(&self, prefs: &Preferences) -> Result<(), UpdateError>;
}

/// The update preferences the user sets (kept by the [`PreferenceStore`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    /// Check a minute after start and then daily (off by default; nothing is downloaded without asking).
    pub auto_check: bool,
    /// A channel id of [`UpdateStatus::channels`].
    pub channel: String,
}

/// What the Updates UI shows, after every change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// The running version (rule 6: the UI shows this, never a copy of its own).
    pub current_version: String,
    pub phase: UpdatePhase,
    /// Seconds since 1970 of the last finished check.
    #[cfg_attr(test, ts(type = "number | null"))]
    pub last_checked: Option<u64>,
    pub auto_check: bool,
    pub channel: String,
    pub channels: Vec<ChannelInfo>,
    /// The host a check contacts (`None` in a build without updates).
    pub endpoint_host: Option<String>,
    pub releases_page: Option<String>,
    /// Whether this build can install what it downloads (an [`Installer`] is set). The dialog offers Download, "Restart to
    /// update" and "Install when the app quits" only when it can; a build that only notifies points to the releases page.
    pub can_install: bool,
    /// What the Updates UI shows about installing (scope, install on quit, the administrator text).
    pub install: InstallInfo,
}

/// A verified package, as handed to the [`Installer`].
#[derive(Debug, Clone, Copy)]
pub struct Package<'a> {
    /// The verified download.
    pub file: &'a Path,
    /// The release information as received.
    pub manifest: &'a serde_json::Value,
    /// The announced (and signed) version.
    pub version: &'a str,
    /// Base64 of the minisign signature file, as the release information has it.
    pub signature: &'a str,
    /// The announced size in bytes, when the release says.
    pub size: Option<u64>,
}

/// Installs a verified package. The adapter's is `tauri-plugin-updater` (it verifies the signature again); tests use a fake.
pub trait Installer: Send + Sync {
    /// Installs `package`. `relaunch`: the installer starts the new version (Windows' installer does that itself and ends
    /// this process).
    fn install(&self, package: &Package<'_>, relaunch: bool) -> Result<(), UpdateError>;
}

/// The app's services across updates (rule 24). An app without background services needs none.
pub trait ServiceHooks: Send + Sync {
    /// Before an install replaces the app: stop what runs from the app bundle. An error stops the install.
    fn before_install(&self) -> Result<(), String> {
        Ok(())
    }

    /// At the first start of a version (`previous`: the version whose services were last registered, `None` on a first
    /// install): re-register them (unregister, `launchctl bootout`, wait, register) and repair a stale registration. Runs
    /// once per version change; an error is logged and the hook runs again at the next start.
    fn on_version_change(&self, previous: Option<&str>, current: &str) -> Result<(), String>;
}

/// What [`Updater::install_at_exit`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitAction {
    /// Nothing was pending.
    Nothing,
    /// The update was installed; start the new version when `relaunch` (not on Windows: its installer does).
    Installed { relaunch: bool },
    /// Installing failed (recorded and shown at the next start); start the old version again when `relaunch`.
    Failed { relaunch: bool },
}

/// What the service hook did at start-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServicesOutcome {
    /// No hook, or this version's services are registered already.
    Unchanged,
    /// The hook ran for a version change and succeeded.
    Registered { previous: Option<String> },
    /// The hook failed; it runs again at the next start.
    Failed(String),
}

/// What a cancel needs from the adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelOutcome {
    /// Cancel this job; its body then ends the step.
    Job(u64),
    /// Done here (a verified download was dropped, or the work had not started): the status says where it is now.
    Done,
}

/// Identifies one check or download; results with an older ticket are dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticket(u64);

/// How the updater is set up.
#[derive(Debug, Clone)]
pub struct UpdaterConfig {
    /// The running version (the app's package version).
    pub current_version: String,
    pub target: Target,
    /// The first is the default. Empty: updates are not available.
    pub channels: Vec<Channel>,
    /// `None`: updates are not available in this build.
    pub key: Option<UpdateKey>,
    /// Where downloads go (the app's cache folder).
    pub download_dir: PathBuf,
    pub max_download_bytes: u64,
    pub releases_page: Option<String>,
    /// How long after start-up the automatic check may run (default [`state::AUTO_CHECK_DELAY_SECS`]).
    pub auto_check_delay_secs: u64,
    /// Where the app is installed (default per user).
    pub scope: InstallScope,
    /// The install folder of a per-machine app: where the hand-off's `updates\setup.log` is, said in the texts.
    pub install_dir: Option<PathBuf>,
    /// Whether the account is an administrator (`None`: not known); a per-machine copy says so to anyone else.
    pub administrator: Option<bool>,
}

impl UpdaterConfig {
    pub fn new(
        current_version: &str,
        key: Option<UpdateKey>,
        channels: Vec<Channel>,
        download_dir: impl Into<PathBuf>,
    ) -> Self {
        UpdaterConfig {
            current_version: current_version.into(),
            target: Target::current(None),
            channels,
            key,
            download_dir: download_dir.into(),
            max_download_bytes: DEFAULT_MAX_DOWNLOAD_BYTES,
            releases_page: None,
            auto_check_delay_secs: state::AUTO_CHECK_DELAY_SECS,
            scope: InstallScope::PerUser,
            install_dir: None,
            administrator: None,
        }
    }
}

/// The update found by the last check: its release information and this system's package.
#[derive(Debug, Clone)]
struct Found {
    manifest: Manifest,
    platform: Platform,
}

/// How long the app waits at start for an installer that is running (ADR 0053 section 4.1 step 4). A timeout means "unknown":
/// the hand-off record settles it at the next start.
pub const INSTALLER_WAIT_BOUND: Duration = Duration::from_secs(300);
/// How long one wait on the installer lock lasts in the background thread (so a stop is noticed); the wait goes on in slices.
pub const INSTALLER_WAIT_SLICE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnExit {
    Nothing,
    Install,
    InstallAndRelaunch,
}

struct Inner {
    phase: UpdatePhase,
    last_checked: Option<u64>,
    prefs: Preferences,
    found: Option<Found>,
    /// The verified package.
    ready: Option<PathBuf>,
    on_exit: OnExit,
    generation: u64,
}

type Emit = Box<dyn Fn(&UpdateStatus) + Send + Sync>;
type Now = Box<dyn Fn() -> u64 + Send + Sync>;

/// The app-wide updater (shared as `Arc<Updater>`).
pub struct Updater {
    config: UpdaterConfig,
    current: Option<semver::Version>,
    downloader: Arc<Downloader>,
    installer: Option<Arc<dyn Installer>>,
    hooks: Option<Arc<dyn ServiceHooks>>,
    installer_lock: Option<Arc<dyn InstallerLock>>,
    store: std::sync::OnceLock<Arc<dyn PreferenceStore>>,
    records: Arc<RecordStore>,
    inner: Mutex<Inner>,
    emit: Emit,
    now: Now,
    started: Instant,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Updater {
    /// The updater. It is [`UpdatePhase::Unavailable`] when the build has no key, no channel, or a running version that is
    /// not a semantic version; an install that failed when the app last quit is shown as the first phase.
    pub fn new(
        config: UpdaterConfig,
        downloader: Arc<Downloader>,
        records: Arc<RecordStore>,
        emit: Emit,
    ) -> Updater {
        let current = semver::Version::parse(config.current_version.trim_start_matches('v')).ok();
        let available = config.key.is_some() && !config.channels.is_empty() && current.is_some();
        let rec = records.load();
        // Shown once: forgotten as soon as it is the phase (only then is the record written).
        // Not shown when the version it names is installed by now (this version or older): a later success, by hand or by
        // another update, must not leave the earlier failure on screen.
        let failed = rec.failed_install.clone().filter(|f| {
            let named = semver::Version::parse(f.version.trim_start_matches('v')).ok();
            match (&named, &current) {
                (Some(n), Some(c)) => n > c,
                _ => true,
            }
        });
        if rec.failed_install.is_some() {
            let _ = records.update(|r| r.failed_install = None);
        }
        // A hand-off record naming a version that is not newer than this one is done with: the new version clears it. One
        // naming a newer version is settled by `settle_handoff` (the installer may still be running).
        let pending = rec.handoff.as_ref().and_then(|h| {
            let v = semver::Version::parse(h.version.trim_start_matches('v')).ok()?;
            (current.as_ref().is_some_and(|c| v > *c)).then(|| h.version.clone())
        });
        if rec.handoff.is_some() && pending.is_none() {
            let _ = records.update(|r| r.handoff = None);
        }
        let phase = match (available, failed, pending) {
            (false, _, _) => UpdatePhase::Unavailable,
            (true, Some(f), _) => UpdatePhase::Failed {
                error: f.error(),
                info: None,
            },
            (true, None, Some(version)) => UpdatePhase::InstallerRunning { version },
            (true, None, None) => UpdatePhase::Idle,
        };
        let channel = config
            .channels
            .first()
            .map(|c| c.id.clone())
            .unwrap_or_default();
        Updater {
            current,
            downloader,
            installer: None,
            hooks: None,
            installer_lock: None,
            store: std::sync::OnceLock::new(),
            inner: Mutex::new(Inner {
                phase,
                last_checked: rec.last_checked,
                prefs: Preferences {
                    auto_check: false,
                    channel,
                },
                found: None,
                ready: None,
                on_exit: OnExit::Nothing,
                generation: 0,
            }),
            config,
            records,
            emit,
            now: Box::new(unix_now),
            started: Instant::now(),
        }
    }

    pub fn with_installer(mut self, installer: Arc<dyn Installer>) -> Updater {
        self.installer = Some(installer);
        self
    }

    /// Whether this build can install a downloaded update (an [`Installer`] is set).
    pub fn can_install(&self) -> bool {
        self.installer.is_some()
    }

    pub fn with_service_hooks(mut self, hooks: Arc<dyn ServiceHooks>) -> Updater {
        self.hooks = Some(hooks);
        self
    }

    /// The installer lock of a per-machine app (`paddock_setup::installer_probe` and `wait_for_installer`, through the app).
    pub fn with_installer_lock(mut self, lock: Arc<dyn InstallerLock>) -> Updater {
        self.installer_lock = Some(lock);
        self
    }

    /// Keeps the preferences in `store` (until it applies stored ones, the updater starts with automatic checks off on
    /// the first channel). Set on the shared updater (the shell, once the store's section is registered); false when it
    /// already has one (the first store stays).
    pub fn set_preference_store(&self, store: Arc<dyn PreferenceStore>) -> bool {
        self.store.set(store).is_ok()
    }

    /// Another clock (seconds since 1970) for the last-checked time and the automatic check (tests).
    pub fn with_clock(mut self, now: impl Fn() -> u64 + Send + Sync + 'static) -> Updater {
        self.now = Box::new(now);
        self
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether this build updates itself.
    pub fn available(&self) -> bool {
        !matches!(self.lock().phase, UpdatePhase::Unavailable)
    }

    fn channel(&self, id: &str) -> Option<&Channel> {
        self.config.channels.iter().find(|c| c.id == id)
    }

    fn status_of(&self, inner: &Inner) -> UpdateStatus {
        let available = !matches!(inner.phase, UpdatePhase::Unavailable);
        let endpoint_host = self
            .channel(&inner.prefs.channel)
            .filter(|_| available)
            .and_then(|c| paddock_download::host_of(&c.endpoint).ok());
        UpdateStatus {
            current_version: self.config.current_version.clone(),
            phase: inner.phase.clone(),
            last_checked: inner.last_checked,
            auto_check: inner.prefs.auto_check,
            channel: inner.prefs.channel.clone(),
            channels: self
                .config
                .channels
                .iter()
                .map(|c| ChannelInfo {
                    id: c.id.clone(),
                    label: c.label.clone(),
                })
                .collect(),
            endpoint_host,
            releases_page: self.config.releases_page.clone(),
            can_install: self.installer.is_some(),
            install: InstallInfo::of(self.config.scope, self.config.administrator),
        }
    }

    pub fn status(&self) -> UpdateStatus {
        self.status_of(&self.lock())
    }

    /// Applies `event` under the lock (then `then` with the new state) and sends the status.
    fn apply(
        &self,
        event: Event,
        then: impl FnOnce(&mut Inner),
    ) -> Result<UpdateStatus, UpdateError> {
        let status = {
            let mut inner = self.lock();
            inner.phase = inner.phase.next(event)?;
            then(&mut inner);
            self.status_of(&inner)
        };
        (self.emit)(&status);
        Ok(status)
    }

    /// As [`apply`](Self::apply), only while `ticket` is the current work; a stale result changes nothing.
    fn apply_for(
        &self,
        ticket: Ticket,
        event: Event,
        then: impl FnOnce(&mut Inner),
    ) -> Option<UpdateStatus> {
        let status = {
            let mut inner = self.lock();
            if inner.generation != ticket.0 {
                return None;
            }
            inner.phase = inner.phase.next(event).ok()?;
            then(&mut inner);
            self.status_of(&inner)
        };
        (self.emit)(&status);
        Some(status)
    }

    fn begin(&self, event: Event) -> Result<(UpdateStatus, Ticket), UpdateError> {
        let mut ticket = Ticket(0);
        let status = self.apply(event, |inner| {
            inner.generation += 1;
            ticket = Ticket(inner.generation);
        })?;
        Ok((status, ticket))
    }

    /// Starts a check: the phase becomes Checking; the caller registers a job and runs [`run_check`](Self::run_check).
    pub fn begin_check(&self) -> Result<(UpdateStatus, Ticket), UpdateError> {
        self.begin(Event::Check)
    }

    /// Starts downloading the update found (refused with `InstallFailed` in a build without an [`Installer`]): the caller registers a job and runs [`run_download`](Self::run_download).
    /// `None` as the ticket: nothing to run (the update is over the size limit, which the status now says).
    pub fn begin_download(&self) -> Result<(UpdateStatus, Option<Ticket>), UpdateError> {
        // Out of turn is "busy" (or "not available") before it is "cannot install": nothing is downloaded that this
        // build could never install.
        self.require_installer(Event::Download)?;
        let size = self.lock().found.as_ref().and_then(|f| f.platform.size);
        let (status, ticket) = self.begin(Event::Download)?;
        if size.is_some_and(|s| s > self.config.max_download_bytes) {
            let limit = self.config.max_download_bytes;
            let failed = self.apply_for(
                ticket,
                Event::Downloaded(Err(UpdateError::TooLarge { limit })),
                |_| {},
            );
            return Ok((failed.unwrap_or(status), None));
        }
        Ok((status, Some(ticket)))
    }

    /// The work of `ticket` will not run (its job could not be started): back to where it began, as after a cancel.
    pub fn abandon(&self, ticket: Ticket) {
        if self
            .apply_for(ticket, Event::CheckCancelled, |_| {})
            .is_none()
        {
            self.apply_for(ticket, Event::Cancel, |_| {});
        }
    }

    /// The job that runs the work of `ticket` (shown in the status, so the UI can follow and cancel it).
    pub fn set_job(&self, ticket: Ticket, job: u64) {
        self.apply_for(ticket, Event::Job(job), |_| {});
    }

    /// The check, as a job body: fetches the channel's release information, validates it and picks this system's package.
    /// A failed check is the job's failure and the status's [`UpdatePhase::Failed`].
    pub fn run_check(&self, ctx: &JobContext, ticket: Ticket) -> Result<(), JobError> {
        let began = std::time::Instant::now();
        ctx.phase(Phase::items(CHECK_PHASE, "Checking for updates", 1));
        let endpoint = {
            let inner = self.lock();
            self.channel(&inner.prefs.channel)
                .map(|c| c.endpoint.clone())
        };
        let result = match endpoint {
            Some(url) => self.fetch(&url, ctx.cancel_token()),
            None => Err(Fetch::Failed(UpdateError::NotAvailable)),
        };
        let result = match result {
            Err(Fetch::Cancelled(reason)) => {
                self.apply_for(ticket, Event::CheckCancelled, |_| {});
                return Err(JobError::Cancelled(reason));
            }
            Err(Fetch::Failed(e)) => Err(e),
            Ok(m) => self.compare(m),
        };
        ctx.add(1);
        let ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX);
        match &result {
            Ok(Some((info, _))) => {
                log::info!(Version = info.version.as_str(), Current = self.config.current_version.as_str(), Ms = ms; "update.check: a newer version is available")
            }
            Ok(None) => {
                log::info!(Current = self.config.current_version.as_str(), Ms = ms; "update.check: up to date")
            }
            Err(e) => {
                log::warn!(Why = e.to_string().chars().take(300).collect::<String>().as_str(), Ms = ms; "update.check failed; the feed or the network did not answer as expected")
            }
        }
        let now = (self.now)();
        let outcome = result
            .as_ref()
            .map(|_| ())
            .map_err(|e| JobError::Failed(e.to_string()));
        let (info, found) = match result {
            Ok(Some((info, found))) => (Ok(Some(info)), Some(found)),
            Ok(None) => (Ok(None), None),
            Err(e) => (Err(e), None),
        };
        let applied = self.apply_for(ticket, Event::Checked(info), |inner| {
            inner.found = found;
            inner.last_checked = Some(now);
        });
        if applied.is_some() {
            let _ = self.records.update(|r| r.last_checked = Some(now));
        }
        outcome
    }

    /// A newer release than the running one, with this system's package; `None`: up to date.
    fn compare(&self, m: Manifest) -> Result<Option<(UpdateInfo, Found)>, UpdateError> {
        let Some(current) = &self.current else {
            return Err(UpdateError::NotAvailable);
        };
        if m.version <= *current {
            return Ok(None);
        }
        let platform = m
            .select(&self.config.target)
            .map(|(_, p)| p.clone())
            .map_err(|detail| UpdateError::NoPlatform { detail })?;
        let info = UpdateInfo {
            version: m.version.to_string(),
            current_version: self.config.current_version.clone(),
            notes: m.notes.clone(),
            pub_date: m.pub_date.clone(),
            size: platform.size,
        };
        Ok(Some((
            info,
            Found {
                manifest: m,
                platform,
            },
        )))
    }

    /// One GET of `latest.json` on its own thread, so a cancel never waits for the server.
    fn fetch(&self, url: &str, cancel: &CancelToken) -> Result<Manifest, Fetch> {
        let (tx, rx) = channel();
        let transport = self.downloader.transport().clone();
        let url = url.to_string();
        std::thread::Builder::new()
            .name("paddock-update-check".into())
            .spawn(move || {
                let _ = tx.send(get_manifest(transport.as_ref(), &url));
            })
            .map_err(|e| {
                Fetch::Failed(UpdateError::Unreachable {
                    detail: e.to_string(),
                })
            })?;
        loop {
            match rx.recv_timeout(TICK) {
                Ok(r) => return r.map_err(Fetch::Failed),
                Err(RecvTimeoutError::Timeout) => {
                    if let Err(c) = cancel.check() {
                        return Err(Fetch::Cancelled(c.0));
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(Fetch::Failed(UpdateError::Unreachable {
                        detail: "the request ended without an answer".into(),
                    }));
                }
            }
        }
    }

    /// Where the package of `found` is downloaded: `<download_dir>/update-<version>-<file name>`.
    fn package_path(&self, found: &Found) -> PathBuf {
        let name = found
            .platform
            .url
            .split(['?', '#'])
            .next()
            .and_then(|u| u.rsplit('/').next())
            .map(|n| {
                n.chars()
                    .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
                    .take(96)
                    .collect::<String>()
            })
            .filter(|n| !n.is_empty() && !n.starts_with('.'))
            .unwrap_or_else(|| "package".into());
        self.config
            .download_dir
            .join(format!("update-{}-{name}", found.manifest.version))
    }

    /// The download, as a job body: `paddock-download`'s phases (connect, download, verify) and then the signature check
    /// ([`SIGNATURE_PHASE`]). A refused signature deletes the file. A cancel leaves the partial file for a resume and the
    /// update available.
    pub fn run_download(&self, ctx: &JobContext, ticket: Ticket) -> Result<(), JobError> {
        let found = self.lock().found.clone();
        let (Some(found), Some(key)) = (found, self.config.key.clone()) else {
            let e = UpdateError::NotAvailable;
            self.apply_for(ticket, Event::Downloaded(Err(e.clone())), |_| {});
            return Err(JobError::Failed(e.to_string()));
        };
        let dest = self.package_path(&found);
        let began = std::time::Instant::now();
        log::info!(Version = found.manifest.version.to_string().as_str(), Size = found.platform.size.unwrap_or(0); "update.download started");
        let fail = |e: UpdateError| {
            let msg = e.to_string();
            log::warn!(Why = msg.chars().take(300).collect::<String>().as_str(), Ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX); "update.download failed; the update stays available to try again");
            self.apply_for(ticket, Event::Downloaded(Err(e)), |_| {});
            JobError::Failed(msg)
        };
        let cancelled = |reason: CancelReason| {
            self.apply_for(ticket, Event::Cancel, |_| {});
            JobError::Cancelled(reason)
        };
        let mut req = Request::new(&found.platform.url, &dest, Integrity::Unpinned)
            .map_err(|e| fail(UpdateError::from_download(&e)))?
            .max_bytes(self.config.max_download_bytes);
        if let Some(size) = found.platform.size {
            req = req.size(size);
        }
        let downloaded = match self.downloader.download(ctx, &req) {
            Ok(d) => d,
            Err(paddock_download::DownloadError::Cancelled(reason)) => {
                return Err(cancelled(reason));
            }
            Err(e) => return Err(fail(UpdateError::from_download(&e))),
        };
        ctx.phase(Phase::bytes(
            SIGNATURE_PHASE,
            "Checking the signature",
            Some(downloaded.bytes),
        ));
        match signature::verify_file(
            &key,
            &downloaded.path,
            &found.platform.signature,
            &found.manifest.version,
            Some(ctx),
        ) {
            Ok(()) => {
                log::info!(Bytes = downloaded.bytes, Ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX); "update.download verified; ready to install");
                self.apply_for(ticket, Event::Downloaded(Ok(())), |inner| {
                    inner.ready = Some(downloaded.path.clone())
                });
                Ok(())
            }
            Err(VerifyError::Cancelled) => {
                remove(&downloaded.path);
                Err(cancelled(
                    ctx.cancel_token().reason().unwrap_or(CancelReason::User),
                ))
            }
            Err(e) => {
                remove(&downloaded.path);
                Err(fail(e.into_update_error()))
            }
        }
    }

    /// Stops the check or download in flight (through its job), or drops a verified download; the update stays available.
    pub fn cancel(&self) -> Result<CancelOutcome, UpdateError> {
        let job = {
            let inner = self.lock();
            match inner.phase {
                UpdatePhase::Unavailable => return Err(UpdateError::NotAvailable),
                UpdatePhase::Checking { job: Some(id) }
                | UpdatePhase::Downloading { job: Some(id), .. } => Some(id),
                _ => None,
            }
        };
        if let Some(id) = job {
            return Ok(CancelOutcome::Job(id));
        }
        let mut dropped = None;
        if matches!(self.lock().phase, UpdatePhase::Checking { .. }) {
            self.apply(Event::CheckCancelled, |inner| inner.generation += 1)?;
        } else {
            self.apply(Event::Cancel, |inner| {
                inner.generation += 1;
                inner.on_exit = OnExit::Nothing;
                dropped = inner.ready.take();
            })?;
        }
        if let Some(path) = dropped {
            remove(&path);
        }
        Ok(CancelOutcome::Done)
    }

    /// Installs the verified update when the app quits (or not).
    pub fn install_on_quit(&self, enabled: bool) -> Result<UpdateStatus, UpdateError> {
        // A prompt after Quit, or at sign-out when nobody answers it, is never right (ADR 0053 decision O4).
        if self.config.scope == InstallScope::PerMachine {
            if enabled {
                return Err(UpdateError::PerMachine);
            }
            return Ok(self.status());
        }
        if enabled {
            self.require_installer(Event::InstallOnQuit(true))?;
        }
        self.apply(Event::InstallOnQuit(enabled), |inner| {
            inner.on_exit = if enabled {
                OnExit::Install
            } else {
                OnExit::Nothing
            };
        })
    }

    /// Refuses an install this build cannot do, at the request and not only at the exit where the user would meet it at the
    /// next start. A request out of turn (`event` is refused by the phase) is "busy" or "not available" first, as in
    /// [`begin_download`](Self::begin_download).
    fn require_installer(&self, event: Event) -> Result<(), UpdateError> {
        if self.installer.is_some() {
            return Ok(());
        }
        self.lock().phase.next(event)?;
        Err(no_installer())
    }

    /// Marks the verified update for installing with a relaunch; the caller then quits the app (its exit hook installs).
    pub fn prepare_restart(&self) -> Result<UpdateStatus, UpdateError> {
        self.require_installer(Event::Install)?;
        self.apply(Event::Install, |inner| {
            inner.on_exit = OnExit::InstallAndRelaunch
        })
    }

    /// Sets the preferences: through the [`PreferenceStore`] when there is one (it applies them), else at once. A channel
    /// change forgets the update found (refused while a download or install runs).
    pub fn set_preferences(&self, prefs: Preferences) -> Result<UpdateStatus, UpdateError> {
        let Some(store) = self.store.get().cloned() else {
            return self.apply_preferences(prefs);
        };
        self.check_preferences(&prefs)?;
        {
            let inner = self.lock();
            if inner.prefs.channel != prefs.channel
                && !matches!(inner.phase, UpdatePhase::Unavailable)
            {
                inner.phase.next(Event::Reset)?;
            }
        }
        store.save(&prefs)?;
        Ok(self.status())
    }

    fn check_preferences(&self, prefs: &Preferences) -> Result<(), UpdateError> {
        match self.channel(&prefs.channel) {
            Some(_) => Ok(()),
            None => Err(UpdateError::Invalid {
                detail: format!(
                    "There is no release channel '{}'.",
                    prefs.channel.chars().take(32).collect::<String>()
                ),
            }),
        }
    }

    /// Puts preferences into effect (the [`PreferenceStore`]'s change path; [`Updater::set_preferences`] without a store).
    /// A channel change forgets the update found (refused while a download or install runs).
    pub fn apply_preferences(&self, prefs: Preferences) -> Result<UpdateStatus, UpdateError> {
        if self.channel(&prefs.channel).is_none() {
            return Err(UpdateError::Invalid {
                detail: format!(
                    "There is no release channel '{}'.",
                    prefs.channel.chars().take(32).collect::<String>()
                ),
            });
        }
        let changed = self.lock().prefs.channel != prefs.channel;
        let status = {
            let mut inner = self.lock();
            if changed && !matches!(inner.phase, UpdatePhase::Unavailable) {
                inner.phase = inner.phase.next(Event::Reset)?;
                inner.found = None;
                inner.generation += 1;
            }
            if inner.prefs == prefs {
                return Ok(self.status_of(&inner));
            }
            inner.prefs = prefs;
            self.status_of(&inner)
        };
        (self.emit)(&status);
        Ok(status)
    }

    /// Whether the automatic check should run now.
    pub fn auto_check_due(&self) -> bool {
        let now = (self.now)();
        let inner = self.lock();
        let since = inner.last_checked.map(|t| now.saturating_sub(t));
        // The start-up delay is the config's: as if the app had run `AUTO_CHECK_DELAY_SECS - delay` seconds longer.
        let uptime = self.started.elapsed().as_secs()
            + state::AUTO_CHECK_DELAY_SECS.saturating_sub(self.config.auto_check_delay_secs);
        state::auto_check_due(inner.prefs.auto_check, &inner.phase, uptime, since)
    }

    /// Runs at exit: installs the update when asked to (after the services' `before_install`).
    pub fn install_at_exit(&self) -> ExitAction {
        let (on_exit, version, file, manifest, platform) = {
            let mut inner = self.lock();
            let version = match inner.phase.info() {
                Some(info)
                    if matches!(
                        inner.phase,
                        UpdatePhase::Ready { .. } | UpdatePhase::Installing { .. }
                    ) =>
                {
                    info.version.clone()
                }
                _ => return ExitAction::Nothing,
            };
            let on_exit = std::mem::replace(&mut inner.on_exit, OnExit::Nothing);
            let manifest = inner.found.as_ref().map(|f| f.manifest.raw.clone());
            let platform = inner.found.as_ref().map(|f| f.platform.clone());
            (on_exit, version, inner.ready.clone(), manifest, platform)
        };
        if on_exit == OnExit::Nothing {
            return ExitAction::Nothing;
        }
        let relaunch = on_exit == OnExit::InstallAndRelaunch;
        let result = (|| {
            let installer = self.installer.as_ref().ok_or_else(no_installer)?;
            let (Some(file), Some(manifest), Some(platform)) =
                (file.filter(|f| f.is_file()), manifest, platform)
            else {
                return Err(UpdateError::InstallFailed {
                    detail: "the verified download is no longer there".into(),
                });
            };
            // The staged file sat on disk since the download: check it against the family key again, before the services
            // are stopped, so a file changed in between is never installed (the installer's own check is not relied on).
            self.reverify_staged(&file, &platform, &version)?;
            if let Some(hooks) = &self.hooks {
                hooks
                    .before_install()
                    .map_err(|detail| UpdateError::InstallFailed {
                        detail: format!("the services could not be stopped: {detail}"),
                    })?;
            }
            if matches!(self.lock().phase, UpdatePhase::Ready { .. }) {
                let _ = self.apply(Event::Install, |_| {});
            }
            // A per-machine hand-off is written first: if the app is gone before it can see the outcome, the next start knows
            // an installer may be running or may have stopped (ADR 0053 section 4.1 step 2).
            let handoff = self.config.scope == InstallScope::PerMachine;
            if handoff {
                let version = version.clone();
                let _ = self
                    .records
                    .update(|r| r.handoff = Some(Handoff { version }));
            }
            let result = installer.install(
                &Package {
                    file: &file,
                    manifest: &manifest,
                    version: &version,
                    signature: &platform.signature,
                    size: platform.size,
                },
                relaunch,
            );
            if handoff && result.is_err() {
                // Refused or never started: no installer is running, so there is nothing to settle at the next start.
                let _ = self.records.update(|r| r.handoff = None);
            }
            result
        })();
        match result {
            // A per-machine install is restarted by the installer (`/R`, in the updating user's session only).
            Ok(()) => ExitAction::Installed {
                relaunch: relaunch
                    && !cfg!(windows)
                    && self.config.scope != InstallScope::PerMachine,
            },
            Err(e) => {
                let _ = self
                    .records
                    .update(|r| r.failed_install = Some(FailedInstall::of(&version, &e)));
                if !matches!(self.lock().phase, UpdatePhase::Installing { .. }) {
                    let _ = self.apply(Event::Install, |_| {});
                }
                let _ = self.apply(Event::InstallFailed(e), |_| {});
                // Start the old version again so the user sees what happened.
                ExitAction::Failed { relaunch }
            }
        }
    }

    /// Verifies the staged `file` against the release's signature and the family key again. On a mismatch the file is deleted
    /// (the next check offers the update again and the download fetches a fresh copy) and the cause is logged.
    fn reverify_staged(
        &self,
        file: &Path,
        platform: &Platform,
        version: &str,
    ) -> Result<(), UpdateError> {
        let began = std::time::Instant::now();
        let outcome = match (&self.config.key, semver::Version::parse(version)) {
            (None, _) => Err(UpdateError::SignatureInvalid {
                detail: "this build has no update key to check the staged file with".into(),
            }),
            (_, Err(e)) => Err(UpdateError::SignatureInvalid {
                detail: format!("the staged update's version cannot be read ({e})"),
            }),
            (Some(key), Ok(announced)) => {
                signature::verify_file(key, file, &platform.signature, &announced, None)
                    .map_err(VerifyError::into_update_error)
            }
        };
        match outcome {
            Ok(()) => {
                log::info!(Ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX); "update.install staged file verified again");
                Ok(())
            }
            Err(e) => {
                let why = e.to_string();
                log::warn!(Why = why.chars().take(300).collect::<String>().as_str(), Version = version; "update.install staged file failed its signature check; deleted, the next check downloads it again");
                remove(file);
                self.lock().ready = None;
                Err(e)
            }
        }
    }

    /// At start-up, when a hand-off record names a newer version than this one (the old version was started again by the
    /// installer's abort, or by the user while the installer runs): shows "An update is being installed" and waits for the
    /// installer through the lock, at most `bound` ([`INSTALLER_WAIT_BOUND`]). Once it has ended, a record still naming a newer
    /// version means the installer did not finish: the phase becomes Failed with "The installer stopped before it finished"
    /// and the record is cleared. A wait that runs out changes nothing: the next start settles it. Blocking: call it from a
    /// background thread. Returns whether the record was settled.
    pub fn settle_handoff(&self, bound: Duration) -> bool {
        if !matches!(self.lock().phase, UpdatePhase::InstallerRunning { .. }) {
            return false;
        }
        let gone = match &self.installer_lock {
            // Without a lock to look at, nothing can be running that this app knows of.
            None => true,
            Some(lock) => !lock.installing() || lock.wait(bound),
        };
        if !gone {
            return false;
        }
        let error = UpdateError::NotInstalled {
            reason: crate::install::installer_stopped(self.config.install_dir.as_deref()),
        };
        let _ = self.records.update(|r| r.handoff = None);
        self.apply(Event::InstallerEnded(error), |_| {}).is_ok()
    }

    /// Whether the phase is "an installer is running" (the background thread keeps waiting while it is).
    pub fn installer_running(&self) -> bool {
        matches!(self.lock().phase, UpdatePhase::InstallerRunning { .. })
    }

    /// At start-up: runs the services' hook once per version change (rule 24). Blocking (the hook waits for the service
    /// manager): call it from a background thread.
    pub fn on_start(&self) -> ServicesOutcome {
        let Some(hooks) = &self.hooks else {
            return ServicesOutcome::Unchanged;
        };
        let current = self.config.current_version.clone();
        let previous = self.records.load().services_version;
        if previous.as_deref() == Some(current.as_str()) {
            return ServicesOutcome::Unchanged;
        }
        match hooks.on_version_change(previous.as_deref(), &current) {
            Ok(()) => {
                let _ = self
                    .records
                    .update(|r| r.services_version = Some(current.clone()));
                ServicesOutcome::Registered { previous }
            }
            Err(e) => ServicesOutcome::Failed(e),
        }
    }
}

enum Fetch {
    Cancelled(CancelReason),
    Failed(UpdateError),
}

/// GETs `latest.json` (bounded) and validates it. 404 means there is no release.
fn get_manifest(
    transport: &dyn paddock_download::Transport,
    url: &str,
) -> Result<Manifest, UpdateError> {
    let get = Get {
        url,
        range: None,
        if_range: None,
        body_timeout: Some(MANIFEST_TIMEOUT),
        headers: &[],
    };
    let resp = transport
        .get(&get)
        .map_err(|e| UpdateError::from_transport(&e))?;
    match resp.status {
        200..=299 => {}
        404 => return Err(UpdateError::NoRelease),
        s => {
            return Err(UpdateError::Unreachable {
                detail: format!("the update server answered HTTP {s}"),
            });
        }
    }
    let mut body = Vec::new();
    resp.body
        .take(MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|e| UpdateError::Unreachable {
            detail: e.to_string(),
        })?;
    let text = String::from_utf8(body).map_err(|_| UpdateError::InvalidManifest {
        detail: "the release information is not UTF-8 text".into(),
    })?;
    manifest::parse(&text).map_err(|detail| UpdateError::InvalidManifest { detail })
}

fn remove(p: &Path) {
    let _ = std::fs::remove_file(p);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoNetwork;

    impl paddock_download::Transport for NoNetwork {
        fn get(
            &self,
            _: &Get<'_>,
        ) -> Result<paddock_download::Response, paddock_download::TransportError> {
            unreachable!("nothing is fetched")
        }
    }

    /// A ready update in a build without an installer cannot happen through the public steps (a download is refused first),
    /// so the install guards are defence in depth: forced here to prove they hold.
    #[test]
    fn a_ready_update_without_an_installer_is_refused_by_every_install_step() {
        let dir = tempfile::tempdir().unwrap();
        let key = UpdateKey::parse(
            &std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/key.pub"),
            )
            .unwrap(),
        )
        .unwrap();
        let config = UpdaterConfig::new(
            "1.0.0",
            Some(key),
            vec![Channel::new(
                "stable",
                "Stable",
                "https://updates.test/latest.json",
            )],
            dir.path().join("downloads"),
        );
        let u = Updater::new(
            config,
            Arc::new(Downloader::new(Arc::new(NoNetwork))),
            Arc::new(RecordStore::new(None)),
            Box::new(|_| {}),
        );
        u.lock().phase = UpdatePhase::Ready {
            info: UpdateInfo {
                version: "9.9.9".into(),
                current_version: "1.0.0".into(),
                notes: String::new(),
                pub_date: None,
                size: None,
            },
            install_on_quit: false,
        };
        let refused = |r: Result<UpdateStatus, UpdateError>| matches!(r, Err(UpdateError::InstallFailed { detail }) if detail.contains("no installer"));
        assert!(refused(u.prepare_restart()));
        assert!(refused(u.install_on_quit(true)));
        assert!(
            matches!(u.status().phase, UpdatePhase::Ready { .. }),
            "nothing moved"
        );
    }
}
