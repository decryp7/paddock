//! The managed [`Windows`] state and the top-level window operations: building a window (native feel, remembered
//! bounds), opening, listing, revealing and closing windows, what closing the last window does, and the window events.
//! Ported from Debrief's `window_manager.rs` and `windows.rs` without Debrief's workspaces.

use super::{popover, tray};
use crate::bounds::{self, Restored, SavedBounds};
use crate::close::{self, Background, CloseDecision, OnClose};
use crate::exit::{EXIT_STALL_LIMIT, ExitState, TEARDOWN_STEP_LIMIT};
use crate::feel::WebviewFeel;
use crate::launch::{LaunchArgs, OpenFilesIn, OpenRequest, existing};
use crate::wire::{CLOSE_REQUESTED_EVENT, WINDOWS_EVENT, WindowError, WindowErrorCode, WindowInfo};
use ::tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Runtime, State, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, Window, WindowEvent,
};
use paddock_commands::Label;
use paddock_commands::labels::{MAIN_LABEL, WINDOW_PREFIX, resolve};
use paddock_dialogs::placement::{Monitor, Rect, screen_position};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// The user's window settings, read when they apply (the app's settings store; `paddock-settings` later).
pub trait Prefs: Send + Sync + 'static {
    /// What closing the last window does.
    fn on_close(&self) -> OnClose;
    /// Where files from another app open when the window in front shows something.
    fn open_files_in(&self) -> OpenFilesIn {
        OpenFilesIn::Ask
    }
    /// The user ticked "remember" in the close question: save `setting`. An error is logged; the window closes anyway.
    fn remember_on_close(&self, setting: OnClose) -> Result<(), String>;
}

/// Settings held in memory: the app's fixed choice (a manager app keeps running in the tray), or a default.
#[derive(Debug)]
pub struct FixedPrefs {
    on_close: Mutex<OnClose>,
    open_files_in: OpenFilesIn,
}

impl FixedPrefs {
    pub fn new(on_close: OnClose, open_files_in: OpenFilesIn) -> FixedPrefs {
        FixedPrefs {
            on_close: Mutex::new(on_close),
            open_files_in,
        }
    }
}

impl Default for FixedPrefs {
    fn default() -> Self {
        FixedPrefs::new(OnClose::default(), OpenFilesIn::default())
    }
}

impl Prefs for FixedPrefs {
    fn on_close(&self) -> OnClose {
        *lock(&self.on_close)
    }

    fn open_files_in(&self) -> OpenFilesIn {
        self.open_files_in
    }

    fn remember_on_close(&self, setting: OnClose) -> Result<(), String> {
        *lock(&self.on_close) = setting;
        Ok(())
    }
}

/// What the app's windows are.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowConfig {
    /// The default title (and the title of a window whose UI set none).
    pub title: String,
    /// The route of the app's UI for windows built here (`index.html`).
    pub url: String,
    /// The inner size of a window without a remembered one, logical px.
    pub size: (f64, f64),
    pub min_size: (f64, f64),
    /// Whether the app has more than one top-level window (`w-<n>`); a manager app has one.
    pub multi_window: bool,
    /// More top-level windows than this are refused.
    pub max_windows: usize,
    /// What keeping running in the tray does to the last window.
    pub background: Background,
    /// The app's URL schemes (lowercase, without `://`): arguments and opened URLs of these are deep links.
    pub schemes: Vec<String>,
    /// How long the windows may take to close after an exit request.
    pub exit_stall_limit: Duration,
    /// How long one teardown step may hold the exit up.
    pub step_limit: Duration,
    /// The clock both limits run on (a `FakeClock` in tests).
    pub clock: ExitClock,
}

/// The clock of the exit limits in a [`WindowConfig`]; equal when it is the same clock.
#[derive(Clone)]
pub struct ExitClock(pub Arc<dyn paddock_jobs::time::Clock>);

impl Default for ExitClock {
    fn default() -> Self {
        ExitClock(Arc::new(paddock_jobs::time::SystemClock))
    }
}

impl std::fmt::Debug for ExitClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ExitClock(..)")
    }
}

impl PartialEq for ExitClock {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl WindowConfig {
    /// A single-window app called `title` with the shell's defaults.
    pub fn new(title: &str) -> WindowConfig {
        WindowConfig {
            title: title.to_string(),
            url: "index.html".into(),
            size: (1280.0, 800.0),
            min_size: (800.0, 560.0),
            multi_window: false,
            max_windows: 32,
            background: Background::default(),
            schemes: Vec::new(),
            exit_stall_limit: EXIT_STALL_LIMIT,
            step_limit: TEARDOWN_STEP_LIMIT,
            clock: ExitClock::default(),
        }
    }

