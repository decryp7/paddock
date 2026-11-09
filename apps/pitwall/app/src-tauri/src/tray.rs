//! The tray (Windows notification area, Linux status notifier, macOS menu bar), on the shell's tray (`paddock_window`):
//! the shell creates the icon at start, redraws it when [`refresh_tray`](paddock_window::tauri::refresh_tray) is called
//! and runs its own items (Open is the shell's `pd.show`); this module turns Pitwall's [`TrayView`] into the shell's view
//! and runs Pitwall's own items. What the tray shows is decided in `tray_model.rs`. Icons are the suite's pattern
//! (`icons/tray/`): macOS template images the system tints, and on Windows a light and a dark variant chosen from the
//! taskbar's theme. Each state has its own shape, not only its own colour.

use crate::commands::{self, SharedClient};
use crate::quit::{self, QuitChoice};
use crate::tray_model::{Entry, TrayAction, TrayState, TrayView};
use crate::SharedModel;
use paddock_window::tauri::{NativeTray, PopoverSpec, Popovers, Tray, TrayBackend};
use paddock_window::tray::{self as shell_tray, SHOW_ID};
use tauri::image::Image;
use tauri::{AppHandle, Manager, Runtime};

/// The name of a state's icon in the shell's view (and back).
pub const fn icon_name(state: TrayState) -> &'static str {
    match state {
        TrayState::Idle => "idle",
        TrayState::Busy => "busy",
        TrayState::Paused => "paused",
        TrayState::Unavailable => "unavailable",
    }
}

/// The state an icon name stands for.
pub fn state_of(name: &str) -> Option<TrayState> {
    [TrayState::Idle, TrayState::Busy, TrayState::Paused, TrayState::Unavailable]
        .into_iter()
        .find(|s| icon_name(*s) == name)
}

/// The PNG bytes for a state: the macOS template image, or the light/dark variant for the taskbar.
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
    icons!(Idle => "normal", Busy => "busy", Paused => "paused", Unavailable => "error")
}

/// Whether the taskbar is dark (Windows reads the system setting; elsewhere dark, the common case for menu bars).
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

/// The shell's view of Pitwall's tray: the icon by name, the tooltip, and the menu with Open as the shell's own item.
pub fn shell_view(view: &TrayView) -> shell_tray::TrayView {
    let menu = view
        .entries
        .iter()
        .map(|e| match e {
            Entry::Status(text) => shell_tray::disabled(text),
            Entry::Separator => shell_tray::TrayEntry::Separator,
            Entry::Item { action, label, enabled } => {
                let id = if *action == TrayAction::Open { SHOW_ID } else { action.id() };
                shell_tray::TrayEntry::Item {
                    id: id.to_string(),
                    text: shell_tray::short(label),
                    enabled: *enabled,
                }
            }
        })
        .collect();
    shell_tray::TrayView::new(&view.tooltip, menu, icon_name(view.state))
}

/// The popover's route in the UI (`main.ts` boots the tray window for it): memory, throughput, loaded models, Pause and
/// Free memory, Open Pitwall.
pub const POPOVER_ROUTE: &str = "index.html#/tray";
/// The popover's width (logical px), the UI's tray layout.
pub const POPOVER_WIDTH: f64 = 360.0;

/// The native tray icon: Tauri's, with Pitwall's pictures per state. A left click opens the popover on every OS; the
/// right click (and Control-click) opens the menu.
pub fn native() -> NativeTray {
    NativeTray::new(|name| state_of(name).and_then(|s| Image::from_bytes(icon_bytes(s, dark_taskbar())).ok())).menu_on_left_click(false)
}

