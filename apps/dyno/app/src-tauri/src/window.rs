//! The main window's life: closing destroys it (frees the webview; a hidden WKWebView came back blank), the tray's
//! "Open Dyno" and the macOS Dock icon build it again from the configuration, and only an explicit Quit exits. The
//! platform it runs on is handed to the page before Angular boots.

use tauri::{AppHandle, Manager, Runtime};

/// The main window's label (`tauri.conf.json`).
pub const MAIN: &str = "main";

/// Marks an app whose tray icon could not be created (Linux without an appindicator): there is no way back to a closed
/// window, so closing it must exit.
pub struct NoTray;

/// Whether the app has a tray icon.
pub fn has_tray<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<NoTray>().is_none()
}

/// Whether an exit request leaves the app running: closing the last window does when there is a tray to reopen it from;
/// Quit (an explicit exit code) never does.
pub fn keeps_running(exit_code: Option<i32>, tray: bool) -> bool {
    exit_code.is_none() && tray
}

/// Whether a start without a window (`--background`) is possible: only when a tray can bring the window back.
pub fn start_hidden(background: bool, tray: bool) -> bool {
    background && tray
}

/// `data-vibrancy` for the page: `vibrancy` (macOS), `mica` (Windows 11, build 22000+), else `none`.
pub fn vibrancy_for(os: &str, windows_build: Option<u32>) -> &'static str {
    match os {
        "macos" => "vibrancy",
        "windows" if windows_build.is_some_and(|b| b >= 22_000) => "mica",
        _ => "none",
    }
}

/// The script run in every new page before the app: sets `data-platform` and `data-vibrancy` on `<html>` (waiting for
/// the element when the script runs before it exists).
pub fn init_script(os: &str, vibrancy: &str) -> String {
    format!(
        "(function(){{var set=function(){{var d=document.documentElement;if(!d)return false;d.dataset.platform='{os}';d.dataset.vibrancy='{vibrancy}';return true;}};\
         if(!set()){{var o=new MutationObserver(function(){{if(set())o.disconnect();}});o.observe(document,{{childList:true}});}}}})();"
    )
}

/// The Windows build number (read only), for Mica.
#[cfg(windows)]
fn windows_build() -> Option<u32> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    winreg::RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .and_then(|k| k.get_value::<String, _>("CurrentBuildNumber"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

#[cfg(not(windows))]
fn windows_build() -> Option<u32> {
    None
}

/// The vibrancy of this machine.
pub fn vibrancy() -> &'static str {
    vibrancy_for(crate::dto::os_name(), windows_build())
}

/// Shows and focuses the main window, building it again from the configuration when it was closed.
pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let Some(mut cfg) = app.config().app.windows.iter().find(|w| w.label == MAIN).cloned() else {
        crate::logging::info("the main window is not in tauri.conf.json");
        return;
    };
    let vibrancy = vibrancy();
    if cfg!(windows) && vibrancy == "none" {
        // Windows 10: no Mica, so no transparent window either (it would show the desktop through).
        cfg.transparent = false;
        cfg.window_effects = None;
    }
    let built = tauri::WebviewWindowBuilder::from_config(app, &cfg)
        .map(|b| b.initialization_script(init_script(crate::dto::os_name(), vibrancy)))
        .and_then(|b| b.build());
    match built {
        Ok(w) => {
            crate::native_feel::apply(&w);
            let _ = w.set_focus();
            crate::logging::info("opened the main window");
        }
        Err(e) => crate::logging::info(&format!("could not open the main window: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_the_last_window_keeps_running_and_quit_exits() {
        assert!(keeps_running(None, true));
        assert!(!keeps_running(Some(0), true), "an explicit Quit exits");
        assert!(
            !keeps_running(None, false),
            "no tray: closing the window is the only exit, so it exits"
        );
        assert!(!keeps_running(Some(1), false));
    }

    #[test]
    fn a_background_start_needs_a_tray_to_come_back_from() {
        assert!(start_hidden(true, true));
        assert!(!start_hidden(true, false));
        assert!(!start_hidden(false, true));
    }

    #[test]
    fn vibrancy_by_platform() {
        assert_eq!(vibrancy_for("macos", None), "vibrancy");
        assert_eq!(vibrancy_for("windows", Some(22_631)), "mica");
        assert_eq!(vibrancy_for("windows", Some(19_045)), "none", "Windows 10 has no Mica");
        assert_eq!(vibrancy_for("windows", None), "none");
        assert_eq!(vibrancy_for("linux", None), "none");
    }

    #[test]
    fn the_init_script_sets_both_attributes_and_waits_for_the_element() {
        let s = init_script("macos", "vibrancy");
        assert!(s.contains("dataset.platform='macos'") && s.contains("dataset.vibrancy='vibrancy'"));
        assert!(s.contains("MutationObserver"));
    }

    /// The window is declared in the configuration but not created by Tauri: `show_main` builds it (with the init
    /// script) at start and every time it is reopened.
    #[test]
    fn the_main_window_is_declared_but_built_by_the_shell() {
        for conf in [
            include_str!("../tauri.conf.json"),
            include_str!("../tauri.macos.conf.json"),
            include_str!("../tauri.windows.conf.json"),
        ] {
            let v: serde_json::Value = serde_json::from_str(conf).unwrap();
            let w = &v["app"]["windows"][0];
            assert_eq!(w["label"], MAIN);
            assert_eq!(w["create"], false);
        }
    }

    #[test]
    fn the_macos_window_is_transparent_with_an_overlay_title_bar_and_private_api_is_on_both_sides() {
        let mac: serde_json::Value = serde_json::from_str(include_str!("../tauri.macos.conf.json")).unwrap();
        let w = &mac["app"]["windows"][0];
        assert_eq!(w["transparent"], true);
        assert_eq!(w["titleBarStyle"], "Overlay");
        assert_eq!(mac["app"]["macOSPrivateApi"], true);
        assert_eq!(mac["bundle"]["macOS"]["signingIdentity"], "-");
        let base: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(base["app"]["macOSPrivateApi"], true);
        assert!(include_str!("../Cargo.toml").contains("\"macos-private-api\""));
    }

    #[test]
    fn the_content_security_policy_has_no_eval_and_no_remote_origin() {
        let base: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let csp = base["app"]["security"]["csp"].as_str().unwrap();
        assert!(!csp.contains("unsafe-eval"));
        assert!(!csp.contains("http://") || csp.matches("http://").count() == csp.matches("http://ipc.localhost").count());
        assert!(!csp.contains("https:"));
        assert!(csp.contains("style-src 'self' 'unsafe-inline'"));
    }
}
