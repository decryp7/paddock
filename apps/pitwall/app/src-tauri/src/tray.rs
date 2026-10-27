//! The tray (Windows notification area, Linux status notifier, macOS menu bar): draws a [`TrayView`] and forwards the
//! menu's choices. What it shows is decided in `tray_model.rs`; when to open or rebuild the window is decided in
//! `window.rs`. Icons are the suite's pattern (`icons/tray/`): macOS template images the system tints, and on Windows a
//! light and a dark variant chosen from the taskbar's theme. Each state has its own shape, not only its own colour.
//! (Adapted from Pitcrew's tray.)

use crate::tray_model::{Entry, TrayAction, TrayState, TrayView};
use std::sync::Mutex;
use tauri::image::Image;
use tauri::menu::{Menu, MenuBuilder, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Runtime};

/// The tray's id.
pub const TRAY_ID: &str = "pitwall";

/// Marks an app whose tray icon could not be created (Linux without an appindicator): there is no way back to a
/// closed window, so closing it must exit.
pub struct NoTray;

/// The view on screen now, so equal views are not redrawn (rebuilding a menu closes it if it is open).
#[derive(Default)]
pub struct Drawn(Mutex<Option<TrayView>>);

/// The icon for a state (PNG bytes built into the app).
pub fn icon_bytes(state: TrayState, dark_taskbar: bool) -> &'static [u8] {
    macro_rules! icons {
        ($($s:ident => $k:literal),*) => {
            match (state, dark_taskbar, cfg!(target_os = "macos")) {
                $(
                    (TrayState::$s, _, true) => include_bytes!(concat!("../icons/tray/macos/pitwall-tray-", $k, "-18ptTemplate@2x.png")),
                    (TrayState::$s, true, false) => include_bytes!(concat!("../icons/tray/windows/pitwall-tray-", $k, "-dark-32.png")),
                    (TrayState::$s, false, false) => include_bytes!(concat!("../icons/tray/windows/pitwall-tray-", $k, "-light-32.png")),
                )*
            }
        };
    }
    // The family's team-radio glyph (paddock icons/pitwall/tray): normal is the plain glyph, working a ring, paused two
    // bars, a stopped server a cross.
    icons!(Idle => "normal", Busy => "busy", Paused => "paused", Unavailable => "error")
}

/// Whether the taskbar is dark (read-only registry lookup on Windows; dark when unknown, the Windows 11 default, and
/// on Linux where panels are mostly dark).
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

fn build_menu<R: Runtime>(app: &AppHandle<R>, view: &TrayView) -> tauri::Result<Menu<R>> {
    let mut menu = MenuBuilder::new(app);
    for (i, e) in view.entries.iter().enumerate() {
        menu = match e {
            Entry::Status(text) => menu.item(&MenuItem::with_id(app, format!("status-{i}"), text, false, None::<&str>)?),
            Entry::Separator => menu.item(&PredefinedMenuItem::separator(app)?),
            Entry::Item { action, label, enabled } => menu.item(&MenuItem::with_id(app, action.id(), label, *enabled, None::<&str>)?),
        };
    }
    menu.build()
}

/// Creates the tray icon. `on_action` runs for every menu choice and for a left click on the icon (Windows, Linux).
pub fn create<R: Runtime>(
    app: &AppHandle<R>,
    view: &TrayView,
    on_action: impl Fn(&AppHandle<R>, TrayAction) + Send + Sync + 'static,
) -> tauri::Result<()> {
    let menu = build_menu(app, view)?;
    let icon = Image::from_bytes(icon_bytes(view.state, dark_taskbar()))?;
    let on_action = std::sync::Arc::new(on_action);
    let on_menu = on_action.clone();
    TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(&view.tooltip)
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .menu(&menu)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(move |app, event| {
            if let Some(action) = TrayAction::parse(event.id().as_ref()) {
                on_menu(app, action);
            }
        })
        .on_tray_icon_event(move |tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if !cfg!(target_os = "macos") {
                    on_action(tray.app_handle(), TrayAction::Open);
                }
            }
        })
        .build(app)?;
    Ok(())
}

/// Draws `view` unless it is what is on screen already.
pub fn update<R: Runtime>(app: &AppHandle<R>, drawn: &Drawn, view: &TrayView) {
    {
        let mut last = drawn.0.lock().unwrap_or_else(|p| p.into_inner());
        if last.as_ref() == Some(view) {
            return;
        }
        *last = Some(view.clone());
    }
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    if let Ok(icon) = Image::from_bytes(icon_bytes(view.state, dark_taskbar())) {
        let _ = tray.set_icon(Some(icon));
        let _ = tray.set_icon_as_template(cfg!(target_os = "macos"));
    }
    let _ = tray.set_tooltip(Some(&view.tooltip));
    if let Ok(menu) = build_menu(app, view) {
        let _ = tray.set_menu(Some(menu));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_has_an_icon_for_both_taskbars() {
        for state in [TrayState::Idle, TrayState::Busy, TrayState::Paused, TrayState::Unavailable] {
            for dark in [false, true] {
                let png = icon_bytes(state, dark);
                assert_eq!(&png[1..4], b"PNG", "{state:?}");
                assert!(Image::from_bytes(png).is_ok(), "{state:?} decodes");
            }
        }
    }

    #[test]
    fn the_states_have_different_pictures() {
        let all = [TrayState::Idle, TrayState::Busy, TrayState::Paused, TrayState::Unavailable].map(|s| icon_bytes(s, true));
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                assert_ne!(all[i], all[j], "{i} vs {j}");
            }
        }
    }

    #[test]
    fn an_equal_view_is_not_drawn_twice() {
        let drawn = Drawn::default();
        let app = tauri::test::mock_app();
        let view = crate::tray_model::TrayModel::new().view();
        update(app.handle(), &drawn, &view);
        assert_eq!(drawn.0.lock().unwrap().as_ref(), Some(&view));
        // No tray exists in the mock app; the second call returns early on equality (nothing to assert beyond not panicking).
        update(app.handle(), &drawn, &view);
    }
}