    pub fn url(mut self, url: &str) -> Self {
        self.url = url.to_string();
        self
    }

    pub fn size(mut self, width: f64, height: f64) -> Self {
        self.size = (width, height);
        self
    }

    pub fn min_size(mut self, width: f64, height: f64) -> Self {
        self.min_size = (width, height);
        self
    }

    /// A multi-window app (at most `max` top-level windows).
    pub fn multi_window(mut self, max: usize) -> Self {
        self.multi_window = true;
        self.max_windows = max.max(1);
        self
    }

    pub fn background(mut self, background: Background) -> Self {
        self.background = background;
        self
    }

    pub fn schemes(mut self, schemes: &[&str]) -> Self {
        self.schemes = schemes.iter().map(|s| s.to_ascii_lowercase()).collect();
        self
    }

    pub fn exit_stall_limit(mut self, limit: Duration) -> Self {
        self.exit_stall_limit = limit;
        self
    }

    pub fn step_limit(mut self, limit: Duration) -> Self {
        self.step_limit = limit;
        self
    }

    /// The clock of the exit limits.
    pub fn clock(mut self, clock: Arc<dyn paddock_jobs::time::Clock>) -> Self {
        self.clock = ExitClock(clock);
        self
    }
}

/// The builder of a window built here.
pub type Builder<'a, R> = WebviewWindowBuilder<'a, R, AppHandle<R>>;
type BuilderHook<R> = dyn for<'a> Fn(Builder<'a, R>) -> Builder<'a, R> + Send + Sync;
type WindowHook<R> = dyn Fn(&AppHandle<R>, &str) + Send + Sync;
type PristineHook<R> = dyn Fn(&AppHandle<R>, &str) -> bool + Send + Sync;
pub(crate) type Step<R> = Arc<dyn Fn(&AppHandle<R>) + Send + Sync>;
pub(crate) type ExitHook<R> = Arc<dyn Fn(&AppHandle<R>, i32) + Send + Sync>;
pub(crate) type EndHook = Arc<dyn Fn(i32) + Send + Sync>;

/// Builds the [`Windows`] state.
pub struct WindowsBuilder<R: Runtime> {
    config: WindowConfig,
    prefs: Arc<dyn Prefs>,
    builder_hook: Option<Box<BuilderHook<R>>>,
    on_destroyed: Option<Box<WindowHook<R>>>,
    pristine: Option<Box<PristineHook<R>>>,
    steps: Vec<(String, Step<R>)>,
    bounds_file: Option<PathBuf>,
    exit_app: ExitHook<R>,
    end: EndHook,
}

impl<R: Runtime> WindowsBuilder<R> {
    /// The user's settings (default: [`FixedPrefs::default`], which asks).
    pub fn prefs(mut self, prefs: Arc<dyn Prefs>) -> Self {
        self.prefs = prefs;
        self
    }

    /// `f` configures the builder of every window built here (the app's webview settings, dev overrides); dialogs get
    /// the same through [`dialog_hooks`](Windows::dialog_hooks).
    pub fn window_builder(
        mut self,
        f: impl for<'a> Fn(Builder<'a, R>) -> Builder<'a, R> + Send + Sync + 'static,
    ) -> Self {
        self.builder_hook = Some(Box::new(f));
        self
    }

    /// `f` frees what the app keeps for a top-level window once it is destroyed (its workspace, its background work).
    pub fn on_destroyed(mut self, f: impl Fn(&AppHandle<R>, &str) + Send + Sync + 'static) -> Self {
        self.on_destroyed = Some(Box::new(f));
        self
    }

    /// Whether a window shows nothing yet (files from outside then open there without asking). Default: every window.
    pub fn pristine(
        mut self,
        f: impl Fn(&AppHandle<R>, &str) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.pristine = Some(Box::new(f));
        self
    }

    /// A step of the exit teardown (stop plugins, interrupt jobs, flush notifications), run in order, each on a thread
    /// of its own and bounded by [`WindowConfig::step_limit`].
    pub fn teardown(
        mut self,
        name: &str,
        f: impl Fn(&AppHandle<R>) + Send + Sync + 'static,
    ) -> Self {
        self.steps.push((name.to_string(), Arc::new(f)));
        self
    }

    /// Where the remembered bounds are kept (default: in memory only).
    pub fn bounds_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.bounds_file = Some(path.into());
        self
    }

    /// How Quit asks the app to exit (default: `AppHandle::exit`).
    pub fn exit_app(mut self, f: impl Fn(&AppHandle<R>, i32) + Send + Sync + 'static) -> Self {
        self.exit_app = Arc::new(f);
        self
    }

    /// How the exit watchdog ends a stalled exit after the teardown (default: `std::process::exit`).
    pub fn end_process(mut self, f: impl Fn(i32) + Send + Sync + 'static) -> Self {
        self.end = Arc::new(f);
        self
    }

    /// The state; reads the bounds file (missing, damaged or newer: no memory, logged).
    pub fn build(self) -> Windows<R> {
        let saved = self
            .bounds_file
            .as_deref()
            .map(load_bounds)
            .unwrap_or_default();
        Windows {
            config: self.config,
            prefs: self.prefs,
            builder_hook: self.builder_hook,
            on_destroyed: self.on_destroyed,
            pristine: self.pristine,
            steps: self.steps,
            exit_app: self.exit_app,
            end: self.end,
            exit: ExitState::default(),
            inner: Mutex::new(Inner {
                next: 2,
                ..Inner::default()
            }),
            bounds: Mutex::new(saved),
            bounds_file: self.bounds_file,
        }
    }
}

