//! [`Builder`]: the workbench plugin with its parts. Every part is optional; a command of a part the app leaves out
//! answers with that crate's "not set up" error (degrade, don't fail).

use crate::tauri::Layouts;
use crate::{COMMANDS, LAYOUT, PLUGIN_NAME};
use paddock_assistant::Assistant;
use paddock_commands::tauri::{Guard, Routes};
use paddock_panels::tauri::{Panels, WindowHooks};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::plugin::TauriPlugin;
use tauri::{Manager, Runtime};

/// The workbench plugin with its parts (`paddock_workbench::Builder::new().layouts(dir).panels(p).assistant(a).build()`).
pub struct Builder<R: Runtime> {
    layouts: Option<PathBuf>,
    panels: Option<Panels>,
    hooks: Option<WindowHooks<R>>,
    assistant: Option<Arc<Assistant>>,
}

impl<R: Runtime> Default for Builder<R> {
    fn default() -> Self {
        Builder {
            layouts: None,
            panels: None,
            hooks: None,
            assistant: None,
        }
    }
}

impl<R: Runtime> Builder<R> {
    /// A plugin with no parts yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Keeps the dock's layouts in `<config_dir>/workbench-layout.json` (versioned, bounded; a damaged file starts empty
    /// and is logged once).
    pub fn layouts(mut self, config_dir: impl Into<PathBuf>) -> Self {
        self.layouts = Some(config_dir.into());
        self
    }

    /// The pop-out panel layer (`Panels::builder().route(..).bounds_file(..).build()`). The app serves the drag ghost's
    /// page (`paddock_panels::ghost::page`) and calls `Panels::set_exiting(true)` on its own quit path.
    pub fn panels(mut self, panels: Panels) -> Self {
        self.panels = Some(panels);
        self
    }

    /// The app's own steps in building a pop-out window (its webview settings).
    pub fn window_hooks(mut self, hooks: WindowHooks<R>) -> Self {
        self.hooks = Some(hooks);
        self
    }

    /// The assistant (`paddock_assistant::Assistant`); its events reach every window as `paddock://assistant`.
    pub fn assistant(mut self, assistant: Arc<Assistant>) -> Self {
        self.assistant = Some(assistant);
        self
    }

    /// The plugin. Its set-up manages the parts. The shell's plugin comes first: the dialogs' grants the guard reads are
    /// the shell's state.
    pub fn build(self) -> TauriPlugin<R> {
        let routes = Routes::new()
            .route(&LAYOUT, crate::tauri::invoke_handler())
            .route(
                &paddock_panels::COMMANDS,
                paddock_panels::tauri::invoke_handler(),
            )
            .route(
                &paddock_assistant::tauri::COMMANDS,
                paddock_assistant::tauri::invoke_handler(),
            );
        let handler = Guard::new(&COMMANDS)
            .dialog_grants(paddock_dialogs::tauri::grant_of)
            .wrap(routes.handler());
        let mut parts = Some(self);
        tauri::plugin::Builder::<R>::new(PLUGIN_NAME)
            .invoke_handler(handler)
            .setup(move |app, _api| {
                if let Some(parts) = parts.take() {
                    parts.install(app.app_handle());
                }
                Ok(())
            })
            .on_window_ready(|window| {
                let w = window.clone();
                window.on_window_event(move |event| on_window_event(&w, event));
            })
            .build()
    }

    fn install(self, app: &tauri::AppHandle<R>) {
        if let Some(dir) = self.layouts {
            app.manage(Layouts::open(&dir));
        }
        if let Some(panels) = self.panels {
            app.manage(panels);
        }
        if let Some(hooks) = self.hooks {
            app.manage(hooks);
        }
        if let Some(assistant) = self.assistant {
            paddock_assistant::tauri::manage(app, assistant);
        }
    }
}

/// Every window's events, for the panel layer (a pop-out closing docks its panel back, a host closing closes its
/// pop-outs, the end of a drag).
pub(crate) fn on_window_event<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    paddock_panels::tauri::on_window_event(window, event);
}
