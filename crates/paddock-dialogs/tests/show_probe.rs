//! The ADR 0026 §6 / ADR 0043 real-window probe of the shell-demo, run from the environment (see
//! `paddock_dialogs::probe`). It needs the real webview, a window system and the built UI, so the ordinary test run skips
//! it: it exits 0 at once unless `PADDOCK_REAL_WINDOWS=1`. A Tauri app owns the main thread, hence `harness = false`.
//!
//! `PADDOCK_REAL_WINDOWS=1 PADDOCK_PROBE_DIST=<ui/dist/shell-demo/browser> cargo nextest run -p paddock-dialogs --features probe
//! --test show_probe --no-capture`
//!
//! Every test run goes through nextest (rule 89), which lists a binary's tests with `--list --format terse` (and again with
//! `--ignored`) before it runs one: this binary answers that with its one test, `show_probe`, and no ignored one.

/// The one test this binary holds, as nextest lists it.
const LISTED: &str = "show_probe: test";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--list") {
        if !args.iter().any(|a| a == "--ignored") {
            println!("{LISTED}");
        }
        return;
    }
    paddock_dialogs::probe::run_from_env();
}
