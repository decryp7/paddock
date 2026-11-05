//! The Tauri adapter of `paddock-updater` (feature `tauri`): the managed [`Updates`] state built by [`UpdatesBuilder`],
//! the commands of [`COMMANDS`](crate::COMMANDS), the status event, the start-up work (services' hook, automatic check) and
//! the exit hook that installs.
//!
//! ```ignore
//! .setup(|app| {
//!     let updates = paddock_updater::tauri::UpdatesBuilder::new("Pitwall")
//!         .public_key(option_env!("TAURI_UPDATER_PUBKEY"))          // a CI secret; else `plugins.updater.pubkey`
//!         .releases_page("https://dev.decryptology.net/decryp7/pitwall/releases")
//!         .changelog(include_str!("../../CHANGELOG.md"))
//!         .legal("notices", "Third-party notices", include_str!("../resources/THIRD_PARTY_NOTICES.md"))
//!         .service_hooks(Arc::new(PitwallService))                   // rule 24, apps with a background service
//!         .installer(Arc::new(paddock_updater::tauri::PluginInstaller::new(app.handle().clone(), KEY))); // feature `install`
//!     paddock_updater::tauri::install(app.handle(), updates)?;
//!     Ok(())
//! })
//! .invoke_handler(paddock_commands::tauri::Guard::new(&paddock_updater::COMMANDS).wrap(paddock_updater::tauri::invoke_handler()))
//! .run(|app, event| if let tauri::RunEvent::Exit = event { paddock_updater::tauri::on_exit(app); })
//! ```
//!
//! The release address and the key come from the builder, else from `tauri.conf.json` (`plugins.updater.endpoints[0]`
//! as the `stable` channel, `plugins.updater.pubkey`). Without a valid key or an address the updater is
//! [`UpdatePhase::Unavailable`] for good: nothing is checked, nothing unsigned is ever installed. Commands return at once:
//! a check and a download are jobs on the app's `paddock-jobs` registry (cancel them with `job_cancel` or
//! `updates_cancel`), and every change is an [`UpdateStatus`] on [`STATUS_EVENT`].

mod native;
#[cfg(feature = "install")]
pub use native::PluginInstaller;
#[cfg(all(windows, feature = "install"))]
pub use native::{ShellElevator, is_administrator};

#[cfg(doc)]
use crate::UpdatePhase;
use crate::{
    About, AboutInfo, CancelOutcome, Channel, ExitAction, INSTALLER_WAIT_BOUND,
    INSTALLER_WAIT_SLICE, InstallScope, Installer, InstallerLock, Preferences, RecordStore,
    STATUS_EVENT, ServiceHooks, ServicesOutcome, Target, UpdateError, UpdateErrorDto, UpdateKey,
    UpdateStatus, Updater, UpdaterConfig, WhatsNew, WhatsNewDecision,
};
use ::tauri::ipc::Invoke;
use ::tauri::{AppHandle, Emitter, Manager, Runtime};
use paddock_commands::DialogGrant;
use paddock_commands::tauri::Guard;
use paddock_download::Downloader;
use paddock_jobs::{JobId, JobPool, JobRegistry, Workers};
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// How often the background thread asks whether the automatic check is due.
pub const AUTO_CHECK_POLL: Duration = Duration::from_secs(60);

/// The managed state (as `Arc<Updates>`): the updater, About, and where their work runs.
pub struct Updates {
    pub updater: Arc<Updater>,
    pub about: Arc<About>,
    jobs: Arc<JobRegistry>,
    pool: Arc<JobPool>,
    stopped: Arc<AtomicBool>,
}

