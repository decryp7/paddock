//! A whole `run`: home guard, queue, take, load gate, the command, release; every phase timed and logged.

use crate::exit::{self, describe_exit, tail_text as tail_of};
use crate::lock::{Acquirer, Heavy, Poll, Request};
use crate::mask::mask_text;
use crate::timefmt::clock_text;
use paddock_proc::{ProcessId, command, kill_tree, lower_priority};
use serde_json::json;
#[cfg(unix)]
use std::io::IsTerminal;
use std::io::{Read, Write};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Options of one run.
#[derive(Debug, Clone, Copy)]
pub struct RunOpts {
    /// Wait for the load gate after taking the lock.
    pub gate: bool,
    /// Run the command at lower priority.
    pub low: bool,
}

/// A stop request (SIGINT/SIGTERM): the exit code to leave with, or None.
pub type Stop<'a> = &'a (dyn Fn() -> Option<i32> + Sync);

const TAIL_BYTES: usize = 16 * 1024;

/// Whether a CI system is running us (`CI` set and not 0/false).
pub fn ci() -> bool {
    std::env::var("CI").is_ok_and(|v| !v.is_empty() && v != "0" && v != "false")
}

/// The failure line: the tool's version and the OS, the exit code in hex (NTSTATUS-friendly), the cause and the next step.
pub fn failure_line(run_id: &str, log: Option<&std::path::Path>, code: i32, cause: &str, next: &str) -> String {
    let log = log.map_or(String::new(), |p| format!(", log {}", p.display()));
    format!(
        "heavy {} ({}) exit 0x{:08X}: {cause}. Next: {next}. (run {run_id}{log})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        code as u32
    )
}

/// Says a failure once: the one `::error::` annotation in CI, a plain line otherwise.
pub fn announce(run_id: &str, log: Option<&std::path::Path>, code: i32, cause: &str, next: &str) {
    let line = failure_line(run_id, log, code, cause, next);
    if ci() {
        eprintln!("::error::{line}");
    } else {
        eprintln!("heavy: {line}");
    }
}

fn ci_error(h: &Heavy, code: i32, cause: &str, next: &str) {
    announce(h.log.run_id(), h.log.file().map(std::path::PathBuf::as_path), code, cause, next);
}

