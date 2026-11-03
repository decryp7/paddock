//! The tray / menu-bar items (Windows notification area, macOS menu bar, Linux appindicator): one tray icon per item
//! (`tray_model` says what each draws), each with the same menu (Open, Quit) and a left click that opens the shell's
//! tray popover (`paddock_window`: the `#/tray` page, placed, fitted and closed by the shell). The shell's tray model
//! has one icon with a named picture; Dyno draws live pictures on several icons, so the icons are Dyno's and only their
//! menu and clicks go through the shell ([`DynoTray`] is the shell's backend for them).
//! Before the first update, and while the agent is away, the static template icons from `icons/tray` stand in.

use crate::logging;
use crate::tray_model::{Frame, ItemSpec};
use paddock_window::popover::Rect;
use paddock_window::tauri::{PopoverSpec, Popovers, Tray, TrayBackend};
use paddock_window::tray::{item, TrayView, QUIT_ID, SHOW_ID};
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime, Wry};

/// The tray id of an item.
pub fn tray_id(item: &str) -> String {
    format!("dyno-{item}")
}

/// The ids of the items that have a tray icon, and which of them currently show a drawn picture.
#[derive(Default)]
pub struct TrayState {
    items: Mutex<std::collections::HashSet<String>>,
    painted: Mutex<HashMap<String, bool>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The static icon: the macOS template image, or the light/dark variant for the taskbar.
pub fn static_icon_bytes(dark_taskbar: bool) -> &'static [u8] {
    if cfg!(target_os = "macos") {
        include_bytes!("../icons/tray/dyno-tray-normal-18ptTemplate@2x.png")
    } else if dark_taskbar {
        include_bytes!("../icons/tray/dyno-tray-normal-dark-32.png")
    } else {
        include_bytes!("../icons/tray/dyno-tray-normal-light-32.png")
    }
}

/// Whether the taskbar or menu bar is dark (dark when unknown).
pub fn dark_taskbar() -> bool {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        winreg::RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
            .and_then(|k| k.get_value::<u32, _>("SystemUsesLightTheme"))
            .map(|v| v == 0)
            .unwrap_or(true)
    }
    #[cfg(not(windows))]
    {
        true
    }
}

fn build_menu(app: &AppHandle<Wry>) -> tauri::Result<Menu<Wry>> {
    Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, SHOW_ID, "Open Dyno", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, QUIT_ID, "Quit Dyno", true, None::<&str>)?,
        ],
    )
}

/// Creates the tray icon of one item.
fn create_item(app: &AppHandle<Wry>, spec: &ItemSpec) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    let icon = Image::from_bytes(static_icon_bytes(dark_taskbar()))
        .inspect_err(|e| logging::info(&format!("tray icon {}: the static image could not be read: {e}", tray_id(&spec.id))))?;
    logging::info(&format!(
        "tray icon {}: static image set, {}x{} px (dark taskbar: {})",
        tray_id(&spec.id),
        icon.width(),
        icon.height(),
        dark_taskbar()
    ));
    TrayIconBuilder::with_id(tray_id(&spec.id))
        .tooltip("Dyno")
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| paddock_window::tauri::menu_chosen(app, event.id().as_ref()))
        .on_tray_icon_event(move |tray, event| {
            if let TrayIconEvent::Click { button, button_state, .. } = &event {
                logging::info(&format!("tray click: {button:?} {button_state:?}"));
            }
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                let p = rect.position.to_physical::<f64>(1.0);
                let s = rect.size.to_physical::<f64>(1.0);
                let anchor = Rect {
                    x: p.x,
                    y: p.y,
                    w: s.width,
                    h: s.height,
                };
                if let Err(e) = paddock_window::tauri::toggle_popover(tray.app_handle(), None, anchor) {
                    logging::info(&format!("could not open the popover: {}", e.message));
                }
            }
        })
        .build(app)
        .inspect_err(|e| logging::info(&format!("tray icon {}: could not be created: {e}", tray_id(&spec.id))))?;
    logging::info(&format!(
        "tray icon {} created (on Windows 11 a new icon may sit in the overflow until it is pinned in Settings > Personalization > Taskbar)",
        tray_id(&spec.id)
    ));
    lock(&app.state::<TrayState>().items).insert(spec.id.clone());
    Ok(())
}

/// Makes the tray icons match `specs`: every existing one is replaced (a click handler holds what its item opened when it
/// was made, so a changed item needs a new icon). Called only when the items changed. Returns the first error.
pub fn sync_items(app: &AppHandle<Wry>, specs: &[ItemSpec]) -> tauri::Result<()> {
    if app.try_state::<TrayState>().is_none() {
        app.manage(TrayState::default());
    }
    let state = app.state::<TrayState>();
    let old: Vec<String> = lock(&state.items).drain().collect();
    for id in old {
        let _ = app.remove_tray_by_id(&tray_id(&id));
    }
    lock(&state.painted).clear();
    for spec in specs {
        create_item(app, spec)?;
    }
    Ok(())
}

