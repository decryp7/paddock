//! The Tauri adapter of `paddock-menu` (built with the `tauri` feature): the macOS native menu bar from the UI's model,
//! clicks routed to the focused window.
//!
//! Set up: [`install`] at start (manages [`NativeMenu`], routes clicks, and on macOS replaces Tauri's default menu with
//! the standard one until the UI sends its model), list [`menu_set`] in the command table, and forward window events to
//! [`on_window_event`] (the menu bar follows the focused top-level window):
//!
//! ```ignore
//! paddock_commands::command_table! {
//!     static COMMANDS, macro handler;
//!     Quiet [MAIN | POPOUT] limit 1 * MIB => paddock_menu::tauri::menu_set,
//!     // ... the app's commands
//! }
//! tauri::Builder::default()
//!     .setup(|app| Ok(paddock_menu::tauri::install(app.handle())))
//!     .invoke_handler(Guard::new(&COMMANDS).wrap(handler!()))
//!     .on_window_event(|w, e| paddock_menu::tauri::on_window_event(w, e))
//! ```
//!
//! The UI sends its model once the app is ready and coalesces later changes (`NativeMenuBridge` in
//! `@decryp7/paddock-ui/shell/menu`); this side diffs again, so the same model builds nothing and a changed check mark
//! or enabled state is updated in place ([`Update`]). [`NativeMenu::builds`] counts the builds (a start builds twice:
//! the standard menus, then the UI's). On Windows and Linux `menu_set` validates the model and shows nothing: the UI
//! draws its own menu bar.
//!
//! Command: `menu_set`. Event: [`MENU_EVENT`] (`paddock://menu`, the item id, to the target window only).

use crate::model::{
    AppMenu, Item, MENU_EVENT, MenuApplied, MenuSetError, MenuSetErrorCode, NativeMenuModel, parse,
};
use crate::native::{NativeNode, Predefined, Update, diff, item_ids, native_spec};
use crate::route::{Focus, applies, route};
use ::tauri::menu::{CheckMenuItem, Menu, MenuItem, MenuItemKind, PredefinedMenuItem, Submenu};
use ::tauri::{AppHandle, Emitter, Manager, Runtime, Window, WindowEvent};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

/// The log target of this crate.
const TARGET: &str = "paddock_menu";

impl From<::tauri::Error> for MenuSetError {
    fn from(e: ::tauri::Error) -> Self {
        MenuSetError {
            code: MenuSetErrorCode::Native,
            message: format!("the native menu bar could not be updated: {e}"),
        }
    }
}

/// Where the native menu bar is drawn: Tauri's menus ([`TauriBar`]); a fake in tests.
pub trait NativeBar<R: Runtime>: Send + Sync + 'static {
    /// Shows a menu bar built from `spec`.
    fn build(&self, app: &AppHandle<R>, spec: &[NativeNode]) -> ::tauri::Result<()>;
    /// Updates the check marks and enabled states of the shown items; false when an item is not found (then it is
    /// built anew).
    fn set_states(&self, app: &AppHandle<R>, changed: &[Item]) -> ::tauri::Result<bool>;
}

/// The menu bar of the app ([`AppHandle::set_menu`]).
pub struct TauriBar;

impl<R: Runtime> NativeBar<R> for TauriBar {
    fn build(&self, app: &AppHandle<R>, spec: &[NativeNode]) -> ::tauri::Result<()> {
        let menu = build_menu(app, spec)?;
        #[cfg(target_os = "macos")]
        for (node, kind) in spec
            .iter()
            .filter(|n| matches!(n, NativeNode::Submenu { .. }))
            .zip(menu.items()?)
        {
            use crate::model::MenuRole;
            if let (NativeNode::Submenu { role, .. }, Some(sub)) = (node, kind.as_submenu()) {
                match role {
                    MenuRole::Window => sub.set_as_windows_menu_for_nsapp()?,
                    MenuRole::Help => sub.set_as_help_menu_for_nsapp()?,
                    _ => {}
                }
            }
        }
        app.set_menu(menu).map(drop)
    }

    fn set_states(&self, app: &AppHandle<R>, changed: &[Item]) -> ::tauri::Result<bool> {
        match app.menu() {
            Some(menu) => set_states_in(&menu, changed),
            None => Ok(false),
        }
    }
}

