//! Starting at login: the one Settings switch `launchAtLogin` means "Dyno opens at login (in the tray) and the agent
//! keeps measuring while the window is closed" (ADR 0010). Two registrations back it:
//!
//! - the app itself: `SMAppService.mainApp` on macOS (one row in Login Items, nothing under `~/Library/LaunchAgents`;
//!   started at login, it hides in the tray because macOS says so, [`paddock_service::apple_event::open_event`]) and
//!   an XDG autostart file with `--hidden` on Linux and the per-user `Run` value on Windows ([`paddock_service::login::UserRunEntry`], [`paddock_service::login::XdgAutostart`]);
//! - the agent as a per-user service: `SMAppService` on macOS (the plist inside the bundle), a systemd user unit on
//!   Linux and the per-user `Run` entry on Windows (`dyno_agent::service`).
//!
//! The OS parts sit behind two small traits so the orchestration is unit-tested with fakes. On macOS the agent service is
//! `paddock_service::Manager` over `SMAppService` (registration, the re-registration after an update, stop and resume are
//! the shared crate's, tested there over fakes); this file only wires it to Dyno's names. Nothing in the tests touches a
//! real launch agent, unit or registry key.

use std::path::PathBuf;

/// Whether the agent answers now, asked from another thread.
pub type AnswerFn = std::sync::Arc<dyn Fn() -> bool + Send + Sync>;

/// The agent as a per-user background service. Registering and removing it are the shared
/// [`paddock_service::login::LoginService`].
pub trait AgentService: paddock_service::login::LoginService + Send {
    /// Whether it is registered.
    fn installed(&mut self) -> bool;
    /// Whether the OS starts the agent now and restarts it when it exits (launchd, systemd), so the app must hand over
    /// to it instead of running its own; a Windows `Run` entry only starts it at the next logon.
    fn supervises(&mut self) -> bool {
        false
    }
    /// Registers it again when the OS cannot start what is registered, and on macOS once after every version change
    /// (an update). `agent_answers` asks whether the agent answers now. Returns whether it registered again.
    fn repair(&mut self, agent_answers: AnswerFn) -> Result<bool, String>;
    /// Whether a registration this app made is being waited for, or is about to be made (ADR 0056): the link neither starts an
    /// agent of its own nor retires one while this is true. False where nothing waits and in every state with no way forward.
    fn pending(&mut self) -> bool {
        false
    }
    /// The service job a stop can name (launchd job, systemd user unit), when the OS supervises the agent now; `None` where
    /// there is none to stop (Windows, or nothing supervises). Quit and stop measuring stops it without removing the registration.
    fn stop_mechanism(&mut self) -> Option<paddock_service::stop::StopMechanism> {
        None
    }
    /// Starts the service again after it was stopped on request (the next start). Returns whether it started something.
    fn resume(&mut self) -> Result<bool, String> {
        Ok(false)
    }
}

pub use paddock_service::login::Autostart;

/// The words of the message [`apply_launch_at_login`] returns.
fn words() -> paddock_service::login::LoginWords {
    paddock_service::login::LoginWords {
        saved: "Your settings are saved".into(),
        service: "the background agent".into(),
        app: "Dyno".into(),
    }
}

/// Applies the `launchAtLogin` setting to both registrations ([`paddock_service::login::apply_launch_at_login_with`]). Each
/// is attempted even if the other fails; the message names what did not work and what to do.
///
/// Turning it on from a place that will not exist after the next login (`location`, [`current_location`]) registers neither
/// and says where to move Dyno; turning it off always runs.
pub fn apply_launch_at_login(
    on: bool,
    location: Result<(), String>,
    service: &mut dyn AgentService,
    autostart: &mut dyn Autostart,
) -> Result<(), String> {
    paddock_service::login::apply_launch_at_login_with(on, location, Some(service), autostart, &words())
}

pub use paddock_service::login::FirstRun;

/// The app's own login item on every OS ([`paddock_service::login::AppItem::for_this_os`]): the XDG file and the `Run` value
/// keep the name the plugin used (the app name), so an upgrade rewrites them instead of adding a second entry; a login start
/// passes `--hidden` (Linux and Windows; macOS says so in the open event instead).
pub const APP_ITEM: paddock_service::login::AppItem<'static> = paddock_service::login::AppItem {
    name: "Dyno",
    file_stem: "Dyno",
    run_value: dyno_agent::service::spec::WINDOWS_APP_VALUE,
    args: &[paddock_window::launch::HIDDEN_FLAG],
};

/// The launch agent Dyno 1.0's `tauri-plugin-autostart` wrote on macOS (`~/Library/LaunchAgents/Dyno.plist`), which
/// `migrate_legacy_login_at_start` (macOS) replaces with `SMAppService.mainApp`.
pub const LEGACY_LAUNCH_AGENT: paddock_service::login::LegacyLaunchAgent<'static> = paddock_service::login::LegacyLaunchAgent {
    label: "Dyno",
    bundle: "Dyno.app",
    exe: "dyno-app",
    flag: paddock_window::launch::HIDDEN_FLAG,
};

/// The marker's file name inside the user's own app data folder.
pub const FIRST_RUN_MARKER: &str = "login-default-applied";

/// Start at login is on by default, so a fresh install registers itself once without Settings being opened
/// ([`paddock_service::login::apply_first_run_with`] with Dyno's words and its agent service).
pub fn apply_first_run(
    marker: &paddock_service::login::FirstRunMarker,
    sys: &mut dyn paddock_service::System,
    wanted: bool,
    location: Result<(), String>,
    service: &mut dyn AgentService,
    autostart: &mut dyn Autostart,
) -> Result<FirstRun, String> {
    paddock_service::login::apply_first_run_with(marker, sys, wanted, location, Some(service), autostart, &words())
}

/// Whether the first run registers start at login, read from the settings file the app already has before any agent runs: a
/// missing file is a fresh install (the default, on); a file that says `false` is the user's choice (an upgrade from 1.0.0).
pub fn first_run_wanted(settings_file: &std::path::Path) -> bool {
    let (settings, warning) = dyno_agent::files::load(settings_file, dyno_core::settings::parse);
    if let Some(w) = warning {
        // ER-27: a damaged settings file decides this by its defaults; the log says so.
        log::warn!(Warning = w.as_str(); "First run: the settings file was not read cleanly; start at login follows what could be read");
    }
    settings.unwrap_or_default().launch_at_login
}

/// One change: the job it registers, the work it does and its own cancel.
type Change = (
    Box<dyn FnOnce() -> paddock_jobs::JobContext + Send>,
    Box<dyn FnOnce(&crate::service_ops::OpCancel) -> Result<(), String> + Send>,
    crate::service_ops::OpCancel,
);

/// Applying the Start-at-login switch takes seconds (a registration can wait for the lock another registration holds, and each
/// cycle waits for the OS), so the save returns at once and one worker does it (ER-17); it reports as a job, which the shell's
/// Jobs view and notices already show (`paddock://jobs`). A change made while one runs is not dropped: it waits its turn, and a
/// newer waiting change replaces an older one (only the final state of the switch matters).
#[derive(Default)]
pub struct SwitchWorker {
    busy: std::sync::Arc<std::sync::atomic::AtomicBool>,
    cancel: std::sync::Arc<std::sync::Mutex<Option<crate::service_ops::OpCancel>>>,
    /// The change that waits for the running one. It is taken, and `busy` cleared when there is none, under this one lock, so a
    /// change made at that moment is never left waiting.
    queued: std::sync::Arc<std::sync::Mutex<Option<Change>>>,
}

impl SwitchWorker {
    /// Asks the running change to stop at its next step (the registration, a lock wait or the wait for the service).
    pub fn cancel(&self) {
        if let Some(cancel) = self.cancel.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            cancel.cancel();
        }
    }
}

/// One change as a job: registered, run and ended `Done`, `Failed` with the message the work returned, or `Cancelled`.
fn run_change((register, work, cancel): Change) {
    if cancel.is_cancelled() {
        log::info!(target: "dyno", "a Start at login change was cancelled before it began (Quit)");
        return;
    }
    let job = register();
    let flag = cancel.clone();
    // Cancel in the Jobs view reaches the work the way a quit does: this operation's own token, which the service checks.
    let _on_cancel = job.cancel_token().on_cancel(move || flag.cancel());
    let _ = job.run(|ctx| {
        ctx.phase(paddock_jobs::Phase::discovering("service", "Setting up the background service"));
        ctx.check()?;
        let result = work(&cancel);
        // A cancel that came during the work ends the job as cancelled, whatever the work made of it (the
        // record says what was registered; the next start puts the rest right).
        if cancel.is_cancelled() {
            return Err(paddock_jobs::JobError::Cancelled(paddock_jobs::CancelReason::User));
        }
        result.map_err(paddock_jobs::JobError::Failed)
    });
}

impl SwitchWorker {
    /// Runs `work` on a worker thread and returns at once. The worker calls `register` for the change's job (the
    /// Jobs view's Cancel stops `work` at its next step, as [`cancel`](Self::cancel) does). While a change runs, this one waits
    /// behind it and the worker runs it next (it registers its job then).
    ///
    /// # Errors
    /// A message when the app is quitting or no worker could be started.
    pub fn start(
        &self,
        ops: &crate::service_ops::ServiceOps,
        register: impl FnOnce() -> paddock_jobs::JobContext + Send + 'static,
        work: impl FnOnce(&crate::service_ops::OpCancel) -> Result<(), String> + Send + 'static,
    ) -> Result<(), String> {
        let Some(cancel) = ops.begin() else {
            return Err("Dyno is quitting. Start at login is not changed.".into());
        };
        let change: Change = (Box::new(register), Box::new(work), cancel);
        {
            let mut queued = self.queued.lock().unwrap_or_else(|p| p.into_inner());
            if self.busy.swap(true, std::sync::atomic::Ordering::SeqCst) {
                let replaced = queued.replace(change).is_some();
                log::info!(target: "dyno", Replaced = replaced; "a Start at login change was made while another runs; it waits its turn");
                return Ok(());
            }
        }
        let (busy, queue, slot) = (self.busy.clone(), self.queued.clone(), self.cancel.clone());
        let spawned = std::thread::Builder::new().name("dyno-login-switch".into()).spawn(move || {
            let mut next = Some(change);
            while let Some(change) = next.take() {
                *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(change.2.clone());
                // A change that panics (even while registering its job) ends that change only: the worker goes on with the next one
                // and is never left busy.
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_change(change))).is_err() {
                    log::warn!(target: "dyno", "a Start at login change panicked; it is dropped, the next one runs");
                }
                let mut queued = queue.lock().unwrap_or_else(|p| p.into_inner());
                next = queued.take();
                if next.is_none() {
                    busy.store(false, std::sync::atomic::Ordering::SeqCst);
                }
            }
        });
        spawned.map(drop).map_err(|e| {
            self.busy.store(false, std::sync::atomic::Ordering::SeqCst);
            format!("Start at login could not be changed: {e}")
        })
    }
}

