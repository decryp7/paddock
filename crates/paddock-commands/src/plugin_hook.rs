//! Which threads are inside a Tauri plugin hook. Tauri 2 holds its plugin store (`Mutex<PluginStore>`) while it runs a
//! plugin's `setup`, `on_event`, `on_page_load`, `on_window_ready` and `on_webview_ready`, and building a webview locks
//! that store again (for the plugins' initialisation scripts) on the same thread: a window built inside one of these hooks
//! blocks the thread for good. On the main thread at start that is an app with no window, no menu and no log line.
//!
//! A plugin marks its hooks with [`plugin_hook_scope`]; every Paddock window builder (`paddock-window`'s windows and
//! tray popover, `paddock-dialogs`' dialogs, `paddock-panels`' pop-outs and ghosts) asks [`in_plugin_hook`] first and
//! fails with [`INSIDE_PLUGIN_HOOK`] instead of deadlocking. The mark is per thread (a hook runs on one thread; a window
//! built on another thread waits for the store to be free, which is not a deadlock).

use std::cell::Cell;
use std::marker::PhantomData;

thread_local! {
    /// How many plugin hooks this thread is inside of (nested scopes count).
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Why a window cannot be built here (the window builders' error message, after the window's name).
pub const INSIDE_PLUGIN_HOOK: &str = "cannot be opened inside a plugin's set-up or event hook: Tauri holds its plugin \
     store there and building a webview would wait for it forever. Open windows from tauri::Builder::setup, or after the \
     hook (paddock_window::tauri::after_plugin_hook).";

/// Marks this thread as running a plugin hook until it is dropped. Not `Send`: the mark belongs to the thread that made
/// it, so the guard cannot end on another one.
#[must_use = "the scope ends when the guard is dropped"]
pub struct PluginHookScope(PhantomData<*const ()>);

impl Drop for PluginHookScope {
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// Starts a plugin hook scope on this thread: Paddock's window builders refuse to build inside it.
pub fn plugin_hook_scope() -> PluginHookScope {
    DEPTH.with(|d| d.set(d.get().saturating_add(1)));
    PluginHookScope(PhantomData)
}

/// Whether this thread is inside a plugin hook.
pub fn in_plugin_hook() -> bool {
    DEPTH.with(|d| d.get() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scope_marks_only_its_own_thread_and_ends_when_dropped() {
        assert!(!in_plugin_hook());
        let outer = plugin_hook_scope();
        {
            let _inner = plugin_hook_scope();
            assert!(in_plugin_hook());
        }
        assert!(in_plugin_hook(), "the outer scope still holds");
        assert!(
            std::thread::spawn(|| !in_plugin_hook()).join().unwrap(),
            "another thread is not in the hook"
        );
        drop(outer);
        assert!(!in_plugin_hook());
        assert!(INSIDE_PLUGIN_HOOK.contains("tauri::Builder::setup"));
    }
}
