//! The tray (Windows notification area, macOS menu bar, Linux status notifier): an icon that shows whether there is
//! something unseen, a tooltip with the counts and, on a right click, a menu with the unseen releases and quick
//! actions. A left click opens the popup window (`index.html#/tray`), a frameless transparent window anchored to the
//! icon that closes when it loses focus.
//!
//! Closing the main window destroys it (a hidden webview could come back blank on macOS and keeps its memory); the
//! tray's Open, or the Dock icon on macOS, creates it again from the configuration. Only an explicit Quit exits.
//! Without a tray (some Linux desktops) closing the window quits.

use spotter_engine::view::WatchSummary;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::menu::{Menu, MenuBuilder, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Runtime, WebviewUrl, WebviewWindowBuilder};

/// The tray's id.
pub const TRAY_ID: &str = "spotter";
/// The main window's label (`tauri.conf.json`).
pub const MAIN: &str = "main";
/// The popup window's label.
pub const POPUP: &str = "tray";
/// The event that asks the main window to show a route.
pub const NAVIGATE_EVENT: &str = "spotter://navigate";

const OPEN: &str = "open";
const QUIT: &str = "quit";
const CHECK_ALL: &str = "check-all";
const PAUSE: &str = "pause";
const RESUME: &str = "resume";
const WATCH_PREFIX: &str = "watch:";

/// The popup's size in logical pixels.
pub const POPUP_SIZE: (f64, f64) = (360.0, 440.0);
/// How many unseen releases the right-click menu lists.
const MENU_WATCHES: usize = 8;
/// A click on the icon within this time after the popup lost focus is the click that closed it, not a new one.
const REOPEN_GUARD: Duration = Duration::from_millis(350);

/// What the tray icon shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
    /// Nothing unseen.
    Normal,
    /// At least one unseen release.
    New,
}

/// The icon state for the current watches.
pub fn tray_state(watches: &[WatchSummary]) -> TrayState {
    if watches.iter().any(|w| w.unseen && !w.paused) {
        TrayState::New
    } else {
        TrayState::Normal
    }
}

/// The tooltip: a summary line.
pub fn tooltip(watches: &[WatchSummary]) -> String {
    let unseen = watches.iter().filter(|w| w.unseen).count();
    let problems = watches
        .iter()
        .filter(|w| !w.paused && matches!(w.status, spotter_core::state::WatchStatus::NeedsYou(_)))
        .count();
    let mut s = match (watches.len(), unseen) {
        (0, _) => "Spotter - no watches yet".to_string(),
        (n, 0) => format!("Spotter - {n} watch{}, nothing new", if n == 1 { "" } else { "es" }),
        (_, u) => format!("Spotter - {u} new release{}", if u == 1 { "" } else { "s" }),
    };
    if problems > 0 {
        s.push_str(&format!(", {problems} need{} you", if problems == 1 { "s" } else { "" }));
    }
    s
}

/// The icon for a state (PNG bytes built into the app): template images on macOS (the system tints them), a light
/// and a dark variant on Windows and Linux chosen from the taskbar's theme.
pub fn icon_bytes(state: TrayState, dark_taskbar: bool) -> &'static [u8] {
    macro_rules! icons {
        ($($s:ident => $k:literal),*) => {
            match (state, dark_taskbar, cfg!(target_os = "macos")) {
                $(
                    (TrayState::$s, _, true) => include_bytes!(concat!("../icons/tray/spotter-tray-", $k, "-18ptTemplate@2x.png")),
                    (TrayState::$s, true, false) => include_bytes!(concat!("../icons/tray/spotter-tray-", $k, "-dark-32.png")),
                    (TrayState::$s, false, false) => include_bytes!(concat!("../icons/tray/spotter-tray-", $k, "-light-32.png")),
                )*
            }
        };
    }
    icons!(Normal => "normal", New => "new")
}

/// Whether the Windows taskbar is dark (read-only registry lookup; dark when unknown, the Windows 11 default).
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

