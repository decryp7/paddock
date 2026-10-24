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
//!     .invoke_handler(paddock_commands::tauri::Guard::new(&paddock_startup::COMMANDS).wrap(paddock_startup::tauri::handler()));
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
    Action, BootErrorKind, Checkpoints, Choice, Config, ConfigError, PING_EVENT, Pong, Prompt,
    Question, ReadyReply, Recovery, Watchdog, clean_error, ready_reply,
};
use ::tauri::ipc::Invoke;
use ::tauri::webview::{PageLoadEvent, PageLoadPayload};
use ::tauri::{AppHandle, Emitter, Manager, Runtime, Webview};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Instant;

/// The commands every dialog may call whatever its contract (the dialog layer adds them to each dialog's base set).
pub const DIALOG_BASE: &[&str] = &["window_ready", "window_boot_error", "window_pong"];

/// Called with `true` when the watchdog gives up on a window's page (start-up question, or unresponsive after ready) and
/// with `false` when the page answers again: `paddock_dialogs::tauri::page_unresponsive` lets a dialog be closed by the
/// OS close meanwhile (rule 19).
pub type UnresponsiveHook<R> = dyn Fn(&AppHandle<R>, &str, bool) + Send + Sync;
/// Restarts the app in Safe Mode for window `label` (the app's relaunch); an error is logged.
pub type SafeModeHook<R> = dyn Fn(&AppHandle<R>, &str) -> Result<(), String> + Send + Sync;
/// Called after a window reported ready (with why the shell reloaded it): e.g. start the work the first paint did not need.
pub type ReadyHook<R> = dyn Fn(&Webview<R>, Option<Recovery>) + Send + Sync;
/// Whether a webview (label, its window's label) is one of the app's own pages the watchdog watches.
pub type WatchFilter = dyn Fn(&str, &str) -> bool + Send + Sync;

/// Shows the native question for window `label`, owned by that window, and calls `answer` once with the user's choice.
pub trait Asker<R: Runtime>: Send + Sync + 'static {
    fn ask(
        &self,
        app: &AppHandle<R>,
        label: &str,
        prompt: Prompt,
        answer: Box<dyn FnOnce(Choice) + Send>,
    );
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
/// label is in the shell's scheme (`main`, `w-<n>`, `d-<host>-<n>`, `p-<host>-<n>`).
pub fn watched_by_default(webview: &str, window: &str) -> bool {
    webview == window && paddock_commands::class_of(webview).is_some()
}

/// Builds a [`Startup`]: the timings and the app's hooks.
pub struct StartupBuilder<R: Runtime> {
    config: Config,
    watch: Box<WatchFilter>,
    safe_mode: Option<Box<SafeModeHook<R>>>,
    unresponsive: Option<Box<UnresponsiveHook<R>>>,
    on_ready: Option<Box<ReadyHook<R>>>,
    asker: Option<Arc<dyn Asker<R>>>,
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
        self.asker(NativeAsker)
    }

    /// The state to install, or why the timings are inconsistent.
    pub fn build(self) -> Result<Startup<R>, ConfigError> {
        Ok(Startup {
            dog: Mutex::new(Watchdog::new(self.config.checked()?)),
            checkpoints: Mutex::new(Checkpoints::default()),
            watch: self.watch,
            safe_mode: self.safe_mode,
            unresponsive: self.unresponsive,
            on_ready: self.on_ready,
            asker: self.asker,
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
    unresponsive: Option<Box<UnresponsiveHook<R>>>,
    on_ready: Option<Box<ReadyHook<R>>>,
    asker: Option<Arc<dyn Asker<R>>>,
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
            unresponsive: None,
            on_ready: None,
            asker: None,
        }
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
                    let visible = window.is_visible().unwrap_or(true)
                        && !window.is_minimized().unwrap_or(false);
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
            Action::Unresponsive(label) => {
                let (n, every) = self.with(|w| {
                    (
                        w.config().missed_pings,
                        w.config().ping_interval.as_millis() as u64,
                    )
                });
                log::error!(Window = label.as_str(), MissedPings = n, PingMs = every; "Window {label} stopped responding; asking the user");
                self.hook(app, &label, true);
                self.ask(app, label, Question::Unresponsive);
            }
        }
    }

    fn ask(&self, app: &AppHandle<R>, label: String, question: Question) {
        let Some(asker) = self.asker.clone() else {
            log::warn!(Window = label.as_str(); "No asker is installed; window {label} keeps waiting");
            return;
        };
        let prompt = Prompt::new(question, &app.package_info().name);
        let answer_app = app.clone();
        let answer_label = label.clone();
        let answer: Box<dyn FnOnce(Choice) + Send> =
            Box::new(move |choice| answered(&answer_app, &answer_label, choice));
        let app2 = app.clone();
        let shown = app.run_on_main_thread(move || asker.ask(&app2, &label, prompt, answer));
        logged(shown, "", "Could not show the not-responding question");
    }

    /// The page in `webview` reported ready: the reply, the hooks, the log line.
    pub fn ready(&self, webview: &Webview<R>) -> ReadyReply {
        let label = webview.label().to_string();
        let reply = {
            let checkpoints = lock(&self.checkpoints);
            self.with(|w| ready_reply(w, &checkpoints, &label, Instant::now()))
        };
        self.hook(webview.app_handle(), &label, false);
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
        lock(&self.checkpoints).put(label, ui);
        Ok(())
    }
}

/// The user's answer to the question about window `label`.
fn answered<R: Runtime>(app: &AppHandle<R>, label: &str, choice: Choice) {
    log::info!(Window = label, Choice = format!("{choice:?}").as_str(); "Not-responding question answered");
    let Some(startup) = app.try_state::<Startup<R>>() else {
        return;
    };
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
pub fn handler<R: Runtime>() -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    ::tauri::generate_handler![window_ready, window_boot_error, ui_checkpoint, window_pong]
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

#[cfg(test)]
mod tests;
