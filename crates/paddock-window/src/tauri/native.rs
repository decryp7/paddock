//! Native glue: the few lines only a real desktop runs, and that tests cannot reach by rule (rule 7: no tray icon or
//! window effects on the desktop) or because Tauri does not let them build the value (`RunEvent::ExitRequested`,
//! `RunEvent::Reopen`, `WindowEvent::CloseRequested`). Never logic: each function hands over to tested code or makes one
//! platform call. This file is left out of the coverage job (`crates/README.md`) and reviewed by hand.

use super::lifecycle::{exit, exit_requested};
use super::manager::keeps_open;
use super::popover::effects;
use crate::popover::Style;
use ::tauri::{AppHandle, RunEvent, Runtime, WebviewWindowBuilder, WindowEvent};

/// The run events of the layer (call it from `App::run`): exit requests, the exit, and on macOS files and links opened
/// with the app and the Dock icon while no window is visible.
pub fn on_run_event<R: Runtime>(app: &AppHandle<R>, event: &RunEvent) {
    match event {
        RunEvent::ExitRequested { code, api, .. } => {
            if exit_requested(app, *code) {
                api.prevent_exit();
            }
        }
        RunEvent::Exit => exit(app),
        #[cfg(target_os = "macos")]
        RunEvent::Opened { urls } => super::instance::opened(app, urls),
        #[cfg(target_os = "macos")]
        RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } => super::instance::reopen(app),
        _ => {}
    }
}

/// A close request of window `label`: prevented when the close setting keeps the window ([`keeps_open`]). Returns
/// whether `event` was a close request.
pub(crate) fn close_request<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    event: &WindowEvent,
) -> bool {
    let WindowEvent::CloseRequested { api, .. } = event else {
        return false;
    };
    if keeps_open(app, label) {
        api.prevent_close();
    }
    true
}

/// The platform's material on a popover's builder ([`effects`]; mock windows crash on window effects).
pub(crate) fn with_material<'a, R: Runtime>(
    b: WebviewWindowBuilder<'a, R, AppHandle<R>>,
    style: Style,
) -> WebviewWindowBuilder<'a, R, AppHandle<R>> {
    match effects(style) {
        Some(e) => b.effects(e),
        None => b,
    }
}

#[cfg(feature = "tray")]
mod tray_icon {
    use super::super::native_tray::{NativeTray, TRAY_ID, host_available, to_menu};
    use super::super::tray::TrayBackend;
    use crate::tray::TrayView;
    use ::tauri::{AppHandle, Runtime};

    impl<R: Runtime> TrayBackend<R> for NativeTray {
        fn create(&self, app: &AppHandle<R>, view: &TrayView) -> Result<(), String> {
            if !host_available() {
                return Err("no taskbar to hold a tray icon".into());
            }
            let builder = self.icon_builder(app, view)?;
            builder.build(app).map(|_| ()).map_err(|e| e.to_string())
        }

        fn update(&self, app: &AppHandle<R>, view: &TrayView) -> Result<(), String> {
            let tray = app.tray_by_id(TRAY_ID).ok_or("the tray icon is gone")?;
            let menu = to_menu(app, &view.menu).map_err(|e| e.to_string())?;
            tray.set_menu(Some(menu)).map_err(|e| e.to_string())?;
            tray.set_tooltip(Some(&view.tooltip))
                .map_err(|e| e.to_string())?;
            if let Some(icon) = (self.icons)(&view.icon) {
                tray.set_icon(Some(icon)).map_err(|e| e.to_string())?;
                tray.set_icon_as_template(self.template)
                    .map_err(|e| e.to_string())?;
            }
            Ok(())
        }
    }
}