/// One entry of the right-click menu, before it becomes a native menu (so the content is testable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    /// A disabled line of text.
    Header(String),
    /// A separator.
    Separator,
    /// A clickable item: id, label.
    Item(String, String),
}

/// The menu for the current watches; `paused` is whether notifications are paused.
pub fn menu_model(watches: &[WatchSummary], paused: bool) -> Vec<MenuEntry> {
    let mut m = vec![MenuEntry::Header(tooltip(watches)), MenuEntry::Separator];
    let unseen: Vec<&WatchSummary> = watches.iter().filter(|w| w.unseen).collect();
    for w in unseen.iter().take(MENU_WATCHES) {
        let version = w.latest.as_ref().map_or("", |l| l.version.as_str());
        m.push(MenuEntry::Item(format!("{WATCH_PREFIX}{}", w.id), format!("{} {version}", w.name)));
    }
    if unseen.len() > MENU_WATCHES {
        m.push(MenuEntry::Header(format!("and {} more", unseen.len() - MENU_WATCHES)));
    }
    if !unseen.is_empty() {
        m.push(MenuEntry::Separator);
    }
    m.push(MenuEntry::Item(CHECK_ALL.into(), "Check all now".into()));
    if paused {
        m.push(MenuEntry::Item(RESUME.into(), "Resume notifications".into()));
    } else {
        m.push(MenuEntry::Item(PAUSE.into(), "Pause notifications for 1 hour".into()));
    }
    m.push(MenuEntry::Separator);
    m.push(MenuEntry::Item(OPEN.into(), "Open Spotter".into()));
    m.push(MenuEntry::Item(QUIT.into(), "Quit Spotter".into()));
    m
}

/// The watch id of a menu item id from [`menu_model`].
pub fn watch_of_item(id: &str) -> Option<&str> {
    id.strip_prefix(WATCH_PREFIX)
}

/// A route the webview or the tray may ask the main window to show: `/` and then letters, digits, `-`, `_`, `/`.
pub fn valid_route(route: &str) -> bool {
    route.len() <= 200
        && route.starts_with('/')
        && !route.contains("//")
        && route.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_'))
}

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Left.
    pub x: i32,
    /// Top.
    pub y: i32,
    /// Width.
    pub w: i32,
    /// Height.
    pub h: i32,
}

/// Where the popup goes: centred on the tray icon, below it when the icon is in the upper half of the screen (macOS
/// menu bar) and above it otherwise (Windows taskbar), kept inside the work area with a small margin.
pub fn popup_position(icon: Rect, work: Rect, popup: (i32, i32)) -> (i32, i32) {
    const GAP: i32 = 6;
    let (w, h) = popup;
    let x = (icon.x + icon.w / 2 - w / 2).clamp(work.x + GAP, (work.x + work.w - w - GAP).max(work.x + GAP));
    let below = icon.y + icon.h / 2 < work.y + work.h / 2;
    let y = if below { icon.y + icon.h + GAP } else { icon.y - h - GAP };
    (x, y.clamp(work.y + GAP, (work.y + work.h - h - GAP).max(work.y + GAP)))
}

/// Marks an app whose tray icon could not be created (Linux without an appindicator): there is no way back to a
/// closed window, so closing it must exit.
pub struct NoTray;

/// Whether the app has a tray icon.
pub fn has_tray<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<NoTray>().is_none()
}

/// Whether an exit request leaves the app running: closing the last window does when there is a tray to reopen it
/// from; Quit (an explicit exit code) never does.
pub fn keeps_running(exit_code: Option<i32>, tray: bool) -> bool {
    exit_code.is_none() && tray
}

/// When the popup last closed because it lost focus.
#[derive(Default)]
pub struct PopupGuard(Mutex<Option<Instant>>);

impl PopupGuard {
    /// Remembers that the popup just closed.
    pub fn closed(&self) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(Instant::now());
    }

    /// Whether a click now is the one that closed the popup.
    pub fn just_closed(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some_and(|t| t.elapsed() < REOPEN_GUARD)
    }
}

