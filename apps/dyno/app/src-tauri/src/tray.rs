//! The tray / menu-bar items (Windows notification area, macOS menu bar, Linux appindicator): one tray icon per item
//! (`tray_model` says what each draws), each with the same menu (Open, Quit) and a left click that opens the shell's
//! tray popover (`paddock_window`: the `#/tray` page, placed, fitted and closed by the shell). The shell's tray model
//! has one icon with a named picture; Dyno draws live pictures on several icons, so the icons are Dyno's and only their
//! menu and clicks go through the shell ([`DynoTray`] is the shell's backend for them).
//! Before the first update, and while the agent is away, the static template icons from `icons/tray` stand in.

use crate::logging;
#[cfg(any(target_os = "macos", windows))]
use crate::tray_identity;
use crate::tray_look::Look;
use crate::tray_model::{plan_sync, Frame, ItemSpec};
use paddock_window::popover::Rect;
use paddock_window::tauri::{PopoverSpec, Popovers, Tray, TrayBackend};
use paddock_window::tray::{item, TrayView, QUIT_ID, SHOW_ID};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
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
    /// The Keep awake entries every icon's menu carries (none: the menu is Open and Quit only).
    power: Mutex<Option<crate::power_menu::Menu>>,
    /// The Wake submenu every icon's menu carries (none: no computers saved or Wake-on-LAN off).
    wol: Mutex<Option<crate::wol_menu::Menu>>,
    painted: Mutex<HashMap<String, bool>>,
    /// Per item: what was last sent and how many frames were drawn (the tooltip changes rarely, a picture every second).
    drawn: Mutex<HashMap<String, Drawn>>,
    /// Per item: how many pictures in a row were refused (the log tells the first, then a count, then the recovery).
    refused: Mutex<HashMap<String, u32>>,
    /// Every icon the app wants (its spec and the frame it starts with), whether or not the OS took it yet: what [`verify`] adds
    /// again when an icon is missing or not shown (the login start, when the taskbar or menu bar is not ready for the first add).
    wanted: Mutex<BTreeMap<String, (ItemSpec, Option<Frame>)>>,
    /// Whether the follow-up checks of [`schedule_verify`] have been started.
    verify_started: AtomicBool,
    /// The agent keeps measuring after a plain Quit (a login service supervises it): the words of Quit in every icon's menu.
    keeps_measuring: AtomicBool,
}

/// What one item last showed: the tooltip and the picture sent, and how many frames were drawn.
#[derive(Default)]
struct Drawn {
    tooltip: String,
    look: Option<Look>,
    count: u64,
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
    let power = app.try_state::<TrayState>().and_then(|s| lock(&s.power).clone());
    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(app, SHOW_ID, "Open Dyno", true, None::<&str>)?)?;
    if let Some(power) = power {
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        if let Some(header) = &power.countdown_header {
            menu.append(&MenuItem::with_id(app, "dy.awake.idle-header", header, false, None::<&str>)?)?;
            for e in &power.countdown {
                menu.append(&MenuItem::with_id(app, &e.id, &e.text, e.enabled, None::<&str>)?)?;
            }
        }
        if let Some(header) = &power.header {
            menu.append(&MenuItem::with_id(app, "dy.awake.header", header, false, None::<&str>)?)?;
        }
        if !power.entries.is_empty() {
            let sub = Submenu::with_id(app, "dy.awake", "Keep awake", true)?;
            for e in &power.entries {
                sub.append(&CheckMenuItem::with_id(app, &e.id, &e.text, e.enabled, e.checked, None::<&str>)?)?;
            }
            sub.append(&PredefinedMenuItem::separator(app)?)?;
            sub.append(&CheckMenuItem::with_id(
                app,
                &power.display.id,
                &power.display.text,
                power.display.enabled,
                power.display.checked,
                None::<&str>,
            )?)?;
            for (id, title, entries) in [
                ("dy.awake.busy", "While busy", &power.busy),
                ("dy.awake.prog", "Until a program quits", &power.programs),
            ] {
                let inner = Submenu::with_id(app, id, title, !entries.is_empty())?;
                for e in entries {
                    inner.append(&MenuItem::with_id(app, &e.id, &e.text, e.enabled, None::<&str>)?)?;
                }
                sub.append(&inner)?;
            }
            menu.append(&sub)?;
        }
    }
    let wol = app.try_state::<TrayState>().and_then(|s| lock(&s.wol).clone());
    if let Some(wol) = wol {
        let sub = Submenu::with_id(app, "dy.wol", "Wake device", true)?;
        for e in &wol.entries {
            sub.append(&MenuItem::with_id(app, &e.id, &e.text, e.enabled, None::<&str>)?)?;
        }
        menu.append(&sub)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    let keeps = app
        .try_state::<TrayState>()
        .is_some_and(|s| s.keeps_measuring.load(Ordering::Relaxed));
    menu.append(&MenuItem::with_id(
        app,
        QUIT_ID,
        crate::quit::quit_label(keeps),
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        crate::quit::QUIT_STOP_ID,
        crate::quit::QUIT_STOP_LABEL,
        true,
        None::<&str>,
    )?)?;
    Ok(menu)
}

/// Whether the agent keeps measuring after a plain Quit changed: every icon's menu says so (rebuilt only when it changed).
pub fn set_keeps_measuring(app: &AppHandle<Wry>, keeps: bool) {
    if app.try_state::<TrayState>().is_none() {
        app.manage(TrayState::default());
    }
    if app.state::<TrayState>().keeps_measuring.swap(keeps, Ordering::Relaxed) != keeps {
        rebuild_menus(app);
    }
}

/// The Keep awake entries changed: every icon's menu is rebuilt (`None`: the menu has none).
pub fn set_power_menu(app: &AppHandle<Wry>, menu: Option<&crate::power_menu::Menu>) {
    if app.try_state::<TrayState>().is_none() {
        app.manage(TrayState::default());
    }
    let state = app.state::<TrayState>();
    *lock(&state.power) = menu.cloned();
    rebuild_menus(app);
}

/// The Wake submenu changed: every icon's menu is rebuilt (`None`: the menu has none).
pub fn set_wol_menu(app: &AppHandle<Wry>, menu: Option<&crate::wol_menu::Menu>) {
    if app.try_state::<TrayState>().is_none() {
        app.manage(TrayState::default());
    }
    let state = app.state::<TrayState>();
    *lock(&state.wol) = menu.cloned();
    rebuild_menus(app);
}

