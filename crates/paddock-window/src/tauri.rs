//! The Tauri adapter of `paddock-window` (feature `tauri`): the managed [`Windows`] state with the app's settings and
//! hooks, building windows with native feel and remembered bounds, the window events, single instance and deep links,
//! the bounded exit, the tray and the tray popover, and the commands of [`COMMANDS`](crate::COMMANDS).
//!
//! ```ignore
//! let launch = paddock_window::launch::parse_args(std::env::args().skip(1), cwd.as_deref(), &["pitwall"]);
//! let mut builder = tauri::Builder::default();
//! if paddock_window::launch::wants_single_instance(&launch, safe_mode_env) {
//!     // First plugin: a second launch hands over and exits before anything else runs.
//!     builder = builder.plugin(tauri_plugin_single_instance::init(paddock_window::tauri::second_instance));
//! }
//! builder
//!     .setup(move |app| {
//!         app.manage(paddock_window::tauri::Windows::builder(WindowConfig::new("Pitwall").schemes(&["pitwall"]))
//!             .prefs(settings.clone())                      // impl Prefs: the close setting, "remember"
//!             .bounds_file(data_dir.join("window-state.json"))
//!             .teardown("server client", |app| client(app).disconnect())
//!             .build());
//!         paddock_window::tauri::create_tray(app.handle(), Tray::new(NativeTray::new(icons), model).popover(spec));
//!         paddock_window::tauri::start(app.handle(), &launch)?;   // the main window (none for `--hidden` with a tray)
//!         Ok(())
//!     })
//!     .on_window_event(|w, e| { paddock_dialogs::tauri::on_window_event(w, e); paddock_window::tauri::on_window_event(w, e) })
//!     .invoke_handler(Guard::new(&paddock_window::COMMANDS).wrap(paddock_window::tauri::invoke_handler()))
//!     .build(context)?
//!     .run(|app, event| paddock_window::tauri::on_run_event(app, &event));
//! ```
//!
//! Window getters wait for the main thread, so window work runs in async commands, in window events or through
//! `run_on_main_thread`; nothing here blocks the main thread on other work (the teardown steps run on threads of their
//! own, bounded).

mod commands;
mod instance;
mod lifecycle;
mod manager;
mod native;
#[cfg(feature = "tray")]
mod native_tray;
mod plugin_hook;
mod popover;
mod tray;

pub use commands::{
    app_quit, invoke_handler, tray_close, tray_fit_height, window_close, window_focus, window_new,
    window_resolve_close, window_resolve_open, window_set_title, window_take_pending, windows_list,
};
pub use instance::{open_from_outside, opened, queue_open, reopen, second_instance};
pub use lifecycle::{arm_exit_watchdog, exit, exit_requested};
pub use manager::{
    ExitClock, FixedPrefs, Prefs, WindowConfig, Windows, WindowsBuilder, changed, close_requested,
    create_main, destroyed, exit_with, list, on_window_event, open_new, quit, reveal, start,
    target,
};
pub use native::on_run_event;
#[cfg(feature = "tray")]
pub use native_tray::NativeTray;
pub use plugin_hook::{PluginHookScope, after_plugin_hook, in_plugin_hook, plugin_hook_scope};
pub use popover::{PopoverSpec, Popovers, close_popover, fit_popover, toggle_popover};
pub use tray::{
    HeadlessTray, Tray, TrayBackend, create_tray, icon_clicked, menu_chosen, refresh_tray,
    set_tray_available, tray_available,
};

#[cfg(test)]
mod tests;