/// Draws a frame on an item: the pixels and the tooltip.
pub fn paint(app: &AppHandle<Wry>, item: &str, frame: &Frame) {
    let Some(tray) = app.tray_by_id(&tray_id(item)) else { return };
    // macOS wants a template image (black with alpha; the menu bar tints it for light, dark and inactive states);
    // Windows and Linux get Dyno's own colours.
    let template = cfg!(target_os = "macos");
    let drawn = if template { frame.icon.as_template() } else { frame.icon.clone() };
    let icon = Image::new_owned(drawn.rgba, drawn.size, drawn.size);
    let size = drawn.size;
    let set = tray.set_icon(Some(icon));
    let _ = tray.set_icon_as_template(template);
    let _ = tray.set_tooltip(Some(frame.tooltip.clone()));
    if let Some(state) = app.try_state::<TrayState>() {
        // Told once per item (a draw every second would flood the log), and every failure.
        let first = lock(&state.painted).insert(item.to_string(), true).is_none();
        if let Err(e) = &set {
            logging::info(&format!("tray icon {}: the drawn image ({size}x{size} px) was refused: {e}", tray_id(item)));
        } else if first {
            logging::info(&format!("tray icon {}: first drawn image set, {size}x{size} px", tray_id(item)));
        }
    }
}

/// While the agent is away the static icons are back (a frozen bar would look like a measurement).
pub fn show_disconnected(app: &AppHandle<Wry>) {
    let Some(state) = app.try_state::<TrayState>() else { return };
    let painted: Vec<String> = lock(&state.painted).drain().filter(|(_, p)| *p).map(|(id, _)| id).collect();
    for id in painted {
        let Some(tray) = app.tray_by_id(&tray_id(&id)) else { continue };
        if let Ok(icon) = Image::from_bytes(static_icon_bytes(dark_taskbar())) {
            let _ = tray.set_icon(Some(icon));
            let _ = tray.set_icon_as_template(cfg!(target_os = "macos"));
            let _ = tray.set_tooltip(Some("Dyno — the agent is not running"));
        }
    }
}

/// The shell's tray over Dyno's icons: the model is constant (the menu), `create` makes the icons of the items and `update` has
/// nothing to draw (the live pictures arrive through [`paint`]).
pub struct DynoTray {
    items: Vec<ItemSpec>,
}

impl DynoTray {
    /// A tray whose icons are `items`.
    pub fn new(items: Vec<ItemSpec>) -> Self {
        DynoTray { items }
    }
}

impl TrayBackend<Wry> for DynoTray {
    fn create(&self, app: &AppHandle<Wry>, _view: &TrayView) -> Result<(), String> {
        sync_items(app, &self.items).map_err(|e| e.to_string())
    }

    fn update(&self, _app: &AppHandle<Wry>, _view: &TrayView) -> Result<(), String> {
        Ok(())
    }
}

/// What the shell draws for the tray's menu: Open and Quit, both the shell's own actions.
pub fn view() -> TrayView {
    TrayView::new("Dyno", vec![item(SHOW_ID, "Open Dyno"), item(QUIT_ID, "Quit Dyno")], "static")
}

/// The route of the popup page (the UI's `#/tray`).
pub const POPOVER_ROUTE: &str = "index.html#/tray";

/// The popover a click on a tray icon opens, which the shell looks up by name: the popup page at the family's width. Without it
/// a click only logs "no tray popover is set up in this app".
pub fn popovers() -> Popovers {
    Popovers::new().popover(None, PopoverSpec::new(POPOVER_ROUTE, "Dyno"))
}

/// The shell's tray drawn by `backend` ([`DynoTray`] with the icons of the items in the app, a headless one in tests).
/// Without a tray (Linux without a status-notifier host) the window is the only way in, and closing it quits.
pub fn tray<R: Runtime>(backend: impl TrayBackend<R>) -> Tray<R> {
    Tray::new(backend, |_| view())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_popup_page_is_the_one_popover_of_the_tray_icons() {
        let pops = popovers();
        assert!(pops.has(None));
        assert!(!pops.has(Some("cpu")), "Dyno has one popup, not one per icon");
        assert_eq!(POPOVER_ROUTE, "index.html#/tray");
    }

    #[test]
    fn the_static_icons_are_png_files() {
        for dark in [false, true] {
            assert_eq!(&static_icon_bytes(dark)[1..4], b"PNG");
        }
    }

    #[test]
    fn the_menu_is_the_shells_open_and_quit() {
        let v = view();
        assert_eq!(v.menu.len(), 2);
        assert!(paddock_window::tray::validate(&v.menu).is_ok());
    }
}