fn rebuild_menus(app: &AppHandle<Wry>) {
    let state = app.state::<TrayState>();
    let ids: Vec<String> = lock(&state.items).keys().cloned().collect();
    for id in ids {
        let Some(tray) = app.tray_by_id(&tray_id(&id)) else { continue };
        match build_menu(app) {
            Ok(m) => {
                if let Err(e) = tray.set_menu(Some(m)) {
                    logging::info(&format!("tray icon {}: the menu could not be updated: {e}", tray_id(&id)));
                }
            }
            Err(e) => logging::info(&format!("tray icon {}: the menu could not be built: {e}", tray_id(&id))),
        }
    }
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

/// Proof that the code runs on the main thread. AppKit (`NSStatusItem`) and the shell's tray must be created, changed and dropped
/// there: a `TrayIcon` handle is reference-counted without atomics, so even cloning or dropping one elsewhere races the main thread,
/// and the last drop off the main thread calls `NSStatusItem _uninstall` there and kills the app with SIGTRAP (both macOS VM
/// crashes). The only way to get a token is [`on_main`] (or [`dispatch`]), and it is not `Send`, so nothing that takes
/// `&MainToken` is reachable from another thread. In debug builds on macOS making a token asserts the thread, so a mistake is a
/// Rust panic with a stack, not an AppKit trap.
///
/// The guard: a `MainToken` must never become `Send` (this fails to compile if it does).
///
/// ```compile_fail
/// fn needs_send<T: Send>() {}
/// needs_send::<dyno_app_lib::tray::MainToken>();
/// ```
pub struct MainToken {
    _not_send: std::marker::PhantomData<*const ()>,
}

impl MainToken {
    fn here() -> MainToken {
        let token = MainToken {
            _not_send: std::marker::PhantomData,
        };
        token.assert_main();
        token
    }

    /// Debug builds on macOS: panics (with a stack) if this is not the main thread.
    fn assert_main(&self) {
        #[cfg(all(target_os = "macos", not(test)))]
        debug_assert!(objc2::MainThreadMarker::new().is_some(), "tray code must run on the main thread");
    }
}

/// Runs `f` on the main thread (inline when already there) and returns its result. Errors when the main thread cannot be reached.
pub fn on_main<R: Runtime, T: Send + 'static>(app: &AppHandle<R>, f: impl FnOnce(&MainToken) -> T + Send + 'static) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f(&MainToken::here()));
    })
    .map_err(|e| format!("could not reach the main thread: {e}"))?;
    rx.recv().map_err(|_| "the main thread dropped the tray work".to_string())
}

/// Sends `f` to the main thread and does not wait (the live pictures: one per item per tick must not block the agent link).
pub fn dispatch<R: Runtime>(app: &AppHandle<R>, f: impl FnOnce(&MainToken) + Send + 'static) {
    if let Err(e) = app.run_on_main_thread(move || f(&MainToken::here())) {
        logging::info(&format!("could not reach the main thread for the tray: {e}"));
    }
}

/// The one handle to a native tray icon. It is not `Send` and every method takes a [`MainToken`], so the agent link's thread can
/// neither hold nor call one: it sends [`TrayCommand`]s and the main thread applies them. Dropping it checks the thread too.
///
/// ```compile_fail
/// fn needs_send<T: Send>() {}
/// needs_send::<dyno_app_lib::tray::NativeIcon>();
/// ```
pub struct NativeIcon {
    icon: tauri::tray::TrayIcon<Wry>,
    _not_send: std::marker::PhantomData<*const ()>,
}

impl Drop for NativeIcon {
    fn drop(&mut self) {
        MainToken::here();
    }
}

impl NativeIcon {
    /// The icon of item `id`, when there is one.
    fn find(main: &MainToken, app: &AppHandle<Wry>, id: &str) -> Option<NativeIcon> {
        main.assert_main();
        app.tray_by_id(&tray_id(id)).map(|icon| NativeIcon {
            icon,
            _not_send: std::marker::PhantomData,
        })
    }