/// An autostart whose registration is not made once the change was cancelled: a cancel that came while the agent was being
/// registered must not still add the app's own item (and be reported as on). Removing is never held back.
pub struct UnlessCancelled<'a> {
    inner: &'a mut dyn Autostart,
    cancel: &'a crate::service_ops::OpCancel,
}

impl<'a> UnlessCancelled<'a> {
    /// `inner`, guarded by `cancel`.
    pub fn new(inner: &'a mut dyn Autostart, cancel: &'a crate::service_ops::OpCancel) -> Self {
        Self { inner, cancel }
    }
}

impl Autostart for UnlessCancelled<'_> {
    fn set(&mut self, on: bool) -> Result<(), String> {
        if on && self.cancel.is_cancelled() {
            log::info!(target: "dyno", "start at login: cancelled, so the app's own item is not registered");
            return Ok(());
        }
        self.inner.set(on)
    }
}

/// What the app remembers as applied after a change that set `wanted` (it was remembered at once, before the worker started, so
/// a second save sees it): kept when the change finished, put back to `before` when it failed or was cancelled, unless a newer
/// change has set its own value meanwhile. Returns whether the change stands.
pub fn settle_applied(value: &std::sync::Mutex<Option<bool>>, wanted: bool, before: Option<bool>, ok: bool, cancelled: bool) -> bool {
    let stands = ok && !cancelled;
    let mut value = value.lock().unwrap_or_else(|e| e.into_inner());
    if !stands && *value == Some(wanted) {
        *value = before;
    }
    stands
}

/// One Start-at-login change as the worker runs it: both registrations (the app's own only while the change is not cancelled), then
/// the remembered value follows the outcome. The caller refreshes what depends on the service afterwards.
pub fn apply_switch_change(
    wanted: bool,
    location: Result<(), String>,
    service: &mut dyn AgentService,
    plain: &mut dyn Autostart,
    cancel: &crate::service_ops::OpCancel,
    value: &std::sync::Mutex<Option<bool>>,
    before: Option<bool>,
) -> Result<(), String> {
    let mut autostart = UnlessCancelled::new(plain, cancel);
    let result = apply_launch_at_login(wanted, location, service, &mut autostart);
    if settle_applied(value, wanted, before, result.is_ok(), cancel.is_cancelled()) {
        crate::logging::info(&format!("start at login is now {}", if wanted { "on" } else { "off" }));
    }
    result
}

/// A change the worker refused to take (the app is quitting, no thread) was never applied: the remembered value goes back.
pub fn undo_if_refused(
    started: Result<(), String>,
    value: &std::sync::Mutex<Option<bool>>,
    wanted: bool,
    before: Option<bool>,
) -> Result<(), String> {
    if started.is_err() {
        settle_applied(value, wanted, before, false, false);
    }
    started
}

/// With the switch off the app's own login item is removed (an interrupted change, or an earlier build, may have left it).
pub fn remove_stale_app_item(start_at_login: bool, autostart: &mut dyn Autostart) -> Result<(), String> {
    if start_at_login {
        return Ok(());
    }
    autostart.set(false)
}

/// The user's switch as the service manager needs it (ADR 0056): the settings file's `launchAtLogin`; a missing file is a fresh
/// install (the default, on); a file that cannot be read is `None`, so nothing is registered unattended until it can.
pub fn wanted_setting(settings_file: &std::path::Path) -> Option<bool> {
    let (settings, warning) = dyno_agent::files::load(settings_file, dyno_core::settings::parse);
    if warning.is_some() {
        return None;
    }
    Some(settings.unwrap_or_default().launch_at_login)
}

/// Where the first-run marker is (set when the first run looks at it; the service manager asks whether it is there).
static FIRST_RUN_MARKER_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Whether the first run has been done: its marker is there.
pub fn first_run_done() -> bool {
    FIRST_RUN_MARKER_PATH.get().is_some_and(|p| p.exists())
}

/// Whether the first-run registration has been tried in this process. A failed attempt is not repeated in the same run
/// (each try may open System Settings or wait on `systemctl`); the missing marker makes the next start try again.
pub struct FirstRunAttempt(std::sync::atomic::AtomicBool);

impl FirstRunAttempt {
    /// Nothing tried yet.
    pub const fn new() -> FirstRunAttempt {
        FirstRunAttempt(std::sync::atomic::AtomicBool::new(false))
    }

    /// True once, for the first caller.
    pub fn claim(&self) -> bool {
        !self.0.swap(true, std::sync::atomic::Ordering::SeqCst)
    }
}

impl Default for FirstRunAttempt {
    fn default() -> Self {
        FirstRunAttempt::new()
    }
}

/// [`apply_first_run_at_start`], at most once per `attempt`: later calls do nothing and say [`FirstRun::Skipped`].
pub fn apply_first_run_once(
    attempt: &FirstRunAttempt,
    settings_file: &std::path::Path,
    marker: &paddock_service::login::FirstRunMarker,
    sys: &mut dyn paddock_service::System,
    location: Result<(), String>,
    service: &mut dyn AgentService,
    autostart: &mut dyn Autostart,
) -> Result<FirstRun, String> {
    if !attempt.claim() {
        return Ok(FirstRun::Skipped);
    }
    apply_first_run_at_start(settings_file, marker, sys, location, service, autostart)
}

/// The first run for the installed app, the one place both callers use (the link's start, and the first sight of the settings
/// for an app that registered nothing yet): the real service, login item and marker, once per process. `handover` is true when
/// an agent of the app may already run, so the link is told to look at the service again.
pub fn run_first_run<R: tauri::Runtime>(app: &tauri::AppHandle<R>, link: &crate::link::AgentLink, handover: bool) {
    use tauri::Manager;
    let Ok(data) = app.path().app_data_dir() else {
        crate::logging::info("first run: no data folder, start at login not applied");
        return;
    };
    let ops = app.state::<crate::service_ops::ServiceOps>();
    let Some(cancel) = ops.begin() else {
        crate::logging::info("first run: the app is quitting, start at login is not applied");
        return;
    };
    let mut service = first_run_service(cancel);
    let mut autostart = platform_autostart(app);
    let marker = paddock_service::login::FirstRunMarker::new(data.join(FIRST_RUN_MARKER));
    FIRST_RUN_MARKER_PATH.set(data.join(FIRST_RUN_MARKER)).ok();
    let mut sys = paddock_service::RealSystem::new();
    let began = std::time::Instant::now();
    match apply_first_run_once(
        ops.first_run_attempt(),
        &link.dirs.settings(),
        &marker,
        &mut sys,
        current_location(),
        service.as_mut(),
        autostart.as_mut(),
    ) {
        Ok(FirstRun::Applied) => {
            link.set_service_installed(service.installed());
            if handover {
                link.refresh_service();
            }
            log::info!(Ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX), HandOver = handover; "First run: start at login (the default) was registered");
        }
        Ok(FirstRun::Skipped) => {}
        Err(e) => {
            log::warn!(target: "dyno", Error = e.as_str(), Ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX); "First run: start at login (the default) could not be registered; it is tried again at the next start, or turn it off and on in Settings.")
        }
    }
}

/// The first run, decided before the link starts any agent (bug 7538-1): registering the agent service after the app has
/// started and connected to an agent of its own ends that agent and waits for the service's (launchd's or systemd's
/// restart throttle), so a fresh install showed three agent processes in its first 20 s. Registered first, the link sees a
/// supervising service and starts none of its own.
pub fn apply_first_run_at_start(
    settings_file: &std::path::Path,
    marker: &paddock_service::login::FirstRunMarker,
    sys: &mut dyn paddock_service::System,
    location: Result<(), String>,
    service: &mut dyn AgentService,
    autostart: &mut dyn Autostart,
) -> Result<FirstRun, String> {
    apply_first_run(marker, sys, first_run_wanted(settings_file), location, service, autostart)
}

/// Whether the running executable may be registered to start at login.
pub fn current_location() -> Result<(), String> {
    match std::env::current_exe() {
        Ok(exe) => current_location_for(&exe.to_string_lossy()),
        Err(e) => Err(format!("the app's folder cannot be found: {e}")),
    }
}

/// [`current_location`] for the executable at `exe`.
fn current_location_for(exe: &str) -> Result<(), String> {
    paddock_service::launch::refuse_unstable_location(std::path::Path::new(exe), "Dyno")
}

/// The agent program next to the app (or `DYNO_AGENT_BIN` in a development build).
pub fn agent_program() -> Result<PathBuf, String> {
    let app = std::env::current_exe().map_err(|e| format!("the app's folder cannot be found: {e}"))?;
    let env = std::env::var_os("DYNO_AGENT_BIN");
    crate::link::agent_exe(&app, agent_override(cfg!(debug_assertions), env.as_deref()))
}

/// The program `DYNO_AGENT_BIN` names, only in a development build: in a release build the environment must not decide
/// which program the app runs (any process of the user could set it).
pub fn agent_override(debug_build: bool, value: Option<&std::ffi::OsStr>) -> Option<&std::ffi::OsStr> {
    value.filter(|_| debug_build)
}

// ----------------------------------------------------------------------------------------------------------- Linux

/// The agent's systemd user unit through the shared service manager (`dyno_agent::service::linux`, ADR 0056): the Settings
/// switch, the repair at start, Quit and the resume are thin callers of `Manager`, so a `systemctl --user disable` is the user's
/// switch and nothing here writes, enables or starts a unit by itself. `make` builds the manager for the waits a call wants
/// (the real one in the app, one over the fake systemd in the tests).
#[cfg(any(target_os = "linux", test))]
pub struct LinuxService {
    make: dyno_agent::service::linux::ManagerFactory,
    cancel: crate::service_ops::OpCancel,
}

#[cfg(any(target_os = "linux", test))]
impl LinuxService {
    /// The service of this user: the real `systemctl`, Dyno's data folder and the agent program next to the app.
    #[cfg(target_os = "linux")]
    pub fn real(cancel: crate::service_ops::OpCancel) -> LinuxService {
        LinuxService::with(
            Box::new(|timings| {
                let manager = dyno_agent::service::linux::real_manager(&agent_program()?, env!("CARGO_PKG_VERSION"), timings)?;
                // The app's own decisions are inputs to the plan, as on macOS (ADR 0056): the user's switch, the first run, the place
                // the app runs from.
                Ok(manager.with_app_state(
                    || wanted_setting(&dyno_agent::paths::Dirs::current().settings()),
                    first_run_done,
                    || current_location().is_ok(),
                ))
            }),
            cancel,
        )
    }