fn load_bounds(path: &std::path::Path) -> BTreeMap<String, SavedBounds> {
    match std::fs::read_to_string(path) {
        Ok(text) => bounds::parse(&text).unwrap_or_else(|problem| {
            log::warn!(Problem = format!("{problem:?}").as_str(); "The window bounds file is not usable; windows open at their default place");
            BTreeMap::new()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(e) => {
            log::warn!(Error = e.to_string().as_str(); "Could not read the window bounds file");
            BTreeMap::new()
        }
    }
}

#[derive(Default)]
struct Inner {
    next: u32,
    focused: Option<String>,
    titles: HashMap<String, String>,
    pending: HashMap<String, OpenRequest>,
    asks: HashMap<String, Vec<String>>,
    close_asked: HashSet<String>,
}

/// The window layer's managed state: the app's configuration and hooks, the per-window bookkeeping (titles, files
/// waiting for a window, open questions), the remembered bounds and the exit flags.
pub struct Windows<R: Runtime> {
    pub(crate) config: WindowConfig,
    pub(crate) prefs: Arc<dyn Prefs>,
    builder_hook: Option<Box<BuilderHook<R>>>,
    on_destroyed: Option<Box<WindowHook<R>>>,
    pristine: Option<Box<PristineHook<R>>>,
    pub(crate) steps: Vec<(String, Step<R>)>,
    pub(crate) exit_app: ExitHook<R>,
    pub(crate) end: EndHook,
    pub(crate) exit: ExitState,
    inner: Mutex<Inner>,
    bounds: Mutex<BTreeMap<String, SavedBounds>>,
    bounds_file: Option<PathBuf>,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl<R: Runtime> Windows<R> {
    /// A builder for an app with `config`.
    pub fn builder(config: WindowConfig) -> WindowsBuilder<R> {
        WindowsBuilder {
            config,
            prefs: Arc::new(FixedPrefs::default()),
            builder_hook: None,
            on_destroyed: None,
            pristine: None,
            steps: Vec::new(),
            bounds_file: None,
            exit_app: Arc::new(|app: &AppHandle<R>, code| app.exit(code)),
            end: Arc::new(|code| std::process::exit(code)),
        }
    }

    pub fn config(&self) -> &WindowConfig {
        &self.config
    }

    /// Whether quitting has begun.
    pub fn quitting(&self) -> bool {
        self.exit.quitting()
    }

    /// The hooks for `paddock-dialogs` (manage them), so dialog windows get the same native feel and app settings.
    pub fn dialog_hooks(app: &AppHandle<R>) -> paddock_dialogs::tauri::WindowHooks<R> {
        let app = app.clone();
        paddock_dialogs::tauri::WindowHooks::new()
            .builder(move |b| {
                let feel = WebviewFeel::current();
                let b = b
                    .zoom_hotkeys_enabled(feel.zoom_hotkeys)
                    .browser_extensions_enabled(feel.browser_extensions);
                match app.try_state::<Windows<R>>() {
                    Some(w) => w.hooked(b),
                    None => b,
                }
            })
            .built(|w| apply_feel(w, WebviewFeel::current()))
    }

    fn hooked<'a>(&self, b: Builder<'a, R>) -> Builder<'a, R> {
        match &self.builder_hook {
            Some(hook) => hook(b),
            None => b,
        }
    }

    fn next_label(&self) -> String {
        let mut inner = lock(&self.inner);
        let n = inner.next;
        inner.next += 1;
        format!("{WINDOW_PREFIX}{n}")
    }

    pub(crate) fn note_focus(&self, label: &str) {
        lock(&self.inner).focused = Some(label.to_string());
    }

    fn last_focused(&self) -> Option<String> {
        lock(&self.inner).focused.clone()
    }

    fn title_of(&self, label: &str) -> Option<String> {
        lock(&self.inner).titles.get(label).cloned()
    }

    /// Stores the title; `false` when it was that already.
    pub(crate) fn set_title(&self, label: &str, title: &str) -> bool {
        let mut inner = lock(&self.inner);
        if inner.titles.get(label).map(String::as_str) == Some(title) {
            return false;
        }
        inner.titles.insert(label.to_string(), title.to_string());
        true
    }

    pub(crate) fn queue(&self, label: &str, request: OpenRequest) {
        lock(&self.inner)
            .pending
            .entry(label.to_string())
            .or_default()
            .extend(request);
    }

    pub(crate) fn take_pending(&self, label: &str) -> OpenRequest {
        lock(&self.inner).pending.remove(label).unwrap_or_default()
    }

    pub(crate) fn set_ask(&self, label: &str, paths: Vec<String>) {
        lock(&self.inner).asks.insert(label.to_string(), paths);
    }

    pub(crate) fn take_ask(&self, label: &str) -> Vec<String> {
        lock(&self.inner).asks.remove(label).unwrap_or_default()
    }

    fn close_asked(&self, label: &str) -> bool {
        lock(&self.inner).close_asked.contains(label)
    }

    pub(crate) fn set_close_asked(&self, label: &str, asked: bool) {
        let mut inner = lock(&self.inner);
        if asked {
            inner.close_asked.insert(label.to_string());
        } else {
            inner.close_asked.remove(label);
        }
    }

    fn forget(&self, label: &str) {
        let mut inner = lock(&self.inner);
        inner.titles.remove(label);
        inner.pending.remove(label);
        inner.asks.remove(label);
        inner.close_asked.remove(label);
        if inner.focused.as_deref() == Some(label) {
            inner.focused = None;
        }
    }

    pub(crate) fn is_pristine_for(&self, app: &AppHandle<R>, label: &str) -> bool {
        self.pristine.as_ref().is_none_or(|f| f(app, label))
    }

    /// What `key` remembers.
    pub fn saved(&self, key: &str) -> Option<SavedBounds> {
        lock(&self.bounds).get(key).copied()
    }

    /// Remembers `bounds` under `key` (in memory; [`persist_bounds`](Self::persist_bounds) writes the file).
    pub fn remember(&self, key: &str, bounds: SavedBounds) {
        if bounds::valid_key(key)
            && let Some(b) = bounds.sanitized()
        {
            lock(&self.bounds).insert(key.to_string(), b);
        }
    }

    /// Writes the remembered bounds to the bounds file (atomically: a temporary file, then a rename).
    pub fn persist_bounds(&self) -> Result<(), String> {
        let Some(path) = &self.bounds_file else {
            return Ok(());
        };
        let text = bounds::to_text(&lock(&self.bounds));
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, text)
            .and_then(|()| std::fs::rename(&tmp, path))
            .map_err(|e| format!("could not save the window bounds: {e}"))
    }
}