    /// Builds the icon of one item (static image first; the item's final picture and identity follow).
    fn build(main: &MainToken, app: &AppHandle<Wry>, spec: &ItemSpec, tooltip: &str) -> tauri::Result<NativeIcon> {
        main.assert_main();
        let menu = build_menu(app)?;
        let image = Image::from_bytes(static_icon_bytes(dark_taskbar()))
            .inspect_err(|e| logging::info(&format!("tray icon {}: the static image could not be read: {e}", tray_id(&spec.id))))?;
        logging::info(&format!(
            "tray icon {}: static image set, {}x{} px (dark taskbar: {})",
            tray_id(&spec.id),
            image.width(),
            image.height(),
            dark_taskbar()
        ));
        #[cfg(target_os = "macos")]
        crate::tray_position::restore(&tray_identity::autosave_name(&spec.id));
        let icon = TrayIconBuilder::with_id(tray_id(&spec.id))
            .tooltip(tooltip)
            .icon(image)
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
                        logging::warn(&format!("could not open the popover: {}", e.message));
                    }
                }
            })
            .build(app)
            .inspect_err(|e| logging::warn(&format!("tray icon {}: could not be created: {e}", tray_id(&spec.id))))?;
        Ok(NativeIcon {
            icon,
            _not_send: std::marker::PhantomData,
        })
    }

    /// Whether the OS holds this icon: Windows, the shell accepted the add (Explorer's notification area was ready); macOS, the status
    /// item's button sits in a visible window of the menu bar; elsewhere the call that made it is all there is to know.
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    fn shown(&self, id: &str) -> Shown {
        #[cfg(windows)]
        {
            match self.icon.with_inner_tray_icon(|t| t.is_added()) {
                Ok(true) => Shown::Yes,
                Ok(false) => Shown::No("the shell refused the add (Explorer's taskbar is not ready)".to_string()),
                Err(e) => Shown::No(format!("the icon could not be asked: {e}")),
            }
        }
        #[cfg(target_os = "macos")]
        {
            use crate::tray_shown::{judge, Verdict};
            // The real test: an on-screen Control Center window named after the item's autosave name.
            let name = tray_identity::autosave_name(id);
            let Some(cc) = crate::tray_platform::control_center_pid() else {
                return Shown::No("ControlCenter is not running".to_string());
            };
            match crate::tray_platform::on_screen_windows() {
                Ok(windows) => match judge(&windows, cc, &name) {
                    Verdict::Shown => Shown::Yes,
                    Verdict::NotShown { control_center_windows } => Shown::No(format!(
                        "no on-screen Control Center window is named {name} ({control_center_windows} Control Center window(s); {} on screen)",
                        crate::tray_shown::owners_summary(&windows)
                    )),
                    Verdict::NamesUnreadable => self.own_window_shown(),
                },
                Err(e) => Shown::No(format!("the window list could not be read: {e}")),
            }
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            Shown::Yes
        }
    }

    /// macOS, when the window server does not name Control Center's windows: the status item's own window is visible, not occluded
    /// and sized (the test before the window list was readable).
    #[cfg(target_os = "macos")]
    fn own_window_shown(&self) -> Shown {
        let asked = self.icon.with_inner_tray_icon(|t| {
            let Some(mtm) = objc2::MainThreadMarker::new() else {
                return Shown::No("not on the main thread".to_string());
            };
            let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
            let window = t.ns_status_item().and_then(|item| item.button(mtm)).and_then(|b| b.window());
            let Some(w) = window else {
                return Shown::No(format!(
                    "the status item has no window in the menu bar (app active {})",
                    app.isActive()
                ));
            };
            let (frame, occluded) = (
                w.frame(),
                !w.occlusionState().contains(objc2_app_kit::NSWindowOcclusionState::Visible),
            );
            if w.isVisible() && !occluded && frame.size.width > 0.0 {
                Shown::Yes
            } else {
                Shown::No(format!(
                    "own window visible {}, occluded {occluded}, frame {:.0},{:.0} {:.0}x{:.0}",
                    w.isVisible(),
                    frame.origin.x,
                    frame.origin.y,
                    frame.size.width,
                    frame.size.height
                ))
            }
        });
        asked.unwrap_or_else(|e| Shown::No(format!("the icon could not be asked: {e}")))
    }

    /// Facts for the log about this icon (see [`IconOps::detail`]).
    fn detail(&self, id: &str) -> String {
        #[cfg(target_os = "macos")]
        {
            let window = match self.own_window_shown() {
                Shown::Yes => "its own window is visible".to_string(),
                Shown::No(why) => format!("its own window: {why}"),
            };
            let name = tray_identity::autosave_name(id);
            let cc = crate::tray_platform::control_center_pid().unwrap_or(-1);
            let named = crate::tray_platform::on_screen_windows().map_or_else(
                |e| format!("window list unreadable ({e})"),
                |w| match crate::tray_shown::judge(&w, cc, &name) {
                    crate::tray_shown::Verdict::Shown => "on-screen Control Center window named after it: yes".to_string(),
                    crate::tray_shown::Verdict::NotShown { control_center_windows } => {
                        format!("on-screen Control Center window named after it: no ({control_center_windows} Control Center windows)")
                    }
                    crate::tray_shown::Verdict::NamesUnreadable => "Control Center's window names are not readable".to_string(),
                },
            );
            format!("{named}; {window}")
        }
        #[cfg(windows)]
        {
            let _ = id;
            self.icon
                .with_inner_tray_icon(|t| {
                    let (n, last) = t.taskbar_restarts();
                    format!(
                        "shell holds the icon {}; TaskbarCreated seen {n} time(s), last re-add outcome {}; steps: {}",
                        t.is_added(),
                        last.map_or_else(|| "none".to_string(), |s| format!("{s:?}")),
                        t.recovery_trace()
                    )
                })
                .unwrap_or_else(|e| format!("the icon could not be asked: {e}"))
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            let _ = id;
            String::new()
        }
    }

    /// Removes the icon of item `id` and drops the handle here, on the main thread.
    fn remove(main: &MainToken, app: &AppHandle<Wry>, id: &str) {
        main.assert_main();
        drop(app.remove_tray_by_id(&tray_id(id)));
    }

    /// Shows `look`: macOS gets both representations in one `setImage` (the patched tray-icon), the others one image.
    fn set_picture(&self, main: &MainToken, look: &Look) -> Result<(), String> {
        main.assert_main();
        #[cfg(target_os = "macos")]
        {
            let reps: Vec<(Vec<u8>, u32, u32)> = look.pictures.iter().map(|p| (p.rgba.clone(), p.width, p.height)).collect();
            let size = (f64::from(look.width), f64::from(look.height));
            let template = look.template;
            self.icon
                .with_inner_tray_icon(move |t| {
                    let refs: Vec<(&[u8], u32, u32)> = reps.iter().map(|(b, w, h)| (b.as_slice(), *w, *h)).collect();
                    t.set_icon_reps(&refs, size, template).map_err(|e| e.to_string())
                })
                .map_err(|e| e.to_string())?
        }
        #[cfg(not(target_os = "macos"))]
        {
            let p = look.pictures.first().ok_or("no picture")?;
            self.icon
                .set_icon(Some(Image::new_owned(p.rgba.clone(), p.width, p.height)))
                .map_err(|e| e.to_string())
        }
    }

    /// Sets the tip (Windows: at most 127 UTF-16 units, cut by `item_tooltip`; macOS: the status item button's tool tip; Linux:
    /// the status notifier's tool tip, which many hosts do not show). A refusal is logged: the tip is sent only when its text
    /// changed, so this is one line per change, not per tick.
    fn set_tooltip(&self, main: &MainToken, text: &str) {
        main.assert_main();
        if let Err(e) = self.icon.set_tooltip(Some(text.to_string())) {
            logging::info(&format!(
                "tray icon {}: the tooltip \"{text}\" was refused: {e}",
                self.icon.id().as_ref()
            ));
        }
    }

    /// Gives the icon its stable OS identity (ADR 0028) and returns what the log says about it. Windows took its GUID when the
    /// icon was built (the provider registered in [`apply`]); macOS gets its autosave name here; Linux's id is the tray id.
    fn identity(&self, main: &MainToken, item: &str) -> String {
        main.assert_main();
        #[cfg(target_os = "macos")]
        {
            // The name was given when the status item was made (the vendored tray-icon's autosave-name provider); read it back.
            let name = tray_identity::autosave_name(item);
            let read = self
                .icon
                .with_inner_tray_icon(|t| t.ns_status_item().map(|i| i.autosaveName().to_string()));
            match read {
                Ok(Some(set)) if set == name => format!("autosave name {name}"),
                Ok(Some(other)) => format!("autosave name {name} NOT set: the status item says \"{other}\""),
                Ok(None) => format!("autosave name {name} NOT set: the icon has no status item"),
                Err(e) => format!("autosave name {name} NOT set: {e}"),
            }
        }
        #[cfg(windows)]
        {
            let guid = tray_identity::guid_text(tray_identity::guid_for(item));
            match self.icon.with_inner_tray_icon(|t| (t.guid_status(), t.recovery_trace())) {
                Ok((status, steps)) => format!("GUID {guid}: {status:?} (steps: {steps})"),
                Err(e) => format!("GUID {guid}: status unknown ({e})"),
            }
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            format!("status notifier id {}", tray_id(item))
        }
    }
}

/// What the agent link's thread asks of the tray. Owned data only: the main thread applies it.
#[derive(Debug, Clone)]
pub enum TrayCommand {
    /// Make the icons match `specs`; `initial` are the frames the new icons start with.
    Sync {
        /// The wanted items.
        specs: Vec<ItemSpec>,
        /// Unavailable frames by item id.
        initial: Vec<(String, Frame)>,
    },
    /// Draw a frame on an item.
    Paint {
        /// The item.
        item: String,
        /// What to draw.
        frame: Frame,
    },
    /// The agent is away: show these frames and send every tooltip again with the next live picture.
    Disconnected {
        /// The unavailable frame of every item.
        frames: Vec<(String, Frame)>,
    },
    /// Log, a few seconds after the icons were made, what the OS shows of each (the evidence a diagnostic bundle needs when icons are
    /// missing); changes nothing.
    Diagnose,
    /// Check that the OS shows every wanted icon, and add again each one that it does not (follow-up number `pass` after the start).
    Verify {
        /// Which follow-up check this is, from 0.
        pass: u32,
    },
}

/// Whether the OS shows an icon now (the add can be accepted by the call and still show nothing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shown {
    /// The OS holds the icon and shows it.
    Yes,
    /// It does not, and why (for the log).
    No(String),
}

/// The pauses before each follow-up check of the icons after the start: the first add can fail or be dropped while the login is
/// still building the taskbar (Windows), the menu bar (macOS) or waiting for a status-notifier host (Linux), and no event says
/// when it is ready. The checks cover about three minutes.
const VERIFY_DELAYS_SECS: [u64; 9] = [1, 2, 4, 8, 15, 30, 30, 45, 60];

/// The pause before follow-up check number `pass` (counted from 0), or none when the checks are over.
pub fn verify_delay(pass: u32) -> Option<std::time::Duration> {
    VERIFY_DELAYS_SECS.get(pass as usize).map(|s| std::time::Duration::from_secs(*s))
}