    /// A service whose manager comes from `make` (tests).
    pub fn with(make: dyno_agent::service::linux::ManagerFactory, cancel: crate::service_ops::OpCancel) -> LinuxService {
        LinuxService { make, cancel }
    }

    /// A manager that is only asked what it sees or told to stop or resume.
    fn quiet(&self) -> Result<paddock_service::Manager, String> {
        (self.make)(paddock_service::Timings::default())
    }
}

#[cfg(any(target_os = "linux", test))]
impl paddock_service::login::LoginService for LinuxService {
    /// Registers the unit without waiting for the agent to answer (the link waits for it and falls back to one of its own).
    fn install(&mut self) -> Result<(), String> {
        let mut m = (self.make)(dyno_agent::service::linux::install_timings())?;
        dyno_agent::service::linux::install_without_waiting(&mut m, self.cancel.flag())
    }

    fn uninstall(&mut self) -> Result<(), String> {
        dyno_agent::service::linux::uninstall_without_waiting(&mut self.quiet()?)
    }
}

#[cfg(any(target_os = "linux", test))]
impl AgentService for LinuxService {
    fn installed(&mut self) -> bool {
        self.quiet().is_ok_and(|mut m| m.installed())
    }

    /// Registered, not disabled by the user and systemd runs the unit or is about to restart it.
    fn supervises(&mut self) -> bool {
        self.quiet().is_ok_and(|mut m| m.supervises())
    }

    /// After an update (or a moved program) the unit is written again once; what the user disabled is left alone and nothing is
    /// registered that the user never turned on.
    fn repair(&mut self, agent_answers: AnswerFn) -> Result<bool, String> {
        // The app's own agent keeps a refused unit from being replaced while the app measures; it never settles anything.
        let mut manager = self.quiet()?.with_cover(move || agent_answers());
        dyno_agent::service::linux::ensure(&mut manager, true, self.cancel.flag())
    }

    /// Whether a registration this app made is being waited for (the link neither starts nor retires an agent meanwhile).
    fn pending(&mut self) -> bool {
        self.quiet().is_ok_and(|mut m| m.pending())
    }

    /// The user unit, while systemd runs it (the registration is not touched by the stop).
    fn stop_mechanism(&mut self) -> Option<paddock_service::stop::StopMechanism> {
        self.quiet().ok()?.stop_mechanism()
    }

    /// After a stop on request: the shared resume starts the unit again unless the user switched it off meanwhile.
    fn resume(&mut self) -> Result<bool, String> {
        self.quiet()?
            .resume(self.cancel.flag(), &mut |step| {
                crate::logging::info(&format!("agent service: {step}"))
            })
            .map_err(|e| dyno_agent::service::linux::words(&e))
    }
}

// ---------------------------------------------------------------------------------------------------------- Windows

/// On Windows the app's own `Run` value is the logon entry and the app starts the agent hidden: the agent is not registered
/// separately (an entry that started only the agent opened a console and left no tray icon, Dyno 1.0.0), so `install` only
/// removes the stale agent entry 1.0.0 wrote, and `repair` does the same and says whether it was there.
#[cfg(windows)]
pub struct CliService;

#[cfg(windows)]
impl CliService {
    fn service(&self) -> Result<dyno_agent::service::Service, String> {
        dyno_agent::service::Service::for_current_user(agent_program()?)
    }
}

#[cfg(windows)]
impl paddock_service::login::LoginService for CliService {
    fn install(&mut self) -> Result<(), String> {
        let mut s = self.service()?;
        if s.app_is_logon_entry() {
            return s.clear_stale_run_entry().map(|_| ());
        }
        s.install()
    }

    fn uninstall(&mut self) -> Result<(), String> {
        self.service()?.uninstall()
    }
}

#[cfg(windows)]
impl AgentService for CliService {
    fn installed(&mut self) -> bool {
        false
    }

    fn repair(&mut self, _agent_answers: AnswerFn) -> Result<bool, String> {
        let mut s = self.service()?;
        if s.app_is_logon_entry() {
            return s.clear_stale_run_entry();
        }
        s.ensure()
    }
}

/// Dyno 1.0.0 on Windows replaced the app's logon entry (`Dyno` = `dyno-app.exe --hidden`) with the agent's (`Dyno` =
/// `dyno-agent.exe run`), five seconds after every start, so the next logon started only the agent. At the first sight of the
/// switch after an update: the stale entry is removed, and the app's own entry is registered again when start at login is on
/// (when it is off, removing it is all). Returns whether a stale entry was there.
pub fn repair_stale_agent_entry(
    start_at_login: bool,
    service: &mut dyn AgentService,
    autostart: &mut dyn Autostart,
) -> Result<bool, String> {
    let cleaned = service.repair(std::sync::Arc::new(|| true))?;
    if cleaned && start_at_login {
        autostart.set(true)?;
    }
    Ok(cleaned)
}

/// Windows, at every start: when start at login is on, the app's own `Run` value is written again (idempotent), so a switch
/// that says on while nothing is registered (an entry written to the wrong hive, deleted by a cleaner, or never written) heals
/// itself. With the switch off the app's own item is removed (on every platform, at the first sight of the settings), so one that
/// outlived the switch does not start Dyno at login. Returns whether it wrote.
pub fn ensure_app_entry(start_at_login: bool, autostart: &mut dyn Autostart) -> Result<bool, String> {
    if !start_at_login {
        // The app's own item is not left behind a switch that is off (a change that was cut short, or an older build's).
        remove_stale_app_item(false, autostart)?;
        return Ok(false);
    }
    autostart.set(true)?;
    Ok(true)
}

// ------------------------------------------------------------------------------- the macOS service's decisions (pure)

/// What a failed registration tells the user. `opened`: System Settings was opened for them.
#[cfg(any(target_os = "macos", test))]
fn registration_words(e: &paddock_service::ServiceError, opened: bool) -> String {
    use paddock_service::ServiceError;
    let shown = if opened { " (opened for you)" } else { "" };
    match e {
        ServiceError::NeedsApproval => {
            format!("macOS needs your approval: allow Dyno in System Settings > General > Login Items{shown}")
        }
        ServiceError::SwitchedOff => {
            format!("Dyno is switched off in System Settings > General > Login Items{shown}: switch it on there")
        }
        ServiceError::Busy => "Another change is still running; try again in a moment.".to_string(),
        other => other.to_string(),
    }
}

/// Whether a registration is made without opening System Settings: the first run (nobody asked), once Login Items has been
/// opened for it before.
#[cfg(any(target_os = "macos", test))]
fn install_is_quiet(first_run: bool) -> bool {
    first_run
}

/// The waits of the Settings switch's registration: it returns at once (the link waits for the agent and falls back to one of
/// its own), so it does not wait for the service to answer.
#[cfg(any(target_os = "macos", test))]
fn install_timings() -> paddock_service::Timings {
    paddock_service::Timings::default().without_start_wait()
}

/// What the repair at start tells the caller: `Ok(true)` the registration was replaced and the service runs, `Ok(false)` nothing
/// was needed, an error when it was registered but never answered, another repair kept the lock, or it failed.
#[cfg(any(target_os = "macos", test))]
fn repair_result(started: paddock_service::manager::Started) -> Result<bool, String> {
    use paddock_service::manager::Started;
    use paddock_service::{RepairOutcome, ServiceError};
    match started {
        Started::Repaired(_, outcome) => match outcome {
            RepairOutcome::Recovered => Ok(true),
            RepairOutcome::Registered => {
                Err("the agent service was registered again but did not answer; it is tried again at the next start".into())
            }
            RepairOutcome::Cancelled => Ok(false),
            RepairOutcome::NeedsApproval => Err(registration_words(&ServiceError::NeedsApproval, false)),
            RepairOutcome::Failed(why) | RepairOutcome::Unavailable(why) => Err(why),
            RepairOutcome::NeedsElevation | RepairOutcome::Declined => Err("the agent service could not be registered again".into()),
        },
        Started::Busy => Err("another registration or repair kept running; it is tried again at the next start".into()),
        Started::Left(_) | Started::Marked | Started::Offered(_) | Started::Removed => Ok(false),
    }
}

// ---------------------------------------------------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
pub use mac::{migrate_legacy_login_at_start, SmService};

#[cfg(target_os = "macos")]
mod mac {
    //! The bundled launch agent through the shared service manager: `Contents/Library/LaunchAgents/net.decryptology.dyno.agent.plist`
    //! in the bundle (`bundle.macOS.files` in tauri.macos.conf.json) with `BundleProgram` pointing at the bundled agent, registered
    //! with `SMAppService` by `paddock_service::backend::LaunchdBundled`. Registration, the re-registration after an update,
    //! stop and resume are `paddock_service::Manager`'s; this file names Dyno's service and maps the results to Dyno's messages.

    use super::*;
    use dyno_agent::service::LABEL;
    use paddock_service::login::{LoginService, MainAppItem};
    use paddock_service::manager::Started;
    use paddock_service::repair::Timings;
    use paddock_service::{Manager, ServiceSpec};

    /// The bundled launch agent. `first_run` is the unasked registration at the first sight of the settings: it opens System
    /// Settings (Login Items) for an approval or a switch at most once, where the Settings switch always does.
    pub struct SmService {
        first_run: bool,
        cancel: crate::service_ops::OpCancel,
    }

    impl SmService {
        /// The service for what the user asks (Settings switch, repair at start, resume, stop).
        pub fn asked(cancel: crate::service_ops::OpCancel) -> SmService {
            SmService { first_run: false, cancel }
        }

        /// The service for the first run (no one asked): Login Items is opened at most once.
        pub fn first_run(cancel: crate::service_ops::OpCancel) -> SmService {
            SmService { first_run: true, cancel }
        }
    }