fn build_menu<R: Runtime>(app: &AppHandle<R>, watches: &[WatchSummary], paused: bool) -> tauri::Result<Menu<R>> {
    let mut menu = MenuBuilder::new(app);
    for (i, e) in menu_model(watches, paused).into_iter().enumerate() {
        menu = match e {
            MenuEntry::Header(t) => menu.item(&MenuItem::with_id(app, format!("header-{i}"), t, false, None::<&str>)?),
            MenuEntry::Separator => menu.item(&PredefinedMenuItem::separator(app)?),
            MenuEntry::Item(id, label) => menu.item(&MenuItem::with_id(app, id, label, true, None::<&str>)?),
        };
    }
    menu.build()
}

/// Creates the tray icon.
pub fn create<R: Runtime>(app: &AppHandle<R>, watches: &[WatchSummary], paused: bool) -> tauri::Result<()> {
    let menu = build_menu(app, watches, paused)?;
    let icon = Image::from_bytes(icon_bytes(tray_state(watches), dark_taskbar()))?;
    TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(tooltip(watches))
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| crate::lifecycle::on_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                toggle_popup(tray.app_handle(), rect);
            }
        })
        .build(app)?;
    Ok(())
}

/// Updates icon, tooltip and menu.
pub fn update<R: Runtime>(app: &AppHandle<R>, watches: &[WatchSummary], paused: bool) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    if let Ok(icon) = Image::from_bytes(icon_bytes(tray_state(watches), dark_taskbar())) {
        let _ = tray.set_icon(Some(icon));
        let _ = tray.set_icon_as_template(cfg!(target_os = "macos"));
    }
    let _ = tray.set_tooltip(Some(tooltip(watches)));
    if let Ok(menu) = build_menu(app, watches, paused) {
        let _ = tray.set_menu(Some(menu));
    }
}

/// Opens the popup next to the tray icon, or closes it when it is open.
pub fn toggle_popup<R: Runtime>(app: &AppHandle<R>, icon: tauri::Rect) {
    if let Some(w) = app.get_webview_window(POPUP) {
        let _ = w.destroy();
        return;
    }
    if app.state::<PopupGuard>().just_closed() {
        return;
    }
    let scale = app.monitor_from_point(0.0, 0.0).ok().flatten().map_or(1.0, |m| m.scale_factor());
    let pos: PhysicalPosition<i32> = icon.position.to_physical(scale);
    let size: tauri::PhysicalSize<u32> = icon.size.to_physical(scale);
    let monitor = app.monitor_from_point(f64::from(pos.x), f64::from(pos.y)).ok().flatten();
    let (scale, work) = match &monitor {
        Some(m) => {
            let a = m.work_area();
            (
                m.scale_factor(),
                Rect {
                    x: a.position.x,
                    y: a.position.y,
                    w: a.size.width as i32,
                    h: a.size.height as i32,
                },
            )
        }
        None => (
            1.0,
            Rect {
                x: 0,
                y: 0,
                w: 1920,
                h: 1080,
            },
        ),
    };
    let popup = ((POPUP_SIZE.0 * scale) as i32, (POPUP_SIZE.1 * scale) as i32);
    let at = popup_position(
        Rect {
            x: pos.x,
            y: pos.y,
            w: size.width as i32,
            h: size.height as i32,
        },
        work,
        popup,
    );
    let builder = WebviewWindowBuilder::new(app, POPUP, WebviewUrl::App("index.html#/tray".into()))
        .title("Spotter")
        .inner_size(POPUP_SIZE.0, POPUP_SIZE.1)
        .decorations(false)
        .transparent(true)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(true)
        .focused(true)
        .visible(true);
    #[cfg(target_os = "macos")]
    let builder = builder.effects(tauri::utils::config::WindowEffectsConfig {
        effects: vec![tauri::utils::WindowEffect::Popover],
        state: Some(tauri::utils::WindowEffectState::Active),
        radius: Some(12.0),
        color: None,
        interactive: false,
    });
    #[cfg(windows)]
    let builder = builder.effects(tauri::utils::config::WindowEffectsConfig {
        effects: vec![tauri::utils::WindowEffect::Acrylic],
        state: None,
        radius: None,
        color: None,
        interactive: false,
    });
    match builder.build() {
        Ok(w) => {
            let _ = w.set_position(PhysicalPosition::new(at.0, at.1));
            crate::native_feel::apply(&w);
            let handle = app.clone();
            w.on_window_event(move |e| {
                if matches!(e, tauri::WindowEvent::Focused(false)) {
                    handle.state::<PopupGuard>().closed();
                    if let Some(p) = handle.get_webview_window(POPUP) {
                        let _ = p.destroy();
                    }
                }
            });
        }
        Err(e) => crate::applog::write(&format!("the tray popup could not be opened: {e}")),
    }
}

