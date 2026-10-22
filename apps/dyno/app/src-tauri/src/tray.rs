//! The tray icon (Windows notification area, macOS menu bar, Linux appindicator) and its dropdown. The icon is drawn
//! from the live numbers (`tray_model`), the tooltip carries them in words, a left click opens a small frameless
//! window with the `#/tray` route (it hides when it loses focus), and the menu has Open, Pause rules and Quit.
//! Before the first update, and while the agent is away, the static template icons from `icons/tray` stand in.

use crate::link::AgentLink;
use crate::logging;
use crate::placement::{click_closes, dropdown_position, Rect};
use crate::tray_model::Frame;
use crate::window;
use dyno_core::protocol::Request;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, PhysicalPosition, Wry};

/// The tray's id.
pub const TRAY_ID: &str = "dyno";
/// The dropdown window's label (in `capabilities/default.json`).
pub const DROPDOWN: &str = "tray";
/// Dropdown size in logical pixels.
pub const DROPDOWN_SIZE: (f64, f64) = (360.0, 520.0);
const OPEN: &str = "open";
const PAUSE: &str = "pause";
const QUIT: &str = "quit";

/// Menu items kept to change their text, and when the dropdown last closed itself.
pub struct TrayState {
    pause: MenuItem<Wry>,
    closed_at: Mutex<Option<Instant>>,
    painted: Mutex<bool>,
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

/// Creates the tray icon and manages its state.
pub fn create(app: &AppHandle<Wry>) -> tauri::Result<()> {
    let (menu, pause) = build_menu(app)?;
    let icon = Image::from_bytes(static_icon_bytes(dark_taskbar()))?;
    TrayIconBuilder::with_id(TRAY_ID)
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
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                let scale = 1.0;
                let p = rect.position.to_physical::<f64>(scale);
                let s = rect.size.to_physical::<f64>(scale);
                toggle_dropdown(
                    tray.app_handle(),
                    Rect {
                        x: p.x,
                        y: p.y,
                        w: s.width,
                        h: s.height,
                    },
                );
            }
        })
        .build(app)?;
    app.manage(TrayState {
        pause,
        closed_at: Mutex::new(None),
        painted: Mutex::new(false),
    });
    Ok(())
}

/// Draws a frame: the pixels, the tooltip, and the Pause item's text.
pub fn paint(app: &AppHandle<Wry>, frame: &Frame, paused: bool) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let icon = Image::new_owned(frame.icon.rgba.clone(), frame.icon.size, frame.icon.size);
    // The drawn icon has its own colours: never a template image.
    let _ = tray.set_icon(Some(icon));
    let _ = tray.set_icon_as_template(false);
    let _ = tray.set_tooltip(Some(frame.tooltip.clone()));
    if let Some(state) = app.try_state::<TrayState>() {
        let _ = state.pause.set_text(pause_text(paused));
        *state.painted.lock().unwrap_or_else(|e| e.into_inner()) = true;
    }
}

/// While the agent is away the static icon is back (a frozen bar would look like a measurement).
pub fn show_disconnected(app: &AppHandle<Wry>) {
    let Some(state) = app.try_state::<TrayState>() else { return };
    let was_painted = std::mem::replace(&mut *state.painted.lock().unwrap_or_else(|e| e.into_inner()), false);
    if !was_painted {
        return;
    }
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    if let Ok(icon) = Image::from_bytes(static_icon_bytes(dark_taskbar())) {
        let _ = tray.set_icon(Some(icon));
        let _ = tray.set_icon_as_template(cfg!(target_os = "macos"));
        let _ = tray.set_tooltip(Some("Dyno — the agent is not running"));
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

/// Opens the dropdown under the icon, or closes it when it is open.
pub fn toggle_dropdown(app: &AppHandle<Wry>, icon: Rect) {
    if let Some(w) = app.get_webview_window(DROPDOWN) {
        let _ = w.destroy();
        return;
    }
    let since = app
        .try_state::<TrayState>()
        .and_then(|s| s.closed_at.lock().unwrap_or_else(|e| e.into_inner()).map(|t| t.elapsed()));
    if click_closes(since) {
        return;
    }
    if let Err(e) = open_dropdown(app, icon) {
        logging::info(&format!("could not open the tray dropdown: {e}"));
    }
}

fn open_dropdown(app: &AppHandle<Wry>, icon: Rect) -> tauri::Result<()> {
    let monitor = app
        .monitor_from_point(icon.x + icon.w / 2.0, icon.y + icon.h / 2.0)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());
    let (scale, bounds) = match &monitor {
        Some(m) => (
            m.scale_factor(),
            Rect {
                x: f64::from(m.position().x),
                y: f64::from(m.position().y),
                w: f64::from(m.size().width),
                h: f64::from(m.size().height),
            },
        ),
        None => (
            1.0,
            Rect {
                x: 0.0,
                y: 0.0,
                w: 1920.0,
                h: 1080.0,
            },
        ),
    };
    let (x, y) = dropdown_position(icon, (DROPDOWN_SIZE.0 * scale, DROPDOWN_SIZE.1 * scale), bounds);
    let w = tauri::WebviewWindowBuilder::new(app, DROPDOWN, tauri::WebviewUrl::App("index.html#/tray".into()))
        .title("Dyno")
        .inner_size(DROPDOWN_SIZE.0, DROPDOWN_SIZE.1)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(true)
        .initialization_script(window::init_script(crate::dto::os_name(), "none"))
        .build()?;
    w.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))?;
    crate::native_feel::apply(&w);
    let _ = w.set_focus();
    Ok(())
}

/// The dropdown lost focus: it closes, and the tray remembers when (see `placement::click_closes`).
pub fn dropdown_blurred(app: &AppHandle<Wry>) {
    if let Some(state) = app.try_state::<TrayState>() {
        *state.closed_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    }
    if let Some(w) = app.get_webview_window(DROPDOWN) {
        let _ = w.destroy();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn the_dropdown_window_may_use_the_ipc_commands() {
        let caps: serde_json::Value = serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let windows: Vec<&str> = caps["windows"].as_array().unwrap().iter().filter_map(|w| w.as_str()).collect();
        assert!(windows.contains(&DROPDOWN) && windows.contains(&window::MAIN));
    }
}
