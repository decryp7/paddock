//! The ADR 0026 §6 / ADR 0043 real-window probe of the shell-demo, run from the environment (see
//! `paddock_dialogs::probe`). It needs the real webview, a window system and the built UI, so the ordinary test run skips
//! it: it exits 0 at once unless `PADDOCK_REAL_WINDOWS=1`. A Tauri app owns the main thread, hence `harness = false`.
//!
//! `PADDOCK_REAL_WINDOWS=1 PADDOCK_PROBE_DIST=<ui/dist/shell-demo/browser> cargo test -p paddock-dialogs --features probe
//! --test show_probe -- --nocapture`

fn main() {
    paddock_dialogs::probe::run_from_env();
}
