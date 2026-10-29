//! Windows are never built inside a Tauri plugin hook (`setup`, `on_event`, `on_page_load`, `on_window_ready`,
//! `on_webview_ready`): Tauri holds its plugin store there and building a webview locks it again on the same thread, a
//! deadlock (see [`paddock_commands::plugin_hook`]). The shell marks its hooks with [`plugin_hook_scope`]; a window
//! built inside one fails with [`WindowErrorCode::InsidePluginHook`]. Window work a hook triggers (a Dock click, a file
//! opened from Finder) goes to [`after_plugin_hook`], which runs it on the main thread after the hook, in the order given.

use crate::wire::{WindowError, WindowErrorCode};
use ::tauri::{AppHandle, Manager, Runtime};
use paddock_commands::plugin_hook::INSIDE_PLUGIN_HOOK;
pub use paddock_commands::plugin_hook::{PluginHookScope, in_plugin_hook, plugin_hook_scope};
use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

type Deferred<R> = Box<dyn FnOnce(&AppHandle<R>) + Send>;

/// Window work deferred past a plugin hook, and whether a task is handing it to the main thread. One task at a time
/// drains the queue, and the event loop runs what it is given in order, so the work runs in the order it was deferred.
struct HookQueue<R: Runtime>(Mutex<(VecDeque<Deferred<R>>, bool)>);

/// Runs `f` on the main thread after the plugin hook that calls this has returned (and Tauri has let go of its plugin
/// store), in the order of the calls.
pub fn after_plugin_hook<R: Runtime>(
    app: &AppHandle<R>,
    f: impl FnOnce(&AppHandle<R>) + Send + 'static,
) {
    // A second `manage` (two hooks racing) is refused and changes nothing.
    app.manage(HookQueue::<R>(Mutex::new((VecDeque::new(), false))));
    let Some(queue) = app.try_state::<HookQueue<R>>() else {
        return;
    };
    let start = {
        let mut q = queue.0.lock().unwrap_or_else(PoisonError::into_inner);
        q.0.push_back(Box::new(f));
        !std::mem::replace(&mut q.1, true)
    };
    if start {
        let app = app.clone();
        ::tauri::async_runtime::spawn(async move { drain(&app) });
    }
}

/// Hands the queued work to the main thread, oldest first, until the queue is empty.
fn drain<R: Runtime>(app: &AppHandle<R>) {
    let Some(queue) = app.try_state::<HookQueue<R>>() else {
        return;
    };
    loop {
        let next = {
            let mut q = queue.0.lock().unwrap_or_else(PoisonError::into_inner);
            let next = q.0.pop_front();
            q.1 = next.is_some();
            next
        };
        let Some(f) = next else { return };
        let handle = app.clone();
        logged(app.run_on_main_thread(move || f(&handle)));
    }
}

/// Logs a failed hand-over (the app is shutting down); returns whether it worked.
fn logged<E: std::fmt::Display>(result: Result<(), E>) -> bool {
    if let Err(e) = &result {
        log::warn!(Error = e.to_string().as_str(); "Could not run window work after a plugin hook");
    }
    result.is_ok()
}

/// `Ok` outside a plugin hook; inside one, the error that names the fix (`label` is the window asked for).
pub(crate) fn check_can_build(label: &str) -> Result<(), WindowError> {
    if !in_plugin_hook() {
        return Ok(());
    }
    Err(WindowError::new(
        WindowErrorCode::InsidePluginHook,
        format!("the window \"{label}\" {INSIDE_PLUGIN_HOOK}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_a_scope_a_window_is_refused_with_its_own_code() {
        assert!(check_can_build("main").is_ok());
        let scope = plugin_hook_scope();
        let e = check_can_build("main").unwrap_err();
        assert_eq!(e.code, WindowErrorCode::InsidePluginHook);
        assert!(
            e.message.contains("tauri::Builder::setup") && e.message.contains("\"main\""),
            "{}",
            e.message
        );
        drop(scope);
        assert!(check_can_build("main").is_ok());
    }

    #[test]
    fn a_failed_hand_over_is_logged() {
        assert!(logged::<String>(Ok(())));
        assert!(!logged(Err("the event loop is gone")));
    }
}
