//! The tray / menu-bar items (Windows notification area, macOS menu bar, Linux appindicator): one tray icon per item
//! (`tray_model` says what each draws), each with the same menu (Open, Quit) and a left click that opens
//! a popover (`popover.rs`): the shared `#/tray` page, or the item's own `#/popover/<id>` page.
//! Before the first update, and while the agent is away, the static template icons from `icons/tray` stand in.

use crate::link::AgentLink;
use crate::logging;
use crate::popover::{self, Rect, Style};
use crate::tray_model::{Frame, ItemSpec, Opens};
use crate::window;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

/// The main popover's window label (in `capabilities/default.json`).
pub const DROPDOWN: &str = "tray";
const OPEN: &str = "open";
/// Menu item: restart the agent with administrator rights for this session.
const RUN_AS_ADMIN: &str = "run-as-admin";
/// Menu item: restart the agent with normal rights.
const NORMAL_RIGHTS: &str = "normal-rights";
const QUIT: &str = "quit";

/// The tray id of an item.
pub fn tray_id(item: &str) -> String {
    format!("dyno-{item}")
}

/// The popover window label and page route an item opens.
pub fn popover_target(spec: &ItemSpec) -> (String, String) {
    // An id that cannot be a window label opens the shared popover.
    if spec.opens == Opens::Own && popover::valid_item_id(&spec.id) {
        (format!("popover-{}", spec.id), format!("index.html#/popover/{}", spec.id))
    } else {
        (DROPDOWN.to_string(), "index.html#/tray".to_string())
    }
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

/// Switches the agent between administrator and normal rights from the tray menu. The OS shows its own prompt; the answer
/// is logged (the windows show the state from the live stream).
fn switch_rights(app: &AppHandle<Wry>, administrator: bool) {
    let app = app.clone();
    std::thread::spawn(move || {
        let (Some(ctl), Some(link)) = (
            app.try_state::<crate::elevation::ElevationCtl>(),
            app.try_state::<std::sync::Arc<crate::link::AgentLink>>(),
        ) else {
            return;
        };
        let result = if administrator {
            ctl.run_as_administrator(link.inner().as_ref())
        } else {
            ctl.run_unelevated(link.inner().as_ref())
        };
        match result {
            Ok(status) => logging::info(&format!("rights switched from the tray menu: elevated={}", status.elevated)),
            Err(message) => logging::info(&format!("rights could not be switched: {message}")),
        }
    });
}

fn build_menu(app: &AppHandle<Wry>) -> tauri::Result<Menu<Wry>> {
    Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, OPEN, "Open Dyno", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, RUN_AS_ADMIN, "Run as administrator", true, None::<&str>)?,
            &MenuItem::with_id(app, NORMAL_RIGHTS, "Go back to normal rights", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, QUIT, "Quit Dyno", true, None::<&str>)?,
        ],
    )
}

fn style() -> Style {
    Style {
        os: crate::dto::os_name(),
        vibrancy: window::vibrancy(),
    }
}

/// Creates the tray icon of one item.
fn create_item(app: &AppHandle<Wry>, spec: &ItemSpec) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    let icon = Image::from_bytes(static_icon_bytes(dark_taskbar()))?;
    let (label, route) = popover_target(spec);
    TrayIconBuilder::with_id(tray_id(&spec.id))
        .tooltip("Dyno")
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            OPEN => window::show_main(app),
            QUIT => quit(app),
            RUN_AS_ADMIN => switch_rights(app, true),
            NORMAL_RIGHTS => switch_rights(app, false),
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| {
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
                if let Err(e) = popover::toggle(tray.app_handle(), &label, &route, anchor, style()) {
                    logging::info(&format!("could not open the popover: {e}"));
                }
            }
        })
        .build(app)?;
    lock(&app.state::<TrayState>().items).insert(spec.id.clone());
    Ok(())
}

/// Makes the tray icons match `specs`: every existing one is replaced (a click handler holds what its item opened when it
/// was made, so a changed `opens` needs a new icon). Called only when the items changed. Returns the first error.
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
    let _ = tray.set_icon(Some(icon));
    let _ = tray.set_icon_as_template(template);
    let _ = tray.set_tooltip(Some(frame.tooltip.clone()));
    if let Some(state) = app.try_state::<TrayState>() {
        lock(&state.painted).insert(item.to_string(), true);
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

/// Quits for real: asks the link what to do about the agent (see `link::stop_agent_on_quit`), then exits.
pub fn quit(app: &AppHandle<Wry>) {
    let app = app.clone();
    std::thread::spawn(move || {
        app.state::<Arc<AgentLink>>().quit();
        app.exit(0);
    });
}

/// A popover lost focus: it closes (with its fade).
pub fn popover_blurred(app: &AppHandle<Wry>, label: &str) {
    popover::close(app, label);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tray_model::Glyph;
    use dyno_core::widgets::Metric;

    #[test]
    fn the_static_icons_are_png_files() {
        for dark in [false, true] {
            assert_eq!(&static_icon_bytes(dark)[1..4], b"PNG");
        }
    }

    #[test]
    fn items_open_the_shared_or_their_own_popover() {
        let mut spec = ItemSpec::new("cpu-1", Metric::CpuTotal, Glyph::Ring, Opens::Main);
        assert_eq!(popover_target(&spec), ("tray".into(), "index.html#/tray".into()));
        spec.opens = Opens::Own;
        let (label, route) = popover_target(&spec);
        assert_eq!((label.as_str(), route.as_str()), ("popover-cpu-1", "index.html#/popover/cpu-1"));
        assert!(popover::is_popover_label(&label));
        assert_eq!(tray_id("cpu-1"), "dyno-cpu-1");
        spec.id = "Bad Id".into();
        assert_eq!(popover_target(&spec).0, "tray", "an id that cannot be a window label falls back");
    }

    #[test]
    fn popover_windows_may_use_the_ipc_commands() {
        let caps: serde_json::Value = serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let windows: Vec<&str> = caps["windows"].as_array().unwrap().iter().filter_map(|w| w.as_str()).collect();
        assert!(windows.contains(&DROPDOWN) && windows.contains(&"popover-*") && windows.contains(&window::MAIN));
    }
}
