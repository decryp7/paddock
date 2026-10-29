//! Child processes: every external program Dyno runs opens no console window and is bounded by a timeout. This is
//! the only module that creates [`Command`]s.

use dyno_core::issues::ReadControl;
use dyno_core::ports::PlatformError;
use std::ffi::OsStr;
use std::io::Read;
use std::path::Path;
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Most bytes kept from each of a tool's output streams.
pub const MAX_TOOL_OUTPUT: usize = 4 * 1024 * 1024;

/// Windows process creation flag: no console window.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A command for `program` that opens no console window on Windows.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[allow(clippy::disallowed_methods)]
    let cmd = Command::new(program);
    #[cfg(windows)]
    let cmd = {
        use std::os::windows::process::CommandExt;
        let mut cmd = cmd;
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    };
    cmd
}

type Drain = Option<std::thread::JoinHandle<Vec<u8>>>;

fn drain(stream: Option<impl Read + Send + 'static>) -> Drain {
    stream.map(|out| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut limited = out.take(MAX_TOOL_OUTPUT as u64);
            let _ = limited.read_to_end(&mut buf);
            // Keep reading (and dropping) so the child never blocks on a full pipe.
            let mut rest = limited.into_inner();
            let _ = std::io::copy(&mut rest, &mut std::io::sink());
            buf
        })
    })
}

/// The program's file name, for messages (never its arguments).
fn program_name(cmd: &Command) -> String {
    Path::new(cmd.get_program())
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Runs `cmd` to completion with stdin closed and stdout/stderr captured (each at most [`MAX_TOOL_OUTPUT`] bytes),
/// killing it after `timeout`. A non-zero exit is not an error: callers look at [`Output::status`].
pub fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Result<Output, PlatformError> {
    let program = program_name(&cmd);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => PlatformError::Unsupported(format!("{program} is not available on this computer.")),
        std::io::ErrorKind::PermissionDenied => PlatformError::AccessDenied(format!("Not allowed to run {program}.")),
        _ => PlatformError::Failed(format!("Could not run {program}: {e}")),
    })?;
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    let started = Instant::now();
    let status: ExitStatus = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(PlatformError::Timeout(format!(
                    "{program} did not finish within {} s.",
                    timeout.as_secs_f32().max(0.1)
                )));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                let _ = child.kill();
                return Err(PlatformError::Failed(format!("Waiting for {program} failed: {e}")));
            }
        }
    };
    let bytes = |h: Drain| h.and_then(|h| h.join().ok()).unwrap_or_default();
    Ok(Output {
        status,
        stdout: bytes(out),
        stderr: bytes(err),
    })
}

/// The pid of a running tool that Cancel and the timeout may signal, until the tool has been reaped. Both take the lock; so does
/// the reaping, which releases the pid under it: a signal either reaches the live tool or finds nothing.
struct TreeGuard {
    pid: std::sync::Mutex<Option<u32>>,
    end: fn(u32),
}

impl TreeGuard {
    fn new(pid: Option<u32>, end: fn(u32)) -> Result<TreeGuard, PlatformError> {
        match pid {
            Some(p) if p != 0 => Ok(TreeGuard {
                pid: std::sync::Mutex::new(Some(p)),
                end,
            }),
            _ => Err(PlatformError::Failed(
                "Dyno could not tell which process it started, so it did not run the tool.".into(),
            )),
        }
    }

    /// Ends the tool's tree if it has not been reaped.
    fn kill(&self) {
        if let Some(p) = *self.pid.lock().unwrap_or_else(std::sync::PoisonError::into_inner) {
            (self.end)(p);
        }
    }

