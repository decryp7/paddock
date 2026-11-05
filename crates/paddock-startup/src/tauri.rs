//! The Tauri adapter of `paddock-startup` (feature `tauri`): the managed [`Startup`] state with the app's hooks, the page
//! load and web-content-terminated handlers, the ticker thread that applies the [`Watchdog`]'s actions, and the
//! commands of [`COMMANDS`](crate::COMMANDS).
//!
//! ```ignore
//! let startup = paddock_startup::tauri::Startup::builder()
//!     .safe_mode(|app, _label| lifecycle::relaunch(app, true).map_err(|e| e.to_string()))
//!     .on_unresponsive(paddock_dialogs::tauri::page_unresponsive) // a dialog whose page cannot answer may be closed
//!     .native_dialogs()                                            // feature `dialog`; the app registers tauri-plugin-dialog
//!     .build()?;
//! let builder = paddock_startup::tauri::install(tauri::Builder::default(), startup)
//!     .invoke_handler(paddock_commands::tauri::Guard::new(&paddock_startup::COMMANDS).wrap(paddock_startup::tauri::invoke_handler()));
//! ```
//!
//! The ticker starts with the first watched page load (or [`start`]) and never runs on the main thread; it reloads pages
//! and shows the question through `run_on_main_thread`. An app with its own table lists the four commands there instead
//! (`Quiet [WINDOW] => paddock_startup::tauri::window_ready`, ...); a dialog's base contract includes [`DIALOG_BASE`].

#[cfg(feature = "dialog")]
pub mod native;
#[cfg(feature = "dialog")]
pub use native::NativeAsker;

use crate::{
    Action, BootErrorKind, Checkpoints, Choice, Config, ConfigError, Lifecycle, PING_EVENT, Pong,
    Prompt, Question, ReadyReply, Recovery, RestorePlan, StartReport, Watchdog, clean_error,
    ready_reply,
};
use ::tauri::ipc::Invoke;
use ::tauri::webview::{PageLoadEvent, PageLoadPayload};
use ::tauri::{AppHandle, Emitter, Manager, Runtime, Webview, WebviewWindow};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Instant;

/// The commands every dialog may call whatever its contract (the dialog layer adds them to each dialog's base set).
pub const DIALOG_BASE: &[&str] = &["window_ready", "window_boot_error", "window_pong"];

/// How many times a question's dialog is looked for to take it down, and how long between tries (a dialog may not be up yet).
pub(crate) const DISMISS_TRIES: u32 = 10;
pub(crate) const DISMISS_EVERY: std::time::Duration = std::time::Duration::from_millis(100);

/// Called with `true` when the watchdog gives up on a window's page (start-up question, or unresponsive after ready) and
/// with `false` when the page answers again: `paddock_dialogs::tauri::page_unresponsive` lets a dialog be closed by the
/// OS close meanwhile (rule 19).
pub type UnresponsiveHook<R> = dyn Fn(&AppHandle<R>, &str, bool) + Send + Sync;
/// Restarts the app in Safe Mode for window `label` (the app's relaunch); an error is logged.
/// Writes a problem report without the stuck window (`StartupBuilder::report`); it shows its own native save dialog, so it runs
/// off the main thread. Returns what to tell the log.
pub type ReportHook<R> = dyn Fn(&AppHandle<R>, &str) -> Result<(), String> + Send + Sync;
/// Whether a window counts as visible to the watchdog (`StartupBuilder::window_visibility`); the default is [`visibility`] over the
/// window's own answers. A seam for tests: the mock runtime reports every window visible.
pub type VisibleProbe<R> = dyn Fn(&WebviewWindow<R>) -> bool + Send + Sync;
/// Restarts in Safe Mode (`StartupBuilder::safe_mode`).
pub type SafeModeHook<R> = dyn Fn(&AppHandle<R>, &str) -> Result<(), String> + Send + Sync;
/// Restarts the whole app, keeping its session (the app's relaunch: `|app| app.restart()`, which does not return on
/// success); an error is reported to the calling page and logged.
pub type RelaunchHook<R> = dyn Fn(&AppHandle<R>) -> Result<(), String> + Send + Sync;
/// Called after a window reported ready (with why the shell reloaded it): e.g. start the work the first paint did not need.
pub type ReadyHook<R> = dyn Fn(&Webview<R>, Option<Recovery>) + Send + Sync;
/// Whether a webview (label, its window's label) is one of the app's own pages the watchdog watches.
pub type WatchFilter = dyn Fn(&str, &str) -> bool + Send + Sync;