impl Updates {
    /// Stops the background thread at its next wake-up (at exit; tests).
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }

    /// Starts a check as a job; returns the status (Checking with its job) at once.
    pub fn start_check(&self) -> Result<UpdateStatus, UpdateError> {
        let (_, ticket) = self.updater.begin_check()?;
        let ctx = self
            .jobs
            .register("update-check", "Checking for updates", None);
        self.updater.set_job(ticket, ctx.id().0);
        let updater = self.updater.clone();
        if self
            .pool
            .spawn_job(ctx.clone(), move |ctx| updater.run_check(ctx, ticket))
            .is_err()
        {
            self.jobs.cancel(ctx.id());
            self.updater.abandon(ticket);
        }
        Ok(self.updater.status())
    }

    /// Starts downloading the update found as a job; returns the status (Downloading with its job) at once.
    pub fn start_download(&self) -> Result<UpdateStatus, UpdateError> {
        let (status, ticket) = self.updater.begin_download()?;
        let Some(ticket) = ticket else {
            return Ok(status);
        };
        let version = status
            .phase
            .info()
            .map(|i| i.version.clone())
            .unwrap_or_default();
        let ctx = self
            .jobs
            .register("update-download", "Downloading the update", Some(&version));
        self.updater.set_job(ticket, ctx.id().0);
        let updater = self.updater.clone();
        if self
            .pool
            .spawn_job(ctx.clone(), move |ctx| updater.run_download(ctx, ticket))
            .is_err()
        {
            self.jobs.cancel(ctx.id());
            self.updater.abandon(ticket);
        }
        Ok(self.updater.status())
    }

    /// Cancels the check or download in flight (its job), or drops a verified download.
    pub fn cancel(&self) -> Result<UpdateStatus, UpdateError> {
        if let CancelOutcome::Job(id) = self.updater.cancel()? {
            self.jobs.cancel(JobId(id));
        }
        Ok(self.updater.status())
    }
}

/// Builds [`Updates`] (see the module example).
pub struct UpdatesBuilder {
    app_name: String,
    channels: Vec<Channel>,
    keys: Vec<Option<String>>,
    releases_page: Option<String>,
    homepage: Option<String>,
    changelog: Cow<'static, str>,
    legal: Vec<(String, String, Cow<'static, str>)>,
    build: (Option<String>, Option<String>),
    hooks: Option<Arc<dyn ServiceHooks>>,
    installer: Option<Arc<dyn Installer>>,
    installer_kind: Option<String>,
    scope: InstallScope,
    install_dir: Option<PathBuf>,
    administrator: Option<bool>,
    installer_lock: Option<Arc<dyn InstallerLock>>,
    max_download_bytes: Option<u64>,
    auto_check_delay_secs: Option<u64>,
    downloader: Option<Arc<Downloader>>,
    config_dir: Option<PathBuf>,
    cache_dir: Option<PathBuf>,
    background: bool,
}

impl UpdatesBuilder {
    pub fn new(app_name: &str) -> UpdatesBuilder {
        UpdatesBuilder {
            app_name: app_name.into(),
            channels: Vec::new(),
            keys: Vec::new(),
            releases_page: None,
            homepage: None,
            changelog: Cow::Borrowed(""),
            legal: Vec::new(),
            build: (None, None),
            hooks: None,
            installer: None,
            installer_kind: None,
            scope: InstallScope::PerUser,
            install_dir: None,
            administrator: None,
            installer_lock: None,
            max_download_bytes: None,
            auto_check_delay_secs: None,
            downloader: None,
            config_dir: None,
            cache_dir: None,
            background: true,
        }
    }

    /// A release channel (the first is the default). Without one, `plugins.updater.endpoints[0]` is the `stable` channel.
    pub fn channel(mut self, id: &str, label: &str, endpoint: &str) -> Self {
        self.channels.push(Channel::new(id, label, endpoint));
        self
    }

    /// The update public key (Tauri's format) given to this build, e.g. `option_env!("TAURI_UPDATER_PUBKEY")`; it comes
    /// before the configured `plugins.updater.pubkey`. May be called more than once: the first non-empty key wins.
    pub fn public_key(mut self, key: Option<&str>) -> Self {
        self.keys.push(key.map(str::to_string));
        self
    }

    pub fn releases_page(mut self, url: &str) -> Self {
        self.releases_page = Some(url.into());
        self
    }