impl From<::tauri::Error> for WindowError {
    fn from(e: ::tauri::Error) -> Self {
        WindowError::new(WindowErrorCode::Failed, e.to_string())
    }
}

pub(crate) fn state<R: Runtime>(app: &AppHandle<R>) -> Result<State<'_, Windows<R>>, WindowError> {
    app.try_state::<Windows<R>>().ok_or_else(|| {
        WindowError::new(
            WindowErrorCode::NotReady,
            "the window layer is not set up in this app (paddock_window::tauri::Windows)",
        )
    })
}

/// Whether `label` is a top-level window (`main`, `w-<n>`).
pub(crate) fn top_level(label: &str) -> bool {
    matches!(Label::parse(label), Some(Label::Main | Label::Window(_)))
}

/// Sort key: `main`, then `w-2`, `w-3`, ... numerically.
fn label_order(label: &str) -> u32 {
    match Label::parse(label) {
        Some(Label::Window(n)) => n,
        _ => 0,
    }
}

/// Applies what the builder cannot set. A failure only leaves the browser default in place, so it is logged.
pub(crate) fn apply_feel<R: Runtime>(window: &WebviewWindow<R>, feel: WebviewFeel) {
    if feel.browser_accelerator_keys {
        return;
    }
    #[cfg(all(windows, feature = "wry"))]
    {
        let label = window.label().to_string();
        let result = window.with_webview(move |webview| {
            if let Err(e) = disable_browser_accelerator_keys(&webview) {
                log::warn!(Window = label.as_str(), Error = e.to_string().as_str(); "Could not turn off the browser keys");
            }
        });
        if let Err(e) = result {
            log::warn!(Error = e.to_string().as_str(); "Could not reach the webview to turn off the browser keys");
        }
    }
    #[cfg(not(all(windows, feature = "wry")))]
    let _ = window;
}

