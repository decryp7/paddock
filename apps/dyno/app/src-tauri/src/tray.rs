//! The tray / menu-bar items (Windows notification area, macOS menu bar, Linux appindicator): one tray icon per item
//! (`tray_model` says what each draws), each with the same menu (Open, Pause rules, Quit) and a left click that opens
//! a popover (`popover.rs`): the shared `#/tray` page, or the item's own `#/popover/<id>` page.
//! Before the first update, and while the agent is away, the static template icons from `icons/tray` stand in.

use crate::link::AgentLink;
use crate::logging;
use crate::popover::{self, Rect, Style};
use crate::tray_model::{Frame, ItemSpec, Opens};
use crate::window;
use dyno_core::protocol::Request;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

/// The main popover's window label (in `capabilities/default.json`).
pub const DROPDOWN: &str = "tray";
const OPEN: &str = "open";
const PAUSE: &str = "pause";
const QUIT: &str = "quit";

/// The tray id of an item.
pub fn tray_id(item: &str) -> String {
    format!("dyno-{item}")
}

/// The popover window label and page route an item opens.
pub fn popover_target(spec: &ItemSpec) -> (String, String) {
    match spec.opens {
        Opens::Main => (DROPDOWN.to_string(), "index.html#/tray".to_string()),
        Opens::Own => (format!("popover-{}", spec.id), format!("index.html#/popover/{}", spec.id)),
    }
}

/// Menu items kept to change their text, and which items currently show a drawn picture.
#[derive(Default)]
pub struct TrayState {
    pause: Mutex<HashMap<String, MenuItem<Wry>>>,
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

fn pause_text(paused: bool) -> &'static str {
    if paused {
        "Resume rules"
    } else {
        "Pause rules"
    }
}

fn build_menu(app: &AppHandle<Wry>) -> tauri::Result<(Menu<Wry>, MenuItem<Wry>)> {
    let pause = MenuItem::with_id(app, PAUSE, pause_text(false), true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, OPEN, "Open Dyno", true, None::<&str>)?,
            &pause,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, QUIT, "Quit Dyno", true, None::<&str>)?,
        ],
    )?;
    Ok((menu, pause))
}

fn style() -> Style {
    Style {
        os: crate::dto::os_name(),
        vibrancy: window::vibrancy(),
    }
}

/// Creates the tray icon of one item.
fn create_item(app: &AppHandle<Wry>, spec: &ItemSpec) -> tauri::Result<()> {
    let (menu, pause) = build_menu(app)?;
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
            PAUSE => toggle_pause(app),
            QUIT => quit(app),
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
    lock(&app.state::<TrayState>().pause).insert(spec.id.clone(), pause);
    Ok(())
}

/// Makes the set of tray icons match `specs`: creates the missing, removes the stale. Returns the first error.
pub fn sync_items(app: &AppHandle<Wry>, specs: &[ItemSpec]) -> tauri::Result<()> {
    if app.try_state::<TrayState>().is_none() {
        app.manage(TrayState::default());
    }
    let state = app.state::<TrayState>();
    let wanted: Vec<String> = specs.iter().map(|s| s.id.clone()).collect();
    let stale: Vec<String> = lock(&state.pause).keys().filter(|id| !wanted.contains(id)).cloned().collect();
    for id in stale {
        let _ = app.remove_tray_by_id(&tray_id(&id));
        lock(&state.pause).remove(&id);
        lock(&state.painted).remove(&id);
    }
    for spec in specs {
        if app.tray_by_id(&tray_id(&spec.id)).is_none() {
            create_item(app, spec)?;
        }
    }
    Ok(())
}

/// Draws a frame on an item: the pixels, the tooltip, and the Pause item's text.
pub fn paint(app: &AppHandle<Wry>, item: &str, frame: &Frame, paused: bool) {
    let Some(tray) = app.tray_by_id(&tray_id(item)) else { return };
    let icon = Image::new_owned(frame.icon.rgba.clone(), frame.icon.size, frame.icon.size);
    // The drawn icon has its own colours: never a template image.
    let _ = tray.set_icon(Some(icon));
    let _ = tray.set_icon_as_template(false);
    let _ = tray.set_tooltip(Some(frame.tooltip.clone()));
    if let Some(state) = app.try_state::<TrayState>() {
        if let Some(p) = lock(&state.pause).get(item) {
            let _ = p.set_text(pause_text(paused));
        }
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

fn toggle_pause(app: &AppHandle<Wry>) {
    let link = app.state::<Arc<AgentLink>>().inner().clone();
    let shell = app.state::<Arc<crate::shell::Shell>>().inner().clone();
    std::thread::spawn(move || {
        if let Err(e) = link.call(Request::PauseRules {
            paused: !shell.rules_paused(),
        }) {
            logging::info(&format!("could not pause the rules: {}", e.message));
        }
    });
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
    use dyno_core::settings::TrayMetric;

    #[test]
    fn the_static_icons_are_png_files() {
        for dark in [false, true] {
            assert_eq!(&static_icon_bytes(dark)[1..4], b"PNG");
        }
    }

    #[test]
    fn the_pause_item_says_what_a_click_does() {
        assert_eq!(pause_text(false), "Pause rules");
        assert_eq!(pause_text(true), "Resume rules");
    }

    #[test]
    fn items_open_the_shared_or_their_own_popover() {
        let mut spec = ItemSpec {
            id: "cpu-1".into(),
            metrics: vec![TrayMetric::Cpu],
            glyph: Glyph::Ring,
            opens: Opens::Main,
        };
        assert_eq!(popover_target(&spec), ("tray".into(), "index.html#/tray".into()));
        spec.opens = Opens::Own;
        let (label, route) = popover_target(&spec);
        assert_eq!((label.as_str(), route.as_str()), ("popover-cpu-1", "index.html#/popover/cpu-1"));
        assert!(popover::is_popover_label(&label));
        assert_eq!(tray_id("cpu-1"), "dyno-cpu-1");
    }

    #[test]
    fn popover_windows_may_use_the_ipc_commands() {
        let caps: serde_json::Value = serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let windows: Vec<&str> = caps["windows"].as_array().unwrap().iter().filter_map(|w| w.as_str()).collect();
        assert!(windows.contains(&DROPDOWN) && windows.contains(&"popover-*") && windows.contains(&window::MAIN));
    }
}
