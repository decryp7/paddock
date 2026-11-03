//! The tray / menu-bar items (Windows notification area, macOS menu bar, Linux appindicator): one tray icon per item
//! (`tray_model` says what each draws), each with the same menu (Open, Quit) and a left click that opens the shell's
//! tray popover (`paddock_window`: the `#/tray` page, placed, fitted and closed by the shell). The shell's tray model
//! has one icon with a named picture; Dyno draws live pictures on several icons, so the icons are Dyno's and only their
//! menu and clicks go through the shell ([`DynoTray`] is the shell's backend for them).
//! Before the first update, and while the agent is away, the static template icons from `icons/tray` stand in.

use crate::logging;
use crate::tray_identity;
use crate::tray_look::Look;
use crate::tray_model::{plan_sync, Frame, ItemSpec};
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
    /// The spec each existing icon was made from (what [`plan_sync`] compares the wanted items with).
    items: Mutex<HashMap<String, ItemSpec>>,
    painted: Mutex<HashMap<String, bool>>,
    /// Per item: the tooltip last sent and how many pictures were drawn (the tooltip changes rarely, a picture every second).
    drawn: Mutex<HashMap<String, (String, u64)>>,
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

/// The notification-area icon size now: the taskbar's DPI on Windows (queried from the taskbar window, cheap enough for every
/// update, so a DPI change shows within a second), 22 px on Linux, 18 on macOS (unused there).
pub fn native_icon_px() -> u32 {
    #[cfg(windows)]
    {
        windows_dpi::taskbar_dpi().map_or(16, crate::tray_look::taskbar_px)
    }
    #[cfg(target_os = "macos")]
    {
        18
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        22
    }
}

#[cfg(windows)]
mod windows_dpi {
    //! `GetDpiForWindow` on the taskbar window; the calls are plain queries.
    #![allow(unsafe_code)]
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW;

    pub fn taskbar_dpi() -> Option<u32> {
        let class: Vec<u16> = "Shell_TrayWnd\0".encode_utf16().collect();
        // Safety: a null-terminated class name that outlives the call; the result is only a handle value.
        let hwnd = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
        if hwnd.is_null() {
            return None;
        }
        // Safety: `hwnd` came from FindWindowW a moment ago; 0 means it is gone.
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        (dpi != 0).then_some(dpi)
    }
}

/// Shows `look` on the icon: macOS gets both representations in one `setImage` (the patched tray-icon), the others one image.
fn set_picture(tray: &tauri::tray::TrayIcon<Wry>, look: &Look) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let reps: Vec<(Vec<u8>, u32, u32)> = look.pictures.iter().map(|p| (p.rgba.clone(), p.width, p.height)).collect();
        let size = (f64::from(look.width), f64::from(look.height));
        let template = look.template;
        tray.with_inner_tray_icon(move |t| {
            let refs: Vec<(&[u8], u32, u32)> = reps.iter().map(|(b, w, h)| (b.as_slice(), *w, *h)).collect();
            t.set_icon_reps(&refs, size, template).map_err(|e| e.to_string())
        })
        .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let p = look.pictures.first().ok_or("no picture")?;
        tray.set_icon(Some(Image::new_owned(p.rgba.clone(), p.width, p.height)))
            .map_err(|e| e.to_string())
    }
}

/// Creates the tray icon of one item.
fn create_item(app: &AppHandle<Wry>, spec: &ItemSpec, initial: Option<&Frame>) -> tauri::Result<()> {
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
    #[cfg(target_os = "macos")]
    crate::tray_position::restore(&tray_identity::autosave_name(&spec.id));
    let tray = TrayIconBuilder::with_id(tray_id(&spec.id))
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
    // Start at the item's final width, as unavailable, so nothing resizes when the first live picture arrives.
    if let Some(frame) = initial {
        if let Err(e) = set_picture(&tray, &frame.look) {
            logging::info(&format!("tray icon {}: the first picture was refused: {e}", tray_id(&spec.id)));
        }
        let _ = tray.set_tooltip(Some(frame.tooltip.clone()));
    }
    let identity = apply_identity(&tray, &spec.id);
    logging::info(&format!(
        "tray icon {} created (metric {}, {identity}; on Windows 11 a new icon may sit in the overflow until it is pinned in Settings > Personalization > Taskbar)",
        tray_id(&spec.id),
        crate::tray_model::metric_name(&spec.metric)
    ));
    lock(&app.state::<TrayState>().items).insert(spec.id.clone(), spec.clone());
    Ok(())
}

/// Gives the icon its stable OS identity (ADR 0028) and returns what the log says about it. Windows took its GUID when the icon
/// was built (the provider registered in [`sync_items`]); macOS gets its autosave name here; Linux's id is the tray id.
#[cfg(target_os = "macos")]
fn apply_identity(tray: &tauri::tray::TrayIcon<Wry>, item: &str) -> String {
    let name = tray_identity::autosave_name(item);
    let for_closure = name.clone();
    let set = tray.with_inner_tray_icon(move |t| match t.ns_status_item() {
        Some(status_item) => {
            status_item.setAutosaveName(Some(&objc2_foundation::NSString::from_str(&for_closure)));
            true
        }
        None => false,
    });
    match set {
        Ok(true) => format!("autosave name {name}"),
        Ok(false) => format!("autosave name {name} NOT set: the icon has no status item"),
        Err(e) => format!("autosave name {name} NOT set: {e}"),
    }
}