/// Closes the popup if it is open.
pub fn hide_popup<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(POPUP) {
        let _ = w.destroy();
    }
}

/// Shows and focuses the main window, creating it again from the configuration when it was closed. `route` (already
/// validated) is shown: through the URL of a window that is being created, through an event for one that exists.
pub fn show_main<R: Runtime>(app: &AppHandle<R>, route: Option<&str>) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        if let Some(r) = route {
            let _ = app.emit_to(MAIN, NAVIGATE_EVENT, r.to_string());
        }
        return;
    }
    let Some(mut cfg) = app.config().app.windows.iter().find(|w| w.label == MAIN).cloned() else {
        return;
    };
    if let Some(r) = route {
        // The hash route is read when the app boots, so a window created for a click lands on the right page.
        cfg.url = WebviewUrl::App(format!("index.html#{r}").into());
    }
    let builder = WebviewWindowBuilder::from_config(app, &cfg);
    match builder.and_then(|b| b.build()) {
        Ok(w) => {
            crate::native_feel::apply(&w);
            let _ = w.set_focus();
        }
        Err(e) => crate::applog::write(&format!("the main window could not be opened: {e}")),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_taskbar_theme_has_a_default() {
        // Read-only lookup; on platforms without a taskbar setting it is dark.
        let _ = dark_taskbar();
        #[cfg(not(windows))]
        assert!(dark_taskbar());
    }

    use super::*;
    use spotter_core::state::WatchStatus;
    use spotter_engine::view::ReleaseView;

    fn watch(id: &str, unseen: bool, paused: bool, status: WatchStatus) -> WatchSummary {
        WatchSummary {
            id: id.into(),
            name: format!("Tool {id}"),
            source_kind: "github".into(),
            source_label: "GitHub".into(),
            source_description: "example.org/tool".into(),
            status,
            paused,
            checking: false,
            latest: Some(ReleaseView {
                version: "1.2.0".into(),
                prerelease: false,
                name: "Tool".into(),
                location: "https://example.org/".into(),
                published_at_ms: None,
                first_seen_ms: 0,
                size: None,
                sha256: None,
            }),
            acknowledged: None,
            installed: None,
            unseen,
            newer_than_installed: None,
            last_check_ms: None,
            next_check_ms: None,
            failures: 0,
            interval_secs: 3600,
        }
    }

    #[test]
    fn the_icon_follows_unseen_releases_and_every_state_has_both_variants() {
        assert_eq!(tray_state(&[]), TrayState::Normal);
        assert_eq!(tray_state(&[watch("a", false, false, WatchStatus::Ok)]), TrayState::Normal);
        assert_eq!(tray_state(&[watch("a", true, false, WatchStatus::Ok)]), TrayState::New);
        assert_eq!(
            tray_state(&[watch("a", true, true, WatchStatus::Ok)]),
            TrayState::Normal,
            "paused watches do not shout"
        );
        for state in [TrayState::Normal, TrayState::New] {
            for dark in [false, true] {
                assert_eq!(&icon_bytes(state, dark)[1..4], b"PNG");
            }
        }
    }

    #[test]
    fn the_tooltip_counts() {
        assert_eq!(tooltip(&[]), "Spotter - no watches yet");
        assert_eq!(
            tooltip(&[watch("a", false, false, WatchStatus::Ok)]),
            "Spotter - 1 watch, nothing new"
        );
        let two = [
            watch("a", true, false, WatchStatus::Ok),
            watch("b", true, false, WatchStatus::NeedsYou("login".into())),
        ];
        assert_eq!(tooltip(&two), "Spotter - 2 new releases, 1 needs you");
    }

    #[test]
    fn the_menu_lists_unseen_releases_and_offers_pause_or_resume() {
        let ws = [watch("a", true, false, WatchStatus::Ok), watch("b", false, false, WatchStatus::Ok)];
        let m = menu_model(&ws, false);
        assert!(m.contains(&MenuEntry::Item("watch:a".into(), "Tool a 1.2.0".into())));
        assert!(!m.iter().any(|e| matches!(e, MenuEntry::Item(id, _) if id == "watch:b")));
        assert!(m.iter().any(|e| matches!(e, MenuEntry::Item(id, _) if id == PAUSE)));
        assert!(menu_model(&ws, true)
            .iter()
            .any(|e| matches!(e, MenuEntry::Item(id, _) if id == RESUME)));
        assert_eq!(watch_of_item("watch:a"), Some("a"));
        assert_eq!(watch_of_item("open"), None);
        let many: Vec<WatchSummary> = (0..12).map(|i| watch(&format!("w{i}"), true, false, WatchStatus::Ok)).collect();
        let m = menu_model(&many, false);
        assert_eq!(
            m.iter()
                .filter(|e| matches!(e, MenuEntry::Item(id, _) if id.starts_with("watch:")))
                .count(),
            MENU_WATCHES
        );
        assert!(m.contains(&MenuEntry::Header("and 4 more".into())));
    }

    #[test]
    fn routes_are_checked_before_the_window_navigates() {
        assert!(valid_route("/watches/ab12-x_y"));
        assert!(valid_route("/"));
        assert!(!valid_route("watches"));
        assert!(!valid_route("//evil.example"));
        assert!(!valid_route("/a?b=c"));
        assert!(!valid_route("/a#b"));
        assert!(!valid_route("/a b"));
        assert!(!valid_route(&format!("/{}", "a".repeat(250))));
    }

    #[test]
    fn the_popup_sits_below_a_menu_bar_icon_and_above_a_taskbar_icon_inside_the_work_area() {
        let work = Rect {
            x: 0,
            y: 24,
            w: 1440,
            h: 876,
        };
        let top_icon = Rect {
            x: 1300,
            y: 0,
            w: 24,
            h: 24,
        };
        let (x, y) = popup_position(top_icon, work, (360, 440));
        assert_eq!(y, 24 + 6, "below the menu bar");
        assert!(x + 360 <= 1440 - 6, "kept on screen: {x}");
        let bottom = Rect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1040,
        };
        let task_icon = Rect {
            x: 1500,
            y: 1040,
            w: 32,
            h: 40,
        };
        let (x, y) = popup_position(task_icon, bottom, (360, 440));
        assert_eq!(x, 1500 + 16 - 180);
        assert_eq!(y, 1040 - 440 - 6, "above the taskbar");
        let left = popup_position(Rect { x: 2, y: 0, w: 20, h: 20 }, work, (360, 440));
        assert_eq!(left.0, 6, "kept inside on the left");
    }

    #[test]
    fn closing_the_last_window_keeps_running_and_quit_exits() {
        assert!(keeps_running(None, true));
        assert!(!keeps_running(Some(0), true));
        assert!(
            !keeps_running(None, false),
            "no tray: the window is the only way in, so closing it exits"
        );
    }

    #[test]
    fn a_click_right_after_the_popup_closed_is_not_a_new_click() {
        let g = PopupGuard::default();
        assert!(!g.just_closed());
        g.closed();
        assert!(g.just_closed());
    }
}