/// Shows the native question for window `label`, owned by that window, and calls `answer` once with the user's choice.
pub trait Asker<R: Runtime>: Send + Sync + 'static {
    /// `seq` is the question's identity: the one [`Asker::dismiss`] is later given.
    fn ask(
        &self,
        app: &AppHandle<R>,
        label: &str,
        seq: u64,
        prompt: Prompt,
        answer: Box<dyn FnOnce(Choice) + Send>,
    );

    /// Takes down question `seq` shown for window `label` because it is no longer needed (the page recovered). Runs on the main
    /// thread. Returns whether the dialog is gone: `false` when it could not be found or closed (not shown yet, queued behind another
    /// sheet, or an asker that cannot dismiss, the default). It is then retried a few times; if it stays up, the question stays
    /// counted as open (the window is not asked a second time) and its answer is ignored.
    fn dismiss(&self, _app: &AppHandle<R>, _label: &str, _seq: u64) -> bool {
        false
    }
}

/// Why a start-up command failed (the command's error; camelCase on the wire).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupError {
    pub code: StartupErrorCode,
    /// For the log and the developer; the page never shows it.
    pub message: String,
}

/// The kind of [`StartupError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupErrorCode {
    /// The app did not install [`Startup`] (a wiring bug).
    NotInstalled,
    /// The request was malformed (a UI checkpoint that is not an object).
    Invalid,
    /// The app did not give the contract a [`Lifecycle`] (`StartupBuilder::lifecycle`): there is nothing to recover.
    NoLifecycle,
    /// The app has no Safe Mode restart (`StartupBuilder::safe_mode`).
    SafeModeUnavailable,
    /// The Safe Mode restart failed; the notice stays so the user can choose again.
    SafeModeFailed,
    /// The app has no relaunch (`StartupBuilder::relaunch`).
    RelaunchUnavailable,
    /// The relaunch failed; the app keeps running.
    RelaunchFailed,
}

impl StartupError {
    fn not_installed() -> Self {
        StartupError {
            code: StartupErrorCode::NotInstalled,
            message: "the start-up contract is not installed in this app (paddock_startup::tauri::install)".into(),
        }
    }
}

/// The default [`WatchFilter`]: a window's own webview (not a second webview in it, such as an embedded browser) whose
/// label is in the shell's scheme (`main`, `w-<n>`, `d-<host>-<n>`, `p-<host>-<n>`). Tray popovers (`tray`, `tray-<item>`)
/// are not watched: they live only while they have the focus and cannot call the start-up commands (`WINDOW` only).
pub fn watched_by_default(webview: &str, window: &str) -> bool {
    webview == window
        && paddock_commands::class_of(webview)
            .is_some_and(|c| c != paddock_commands::WindowClass::Tray)
}

/// Builds a [`Startup`]: the timings and the app's hooks.
pub struct StartupBuilder<R: Runtime> {
    config: Config,
    watch: Box<WatchFilter>,
    safe_mode: Option<Box<SafeModeHook<R>>>,
    report: Option<Arc<ReportHook<R>>>,
    visible_probe: Option<Box<VisibleProbe<R>>>,
    relaunch: Option<Box<RelaunchHook<R>>>,
    unresponsive: Option<Box<UnresponsiveHook<R>>>,
    on_ready: Option<Box<ReadyHook<R>>>,
    asker: Option<Arc<dyn Asker<R>>>,
    lifecycle: Option<Lifecycle>,
}

impl<R: Runtime> StartupBuilder<R> {
    /// Other timings than ADR 0058's (checked by [`StartupBuilder::build`]).
    pub fn config(mut self, config: Config) -> Self {
        self.config = config;
        self
    }

    /// Which webviews are watched (default: [`watched_by_default`]).
    pub fn watch(mut self, filter: impl Fn(&str, &str) -> bool + Send + Sync + 'static) -> Self {
        self.watch = Box::new(filter);
        self
    }

    /// The app's Safe Mode restart: the start-up question then offers Restart in Safe Mode.
    pub fn safe_mode(
        mut self,
        restart: impl Fn(&AppHandle<R>, &str) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.safe_mode = Some(Box::new(restart));
        self
    }

    /// The app's relaunch: `app_relaunch` (a language change, an update) saves the session, then runs it. It should not
    /// return on success (`|app| app.restart()`); the app then starts again and restores what it checkpointed.
    pub fn relaunch(
        mut self,
        restart: impl Fn(&AppHandle<R>) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.relaunch = Some(Box::new(restart));
        self
    }

