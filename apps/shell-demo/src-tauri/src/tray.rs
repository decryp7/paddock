//! The tray icon and its popover: a left click opens the popover (`index.html#/tray`, the UI's tray window), the menu
//! shows the window or quits. Without a tray on this desktop the app runs without one (paddock-window logs it).

use paddock_window::tauri::{NativeTray, PopoverSpec, Popovers, Tray, create_tray};
use paddock_window::tray::{TrayView, item};
use tauri::image::Image;
use tauri::{AppHandle, Manager, Runtime};

/// The popover's route in the UI (`main.ts` boots the tray window for it).
pub const POPOVER_ROUTE: &str = "index.html#/tray";

/// The tray's menu: the shell's own items (`pd.show`, `pd.quit`), drawn by paddock-window.
pub fn view() -> TrayView {
    TrayView::new(
        crate::APP_NAME,
        vec![
            item("pd.show", "Show Paddock Shell Demo"),
            item("pd.quit", "Quit Paddock Shell Demo"),
        ],
        "default",
    )
}

/// The popover spec and the tray (managed state). Call from the app's set-up, before the main window.
pub fn create<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.manage(Popovers::new().popover(None, PopoverSpec::new(POPOVER_ROUTE, crate::APP_NAME)));
    let native = NativeTray::new(|_| icon()).menu_on_left_click(false);
    create_tray(app, Tray::new(native, |_| view()).popover(true))
}

/// The menu-bar template image on macOS (tinted by the system), the white glyph for the dark taskbars elsewhere.
fn icon() -> Option<Image<'static>> {
    let bytes: &'static [u8] = if cfg!(target_os = "macos") {
        include_bytes!("../icons/tray/shell-demo-tray-18ptTemplate@2x.png")
    } else {
        include_bytes!("../icons/tray/shell-demo-tray-dark-32.png")
    };
    Image::from_bytes(bytes).ok()
}