/// Turns off WebView2's browser accelerator keys (from the next navigation commit: the first page load, as this runs
/// right after the window is built). The UI guard covers the keys it can in between.
#[cfg(all(windows, feature = "wry"))]
#[allow(unsafe_code)]
fn disable_browser_accelerator_keys(
    webview: &::tauri::webview::PlatformWebview,
) -> windows::core::Result<()> {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
    use windows::core::Interface;
    // SAFETY: COM calls on the webview's own controller, on the thread `with_webview` runs them on (the UI thread).
    unsafe {
        let settings: ICoreWebView2Settings3 =
            webview.controller().CoreWebView2()?.Settings()?.cast()?;
        settings.SetAreBrowserAcceleratorKeysEnabled(false)
    }
}

/// The monitors a window can see (work areas and scales), for the bounds.
fn monitors_of<R: Runtime>(w: &WebviewWindow<R>) -> Vec<Monitor> {
    w.available_monitors()
        .unwrap_or_default()
        .iter()
        .map(paddock_dialogs::tauri::monitor_rect)
        .collect()
}

/// A window's outer rectangle in physical px.
fn outer_rect<R: Runtime>(w: &WebviewWindow<R>) -> Option<Rect> {
    let pos = w.outer_position().ok()?;
    let size = w.outer_size().ok()?;
    Some(Rect {
        x: f64::from(pos.x),
        y: f64::from(pos.y),
        width: f64::from(size.width),
        height: f64::from(size.height),
    })
}

/// Puts a built window where `place` says (and maximizes or makes it full screen as remembered).
pub(crate) fn apply_place<R: Runtime>(w: &WebviewWindow<R>, place: &Restored) {
    let _ = w.set_size(LogicalSize::new(place.width, place.height));
    let _ = w.set_position(PhysicalPosition::new(
        place.x.round() as i32,
        place.y.round() as i32,
    ));
    if place.maximized {
        let _ = w.maximize();
    }
    if place.fullscreen {
        let _ = w.set_fullscreen(true);
    }
}

/// Where a window for `label` goes among `monitors`: a new `w-<n>` cascades from `parent` (its outer rectangle) with
/// its remembered size; `main` (and a window without a parent) opens at its remembered place. `None`: the builder's
/// default place.
pub(crate) fn place_for(
    label: &str,
    saved: Option<SavedBounds>,
    parent: Option<Rect>,
    monitors: &[Monitor],
    config: &WindowConfig,
) -> Option<Restored> {
    let size = saved.map_or(config.size, |s| (s.width, s.height));
    let cascaded = parent
        .filter(|_| label != MAIN_LABEL)
        .and_then(|p| bounds::cascade(Some(p), size, monitors));
    cascaded.or_else(|| saved.map(|s| bounds::restore(&s, monitors, config.min_size)))
}

/// Builds a window: the remembered bounds (or a cascade from the window in front), native feel, the app's builder
/// hook; after building, the place is checked against the monitors the window can see and moved inside one when it
/// is off-screen.
pub(crate) fn build<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    builder: Builder<'_, R>,
) -> Result<WebviewWindow<R>, WindowError> {
    super::plugin_hook::check_can_build(label)?;
    let windows = state(app)?;
    let feel = WebviewFeel::current();
    let config = &windows.config;
    let saved = bounds::key_of(label).and_then(|k| windows.saved(k));
    let parent = target(app).and_then(|l| app.get_webview_window(&l));
    let (parent_rect, parent_monitors) = parent
        .map(|p| (outer_rect(&p), monitors_of(&p)))
        .unwrap_or_default();
    let first = place_for(label, saved, parent_rect, &parent_monitors, config);
    let mut b = builder
        .min_inner_size(config.min_size.0, config.min_size.1)
        .zoom_hotkeys_enabled(feel.zoom_hotkeys)
        .browser_extensions_enabled(feel.browser_extensions);
    if let Some(r) = &first {
        let at = screen_position(&r.placement(), std::env::consts::OS);
        b = b
            .inner_size(r.width, r.height)
            .position(at.logical.0, at.logical.1);
    }
    let window = windows.hooked(b).build().map_err(|e| {
        WindowError::new(
            WindowErrorCode::Failed,
            format!("could not open a window: {e}"),
        )
    })?;
    apply_feel(&window, feel);
    let seen = Some(monitors_of(&window)).filter(|m| !m.is_empty());
    let monitors = seen.unwrap_or(parent_monitors);
    if let Some(place) = place_for(label, saved, parent_rect, &monitors, config).or(first) {
        apply_place(&window, &place);
    }
    Ok(window)
}

