//! The macOS menu bar: the standard app, Edit and Window menus, with "Quit Pitwall" following the setting "When you
//! quit Pitwall" and "Quit Pitwall and Stop Serving" next to it (ADR 0021).

use crate::quit::{self, QuitChoice};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, SubmenuBuilder};
use tauri::{AppHandle, Runtime};

/// Menu item id of "Quit Pitwall".
pub const QUIT: &str = "app-quit";
/// Menu item id of "Quit Pitwall and Stop Serving".
pub const QUIT_AND_STOP: &str = "app-quit-and-stop";

/// Builds the menu bar.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let quit = MenuItem::with_id(app, QUIT, "Quit Pitwall", true, Some("CmdOrCtrl+Q"))?;
    let quit_stop = MenuItem::with_id(app, QUIT_AND_STOP, "Quit Pitwall and Stop Serving", true, None::<&str>)?;
    let app_menu = SubmenuBuilder::new(app, "Pitwall")
        .about(None)
        .separator()
        .item(&PredefinedMenuItem::hide(app, None)?)
        .item(&PredefinedMenuItem::hide_others(app, None)?)
        .item(&PredefinedMenuItem::show_all(app, None)?)
        .separator()
        .item(&quit)
        .item(&quit_stop)
        .build()?;
    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let window = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .separator()
        .close_window()
        .build()?;
    Menu::with_items(app, &[&app_menu, &edit, &window])
}

/// What a menu item id asks for: `None` is not ours, `Some(None)` is "Quit" (the setting decides) and
/// `Some(Some(choice))` is a quit that stops serving.
pub fn quit_request(id: &str) -> Option<Option<QuitChoice>> {
    match id {
        QUIT => Some(None),
        QUIT_AND_STOP => Some(Some(QuitChoice::StopServing)),
        _ => None,
    }
}

/// Handles the quit items of the menu bar (other ids are not ours).
pub fn on_event<R: Runtime>(app: &AppHandle<R>, event: tauri::menu::MenuEvent) {
    let Some(forced) = quit_request(event.id().as_ref()) else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move { quit::request(&app, forced).await });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_quit_items_are_ours_and_nothing_else_is() {
        assert_eq!(quit_request(QUIT), Some(None), "Quit follows the setting");
        assert_eq!(quit_request(QUIT_AND_STOP), Some(Some(QuitChoice::StopServing)));
        for other in ["quit", "toggle_serving", "open", "", "app-quit-and-stop2"] {
            assert_eq!(quit_request(other), None, "{other}");
        }
    }
}
