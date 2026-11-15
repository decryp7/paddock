//! Repository chores for Pitwall developers, run as `cargo xtask <task>` (alias in `.cargo/config.toml`).
//!
//! - `verify-bundle [Pitwall.app] [--smoke]`: checks a built macOS app bundle (the port of `scripts/macos-verify.sh`): its binaries,
//!   its launch agent plist, the code signature (SMAppService refuses a bundle that is only linker-signed, error -67056), the rpath
//!   and the minimum macOS version; with `--smoke` it also starts the bundled server on a free port in a throw-away data folder and
//!   the app hidden, and checks both. An installed app and its service are `./dev app verify-installed pitwall`.
//!
//! The checks are pure functions over text with unit tests; the thin edge runs the OS tools (`plutil`, `codesign`, `otool`) and the
//! bundle's own programs through `paddock-proc` (bounded; every process the smoke test starts is ended as a tree by identity).

mod bundle;
mod smoke;

use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <task>\n\ntasks:\n  verify-bundle [Pitwall.app] [--smoke]   check a built macOS app bundle (default /Applications/Pitwall.app); --smoke also runs the bundled server and the app headless\n  help                                    show this message";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("verify-bundle") => bundle::run_task(&args[1..]),
        Some("help" | "-h" | "--help") | None => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown task `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