/// Managed state: the spec the menu bar shows, the target window and the build count.
pub struct NativeMenu<R: Runtime> {
    app_name: String,
    native: bool,
    bar: Box<dyn NativeBar<R>>,
    shown: Mutex<Option<Vec<NativeNode>>>,
    focus: Mutex<Focus>,
    builds: AtomicU64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl<R: Runtime> NativeMenu<R> {
    /// The menu state of this platform: a native menu bar on macOS, none elsewhere. `app_name` titles the app menu.
    pub fn new(app_name: impl Into<String>) -> Self {
        Self::with_bar(app_name, cfg!(target_os = "macos"), TauriBar)
    }

    /// The menu state with its menu bar given: `native` false validates models and shows nothing (Windows, Linux).
    pub fn with_bar(app_name: impl Into<String>, native: bool, bar: impl NativeBar<R>) -> Self {
        NativeMenu {
            app_name: app_name.into(),
            native,
            bar: Box::new(bar),
            shown: Mutex::new(None),
            focus: Mutex::new(Focus::new()),
            builds: AtomicU64::new(0),
        }
    }

    /// How often the menu bar was built.
    pub fn builds(&self) -> u64 {
        self.builds.load(Ordering::Relaxed)
    }

    /// Whether this platform shows the native menu bar.
    pub fn is_native(&self) -> bool {
        self.native
    }

    /// The ids of the items the menu bar shows, depth first.
    pub fn shown_ids(&self) -> Vec<String> {
        let shown = lock(&self.shown);
        shown
            .as_deref()
            .map(item_ids)
            .unwrap_or_default()
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    /// The top-level window clicks go to.
    pub fn target(&self) -> Option<String> {
        lock(&self.focus).target().map(str::to_string)
    }

    /// Shows `model`: nothing when unchanged, states in place when only they changed, else a rebuild.
    pub fn apply(&self, app: &AppHandle<R>, model: &AppMenu) -> ::tauri::Result<Update> {
        let spec = native_spec(model, &self.app_name);
        let mut shown = lock(&self.shown);
        let update = diff(shown.as_deref(), &spec);
        let rebuild = match &update {
            Update::Unchanged => false,
            Update::States(changed) => !self.bar.set_states(app, changed)?,
            Update::Rebuild => true,
        };
        if rebuild {
            self.bar.build(app, &spec)?;
            self.builds.fetch_add(1, Ordering::Relaxed);
            log::info!(target: TARGET, Menus = spec.len(), Items = item_ids(&spec).len(); "Native menu bar built with {} menus", spec.len());
        }
        *shown = Some(spec);
        Ok(update)
    }

    /// Where a click on `id` goes (a shown item, to the target window).
    pub fn route(&self, id: &str) -> Option<String> {
        let shown = lock(&self.shown);
        let ids = shown.as_deref().map(item_ids).unwrap_or_default();
        route(id, &ids, lock(&self.focus).target())
    }
}

/// Manages [`NativeMenu`] (titled with the app's product name) unless the app manages its own, routes native menu
/// clicks to the target window, and on macOS shows the standard menus until the UI sends its model.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    if app.try_state::<NativeMenu<R>>().is_none() {
        app.manage(NativeMenu::<R>::new(app.package_info().name.clone()));
    }
    app.on_menu_event(|app, event| {
        menu_chosen(app, event.id().as_ref());
    });
    let state = app.state::<NativeMenu<R>>();
    if state.is_native()
        && let Err(e) = state.apply(app, &AppMenu::default())
    {
        log::warn!(target: TARGET, Error = e.to_string().as_str(); "The native menu bar could not be built");
    }
}

/// Sends a chosen item to the target window only (not its pop-outs or dialogs: it runs the command once). Returns the
/// window it went to; `None` when the id is not one of the shown items or no window is the target.
pub fn menu_chosen<R: Runtime>(app: &AppHandle<R>, id: &str) -> Option<String> {
    let label = app.try_state::<NativeMenu<R>>()?.route(id)?;
    if let Err(e) = app.emit_to(label.as_str(), MENU_EVENT, id) {
        log::warn!(target: TARGET, Window = label.as_str(), Error = e.to_string().as_str(); "A menu click could not be sent");
        return None;
    }
    Some(label)
}

/// Follows the focus of the windows (the menu bar is the focused top-level window's). Forward every window event.
pub fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    let Some(state) = window.try_state::<NativeMenu<R>>() else {
        return;
    };
    match event {
        WindowEvent::Focused(true) => lock(&state.focus).focused(window.label()),
        WindowEvent::Destroyed => lock(&state.focus).destroyed(window.label()),
        _ => {}
    }
}

/// The calling window's menus for the native menu bar (macOS). Elsewhere the model is validated and not shown.
/// The invoke handler of [`COMMANDS`](crate::COMMANDS) (put it behind `paddock_commands::tauri::Guard`).
pub fn invoke_handler<R: Runtime>()
-> impl Fn(::tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    ::tauri::generate_handler![menu_set]
}

#[::tauri::command]
pub async fn menu_set<R: Runtime>(
    app: AppHandle<R>,
    window: Window<R>,
    menu: NativeMenuModel,
) -> Result<MenuApplied, MenuSetError> {
    set(&app, &window, menu)
}

