//! Programs a test writes and starts: the `ETXTBSY` ("text file busy") rule in one place.
//!
//! A test that writes a program (a script, a copied binary, a changed installed program) and runs it, or runs a program and
//! then rewrites it, can meet Linux's refusal to execute a file that is open for writing and to write a file that is being
//! executed: a child another thread forked in the middle of the write still holds the write handle, and the address space of a
//! process that just ended is released a moment after `wait` returned. The refusal passes by itself, but nothing can be waited
//! on, so [`paddock_env::text_busy`] retries that one error for a bounded time (the cause, the bound and the sources are
//! its module text; CI run 8174 failed `a_program_changed_after_install...` this way once in about 3000 runs).
//!
//! Use [`write_program`] to write a program (it never holds a write handle itself and replaces a program that runs) and
//! [`spawn`], [`output`] and [`status`] to start one the test did not write (a copied binary), in every test, instead of
//! `std::fs::write` plus `Command::spawn`: a test that does the plain thing fails now and then on a loaded Linux machine.

use paddock_env::text_busy::retry;
use std::io;
#[cfg(not(unix))]
use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Output};

/// Writes `bytes` to `path` as a program (mode 0o755 on Unix), replacing what is there, and closes it before returning, so
/// the file is ready to run.
///
/// On Unix the file is made by a short-lived `install` process from a staging file beside it, never by this process:
///
/// - **no leaked write handle.** The only descriptor that was ever open for writing on `path` belongs to a process that has
///   ended when this returns, so a child another thread forks cannot hold it, and the exec of `path` cannot meet `ETXTBSY`
///   for that reason. (The staging file's write handle can leak, which is harmless: the staging file is never run, and is
///   removed.)
/// - **a running program is replaced, not rewritten.** `install` unlinks an existing `path` and makes a new file, so a program
///   that has just ended, or still runs, never makes this fail with `ETXTBSY`.
///
/// # Errors
/// The file could not be written.
pub fn write_program(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    let (path, bytes) = (path.as_ref(), bytes.as_ref());
    #[cfg(unix)]
    {
        install_program(path, bytes)
    }
    #[cfg(not(unix))]
    {
        retry(&format!("write {}", path.display()), || {
            let mut file = std::fs::File::create(path)?;
            file.write_all(bytes)?;
            file.flush()
        })
    }
}

#[cfg(unix)]
fn install_program(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    // The OS tool by its absolute path: a test may have emptied PATH.
    const INSTALL: &str = "/usr/bin/install";
    let name = path
        .file_name()
        .map_or_else(|| "program".into(), |n| n.to_string_lossy().into_owned());
    let staging = path.with_file_name(format!(
        ".{name}.staging-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&staging, bytes)?;
    let made = Command::new(INSTALL)
        .args(["-m", "755"])
        .arg(&staging)
        .arg(path)
        .stdin(std::process::Stdio::null())
        .output();
    let removed = std::fs::remove_file(&staging);
    let out = made?;
    if !out.status.success() {
        return Err(io::Error::other(format!(
            "install could not write the program {}: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    removed
}

/// [`Command::spawn`], retrying `ETXTBSY`.
///
/// # Errors
/// The program could not be started.
pub fn spawn(cmd: &mut Command) -> io::Result<Child> {
    let what = format!("start {}", cmd.get_program().to_string_lossy());
    retry(&what, || cmd.spawn())
}

/// [`Command::output`], retrying `ETXTBSY`.
///
/// # Errors
/// The program could not be started.
pub fn output(cmd: &mut Command) -> io::Result<Output> {
    let what = format!("run {}", cmd.get_program().to_string_lossy());
    retry(&what, || cmd.output())
}

/// [`Command::status`], retrying `ETXTBSY`.
///
/// # Errors
/// The program could not be started.
pub fn status(cmd: &mut Command) -> io::Result<ExitStatus> {
    let what = format!("run {}", cmd.get_program().to_string_lossy());
    retry(&what, || cmd.status())
}