    /// The tool has been reaped: no signal may reach the pid any more (tests; the runner releases it in `reap_when_ended`).
    #[cfg(test)]
    fn reaped(&self) {
        *self.pid.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    /// Reaps the tool if it has ended, under the lock, and releases the pid with it; false while it still runs.
    fn reap_when_ended(&self, tool: &mut paddock_proc::Supervised) -> bool {
        let mut pid = self.pid.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if tool.is_running() {
            return false;
        }
        *pid = None;
        true
    }
}

/// Runs `cmd` with stdout read line by line as it arrives: `on_line` sees every line (the caller counts what it parses, so
/// progress grows while the tool works). The child is killed after `timeout` and at once when `control` is cancelled; the call
/// then returns [`PlatformError::Failed`] ("stopped") and the caller, who knows it cancelled, drops the result. A non-zero exit
/// is an error carrying the start of the error output. Only [`MAX_TOOL_OUTPUT`] bytes of stdout are read.
pub fn run_streaming(
    mut cmd: Command,
    timeout: Duration,
    control: &dyn ReadControl,
    mut on_line: impl FnMut(&str),
) -> Result<(), PlatformError> {
    use std::io::BufRead;
    let program = program_name(&cmd);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // Supervised (ER-22): its own process group ended as a whole, a kill-on-close job object on Windows, so a tool's helpers end
    // with it and nothing outlives Dyno.
    let mut tool = paddock_proc::Supervised::spawn(cmd).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => PlatformError::Unsupported(format!("{program} is not available on this computer.")),
        std::io::ErrorKind::PermissionDenied => PlatformError::AccessDenied(format!("Not allowed to run {program}.")),
        _ => PlatformError::Failed(format!("Could not run {program}: {e}")),
    })?;
    // Never a guess: pid 0 would signal our own process group. A child with no id is ended and reported.
    let guard = match TreeGuard::new(tool.id(), paddock_proc::kill_tree) {
        Ok(g) => std::sync::Arc::new(g),
        Err(e) => {
            tool.kill();
            return Err(e);
        }
    };
    let stdout = tool.take_stdout();
    let err = drain(tool.take_stderr());
    let timed_out = std::sync::Arc::new(AtomicBool::new(false));
    // Cancel ends the tree at once: the read below sees the end of the stream and returns, it does not wait for a line.
    let g = guard.clone();
    let _on_cancel = control.on_cancel(Box::new(move || g.kill()));
    // The timeout is a watcher that ends when the read does.
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let watcher = {
        let (timed_out, guard) = (timed_out.clone(), guard.clone());
        std::thread::spawn(move || {
            if done_rx.recv_timeout(timeout) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
                timed_out.store(true, Ordering::SeqCst);
                guard.kill();
            }
        })
    };
    // The lines are read on a thread of their own and handed over here, so Cancel and the timeout never wait for the next line
    // (a helper that keeps the pipe open after the tool was killed cannot hold this call).
    let (lines_tx, lines_rx) = std::sync::mpsc::channel::<String>();
    if let Some(out) = stdout {
        std::thread::spawn(move || {
            let mut lines = std::io::BufReader::new(out.take(MAX_TOOL_OUTPUT as u64));
            let mut line = String::new();
            loop {
                line.clear();
                match lines.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if lines_tx.send(line.trim_end_matches(['\n', '\r']).to_string()).is_err() {
                            break;
                        }
                    }
                }
            }
            // Past the cap the rest is read and dropped so the child never blocks on a full pipe.
            let _ = std::io::copy(&mut lines.into_inner().into_inner(), &mut std::io::sink());
        });
    } else {
        drop(lines_tx);
    }
    loop {
        match lines_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(line) => on_line(&line),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if control.cancelled() || timed_out.load(Ordering::SeqCst) {
                    break;
                }
            }
        }
    }
    // Reap the tool under the guard's lock: a Cancel or a timeout that comes later finds the pid released and signals nothing (the
    // id may belong to another process by then).
    loop {
        if guard.reap_when_ended(&mut tool) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let status = tool.wait();
    let _ = done_tx.send(());
    let _ = watcher.join();
    if timed_out.load(Ordering::SeqCst) {
        return Err(PlatformError::Timeout(format!(
            "{program} did not finish within {} s.",
            timeout.as_secs_f32().max(0.1)
        )));
    }
    if control.cancelled() {
        return Err(PlatformError::Failed(format!("{program} was stopped.")));
    }
    match status {
        Some(s) if s.success() => Ok(()),
        _ => {
            let stderr = err.and_then(|h| h.join().ok()).unwrap_or_default();
            let text = String::from_utf8_lossy(&stderr);
            let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("no message").trim();
            Err(PlatformError::Failed(format!("{program} failed: {}", truncate(first, 200))))
        }
    }
}

