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
use std::collections::HashMap;
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
    painted: Mutex<HashMap<String, bool>>,
    /// Per item: what was last sent and how many frames were drawn (the tooltip changes rarely, a picture every second).
    drawn: Mutex<HashMap<String, Drawn>>,
    /// Per item: how many pictures in a row were refused (the log tells the first, then a count, then the recovery).
    refused: Mutex<HashMap<String, u32>>,
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
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, QUIT_ID, "Quit Dyno", true, None::<&str>)?)?;
    Ok(menu)
}

/// The Keep awake entries changed: every icon's menu is rebuilt (`None`: the menu has none).
pub fn set_power_menu(app: &AppHandle<Wry>, menu: Option<&crate::power_menu::Menu>) {
    if app.try_state::<TrayState>().is_none() {
        app.manage(TrayState::default());
    }
    let state = app.state::<TrayState>();
    *lock(&state.power) = menu.cloned();
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
    fn build(main: &MainToken, app: &AppHandle<Wry>, spec: &ItemSpec) -> tauri::Result<NativeIcon> {
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
            .tooltip("Dyno")
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
                        logging::info(&format!("could not open the popover: {}", e.message));
                    }
                }
            })
            .build(app)
            .inspect_err(|e| logging::info(&format!("tray icon {}: could not be created: {e}", tray_id(&spec.id))))?;
        Ok(NativeIcon {
            icon,
            _not_send: std::marker::PhantomData,
        })
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

    fn set_tooltip(&self, main: &MainToken, text: &str) {
        main.assert_main();
        let _ = self.icon.set_tooltip(Some(text.to_string()));
    }

    /// Gives the icon its stable OS identity (ADR 0028) and returns what the log says about it. Windows took its GUID when the
    /// icon was built (the provider registered in [`apply`]); macOS gets its autosave name here; Linux's id is the tray id.
    fn identity(&self, main: &MainToken, item: &str) -> String {
        main.assert_main();
        #[cfg(target_os = "macos")]
        {
            let name = tray_identity::autosave_name(item);
            let for_closure = name.clone();
            let set = self.icon.with_inner_tray_icon(move |t| match t.ns_status_item() {
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
        #[cfg(windows)]
        {
            let guid = tray_identity::guid_text(tray_identity::guid_for(item));
            match self.icon.with_inner_tray_icon(|t| t.guid_status()) {
                Ok(status) => format!("GUID {guid}: {status:?}"),
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
}

/// The native operations [`apply`] needs: [`NativeOps`] over real icons, a fake in the tests.
pub trait IconOps {
    /// Makes the icon of `spec`, starting with `initial`.
    fn make(&mut self, spec: &ItemSpec, initial: Option<&Frame>) -> Result<(), String>;
    /// Removes the icon of item `id`.
    fn remove(&mut self, id: &str);
    /// Sets the picture of item `id` when `picture`, and its tooltip when `tooltip` (never called with neither).
    fn draw(&mut self, id: &str, frame: &Frame, picture: bool, tooltip: bool) -> Result<(), String>;
    /// Copies the OS's saved position of item `id` aside (macOS).
    fn save_position(&mut self, id: &str);
}

/// [`IconOps`] over the real icons; only the main thread can make one (it holds a [`MainToken`]).
pub struct NativeOps<'a> {
    main: &'a MainToken,
    app: &'a AppHandle<Wry>,
}

impl IconOps for NativeOps<'_> {
    fn make(&mut self, spec: &ItemSpec, initial: Option<&Frame>) -> Result<(), String> {
        let icon = NativeIcon::build(self.main, self.app, spec).map_err(|e| e.to_string())?;
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
                // AppKit may forget the position of a removed status item: keep a copy.
                ops.save_position(id);
                ops.remove(id);
                lock(&state.items).remove(id);
                lock(&state.painted).remove(id);
                lock(&state.drawn).remove(id);
                lock(&state.refused).remove(id);
            }
            for spec in &plan.create {
                ops.make(spec, initial.iter().find(|(id, _)| id == &spec.id).map(|(_, f)| f))?;
                lock(&state.items).insert(spec.id.clone(), spec.clone());
            }
            Ok(())
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
    }
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
    let (cmd, handle) = (
        TrayCommand::Sync {
            specs: specs.to_vec(),
            initial: initial.to_vec(),
        },
        app.clone(),
    );
    on_main(app, move |main| {
        #[cfg(windows)]
        tray_icon::set_guid_provider(tray_identity::guid_for_tray_id);
        let state = state_of(&handle);
        apply(&mut NativeOps { main, app: &handle }, &state, cmd)
    })?
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
    }

    impl FakeOps {
        fn here(&self) {
            assert_eq!(std::thread::current().id(), self.owner, "a native tray call off the owner thread");
        }
    }

    impl IconOps for FakeOps {
        fn make(&mut self, spec: &ItemSpec, initial: Option<&Frame>) -> Result<(), String> {
            self.here();
            assert!(initial.is_some(), "every new icon starts with its unavailable frame");
            assert!(self.live.insert(spec.id.clone()), "{} made twice", spec.id);
            self.made += 1;
            Ok(())
        }
        fn remove(&mut self, id: &str) {
            self.here();
            assert!(self.live.remove(id), "{id} removed but not there");
            self.removed += 1;
        }
        fn draw(&mut self, id: &str, _: &Frame, picture: bool, tooltip: bool) -> Result<(), String> {
            self.here();
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
    fn a_bulk_change_32_to_6_to_32_goes_through_one_owner_thread_and_stays_consistent() {
        let (tx, rx) = mpsc::channel::<TrayCommand>();
        // The "main thread": the only one that owns icons; the test thread plays the agent link and only sends messages.
        let owner = std::thread::spawn(move || {
            let state = TrayState::default();
            let mut ops = FakeOps {
                owner: std::thread::current().id(),
                live: BTreeSet::new(),
                made: 0,
                removed: 0,
                drawn: 0,
                pictures: 0,
                saved: 0,
            };
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
        let mut ops = FakeOps {
            owner: std::thread::current().id(),
            live: BTreeSet::new(),
            made: 0,
            removed: 0,
            drawn: 0,
            pictures: 0,
            saved: 0,
        };
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

    // A frame with nothing new returns before drawing; the every-30th position save must still happen on its count.
    #[test]
    fn the_position_is_saved_on_its_count_even_when_nothing_is_drawn() {
        let state = TrayState::default();
        let mut ops = FakeOps {
            owner: std::thread::current().id(),
            live: BTreeSet::new(),
            made: 0,
            removed: 0,
            drawn: 0,
            pictures: 0,
            saved: 0,
        };
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
        assert_eq!(v.menu.len(), 2);
        assert!(paddock_window::tray::validate(&v.menu).is_ok());
    }
}
