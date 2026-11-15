//! The one place the launcher starts programs: every run is bounded (ER-32) and the child is ended by the handle this process
//! holds, never by a name or a bare pid (rule 53). Standard library only, so it cannot use `paddock-proc`.
#![expect(
    clippy::disallowed_methods,
    reason = "the launcher runs before any Paddock binary exists and is std only, so it cannot use paddock-proc; every program starts here, bounded, and is ended through its own Child handle"
)]

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The exit code of a run that hit its limit (as `timeout(1)` and the shell launcher it replaces report it).
pub const TIMED_OUT: i32 = 124;
/// The exit code of a program that could not be started (the shell's "command not found").
pub const NOT_FOUND: i32 = 127;
/// The most of a program's output that is kept.
const KEEP: usize = 16 << 20;

/// A program to run. The restart marker `PADDOCK_DEV_RESTARTED` is for this launcher's own re-run only: nothing it starts,
/// the tool and a nested `./dev` included, inherits it. A git hook runs its commands with `GIT_DIR` (and `GIT_WORK_TREE`) set;
/// inherited, they make `--show-toplevel` answer the folder the hook runs in, or name another repository. The repository is the
/// one the current folder is in, for the launcher and the tool, so neither is passed on.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut cmd = Command::new(program);
    cmd.env_remove("PADDOCK_DEV_RESTARTED")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE");
    cmd
}

/// What a bounded run left.
#[derive(Debug, Clone, Default)]
pub struct Captured {
    /// The exit code: [`TIMED_OUT`], [`NOT_FOUND`], 255 for a program ended by a signal, else its own.
    pub code: i32,
    /// Standard output (lossy UTF-8, up to 16 MiB).
    pub stdout: String,
    /// Standard error (lossy UTF-8, up to 16 MiB).
    pub stderr: String,
}

impl Captured {
    /// The program ran and succeeded.
    pub fn ok(&self) -> bool {
        self.code == 0
    }
}

/// The bytes a reader thread has collected, and the note that it reached the end of the stream.
type Pump = (Arc<Mutex<Vec<u8>>>, mpsc::Receiver<()>);

fn pump(mut pipe: impl Read + Send + 'static) -> Pump {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let (tx, rx) = mpsc::channel();
    let shared = Arc::clone(&buf);
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        while let Ok(n) = pipe.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let mut g = shared.lock().unwrap_or_else(|e| e.into_inner());
            if g.len() < KEEP {
                g.extend_from_slice(&chunk[..n]);
            }
        }
        let _ = tx.send(());
    });
    (buf, rx)
}

/// Waits for `child` for at most `limit`; ends it at the limit. Returns its code.
fn wait_bounded(child: &mut Child, limit: Duration) -> i32 {
    let deadline = Instant::now() + limit;
    let mut naps = 0u32;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code().unwrap_or(255),
            Ok(None) => {}
            Err(_) => return 255,
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return TIMED_OUT;
        }
        // Short naps at first (most programs here end within milliseconds), then longer ones.
        naps += 1;
        std::thread::sleep(Duration::from_millis(if naps < 40 { 2 } else { 20 }));
    }
}

/// Runs `cmd` with its output captured, feeding `stdin` (none: closed), for at most `limit`.
pub fn capture(mut cmd: Command, limit: Duration, stdin: Option<&[u8]>) -> Captured {
    cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Captured {
                code: NOT_FOUND,
                stdout: String::new(),
                stderr: e.to_string(),
            };
        }
    };
    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let bytes = bytes.to_vec();
        // On its own thread, so a child that never reads cannot stall the wait; it may end before reading everything.
        std::thread::spawn(move || {
            let _ = pipe.write_all(&bytes);
        });
    }
    let out = child.stdout.take().map(pump);
    let err = child.stderr.take().map(pump);
    let code = wait_bounded(&mut child, limit);
    // A grandchild may still hold a pipe open (a rustup shim's compiler): wait briefly for the end of the stream, then take what came.
    let take = |p: Option<Pump>| match p {
        Some((buf, done)) => {
            let _ = done.recv_timeout(Duration::from_secs(2));
            let g = buf.lock().unwrap_or_else(|e| e.into_inner());
            String::from_utf8_lossy(&g).into_owned()
        }
        None => String::new(),
    };
    Captured {
        code,
        stdout: take(out),
        stderr: take(err),
    }
}

/// Runs `cmd` with standard output sent to standard error and standard error inherited (what a step that prints progress for a
/// log shows), standard input closed, for at most `limit`. Returns the exit code.
pub fn run_to_stderr(mut cmd: Command, limit: Duration) -> i32 {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::from(std::io::stderr()))
        .stderr(Stdio::inherit());
    match cmd.spawn() {
        Ok(mut child) => wait_bounded(&mut child, limit),
        Err(_) => NOT_FOUND,
    }
}

/// Runs `cmd` with all three streams inherited and no time limit (the tool the user asked for runs as long as it runs), and
/// returns its exit code. On Unix this replaces the process and only returns when the start failed.
pub fn replace_with(mut cmd: Command) -> i32 {
    #[cfg(unix)]
    {
        let e = std::os::unix::process::CommandExt::exec(&mut cmd);
        eprintln!("dev: could not start {:?}: {e}", cmd.get_program());
        NOT_FOUND
    }
    #[cfg(not(unix))]
    {
        match cmd.status() {
            Ok(s) => s.code().unwrap_or(255),
            Err(e) => {
                eprintln!("dev: could not start {:?}: {e}", cmd.get_program());
                NOT_FOUND
            }
        }
    }
}

/// Runs `cmd` with all three streams inherited, with no limit of its own (a build ends when cargo ends it; the caller's job
/// timeout bounds the whole run), and returns its exit code.
pub fn run_inherit(mut cmd: Command) -> i32 {
    cmd.stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit());
    match cmd.status() {
        Ok(s) => s.code().unwrap_or(255),
        Err(_) => NOT_FOUND,
    }
}