fn set<R: Runtime>(
    app: &AppHandle<R>,
    window: &Window<R>,
    menu: NativeMenuModel,
) -> Result<MenuApplied, MenuSetError> {
    let model = parse(menu)?;
    let state = app
        .try_state::<NativeMenu<R>>()
        .ok_or_else(|| MenuSetError {
            code: MenuSetErrorCode::Native,
            message: "the menus are not set up (paddock_menu::tauri::install was not called)"
                .into(),
        })?;
    if !state.is_native() {
        return Ok(MenuApplied::InApp);
    }
    let focused = window.is_focused().unwrap_or(false);
    if !applies(window.label(), focused, state.target().as_deref()) {
        return Ok(MenuApplied::NotFocused);
    }
    if focused {
        lock(&state.focus).focused(window.label());
    }
    Ok(match state.apply(app, &model)? {
        Update::Unchanged => MenuApplied::Unchanged,
        Update::States(_) => MenuApplied::States,
        Update::Rebuild => MenuApplied::Rebuilt,
    })
}

// ---- Tauri menus from the spec ----

/// The Tauri menu of a spec (top-level submenus only; other nodes at the top are not shown).
pub fn build_menu<R: Runtime>(app: &AppHandle<R>, spec: &[NativeNode]) -> ::tauri::Result<Menu<R>> {
    let menu = Menu::new(app)?;
    for n in spec {
        if let NativeNode::Submenu { label, items, .. } = n {
            menu.append(&submenu(app, label, items)?)?;
        }
    }
    Ok(menu)
}

fn submenu<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    items: &[NativeNode],
) -> ::tauri::Result<Submenu<R>> {
    let sub = Submenu::new(app, label, true)?;
    for n in items {
        match n {
            NativeNode::Item(i) => match i.checked {
                Some(checked) => sub.append(&CheckMenuItem::with_id(
                    app,
                    &i.id,
                    &i.label,
                    i.enabled,
                    checked,
                    i.accelerator.as_deref(),
                )?)?,
                None => sub.append(&MenuItem::with_id(
                    app,
                    &i.id,
                    &i.label,
                    i.enabled,
                    i.accelerator.as_deref(),
                )?)?,
            },
            NativeNode::Header(text) => {
                sub.append(&MenuItem::new(app, text, false, None::<&str>)?)?
            }
            NativeNode::Separator => sub.append(&PredefinedMenuItem::separator(app)?)?,
            NativeNode::Predefined(p) => sub.append(&predefined(app, *p)?)?,
            NativeNode::Submenu { label, items, .. } => sub.append(&submenu(app, label, items)?)?,
        }
    }
    Ok(sub)
}

fn predefined<R: Runtime>(
    app: &AppHandle<R>,
    p: Predefined,
) -> ::tauri::Result<PredefinedMenuItem<R>> {
    use PredefinedMenuItem as M;
    match p {
        Predefined::Services => M::services(app, None),
        Predefined::Hide => M::hide(app, None),
        Predefined::HideOthers => M::hide_others(app, None),
        Predefined::ShowAll => M::show_all(app, None),
        Predefined::Undo => M::undo(app, None),
        Predefined::Redo => M::redo(app, None),
        Predefined::Cut => M::cut(app, None),
        Predefined::Copy => M::copy(app, None),
        Predefined::Paste => M::paste(app, None),
        Predefined::SelectAll => M::select_all(app, None),
        Predefined::Minimize => M::minimize(app, None),
        Predefined::Zoom => M::maximize(app, Some("Zoom")),
    }
}

/// Finds an item by id anywhere in a menu.
fn find<R: Runtime>(
    items: Vec<MenuItemKind<R>>,
    id: &str,
) -> ::tauri::Result<Option<MenuItemKind<R>>> {
    for kind in items {
        if kind.id().as_ref() == id {
            return Ok(Some(kind));
        }
        if let Some(sub) = kind.as_submenu()
            && let Some(found) = find(sub.items()?, id)?
        {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

/// Updates the check marks and enabled states of items of `menu`; false when one of them is not found or is of
/// another kind (a toggle that is a plain item there), which the caller answers with a rebuild.
pub fn set_states_in<R: Runtime>(menu: &Menu<R>, changed: &[Item]) -> ::tauri::Result<bool> {
    for item in changed {
        match (find(menu.items()?, &item.id)?, item.checked) {
            (Some(MenuItemKind::Check(c)), Some(checked)) => {
                c.set_enabled(item.enabled)?;
                c.set_checked(checked)?;
            }
            (Some(MenuItemKind::MenuItem(m)), None) => m.set_enabled(item.enabled)?,
            _ => return Ok(false),
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