/// The shell's tray over `backend`, with its popover: the model reads the facts the live feed keeps, and Pitwall's own items
/// run here.
pub fn tray<R: Runtime>(app: &AppHandle<R>, backend: impl TrayBackend<R>) -> Tray<R> {
    app.manage(Popovers::new().popover(None, PopoverSpec::new(POPOVER_ROUTE, crate::APP_NAME).width(POPOVER_WIDTH)));
    Tray::new(backend, |app: &AppHandle<R>| {
        let view = match app.try_state::<SharedModel>() {
            Some(m) => m.lock().unwrap_or_else(|p| p.into_inner()).view(),
            None => crate::tray_model::TrayModel::new().view(),
        };
        shell_view(&view)
    })
    .on_action(|app, id| {
        if let Some(action) = TrayAction::parse(id) {
            act(app, action);
        }
    })
    .popover(true)
}

/// Runs one of Pitwall's tray items (Open is the shell's).
fn act<R: Runtime>(app: &AppHandle<R>, action: TrayAction) {
    match action {
        TrayAction::Open => {
            if let Err(e) = paddock_window::tauri::reveal(app, "main") {
                eprintln!("Pitwall: the window could not be shown ({})", e.message);
            }
        }
        TrayAction::Quit | TrayAction::QuitAndStop => {
            let app = app.clone();
            let forced = (action == TrayAction::QuitAndStop).then_some(QuitChoice::StopServing);
            tauri::async_runtime::spawn(async move { quit::request(&app, forced).await });
        }
        TrayAction::ToggleServing | TrayAction::ReleaseMemory => {
            let Some(client) = app.try_state::<SharedClient>().map(|c| c.inner().clone()) else {
                return;
            };
            let paused = app
                .try_state::<SharedModel>()
                .is_some_and(|m| m.lock().unwrap_or_else(|p| p.into_inner()).paused());
            tauri::async_runtime::spawn(async move {
                let result = match action {
                    TrayAction::ToggleServing => {
                        commands::forward(client.as_ref(), "set_serving", serde_json::json!({ "paused": !paused })).await
                    }
                    _ => commands::forward(client.as_ref(), "release_memory", serde_json::Value::Null).await,
                };
                if let Err(e) = result {
                    eprintln!("Pitwall: the tray action failed ({:?})", e.code);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tray_model::TrayModel;

    const STATES: [TrayState; 4] = [TrayState::Idle, TrayState::Busy, TrayState::Paused, TrayState::Unavailable];

    #[test]
    fn every_state_has_an_icon_for_both_taskbars() {
        for state in STATES {
            for dark in [false, true] {
                let png = icon_bytes(state, dark);
                assert_eq!(&png[1..4], b"PNG", "{state:?}");
                assert!(Image::from_bytes(png).is_ok(), "{state:?} decodes");
            }
        }
    }

    #[test]
    fn the_states_have_different_pictures() {
        let all = STATES.map(|s| icon_bytes(s, true));
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                assert_ne!(all[i], all[j], "{i} vs {j}");
            }
        }
    }

    #[test]
    fn every_state_round_trips_through_its_icon_name() {
        for s in STATES {
            assert_eq!(state_of(icon_name(s)), Some(s));
        }
        assert_eq!(state_of("nonsense"), None);
    }

    /// The shell's menu rules hold for the view Pitwall draws (ids, depth, count), Open is the shell's own item and every
    /// other item comes back to one of Pitwall's actions.
    #[test]
    fn the_shell_view_is_valid_and_routes_the_items() {
        let view = shell_view(&TrayModel::new().view());
        assert_eq!(paddock_window::tray::validate(&view.menu), Ok(()));
        assert_eq!(view.icon, "unavailable");
        let ids: Vec<&str> = view
            .menu
            .iter()
            .filter_map(|e| match e {
                shell_tray::TrayEntry::Item { id, enabled: true, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert!(ids.contains(&SHOW_ID), "{ids:?}");
        for id in ids.iter().filter(|id| **id != SHOW_ID) {
            assert!(TrayAction::parse(id).is_some(), "{id} is one of Pitwall's actions");
            assert!(!id.starts_with("pd."), "{id} would be taken by the shell");
        }
    }
}
