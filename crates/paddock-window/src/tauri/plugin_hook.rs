//! Windows are never built inside a plugin's hooks. Tauri runs every plugin's `setup` and `on_event` while it holds its
//! plugin store, and building a webview locks that store again (for the plugins' initialisation scripts) on the same
//! thread: a window built there blocks the main thread for good (at start: no window, no menu, no log line). The shell
//! marks its plugin hooks with [`plugin_hook_scope`]; building a window inside one fails at once with a message that
//! names the fix: open windows from `tauri::Builder::setup` (after every plugin is set up), or after the hook returns
//! ([`after_plugin_hook`]).

use crate::wire::{WindowError, WindowErrorCode};
use ::tauri::{AppHandle, Runtime};
use std::cell::Cell;

thread_local! {
    /// How many plugin hooks this thread is inside of (nested scopes count).
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Marks this thread as running a plugin hook until it is dropped (see the module doc).
#[must_use = "the scope ends when the guard is dropped"]
pub struct PluginHookScope(());

impl Drop for PluginHookScope {
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// Starts a plugin hook scope on this thread: windows built inside it fail instead of deadlocking.
pub fn plugin_hook_scope() -> PluginHookScope {
    DEPTH.with(|d| d.set(d.get().saturating_add(1)));
    PluginHookScope(())
}

/// Whether this thread is inside a plugin hook.
pub fn in_plugin_hook() -> bool {
    DEPTH.with(|d| d.get() > 0)
}

/// Runs `f` on the main thread after the plugin hook that calls this has returned (and Tauri has let go of its plugin
/// store): the call is posted from an async task, so it goes through the event loop instead of running at once.
pub fn after_plugin_hook<R: Runtime>(
    app: &AppHandle<R>,
    f: impl FnOnce(&AppHandle<R>) + Send + 'static,
) {
    let app = app.clone();
    ::tauri::async_runtime::spawn(async move {
        let handle = app.clone();
        if let Err(e) = app.run_on_main_thread(move || f(&handle)) {
            log::warn!(Error = e.to_string().as_str(); "Could not run window work after a plugin hook");
        }
    });
}

/// `Ok` outside a plugin hook; inside one, the error that names the fix (`label` is the window asked for).
pub(crate) fn check_can_build(label: &str) -> Result<(), WindowError> {
    if !in_plugin_hook() {
        return Ok(());
    }
    Err(WindowError::new(
        WindowErrorCode::NotReady,
        format!(
            "the window \"{label}\" cannot be opened inside a plugin's set-up or event hook: Tauri holds its plugin \
             store there and building a webview would wait for it forever. Open windows from tauri::Builder::setup \
             instead."
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scope_marks_only_its_own_thread_and_ends_when_dropped() {
        assert!(check_can_build("main").is_ok());
        let outer = plugin_hook_scope();
        {
            let _inner = plugin_hook_scope();
            assert!(in_plugin_hook());
        }
        assert!(in_plugin_hook(), "the outer scope still holds");
        let e = check_can_build("main").unwrap_err();
        assert_eq!(e.code, WindowErrorCode::NotReady);
        assert!(
            e.message.contains("tauri::Builder::setup") && e.message.contains("\"main\""),
            "{}",
            e.message
        );
        assert!(
            std::thread::spawn(|| !in_plugin_hook()).join().unwrap(),
            "another thread is not in the hook"
        );
        drop(outer);
        assert!(!in_plugin_hook());
        assert!(check_can_build("main").is_ok());
    }
}