/// The main window: from the app's configuration (`tauri.conf.json`, with `"create": false` so it is built here), else
/// from [`WindowConfig`]. An existing main window is returned as it is.
pub fn create_main<R: Runtime>(app: &AppHandle<R>) -> Result<WebviewWindow<R>, WindowError> {
    if let Some(w) = app.get_webview_window(MAIN_LABEL) {
        return Ok(w);
    }
    let config = state(app)?.config.clone();
    let declared = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == MAIN_LABEL)
        .cloned();
    let builder = match declared {
        Some(c) => WebviewWindowBuilder::from_config(app, &c).map_err(WindowError::from)?,
        None => WebviewWindowBuilder::new(app, MAIN_LABEL, WebviewUrl::App(config.url.into()))
            .title(&config.title)
            .inner_size(config.size.0, config.size.1)
            .resizable(true)
            .focused(true),
    };
    let w = build(app, MAIN_LABEL, builder)?;
    state(app)?.note_focus(MAIN_LABEL);
    log::info!("Main window built");
    changed(app);
    Ok(w)
}

/// At start: the launch's files and links wait for the main window, which is built unless the app starts in the tray
/// (`--hidden`) and a tray can bring it back. Create the tray first.
pub fn start<R: Runtime>(app: &AppHandle<R>, launch: &LaunchArgs) -> Result<(), WindowError> {
    let windows = state(app)?;
    let request = OpenRequest {
        paths: existing(&launch.paths),
        links: launch.links.clone(),
    };
    if !request.is_empty() {
        windows.queue(MAIN_LABEL, request);
    }
    if close::creates_main_at_start(launch.hidden, tray::tray_available(app)) {
        create_main(app)?;
    } else {
        log::info!("Started in the tray without a window");
    }
    Ok(())
}

/// The top-level windows, `main` first.
pub fn list<R: Runtime>(app: &AppHandle<R>) -> Vec<WindowInfo> {
    let windows = app.try_state::<Windows<R>>();
    let default = windows
        .as_ref()
        .map(|w| w.config.title.clone())
        .unwrap_or_default();
    let mut out: Vec<WindowInfo> = app
        .webview_windows()
        .into_iter()
        .filter(|(label, _)| top_level(label))
        .map(|(label, w)| {
            let title = windows
                .as_ref()
                .and_then(|ws| ws.title_of(&label))
                .unwrap_or_else(|| w.title().unwrap_or_default());
            WindowInfo {
                title: if title.is_empty() {
                    default.clone()
                } else {
                    title
                },
                focused: w.is_focused().unwrap_or(false),
                visible: w.is_visible().unwrap_or(false),
                label,
            }
        })
        .collect();
    out.sort_by_key(|w| label_order(&w.label));
    out
}

/// Tells every window and the tray that the window list changed.
pub fn changed<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.emit(WINDOWS_EVENT, list(app));
    tray::refresh_tray(app);
}

/// The window files and tray actions go to: the last focused one that still exists, else `main`, else any.
pub fn target<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    let windows = app.try_state::<Windows<R>>()?;
    windows
        .last_focused()
        .filter(|l| app.get_webview_window(l).is_some())
        .or_else(|| {
            app.get_webview_window(MAIN_LABEL)
                .map(|_| MAIN_LABEL.to_string())
        })
        .or_else(|| list(app).into_iter().next().map(|w| w.label))
}

/// Opens a new, empty top-level window (multi-window apps) with `request` waiting for it; returns its label. Call it
/// from async commands, events or the main thread (a window built from a synchronous command never finishes loading
/// on Windows).
pub fn open_new<R: Runtime>(
    app: &AppHandle<R>,
    request: OpenRequest,
) -> Result<String, WindowError> {
    let windows = state(app)?;
    if !windows.config.multi_window {
        return Err(WindowError::new(
            WindowErrorCode::NotAllowed,
            "this app has one window",
        ));
    }
    let open = app
        .webview_windows()
        .keys()
        .filter(|l| top_level(l))
        .count();
    if open >= windows.config.max_windows {
        return Err(WindowError::new(
            WindowErrorCode::TooMany,
            format!(
                "{} windows are open already; close one first",
                windows.config.max_windows
            ),
        ));
    }
    let label = windows.next_label();
    windows.queue(&label, request);
    let builder = WebviewWindowBuilder::new(
        app,
        &label,
        WebviewUrl::App(windows.config.url.clone().into()),
    )
    .title(&windows.config.title)
    .inner_size(windows.config.size.0, windows.config.size.1)
    .resizable(true)
    .focused(true);
    if let Err(e) = build(app, &label, builder) {
        windows.forget(&label);
        return Err(e);
    }
    windows.note_focus(&label);
    log::info!(Windows = list(app).len() as u64; "Window opened");
    changed(app);
    Ok(label)
}