/// Runs `cmd` and returns its standard output as text when it exits successfully; otherwise a
/// [`PlatformError::Failed`] with the start of its error output.
pub fn output_text(cmd: Command, timeout: Duration) -> Result<String, PlatformError> {
    let program = program_name(&cmd);
    let out = run_with_timeout(cmd, timeout)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let first = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("no message").trim();
        Err(PlatformError::Failed(format!("{program} failed: {}", truncate(first, 200))))
    }
}

/// At most `max` characters of `s`.
pub(crate) fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    pub(crate) fn shell(script: &str) -> Command {
        if cfg!(windows) {
            let mut c = command("cmd.exe");
            c.args(["/d", "/c", script]);
            c
        } else {
            let mut c = command("/bin/sh");
            c.args(["-c", script]);
            c
        }
    }

    #[test]
    fn captures_output_of_a_finished_tool() {
        let out = run_with_timeout(shell("echo hello && echo oops 1>&2"), Duration::from_secs(20)).unwrap();
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("hello"));
        assert!(String::from_utf8_lossy(&out.stderr).contains("oops"));
        assert!(output_text(shell("echo hi"), Duration::from_secs(20)).unwrap().contains("hi"));
    }

    #[test]
    fn a_failing_tool_reports_its_error_output() {
        let err = output_text(shell("echo bad thing 1>&2 && exit 3"), Duration::from_secs(20)).unwrap_err();
        assert!(matches!(&err, PlatformError::Failed(m) if m.contains("bad thing")), "{err:?}");
    }

    #[test]
    fn stops_a_tool_that_hangs() {
        let hang = if cfg!(windows) { "ping -n 30 127.0.0.1 >nul" } else { "sleep 30" };
        let started = Instant::now();
        let err = run_with_timeout(shell(hang), Duration::from_millis(300)).unwrap_err();
        assert!(matches!(err, PlatformError::Timeout(_)), "{err:?}");
        assert!(started.elapsed() < Duration::from_secs(15));
    }

    #[test]
    fn a_missing_program_is_unsupported_and_names_no_arguments() {
        let mut cmd = command("dyno-no-such-program");
        cmd.args(["--token", "SECRET"]);
        let err = run_with_timeout(cmd, Duration::from_secs(5)).unwrap_err();
        assert!(matches!(&err, PlatformError::Unsupported(m) if !m.contains("SECRET")), "{err:?}");
        assert_eq!(truncate("abcdef", 3), "abc");
    }

    /// A control a test cancels by hand; it runs the registered stop once, at once, like a job's cancel token.
    #[derive(Default)]
    struct Hand {
        cancelled: AtomicBool,
        stop: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
        parsed: std::sync::atomic::AtomicU64,
    }

    impl Hand {
        fn cancel(&self) {
            self.cancelled.store(true, Ordering::SeqCst);
            if let Some(f) = self.stop.lock().unwrap().as_ref() {
                f();
            }
        }
    }

    impl ReadControl for Hand {
        fn cancelled(&self) -> bool {
            self.cancelled.load(Ordering::SeqCst)
        }
        fn parsed(&self, n: u64) {
            self.parsed.store(n, Ordering::SeqCst);
        }
        fn on_cancel(&self, stop: Box<dyn Fn() + Send + Sync>) -> Box<dyn Send> {
            *self.stop.lock().unwrap() = Some(stop);
            Box::new(())
        }
    }

    #[test]
    fn a_streamed_tool_hands_over_its_lines_as_they_arrive() {
        let hand = Hand::default();
        let mut seen = Vec::new();
        run_streaming(shell("echo one && echo two"), Duration::from_secs(20), &hand, |l| {
            seen.push(l.trim().to_string())
        })
        .unwrap();
        assert_eq!(seen, ["one", "two"]);
        let err = run_streaming(shell("echo bad thing 1>&2 && exit 3"), Duration::from_secs(20), &hand, |_| {}).unwrap_err();
        assert!(matches!(&err, PlatformError::Failed(m) if m.contains("bad thing")), "{err:?}");
    }

    #[cfg(unix)]
    #[test]
    fn cancel_kills_a_streamed_tool_that_is_silent() {
        let hand = std::sync::Arc::new(Hand::default());
        let h = hand.clone();
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            h.cancel();
        });
        let started = Instant::now();
        let mut lines = 0;
        let err = run_streaming(shell("echo first && sleep 30"), Duration::from_secs(60), hand.as_ref(), |_| {
            lines += 1
        })
        .unwrap_err();
        canceller.join().unwrap();
        assert_eq!(lines, 1, "the line that arrived before the cancel was handed over");
        assert!(matches!(&err, PlatformError::Failed(m) if m.contains("stopped")), "{err:?}");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "Cancel did not wait for the tool: {:?}",
            started.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_streamed_tool_that_hangs_is_stopped_at_its_timeout() {
        let started = Instant::now();
        let err = run_streaming(shell("sleep 30"), Duration::from_millis(300), &Hand::default(), |_| {}).unwrap_err();
        assert!(matches!(err, PlatformError::Timeout(_)), "{err:?}");
        assert!(started.elapsed() < Duration::from_secs(15));
    }

    #[cfg(unix)]
    #[test]
    fn cancel_ends_the_whole_tree_of_a_streamed_tool_not_just_its_first_process() {
        // The tool says the pid of its helper as its first line; Cancel comes when that line has been handed over.
        let hand = Hand::default();
        let mut helper: Option<u32> = None;
        let started = Instant::now();
        let err = run_streaming(shell("sleep 30 & echo $!; wait"), Duration::from_secs(60), &hand, |line| {
            helper = line.trim().parse().ok();
            hand.cancel();
        })
        .unwrap_err();
        assert!(matches!(&err, PlatformError::Failed(m) if m.contains("stopped")), "{err:?}");
        assert!(started.elapsed() < Duration::from_secs(10));
        let pid = helper.expect("the first line was the helper's pid");
        for _ in 0..200 {
            if !paddock_proc::is_alive(pid) {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("the tool's helper process {pid} is still running after Cancel");
    }

    static KILLED: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());
    fn record(pid: u32) {
        KILLED.lock().unwrap().push(pid);
    }

    #[test]
    fn a_missing_or_zero_pid_is_an_error_never_a_signal_to_our_own_process_group() {
        assert!(matches!(TreeGuard::new(None, record), Err(PlatformError::Failed(_))));
        assert!(
            matches!(TreeGuard::new(Some(0), record), Err(PlatformError::Failed(_))),
            "pid 0 is every process of our own group"
        );
        assert!(TreeGuard::new(Some(4242), record).is_ok());
    }

    #[test]
    fn a_cancel_after_the_tool_was_reaped_signals_nothing_because_the_pid_may_belong_to_someone_else_by_then() {
        let before = KILLED.lock().unwrap().len();
        let guard = TreeGuard::new(Some(777_001), record).unwrap();
        guard.kill();
        assert_eq!(
            KILLED.lock().unwrap()[before..],
            [777_001],
            "while the tool lives, Cancel reaches it"
        );
        guard.reaped();
        guard.kill();
        guard.kill();
        assert_eq!(
            KILLED.lock().unwrap().len(),
            before + 1,
            "after it was reaped, nothing is signalled"
        );
    }
}
