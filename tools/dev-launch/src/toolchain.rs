//! The Rust toolchain the repository pins: finding it, checking that it works and installing it with rustup.
//!
//! One `rustup toolchain install` of a pin at a time on a host: two installs of one pin in one `RUSTUP_HOME` corrupt each
//! other. The wait is an advisory file lock (`File::lock`, std 1.89) that the operating system releases when the holder ends
//! for any reason, so no owner is recorded and no pid is ever judged (rule 53). A toolchain is never uninstalled here, except
//! the half-installed one of our own install that ran out of time: on a shared runner other jobs may be compiling with it
//! (run 8063: a repair's uninstall took std away from four running jobs).

use crate::proc::{self, TIMED_OUT};
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Seconds an install, and the wait for another install, may take: `PADDOCK_DEV_TOOLCHAIN_TIMEOUT`, default 900.
pub fn install_timeout() -> u64 {
    std::env::var("PADDOCK_DEV_TOOLCHAIN_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(900)
}

/// `$RUSTUP_HOME`, else `$HOME/.rustup`.
pub fn rustup_home() -> PathBuf {
    match std::env::var_os("RUSTUP_HOME").filter(|v| !v.is_empty()) {
        Some(h) => PathBuf::from(h),
        None => {
            let home = std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .or_else(|| std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()))
                .unwrap_or_else(|| "/nonexistent".into());
            PathBuf::from(home).join(".rustup")
        }
    }
}

/// Whether `program` is found on `PATH` (`command -v`).
pub fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else { return false };
    let names: Vec<String> = if cfg!(windows) {
        vec![format!("{program}.exe"), format!("{program}.cmd"), program.to_string()]
    } else {
        vec![program.to_string()]
    };
    std::env::split_paths(&path).any(|dir| names.iter().any(|n| dir.join(n).is_file()))
}

/// The lock file of one install of `pin` in this rustup home.
pub fn lock_path(pin: &str) -> PathBuf {
    let safe: String = pin
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    rustup_home().join(format!(".paddock-install-{safe}.flock"))
}