fn ms(a: Instant) -> u64 {
    u64::try_from(a.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Runs `args` under the lock. Returns the exit code.
pub fn run(h: &Heavy, req: Request, args: &[String], opts: RunOpts, stop: Stop<'_>) -> i32 {
    let name = req.name.clone();
    h.log.info(
        "heavy.run",
        "run starts",
        &[
            ("Agent", json!(name)),
            ("Command", json!(req.cmdline)),
            ("Cwd", json!(req.cwd)),
            ("High", json!(req.high)),
            ("Gate", json!(opts.gate)),
            ("GateLoad", json!(h.cfg.gate_load)),
            ("Slots", json!(h.cfg.slots)),
            ("Root", json!(h.cfg.root.to_string_lossy())),
        ],
    );
    let mut acq = match Acquirer::new(h, req.clone()) {
        Ok(a) => a,
        Err(e) => {
            (h.say)(&e);
            h.log.event(crate::log::Level::Error, "heavy.run", &e, &[]);
            ci_error(
                h,
                exit::LOCK_NOT_TAKEN,
                &e,
                &format!("make {} writable or set HEAVY_DIR", h.cfg.root.display()),
            );
            return exit::LOCK_NOT_TAKEN;
        }
    };
    let t_queue = Instant::now();
    loop {
        if let Some(code) = stop() {
            h.log
                .warn("heavy.run", "stopped while waiting for the lock", &[("ExitCode", json!(code))]);
            ci_error(
                h,
                code,
                "stopped by a signal while waiting for the lock",
                "run it again when you want the step",
            );
            return code;
        }
        match acq.poll() {
            Poll::Held => break,
            Poll::GaveUp(e) => {
                (h.say)(&e);
                h.log.event(crate::log::Level::Error, "heavy.run", &e, &[]);
                ci_error(
                    h,
                    exit::LOCK_NOT_TAKEN,
                    &e,
                    &format!("check free disk and permissions of {}", h.cfg.root.display()),
                );
                return exit::LOCK_NOT_TAKEN;
            }
            Poll::Waiting => {
                if !h.nap(h.cfg.poll, &|| stop().is_some()) {
                    let code = stop().unwrap_or(130);
                    h.log
                        .warn("heavy.run", "stopped while waiting for the lock", &[("ExitCode", json!(code))]);
                    ci_error(
                        h,
                        code,
                        "stopped by a signal while waiting for the lock",
                        "run it again when you want the step",
                    );
                    return code;
                }
            }
        }
    }
    let waited = acq.waited_micros() / 1_000_000;
    let t1 = h.now();
    (h.say)(&format!(
        "{name} holds the lock (pid {}) at {}, waited {waited}s, load {}",
        h.pid,
        clock_text(t1),
        h.load_now()
    ));
    h.alog(&format!("{name} took the lock after {waited}s: {} (cwd {})", req.cmdline, req.cwd));

    if opts.gate {
        let t_gate = Instant::now();
        if h.load.load().is_none() {
            (h.say)("load gate skipped: this platform has no load average");
            h.log.info("heavy.gate", "load gate skipped: no load average on this platform", &[]);
        } else {
            let mut last = h.now();
            let mut closed_once = false;
            while let Some(msg) = h.gate_closed() {
                if !closed_once {
                    closed_once = true;
                    h.log.info(
                        "heavy.gate",
                        &format!("waiting: {msg}"),
                        &[("Load", json!(h.load_now())), ("Threshold", json!(h.cfg.gate_load))],
                    );
                }
                if h.now().saturating_sub(last) as f64 >= h.cfg.wait_log.as_secs_f64() {
                    let w = h.now().saturating_sub(acq.t0());
                    (h.say)(&format!("{name} waiting {w}s: {msg}"));
                    h.alog(&format!("{name} waiting {w}s: {msg}"));
                    h.log
                        .info("heavy.gate", &format!("still waiting: {msg}"), &[("Load", json!(h.load_now()))]);
                    last = h.now();
                }
                if !h.nap(h.cfg.gate_poll, &|| stop().is_some()) {
                    let code = stop().unwrap_or(130);
                    h.log.warn("heavy.run", "stopped at the load gate", &[("ExitCode", json!(code))]);
                    ci_error(
                        h,
                        code,
                        "stopped by a signal at the load gate",
                        "run it again when the load is lower",
                    );
                    return code;
                }
            }
            (h.say)(&format!("gate ok, load {}", h.load_now()));
        }
        h.log.info(
            "heavy.phase",
            "gate over",
            &[("Phase", json!("gate_wait")), ("DurationMs", json!(ms(t_gate)))],
        );
    }
    let _ = t_queue;

    let t2 = h.now();
    let t_run = Instant::now();
    let (code, desc, tail) = exec(h, args, opts, stop, &mut acq);
    let ran = h.now().saturating_sub(t2);
    let t3 = h.now();
    h.log.info(
        "heavy.phase",
        "command ended",
        &[
            ("Phase", json!("run")),
            ("DurationMs", json!(ms(t_run))),
            ("ExitCode", json!(code)),
            ("Exit", json!(desc)),
        ],
    );
    let t_rel = Instant::now();
    acq.release();
    h.log.info(
        "heavy.phase",
        "released",
        &[("Phase", json!("release")), ("DurationMs", json!(ms(t_rel)))],
    );
    (h.say)(&format!(
        "{name} done rc={code} at {}, ran {ran}s (held {}s, waited {waited}s), releasing",
        clock_text(t3),
        t3.saturating_sub(t1)
    ));
    h.alog(&format!(
        "{name} done rc={code}, ran {ran}s (held {}s, waited {waited}s): {}",
        t3.saturating_sub(t1),
        req.cmdline
    ));
    if code == 0 {
        h.log
            .info("heavy.run", "run ends", &[("ExitCode", json!(0)), ("RanSeconds", json!(ran))]);
    } else {
        let tail_s = tail.unwrap_or_default();
        h.log.event(
            crate::log::Level::Error,
            "heavy.run",
            &format!("the command failed: {desc}"),
            &[
                ("Command", json!(req.cmdline)),
                ("ExitCode", json!(code)),
                ("Exit", json!(desc)),
                ("StderrTail", json!(tail_s)),
                ("RanSeconds", json!(ran)),
            ],
        );
        ci_error(
            h,
            code,
            &format!("the command `{}` failed: {desc}", req.cmdline),
            &format!(
                "read the stderr tail in the log (run {}) and fix the command, not the lock",
                h.log.run_id()
            ),
        );
    }
    code
}

/// Starts the command and waits for it. Returns (exit code, description, stderr tail).
fn exec(h: &Heavy, args: &[String], opts: RunOpts, stop: Stop<'_>, acq: &mut Acquirer<'_>) -> (i32, String, Option<String>) {
    let mut cmd = command(&args[0]);
    cmd.args(&args[1..]);
    // The correlation id travels to the command, so its own logs can be matched to this run.
    cmd.env("DEVKIT_RUN_ID", h.log.run_id());
    if opts.low {
        lower_priority(&mut cmd);
    }
    #[cfg(unix)]
    if !std::io::stdin().is_terminal() {
        use std::os::unix::process::CommandExt;
        // Its own process group, so a stop reaches the whole tree (a terminal user keeps Ctrl-C going straight to the command).
        cmd.process_group(0);
    }
    if h.cfg.stderr_tail {
        cmd.stderr(Stdio::piped());
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let code = if e.kind() == std::io::ErrorKind::NotFound {
                exit::NOT_FOUND
            } else {
                exit::NOT_RUNNABLE
            };
            let msg = format!("cannot run '{}': {e}", args[0]);
            (h.say)(&format!("heavy: {msg}"));
            return (code, msg, None);
        }
    };
    let tail = Arc::new(Mutex::new(Vec::<u8>::new()));
    let pipe_closed = Arc::new(AtomicBool::new(false));
    let pump = child.stderr.take().map(|mut err| {
        let tail = Arc::clone(&tail);
        let closed = Arc::clone(&pipe_closed);
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let mut out = std::io::stderr();
            while let Ok(n) = err.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let _ = out.write_all(&buf[..n]);
                let _ = out.flush();
                if let Ok(mut t) = tail.lock() {
                    t.extend_from_slice(&buf[..n]);
                    let over = t.len().saturating_sub(TAIL_BYTES);
                    t.drain(..over);
                }
            }
            closed.store(true, Ordering::SeqCst);
        })
    });
    let id = ProcessId::of(child.id());
    if let Some(id) = id {
        acq.set_child(id.pid(), id.started().as_micros());
    }
    h.log.info("heavy.run", "command started", &[("ChildPid", json!(child.id()))]);
    let mut stopped: Option<i32> = None;
    let mut deadline: Option<Instant> = None;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {}
            Err(_) => break None,
        }
        if stopped.is_none()
            && let Some(code) = stop()
        {
            stopped = Some(code);
            h.log.warn(
                "heavy.run",
                "stop requested: ending the command and everything it started",
                &[("ExitCode", json!(code))],
            );
            if let Some(id) = id {
                let _ = kill_tree(id);
            }
            deadline = Some(Instant::now() + Duration::from_secs(10));
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            let _ = child.kill();
            deadline = None;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    // The command ended, but a background process it started may still hold the stderr pipe open. Never wait for that
    // for long: give the pipe `drain` to run dry, keep watching for a stop request, then leave the reader behind.
    if let Some(p) = pump {
        if !pipe_closed.load(Ordering::SeqCst) {
            h.log.info(
                "heavy.run",
                "command exited; draining its stderr",
                &[("DrainMs", json!(h.cfg.drain.as_millis() as u64))],
            );
        }
        let end = Instant::now() + h.cfg.drain;
        while !pipe_closed.load(Ordering::SeqCst) && Instant::now() < end {
            if stopped.is_none()
                && let Some(code) = stop()
            {
                stopped = Some(code);
                h.log.warn(
                    "heavy.run",
                    "stop requested while draining the command's stderr",
                    &[("ExitCode", json!(code))],
                );
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if pipe_closed.load(Ordering::SeqCst) {
            let _ = p.join();
        } else {
            h.log.warn(
                "heavy.run",
                "stderr is still held open by a process the command left running; not waiting for it",
                &[],
            );
        }
    }
    let tail_str = tail
        .lock()
        .ok()
        .map(|t| mask_text(&tail_of(&t, 40, TAIL_BYTES)))
        .filter(|t| !t.is_empty());
    if let Some(code) = stopped {
        return (code, "stopped by a signal to heavy".into(), tail_str);
    }
    let Some(status) = status else {
        return (1, "could not wait for the command".into(), tail_str);
    };
    #[cfg(unix)]
    let sig = std::os::unix::process::ExitStatusExt::signal(&status);
    #[cfg(not(unix))]
    let sig = None;
    let (code, desc) = describe_exit(status.code(), sig);
    (code, desc, if code == 0 { None } else { tail_str })
}