/// Shows or hides a window's dialogs and pop-outs with it.
fn set_family_visible<R: Runtime>(app: &AppHandle<R>, host: &str, visible: bool) {
    paddock_dialogs::tauri::set_visible(app, host, visible);
    for (label, w) in app.webview_windows() {
        if matches!(Label::parse(&label), Some(Label::Popout { host: h, .. }) if h == host) {
            let _ = if visible { w.show() } else { w.hide() };
        }
    }
}

/// Shows, restores and focuses a top-level window with its dialogs and pop-outs. `main` is rebuilt when a tray app
/// destroyed it (tray "Open", the Dock icon).
pub fn reveal<R: Runtime>(app: &AppHandle<R>, label: &str) -> Result<(), WindowError> {
    if !top_level(label) {
        return Err(WindowError::new(
            WindowErrorCode::NotAllowed,
            format!("{label} is not a top-level window"),
        ));
    }
    let w = match app.get_webview_window(label) {
        Some(w) => w,
        None if label == MAIN_LABEL => create_main(app)?,
        None => {
            return Err(WindowError::new(
                WindowErrorCode::Closed,
                "this window was closed",
            ));
        }
    };
    let _ = w.show();
    let _ = w.unminimize();
    let _ = w.set_focus();
    set_family_visible(app, label, true);
    state(app)?.note_focus(label);
    changed(app);
    Ok(())
}

/// Quits the app: every window closes at once, and windows destroyed from now on are not "closed by the user".
pub fn quit<R: Runtime>(app: &AppHandle<R>) {
    let Ok(windows) = state(app) else { return };
    windows.exit.begin_quit();
    log::info!(Windows = app.webview_windows().len() as u64; "Quit requested");
    let exit = windows.exit_app.clone();
    exit(app, 0);
}

/// The app keeps running in the tray after the last window: hidden with its family, or (the default) let it close.
/// Returns whether the close is prevented.
pub(crate) fn to_background<R: Runtime>(app: &AppHandle<R>, window: &WebviewWindow<R>) -> bool {
    let background = state(app).map(|w| w.config.background).unwrap_or_default();
    match background {
        Background::Hide => {
            let _ = window.hide();
            set_family_visible(app, window.label(), false);
            log::info!("Last window hidden; the app keeps running in the tray");
            changed(app);
            true
        }
        Background::Destroy => {
            log::info!("Last window closed; the app keeps running in the tray");
            false
        }
    }
}

/// Decides and applies what a close request of a top-level window does; returns whether the close is prevented.
pub fn close_requested<R: Runtime>(app: &AppHandle<R>, window: &WebviewWindow<R>) -> bool {
    let label = window.label();
    let Ok(windows) = state(app) else {
        return false;
    };
    if windows.exit.quitting() || !top_level(label) {
        return false;
    }
    record_bounds(&windows, window);
    let other_visible = app
        .webview_windows()
        .iter()
        .filter(|(l, w)| l.as_str() != label && top_level(l) && w.is_visible().unwrap_or(false))
        .count();
    let decision = close::decide_close(
        windows.prefs.on_close(),
        other_visible,
        tray::tray_available(app),
        windows.close_asked(label),
    );
    match decision {
        CloseDecision::Close => false,
        CloseDecision::Background => to_background(app, window),
        CloseDecision::Quit => {
            quit(app);
            true
        }
        CloseDecision::Ask => {
            windows.set_close_asked(label, true);
            let _ = app.emit_to(label, CLOSE_REQUESTED_EVENT, ());
            true
        }
    }
}