/// The native operations [`apply`] needs: [`NativeOps`] over real icons, a fake in the tests.
pub trait IconOps {
    /// Makes the icon of `spec`, starting with `initial`; `tooltip` is the text the OS registers with the icon (never a generic
    /// app name, or the OS records it as the icon's initial tooltip).
    fn make(&mut self, spec: &ItemSpec, initial: Option<&Frame>, tooltip: &str) -> Result<(), String>;
    /// Removes the icon of item `id`.
    fn remove(&mut self, id: &str);
    /// Sets the picture of item `id` when `picture`, and its tooltip when `tooltip` (never called with neither).
    fn draw(&mut self, id: &str, frame: &Frame, picture: bool, tooltip: bool) -> Result<(), String>;
    /// Copies the OS's saved position of item `id` aside (macOS).
    fn save_position(&mut self, id: &str);
    /// Whether the OS shows the icon of item `id` now (a made icon the OS dropped, or never took, is not shown).
    fn shown(&mut self, id: &str) -> Shown;
    /// Whether a check may remove and make again an icon that is not shown. Windows and Linux: yes (the shell or host dropped the
    /// add). macOS: never. The check reads the local window, which can be wrong while the menu bar or ControlCenter is not ready, and
    /// a re-make was followed by one crash of the app in a test run.
    fn remakes_unshown(&self) -> bool;
    /// Platform facts about item `id` for the log (macOS: the window server's view of the item; Windows: whether the shell holds it
    /// and the `TaskbarCreated` re-adds).
    fn detail(&mut self, id: &str) -> String;
    /// Platform facts for the log that do not belong to one item (macOS: ControlCenter's pid and start time, whether Dyno is
    /// active; Windows: Explorer's pid and start time).
    fn context(&mut self) -> String;
    /// What it usually means when none of the icons is shown a few seconds after they were made, if the platform has such a cause.
    fn off_screen_hint(&self) -> Option<&'static str>;
}

/// [`IconOps`] over the real icons; only the main thread can make one (it holds a [`MainToken`]).
pub struct NativeOps<'a> {
    main: &'a MainToken,
    app: &'a AppHandle<Wry>,
}

impl IconOps for NativeOps<'_> {
    fn make(&mut self, spec: &ItemSpec, initial: Option<&Frame>, tooltip: &str) -> Result<(), String> {
        let icon = NativeIcon::build(self.main, self.app, spec, tooltip).map_err(|e| e.to_string())?;
        // Start at the item's final width, as unavailable, so nothing resizes when the first live picture arrives.
        if let Some(frame) = initial {
            if let Err(e) = icon.set_picture(self.main, &frame.look) {
                logging::info(&format!("tray icon {}: the first picture was refused: {e}", tray_id(&spec.id)));
            }
            icon.set_tooltip(self.main, &frame.tooltip);
        }
        let identity = icon.identity(self.main, &spec.id);
        logging::info(&format!(
            "tray icon {} created (metric {}, {identity}; on Windows 11 a new icon may sit in the overflow until it is pinned in Settings > Personalization > Taskbar)",
            tray_id(&spec.id),
            crate::tray_model::metric_name(&spec.metric)
        ));
        Ok(())
    }

    fn remove(&mut self, id: &str) {
        NativeIcon::remove(self.main, self.app, id);
    }

    fn draw(&mut self, id: &str, frame: &Frame, picture: bool, tooltip: bool) -> Result<(), String> {
        let Some(icon) = NativeIcon::find(self.main, self.app, id) else {
            return Ok(());
        };
        let set = if picture {
            icon.set_picture(self.main, &frame.look)
        } else {
            Ok(())
        };
        if tooltip {
            icon.set_tooltip(self.main, &frame.tooltip);
        }
        set
    }

    fn save_position(&mut self, id: &str) {
        #[cfg(target_os = "macos")]
        crate::tray_position::save(&tray_identity::autosave_name(id));
        #[cfg(not(target_os = "macos"))]
        let _ = id;
    }

    fn shown(&mut self, id: &str) -> Shown {
        let Some(icon) = NativeIcon::find(self.main, self.app, id) else {
            return Shown::No("the app has no icon of that id".to_string());
        };
        // Linux: an icon the desktop has no StatusNotifier host to show is not shown, whatever the add said.
        #[cfg(target_os = "linux")]
        if !crate::tray_host::current(self.app).is_shown() {
            return Shown::No("the desktop has no StatusNotifier host".to_string());
        }
        icon.shown(id)
    }

    fn remakes_unshown(&self) -> bool {
        !cfg!(target_os = "macos")
    }

    fn detail(&mut self, id: &str) -> String {
        NativeIcon::find(self.main, self.app, id).map_or_else(|| "no icon".to_string(), |icon| icon.detail(id))
    }

    fn context(&mut self) -> String {
        #[cfg(any(target_os = "macos", windows))]
        {
            crate::tray_platform::context()
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            String::new()
        }
    }

    fn off_screen_hint(&self) -> Option<&'static str> {
        if cfg!(target_os = "macos") {
            Some("none of Dyno's menu-bar items is on screen: the lines above say whether the session has a menu bar and whether ControlCenter is running")
        } else if cfg!(windows) {
            Some("Explorer shows none of Dyno's icons: they may sit in the overflow, or Explorer was not ready (see the GUID and TaskbarCreated lines)")
        } else {
            None
        }
    }
}

/// Applies one command to the icons and the bookkeeping, in the order commands arrive. This is the only place that decides what
/// the tray does; it runs on the main thread, and in the tests on a thread standing in for it.
pub fn apply<O: IconOps>(ops: &mut O, state: &TrayState, cmd: TrayCommand) -> Result<(), String> {
    match cmd {
        TrayCommand::Sync { specs, initial } => {
            let plan = plan_sync(&lock(&state.items), &specs);
            logging::info(&format!(
                "tray sync: {} item(s) wanted, removing {:?}, making {:?}",
                specs.len(),
                plan.remove,
                plan.create.iter().map(|s| s.id.as_str()).collect::<Vec<_>>()
            ));
            for id in &plan.remove {
                logging::info(&format!("tray icon {}: removing it (it is no longer in the layout)", tray_id(id)));
                // AppKit may forget the position of a removed status item: keep a copy.
                ops.save_position(id);
                ops.remove(id);
                forget(state, id);
                lock(&state.wanted).remove(id);
            }
            let mut first_error = None;
            for spec in &plan.create {
                let first = initial.iter().find(|(id, _)| id == &spec.id).map(|(_, f)| f);
                lock(&state.wanted).insert(spec.id.clone(), (spec.clone(), first.cloned()));
                // One icon the OS refuses must not stop the others, and is tried again by `Verify`.
                if let Err(e) = make_one(ops, state, spec, first) {
                    logging::info(&format!(
                        "tray icon {}: could not be added: {e}; it is tried again in the follow-up checks",
                        tray_id(&spec.id)
                    ));
                    first_error.get_or_insert(e);
                }
            }
            if !plan.create.is_empty() {
                let context = ops.context();
                logging::info(&format!("tray sync: {} icon(s) made; {context}", plan.create.len()));
            }
            first_error.map_or(Ok(()), Err)
        }
        TrayCommand::Paint { item, frame } => {
            paint_one(ops, state, &item, &frame);
            Ok(())
        }
        TrayCommand::Disconnected { frames } => {
            for (id, frame) in &frames {
                // The next picture's tooltip is always sent again.
                lock(&state.drawn).remove(id);
                paint_one(ops, state, id, frame);
            }
            Ok(())
        }
        TrayCommand::Verify { pass } => {
            verify(ops, state, pass);
            Ok(())
        }
        TrayCommand::Diagnose => {
            diagnose(ops, state);
            Ok(())
        }
    }
}

/// Makes the icon of `spec` and records it as existing.
fn make_one<O: IconOps>(ops: &mut O, state: &TrayState, spec: &ItemSpec, first: Option<&Frame>) -> Result<(), String> {
    let tooltip = first_tooltip(spec, first);
    ops.make(spec, first, &tooltip)?;
    lock(&state.items).insert(spec.id.clone(), spec.clone());
    Ok(())
}

