//! The ADR 0026 §6 / ADR 0043 real-window probe of the shell-demo, run from the environment (see
//! `paddock_dialogs::probe`). It needs the real webview, a window system and the built UI, so the ordinary test run skips
//! it: it exits 0 at once unless `PADDOCK_REAL_WINDOWS=1` and the real-browser gate allows a window (rule 7:
//! `PADDOCK_LIVE_BROWSER=1` under xvfb-run, or `vm` in a test VM; set up wrongly, it fails). A Tauri app owns the main
//! thread, hence `harness = false`.
//!
//! `PADDOCK_LIVE_BROWSER=vm PADDOCK_REAL_WINDOWS=1 PADDOCK_PROBE_DIST=<ui/dist/shell-demo/browser> cargo test -p
//! paddock-dialogs --features probe --test show_probe -- --nocapture`

fn main() {
    if std::env::var("PADDOCK_REAL_WINDOWS").as_deref() == Ok("1") {
        // A real window only in a throwaway Linux container (xvfb-run, PADDOCK_LIVE_BROWSER=1) or a test VM (rule 7).
        use paddock_testkit::live_browser::{
            BrowserWindow, Host, LiveBrowser, live_browser_with, process_env,
        };
        match live_browser_with(
            Host::current(),
            &process_env,
            BrowserWindow::Shown,
            "PADDOCK_REAL_WINDOWS",
        ) {
            Ok(LiveBrowser::Allowed) => {}
            Ok(LiveBrowser::NotHere(why)) => {
                eprintln!("show_probe: not run here: {why}");
                return;
            }
            Err(why) => {
                eprintln!("show_probe: {why}");
                std::process::exit(1);
            }
        }
    }
    paddock_dialogs::probe::run_from_env();
}
