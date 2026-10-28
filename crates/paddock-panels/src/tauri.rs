//! The Tauri adapter of `paddock-panels` (feature `tauri`): the managed [`Panels`] state, pop-out windows built on the
//! right display, the window events (a pop-out closing docks its panel back, a host closing closes its pop-outs), the drag
//! ghost window and the UI-state relay, and the commands of [`COMMANDS`](crate::COMMANDS).
//!
//! ```ignore
//! app.manage(paddock_panels::tauri::Panels::builder()
//!     .route("index.html#/popout")
//!     .bounds_file(data_dir.join("popout-places.json"))   // where each view's pop-out was last seen
//!     .build());
//! // ...
//! .on_window_event(|w, e| {
//!     paddock_dialogs::tauri::on_window_event(w, e);
//!     paddock_panels::tauri::on_window_event(w, e);
//! })
//! .invoke_handler(Guard::new(&paddock_panels::COMMANDS).wrap(paddock_panels::tauri::invoke_handler()))
//! ```
//!
//! Window work runs in async commands, in window events or through `run_on_main_thread`; nothing here blocks the main
//! thread on other work. The ghost's static page (`ghost::page`) is served by the app.

mod commands;
mod events;
mod ghost;
mod native;
mod popout;
mod redock;
mod relay;

pub use commands::{
    drag_ghost_end, drag_ghost_show, drag_ghost_start, invoke_handler, panel_dock_back,
    panel_focus, panel_keep_on_top, panel_pop_out, panels_list, pointer_button_down,
    ui_sync_publish,
};
pub use events::{close_all_of, on_window_event, set_visible};
pub use ghost::{end_for, positioning_supported_here};

pub use popout::{dock_back, focus, list, monitors_from, open, set_keep_on_top};
pub use relay::publish;

use crate::ghost::State;
use crate::redock::Opacity;
use crate::registry::{Memory, PopOuts};
use crate::sync::Limiter;
use ::tauri::{AppHandle, Runtime, WebviewWindow, WebviewWindowBuilder};
use paddock_commands::labels::{Label, resolve};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;

/// The log target of this crate.
const TARGET: &str = "paddock_panels";

/// The route of the app's UI that renders a pop-out (the default of [`PanelsBuilder`]).
pub const DEFAULT_ROUTE: &str = "index.html#/popout";

/// The builder of a pop-out window.
pub type Builder<'a, R> = WebviewWindowBuilder<'a, R, AppHandle<R>>;
type BuilderHook<R> = dyn for<'a> Fn(Builder<'a, R>) -> Builder<'a, R> + Send + Sync;
type BuiltHook<R> = dyn Fn(&WebviewWindow<R>) + Send + Sync;

/// The app's own steps in building a pop-out window (its webview settings: zoom keys, dev overrides). Optional managed
/// state: without it pop-outs are built as they are.
pub struct WindowHooks<R: Runtime> {
    builder: Option<Box<BuilderHook<R>>>,
    built: Option<Box<BuiltHook<R>>>,
}

impl<R: Runtime> Default for WindowHooks<R> {
    fn default() -> Self {
        WindowHooks {
            builder: None,
            built: None,
        }
    }
}

impl<R: Runtime> WindowHooks<R> {
    /// No hooks.
    pub fn new() -> Self {
        Self::default()
    }

    /// `f` configures every pop-out window's builder before it is built.
    pub fn builder(
        mut self,
        f: impl for<'a> Fn(Builder<'a, R>) -> Builder<'a, R> + Send + Sync + 'static,
    ) -> Self {
        self.builder = Some(Box::new(f));
        self
    }

    /// `f` runs on every pop-out window right after it is built.
    pub fn built(mut self, f: impl Fn(&WebviewWindow<R>) + Send + Sync + 'static) -> Self {
        self.built = Some(Box::new(f));
        self
    }
}

/// Sets the layer up.
pub struct PanelsBuilder {
    route: String,
    bounds_file: Option<PathBuf>,
    redock_opacity: Opacity,
}

impl PanelsBuilder {
    /// The route of the UI that renders a pop-out (default [`DEFAULT_ROUTE`]).
    pub fn route(mut self, route: impl Into<String>) -> Self {
        self.route = route.into();
        self
    }

    /// Keeps each view's last pop-out place in this file (a versioned JSON file; without it nothing survives a restart).
    pub fn bounds_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.bounds_file = Some(path.into());
        self
    }

    /// How opaque a pop-out dragged back over its host is (default 0.5, kept within 0.25 to 0.9). The system's "reduce
    /// transparency" setting wins: the window stays opaque and the dock guides get an outline.
    pub fn redock_opacity(mut self, opacity: f64) -> Self {
        self.redock_opacity = Opacity::new(opacity);
        self
    }

    /// The managed state: `app.manage(builder.build())`.
    pub fn build(self) -> Panels {
        Panels {
            popouts: PopOuts::default(),
            memory: self
                .bounds_file
                .map_or_else(Memory::in_memory, Memory::in_file),
            route: self.route,
            limiter: Limiter::default(),
            ghost: Mutex::new(State::default()),
            exiting: AtomicBool::new(false),
            redock: redock::State::default(),
            redock_opacity: self.redock_opacity,
            started: Instant::now(),
        }
    }
}

/// The panel layer's state: the pop-outs, the remembered places, the UI-state rate limits and the drag session.
pub struct Panels {
    pub(crate) popouts: PopOuts,
    pub(crate) memory: Memory,
    route: String,
    pub(crate) limiter: Limiter,
    ghost: Mutex<State>,
    exiting: AtomicBool,
    pub(crate) redock: redock::State,
    pub(crate) redock_opacity: Opacity,
    started: Instant,
}

impl Panels {
    /// Sets the layer up.
    pub fn builder() -> PanelsBuilder {
        PanelsBuilder {
            route: DEFAULT_ROUTE.to_string(),
            bounds_file: None,
            redock_opacity: Opacity::default(),
        }
    }

    /// The pop-outs.
    pub fn popouts(&self) -> &PopOuts {
        &self.popouts
    }

    /// The route pop-outs load.
    pub fn route(&self) -> &str {
        &self.route
    }

    /// The app is quitting: a pop-out closing then docks nothing back (the layout keeps it for next time).
    pub fn set_exiting(&self, exiting: bool) {
        self.exiting.store(exiting, Ordering::SeqCst);
    }

    pub(crate) fn exiting(&self) -> bool {
        self.exiting.load(Ordering::SeqCst)
    }

    pub(crate) fn ghost(&self) -> MutexGuard<'_, State> {
        self.ghost.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// The window `label` (a webview window's OS window).
pub(crate) fn window<R: Runtime>(app: &AppHandle<R>, label: &str) -> Option<::tauri::Window<R>> {
    ::tauri::Manager::get_webview_window(app, label).map(|w| w.as_ref().window())
}

/// The top-level window a label works for (a pop-out's or dialog's host, else the label itself).
pub(crate) fn host_of(label: &str) -> &str {
    resolve(label)
}

/// Whether `label` is a pop-out window.
pub(crate) fn is_popout(label: &str) -> bool {
    matches!(Label::parse(label), Some(Label::Popout { .. }))
}

#[cfg(test)]
mod tests;