/// Forgets what was drawn on the icon of `id`, so the next frame draws it whole.
fn forget(state: &TrayState, id: &str) {
    lock(&state.items).remove(id);
    lock(&state.painted).remove(id);
    lock(&state.drawn).remove(id);
    lock(&state.refused).remove(id);
}

/// One follow-up check: every wanted icon that was never made, or that the OS does not show, is made again. Returns how many are
/// still missing. Logs each add outcome, and a summary line on the first and last check and whenever something was wrong.
fn verify<O: IconOps>(ops: &mut O, state: &TrayState, pass: u32) -> usize {
    let wanted: Vec<(String, (ItemSpec, Option<Frame>))> = lock(&state.wanted).iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let (mut shown, mut repaired, mut missing) = (0, 0, 0);
    for (id, (spec, first)) in &wanted {
        let present = lock(&state.items).contains_key(id);
        let why = if present {
            match ops.shown(id) {
                Shown::Yes => None,
                Shown::No(why) => Some(why),
            }
        } else {
            Some("it was never added".to_string())
        };
        let Some(why) = why else {
            shown += 1;
            continue;
        };
        if present && !ops.remakes_unshown() {
            // macOS: the check can be wrong while the menu bar or ControlCenter is not ready (it called a good login "not shown"), and a
            // re-make was followed by one crash of the app in a test run. The check reports and leaves the icon.
            missing += 1;
            logging::info(&format!(
                "tray check #{pass}: icon {} is not shown ({why}); not made again on this platform",
                tray_id(id)
            ));
            continue;
        }
        logging::info(&format!(
            "tray check #{pass}: icon {} is not shown ({why}); adding it again",
            tray_id(id)
        ));
        if present {
            logging::info(&format!("tray icon {}: removing it to add it again", tray_id(id)));
            ops.remove(id);
            forget(state, id);
        }
        match make_one(ops, state, spec, first.as_ref()) {
            Ok(()) => {
                repaired += 1;
                logging::info(&format!("tray check #{pass}: icon {} added again", tray_id(id)));
            }
            Err(e) => {
                missing += 1;
                logging::info(&format!("tray check #{pass}: icon {} could not be added again: {e}", tray_id(id)));
            }
        }
    }
    if pass == 0 || repaired + missing > 0 || verify_delay(pass + 1).is_none() {
        logging::info(&format!(
            "tray check #{pass}: {} wanted, {shown} shown, {repaired} added again, {missing} still missing",
            wanted.len()
        ));
    }
    missing
}

/// How long after the icons were made [`diagnose`] looks at what the OS shows.
const DIAGNOSE_AFTER_SECS: u64 = 5;

/// Logs what the OS shows of every wanted icon, with the platform's facts, and one summary line when none is shown (ER-27).
fn diagnose<O: IconOps>(ops: &mut O, state: &TrayState) {
    let wanted: Vec<String> = lock(&state.wanted).keys().cloned().collect();
    let context = ops.context();
    let mut shown = 0;
    for id in &wanted {
        let is_shown = ops.shown(id);
        let detail = ops.detail(id);
        let (word, why) = match &is_shown {
            Shown::Yes => ("shown", String::new()),
            Shown::No(why) => ("NOT shown", format!(" ({why})")),
        };
        shown += usize::from(is_shown == Shown::Yes);
        logging::info(&format!("tray diagnose: icon {} {word}{why}; {detail}", tray_id(id)));
    }
    logging::info(&format!(
        "tray diagnose: {shown} of {} icon(s) shown {DIAGNOSE_AFTER_SECS} s after they were made; {context}",
        wanted.len()
    ));
    if shown == 0 && !wanted.is_empty() {
        if let Some(hint) = ops.off_screen_hint() {
            logging::info(&format!("tray diagnose: {hint}"));
        }
    }
}

/// Starts the follow-up checks once per process: a thread that waits [`verify_delay`] before each check and sends it to the main
/// thread. The first add at login can fail without a signal (Windows' taskbar, the menu bar, a status-notifier host not up yet).
fn schedule_verify(app: &AppHandle<Wry>) {
    let state = state_of(app);
    if state.verify_started.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let diagnose_app = app.clone();
    let diagnosing = std::thread::Builder::new().name("tray-diagnose".into()).spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(DIAGNOSE_AFTER_SECS));
        send(&diagnose_app, TrayCommand::Diagnose);
    });
    if let Err(e) = diagnosing {
        logging::info(&format!("tray diagnose: the thread could not start: {e}"));
    }
    let spawned = std::thread::Builder::new().name("tray-checks".into()).spawn(move || {
        let mut pass = 0;
        while let Some(wait) = verify_delay(pass) {
            std::thread::sleep(wait);
            send(&app, TrayCommand::Verify { pass });
            pass += 1;
        }
    });
    if let Err(e) = spawned {
        logging::info(&format!("tray checks: the thread could not start: {e}"));
    }
}

/// The tooltip a new icon is registered with: the first frame's, or the metric's name when there is none yet.
fn first_tooltip(spec: &ItemSpec, initial: Option<&Frame>) -> String {
    initial.map_or_else(|| crate::tray_model::metric_name(&spec.metric), |f| f.tooltip.clone())
}

fn paint_one<O: IconOps>(ops: &mut O, state: &TrayState, item: &str, frame: &Frame) {
    if !lock(&state.items).contains_key(item) {
        return;
    }
    let (w, h) = (frame.look.width, frame.look.height);
    let (paint, count) = {
        let mut map = lock(&state.drawn);
        let entry = map.entry(item.to_string()).or_default();
        entry.count += 1;
        let picture_changed = entry.look.as_ref() != Some(&frame.look);
        let paint = crate::tray_model::paint_ops(&entry.tooltip, &frame.tooltip, picture_changed, entry.count);
        if paint.tooltip {
            entry.tooltip = frame.tooltip.clone();
        }
        if paint.image_sets > 0 {
            entry.look = Some(frame.look.clone());
        }
        (paint, entry.count)
    };
    // The saved position is refreshed on its own count, whether or not this frame draws anything.
    if paint.save_position {
        ops.save_position(item);
    }
    // Each picture set redraws the status item (in the window server on macOS): a tooltip-only change sets none, and a frame
    // with nothing new touches the icon not at all.
    if paint.image_sets == 0 && !paint.tooltip {
        return;
    }
    let set = ops.draw(item, frame, paint.image_sets > 0, paint.tooltip);
    if set.is_err() {
        // A refused picture is not on the icon: the next frame sets it again even if it is the same.
        if let Some(entry) = lock(&state.drawn).get_mut(item) {
            entry.look = None;
        }
    }
    // One line per frame, at debug: the flow of image sets per tick (a flicker shows as more than one set per tick).
    logging::debug(&format!(
        "tray icon {}: frame #{count}: {} picture set(s) ({} representation(s), {w}x{h}, template {}), tooltip {}",
        tray_id(item),
        paint.image_sets,
        frame.look.pictures.len(),
        frame.look.template,
        if paint.tooltip { "sent" } else { "unchanged" }
    ));
    let first = lock(&state.painted).insert(item.to_string(), true).is_none();
    // A line per second per item would flood the log: the first refusal in a row is told with its cause, then every 60th with the
    // count, and the recovery with how many were lost; the first picture of an item is told once.
    match &set {
        Err(e) => {
            let n = {
                let mut map = lock(&state.refused);
                let n = map.entry(item.to_string()).or_default();
                *n += 1;
                *n
            };
            if n == 1 {
                logging::info(&format!("tray icon {}: the drawn image ({w}x{h}) was refused: {e}", tray_id(item)));
            } else if n % 60 == 0 {
                logging::info(&format!(
                    "tray icon {}: {n} drawn images in a row were refused (last: {e})",
                    tray_id(item)
                ));
            }
        }
        Ok(()) => {
            if let Some(n) = lock(&state.refused).remove(item).filter(|n| *n > 1) {
                logging::info(&format!(
                    "tray icon {}: drawing works again after {n} refused images",
                    tray_id(item)
                ));
            }
            if first {
                logging::info(&format!("tray icon {}: first drawn image set, {w}x{h}", tray_id(item)));
            }
        }
    }
}