    /// Told when a page stops answering and when it answers again (see [`UnresponsiveHook`]).
    pub fn on_unresponsive(
        mut self,
        hook: impl Fn(&AppHandle<R>, &str, bool) + Send + Sync + 'static,
    ) -> Self {
        self.unresponsive = Some(Box::new(hook));
        self
    }

    /// Called after each `window_ready` (see [`ReadyHook`]).
    pub fn on_ready(
        mut self,
        hook: impl Fn(&Webview<R>, Option<Recovery>) + Send + Sync + 'static,
    ) -> Self {
        self.on_ready = Some(Box::new(hook));
        self
    }

    /// Shows the questions through `asker`. Without one they are only logged and the window keeps waiting.
    pub fn asker(mut self, asker: impl Asker<R>) -> Self {
        self.asker = Some(Arc::new(asker));
        self
    }

    /// Shows the questions as native message dialogs (`tauri-plugin-dialog`, which the app registers).
    #[cfg(feature = "dialog")]
    pub fn native_dialogs(self) -> Self {
        self.asker(NativeAsker::default())
    }

    /// The run's lifecycle ([`Lifecycle::begin`], called as early as possible in `main`): the running marker, the
    /// autosaved session and the restore offer behind the `recovery_*` commands. Without one the contract keeps UI
    /// checkpoints in memory only and reports a normal start. [`end_run`] ends it on a clean exit.
    pub fn lifecycle(mut self, lifecycle: Lifecycle) -> Self {
        self.lifecycle = Some(lifecycle);
        self
    }

    /// The state to install, or why the timings are inconsistent.
    /// The app's way to write a problem report without the window: the questions then offer Save a problem report…, whose answer
    /// calls `write` on its own thread (it asks where to save with a native dialog).
    pub fn report(
        mut self,
        write: impl Fn(&AppHandle<R>, &str) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.report = Some(Arc::new(write));
        self
    }

    /// Another way to decide whether a window is visible (tests; an app with windows the OS cannot report on).
    pub fn window_visibility(
        mut self,
        probe: impl Fn(&WebviewWindow<R>) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.visible_probe = Some(Box::new(probe));
        self
    }

    pub fn build(self) -> Result<Startup<R>, ConfigError> {
        Ok(Startup {
            dog: Mutex::new(Watchdog::new(self.config.checked()?)),
            checkpoints: Mutex::new(Checkpoints::default()),
            watch: self.watch,
            safe_mode: self.safe_mode,
            report: self.report,
            visible_probe: self.visible_probe,
            relaunch: self.relaunch,
            unresponsive: self.unresponsive,
            on_ready: self.on_ready,
            asker: self.asker,
            lifecycle: self.lifecycle,
            session_refusal_logged: AtomicBool::new(false),
            started: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
        })
    }
}

/// The start-up contract's managed state: the watchdog, the checkpoints and the app's hooks.
pub struct Startup<R: Runtime> {
    dog: Mutex<Watchdog>,
    checkpoints: Mutex<Checkpoints>,
    watch: Box<WatchFilter>,
    safe_mode: Option<Box<SafeModeHook<R>>>,
    report: Option<Arc<ReportHook<R>>>,
    visible_probe: Option<Box<VisibleProbe<R>>>,
    relaunch: Option<Box<RelaunchHook<R>>>,
    unresponsive: Option<Box<UnresponsiveHook<R>>>,
    on_ready: Option<Box<ReadyHook<R>>>,
    asker: Option<Arc<dyn Asker<R>>>,
    lifecycle: Option<Lifecycle>,
    /// A state the session refuses (too large) is logged once, not at every checkpoint.
    session_refusal_logged: AtomicBool,
    started: AtomicBool,
    stopped: AtomicBool,
}