    pub fn homepage(mut self, url: &str) -> Self {
        self.homepage = Some(url.into());
        self
    }

    /// The app's `CHANGELOG.md` for What's New.
    pub fn changelog(mut self, text: impl Into<Cow<'static, str>>) -> Self {
        self.changelog = text.into();
        self
    }

    /// A legal document served by `about_legal` (its id is checked by [`install`]).
    pub fn legal(mut self, id: &str, title: &str, text: impl Into<Cow<'static, str>>) -> Self {
        self.legal.push((id.into(), title.into(), text.into()));
        self
    }

    /// The commit and build date, e.g. from `option_env!` set by CI.
    pub fn build_info(mut self, commit: Option<&str>, built: Option<&str>) -> Self {
        self.build = (commit.map(str::to_string), built.map(str::to_string));
        self
    }

    pub fn service_hooks(mut self, hooks: Arc<dyn ServiceHooks>) -> Self {
        self.hooks = Some(hooks);
        self
    }

    pub fn installer(mut self, installer: Arc<dyn Installer>) -> Self {
        self.installer = Some(installer);
        self
    }

    /// Where the app is installed (default per user). A per-machine app (ADR 0053): updates install only when the user clicks
    /// Install (the installer is the app's [`ElevatedHandoff`]), `install_dir` is its folder (the texts name its
    /// `updates\setup.log`), `administrator` says whether this account may approve the prompt itself.
    pub fn install_scope(
        mut self,
        scope: InstallScope,
        install_dir: Option<PathBuf>,
        administrator: Option<bool>,
    ) -> Self {
        self.scope = scope;
        self.install_dir = install_dir;
        self.administrator = administrator;
        self
    }

    /// The installer lock of a per-machine app: at start the updater waits on it, bounded, while another user's install runs.
    pub fn installer_lock(mut self, lock: Arc<dyn InstallerLock>) -> Self {
        self.installer_lock = Some(lock);
        self
    }

    /// Whether an installer is set (a build without one finds updates but cannot install them).
    pub fn has_installer(&self) -> bool {
        self.installer.is_some()
    }

    /// What this build was bundled as (`nsis`, `msi`, `appimage`, `deb`, `rpm`, `app`): its package is preferred.
    pub fn installer_kind(mut self, kind: &str) -> Self {
        self.installer_kind = Some(kind.into());
        self
    }

    pub fn max_download_bytes(mut self, bytes: u64) -> Self {
        self.max_download_bytes = Some(bytes);
        self
    }

    /// How long after start-up the automatic check may first run (default a minute: not during start-up).
    pub fn auto_check_delay_secs(mut self, secs: u64) -> Self {
        self.auto_check_delay_secs = Some(secs);
        self
    }

    /// The downloader to use (default: the app's `paddock_download::tauri` one, else a new HTTPS downloader).
    pub fn downloader(mut self, downloader: Arc<Downloader>) -> Self {
        self.downloader = Some(downloader);
        self
    }

    /// Where `updates.json` is kept (default: the app's config folder) and downloads go (default: its cache folder).
    pub fn dirs(mut self, config: impl Into<PathBuf>, cache: impl Into<PathBuf>) -> Self {
        self.config_dir = Some(config.into());
        self.cache_dir = Some(cache.into());
        self
    }

    /// Whether [`install`] starts the background thread (services' hook, automatic check). On by default.
    pub fn background(mut self, on: bool) -> Self {
        self.background = on;
        self
    }
}

/// Why [`install`] could not set the updater up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// A legal document id is not usable.
    Legal(String),
    /// No downloader could be made.
    Downloader(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::Legal(e) => write!(f, "cannot register the legal document: {e}"),
            InstallError::Downloader(e) => write!(f, "cannot create the update downloader: {e}"),
        }
    }
}

impl std::error::Error for InstallError {}

