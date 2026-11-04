//! Tiny child processes for the tests: the test binary itself, re-executed with a mode in the environment, so the
//! same helpers work on every OS and nothing outside the test run is started. Every mode ends by itself after
//! [`LIFETIME`] if a failing test never reaps it, so a broken run cannot leave processes behind.

use std::io::Write;
use std::process::Command;
use std::time::Duration;

const MODE: &str = "PADDOCK_PROC_MODE";
/// A child never outlives this, whatever the test did.
const LIFETIME: Duration = Duration::from_secs(120);
/// Prefix of the lines a child prints (libtest prints its own banner on the same stream).
const PREFIX: &str = "CHILD:";

/// What the re-executed binary does.
#[derive(Clone, Copy)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // some modes are driven by Unix- or macOS-only tests
pub enum Mode {
    /// Wait for [`LIFETIME`].
    Sleep,
    /// Start a sleeping grandchild that shares the stdout pipe, print its pid, sleep.
    Tree,
    /// Start a sleeping grandchild that shares the pipes and exit at once.
    Leave,
    /// Write to stdout forever.
    Endless,
    /// Print the environment.
    PrintEnv,
    /// Print a line, then exit with code 3.
    Exit3,
    /// Ignore SIGTERM, print `ready`, sleep (Unix).
    IgnoreTerm,
    /// Print `ready`, sleep; exits cleanly on SIGTERM (the default action).
    Polite,
    /// Stop the programs named in `PADDOCK_STOP_IMAGES` in the folder `PADDOCK_STOP_DIR` with `installed::stop_all_here`,
    /// print the exit code, then `alive` and sleep (Windows): the stopper must survive its own folder being cleared.
    #[cfg(windows)]
    StopSelf,
    /// Touch [`FREED_BYTES`] of fresh pages, give them back the way mimalloc does (`MADV_FREE_REUSABLE`, they stay
    /// resident until the kernel needs them), print `ready`, sleep (macOS).
    Freed,
}

/// How much the [`Mode::Freed`] child touches and frees.
#[cfg(target_os = "macos")]
pub const FREED_BYTES: usize = 512 << 20;

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Sleep => "sleep",
            Mode::Tree => "tree",
            Mode::Leave => "leave",
            Mode::Endless => "endless",
            Mode::PrintEnv => "print_env",
            Mode::Exit3 => "exit3",
            Mode::IgnoreTerm => "ignore_term",
            Mode::Polite => "polite",
            #[cfg(windows)]
            Mode::StopSelf => "stop_self",
            Mode::Freed => "freed",
        }
    }
}

/// A command that re-executes this test binary in `mode`.
pub fn child(mode: Mode) -> Command {
    child_of(&std::env::current_exe().expect("the test binary has a path"), mode)
}

/// A command that runs `exe` (a copy of this test binary, for tests that need a program in a folder of their own) in `mode`.
pub fn child_of(exe: &std::path::Path, mode: Mode) -> Command {
    let mut c = crate::command(exe);
    c.args([
        "--exact",
        "testchild::helper_main",
        "--nocapture",
        "--test-threads=1",
    ]);
    c.env(MODE, mode.name());
    c
}

/// The payload of the lines a child printed.
pub fn lines(text: &str) -> Vec<&str> {
    // libtest starts the first line with "test name ... ", so the prefix is searched for, not required at the start.
    text.lines()
        .filter_map(|l| l.find(PREFIX).map(|i| &l[i + PREFIX.len()..]))
        .collect()
}

fn say(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{PREFIX}{line}");
    let _ = out.flush();
}

fn sleep_out() {
    std::thread::sleep(LIFETIME);
}

#[test]
fn helper_main() {
    let Ok(mode) = std::env::var(MODE) else {
        return;
    };
    match mode.as_str() {
        "sleep" => sleep_out(),
        "tree" | "leave" => {
            let mut g = child(Mode::Sleep);
            let Ok(grandchild) = g.spawn() else { return };
            say(&format!("pid {}", grandchild.id()));
            if mode == "tree" {
                sleep_out();
            }
        }
        "endless" => {
            let mut out = std::io::stdout().lock();
            let block = [b'x'; 4096];
            while out.write_all(&block).is_ok() {}
        }
        "print_env" => {
            for (k, v) in std::env::vars() {
                say(&format!("{k}={v}"));
            }
        }
        "exit3" => {
            say("bye");
            std::process::exit(3);
        }
        "ignore_term" => {
            #[cfg(unix)]
            crate::sys::ignore_sigterm();
            say("ready");
            sleep_out();
        }
        #[cfg(target_os = "macos")]
        "freed" => {
            crate::sys::touch_and_free(FREED_BYTES);
            say("ready");
            sleep_out();
        }
        #[cfg(windows)]
        "stop_self" => {
            let dir = std::env::var("PADDOCK_STOP_DIR").unwrap_or_default();
            let images: Vec<String> = std::env::var("PADDOCK_STOP_IMAGES")
                .unwrap_or_default()
                .split(',')
                .map(str::to_owned)
                .collect();
            let report = crate::installed::stop_all_here(
                std::path::Path::new(&dir),
                &images,
                &crate::installed::Bounds::default(),
            );
            say(&format!("exit {} actions {}", report.exit_code(), report.actions.len()));
            say("alive");
            sleep_out();
        }
        "polite" => {
            say("ready");
            sleep_out();
        }
        _ => {}
    }
}

/// Whether `pid` ends within a few seconds. A shared pipe reaches EOF when the last holder closes its files, which happens a
/// moment before the process shows as a zombie, so a check made right after EOF has to wait for that state (a deadline, not a sleep).
pub(crate) fn is_gone(pid: u32) -> bool {
    let end_at = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while crate::is_alive(pid) {
        if std::time::Instant::now() >= end_at {
            return false;
        }
        std::thread::yield_now();
    }
    true
}