/// A lock that recovers from poisoning: the watchdog's state is a plain map, valid after any panic.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl<R: Runtime> Startup<R> {
    /// A builder with ADR 0058's timings and no hooks.
    pub fn builder() -> StartupBuilder<R> {
        StartupBuilder {
            config: Config::default(),
            watch: Box::new(watched_by_default),
            safe_mode: None,
            report: None,
            visible_probe: None,
            relaunch: None,
            unresponsive: None,
            on_ready: None,
            asker: None,
            lifecycle: None,
        }
    }

    /// The run's lifecycle, if the app gave one.
    pub fn lifecycle(&self) -> Option<&Lifecycle> {
        self.lifecycle.as_ref()
    }

    /// Runs `f` on the watchdog.
    pub fn with<T>(&self, f: impl FnOnce(&mut Watchdog) -> T) -> T {
        f(&mut lock(&self.dog))
    }

    /// Whether the webview `webview` in window `window` is watched.
    pub fn watches(&self, webview: &str, window: &str) -> bool {
        (self.watch)(webview, window)
    }

    /// Stops the ticker (it ends at its next tick).
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }

    fn hook(&self, app: &AppHandle<R>, label: &str, unresponsive: bool) {
        if let Some(h) = &self.unresponsive {
            h(app, label, unresponsive);
        }
    }

    /// One tick at `now`: forgets closed windows, updates visibility, and applies what is due. The ticker calls it every
    /// [`Config::tick`]; tests call it with a fake clock.
    pub fn tick(&self, app: &AppHandle<R>, now: Instant) {
        let labels = self.with(|w| w.labels());
        let mut gone = Vec::new();
        let mut shown = Vec::new();
        for label in labels {
            match app.get_webview_window(&label) {
                None => gone.push(label),
                Some(window) => {
                    let visible = match &self.visible_probe {
                        Some(probe) => probe(&window),
                        None => visibility(window.is_visible().ok(), window.is_minimized().ok()),
                    };
                    shown.push((label, visible));
                }
            }
        }
        {
            let mut checkpoints = lock(&self.checkpoints);
            gone.iter().for_each(|l| checkpoints.closed(l));
        }
        let actions = self.with(|w| {
            gone.iter().for_each(|l| w.closed(l));
            shown.iter().for_each(|(l, v)| w.set_visible(l, *v, now));
            w.due(now)
        });
        for action in actions {
            self.apply(app, action);
        }
    }

    fn apply(&self, app: &AppHandle<R>, action: Action) {
        match action {
            Action::Reload(label) => {
                let timeout = self.with(|w| w.config().ready_timeout.as_secs());
                log::error!(Window = label.as_str(), TimeoutS = timeout; "Window {label} did not finish starting in time; reloading it");
                reload(app, label);
            }
            Action::AskStartup(label) => {
                log::error!(Window = label.as_str(); "Window {label} did not start after a reload; asking the user");
                self.hook(app, &label, true);
                let question = Question::Startup {
                    safe_mode: self.safe_mode.is_some(),
                };
                self.ask(app, label, question);
            }
            Action::Ping { label, seq } => {
                let sent = app.emit_to(
                    label.as_str(),
                    PING_EVENT,
                    serde_json::json!({ "seq": seq }),
                );
                logged(sent, &label, "Could not ping the window");
            }
            Action::AskHung(label) => {
                log::error!(Window = label.as_str(); "Window {label} has not answered for a long time; asking the user");
                self.ask(app, label, Question::Unresponsive);
            }
            Action::Unresponsive(label) => {
                let (n, every) = self.with(|w| {
                    (
                        w.config().missed_pings,
                        w.config().ping_interval.as_millis() as u64,
                    )
                });
                log::error!(Window = label.as_str(), MissedPings = n, PingMs = every; "Window {label} stopped responding; the user is asked if it stays silent for a long time");
                self.hook(app, &label, true);
            }
        }
    }

    fn ask(&self, app: &AppHandle<R>, label: String, question: Question) {
        let Some(asker) = self.asker.clone() else {
            log::warn!(Window = label.as_str(); "No asker is installed; window {label} keeps waiting");
            return;
        };
        let Some(seq) = self.with(|w| w.question_seq(&label)) else {
            log::info!(Window = label.as_str(); "Question about window {label} was closed before it was shown");
            return;
        };
        let prompt = Prompt::new(question, &app.package_info().name);
        let prompt = if self.report.is_some() {
            prompt.with_report()
        } else {
            prompt
        };
        let answer_app = app.clone();
        let answer_label = label.clone();
        let answer: Box<dyn FnOnce(Choice) + Send> =
            Box::new(move |choice| answered(&answer_app, &answer_label, seq, choice));
        let app2 = app.clone();
        let shown = app.run_on_main_thread(move || {
            // The page may have recovered while this was queued: then the question is not shown at all.
            let startup = app2.try_state::<Startup<R>>();
            if startup.as_ref().and_then(|s| s.with(|w| w.question_seq(&label))) != Some(seq) {
                log::info!(Window = label.as_str(), Seq = seq; "Question {seq} about window {label} was closed before it was shown");
                if let Some(s) = startup {
                    s.with(|w| w.question_dismissed(&label, seq));
                }
                return;
            }
            asker.ask(&app2, &label, seq, prompt, answer)
        });
        logged(shown, "", "Could not show the not-responding question");
    }

    /// The page of window `label` is fine again (`why`): a question about it that is still on screen is closed and taken down. Its
    /// answer, if the user pressed a button just before, is ignored ([`answered`]).
    fn question_closed(&self, app: &AppHandle<R>, label: &str, why: &str) {
        let Some(seq) = self.with(|w| w.close_question(label)) else {
            return;
        };
        log::info!(Window = label, Seq = seq; "Question {seq} about window {label} closed: {why}");
        let Some(asker) = self.asker.clone() else {
            self.with(|w| w.question_dismissed(label, seq));
            return;
        };
        // The dialog may not be up yet (the OS shows it from another thread): look for it a few times, off the main thread.
        let (app, label) = (app.clone(), label.to_string());
        let spawned = std::thread::Builder::new().name("paddock-startup-dismiss".into()).spawn(move || {
            for _ in 0..DISMISS_TRIES {
                let (tx, rx) = std::sync::mpsc::channel();
                let (app2, label2, asker2) = (app.clone(), label.clone(), asker.clone());
                let queued = app.run_on_main_thread(move || {
                    let _ = tx.send(asker2.dismiss(&app2, &label2, seq));
                });
                if queued.is_err() {
                    break;
                }
                if rx.recv().unwrap_or(false) {
                    log::info!(Window = label.as_str(), Seq = seq; "Question {seq} about window {label}: dialog taken down");
                    if let Some(s) = app.try_state::<Startup<R>>() {
                        s.with(|w| w.question_dismissed(&label, seq));
                    }
                    return;
                }
                std::thread::sleep(DISMISS_EVERY);
            }
            log::warn!(Window = label.as_str(), Seq = seq; "Question {seq} about window {label} could not be closed; it stays up until answered and its answer is ignored");
        });
        logged(spawned, "", "Could not start the dialog dismissal");
    }

    /// The page in `webview` reported ready: the reply, the hooks, the log line.
    pub fn ready(&self, webview: &Webview<R>) -> ReadyReply {
        let label = webview.label().to_string();
        let reply = {
            let checkpoints = lock(&self.checkpoints);
            self.with(|w| ready_reply(w, &checkpoints, &label, Instant::now()))
        };
        self.hook(webview.app_handle(), &label, false);
        self.question_closed(webview.app_handle(), &label, "the page is ready");
        if let Some(h) = &self.on_ready {
            h(webview, reply.recovered);
        }
        log::info!(Window = label.as_str(), Recovered = format!("{:?}", reply.recovered).as_str(), RestoresUi = reply.ui.is_some(); "Window {label} ready");
        reply
    }

    /// The page in `webview` answered ping `seq`.
    pub fn pong(&self, webview: &Webview<R>, seq: u64) {
        let label = webview.label();
        if self.with(|w| w.pong(label, seq, Instant::now())) == Pong::Recovered {
            log::info!(Window = label; "Window {label} responds again");
            self.hook(webview.app_handle(), label, false);
            self.question_closed(webview.app_handle(), label, "the page responds again");
        }
    }

    /// A boot error of the page in window `label`: one bounded log line, at most [`Config::max_boot_errors`] per load.
    pub fn boot_error(&self, label: &str, kind: &str, message: &str) {
        let (admitted, max_len) =
            self.with(|w| (w.admit_boot_error(label), w.config().max_error_len));
        if !admitted {
            return;
        }
        let kind = BootErrorKind::parse(kind).as_str();
        log::error!(Window = label, Kind = kind; "Uncaught {kind} in window {label}: {}", clean_error(message, max_len));
    }

    /// Keeps `ui` as window `label`'s UI state checkpoint.
    pub fn checkpoint(&self, label: &str, ui: serde_json::Value) -> Result<(), StartupError> {
        if !ui.is_object() {
            return Err(StartupError {
                code: StartupErrorCode::Invalid,
                message: "the UI state must be an object".into(),
            });
        }
        if let Some(life) = &self.lifecycle
            && let Err(e) = life.checkpoint(label, ui.clone())
            && !self.session_refusal_logged.swap(true, Ordering::Relaxed)
        {
            log::warn!(Window = label; "Window {label}'s UI state is not kept in the session: {e}");
        }
        lock(&self.checkpoints).put(label, ui);
        Ok(())
    }

    /// How this run started and what the restore notice offers (a normal start without a lifecycle). The notice is the
    /// `main` window's: another window hears about safe mode only.
    pub fn recovery_status(&self, label: &str) -> StartReport {
        match &self.lifecycle {
            Some(life) => {
                let mut report = life.report(self.safe_mode.is_some());
                if label != "main" {
                    report.unclean = None;
                }
                report
            }
            None => StartReport {
                safe_mode: false,
                unclean: None,
            },
        }
    }

    fn life(&self) -> Result<&Lifecycle, StartupError> {
        self.lifecycle.as_ref().ok_or_else(|| StartupError {
            code: StartupErrorCode::NoLifecycle,
            message: "the app gave the start-up contract no lifecycle (StartupBuilder::lifecycle)"
                .into(),
        })
    }

    /// The user chose Restore in window `label`.
    pub fn recovery_restore(&self, label: &str) -> Result<RestorePlan, StartupError> {
        let plan = self.life()?.restore(label);
        log::info!(Window = label, Windows = plan.windows.len() as u64; "Restoring the previous session");
        Ok(plan)
    }

    /// The user chose Start fresh.
    pub fn recovery_start_fresh(&self) -> Result<(), StartupError> {
        self.life()?.start_fresh();
        log::info!("Starting fresh; the previous session was forgotten");
        Ok(())
    }

    /// The user chose Safe mode in window `label`: the app's restart runs, and only when it worked is the notice settled.
    pub fn recovery_safe_mode(&self, app: &AppHandle<R>, label: &str) -> Result<(), StartupError> {
        let life = self.life()?;
        let Some(restart) = &self.safe_mode else {
            return Err(StartupError {
                code: StartupErrorCode::SafeModeUnavailable,
                message: "the app has no Safe Mode restart (StartupBuilder::safe_mode)".into(),
            });
        };
        restart(app, label).map_err(|message| {
            log::error!(Window = label; "Could not restart in Safe Mode: {message}");
            StartupError {
                code: StartupErrorCode::SafeModeFailed,
                message,
            }
        })?;
        life.chose_safe_mode();
        Ok(())
    }

    /// The calling window asked to restart the whole app: every checkpoint so far is written to the session first, then
    /// the app's relaunch runs. Without a relaunch, or when it fails, the app keeps running and the page is told why.
    pub fn app_relaunch(&self, app: &AppHandle<R>, label: &str) -> Result<(), StartupError> {
        let Some(restart) = &self.relaunch else {
            return Err(StartupError {
                code: StartupErrorCode::RelaunchUnavailable,
                message: "the app has no relaunch (StartupBuilder::relaunch)".into(),
            });
        };
        if let Some(life) = &self.lifecycle {
            life.flush();
        }
        log::info!(Window = label; "Restarting the app on request");
        restart(app).map_err(|message| {
            log::error!(Window = label; "Could not restart the app: {message}");
            StartupError {
                code: StartupErrorCode::RelaunchFailed,
                message,
            }
        })
    }
}