/// The `plugins.updater` part of `tauri.conf.json`: its first endpoint and its key.
fn plugin_config<R: Runtime>(app: &AppHandle<R>) -> (Option<String>, Option<String>) {
    let cfg = app.config().plugins.0.get("updater");
    let text = |v: Option<&serde_json::Value>| v.and_then(|v| v.as_str()).map(str::to_string);
    let key = text(cfg.and_then(|c| c.get("pubkey")));
    let endpoint = text(cfg.and_then(|c| c.get("endpoints")).and_then(|v| v.get(0)));
    (endpoint, key)
}

/// Sets the updater and About up and manages them as `Arc<Updates>`; starts the background thread unless switched off.
/// A second call returns the first state.
pub fn install<R: Runtime>(
    app: &AppHandle<R>,
    b: UpdatesBuilder,
) -> Result<Arc<Updates>, InstallError> {
    if let Some(u) = app.try_state::<Arc<Updates>>() {
        return Ok(u.inner().clone());
    }
    let (endpoint, config_key) = plugin_config(app);
    let mut channels = b.channels;
    if channels.is_empty()
        && let Some(url) = endpoint
    {
        channels.push(Channel::new("stable", "Stable", &url));
    }
    let key = UpdateKey::resolve(
        b.keys
            .iter()
            .map(Option::as_deref)
            .chain([config_key.as_deref()]),
    );
    let config_dir = b.config_dir.or_else(|| app.path().app_config_dir().ok());
    let cache_dir = b
        .cache_dir
        .or_else(|| app.path().app_cache_dir().ok())
        .unwrap_or_else(std::env::temp_dir)
        .join("updates");
    let downloader = match b
        .downloader
        .or_else(|| paddock_download::tauri::downloader(app))
    {
        Some(d) => d,
        None => Arc::new(
            Downloader::http(Default::default())
                .map_err(|e| InstallError::Downloader(e.to_string()))?,
        ),
    };
    let records = Arc::new(RecordStore::new(config_dir.as_deref()));
    let version = app.package_info().version.to_string();
    let mut config = UpdaterConfig::new(&version, key, channels, cache_dir);
    config.target = Target::current(b.installer_kind.as_deref());
    config.releases_page = b.releases_page.clone();
    if let Some(max) = b.max_download_bytes {
        config.max_download_bytes = max;
    }
    if let Some(delay) = b.auto_check_delay_secs {
        config.auto_check_delay_secs = delay;
    }
    config.scope = b.scope;
    config.install_dir = b.install_dir.clone();
    config.administrator = b.administrator;
    let handle = app.clone();
    let mut updater = Updater::new(
        config,
        downloader,
        records.clone(),
        Box::new(move |s| {
            let _ = handle.emit(STATUS_EVENT, s);
        }),
    );
    if let Some(i) = b.installer {
        updater = updater.with_installer(i);
    }
    if let Some(h) = b.hooks {
        updater = updater.with_service_hooks(h);
    }
    if let Some(l) = b.installer_lock {
        updater = updater.with_installer_lock(l);
    }
    let available = updater.available();
    let mut about = About::new(&b.app_name, &version, records)
        .build(b.build.0.as_deref(), b.build.1.as_deref())
        .changelog(b.changelog)
        .updates_available(available);
    if let Some(page) = &b.releases_page {
        about = about.releases_page(page);
    }
    if let Some(home) = &b.homepage {
        about = about.homepage(home);
    }
    for (id, title, text) in b.legal {
        about = about
            .legal(&id, &title, text)
            .map_err(|e| InstallError::Legal(e.to_string()))?;
    }
    let updates = Arc::new(Updates {
        updater: Arc::new(updater),
        about: Arc::new(about),
        jobs: paddock_jobs::tauri::manage(app),
        pool: Arc::new(JobPool::new(Workers::new(2))),
        stopped: Arc::new(AtomicBool::new(false)),
    });
    app.manage(updates.clone());
    log::info!(Available = available; "Updates are {}", if available { "available" } else { "not available in this build" });
    if b.background {
        background(updates.clone(), AUTO_CHECK_POLL);
    }
    Ok(updates)
}