    /// Dyno 1.1.0 records the registered version with the shared service record; the 1.0.0 file is of no use any more.
    fn forget_old_version_file(root: &std::path::Path) {
        let old = root.join("service-registered-version");
        match std::fs::remove_file(&old) {
            Ok(()) => crate::logging::info("removed the 1.0.0 registered-version file (the service record replaces it)"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => crate::logging::warn(&format!("the 1.0.0 registered-version file could not be removed: {e}")),
        }
    }

    /// The service of the agent: its spec, the real `SMAppService` and `launchctl` (5 s per call), Dyno's data folder.
    /// `agent_answers` says whether the agent answers; `timings` are the waits of a repair.
    fn manager(agent_answers: AnswerFn, timings: Timings) -> Result<Manager, String> {
        let spec = ServiceSpec::builder(LABEL, "Dyno", "dyno-agent")
            .build()
            .map_err(|e| format!("the agent's service definition is wrong: {e}"))?;
        let uid = paddock_service::native::current_uid();
        let target = paddock_service::paths::launchd_target(&spec, uid);
        let system = || paddock_service::system::RealSystem::new().with_timeout(std::time::Duration::from_secs(5));
        let backend = paddock_service::backend::LaunchdBundled::new(
            spec.clone(),
            uid,
            Box::new(paddock_service::native::SmAppService::new(&spec)),
            Box::new(system()),
        );
        let dirs = dyno_agent::paths::Dirs::current();
        let manager = Manager::new(
            "Dyno",
            Box::new(backend),
            dirs.root(),
            LABEL,
            env!("CARGO_PKG_VERSION"),
            // The service's own signal only: launchd running the job. The app's own agent, which the link may have started
            // meanwhile, is `cover`: it keeps a refused job from being replaced, but never settles a registration.
            move || paddock_service::launchctl::job_running(&mut system(), &target),
        )
        .with_cover(move || agent_answers())
        .with_app_state(
            || super::wanted_setting(&dyno_agent::paths::Dirs::current().settings()),
            super::first_run_done,
            || super::current_location().is_ok(),
        )
        .with_timings(timings);
        // This copy's identity (ADR 0056 section 8): the record keeps the program that registered, so another installed copy
        // can tell whose registration it sees.
        Ok(match std::env::current_exe() {
            Ok(exe) => manager.with_program(&exe.to_string_lossy()),
            Err(_) => manager,
        })
    }

    /// A manager that is only asked what it sees or told to stop or resume: nothing waits for an agent.
    fn quiet_manager() -> Result<Manager, String> {
        manager(std::sync::Arc::new(|| false), Timings::default())
    }

    /// Moves a start-at-login the old route (`tauri-plugin-autostart`) wrote to `SMAppService.mainApp`
    /// ([`paddock_service::login::LegacyLaunchAgent::migrate`]).
    pub fn migrate_legacy_login_at_start() {
        let Some(home) = std::env::var_os("HOME") else { return };
        let dir = PathBuf::from(home).join("Library/LaunchAgents");
        match super::LEGACY_LAUNCH_AGENT.migrate(&dir, super::current_location(), &mut MainAppItem::new()) {
            Ok(paddock_service::login::LegacyPlist::Removed) => {
                crate::logging::info("moved start at login from the old Dyno.plist to a Login Items entry")
            }
            Ok(_) => {}
            Err(e) => crate::logging::warn(&format!("the old start-at-login file could not be moved: {e}")),
        }
    }

    impl LoginService for SmService {
        /// Registers the agent without waiting for it to answer (`install_timings`): the link waits for it and falls back to an
        /// agent of its own. A change that finds another one running answers Busy after the lock's own look (about 0.4 s); the
        /// registration itself runs on the switch's worker, off the save.
        fn install(&mut self) -> Result<(), String> {
            let mut m = manager(std::sync::Arc::new(|| false), install_timings())?;
            let quiet = install_is_quiet(self.first_run);
            // A first run that lost the lock to another registration waits for it instead of leaving nothing registered.
            // The Settings switch never waits for another change (it answers Busy at once, after the lock's own short look); the
            // first run waits for one, so a first run that lost the lock does not end with nothing registered.
            let (tries, pause) = if self.first_run { (6, 5) } else { (0, 0) };
            let result = m.install_patient(self.cancel.flag(), quiet, tries, std::time::Duration::from_secs(pause));
            result.map(drop).map_err(|e| registration_words(&e, !quiet))
        }

        fn uninstall(&mut self) -> Result<(), String> {
            // Another change that holds the lock is answered with Busy (the switch does not wait for it).
            quiet_manager()?.uninstall().map_err(|e| registration_words(&e, false))
        }
    }

    impl AgentService for SmService {
        fn installed(&mut self) -> bool {
            quiet_manager().is_ok_and(|mut m| m.installed())
        }

        fn pending(&mut self) -> bool {
            quiet_manager().is_ok_and(|mut m| m.pending())
        }

        /// Enabled, not switched off in Login Items and loaded in launchd: it runs the job (`RunAtLoad`, `KeepAlive`).
        /// Waiting for approval, switched off or dropped by launchd, it runs nothing.
        fn supervises(&mut self) -> bool {
            quiet_manager().is_ok_and(|mut m| m.supervises())
        }

        /// After an update launchd refuses the registered agent (its stored launch constraint is the previous build's
        /// cdhash) or keeps the old job: replace the registration when launchd reports it stale while the agent does
        /// not answer, and once after every version change. What the user switched off in Login Items is left alone, and
        /// nothing is registered that the user never turned on.
        fn repair(&mut self, agent_answers: AnswerFn) -> Result<bool, String> {
            // From a disk image or a translocated path the bundle cannot register the agent: replacing would remove a job
            // that runs and fail to put it back.
            super::current_location()?;
            forget_old_version_file(dyno_agent::paths::Dirs::current().root());
            let mut m = manager(agent_answers, Timings::default())?;
            // Another registration or repair (the first run, a second copy) may hold the lock: wait for it a while.
            let started = m.on_start_patient(self.cancel.flag(), 6, std::time::Duration::from_secs(5));
            if let Started::Repaired(reason, outcome) = &started {
                crate::logging::info(&format!("agent service: replaced the registration ({reason:?}): {outcome:?}"));
            }
            repair_result(started)
        }

        /// The launchd job `gui/<uid>/net.decryptology.dyno.agent`, while launchd holds it. The `SMAppService` registration is
        /// not touched: `launchctl bootout` takes the job out of launchd (so `KeepAlive` cannot restart it) and the registration
        /// loads it again at the next login.
        fn stop_mechanism(&mut self) -> Option<paddock_service::stop::StopMechanism> {
            quiet_manager().ok()?.stop_mechanism()
        }

        /// After a stop on request launchd holds no job and `RunAtLoad` fires only when a job is loaded: the shared resume decides and
        /// runs it (an item switched off or a list that cannot be read is left alone; a version change goes through the full repair;
        /// an enabled item of this version is registered again without removal, which macOS would announce as a new item).
        /// Returns whether launchd holds the job afterwards (found, not necessarily running yet).
        ///
        /// This runs before the link's first look, so nothing else is shown while it works: it does not wait for the agent to
        /// answer (`install_timings`). A registration that was made and has not answered yet is `pending`, which the link
        /// waits for in a labelled phase ("Waiting for the background service") instead of a silent wait of up to 120 s here.
        fn resume(&mut self) -> Result<bool, String> {
            let mut m = manager(std::sync::Arc::new(|| false), install_timings())?;
            match m.resume(self.cancel.flag(), &mut |step| {
                crate::logging::info(&format!("agent service: {step}"))
            }) {
                Ok(held) => Ok(held),
                Err(_) if m.pending() => {
                    crate::logging::info("agent service: registered again; the link waits for it to answer");
                    Ok(true)
                }
                Err(e) => Err(registration_words(&e, false)),
            }
        }
    }
}

/// The platform's agent service; `cancel` is the cancel of the operation that uses it.
#[cfg(target_os = "macos")]
pub fn platform_service(cancel: crate::service_ops::OpCancel) -> Box<dyn AgentService> {
    Box::new(SmService::asked(cancel))
}

/// The agent service for the first run, which nobody asked for: System Settings is opened at most once.
#[cfg(target_os = "macos")]
pub fn first_run_service(cancel: crate::service_ops::OpCancel) -> Box<dyn AgentService> {
    Box::new(SmService::first_run(cancel))
}

/// The agent service for the first run (the same as [`platform_service`] off macOS).
#[cfg(not(target_os = "macos"))]
pub fn first_run_service(cancel: crate::service_ops::OpCancel) -> Box<dyn AgentService> {
    platform_service(cancel)
}

/// The platform's agent service.
#[cfg(target_os = "linux")]
pub fn platform_service(cancel: crate::service_ops::OpCancel) -> Box<dyn AgentService> {
    Box::new(LinuxService::real(cancel))
}

/// The platform's agent service (Windows has nothing to wait for or cancel).
#[cfg(windows)]
pub fn platform_service(_cancel: crate::service_ops::OpCancel) -> Box<dyn AgentService> {
    Box::new(CliService)
}

/// Whether the app is waiting for a registration it made (see [`AgentService::pending`]).
pub fn service_pending() -> bool {
    platform_service(crate::service_ops::OpCancel::default()).pending()
}

/// The probe the agent link asks before it starts an agent of its own (see [`crate::link::ServiceState`]).
pub fn service_probe() -> crate::link::ServiceProbe {
    std::sync::Arc::new(|| {
        let mut service = platform_service(crate::service_ops::OpCancel::default());
        crate::link::ServiceState {
            installed: service.installed(),
            supervises: service.supervises(),
            pending: service.pending(),
        }
    })
}

/// The app's own login item ([`APP_ITEM`]): `SMAppService.mainApp` on macOS, the per-user `Run` value on Windows (never
/// written while elevated) and an XDG autostart file on Linux, each starting the app hidden in the tray.
pub fn platform_autostart<R: tauri::Runtime>(_app: &tauri::AppHandle<R>) -> Box<dyn Autostart> {
    APP_ITEM.for_this_os(
        std::env::current_exe(),
        &|key| std::env::var_os(key),
        Box::new(paddock_service::RealSystem::new()),
    )
}

#[cfg(test)]
mod tests {
    use crate::service_ops::ServiceOps;

    /// The app's job registry with a sink that keeps every snapshot, as the Jobs view would see them.
    fn jobs_rig() -> (Arc<paddock_jobs::JobRegistry>, std::sync::mpsc::Receiver<paddock_jobs::JobSnapshot>) {
        struct Sink(Mutex<std::sync::mpsc::Sender<paddock_jobs::JobSnapshot>>);
        impl paddock_jobs::EventSink for Sink {
            fn emit(&self, snapshot: &paddock_jobs::JobSnapshot) {
                let _ = self.0.lock().unwrap().send(snapshot.clone());
            }
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let registry = paddock_jobs::JobRegistry::with_clock(Arc::new(Sink(Mutex::new(tx))), Arc::new(paddock_jobs::SystemClock), 0);
        (Arc::new(registry), rx)
    }

    /// Waits for the first snapshot in `state`'s kind of state, as the window would.
    fn next_state(
        rx: &std::sync::mpsc::Receiver<paddock_jobs::JobSnapshot>,
        want: impl Fn(&paddock_jobs::JobState) -> bool,
    ) -> paddock_jobs::JobSnapshot {
        loop {
            let s = rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("the job did not reach the state");
            if want(&s.state) {
                return s;
            }
        }
    }

    fn register_switch(registry: &Arc<paddock_jobs::JobRegistry>) -> impl FnOnce() -> paddock_jobs::JobContext + Send + 'static {
        let registry = registry.clone();
        move || registry.register("start-at-login", "Start Dyno at login", None)
    }