/// A clean exit: stops the ticker, writes the last checkpoint and removes the running marker, so the next start does
/// not report a crash. The shell calls it at `RunEvent::Exit`; an app without the shell plugin does the same.
pub fn end_run<R: Runtime>(app: &AppHandle<R>) {
    if let Some(startup) = app.try_state::<Startup<R>>() {
        startup.stop();
        if let Some(life) = startup.lifecycle() {
            life.end();
        }
    }
}

/// Whether a window is visible, from what the OS said: shown and not minimised. A window the OS cannot report on counts as visible (a
/// question about a window that is perhaps hidden is better than none about a broken one).
pub fn visibility(is_visible: Option<bool>, is_minimized: Option<bool>) -> bool {
    is_visible.unwrap_or(true) && !is_minimized.unwrap_or(false)
}

/// The user's answer to the question about window `label`.
///
/// `seq` names the question the button belonged to. When that question was closed meanwhile (the page recovered, or answered
/// already) the answer is stale and does nothing: pressing Reload as the page comes back must not reload a healthy page.
fn answered<R: Runtime>(app: &AppHandle<R>, label: &str, seq: u64, choice: Choice) {
    let choice_name = format!("{choice:?}");
    let Some(startup) = app.try_state::<Startup<R>>() else {
        log::info!(Window = label, Choice = choice_name.as_str(), Seq = seq; "Not-responding question answered");
        return;
    };
    if !startup.with(|w| w.accept_answer(label, seq, choice)) {
        log::info!(Window = label, Choice = choice_name.as_str(), Seq = seq; "Answer {choice_name} to question {seq}, which was closed, is ignored");
        return;
    }
    log::info!(Window = label, Choice = choice_name.as_str(), Seq = seq; "Not-responding question answered");
    match choice {
        Choice::Reload => {
            // Waiting again first: if the reload never starts a page, the watchdog still sees it.
            startup.with(|w| w.keep_waiting(label, Instant::now()));
            reload(app, label.to_string());
        }
        Choice::SafeMode => match &startup.safe_mode {
            Some(restart) => {
                if let Err(e) = restart(app, label) {
                    log::error!(Window = label; "Could not restart in Safe Mode: {e}");
                }
            }
            None => startup.with(|w| w.keep_waiting(label, Instant::now())),
        },
        Choice::Wait => startup.with(|w| w.keep_waiting(label, Instant::now())),
        Choice::SaveReport => {
            // The question stays open (the window is not asked again) while the report is written and its save dialog is up; only
            // when the hook returns does the window go back to waiting, with the longer wait.
            match startup.report.clone() {
                Some(write) => {
                    let (app, label) = (app.clone(), label.to_string());
                    std::thread::spawn(move || {
                        // Whatever the hook does, even panic, the window goes back to waiting (drop guard).
                        let _done = ReportDone {
                            app: app.clone(),
                            label: label.clone(),
                        };
                        match write(&app, &label) {
                            Ok(()) => {
                                log::info!(Window = label.as_str(); "Problem report from the not-responding question finished")
                            }
                            Err(e) => {
                                log::error!(Window = label.as_str(); "Problem report from the not-responding question failed: {e}")
                            }
                        }
                    });
                }
                None => startup.with(|w| w.keep_waiting(label, Instant::now())),
            }
        }
    }
}