/// One more wait slice: the time waited so far and whether a whole [`INSTALLER_WAIT_BOUND`] has now passed (then the count
/// starts again and one line is logged). A slice that returned at once still counts a tenth of a slice, so a lock that cannot
/// wait never spins without the line coming.
fn advance_wait(waited: Duration, took: Duration) -> (Duration, bool) {
    let next = waited + took.max(INSTALLER_WAIT_SLICE / 10);
    if next >= INSTALLER_WAIT_BOUND {
        (Duration::ZERO, true)
    } else {
        (next, false)
    }
}

/// The start-up work off the main thread: the services' hook once per version change, then, until [`Updates::stop`], either the
/// wait for an installer that is running (in slices of [`INSTALLER_WAIT_SLICE`], so a stop is noticed; each
/// [`INSTALLER_WAIT_BOUND`] that passes is logged and the wait goes on) or the automatic check whenever it is due (looked at
/// every `poll`).
pub fn background(updates: Arc<Updates>, poll: Duration) -> Option<std::thread::JoinHandle<()>> {
    let spawned = std::thread::Builder::new().name("paddock-updates".into()).spawn(move || {
        match updates.updater.on_start() {
            ServicesOutcome::Unchanged => {}
            ServicesOutcome::Registered { previous } => {
                log::info!(Previous = previous.as_deref().unwrap_or("none"); "Services registered for this version")
            }
            ServicesOutcome::Failed(e) => {
                log::error!("Registering the services for this version failed (retried at the next start): {e}")
            }
        }
        let (mut waited, since) = (Duration::ZERO, std::time::Instant::now());
        while !updates.stopped.load(Ordering::SeqCst) {
            // While another user's installer runs, nothing else starts (ADR 0053 section 4.1 step 4). The wait is bounded
            // per slice, never forever in one call; what a bound that passes means is logged (ER-27).
            if updates.updater.installer_running() {
                let slice_began = std::time::Instant::now();
                if updates.updater.settle_handoff(INSTALLER_WAIT_SLICE) {
                    log::warn!(Ms = u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX); "The installer of a newer version stopped before it finished; shown in the Updates dialog");
                    continue;
                }
                let (now_waited, bound_passed) = advance_wait(waited, slice_began.elapsed());
                waited = now_waited;
                if bound_passed {
                    log::warn!(Seconds = since.elapsed().as_secs(); "The installer of a newer version is still running; still waiting (the Updates dialog says so)");
                }
                if slice_began.elapsed() < INSTALLER_WAIT_SLICE / 10 {
                    std::thread::park_timeout(poll);
                }
                continue;
            }
            if updates.updater.auto_check_due() {
                let _ = updates.start_check();
            }
            std::thread::park_timeout(poll);
        }
    });
    spawned
        .map_err(|e| log::warn!("Automatic update checks are off: {e}"))
        .ok()
}

/// Called from `RunEvent::Exit`, last: installs a pending update, and starts the app again when asked. Returns what it did.
pub fn on_exit<R: Runtime>(app: &AppHandle<R>) -> ExitAction {
    let Some(updates) = app.try_state::<Arc<Updates>>() else {
        return ExitAction::Nothing;
    };
    updates.stop();
    let action = updates.updater.install_at_exit();
    if matches!(action, ExitAction::Failed { .. }) {
        log::error!("Installing the update failed; it is shown at the next start");
    }
    native::relaunch_after(app, action);
    action
}

/// The commands' invoke handler (every command of [`COMMANDS`](crate::COMMANDS)); wrap it in the app's guard.
pub fn invoke_handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    ::tauri::generate_handler![
        updates_status,
        updates_check,
        updates_download,
        updates_cancel,
        updates_install_on_quit,
        updates_restart,
        updates_set_preferences,
        about_info,
        about_legal,
        about_whats_new,
        about_whats_new_start
    ]
}