/// What a window's place is now, as memory: its current place, or (maximized or full screen) the last normal place
/// with the flag. [`Windows::remember`] drops nonsense (a zero size).
pub(crate) fn bounds_now(
    position: (f64, f64),
    inner_logical: (f64, f64),
    scale: f64,
    maximized: bool,
    fullscreen: bool,
    previous: Option<SavedBounds>,
) -> SavedBounds {
    let current = SavedBounds {
        x: position.0,
        y: position.1,
        width: inner_logical.0,
        height: inner_logical.1,
        scale,
        maximized: false,
        fullscreen: false,
    };
    match previous {
        Some(p) if maximized || fullscreen => SavedBounds {
            maximized,
            fullscreen,
            ..p
        },
        _ => SavedBounds {
            maximized,
            fullscreen,
            ..current
        },
    }
}

/// Remembers where a top-level window is (on move, resize and close).
fn record_bounds<R: Runtime>(windows: &Windows<R>, w: &WebviewWindow<R>) {
    let scale = w.scale_factor().unwrap_or(1.0);
    if let Some(key) = bounds::key_of(w.label())
        && let (Ok(pos), Ok(size)) = (w.outer_position(), w.inner_size())
    {
        let now = bounds_now(
            (f64::from(pos.x), f64::from(pos.y)),
            size.to_logical::<f64>(scale).into(),
            scale,
            w.is_maximized().unwrap_or(false),
            w.is_fullscreen().unwrap_or(false),
            windows.saved(key),
        );
        windows.remember(key, now);
    }
}

/// Runs `f` on a named thread of its own; a thread that cannot start is logged.
pub(crate) fn spawn(name: &str, f: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name(name.into()).spawn(f) {
        log::warn!(Thread = name, Error = e.to_string().as_str(); "Could not start a thread");
    }
}

/// Saves the remembered bounds off the calling thread (a window event runs on the main thread).
fn persist_soon<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    spawn("paddock-window-bounds", move || persist_now(&app));
}

/// A top-level window is gone: forget its bookkeeping, let the app free its state, save the bounds, and (unless
/// quitting) tell the other windows and the tray.
pub fn destroyed<R: Runtime>(app: &AppHandle<R>, label: &str) {
    if !top_level(label) {
        return;
    }
    let Ok(windows) = state(app) else { return };
    log::info!(Window = label; "Window destroyed");
    windows.forget(label);
    if let Some(f) = &windows.on_destroyed {
        f(app, label);
    }
    persist_soon(app);
    if !windows.exit.quitting() {
        changed(app);
    }
}

/// Whether the close request of window `label` is prevented (see [`close_requested`]).
pub(crate) fn keeps_open<R: Runtime>(app: &AppHandle<R>, label: &str) -> bool {
    top_level(label)
        && app
            .get_webview_window(label)
            .is_some_and(|w| close_requested(app, &w))
}

/// The window events of the layer (call it from `Builder::on_window_event`, next to `paddock_dialogs`'s): close requests
/// and the close setting, focus (pop-outs and dialogs stand for their host), bounds, destroyed windows, and the tray
/// popover's blur.
pub fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    let app = window.app_handle();
    let label = window.label();
    if popover::on_window_event(app, label, event)
        || super::native::close_request(app, label, event)
    {
        return;
    }
    match event {
        WindowEvent::Focused(true) => {
            let host = resolve(label);
            if top_level(host)
                && let Ok(ws) = state(app)
            {
                ws.note_focus(host);
            }
        }
        WindowEvent::Moved(_) | WindowEvent::Resized(_) if top_level(label) => {
            if let (Ok(ws), Some(w)) = (state(app), app.get_webview_window(label)) {
                record_bounds(&ws, &w);
            }
        }
        WindowEvent::Destroyed => destroyed(app, label),
        _ => {}
    }
}

/// The calling window's answer to the close question; with `remember` it becomes the setting.
pub(crate) fn resolve_close<R: Runtime>(
    app: &AppHandle<R>,
    window: &WebviewWindow<R>,
    choice: crate::CloseChoice,
    remember: bool,
) -> Result<(), WindowError> {
    let windows = state(app)?;
    windows.set_close_asked(window.label(), false);
    if remember && let Err(e) = windows.prefs.remember_on_close(choice.setting()) {
        log::warn!(Error = e.as_str(); "The close choice could not be remembered");
    }
    match choice {
        crate::CloseChoice::Tray if tray::tray_available(app) => {
            if !to_background(app, window) {
                window.destroy().map_err(WindowError::from)?;
            }
        }
        // Quit, or nothing to keep running in.
        _ => quit(app),
    }
    Ok(())
}

/// The bounds are saved as a teardown step (`lifecycle::exit`).
pub(crate) fn persist_now<R: Runtime>(app: &AppHandle<R>) {
    if let Ok(w) = state(app)
        && let Err(e) = w.persist_bounds()
    {
        log::warn!("{e}");
    }
}