/// Ends a question whose answer was Save a problem report… when the report's thread ends in any way: the question is closed and the
/// window waits again.
struct ReportDone<R: Runtime> {
    app: AppHandle<R>,
    label: String,
}

impl<R: Runtime> Drop for ReportDone<R> {
    fn drop(&mut self) {
        if let Some(startup) = self.app.try_state::<Startup<R>>() {
            startup.with(|w| {
                w.question_answered(&self.label);
                w.keep_waiting(&self.label, Instant::now());
            });
        }
    }
}

/// `result`'s value, or `None` with the error logged for window `label` (empty: none) as `what: error`.
fn logged<T, E: std::fmt::Display>(result: Result<T, E>, label: &str, what: &str) -> Option<T> {
    match result {
        Ok(v) => Some(v),
        Err(e) => {
            log::error!(Window = label; "{what}: {e}");
            None
        }
    }
}

/// Reloads window `label`'s webview on the main thread.
fn reload<R: Runtime>(app: &AppHandle<R>, label: String) {
    let app2 = app.clone();
    let queued = app.run_on_main_thread(move || {
        if let Some(reloaded) = app2.get_webview_window(&label).map(|w| w.reload()) {
            logged(reloaded, &label, "Could not reload the window");
        }
    });
    logged(queued, "", "Could not queue a window reload");
}