/// Takes the install lock of `pin`, waiting for a live holder at most [`install_timeout`] seconds. The lock ends with the
/// returned file. On failure the `::error::` line is the error (ER-27).
pub fn lock(pin: &str) -> Result<File, String> {
    let path = lock_path(pin);
    let _ = std::fs::create_dir_all(rustup_home());
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("::error::./dev: cannot open the toolchain install lock {}: {e}. Make that folder writable or set RUSTUP_HOME to one you can write, then run ./dev again.", path.display()))?;
    let limit = install_timeout();
    let start = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(e)) => {
                return Err(format!(
                    "::error::./dev: cannot lock {}: {e}. Run ./dev again; if it repeats, report it with this log.",
                    path.display()
                ));
            }
        }
        if start.elapsed().as_secs() >= limit {
            return Err(format!(
                "::error::./dev: another install of the toolchain {pin} holds {} and did not finish within {limit} s. Wait for it, then run ./dev again; the lock ends with that install, so no file needs deleting.",
                path.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn with_toolchain(name: &str, program: &str, args: &[&str]) -> std::process::Command {
    let mut c = proc::command(program);
    c.args(args).env("RUSTUP_TOOLCHAIN", name);
    c
}

/// A scratch folder for the std check. The launcher runs before any Paddock tool exists, so it makes its own.
#[expect(
    clippy::disallowed_methods,
    reason = "no Paddock scratch helper exists before the launcher has run; the folder is removed at once"
)]
fn scratch_dir() -> Option<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("paddock-dev-std-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).ok().map(|()| dir)
}

/// The first part of the toolchain that does not work, `None` when all do: `rustc`, `cargo`, a one-line program with std (a
/// toolchain whose std went missing still answers `--version`; run 8063), and `cargo clippy` and `rustfmt` when asked for.
/// Each check is bounded (ER-32).
pub fn broken(name: &str, components: &[String]) -> Option<String> {
    let s = Duration::from_secs;
    if !proc::capture(with_toolchain(name, "rustc", &["--version"]), s(60), None).ok() {
        return Some("rustc does not run".into());
    }
    if !proc::capture(with_toolchain(name, "cargo", &["--version"]), s(60), None).ok() {
        return Some("cargo does not run".into());
    }
    let Some(dir) = scratch_dir() else {
        return Some("no temporary folder for the std check".into());
    };
    let src = dir.join("t.rs");
    let exe = dir.join(if cfg!(windows) { "t.exe" } else { "t" });
    let wrote = std::fs::write(&src, "fn main() { println!(\"{}\", std::env::args().count()); }\n").is_ok();
    let compiled = wrote
        && proc::capture(
            with_toolchain(
                name,
                "rustc",
                &["--edition", "2021", "-o", &exe.display().to_string(), &src.display().to_string()],
            ),
            s(120),
            None,
        )
        .ok();
    let _ = std::fs::remove_dir_all(&dir);
    if !compiled {
        return Some("a std program does not compile".into());
    }
    for c in components {
        match c.as_str() {
            "clippy" if !proc::capture(with_toolchain(name, "cargo", &["clippy", "--version"]), s(60), None).ok() => {
                return Some("cargo clippy does not run".into());
            }
            "rustfmt" if !proc::capture(with_toolchain(name, "rustfmt", &["--version"]), s(60), None).ok() => {
                return Some("rustfmt does not run".into());
            }
            _ => {}
        }
    }
    None
}

/// With the install lock held by the caller: runs `install` and checks the result ([`broken`]). `Ok` when the toolchain
/// works; otherwise the install's exit code (124: the time limit, or 3 when it installed but does not work) and why.
pub fn install_checked(name: &str, components: &[String], install: &dyn Fn() -> i32) -> Result<(), (i32, String)> {
    let code = install();
    if code != 0 {
        return Err((code, format!("exit {code}")));
    }
    match broken(name, components) {
        None => Ok(()),
        Some(bad) => Err((3, format!("{bad} after the install"))),
    }
}

/// The manual repair, for the `::error::` lines.
pub fn repair_step(name: &str, components: &[String]) -> String {
    let mut step = format!("rustup toolchain install {name} --profile minimal");
    for c in components {
        step.push_str(&format!(" --component {c}"));
    }
    format!("on the machine, with no jobs running, run 'rustup toolchain uninstall {name}' and then '{step}'")
}

/// `rustup toolchain install <name> ...` with output on standard error, bounded by [`install_timeout`].
pub fn install_named(name: &str, components: &[String], targets: &[String]) -> i32 {
    let mut c = proc::command("rustup");
    c.args(["toolchain", "install", name, "--profile", "minimal"]);
    for comp in components {
        c.args(["--component", comp]);
    }
    for t in targets {
        c.args(["--target", t]);
    }
    c.arg("--no-self-update");
    proc::run_to_stderr(c, Duration::from_secs(install_timeout()))
}

/// The `rust-toolchain(.toml)` rustup reads for `tool`, found the way rustup finds it: the closest one above.
pub fn toolchain_file(tool: &Path) -> Option<PathBuf> {
    tool.ancestors().find_map(|d| {
        ["rust-toolchain.toml", "rust-toolchain"]
            .iter()
            .map(|f| d.join(f))
            .find(|p| p.is_file())
    })
}

/// The pin (`channel`, else the first line of a plain `rust-toolchain`) and the components a toolchain file asks for.
pub fn read_pin(text: &str) -> (String, Vec<String>) {
    let text = text.replace('\r', "");
    let value = |key: &str, close: char, open: char| {
        text.lines().find_map(|l| {
            let rest = l
                .trim_start()
                .strip_prefix(key)?
                .trim_start()
                .strip_prefix('=')?
                .trim_start()
                .strip_prefix(open)?;
            Some(rest[..rest.rfind(close)?].to_string())
        })
    };
    let pin = value("channel", '"', '"')
        .and_then(|v| v.split('"').next().map(str::to_string))
        .or_else(|| text.lines().next().map(str::to_string))
        .unwrap_or_default();
    let comps = value("components", ']', '[')
        .map(|v| v.replace(['"', ','], " ").split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();
    (pin, comps)
}

/// What `ensure` needs to know about the run.
pub struct Ensure<'a> {
    /// The tool's folder (`tools/paddock-dev`): rustup reads the pin from there.
    pub tool: &'a Path,
    /// The compiler named by `RUSTC` or a cargo config (then nothing is installed).
    pub named_rustc: Option<&'a str>,
}

/// The toolchain the repository pins is installed (rustup 1.28 no longer installs a pinned toolchain by itself, so a runner
/// that has rustup but not yet that version has a `rustc` shim that cannot run; run 8034). Only when rustup exists, no
/// compiler was named by `RUSTC` or a cargo config, a toolchain file exists and an earlier step of the job did not already fail
/// to install it (`PADDOCK_TOOLCHAIN_FAILED`). Logged with its duration; a failure says the next step. Returns whether it works.
pub fn ensure(e: &Ensure) -> bool {
    if e.named_rustc.is_some() || !on_path("rustup") {
        return false;
    }
    let Some(tf) = toolchain_file(e.tool) else { return false };
    if std::env::var("PADDOCK_TOOLCHAIN_FAILED").as_deref() == Ok("1") {
        eprintln!(
            "dev: not installing the toolchain {} pins again: an earlier step of this job failed to (PADDOCK_TOOLCHAIN_FAILED=1).",
            tf.display()
        );
        return false;
    }
    let (pin, comps) = read_pin(&std::fs::read_to_string(&tf).unwrap_or_default());
    let t0 = Instant::now();
    let _ = std::fs::create_dir_all(rustup_home());
    let Ok(held) = lock(&pin).inspect_err(|m| eprintln!("{m}")) else {
        return false;
    };
    let runs_here = || {
        let mut c = proc::command("rustc");
        c.arg("-vV").current_dir(e.tool);
        proc::capture(c, Duration::from_secs(60), None).ok()
    };
    // The holder before us may have installed it.
    if runs_here() && broken(&pin, &comps).is_none() {
        drop(held);
        eprintln!("dev: the toolchain {pin} was installed by another run meanwhile.");
        return true;
    }
    eprintln!(
        "dev: rustc does not run here; installing the toolchain {} pins ({pin}) with rustup (at most {} s).",
        tf.display(),
        install_timeout()
    );
    let install_pinned = || {
        let mut c = proc::command("rustup");
        c.args(["toolchain", "install"]).current_dir(e.tool);
        proc::run_to_stderr(c, Duration::from_secs(install_timeout()))
    };
    let result = install_checked(&pin, &comps, &install_pinned);
    if matches!(result, Err((TIMED_OUT, _))) {
        // Only here, where rustc did not run before (no job can be compiling with this toolchain), is our own cut-off install removed.
        eprintln!("dev: the install ran out of time; removing the half-installed toolchain {pin}.");
        let mut c = proc::command("rustup");
        c.args(["toolchain", "uninstall", &pin]).current_dir(e.tool);
        let _ = proc::run_to_stderr(c, Duration::from_secs(120));
    }
    drop(held);
    if let Err((_, why)) = result {
        eprintln!(
            "::error::./dev: 'rustup toolchain install' for {} failed ({why}, {} s; exit 124 means the time limit). ./dev never uninstalls a toolchain, since other jobs may be using it. Next step: {}, by hand in tools/paddock-dev to see why (network, disk, a proxy), then run ./dev again.",
            tf.display(),
            t0.elapsed().as_secs(),
            repair_step(&pin, &comps)
        );
        return false;
    }
    eprintln!("dev: installed the pinned toolchain in {} s.", t0.elapsed().as_secs());
    true
}

#[cfg(test)]
mod tests {
    use super::read_pin;

    #[test]
    fn the_pin_and_components_come_from_a_toml_or_a_plain_file() {
        let toml = "# c\r\n[toolchain]\r\nchannel = \"1.100.0\"\r\nprofile = \"minimal\"\r\ncomponents = [\"rustfmt\", \"clippy\"]\r\n";
        assert_eq!(
            read_pin(toml),
            ("1.100.0".to_string(), vec!["rustfmt".to_string(), "clippy".to_string()])
        );
        assert_eq!(read_pin("1.90.0\n"), ("1.90.0".to_string(), vec![]));
    }
}