/// See the macOS version.
#[cfg(windows)]
fn apply_identity(tray: &tauri::tray::TrayIcon<Wry>, item: &str) -> String {
    let guid = tray_identity::guid_text(tray_identity::guid_for(item));
    match tray.with_inner_tray_icon(|t| t.guid_status()) {
        Ok(status) => format!("GUID {guid}: {status:?}"),
        Err(e) => format!("GUID {guid}: status unknown ({e})"),
    }
}

/// See the macOS version.
#[cfg(not(any(target_os = "macos", windows)))]
fn apply_identity(_tray: &tauri::tray::TrayIcon<Wry>, item: &str) -> String {
    format!("status notifier id {}", tray_id(item))
}

/// Makes the tray icons match `specs`: an unchanged item keeps its icon (so its place in the bar), a changed or removed one is
/// removed, a new or changed one is made ([`plan_sync`]). Called only when the items changed. Returns the first error.
pub fn sync_items(app: &AppHandle<Wry>, specs: &[ItemSpec], initial: &[(String, Frame)]) -> tauri::Result<()> {
    if app.try_state::<TrayState>().is_none() {
        app.manage(TrayState::default());
    }
    #[cfg(windows)]
    tray_icon::set_guid_provider(tray_identity::guid_for_tray_id);
    let state = app.state::<TrayState>();
    let plan = plan_sync(&lock(&state.items), specs);
    logging::info(&format!(
        "tray sync: {} item(s) wanted, removing {:?}, making {:?}",
        specs.len(),
        plan.remove,
        plan.create.iter().map(|s| s.id.as_str()).collect::<Vec<_>>()
    ));
    for id in &plan.remove {
        // AppKit may forget the position of a removed status item: keep a copy.
        #[cfg(target_os = "macos")]
        crate::tray_position::save(&tray_identity::autosave_name(id));
        let _ = app.remove_tray_by_id(&tray_id(id));
        lock(&state.items).remove(id);
        lock(&state.painted).remove(id);
        lock(&state.drawn).remove(id);
    }
    for spec in &plan.create {
        create_item(app, spec, initial.iter().find(|(id, _)| id == &spec.id).map(|(_, f)| f))?;
    }
    Ok(())
}

/// macOS: copies the position AppKit holds for every icon to Dyno's own keys (the icons are about to be removed on quit).
#[cfg(target_os = "macos")]
pub fn save_positions(app: &AppHandle<Wry>) {
    let Some(state) = app.try_state::<TrayState>() else { return };
    let ids: Vec<String> = lock(&state.items).keys().cloned().collect();
    let saved: usize = ids
        .iter()
        .map(|id| crate::tray_position::save(&tray_identity::autosave_name(id)))
        .sum();
    logging::info(&format!(
        "tray positions: {saved} key(s) of {} item(s) saved before the icons go",
        ids.len()
    ));
}

/// Draws a frame on an item: the pictures in one call, and the tooltip when it changed.
pub fn paint(app: &AppHandle<Wry>, item: &str, frame: &Frame) {
    let Some(tray) = app.tray_by_id(&tray_id(item)) else { return };
    let (w, h) = (frame.look.width, frame.look.height);
    let set = set_picture(&tray, &frame.look);
    let Some(state) = app.try_state::<TrayState>() else { return };
    let (ops, count) = {
        let mut map = lock(&state.drawn);
        let entry = map.entry(item.to_string()).or_default();
        entry.1 += 1;
        let ops = crate::tray_model::paint_ops(&entry.0, &frame.tooltip, entry.1);
        if ops.tooltip {
            entry.0 = frame.tooltip.clone();
        }
        (ops, entry.1)
    };
    if ops.tooltip {
        let _ = tray.set_tooltip(Some(frame.tooltip.clone()));
    }
    // One line per picture, at debug: the flow of image sets per tick (a flicker shows as more than one set per tick).
    logging::debug(&format!(
        "tray icon {}: picture #{count} set ({} call, {} representation(s), {w}x{h}, template {}), tooltip {}",
        tray_id(item),
        ops.image_sets,
        frame.look.pictures.len(),
        frame.look.template,
        if ops.tooltip { "sent" } else { "unchanged" }
    ));
    let first = lock(&state.painted).insert(item.to_string(), true).is_none();
    // Told once per item and on every failure (a line per second per item would flood the log).
    if let Err(e) = &set {
        logging::info(&format!("tray icon {}: the drawn image ({w}x{h}) was refused: {e}", tray_id(item)));
    } else if first {
        logging::info(&format!("tray icon {}: first drawn image set, {w}x{h}", tray_id(item)));
    }
    #[cfg(target_os = "macos")]
    if ops.save_position {
        crate::tray_position::save(&tray_identity::autosave_name(item));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = count;
}

/// While the agent is away every item shows its unavailable frame (a dimmed dash at the same width): a frozen bar would look
/// like a measurement.
pub fn show_disconnected(app: &AppHandle<Wry>, frames: &[(String, Frame)]) {
    for (id, frame) in frames {
        if let Some(state) = app.try_state::<TrayState>() {
            // The next picture's tooltip is always sent again.
            lock(&state.drawn).remove(id);
        }
        paint(app, id, frame);
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
        // The icons start as unavailable at their final widths; the first live update replaces them.
        let initial =
            crate::tray_model::TrayItems::new(self.items.clone(), crate::tray_model::Env::native()).unavailable_frames(dark_taskbar());
        sync_items(app, &self.items, &initial).map_err(|e| e.to_string())
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