    /// Starts a switch change that waits for its cancel and returns once its work has begun.
    fn start_a_waiting_change(
        worker: &SwitchWorker,
        ops: &ServiceOps,
        registry: &Arc<paddock_jobs::JobRegistry>,
        begun: std::sync::mpsc::Sender<()>,
    ) {
        // The work waits for its cancel (as a registration waits for the service) and gives up when it comes.
        worker
            .start(ops, register_switch(registry), move |cancel| {
                begun.send(()).unwrap();
                let _ = paddock_testkit::poll_until_within(std::time::Duration::from_secs(5), || cancel.is_cancelled());
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn a_cancelled_switch_change_leaves_no_cancel_for_the_next_one() {
        let (ops, worker) = (ServiceOps::default(), SwitchWorker::default());
        let (registry, _rx) = jobs_rig();
        let (begun, wake) = std::sync::mpsc::channel::<()>();
        start_a_waiting_change(&worker, &ops, &registry, begun);
        wake.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        worker.cancel();
        paddock_testkit::wait_until("the cancelled change to end", || {
            !worker.busy.load(std::sync::atomic::Ordering::SeqCst)
        });
        let (done, seen) = std::sync::mpsc::channel();
        worker
            .start(&ops, register_switch(&registry), move |cancel| {
                done.send(cancel.is_cancelled()).map_err(|e| e.to_string())
            })
            .unwrap();
        assert!(
            !seen.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
            "the next change starts cancelled"
        );
    }

    #[test]
    fn quit_stops_the_running_change_and_refuses_the_next() {
        use paddock_jobs::{CancelReason, JobState};
        let (ops, worker) = (ServiceOps::default(), SwitchWorker::default());
        let (registry, rx) = jobs_rig();
        let (begun, wake) = std::sync::mpsc::channel::<()>();
        start_a_waiting_change(&worker, &ops, &registry, begun);
        wake.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        ops.quit();
        let end = next_state(&rx, JobState::is_terminal);
        assert_eq!(
            end.state,
            JobState::Cancelled {
                reason: CancelReason::User
            }
        );
        while worker.busy.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::yield_now();
        }
        assert!(
            worker.start(&ops, register_switch(&registry), |_| Ok(())).is_err(),
            "nothing starts after Quit"
        );
    }

    #[test]
    fn a_cancel_from_the_jobs_view_stops_the_switch_change_and_it_ends_as_a_cancelled_job() {
        use paddock_jobs::{CancelReason, JobState};
        let (ops, worker) = (ServiceOps::default(), SwitchWorker::default());
        let (registry, rx) = jobs_rig();
        let (begun, wake) = std::sync::mpsc::channel::<()>();
        start_a_waiting_change(&worker, &ops, &registry, begun);
        wake.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        let running = next_state(&rx, |s| matches!(s, JobState::Running { .. }));
        assert!(running.cancellable, "the Jobs view offers Cancel on the switch job");
        // What the Jobs view's Cancel button does (`job_cancel`).
        assert!(registry.cancel(running.id));
        let end = next_state(&rx, JobState::is_terminal);
        assert_eq!(
            end.state,
            JobState::Cancelled {
                reason: CancelReason::User
            }
        );
        assert_eq!(end.id, running.id);
        while worker.busy.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_cancel_stops_the_switch_change_and_it_ends_as_a_cancelled_job() {
        use paddock_jobs::{CancelReason, JobState};
        let (ops, worker) = (ServiceOps::default(), SwitchWorker::default());
        let (registry, rx) = jobs_rig();
        let (begun, wake) = std::sync::mpsc::channel::<()>();
        start_a_waiting_change(&worker, &ops, &registry, begun);
        wake.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        worker.cancel();
        let end = next_state(&rx, JobState::is_terminal);
        assert_eq!(
            end.state,
            JobState::Cancelled {
                reason: CancelReason::User
            }
        );
    }

    #[test]
    fn quit_waits_for_the_start_repair_up_to_its_bound() {
        let ops = ServiceOps::default();
        let done = ops.track_repair();
        // The repair ends when the test says so. Quit waits on its own thread: once it has taken the tracked operation over it must
        // still be waiting, because the repair is blocked until `finish`; a Quit that did not wait would already have answered.
        let (finish, finished_rx) = std::sync::mpsc::channel::<()>();
        let slow = std::thread::spawn(move || {
            finished_rx.recv().unwrap();
            done.send(()).ok();
        });
        std::thread::scope(|s| {
            let joiner = s.spawn(|| ops.join_repair(paddock_testkit::HANG_GUARD));
            paddock_testkit::wait_until("quit to begin waiting", || ops.tracked() == 0);
            assert!(!joiner.is_finished(), "quit is waiting for the repair that has not ended");
            finish.send(()).unwrap();
            assert!(joiner.join().unwrap(), "it ended within the bound");
        });
        slow.join().unwrap();
        // A repair that does not end within the bound is given up on, and nothing registered is nothing to wait for.
        let _kept = ops.track_repair();
        assert!(!ops.join_repair(std::time::Duration::from_millis(50)));
        assert!(
            ops.join_repair(std::time::Duration::from_millis(50)),
            "taken once: nothing left to wait for"
        );
    }

    #[test]
    fn the_switch_returns_at_once_while_the_os_step_is_blocked_and_reports_as_a_job() {
        use paddock_jobs::JobState;
        let (ops, worker) = (ServiceOps::default(), SwitchWorker::default());
        let (registry, rx) = jobs_rig();
        let (release, blocked) = std::sync::mpsc::channel::<()>();
        let began = std::time::Instant::now();
        // The OS step (a registration waiting for a lock) blocks until the test lets it go.
        worker
            .start(&ops, register_switch(&registry), move |_| {
                blocked.recv_timeout(std::time::Duration::from_secs(5)).map_err(|e| e.to_string())
            })
            .unwrap();
        assert!(
            began.elapsed() < std::time::Duration::from_secs(2),
            "the save did not wait for the OS step"
        );
        next_state(&rx, |s| matches!(s, JobState::Running { .. }));
        worker
            .start(&ops, register_switch(&registry), |_| Ok(()))
            .expect("a second change waits for the first");
        assert_eq!(
            registry.snapshots().len(),
            1,
            "a waiting change registers its job when it runs, not before"
        );
        release.send(()).unwrap();
        assert_eq!(next_state(&rx, JobState::is_terminal).state, JobState::Done);
        // Free again; a failure carries its message.
        while worker.busy.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::yield_now();
        }
        worker
            .start(&ops, register_switch(&registry), |_| Err("needs approval".into()))
            .unwrap();
        let failed = next_state(&rx, |s| matches!(s, JobState::Failed { .. }));
        assert_eq!(
            failed.state,
            JobState::Failed {
                message: "needs approval".into(),
                limit: None
            }
        );
    }

    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "a test builds its scratch path from the OS temp folder and removes it"
    )]
    fn the_switch_as_the_service_manager_reads_it_missing_is_on_damaged_is_unknown() {
        let dir = std::env::temp_dir().join(format!("dyno-wanted-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        assert_eq!(wanted_setting(&file), Some(true), "a fresh install: the default");
        std::fs::write(&file, "{ not json").unwrap();
        assert_eq!(wanted_setting(&file), None, "unreadable: nothing is registered unattended");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_first_run_is_always_quiet_and_removes_nothing_and_the_settings_switch_never_is() {
        assert!(
            install_is_quiet(true),
            "nobody asked: no System Settings, no removal of a healthy registration"
        );
        assert!(
            !install_is_quiet(false),
            "the Settings switch opens Login Items for what the user has to do"
        );
    }

    #[test]
    fn the_settings_switch_registers_without_waiting_for_the_service_to_answer() {
        assert_eq!(install_timings().start_wait, std::time::Duration::ZERO);
        assert!(
            install_timings().gone_wait > std::time::Duration::ZERO,
            "the removal is still waited for"
        );
    }

    #[test]
    fn the_repair_at_start_reports_what_happened_to_the_caller() {
        use paddock_service::manager::Started;
        use paddock_service::{Hold, Reason, RepairOutcome};
        let repaired = |o| repair_result(Started::Repaired(Reason::VersionChanged, o));
        assert_eq!(repaired(RepairOutcome::Recovered), Ok(true));
        assert!(repaired(RepairOutcome::Registered).unwrap_err().contains("did not answer"));
        assert!(repaired(RepairOutcome::NeedsApproval).unwrap_err().contains("Login Items"));
        assert_eq!(repaired(RepairOutcome::Failed("why".into())), Err("why".into()));
        assert_eq!(repaired(RepairOutcome::Cancelled), Ok(false));
        assert!(repair_result(Started::Busy).unwrap_err().contains("next start"));
        assert_eq!(repair_result(Started::Left(Hold::Healthy)), Ok(false));
        assert_eq!(repair_result(Started::Marked), Ok(false));
    }

    #[test]
    fn a_failed_registration_names_the_login_items_page_and_says_whether_it_was_opened() {
        use paddock_service::ServiceError;
        assert!(registration_words(&ServiceError::NeedsApproval, true).ends_with("(opened for you)"));
        assert!(!registration_words(&ServiceError::SwitchedOff, false).contains("opened for you"));
        assert_eq!(
            registration_words(&ServiceError::Busy, false),
            "Another change is still running; try again in a moment."
        );
    }

    #[test]
    fn the_agent_override_from_the_environment_counts_only_in_a_development_build() {
        let v = std::ffi::OsString::from("/tmp/other-agent");
        assert_eq!(super::agent_override(true, Some(v.as_os_str())), Some(v.as_os_str()));
        assert_eq!(
            super::agent_override(false, Some(v.as_os_str())),
            None,
            "a release build ignores it"
        );
        assert_eq!(super::agent_override(true, None), None);
    }

    /// The wiring of the app's Linux service (`LinuxService`) over the shared simulator: what the transition decides is tested in
    /// `paddock-service` (its model and `model_linux`); here the Settings switch, the repair at start, Quit and the resume reach
    /// it with Dyno's unit, its paths and its words.
    mod linux_wiring {
        use super::super::*;
        use crate::service_ops::{OpCancel, ServiceOps};
        use dyno_agent::service::linux;
        use paddock_env::MapVars;
        use paddock_service::fake_systemd::{Active, FakeSystemd};
        use paddock_service::login::LoginService;
        use paddock_service::testing::FakeSystem;
        use paddock_service::Timings;

        const EXE: &str = "/opt/dyno/dyno-agent";

        struct Rig {
            sys: FakeSystem,
            ctl: FakeSystemd,
            unit: PathBuf,
            data: tempfile::TempDir,
        }

        impl Rig {
            fn new() -> Rig {
                let vars = MapVars::new().with("HOME", "/home/ana");
                let unit = linux::unit_path(&vars).unwrap();
                let sys = FakeSystem::new();
                sys.put_program(EXE);
                let ctl = FakeSystemd::new(&sys, "dyno-agent.service", &unit);
                Rig {
                    sys,
                    ctl,
                    unit,
                    data: tempfile::tempdir().unwrap(),
                }
            }

            fn service(&self) -> LinuxService {
                self.service_with(OpCancel::default())
            }

            fn service_with(&self, cancel: OpCancel) -> LinuxService {
                let (sys, data) = (self.sys.clone(), self.data.path().to_path_buf());
                LinuxService::with(
                    Box::new(move |timings: Timings| {
                        let sys = sys.clone();
                        let factory: linux::SystemFactory = Box::new(move || Box::new(sys.clone()));
                        linux::manager(
                            std::path::Path::new(EXE),
                            &MapVars::new().with("HOME", "/home/ana"),
                            &data,
                            "1.1.0",
                            &factory,
                            timings,
                        )
                    }),
                    cancel,
                )
            }
        }

        #[test]
        fn a_change_after_a_cancelled_one_succeeds() {
            let (rig, ops) = (Rig::new(), ServiceOps::default());
            ops.begin().unwrap().cancel();
            rig.service_with(ops.begin().unwrap()).install().unwrap();
            assert!(rig.sys.file(&rig.unit).is_some());
        }

        #[test]
        fn the_settings_switch_registers_and_removes_the_unit() {
            let rig = Rig::new();
            let mut s = rig.service();
            assert!(!s.installed());
            s.install().unwrap();
            assert!(rig.sys.file(&rig.unit).is_some() && rig.ctl.enabled());
            assert_eq!(rig.ctl.active(), Active::Active);
            assert!(s.installed() && s.supervises());
            s.uninstall().unwrap();
            assert!(rig.sys.file(&rig.unit).is_none() && !rig.ctl.enabled());
            assert!(!s.installed() && !s.supervises());
        }

        #[test]
        fn a_registration_that_is_awaited_is_pending_for_the_link_and_one_that_is_up_or_used_up_is_not() {
            use paddock_service::record::{Phase, RecordFile, Recorded, State};
            let rig = Rig::new();
            let mut s = rig.service();
            s.install().unwrap();
            assert!(!s.pending(), "the unit is up: nothing to wait for");
            rig.ctl.user_stops();
            let record = RecordFile::new(rig.data.path(), dyno_agent::service::LABEL);
            let awaiting = |attempts| Recorded {
                state: State::Attempting {
                    version: "1.1.0".into(),
                    attempts,
                    phase: Phase::Awaiting,
                },
                ..Recorded::default()
            };
            record.store(&awaiting(1)).unwrap();
            assert!(
                s.pending(),
                "registered and not up yet: the link neither starts nor retires an agent"
            );
            record.store(&awaiting(paddock_service::plan::MAX_ATTEMPTS)).unwrap();
            assert!(
                !s.pending(),
                "the attempts are used up: nothing is coming, so the app starts its own agent"
            );
        }

        #[test]
        fn the_repair_at_start_replaces_a_stale_unit_and_leaves_a_missing_or_disabled_one_alone() {
            let rig = Rig::new();
            let mut s = rig.service();
            let answers: AnswerFn = std::sync::Arc::new(|| true);
            assert_eq!(s.repair(answers.clone()), Ok(false), "never installed unasked");
            assert!(rig.sys.file(&rig.unit).is_none());
            s.install().unwrap();
            rig.sys.put_file(&rig.unit, "[Service]\nExecStart=/old/dyno-agent run\n");
            assert_eq!(s.repair(answers.clone()), Ok(true));
            assert!(rig.sys.file(&rig.unit).unwrap().contains(EXE));
            rig.ctl.user_disables();
            rig.sys.put_file(&rig.unit, "[Service]\nExecStart=/old/dyno-agent run\n");
            assert_eq!(s.repair(answers), Ok(false), "the user's disable is theirs");
            assert!(!s.supervises(), "a disabled unit supervises nothing: the app starts its own agent");
        }

        #[test]
        fn quit_and_stop_names_the_unit_and_the_next_start_resumes_it_unless_the_user_disabled_it() {
            let rig = Rig::new();
            let mut s = rig.service();
            assert!(s.stop_mechanism().is_none(), "nothing runs, nothing to stop");
            s.install().unwrap();
            let how = s.stop_mechanism().expect("the running unit");
            assert_eq!(
                how,
                paddock_service::stop::StopMechanism::SystemdUser {
                    unit: "dyno-agent.service".into()
                }
            );
            let mut sys = rig.sys.clone();
            paddock_service::stop::stop(&mut sys, &how, paddock_service::stop::StopTimings::default()).unwrap();
            assert_eq!(rig.ctl.active(), Active::Inactive);
            assert_eq!(s.resume(), Ok(true));
            assert_eq!(rig.ctl.active(), Active::Active);
            // Stopped, then disabled by the user: the resume leaves it.
            let mut sys = rig.sys.clone();
            paddock_service::stop::stop(&mut sys, &how, paddock_service::stop::StopTimings::default()).unwrap();
            rig.ctl.user_disables();
            assert_eq!(s.resume(), Ok(false));
            assert_eq!(rig.ctl.active(), Active::Inactive);
        }
    }

    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Calls(Arc<Mutex<Vec<String>>>);

    struct FakeService {
        calls: Arc<Mutex<Vec<String>>>,
        fail: bool,
    }

    impl paddock_service::login::LoginService for FakeService {
        fn install(&mut self) -> Result<(), String> {
            self.calls.lock().unwrap().push("service install".into());
            if self.fail {
                Err("no".into())
            } else {
                Ok(())
            }
        }
        fn uninstall(&mut self) -> Result<(), String> {
            self.calls.lock().unwrap().push("service uninstall".into());
            Ok(())
        }
    }

    impl AgentService for FakeService {
        fn installed(&mut self) -> bool {
            false
        }
        fn repair(&mut self, _: AnswerFn) -> Result<bool, String> {
            Ok(false)
        }
    }

    #[test]
    fn a_service_that_does_not_say_otherwise_waits_for_nothing() {
        let mut s = FakeService {
            calls: Calls::default().0,
            fail: false,
        };
        assert!(!s.pending(), "the default: nothing is awaited, so the link starts its own agent");
    }

    struct FakeAutostart {
        calls: Arc<Mutex<Vec<String>>>,
        fail: bool,
    }

    impl Autostart for FakeAutostart {
        fn set(&mut self, on: bool) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("autostart {on}"));
            if self.fail {
                Err("denied".into())
            } else {
                Ok(())
            }
        }
    }

    fn apply(on: bool, service_fails: bool, autostart_fails: bool) -> (Result<(), String>, Vec<String>) {
        let calls = Calls::default();
        let mut s = FakeService {
            calls: calls.0.clone(),
            fail: service_fails,
        };
        let mut a = FakeAutostart {
            calls: calls.0.clone(),
            fail: autostart_fails,
        };
        let r = apply_launch_at_login(on, Ok(()), &mut s, &mut a);
        let log = calls.0.lock().unwrap().clone();
        (r, log)
    }

    fn first_run(
        marker: &paddock_service::login::FirstRunMarker,
        sys: &paddock_service::testing::FakeSystem,
        wanted: bool,
        autostart_fails: bool,
    ) -> (Result<FirstRun, String>, Vec<String>) {
        let calls = Calls::default();
        let mut s = FakeService {
            calls: calls.0.clone(),
            fail: false,
        };
        let mut a = FakeAutostart {
            calls: calls.0.clone(),
            fail: autostart_fails,
        };
        let r = apply_first_run(marker, &mut sys.clone(), wanted, Ok(()), &mut s, &mut a);
        let log = calls.0.lock().unwrap().clone();
        (r, log)
    }

    fn marker() -> paddock_service::login::FirstRunMarker {
        paddock_service::login::FirstRunMarker::new(format!("/data/{FIRST_RUN_MARKER}"))
    }

    #[test]
    fn a_first_run_with_the_default_registers_once_and_never_again() {
        let sys = paddock_service::testing::FakeSystem::new();
        let m = marker();
        let (r, log) = first_run(&m, &sys, true, false);
        assert_eq!(r, Ok(FirstRun::Applied));
        assert_eq!(log, vec!["service install".to_string(), "autostart true".to_string()]);
        // The user turns it off; the next start sees the marker and leaves it off.
        let (r, log) = first_run(&m, &sys, true, false);
        assert_eq!(r, Ok(FirstRun::Skipped));
        assert!(log.is_empty());
    }

    fn at_start(settings_text: Option<&str>) -> (Result<FirstRun, String>, Vec<String>, bool) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        if let Some(t) = settings_text {
            std::fs::write(&file, t).unwrap();
        }
        let sys = paddock_service::testing::FakeSystem::new();
        let m = marker();
        let calls = Calls::default();
        let mut s = FakeService {
            calls: calls.0.clone(),
            fail: false,
        };
        let mut a = FakeAutostart {
            calls: calls.0.clone(),
            fail: false,
        };
        let r = apply_first_run_at_start(&file, &m, &mut sys.clone(), Ok(()), &mut s, &mut a);
        let log = calls.0.lock().unwrap().clone();
        (r, log, m.is_set(&mut sys.clone()))
    }