fn state_of(app: &AppHandle<Wry>) -> tauri::State<'_, TrayState> {
    if app.try_state::<TrayState>().is_none() {
        app.manage(TrayState::default());
    }
    app.state::<TrayState>()
}

/// Makes the tray icons match `specs` (waits for the main thread, which does the work; see [`MainToken`]). Called only when the
/// items changed. Returns the first error.
pub fn sync_items(app: &AppHandle<Wry>, specs: &[ItemSpec], initial: &[(String, Frame)]) -> Result<(), String> {
    #[cfg(windows)]
    crate::tray_start::log_once();
    let (cmd, handle) = (
        TrayCommand::Sync {
            specs: specs.to_vec(),
            initial: initial.to_vec(),
        },
        app.clone(),
    );
    let applied = on_main(app, move |main| {
        #[cfg(windows)]
        tray_icon::set_guid_provider(tray_identity::guid_for_tray_id);
        #[cfg(target_os = "macos")]
        tray_icon::set_autosave_name_provider(tray_identity::autosave_name_for_tray_id);
        let state = state_of(&handle);
        apply(&mut NativeOps { main, app: &handle }, &state, cmd)
    })?;
    // Whatever the first add did, the checks follow: a refused or dropped add is made again when the OS is ready.
    schedule_verify(app);
    applied
}

/// The pass number of a check asked for by the tray host appearing (not one of the timed follow-ups, whose passes count from 0).
const HOST_CHECK_PASS: u32 = 1000;

/// The desktop's tray host appeared (Linux: an extension turned on, a panel that started late): check the icons now, so each one
/// the host was not there to show is added again by the same check the login start uses ([`TrayCommand::Verify`]). Returns at once.
pub fn verify_now(app: &AppHandle<Wry>) {
    send(app, TrayCommand::Verify { pass: HOST_CHECK_PASS });
}

/// Draws a frame on an item. Returns at once; the main thread does it.
pub fn paint(app: &AppHandle<Wry>, item: &str, frame: &Frame) {
    send(
        app,
        TrayCommand::Paint {
            item: item.to_string(),
            frame: frame.clone(),
        },
    );
}

/// While the agent is away every item shows its unavailable frame (a dimmed dash at the same width): a frozen bar would look
/// like a measurement.
pub fn show_disconnected(app: &AppHandle<Wry>, frames: &[(String, Frame)]) {
    send(app, TrayCommand::Disconnected { frames: frames.to_vec() });
}

fn send(app: &AppHandle<Wry>, cmd: TrayCommand) {
    let handle = app.clone();
    dispatch(app, move |main| {
        let Some(state) = handle.try_state::<TrayState>() else { return };
        let _ = apply(&mut NativeOps { main, app: &handle }, &state, cmd);
    });
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
        sync_items(app, &self.items, &initial).map_err(|e| e.to_string())?;
        // The icons exist; whether the desktop shows them is another matter (Linux: a StatusNotifier host). With none the shell
        // is told there is no tray (a hidden start opens the window), and told again when the host appears.
        if crate::tray_host::start(app) {
            Ok(())
        } else {
            Err("the desktop shows no tray icons (no StatusNotifier host)".to_string())
        }
    }

    fn update(&self, _app: &AppHandle<Wry>, _view: &TrayView) -> Result<(), String> {
        Ok(())
    }
}

