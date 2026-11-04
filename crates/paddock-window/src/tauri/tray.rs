//! The tray icon: the app's [`TrayView`] model drawn by a [`TrayBackend`] (the native one with the `tray` feature; a
//! fake in tests, so no icon ever reaches the desktop), redrawn only when it changed, and the shell's menu items and
//! clicks applied here.

use super::manager::{self, open_new, quit, reveal};
use super::popover::toggle_popover;
use crate::launch::OpenRequest;
use crate::popover::Rect;
use crate::tray::{Click, TrayAction, TrayView, action_of, left_click, validate};
use ::tauri::{AppHandle, Manager, Runtime};
use paddock_commands::labels::MAIN_LABEL;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Draws the tray icon.
pub trait TrayBackend<R: Runtime>: Send + Sync + 'static {
    /// Creates the icon showing `view`; an error means this desktop has no tray (the app runs without one).
    fn create(&self, app: &AppHandle<R>, view: &TrayView) -> Result<(), String>;
    /// Draws `view` (called only when it changed).
    fn update(&self, app: &AppHandle<R>, view: &TrayView) -> Result<(), String>;
}

/// A tray that draws nothing and always exists: for tests of an app's start (no icon reaches the desktop) and for
/// platforms where the app keeps its tray state without an icon.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeadlessTray;

impl<R: Runtime> TrayBackend<R> for HeadlessTray {
    fn create(&self, _: &AppHandle<R>, _: &TrayView) -> Result<(), String> {
        Ok(())
    }

    fn update(&self, _: &AppHandle<R>, _: &TrayView) -> Result<(), String> {
        Ok(())
    }
}

type Model<R> = dyn Fn(&AppHandle<R>) -> TrayView + Send + Sync;
type Action<R> = dyn Fn(&AppHandle<R>, &str) + Send + Sync;

/// The tray (managed state): its backend, the app's model and menu handler, whether a left click toggles the `tray`
/// popover, whether the icon exists, and what is on screen.
pub struct Tray<R: Runtime> {
    backend: Box<dyn TrayBackend<R>>,
    model: Box<Model<R>>,
    on_action: Option<Box<Action<R>>>,
    popover: bool,
    available: AtomicBool,
    drawn: Mutex<Option<TrayView>>,
}

impl<R: Runtime> Tray<R> {
    /// A tray drawn by `backend` showing `model(app)` (called on every refresh; it reads the app's state, and the window
    /// list through [`list`](super::list)).
    pub fn new(
        backend: impl TrayBackend<R>,
        model: impl Fn(&AppHandle<R>) -> TrayView + Send + Sync + 'static,
    ) -> Tray<R> {
        Tray {
            backend: Box::new(backend),
            model: Box::new(model),
            on_action: None,
            popover: false,
            available: AtomicBool::new(false),
            drawn: Mutex::new(None),
        }
    }

    /// `f` runs the app's own menu items (every id that is not a shell one: `pd.*`).
    pub fn on_action(mut self, f: impl Fn(&AppHandle<R>, &str) + Send + Sync + 'static) -> Self {
        self.on_action = Some(Box::new(f));
        self
    }

    /// A left click toggles the `tray` popover (register its spec in [`Popovers`](super::Popovers)).
    pub fn popover(mut self, on: bool) -> Self {
        self.popover = on;
        self
    }

    pub fn available(&self) -> bool {
        self.available.load(Ordering::SeqCst)
    }

    /// Draws the model now unless it is on screen already or invalid (logged).
    fn draw(&self, app: &AppHandle<R>) {
        if !self.available() {
            return;
        }
        let view = (self.model)(app);
        if let Err(problem) = validate(&view.menu) {
            log::error!(Problem = problem.to_string().as_str(); "The tray menu was not drawn");
            return;
        }
        {
            let mut drawn = manager::lock(&self.drawn);
            if drawn.as_ref() == Some(&view) {
                return;
            }
            *drawn = Some(view.clone());
        }
        if let Err(e) = self.backend.update(app, &view) {
            log::warn!(Error = e.as_str(); "Could not update the tray");
        }
    }
}