/// Manages `startup` and installs the page load and (macOS, iOS) web-content-terminated handlers on `builder`. An app
/// that has its own `on_page_load` calls [`on_page_load`] from it instead.
pub fn install<R: Runtime>(
    builder: ::tauri::Builder<R>,
    startup: Startup<R>,
) -> ::tauri::Builder<R> {
    let builder = builder.manage(startup).on_page_load(on_page_load::<R>);
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let builder = builder.on_web_content_process_terminate(web_content_terminated::<R>);
    builder
}

/// `Builder::on_page_load`: arms the watchdog when a watched page starts loading (first load and every reload) and
/// starts the ticker on the first one.
pub fn on_page_load<R: Runtime>(webview: &Webview<R>, payload: &PageLoadPayload<'_>) {
    page_load(webview, payload.event());
}

/// [`on_page_load`] by event (testable: Tauri's payload cannot be built outside Tauri).
pub fn page_load<R: Runtime>(webview: &Webview<R>, event: PageLoadEvent) {
    if event != PageLoadEvent::Started {
        return;
    }
    let Some(startup) = webview.try_state::<Startup<R>>() else {
        return;
    };
    if !startup.watches(webview.label(), webview.window().label()) {
        return;
    }
    startup.with(|w| w.page_started(webview.label(), Instant::now()));
    start(webview.app_handle());
}

/// `Builder::on_web_content_process_terminate` (macOS and iOS: Tauri 2 has no such event for WebView2 or WebKitGTK):
/// logs the crash and reloads the page, at most [`Config::terminate_reloads`] times within [`Config::terminate_window`];
/// the reloaded page hears why with its `window_ready` reply.
pub fn web_content_terminated<R: Runtime>(webview: &Webview<R>) {
    let Some(startup) = webview.try_state::<Startup<R>>() else {
        return;
    };
    let label = webview.label();
    if !startup.watches(label, webview.window().label()) {
        return;
    }
    let allowed = startup.with(|w| w.terminated(label, Instant::now()));
    log::error!(Severity = "Fatal", Window = label, Reload = allowed; "The web content process of window {label} terminated");
    if let Some(reloaded) = allowed.then(|| webview.reload()) {
        logged(
            reloaded,
            label,
            "Could not reload the window after its web content process terminated",
        );
    }
}