    /// Bug 7538-1: the registration runs before the link starts an agent, from the settings file (no agent to ask yet).
    #[test]
    fn the_first_run_before_any_agent_registers_on_a_fresh_install() {
        let (r, log, set) = at_start(None);
        assert_eq!(r, Ok(FirstRun::Applied));
        assert_eq!(log, vec!["service install".to_string(), "autostart true".to_string()]);
        assert!(set);
    }

    #[test]
    fn the_first_run_before_any_agent_keeps_an_explicit_off_from_an_upgrade() {
        let (r, log, set) = at_start(Some(r#"{"launchAtLogin":false}"#));
        assert_eq!(r, Ok(FirstRun::Skipped));
        assert!(log.is_empty(), "nothing registered: {log:?}");
        assert!(set, "recorded, so the later first sight of the settings is not a first run");
    }

    /// A failed start-time attempt is not repeated by the first sight of the settings in the same run (bug 7538-1, review P2-2):
    /// each repeat opens Login Items on macOS, and a later success would hand the running agent over again.
    #[test]
    fn a_failed_first_run_is_not_tried_again_in_the_same_run() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let sys = paddock_service::testing::FakeSystem::new();
        let m = marker();
        let attempt = FirstRunAttempt::new();
        let calls = Calls::default();
        let try_once = |autostart_fails: bool| {
            let mut s = FakeService {
                calls: calls.0.clone(),
                fail: false,
            };
            let mut a = FakeAutostart {
                calls: calls.0.clone(),
                fail: autostart_fails,
            };
            apply_first_run_once(&attempt, &file, &m, &mut sys.clone(), Ok(()), &mut s, &mut a)
        };
        assert!(try_once(true).is_err(), "the start-time attempt fails");
        let before = calls.0.lock().unwrap().len();
        assert_eq!(try_once(false), Ok(FirstRun::Skipped), "the second caller of this run does nothing");
        assert_eq!(calls.0.lock().unwrap().len(), before, "no second registration in the same run");
        assert!(!m.is_set(&mut sys.clone()), "no marker, so the next start tries again");
        assert!(FirstRunAttempt::new().claim(), "a new process (attempt) tries");
    }

    #[test]
    fn an_explicit_off_is_not_overridden() {
        let sys = paddock_service::testing::FakeSystem::new();
        let m = marker();
        let (r, log) = first_run(&m, &sys, false, false);
        assert_eq!(r, Ok(FirstRun::Skipped));
        assert!(log.is_empty(), "nothing registered: {log:?}");
        assert!(
            m.is_set(&mut sys.clone()),
            "recorded, so a later change is not taken for a first run"
        );
    }

    #[test]
    fn a_failed_first_run_is_tried_again_at_the_next_start() {
        let sys = paddock_service::testing::FakeSystem::new();
        let m = marker();
        assert!(first_run(&m, &sys, true, true).0.is_err());
        assert!(!m.is_set(&mut sys.clone()));
    }

    #[test]
    fn the_switch_registers_or_removes_both() {
        assert_eq!(
            apply(true, false, false),
            (Ok(()), vec!["service install".into(), "autostart true".into()])
        );
        assert_eq!(
            apply(false, false, false),
            (Ok(()), vec!["service uninstall".into(), "autostart false".into()])
        );
    }

    /// The logic (both attempted, one message, the location refusal) is `paddock_service::login`'s and tested there; this
    /// pins Dyno's words and that a refused location registers nothing through Dyno's wiring.
    #[test]
    fn the_wiring_speaks_dynos_words_and_a_refused_location_registers_nothing() {
        let (r, log) = apply(true, true, true);
        assert_eq!(log.len(), 2, "autostart is still attempted");
        let m = r.unwrap_err();
        assert!(
            m.starts_with("Your settings are saved, but the background agent could not be registered: no; and Dyno could not be added to the login items: denied"),
            "{m}"
        );
        let calls = Calls::default();
        let mut s = FakeService {
            calls: calls.0.clone(),
            fail: false,
        };
        let mut a = FakeAutostart {
            calls: calls.0.clone(),
            fail: false,
        };
        let m = apply_launch_at_login(true, Err("move Dyno to Applications".into()), &mut s, &mut a).unwrap_err();
        assert!(
            m.contains("move Dyno to Applications") && m.starts_with("Your settings are saved"),
            "{m}"
        );
        assert!(calls.0.lock().unwrap().is_empty(), "nothing was registered");
        assert_eq!(apply_launch_at_login(false, Err("x".into()), &mut s, &mut a), Ok(()));
        assert_eq!(calls.0.lock().unwrap().len(), 2, "turning off always runs");
    }

    #[test]
    fn the_location_check_names_dyno() {
        let m = current_location_for("/Volumes/Dyno/Dyno.app/Contents/MacOS/dyno-app").unwrap_err();
        assert!(m.starts_with("Dyno is running from a temporary location"), "{m}");
        assert!(current_location_for("/Applications/Dyno.app/Contents/MacOS/dyno-app").is_ok());
    }

    /// Pins the app's own login item as Dyno 1.1 writes it (the names the plugin used, started hidden), so the move into
    /// `paddock_service::login::AppItem` changes nothing a user has registered.
    #[test]
    fn the_apps_own_login_item_keeps_its_names_and_starts_hidden() {
        let sys = paddock_service::testing::FakeSystem::new();
        let xdg = APP_ITEM
            .xdg(
                Some("/home/u/.config".into()),
                Ok("/opt/dyno/dyno-app".into()),
                Box::new(sys.clone()),
            )
            .unwrap();
        assert_eq!(
            xdg.definition(),
            "[Desktop Entry]\nType=Application\nName=Dyno\nExec=\"/opt/dyno/dyno-app\" \"--hidden\"\nTerminal=false\n\
             X-GNOME-Autostart-enabled=true\n"
        );
        let mut xdg = xdg;
        xdg.set(true).unwrap();
        assert!(sys.file(std::path::Path::new("/home/u/.config/autostart/Dyno.desktop")).is_some());

        let run = APP_ITEM
            .windows_run(
                Ok(r"C:\Program Files\Dyno\dyno-app.exe".into()),
                Box::new(sys.clone()),
                Box::new(paddock_service::login::ProcessElevation),
            )
            .unwrap();
        assert_eq!(run.command(), r#""C:\Program Files\Dyno\dyno-app.exe" --hidden"#);
        assert_eq!(APP_ITEM.run_value, "Dyno");
        assert_eq!(
            APP_ITEM.xdg(None, Ok("/opt/dyno/dyno-app".into()), Box::new(sys)).err().as_deref(),
            Some("the home folder or the app's folder cannot be found")
        );
    }

    /// Pins which old file is Dyno 1.0's: label `Dyno`, `Dyno.app/Contents/MacOS/dyno-app`, optionally `--hidden`.
    #[test]
    fn the_legacy_launch_agent_is_dyno_1_0s_file_and_no_other() {
        let file = |label: &str, args: &str| {
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n<key>Label</key>\n<string>{label}</string>\n\
                 <key>AssociatedBundleIdentifiers</key>\n<array></array>\n<key>ProgramArguments</key>\n<array>{args}</array>\n\
                 <key>RunAtLoad</key>\n<true/>\n</dict>\n</plist>"
            )
        };
        let exe = "<string>/Applications/Dyno.app/Contents/MacOS/dyno-app</string>";
        assert!(LEGACY_LAUNCH_AGENT.matches(&file("Dyno", &format!("{exe}<string>--hidden</string>"))));
        assert!(LEGACY_LAUNCH_AGENT.matches(&file("Dyno", exe)));
        assert!(!LEGACY_LAUNCH_AGENT.matches(&file("Spotter", exe)));
        assert!(!LEGACY_LAUNCH_AGENT.matches(&file("Dyno", "<string>/Applications/Dyno.app/Contents/MacOS/dyno</string>")));
        assert!(!LEGACY_LAUNCH_AGENT.matches(&file("Dyno", &format!("{exe}<string>--safe-mode</string>"))));
    }

    #[test]
    fn a_removed_legacy_file_registers_the_new_entry_once_and_nothing_else_does() {
        let dir = tempfile::tempdir().unwrap();
        let old = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n<key>Label</key>\n<string>Dyno</string>\n\
            <key>AssociatedBundleIdentifiers</key>\n<array></array>\n<key>ProgramArguments</key>\n<array><string>/Applications/Dyno.app/Contents/MacOS/dyno-app</string>\
            <string>--hidden</string></array>\n<key>RunAtLoad</key>\n<true/>\n</dict>\n</plist>";
        let calls = Calls::default();
        let mut a = FakeAutostart {
            calls: calls.0.clone(),
            fail: false,
        };
        // Nothing there: nothing registered.
        assert_eq!(
            LEGACY_LAUNCH_AGENT.migrate(dir.path(), Ok(()), &mut a),
            Ok(paddock_service::login::LegacyPlist::Absent)
        );
        // A foreign file is kept and nothing is registered.
        std::fs::write(dir.path().join("Dyno.plist"), "mine").unwrap();
        assert_eq!(
            LEGACY_LAUNCH_AGENT.migrate(dir.path(), Ok(()), &mut a),
            Ok(paddock_service::login::LegacyPlist::Foreign)
        );
        assert!(calls.0.lock().unwrap().is_empty());
        // The old file: removed, new entry registered.
        std::fs::write(dir.path().join("Dyno.plist"), old).unwrap();
        assert_eq!(
            LEGACY_LAUNCH_AGENT.migrate(dir.path(), Ok(()), &mut a),
            Ok(paddock_service::login::LegacyPlist::Removed)
        );
        assert_eq!(*calls.0.lock().unwrap(), vec!["autostart true".to_string()]);
        // From an unstable place the file goes but nothing is registered.
        std::fs::write(dir.path().join("Dyno.plist"), old).unwrap();
        assert_eq!(
            LEGACY_LAUNCH_AGENT.migrate(dir.path(), Err("x".into()), &mut a),
            Ok(paddock_service::login::LegacyPlist::Removed)
        );
        assert_eq!(calls.0.lock().unwrap().len(), 1);
    }

    struct StaleService {
        stale: bool,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl paddock_service::login::LoginService for StaleService {
        fn install(&mut self) -> Result<(), String> {
            self.calls.lock().unwrap().push("service install".into());
            Ok(())
        }
        fn uninstall(&mut self) -> Result<(), String> {
            Ok(())
        }
    }

    impl AgentService for StaleService {
        fn installed(&mut self) -> bool {
            false
        }
        fn repair(&mut self, _: AnswerFn) -> Result<bool, String> {
            self.calls.lock().unwrap().push("repair".into());
            Ok(std::mem::take(&mut self.stale))
        }
    }

    fn upgrade(stale: bool, start_at_login: bool) -> (Result<bool, String>, Vec<String>) {
        let calls = Calls::default();
        let mut s = StaleService {
            stale,
            calls: calls.0.clone(),
        };
        let mut a = FakeAutostart {
            calls: calls.0.clone(),
            fail: false,
        };
        let r = repair_stale_agent_entry(start_at_login, &mut s, &mut a);
        let log = calls.0.lock().unwrap().clone();
        (r, log)
    }

    #[test]
    fn upgrading_from_1_0_0_with_start_at_login_on_puts_the_apps_entry_back() {
        assert_eq!(
            upgrade(true, true),
            (Ok(true), vec!["repair".to_string(), "autostart true".to_string()])
        );
    }

    #[test]
    fn upgrading_from_1_0_0_with_start_at_login_off_only_removes_the_stale_entry() {
        assert_eq!(upgrade(true, false), (Ok(true), vec!["repair".to_string()]));
    }

    #[test]
    fn without_a_stale_entry_the_apps_entry_is_left_alone() {
        assert_eq!(upgrade(false, true), (Ok(false), vec!["repair".to_string()]));
        assert_eq!(upgrade(false, false), (Ok(false), vec!["repair".to_string()]));
    }

    // ---- the Settings switch while a change runs, a cancel in the middle of one, and a stale app item

    fn switch_change(
        on: bool,
        calls: &Calls,
        gate: Option<std::sync::mpsc::Receiver<()>>,
    ) -> impl FnOnce(&crate::service_ops::OpCancel) -> Result<(), String> + Send + 'static {
        let log = calls.0.clone();
        move |_| {
            if let Some(gate) = gate {
                gate.recv_timeout(std::time::Duration::from_secs(5)).map_err(|e| e.to_string())?;
            }
            let mut s = FakeService {
                calls: log.clone(),
                fail: false,
            };
            let mut a = FakeAutostart { calls: log, fail: false };
            apply_launch_at_login(on, Ok(()), &mut s, &mut a)
        }
    }

    fn idle(worker: &SwitchWorker) {
        while worker.busy.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_change_made_while_another_runs_is_queued_and_on_then_off_ends_off_with_no_app_item() {
        let (ops, worker) = (ServiceOps::default(), SwitchWorker::default());
        let (registry, _rx) = jobs_rig();
        let calls = Calls::default();
        let (release, gate) = std::sync::mpsc::channel::<()>();
        worker
            .start(&ops, register_switch(&registry), switch_change(true, &calls, Some(gate)))
            .unwrap();
        worker
            .start(&ops, register_switch(&registry), switch_change(false, &calls, None))
            .expect("a change while one runs waits its turn; it is not dropped");
        release.send(()).unwrap();
        idle(&worker);
        assert_eq!(
            *calls.0.lock().unwrap(),
            ["service install", "autostart true", "service uninstall", "autostart false"],
            "the last change decides"
        );
    }

    #[test]
    fn of_several_changes_made_while_one_runs_only_the_last_runs() {
        let (ops, worker) = (ServiceOps::default(), SwitchWorker::default());
        let (registry, _rx) = jobs_rig();
        let calls = Calls::default();
        let (release, gate) = std::sync::mpsc::channel::<()>();
        worker
            .start(&ops, register_switch(&registry), switch_change(true, &calls, Some(gate)))
            .unwrap();
        worker
            .start(&ops, register_switch(&registry), switch_change(false, &calls, None))
            .unwrap();
        worker
            .start(&ops, register_switch(&registry), switch_change(true, &calls, None))
            .unwrap();
        release.send(()).unwrap();
        idle(&worker);
        let log = calls.0.lock().unwrap().clone();
        assert!(!log.contains(&"service uninstall".to_string()), "a superseded change ran: {log:?}");
        assert_eq!(log.last().map(String::as_str), Some("autostart true"));
    }

    #[test]
    fn the_applied_value_follows_the_change_and_goes_back_when_it_did_not_finish() {
        let value = Mutex::new(Some(true));
        // Remembered at once, before the worker starts (a second save sees it).
        *value.lock().unwrap() = Some(false);
        assert!(settle_applied(&value, false, Some(true), true, false));
        assert_eq!(*value.lock().unwrap(), Some(false), "applied");
        assert!(!settle_applied(&value, false, Some(true), false, false));
        assert_eq!(*value.lock().unwrap(), Some(true), "a failed change is not remembered as applied");
        *value.lock().unwrap() = Some(false);
        assert!(!settle_applied(&value, false, Some(true), true, true));
        assert_eq!(
            *value.lock().unwrap(),
            Some(true),
            "a cancelled change is not remembered as applied"
        );
        // A newer change has set its own value meanwhile: this one leaves it alone.
        *value.lock().unwrap() = Some(true);
        settle_applied(&value, false, Some(true), false, false);
        assert_eq!(*value.lock().unwrap(), Some(true));
        *value.lock().unwrap() = Some(false);
        settle_applied(&value, true, Some(false), false, false);
        assert_eq!(*value.lock().unwrap(), Some(false), "not this change's value");
    }

    struct CancelsAfterInstall {
        inner: FakeService,
        cancel: crate::service_ops::OpCancel,
    }

    impl paddock_service::login::LoginService for CancelsAfterInstall {
        fn install(&mut self) -> Result<(), String> {
            let r = self.inner.install();
            self.cancel.cancel();
            r
        }
        fn uninstall(&mut self) -> Result<(), String> {
            self.inner.uninstall()
        }
    }

    impl AgentService for CancelsAfterInstall {
        fn installed(&mut self) -> bool {
            false
        }
        fn repair(&mut self, _: AnswerFn) -> Result<bool, String> {
            Ok(false)
        }
    }

    #[test]
    fn a_cancel_after_the_agent_install_registers_no_app_item_but_a_removal_still_runs() {
        let calls = Calls::default();
        let cancel = crate::service_ops::OpCancel::default();
        let mut service = CancelsAfterInstall {
            inner: FakeService {
                calls: calls.0.clone(),
                fail: false,
            },
            cancel: cancel.clone(),
        };
        let mut plain = FakeAutostart {
            calls: calls.0.clone(),
            fail: false,
        };
        let mut guarded = UnlessCancelled::new(&mut plain, &cancel);
        apply_launch_at_login(true, Ok(()), &mut service, &mut guarded).unwrap();
        assert_eq!(
            *calls.0.lock().unwrap(),
            ["service install"],
            "the cancelled change still registered the app"
        );
        // Off is never held back by a cancel: the item must not stay behind.
        guarded.set(false).unwrap();
        assert_eq!(calls.0.lock().unwrap().last().map(String::as_str), Some("autostart false"));
    }

    #[test]
    fn at_start_with_the_switch_off_the_apps_own_login_item_is_removed_and_with_it_on_it_is_written() {
        let calls = Calls::default();
        let mut a = FakeAutostart {
            calls: calls.0.clone(),
            fail: false,
        };
        assert_eq!(ensure_app_entry(false, &mut a), Ok(false));
        assert_eq!(
            *calls.0.lock().unwrap(),
            ["autostart false"],
            "a stale item stays behind a switch that is off"
        );
        assert_eq!(ensure_app_entry(true, &mut a), Ok(true));
        assert_eq!(calls.0.lock().unwrap().last().map(String::as_str), Some("autostart true"));
    }

    #[test]
    fn the_first_run_attempt_belongs_to_the_service_operations_not_to_the_process() {
        let (first, second) = (ServiceOps::default(), ServiceOps::default());
        assert!(first.first_run_attempt().claim());
        assert!(!first.first_run_attempt().claim(), "once per app");
        assert!(second.first_run_attempt().claim(), "another app state starts clean");
    }

    #[test]
    fn a_change_cancelled_after_the_agent_install_ends_with_no_app_item_and_the_old_value() {
        let calls = Calls::default();
        let cancel = crate::service_ops::OpCancel::default();
        let value = Mutex::new(Some(true));
        let mut service = CancelsAfterInstall {
            inner: FakeService {
                calls: calls.0.clone(),
                fail: false,
            },
            cancel: cancel.clone(),
        };
        let mut plain = FakeAutostart {
            calls: calls.0.clone(),
            fail: false,
        };
        let r = apply_switch_change(true, Ok(()), &mut service, &mut plain, &cancel, &value, Some(false));
        assert_eq!(r, Ok(()));
        assert_eq!(
            *calls.0.lock().unwrap(),
            ["service install"],
            "the cancelled change registered the app"
        );
        assert_eq!(*value.lock().unwrap(), Some(false), "a cancelled change was remembered as on");
        // Not cancelled: both registrations, remembered.
        let calls = Calls::default();
        let value = Mutex::new(Some(true));
        let mut service = FakeService {
            calls: calls.0.clone(),
            fail: false,
        };
        let mut plain = FakeAutostart {
            calls: calls.0.clone(),
            fail: false,
        };
        apply_switch_change(
            true,
            Ok(()),
            &mut service,
            &mut plain,
            &crate::service_ops::OpCancel::default(),
            &value,
            Some(false),
        )
        .unwrap();
        assert_eq!(*calls.0.lock().unwrap(), ["service install", "autostart true"]);
        assert_eq!(*value.lock().unwrap(), Some(true));
    }

    #[test]
    fn a_change_the_worker_refused_puts_the_remembered_value_back_and_a_started_one_leaves_it() {
        let value = Mutex::new(Some(true));
        assert!(undo_if_refused(Err("quitting".into()), &value, true, Some(false)).is_err());
        assert_eq!(*value.lock().unwrap(), Some(false));
        *value.lock().unwrap() = Some(true);
        assert!(undo_if_refused(Ok(()), &value, true, Some(false)).is_ok());
        assert_eq!(*value.lock().unwrap(), Some(true));
    }

    #[test]
    fn only_a_switch_that_is_off_removes_the_app_item() {
        let calls = Calls::default();
        let mut a = FakeAutostart {
            calls: calls.0.clone(),
            fail: false,
        };
        remove_stale_app_item(true, &mut a).unwrap();
        assert!(
            calls.0.lock().unwrap().is_empty(),
            "an item behind a switch that is on is left alone"
        );
        remove_stale_app_item(false, &mut a).unwrap();
        assert_eq!(*calls.0.lock().unwrap(), ["autostart false"]);
        let mut failing = FakeAutostart {
            calls: calls.0.clone(),
            fail: true,
        };
        assert!(
            remove_stale_app_item(false, &mut failing).is_err(),
            "a failed removal is said, not hidden"
        );
    }

    #[test]
    fn a_worker_that_panics_is_free_for_the_next_change() {
        let (ops, worker) = (ServiceOps::default(), SwitchWorker::default());
        let (registry, _rx) = jobs_rig();
        worker
            .start(&ops, register_switch(&registry), |_| -> Result<(), String> {
                panic!("a test panic")
            })
            .unwrap();
        idle(&worker);
        let (done, seen) = std::sync::mpsc::channel();
        worker
            .start(&ops, register_switch(&registry), move |_| done.send(()).map_err(|e| e.to_string()))
            .unwrap();
        seen.recv_timeout(std::time::Duration::from_secs(5))
            .expect("the next change did not run");
    }

    #[test]
    fn a_change_that_panics_before_its_job_exists_is_dropped_and_the_next_one_runs() {
        let (ops, worker) = (ServiceOps::default(), SwitchWorker::default());
        let (registry, _rx) = jobs_rig();
        worker
            .start(
                &ops,
                || -> paddock_jobs::JobContext { panic!("a test panic while registering") },
                |_| Ok(()),
            )
            .unwrap();
        let (done, seen) = std::sync::mpsc::channel();
        worker
            .start(&ops, register_switch(&registry), move |_| done.send(()).map_err(|e| e.to_string()))
            .unwrap();
        seen.recv_timeout(std::time::Duration::from_secs(5))
            .expect("the change after the panic did not run");
        idle(&worker);
    }
}
