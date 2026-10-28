//! Spotter's part of the tray (Windows notification area, macOS menu bar, Linux status notifier): what the icon shows
//! (unseen releases), its tooltip and its menu. The icon, the menu drawing, the click rules, the popover window
//! (`index.html#/tray`) and what closing the last window does are the shell's (`paddock-window`); without a tray the
//! shell quits when the last window closes.

use paddock_window::tauri::{create_tray, refresh_tray, NativeTray, PopoverSpec, Popovers, Tray};
use paddock_window::tray::{disabled, item, TrayEntry, TrayView};
use spotter_engine::view::WatchSummary;
use tauri::image::Image;
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// The main window's label (`tauri.conf.json`).
pub const MAIN: &str = "main";
/// The event that asks the main window to show a route.
pub const NAVIGATE_EVENT: &str = "spotter://navigate";

const CHECK_ALL: &str = "check-all";
const PAUSE: &str = "pause";
const RESUME: &str = "resume";
const WATCH_PREFIX: &str = "watch:";

/// The popover's route in the UI (`main.ts` boots the tray window for it).
pub const POPOVER_ROUTE: &str = "index.html#/tray";
/// How many unseen releases the right-click menu lists.
const MENU_WATCHES: usize = 8;

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
    m.push(MenuEntry::Item(paddock_window::tray::SHOW_ID.into(), "Open Spotter".into()));
    m.push(MenuEntry::Item(paddock_window::tray::QUIT_ID.into(), "Quit Spotter".into()));
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

/// The menu as the shell draws it.
pub fn view(watches: &[WatchSummary], paused: bool) -> TrayView {
    let menu = menu_model(watches, paused)
        .into_iter()
        .map(|e| match e {
            MenuEntry::Header(t) => disabled(&t),
            MenuEntry::Separator => TrayEntry::Separator,
            MenuEntry::Item(id, label) => item(id, &label),
        })
        .collect();
    let icon = match tray_state(watches) {
        TrayState::Normal => "normal",
        TrayState::New => "new",
    };
    TrayView::new(&tooltip(watches), menu, icon)
}

/// The icon image for a state name of [`view`].
fn image(name: &str) -> Option<Image<'static>> {
    let state = if name == "new" { TrayState::New } else { TrayState::Normal };
    Image::from_bytes(icon_bytes(state, dark_taskbar())).ok()
}

/// The shell's tray with Spotter's model, and the popover. Call from the app's set-up, before the main window. Returns
/// whether the icon exists (Linux without a status-notifier host has none).
pub fn create<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.manage(Popovers::new().popover(None, PopoverSpec::new(POPOVER_ROUTE, "Spotter")));
    let native = NativeTray::new(image).menu_on_left_click(false);
    let tray = Tray::new(native, |app| {
        let (watches, paused) = crate::lifecycle::snapshot(app);
        view(&watches, paused)
    })
    .popover(true)
    .on_action(|app, id| crate::lifecycle::on_menu(app, id));
    create_tray(app, tray)
}

/// Redraws the tray after the watches changed (only when the model differs).
pub fn update<R: Runtime>(app: &AppHandle<R>) {
    refresh_tray(app);
}

/// Shows and focuses the main window (rebuilt when it was closed). `route` (already validated) is shown: the window
/// is told through an event, and the route also waits for a window that was just built and cannot listen yet.
pub fn show_main<R: Runtime>(app: &AppHandle<R>, route: Option<&str>) {
    if let Some(r) = route {
        if let Some(state) = app.try_state::<crate::state::Shared>() {
            *state.pending_route.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(r.to_string());
        }
    }
    if let Err(e) = paddock_window::tauri::reveal(app, MAIN) {
        crate::applog::write(&format!("the main window could not be opened: {}", e.message));
        return;
    }
    if let Some(r) = route {
        let _ = app.emit_to(MAIN, NAVIGATE_EVENT, r.to_string());
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
                product: None,
                channel: String::new(),
                fields: Vec::new(),
                artifacts: Vec::new(),
                updated_in_place: false,
                upgrade_command: None,
            }),
            acknowledged: None,
            installed: None,
            unseen,
            newer_than_installed: None,
            last_check_ms: None,
            next_check_ms: None,
            failures: 0,
            schedule_text: "Every hour".into(),
            product_count: 0,
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
}
