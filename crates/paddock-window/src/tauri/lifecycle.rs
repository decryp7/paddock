//! The exit: an exit request that only means "the last window closed" keeps a tray app running; any other one begins
//! quitting and arms the exit watchdog; the exit itself runs the bounded teardown once. Ported from Debrief's
//! `lifecycle.rs` and `lib.rs` (`on_run_event`) and Pitwall's `window.rs`.

#[cfg(target_os = "macos")]
use super::instance::{opened, reopen};
use super::manager::{persist_now, state};
use super::tray::tray_available;
use crate::close::keeps_running;
use crate::exit::{end_stalled_exit, teardown_step};
use ::tauri::{AppHandle, Manager, RunEvent, Runtime};

/// Tauri asks to exit, with `code` for an explicit exit and `None` when the last window closed. Returns whether the
/// exit is refused (the app keeps running in the tray); otherwise quitting begins and the exit watchdog is armed.
pub fn exit_requested<R: Runtime>(app: &AppHandle<R>, code: Option<i32>) -> bool {
    let Ok(windows) = state(app) else {
        return false;
    };
    if keeps_running(code, tray_available(app), windows.exit.quitting()) {
        log::info!("The last window closed; the app keeps running in the tray");
        return true;
    }
    log::info!(Code = code.unwrap_or_default(); "Exit requested");
    windows.exit.begin_quit();
    arm_exit_watchdog(app, code.unwrap_or_default());
    false
}

/// The app exits (`RunEvent::Exit`): the window bounds are saved and the app's teardown steps run in order, each on a
/// thread of its own and bounded, once (the exit watchdog and the normal exit may both get here).
pub fn exit<R: Runtime>(app: &AppHandle<R>) {
    let Ok(windows) = state(app) else { return };
    if !windows.exit.begin_teardown() {
        return;
    }
    let limit = windows.config.step_limit;
    let a = app.clone();
    teardown_step("window bounds", limit, move || persist_now(&a));
    for (name, step) in &windows.steps {
        let (a, step) = (app.clone(), step.clone());
        teardown_step(name, limit, move || step(&a));
    }
    log::info!("Teardown finished");
}

/// Ends a quit that stalls while its windows close: after [`WindowConfig::exit_stall_limit`](super::WindowConfig)
/// without reaching the exit, logs the windows still open, runs the bounded teardown and ends the process with
/// `code`. Armed once.
pub fn arm_exit_watchdog<R: Runtime>(app: &AppHandle<R>, code: i32) {
    let Ok(windows) = state(app) else { return };
    if !windows.exit.arm() {
        return;
    }
    let limit = windows.config.exit_stall_limit;
    let end = windows.end.clone();
    let app = app.clone();
    super::manager::spawn("paddock-exit-watchdog", move || {
        std::thread::sleep(limit);
        let tearing_down = state(&app).is_ok_and(|w| w.exit.tearing_down());
        let open = || app.webview_windows().into_keys().collect::<Vec<_>>();
        end_stalled_exit(tearing_down, open, || exit(&app), |c| end(c), code, limit);
    });
}

/// The run events of the layer (call it from `App::run`): exit requests, the exit, and on macOS files and links opened
/// with the app and the Dock icon while no window is visible.
pub fn on_run_event<R: Runtime>(app: &AppHandle<R>, event: &RunEvent) {
    match event {
        RunEvent::ExitRequested { code, api, .. } if exit_requested(app, *code) => {
            api.prevent_exit()
        }
        RunEvent::Exit => exit(app),
        #[cfg(target_os = "macos")]
        RunEvent::Opened { urls } => opened(app, urls),
        #[cfg(target_os = "macos")]
        RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } => reopen(app),
        _ => {}
    }
}