/// [`handler`] behind the command guard, with each dialog's contract from `lookup` (`paddock-dialogs`): an app without a
/// guard of its own for these commands uses this.
pub fn handler_with_grants<R: Runtime, G: DialogGrant + 'static>(
    lookup: impl Fn(&AppHandle<R>, &str) -> Option<G> + Send + Sync + 'static,
) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    Guard::new(&crate::COMMANDS)
        .dialog_grants(lookup)
        .wrap(invoke_handler())
}

/// The managed state as a command argument: a command of an app that did not call [`install`] is refused by Tauri
/// ("state not managed").
type Managed<'a> = ::tauri::State<'a, Arc<Updates>>;

/// The updater's status (the UI then follows [`STATUS_EVENT`]).
#[::tauri::command]
pub async fn updates_status(updates: Managed<'_>) -> Result<UpdateStatus, UpdateErrorDto> {
    Ok(updates.updater.status())
}

/// Starts a check (a job); the result arrives as a status event.
#[::tauri::command]
pub async fn updates_check(updates: Managed<'_>) -> Result<UpdateStatus, UpdateErrorDto> {
    updates.start_check().map_err(UpdateErrorDto::from)
}

/// Starts downloading the update found (a job: the download, then the signature check).
#[::tauri::command]
pub async fn updates_download(updates: Managed<'_>) -> Result<UpdateStatus, UpdateErrorDto> {
    updates.start_download().map_err(UpdateErrorDto::from)
}

/// Cancels the check or download in flight, or drops the verified download; the update stays available.
#[::tauri::command]
pub async fn updates_cancel(updates: Managed<'_>) -> Result<UpdateStatus, UpdateErrorDto> {
    updates.cancel().map_err(UpdateErrorDto::from)
}

/// Installs the verified update when the app quits (or not).
#[::tauri::command]
pub async fn updates_install_on_quit(
    updates: Managed<'_>,
    enabled: bool,
) -> Result<UpdateStatus, UpdateErrorDto> {
    updates
        .updater
        .install_on_quit(enabled)
        .map_err(UpdateErrorDto::from)
}

/// Quits the app; its exit hook installs the verified update and starts the new version.
#[::tauri::command]
pub async fn updates_restart<R: Runtime>(
    app: AppHandle<R>,
    updates: Managed<'_>,
) -> Result<UpdateStatus, UpdateErrorDto> {
    updates
        .updater
        .prepare_restart()
        .map_err(UpdateErrorDto::from)
        .inspect(|_| native::quit(&app))
}

/// Sets automatic checks and the channel (an unknown channel is refused).
#[::tauri::command]
pub async fn updates_set_preferences(
    updates: Managed<'_>,
    preferences: Preferences,
) -> Result<UpdateStatus, UpdateErrorDto> {
    updates
        .updater
        .set_preferences(preferences)
        .map_err(UpdateErrorDto::from)
}

/// The running version, build facts and the legal documents' titles.
#[::tauri::command]
pub async fn about_info(updates: Managed<'_>) -> Result<AboutInfo, UpdateErrorDto> {
    Ok(updates.about.info())
}

/// The text of one legal document (read-only, compiled into the app).
#[::tauri::command]
pub async fn about_legal(updates: Managed<'_>, id: String) -> Result<String, UpdateErrorDto> {
    updates.about.legal_text(&id).map_err(|e| {
        UpdateError::Invalid {
            detail: e.to_string(),
        }
        .to_dto()
    })
}

/// The running version's release notes (none when the changelog has no section for it).
#[::tauri::command]
pub async fn about_whats_new(updates: Managed<'_>) -> Result<WhatsNew, UpdateErrorDto> {
    Ok(updates.about.whats_new())
}

/// Once per start, from the main window: whether to open What's New (after an update to a version with notes).
#[::tauri::command]
pub async fn about_whats_new_start(
    updates: Managed<'_>,
) -> Result<WhatsNewDecision, UpdateErrorDto> {
    Ok(updates.about.whats_new_on_start())
}

#[cfg(test)]
mod tests;