/// What the shell draws for the tray's menu: Open and Quit, both the shell's own actions.
pub fn view() -> TrayView {
    TrayView::new(
        "Dyno",
        vec![
            item(SHOW_ID, "Open Dyno"),
            item(QUIT_ID, "Quit Dyno"),
            item(crate::quit::QUIT_STOP_ID, crate::quit::QUIT_STOP_LABEL),
        ],
        "static",
    )
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

    use crate::tray_model::{Env, Platform, TrayItems};
    use dyno_core::widgets::Metric;
    use std::collections::BTreeSet;
    use std::sync::mpsc;
    use std::thread::ThreadId;

    /// Stands in for the native icons and refuses to be touched from any thread but its owner's, as AppKit does.
    struct FakeOps {
        owner: ThreadId,
        live: BTreeSet<String>,
        made: usize,
        removed: usize,
        drawn: usize,
        pictures: usize,
        saved: usize,
        /// The tooltip the platform call got, per icon, in order.
        tips: Vec<(String, String)>,
        /// How many of the next `make` calls the OS refuses (the taskbar or menu bar is not ready).
        refuse_makes: usize,
        /// Icons the OS took but does not show (a dropped add).
        not_shown: BTreeSet<String>,
        /// Whether the platform lets a check re-make an icon that is not shown (false stands in for macOS).
        remake: bool,
    }

    impl FakeOps {
        fn new() -> FakeOps {
            FakeOps {
                owner: std::thread::current().id(),
                live: BTreeSet::new(),
                made: 0,
                removed: 0,
                drawn: 0,
                pictures: 0,
                saved: 0,
                tips: Vec::new(),
                refuse_makes: 0,
                not_shown: BTreeSet::new(),
                remake: true,
            }
        }

        fn here(&self) {
            assert_eq!(std::thread::current().id(), self.owner, "a native tray call off the owner thread");
        }
    }

    impl IconOps for FakeOps {
        fn make(&mut self, spec: &ItemSpec, initial: Option<&Frame>, tooltip: &str) -> Result<(), String> {
            self.here();
            if self.refuse_makes > 0 {
                self.refuse_makes -= 1;
                return Err("the taskbar is not ready".to_string());
            }
            assert!(initial.is_some(), "every new icon starts with its unavailable frame");
            self.tips.push((spec.id.clone(), tooltip.to_string()));
            assert!(self.live.insert(spec.id.clone()), "{} made twice", spec.id);
            self.made += 1;
            Ok(())
        }
        fn remove(&mut self, id: &str) {
            self.here();
            assert!(self.live.remove(id), "{id} removed but not there");
            self.not_shown.remove(id);
            self.removed += 1;
        }
        fn draw(&mut self, id: &str, frame: &Frame, picture: bool, tooltip: bool) -> Result<(), String> {
            self.here();
            if tooltip {
                self.tips.push((id.to_string(), frame.tooltip.clone()));
            }
            assert!(self.live.contains(id), "{id} drawn but not there");
            assert!(picture || tooltip, "{id} drawn with nothing to draw");
            self.drawn += 1;
            self.pictures += usize::from(picture);
            Ok(())
        }
        fn save_position(&mut self, _: &str) {
            self.here();
            self.saved += 1;
        }
        fn shown(&mut self, id: &str) -> Shown {
            self.here();
            if self.live.contains(id) && !self.not_shown.contains(id) {
                Shown::Yes
            } else {
                Shown::No("fake".to_string())
            }
        }
        fn remakes_unshown(&self) -> bool {
            self.remake
        }
        fn detail(&mut self, _: &str) -> String {
            "fake detail".to_string()
        }
        fn context(&mut self) -> String {
            "fake context".to_string()
        }
        fn off_screen_hint(&self) -> Option<&'static str> {
            Some("fake hint")
        }
    }

    fn items(n: usize) -> Vec<ItemSpec> {
        (0..n)
            .map(|i| ItemSpec::new(&format!("cpu-{i}"), Metric::CpuTotal, crate::tray_model::Glyph::Number))
            .collect()
    }

    fn frames(specs: &[ItemSpec]) -> Vec<(String, Frame)> {
        let env = Env {
            platform: Platform::Pixels,
            px: 16,
            text: std::sync::Arc::new(dyno_core::tray::text::BoxText),
        };
        TrayItems::new(specs.to_vec(), env).unavailable_frames(true)
    }

    #[test]
    fn an_icon_is_created_with_its_metric_tooltip_never_the_app_name() {
        let state = TrayState::default();
        let specs = items(2);
        let initial = frames(&specs);
        let mut ops = FakeOps::new();
        apply(
            &mut ops,
            &state,
            TrayCommand::Sync {
                specs,
                initial: initial.clone(),
            },
        )
        .unwrap();
        assert_eq!(ops.tips.len(), 2);
        for ((id, tip), (fid, frame)) in ops.tips.iter().zip(&initial) {
            assert_eq!(id, fid);
            assert_eq!(tip, &frame.tooltip, "{id}: registered with the first frame's tooltip");
            assert!(tip.starts_with("CPU") && tip != "Dyno", "{id}: {tip}");
        }
    }

    fn sync(ops: &mut FakeOps, state: &TrayState, n: usize) -> Result<(), String> {
        let specs = items(n);
        let initial = frames(&specs);
        apply(ops, state, TrayCommand::Sync { specs, initial })
    }

    #[test]
    fn a_refused_add_at_login_does_not_stop_the_other_icons_and_is_added_again_by_the_next_check() {
        let state = TrayState::default();
        let mut ops = FakeOps::new();
        ops.refuse_makes = 1;
        // The taskbar or menu bar is not ready: the first icon is refused, the second is taken.
        assert!(sync(&mut ops, &state, 2).is_err());
        assert_eq!(ops.live.len(), 1, "the refused icon must not stop the next one");
        apply(&mut ops, &state, TrayCommand::Verify { pass: 0 }).unwrap();
        assert_eq!(ops.live, BTreeSet::from(["cpu-0".to_string(), "cpu-1".to_string()]));
        // A later check finds nothing to do.
        let made = ops.made;
        apply(&mut ops, &state, TrayCommand::Verify { pass: 1 }).unwrap();
        assert_eq!(ops.made, made);
    }

    #[test]
    fn a_check_keeps_trying_while_the_os_keeps_refusing() {
        let state = TrayState::default();
        let mut ops = FakeOps::new();
        ops.refuse_makes = 3;
        assert!(sync(&mut ops, &state, 1).is_err());
        for pass in 0..2 {
            apply(&mut ops, &state, TrayCommand::Verify { pass }).unwrap();
            assert!(ops.live.is_empty(), "pass {pass}: still refused");
        }
        apply(&mut ops, &state, TrayCommand::Verify { pass: 2 }).unwrap();
        assert_eq!(ops.live.len(), 1);
    }

    #[test]
    fn an_icon_the_os_took_but_does_not_show_is_removed_and_added_again_and_drawn_afresh() {
        let state = TrayState::default();
        let mut ops = FakeOps::new();
        sync(&mut ops, &state, 2).unwrap();
        let specs = items(2);
        let frame = frames(&specs).remove(0).1;
        apply(
            &mut ops,
            &state,
            TrayCommand::Paint {
                item: "cpu-0".into(),
                frame: frame.clone(),
            },
        )
        .unwrap();
        let drawn = ops.drawn;
        ops.not_shown.insert("cpu-0".to_string());
        apply(&mut ops, &state, TrayCommand::Verify { pass: 0 }).unwrap();
        assert_eq!((ops.removed, ops.made), (1, 3), "only the hidden icon is made again");
        assert!(ops.not_shown.is_empty());
        // The new icon has nothing on it yet: the same frame is drawn again.
        apply(
            &mut ops,
            &state,
            TrayCommand::Paint {
                item: "cpu-0".into(),
                frame,
            },
        )
        .unwrap();
        assert!(ops.drawn > drawn, "the re-made icon gets its picture");
    }

    #[test]
    fn verify_on_macos_never_removes_an_icon() {
        let state = TrayState::default();
        let mut ops = FakeOps::new();
        ops.remake = false;
        sync(&mut ops, &state, 4).unwrap();
        // The shown test says No for every item at every check (as when the menu bar is not ready).
        ops.not_shown.extend(ops.live.clone());
        for pass in 0..3 {
            apply(&mut ops, &state, TrayCommand::Verify { pass }).unwrap();
        }
        assert_eq!(
            ops.removed, 0,
            "a re-make is followed by a crash in one test run, and the check can be wrong; macOS only reports"
        );
        assert_eq!(ops.made, 4, "no icon is made again");
        assert_eq!(ops.live.len(), 4);
    }

    #[test]
    fn every_removal_of_an_icon_is_logged_with_its_reason_before_it_happens() {
        let _ = crate::logging::testlog::lines(); // installs the capture before anything is logged
        let state = TrayState::default();
        let mut ops = FakeOps::new();
        let gone = ItemSpec::new("removal-log-probe", Metric::CpuTotal, crate::tray_model::Glyph::Number);
        let kept = ItemSpec::new("removal-log-kept", Metric::CpuTotal, crate::tray_model::Glyph::Number);
        let all = vec![gone.clone(), kept.clone()];
        apply(
            &mut ops,
            &state,
            TrayCommand::Sync {
                specs: all.clone(),
                initial: frames(&all),
            },
        )
        .unwrap();
        // The layout drops one item: its icon is removed, and the log says so (and why) first.
        apply(
            &mut ops,
            &state,
            TrayCommand::Sync {
                specs: vec![kept.clone()],
                initial: frames(&[kept]),
            },
        )
        .unwrap();
        let lines = crate::logging::testlog::lines();
        let at = lines
            .iter()
            .position(|l| l.contains("dyno-removal-log-probe") && l.contains("removing"))
            .expect("a removal line");
        assert!(lines[at].contains("no longer in the layout"), "{}", lines[at]);
        // Windows and Linux re-make an icon the OS does not show: that removal is logged too.
        let mut ops = FakeOps::new();
        let state = TrayState::default();
        let one = vec![ItemSpec::new(
            "removal-log-remake",
            Metric::CpuTotal,
            crate::tray_model::Glyph::Number,
        )];
        apply(
            &mut ops,
            &state,
            TrayCommand::Sync {
                specs: one.clone(),
                initial: frames(&one),
            },
        )
        .unwrap();
        ops.not_shown.insert("removal-log-remake".into());
        apply(&mut ops, &state, TrayCommand::Verify { pass: 0 }).unwrap();
        let lines = crate::logging::testlog::lines();
        assert!(
            lines
                .iter()
                .any(|l| l.contains("dyno-removal-log-remake") && l.contains("removing it to add it again")),
            "{lines:?}"
        );
    }

    #[test]
    fn the_tray_host_appearing_adds_again_the_icons_it_was_not_there_to_show_through_the_same_check() {
        let state = TrayState::default();
        let mut ops = FakeOps::new();
        sync(&mut ops, &state, 2).unwrap();
        // No host: the desktop shows neither icon.
        ops.not_shown.extend(["cpu-0".to_string(), "cpu-1".to_string()]);
        // The host appears: `verify_now` sends this check, which is not one of the timed follow-ups.
        assert!(verify_delay(HOST_CHECK_PASS).is_none());
        apply(&mut ops, &state, TrayCommand::Verify { pass: HOST_CHECK_PASS }).unwrap();
        assert_eq!((ops.removed, ops.made), (2, 4));
        assert!(ops.not_shown.is_empty());
        // Asked again with nothing wrong: nothing is touched.
        apply(&mut ops, &state, TrayCommand::Verify { pass: HOST_CHECK_PASS }).unwrap();
        assert_eq!((ops.removed, ops.made), (2, 4));
    }

    #[test]
    fn a_removed_item_is_not_added_again_by_a_check() {
        let state = TrayState::default();
        let mut ops = FakeOps::new();
        sync(&mut ops, &state, 2).unwrap();
        sync(&mut ops, &state, 1).unwrap();
        apply(&mut ops, &state, TrayCommand::Verify { pass: 0 }).unwrap();
        assert_eq!(ops.live, BTreeSet::from(["cpu-0".to_string()]));
    }

    #[test]
    fn the_follow_up_checks_start_within_two_seconds_and_cover_two_minutes_then_stop() {
        let delays: Vec<_> = (0..).map_while(verify_delay).collect();
        assert!(delays[0] <= std::time::Duration::from_secs(2));
        assert!(delays.iter().sum::<std::time::Duration>() >= std::time::Duration::from_secs(120));
        assert!(delays.windows(2).all(|w| w[0] <= w[1]), "backs off, never speeds up");
        assert!(verify_delay(delays.len() as u32).is_none());
    }

    #[test]
    fn a_bulk_change_32_to_6_to_32_goes_through_one_owner_thread_and_stays_consistent() {
        let (tx, rx) = mpsc::channel::<TrayCommand>();
        // The "main thread": the only one that owns icons; the test thread plays the agent link and only sends messages.
        let owner = std::thread::spawn(move || {
            let state = TrayState::default();
            let mut ops = FakeOps::new();
            for cmd in rx {
                apply(&mut ops, &state, cmd).unwrap();
            }
            let tracked: BTreeSet<String> = lock(&state.items).keys().cloned().collect();
            (ops, tracked)
        });
        let sync = |n: usize| {
            let specs = items(n);
            let initial = frames(&specs);
            tx.send(TrayCommand::Sync { specs, initial }).unwrap();
        };
        let paint_all = |n: usize| {
            for (item, frame) in frames(&items(n)) {
                tx.send(TrayCommand::Paint { item, frame }).unwrap();
            }
        };
        sync(32);
        paint_all(32);
        sync(6);
        // Pictures for the 26 removed items are still in flight: they must be dropped, not drawn on a dead icon.
        paint_all(32);
        sync(32);
        paint_all(32);
        drop(tx);
        let (ops, tracked) = owner.join().unwrap();
        assert_eq!((ops.made, ops.removed), (32 + 26, 26));
        assert_eq!(ops.live.len(), 32);
        assert_eq!(tracked, ops.live, "the bookkeeping and the icons agree");
        // The 6 kept items get the same frames again and are not redrawn; the 26 made again are.
        assert_eq!(
            ops.drawn,
            32 + 26,
            "no picture reached a removed icon, and an unchanged one is not set again"
        );
    }

    // Each picture set redraws the status item, the window server's main cost for an idle Dyno (docs/performance.md
    // 2026-11-03): a frame whose only change is the tooltip sends the tooltip and sets no picture.
    #[test]
    fn a_tooltip_only_change_sets_no_picture() {
        let state = TrayState::default();
        let mut ops = FakeOps::new();
        let specs = items(1);
        let initial = frames(&specs);
        apply(
            &mut ops,
            &state,
            TrayCommand::Sync {
                specs,
                initial: initial.clone(),
            },
        )
        .unwrap();
        let (item, frame) = initial[0].clone();
        apply(
            &mut ops,
            &state,
            TrayCommand::Paint {
                item: item.clone(),
                frame: frame.clone(),
            },
        )
        .unwrap();
        assert_eq!((ops.drawn, ops.pictures), (1, 1));
        let mut retitled = frame;
        retitled.tooltip.push_str(" (again)");
        apply(&mut ops, &state, TrayCommand::Paint { item, frame: retitled }).unwrap();
        assert_eq!(
            (ops.drawn, ops.pictures),
            (2, 1),
            "the tooltip went out, the picture was not set again"
        );
    }

    // The wiring: the text a live frame carries reaches the platform call for that icon (the metric's name and reading, not the app
    // name the icon was built with), and an unchanged text is not sent again.
    #[test]
    fn each_icon_gets_its_own_metric_tooltip_from_the_frame() {
        let state = TrayState::default();
        let mut ops = FakeOps::new();
        let specs = vec![
            ItemSpec::new("cpu", Metric::CpuTotal, crate::tray_model::Glyph::Number),
            ItemSpec::new("mem", Metric::Memory, crate::tray_model::Glyph::Number),
        ];
        let initial = frames(&specs);
        apply(
            &mut ops,
            &state,
            TrayCommand::Sync {
                specs,
                initial: initial.clone(),
            },
        )
        .unwrap();
        for (item, mut frame) in initial {
            frame.tooltip = format!("{} 34%", if item == "cpu" { "CPU" } else { "Memory" });
            for _ in 0..2 {
                apply(
                    &mut ops,
                    &state,
                    TrayCommand::Paint {
                        item: item.clone(),
                        frame: frame.clone(),
                    },
                )
                .unwrap();
            }
        }
        let sent = |id: &str| -> Vec<String> { ops.tips.iter().filter(|(i, _)| i == id).map(|(_, t)| t.clone()).collect() };
        assert_eq!(sent("cpu").last().map(String::as_str), Some("CPU 34%"));
        assert_eq!(sent("mem").last().map(String::as_str), Some("Memory 34%"));
        assert_eq!(
            sent("cpu").len(),
            2,
            "made with the unavailable tip, then one live tip; the repeat is not sent"
        );
        assert!(
            sent("cpu")[0].starts_with("CPU"),
            "the first tip names the metric too: {:?}",
            sent("cpu")
        );
    }

    // A frame with nothing new returns before drawing; the every-30th position save must still happen on its count.
    #[test]
    fn the_position_is_saved_on_its_count_even_when_nothing_is_drawn() {
        let state = TrayState::default();
        let mut ops = FakeOps::new();
        let specs = items(1);
        let initial = frames(&specs);
        apply(
            &mut ops,
            &state,
            TrayCommand::Sync {
                specs,
                initial: initial.clone(),
            },
        )
        .unwrap();
        let (item, frame) = initial[0].clone();
        for _ in 0..30 {
            apply(
                &mut ops,
                &state,
                TrayCommand::Paint {
                    item: item.clone(),
                    frame: frame.clone(),
                },
            )
            .unwrap();
        }
        assert_eq!(ops.drawn, 1, "the same frame is drawn once");
        assert_eq!(ops.saved, 2, "saved on the 1st and the 30th draw");
    }

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
        assert_eq!(v.menu.len(), 3);
        assert!(paddock_window::tray::validate(&v.menu).is_ok());
    }
}
