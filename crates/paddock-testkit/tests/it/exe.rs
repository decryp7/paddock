//! `exe`: a program the test wrote is ready to run, and a start that meets `ETXTBSY` is retried. Unix only (`ETXTBSY` and the
//! program's mode are Unix refusals). The program written is the kit's stand-in binary with its script beside it ([`program`]).
#![cfg(unix)]

use paddock_testkit::{StandIn, exe};
use std::path::Path;
use std::process::Command;

/// Writes the stand-in binary to `path` through `exe::write_program` (the function under test) and its script, which prints
/// `said` and exits with `code`, beside it: a program that runs the same on every OS, with no shell script planted.
fn program(path: &Path, said: Option<&str>, code: i32) {
    let src = paddock_testkit::tempdir();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap();
    let mut spec = StandIn::new(src.path(), name).exit(code);
    if let Some(line) = said {
        spec = spec.stdout(line);
    }
    let installed = spec.install().unwrap();
    exe::write_program(path, &std::fs::read(installed.program()).unwrap()).unwrap();
    std::fs::copy(
        src.path().join(format!("{name}.stand-in.json")),
        path.with_file_name(format!("{name}.stand-in.json")),
    )
    .unwrap();
}

#[test]
fn a_written_program_is_executable_and_runs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prog");
    program(&path, Some("hi"), 0);
    let out = exe::output(&mut Command::new(&path)).unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hi");
    // Written again over itself (the file exists, the mode stays right).
    program(&path, None, 3);
    assert_eq!(
        exe::status(&mut Command::new(&path)).unwrap().code(),
        Some(3)
    );
    assert!(exe::spawn(&mut Command::new(&path)).unwrap().wait().is_ok());
}

/// While a handle opened for writing is held, a plain start is refused; `exe::status` waits it out (the handle is let go when
/// the first attempt has failed, which a flag set from inside the retried start proves).
#[cfg(target_os = "linux")]
#[test]
fn a_start_refused_because_the_file_is_held_open_for_writing_succeeds_after_it_is_let_go() {
    use paddock_env::text_busy::{is_text_busy, retries_for};
    use std::time::{Duration, Instant};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prog");
    program(&path, None, 0);
    let writer = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    let raw = Command::new(&path).status();
    assert!(
        raw.as_ref().is_err_and(is_text_busy),
        "precondition: {raw:?}"
    );
    let label = format!("run {}", path.display());
    let releaser = std::thread::spawn(move || {
        let guard = Instant::now() + Duration::from_secs(20);
        while retries_for(&label) == 0 && Instant::now() < guard {
            std::thread::yield_now();
        }
        drop(writer);
    });
    let status = exe::status(&mut Command::new(&path)).unwrap();
    releaser.join().unwrap();
    assert!(status.success());
}

/// The parent never holds the program's write handle, so a child another thread forks cannot inherit it, and a program that runs
/// is replaced (a new file), not written to. The running program is a copy of `sleep`: only a binary that the kernel maps (not a
/// script, which the shell reads) makes its file busy.
#[cfg(target_os = "linux")]
#[test]
fn the_program_is_written_by_another_process_and_a_running_one_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prog");
    program(&path, None, 0);
    let held: Vec<_> = std::fs::read_dir("/proc/self/fd")
        .unwrap()
        .flatten()
        .filter(|e| std::fs::read_link(e.path()).is_ok_and(|t| t == path))
        .collect();
    assert!(
        held.is_empty(),
        "this process holds {} handle(s) on the program it wrote",
        held.len()
    );
    // A running binary at the path.
    std::fs::remove_file(&path).unwrap();
    std::fs::copy("/bin/sleep", &path).unwrap();
    let mut child = exe::spawn(Command::new(&path).arg("600")).unwrap();
    // The precondition (ER-33): a plain write to the running program is refused.
    let plain = std::fs::OpenOptions::new().write(true).open(&path);
    assert!(
        plain
            .as_ref()
            .is_err_and(paddock_env::text_busy::is_text_busy),
        "a running program must refuse a plain write: {plain:?}"
    );
    // Replaced while the copy of `sleep` runs: the stand-in binary and its script, exiting 7 (the script is written first, the
    // program is the file `write_program` replaces).
    program(&path, None, 7);
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(
        exe::status(&mut Command::new(&path)).unwrap().code(),
        Some(7)
    );
}
