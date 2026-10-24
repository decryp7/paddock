//! The native tray icon (feature `tray`): Tauri's tray icon with the [`TrayView`]'s menu, tooltip and icon. On macOS
//! the icon is a template image the menu bar tints; on Windows a session without a taskbar (session 0, a service, a CI
//! runner) has no tray, and the app runs without one.

use super::tray::{TrayBackend, icon_clicked, menu_chosen};
use crate::popover::Rect;
use crate::tray::{TrayEntry, TrayView};
use ::tauri::image::Image;
use ::tauri::menu::{IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use ::tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use ::tauri::{AppHandle, Runtime};

/// The tray icon's id.
pub const TRAY_ID: &str = "paddock";

type Icons = dyn Fn(&str) -> Option<Image<'static>> + Send + Sync;

/// Draws the tray with Tauri's tray icon.
pub struct NativeTray {
    icons: Box<Icons>,
    template: bool,
    menu_on_left_click: bool,
}

impl NativeTray {
    /// `icons` maps the view's icon name to the image (template images on macOS); `None` falls back to the app's
    /// window icon.
    pub fn new(
        icons: impl Fn(&str) -> Option<Image<'static>> + Send + Sync + 'static,
    ) -> NativeTray {
        NativeTray {
            icons: Box::new(icons),
            template: cfg!(target_os = "macos"),
            menu_on_left_click: cfg!(target_os = "macos"),
        }
    }

    /// Whether the images are templates the menu bar tints (default: on macOS).
    pub fn template(mut self, on: bool) -> Self {
        self.template = on;
        self
    }

    /// Whether a left click opens the menu (default: on macOS). Turn it off for an app whose left click opens the
    /// popover.
    pub fn menu_on_left_click(mut self, on: bool) -> Self {
        self.menu_on_left_click = on;
        self
    }
}

/// The menu of `entries`. Disabled lines without an id get `pd.none-<n>`.
pub(crate) fn to_menu<R: Runtime>(
    app: &AppHandle<R>,
    entries: &[TrayEntry],
) -> ::tauri::Result<Menu<R>> {
    fn add<R: Runtime>(
        app: &AppHandle<R>,
        entry: &TrayEntry,
        n: &mut usize,
        push: &mut dyn FnMut(&dyn IsMenuItem<R>) -> ::tauri::Result<()>,
    ) -> ::tauri::Result<()> {
        *n += 1;
        match entry {
            TrayEntry::Item { id, text, enabled } => {
                let id = if id.is_empty() {
                    format!("pd.none-{n}")
                } else {
                    id.clone()
                };
                push(&MenuItem::with_id(app, id, text, *enabled, None::<&str>)?)
            }
            TrayEntry::Separator => push(&PredefinedMenuItem::separator(app)?),
            TrayEntry::Submenu { text, items } => {
                let sub = Submenu::new(app, text, true)?;
                for i in items {
                    add(app, i, n, &mut |item| sub.append(item))?;
                }
                push(&sub)
            }
        }
    }
    let menu = Menu::new(app)?;
    let mut n = 0;
    for e in entries {
        add(app, e, &mut n, &mut |item| menu.append(item))?;
    }
    Ok(menu)
}

/// Whether the desktop has somewhere to show a tray icon: on Windows, the taskbar's window exists.
fn host_available() -> bool {
    #[cfg(windows)]
    #[allow(unsafe_code)]
    {
        use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
        use windows::core::{PCWSTR, w};
        // SAFETY: a plain lookup with a constant, NUL-terminated class name; nothing is retained.
        unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.is_ok_and(|h| !h.is_invalid())
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// The physical rectangle of the clicked icon.
fn icon_rect(rect: &::tauri::Rect) -> Rect {
    let p = rect.position.to_physical::<f64>(1.0);
    let s = rect.size.to_physical::<f64>(1.0);
    Rect {
        x: p.x,
        y: p.y,
        w: s.width,
        h: s.height,
    }
}

/// The anchor of a left click (the released left button), from a tray icon event.
pub(crate) fn left_click_rect(event: &TrayIconEvent) -> Option<Rect> {
    match event {
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            rect,
            ..
        } => Some(icon_rect(rect)),
        _ => None,
    }
}

fn on_icon<R: Runtime>(app: &AppHandle<R>, event: &TrayIconEvent) {
    if let Some(rect) = left_click_rect(event) {
        icon_clicked(app, Some(rect));
    }
}

fn on_menu<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    menu_chosen(app, event.id().as_ref());
}

impl NativeTray {
    /// The builder [`create`](TrayBackend::create) builds: the view's menu, tooltip and icon and the shell's event
    /// handlers. Building it creates the icon; making it does not.
    pub fn icon_builder<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        view: &TrayView,
    ) -> Result<TrayIconBuilder<R>, String> {
        let menu = to_menu(app, &view.menu).map_err(|e| e.to_string())?;
        let icon = (self.icons)(&view.icon)
            .or_else(|| app.default_window_icon().cloned())
            .ok_or("the app has no tray icon image")?;
        Ok(TrayIconBuilder::with_id(TRAY_ID)
            .tooltip(&view.tooltip)
            .menu(&menu)
            .show_menu_on_left_click(self.menu_on_left_click)
            .icon(icon)
            .icon_as_template(self.template)
            .on_menu_event(on_menu)
            .on_tray_icon_event(|tray, event| on_icon(tray.app_handle(), &event)))
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clicked_icon_is_its_physical_rectangle() {
        let r = ::tauri::Rect {
            position: ::tauri::PhysicalPosition::new(10, 20).into(),
            size: ::tauri::PhysicalSize::new(30, 40).into(),
        };
        assert_eq!(
            icon_rect(&r),
            Rect {
                x: 10.0,
                y: 20.0,
                w: 30.0,
                h: 40.0
            }
        );
        assert!(host_available() || cfg!(windows));
        let click = |button, button_state| TrayIconEvent::Click {
            id: TRAY_ID.into(),
            position: ::tauri::PhysicalPosition::new(0.0, 0.0),
            rect: r,
            button,
            button_state,
        };
        assert!(left_click_rect(&click(MouseButton::Left, MouseButtonState::Up)).is_some());
        assert!(left_click_rect(&click(MouseButton::Left, MouseButtonState::Down)).is_none());
        assert!(left_click_rect(&click(MouseButton::Right, MouseButtonState::Up)).is_none());
        let t = NativeTray::new(|_| None)
            .template(true)
            .menu_on_left_click(false);
        assert!(t.template && !t.menu_on_left_click && (t.icons)("x").is_none());
    }

    #[test]
    fn clicks_and_menu_choices_reach_the_shell_and_a_missing_icon_is_reported() {
        let app = ::tauri::test::mock_app();
        let h = app.handle();
        let r = ::tauri::Rect {
            position: ::tauri::PhysicalPosition::new(10, 20).into(),
            size: ::tauri::PhysicalSize::new(30, 40).into(),
        };
        // Without the window layer both are logged no-ops.
        on_icon(
            h,
            &TrayIconEvent::Click {
                id: TRAY_ID.into(),
                position: ::tauri::PhysicalPosition::new(0.0, 0.0),
                rect: r,
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
            },
        );
        on_menu(
            h,
            MenuEvent {
                id: "pd.show".into(),
            },
        );
        let view = TrayView::new("x", vec![], "idle");
        let t = NativeTray::new(|_| None);
        assert!(TrayBackend::update(&t, h, &view).is_err(), "no icon yet");
    }
}