/// Starts the ticker thread (once; background, never the main thread). Returns its handle when this call started it.
/// It ends after [`Startup::stop`] (or with the process).
pub fn start<R: Runtime>(app: &AppHandle<R>) -> Option<JoinHandle<()>> {
    let startup = app.try_state::<Startup<R>>()?;
    if startup.started.swap(true, Ordering::SeqCst) {
        return None;
    }
    let tick = startup.with(|w| w.config().tick);
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("paddock-startup-watchdog".into())
        .spawn(move || {
            loop {
                std::thread::sleep(tick);
                match app.try_state::<Startup<R>>() {
                    Some(startup) if !startup.stopped.load(Ordering::SeqCst) => {
                        startup.tick(&app, Instant::now())
                    }
                    _ => return,
                }
            }
        });
    logged(spawned, "", "The start-up watchdog could not start")
}

/// The invoke handler of [`COMMANDS`](crate::COMMANDS) (put it behind `paddock_commands::tauri::Guard`).
pub fn invoke_handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    ::tauri::generate_handler![
        window_ready,
        window_boot_error,
        ui_checkpoint,
        window_pong,
        recovery_status,
        recovery_restore,
        recovery_start_fresh,
        recovery_safe_mode,
        app_relaunch
    ]
}

fn state<R: Runtime>(webview: &Webview<R>) -> Result<::tauri::State<'_, Startup<R>>, StartupError> {
    webview
        .try_state::<Startup<R>>()
        .ok_or_else(StartupError::not_installed)
}

/// The calling page painted its frame (the start-up contract's "ready"). Tells it whether the shell reloaded it and, if
/// so, gives it its last UI checkpoint.
#[::tauri::command]
pub async fn window_ready<R: Runtime>(webview: Webview<R>) -> Result<ReadyReply, StartupError> {
    Ok(state(&webview)?.ready(&webview))
}

/// An uncaught error or unhandled rejection in the page before it was ready (`kind`: `error`, `rejection` or
/// `bootstrap`; the message is its message and first stack frame).
#[::tauri::command]
pub async fn window_boot_error<R: Runtime>(
    webview: Webview<R>,
    kind: String,
    message: String,
) -> Result<(), StartupError> {
    state(&webview)?.boot_error(webview.label(), &kind, &message);
    Ok(())
}

/// The calling window's current UI state (memory only; debounced by the UI), restored if the shell reloads the page.
#[::tauri::command]
pub async fn ui_checkpoint<R: Runtime>(
    webview: Webview<R>,
    ui: serde_json::Value,
) -> Result<(), StartupError> {
    state(&webview)?.checkpoint(webview.label(), ui)
}

/// The answer to liveness ping `seq`.
#[::tauri::command]
pub async fn window_pong<R: Runtime>(webview: Webview<R>, seq: u64) -> Result<(), StartupError> {
    state(&webview)?.pong(&webview, seq);
    Ok(())
}

/// How this run started: safe mode, and after an unclean shutdown what the notice offers until the user decides.
#[::tauri::command]
pub async fn recovery_status<R: Runtime>(webview: Webview<R>) -> Result<StartReport, StartupError> {
    Ok(state(&webview)?.recovery_status(webview.label()))
}

/// The user chose Restore: the previous session, with the calling window's own UI state.
#[::tauri::command]
pub async fn recovery_restore<R: Runtime>(
    webview: Webview<R>,
) -> Result<RestorePlan, StartupError> {
    state(&webview)?.recovery_restore(webview.label())
}

/// The user chose Start fresh: the previous session is forgotten.
#[::tauri::command]
pub async fn recovery_start_fresh<R: Runtime>(webview: Webview<R>) -> Result<(), StartupError> {
    state(&webview)?.recovery_start_fresh()
}

/// The user chose Safe mode: the app restarts in it.
#[::tauri::command]
pub async fn recovery_safe_mode<R: Runtime>(webview: Webview<R>) -> Result<(), StartupError> {
    state(&webview)?.recovery_safe_mode(webview.app_handle(), webview.label())
}

/// The calling window asks to restart the whole app (the session is saved first).
#[::tauri::command]
pub async fn app_relaunch<R: Runtime>(webview: Webview<R>) -> Result<(), StartupError> {
    state(&webview)?.app_relaunch(webview.app_handle(), webview.label())
}

#[cfg(test)]
mod tests;