/// Creates the tray icon and manages `tray`. Without a tray on this desktop the app runs without one (logged), and
/// closing the last window quits. Returns whether the icon exists.
pub fn create_tray<R: Runtime>(app: &AppHandle<R>, tray: Tray<R>) -> bool {
    let view = (tray.model)(app);
    let created = match validate(&view.menu) {
        Err(problem) => Err(problem.to_string()),
        Ok(()) => tray.backend.create(app, &view),
    };
    match created {
        Ok(()) => {
            tray.available.store(true, Ordering::SeqCst);
            *manager::lock(&tray.drawn) = Some(view);
        }
        Err(e) => log::info!(Reason = e.as_str(); "No tray icon; the app runs without one"),
    }
    let available = tray.available();
    app.manage(tray);
    available
}

/// Whether a tray icon exists (closing the last window may keep the app running in it).
pub fn tray_available<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<Tray<R>>().is_some_and(|t| t.available())
}

/// Tells the shell whether the desktop shows the tray, for an app whose icon can exist without being shown (Linux without a
/// StatusNotifier host, until one appears). With no tray a hidden start opens the window and closing the last window quits
/// ([`creates_main_at_start`](crate::close::creates_main_at_start), `decide_close`). Does nothing when the app has no tray at all.
pub fn set_tray_available<R: Runtime>(app: &AppHandle<R>, available: bool) {
    let Some(tray) = app.try_state::<Tray<R>>() else {
        return;
    };
    if tray.available.swap(available, Ordering::SeqCst) != available {
        log::info!(Available = available; "The desktop now {} the tray", if available { "shows" } else { "does not show" });
    }
}

/// Redraws the tray when its model changed, on the main thread (tray calls wait for it). Never fails; callable from
/// any thread.
pub fn refresh_tray<R: Runtime>(app: &AppHandle<R>) {
    if app.try_state::<Tray<R>>().is_none() {
        return;
    }
    let a = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(t) = a.try_state::<Tray<R>>() {
            t.draw(&a);
        }
    });
}

/// A menu item was chosen: the shell's items here, the app's through its handler.
pub fn menu_chosen<R: Runtime>(app: &AppHandle<R>, id: &str) {
    let result = match action_of(id) {
        TrayAction::Show => reveal(app, MAIN_LABEL),
        TrayAction::NewWindow => open_new(app, OpenRequest::default()).map(|_| ()),
        TrayAction::ShowWindow(label) => reveal(app, &label),
        TrayAction::Quit => {
            quit(app);
            Ok(())
        }
        TrayAction::App(id) => {
            if let Some(f) = app
                .try_state::<Tray<R>>()
                .as_ref()
                .and_then(|t| t.on_action.as_ref())
            {
                f(app, &id);
            }
            Ok(())
        }
    };
    if let Err(e) = result {
        log::warn!(Error = e.message.as_str(); "Tray action failed");
    }
}

/// A left click on the tray icon at `icon` (physical px, when the platform says): the popover, the window, or nothing
/// (the platform opens the menu).
pub fn icon_clicked<R: Runtime>(app: &AppHandle<R>, icon: Option<Rect>) {
    let has_popover = app.try_state::<Tray<R>>().is_some_and(|t| t.popover);
    let result = match (left_click(has_popover, std::env::consts::OS), icon) {
        (Click::Popover, Some(rect)) => toggle_popover(app, None, rect).map(|_| ()),
        (Click::Menu, _) => Ok(()),
        // A popover without a place to anchor it, or no popover: the window.
        _ => reveal(app, MAIN_LABEL),
    };
    if let Err(e) = result {
        log::warn!(Error = e.message.as_str(); "Tray click failed");
    }
}
