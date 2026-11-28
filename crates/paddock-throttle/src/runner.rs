//! A whole `run`: home guard, queue, take, load gate, the command, release; every phase timed and logged.

use crate::capture::{self, RunLog};
use crate::exit::{self, describe_exit, tail_text as tail_of};
use crate::hang::{HangWatch, Verdict};
use crate::lock::{Acquirer, Poll, Request, Throttle};
use crate::mask::mask_text;
use crate::timefmt::clock_text;
use paddock_proc::queue_stall::StallClock;
use paddock_proc::{
    ProcessId, RunState, command, kill_tree, lower_priority, run_state, tree_cpu_time,
};
use serde_json::json;
#[cfg(unix)]
use std::io::IsTerminal;
use std::io::{Read, Write};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Options of one run.
#[derive(Debug, Clone, Copy)]
pub struct RunOpts {
    /// Wait for the load gate after taking the lock.
    pub gate: bool,
    /// Run the command at lower priority.
    pub low: bool,
    /// Give up with [`exit::QUEUE_LIMIT`] when no slot was taken (queue and load gate included) within this long.
    pub queue_max: Option<Duration>,
    /// Give up with [`exit::QUEUE_LIMIT`] when the queue (or the load gate) made no progress for this long ([`paddock_proc::queue_stall`]): the
    /// wait is bounded by progress, not by its length, so a queue that keeps moving is waited for.
    pub queue_stall: Option<Duration>,
    /// End the command with [`exit::RUN_LIMIT`] once it has run this long (queue time is not counted).
    pub run_max: Option<Duration>,
    /// End the command with [`exit::HUNG`] when it has written nothing and its process tree used no CPU for this long ([`crate::hang`]).
    pub stall: Option<Duration>,
}

/// A stop request (SIGINT/SIGTERM): the exit code to leave with, or None.
pub type Stop<'a> = &'a (dyn Fn() -> Option<i32> + Sync);

const TAIL_BYTES: usize = 16 * 1024;

/// The variable throttle hands the command it runs: `<run id>:<pid>` of the run that holds the slot. A throttle run that inherits it (a throttle
/// step of a command that is itself a heavy step: `./dev throttle -- ./dev gen check`) runs inside that slot instead of queuing for a
/// slot its own parent holds (ER-43: a waiter never holds the capacity the work it waits for needs).
pub const HELD_VAR: &str = "THROTTLE_HELD";

/// The holder a `THROTTLE_HELD` value names, when that process is still alive: `Some(text)` to run inside its slot, `None` to queue as
/// usual (no marker, a malformed one, or a holder that is gone: a marker left in a shell that outlived its run).
pub fn held_by(h: &Throttle, value: Option<&str>) -> Option<String> {
    held_by_procs(h.procs.as_ref(), value)
}

/// [`held_by`] over a process table alone: for a caller that decides before it has booted a [`Throttle`] (the shim's nested pass-through).
pub fn held_by_procs(procs: &dyn crate::procs::Procs, value: Option<&str>) -> Option<String> {
    let v = value?.trim();
    let (_, pid) = v.rsplit_once(':')?;
    let pid: u32 = pid.parse().ok()?;
    crate::procs::pid_alive(procs, pid, None).then(|| v.to_string())
}

/// A run inside the slot an ancestor holds ([`HELD_VAR`]): no ticket, no queue, no load gate; the command runs with the same run
/// limit and stop handling as a normal run, and its exit code is passed on.
pub fn run_nested(
    h: &Throttle,
    name: &str,
    args: &[String],
    opts: RunOpts,
    stop: Stop<'_>,
    outer: &str,
) -> i32 {
    (h.say)(&format!(
        "{name} runs inside the slot {outer} already holds ({HELD_VAR}): no second slot, no queue (ER-43)"
    ));
    h.log.info(
        "throttle.run",
        "nested run inside a held slot",
        &[
            ("Agent", json!(name)),
            ("HeldBy", json!(outer)),
            ("Command", json!(args.join(" "))),
        ],
    );
    let t = Instant::now();
    let run_log = open_run_log(h, name, args);
    let Ended { code, desc, .. } = exec(h, args, opts, stop, None, run_log);
    (h.say)(&format!(
        "{name} done rc={code} inside {outer}, ran {:.1} s",
        t.elapsed().as_secs_f64()
    ));
    if code != 0 {
        h.log.event(
            crate::log::Level::Error,
            "throttle.run",
            &format!("the nested command failed: {desc}"),
            &[("ExitCode", json!(code))],
        );
    }
    code
}

/// Whether a CI system is running us (`CI` set and not 0/false).
pub fn ci() -> bool {
    std::env::var("CI").is_ok_and(|v| !v.is_empty() && v != "0" && v != "false")
}

/// The failure line: the tool's version and the OS, the exit code in hex (NTSTATUS-friendly), the cause and the next step.
pub fn failure_line(
    run_id: &str,
    log: Option<&std::path::Path>,
    code: i32,
    cause: &str,
    next: &str,
) -> String {
    let log = log.map_or(String::new(), |p| format!(", log {}", p.display()));
    format!(
        "throttle {} ({}) exit 0x{:08X}: {cause}. Next: {next}. (run {run_id}{log})",
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
        eprintln!("throttle: {line}");
    }
}

fn ci_error(h: &Throttle, code: i32, cause: &str, next: &str) {
    announce(
        h.log.run_id(),
        h.log.file().map(std::path::PathBuf::as_path),
        code,
        cause,
        next,
    );
}

fn ms(a: Instant) -> u64 {
    u64::try_from(a.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// A slot taken: the holder's handle and what the wait cost.
pub struct Admitted<'a> {
    /// The slot, held until released or dropped.
    pub acq: Acquirer<'a>,
    /// Seconds from the first ticket to the slot (queue and load gate).
    pub queued: f64,
    /// Whole seconds waited, as the lines say it.
    pub waited: u64,
    /// Epoch second the slot was taken.
    pub held_from: u64,
}

/// Admission of one run: a ticket, the queue, the load gate and the slot, with every wait timed and logged and the queue limit, the queue
/// stall, a stop request and a lock that cannot be taken ending it with the exit code to leave with. The run itself ([`run`]) and a
/// participant that supervises its own command (the shim) both take their slot through it, so the rules are written once (ER-22).
pub fn admit<'a>(
    h: &'a Throttle,
    req: &Request,
    opts: &RunOpts,
    stop: Stop<'_>,
) -> Result<Admitted<'a>, i32> {
    let name = req.name.clone();
    let mut acq = match Acquirer::new(h, req.clone()) {
        Ok(a) => a,
        Err(e) => {
            (h.say)(&e);
            h.log
                .event(crate::log::Level::Error, "throttle.run", &e, &[]);
            ci_error(
                h,
                exit::LOCK_NOT_TAKEN,
                &e,
                &format!("make {} writable or set THROTTLE_DIR", h.cfg.root.display()),
            );
            return Err(exit::LOCK_NOT_TAKEN);
        }
    };
    let q0 = h.clock.now_micros();
    let queued_secs = || h.clock.now_micros().saturating_sub(q0) as f64 / 1_000_000.0;
    let timing = |q: f64, r: f64| (h.say)(&format!("{name} timing queued {q:.1} s, ran {r:.1} s"));
    let over_queue_limit = || {
        opts.queue_max
            .is_some_and(|m| queued_secs() >= m.as_secs_f64())
    };
    let queue_limit = |what: &str| {
        let (q, max) = (
            queued_secs(),
            opts.queue_max.map_or(0.0, |m| m.as_secs_f64()),
        );
        let msg = format!(
            "{name} queue limit: no slot {what} within {max:.0} s, so the command never ran"
        );
        (h.say)(&msg);
        h.alog(&msg);
        timing(q, 0.0);
        h.log.event(
            crate::log::Level::Error,
            "throttle.run",
            &msg,
            &[("QueuedSeconds", json!(q)), ("QueueMaxSeconds", json!(max))],
        );
        ci_error(
            h,
            exit::QUEUE_LIMIT,
            &format!(
                "no throttle slot within the queue limit of {max:.0} s (queued {q:.1} s); the command never ran"
            ),
            "the machine is busy with other agents' heavy steps: re-run when `throttle status` shows it free; this is not a pass",
        );
        exit::QUEUE_LIMIT
    };
    // A stalled queue ends the wait ([`paddock_proc::queue_stall`]): the same exit as the queue limit, but the line names who holds the slot and
    // for how long nothing moved, so a wrapper can tell a busy machine that keeps moving (waited for) from one that does not.
    let mut stall = opts
        .queue_stall
        .map(|d| StallClock::new(d, h.clock.now_micros()));
    let queue_stalled = |quiet: f64, holders: &str| {
        let q = queued_secs();
        let msg = format!(
            "{name} queue stalled: the queue made no progress for {quiet:.0} s (queued {q:.1} s, held by {holders}), so the command never ran"
        );
        (h.say)(&msg);
        h.alog(&msg);
        timing(q, 0.0);
        h.log.event(
            crate::log::Level::Error,
            "throttle.run",
            &msg,
            &[
                ("QueuedSeconds", json!(q)),
                ("QuietSeconds", json!(quiet)),
                ("HeldBy", json!(holders)),
            ],
        );
        ci_error(
            h,
            exit::QUEUE_LIMIT,
            &format!(
                "queue stalled: no progress for {quiet:.0} s, held by {holders} (queued {q:.1} s); the command never ran"
            ),
            "look at `throttle status` for the holder, and re-run when it moves; this is not a pass",
        );
        exit::QUEUE_LIMIT
    };
    // The load gate is waited for BEFORE a slot is taken (ER-43: a waiter never holds the capacity the work it waits for needs): a
    // run keeps its ticket and place while the load is high, and takes a slot only when the load lets it start. The gate is read once
    // more after the take, and a slot taken just as the load rose is given back.
    let gate_on = opts.gate && h.load.load().is_some();
    if opts.gate && !gate_on {
        (h.say)("load gate skipped: this platform has no load average");
        h.log.info(
            "throttle.gate",
            "load gate skipped: no load average on this platform",
            &[],
        );
    }
    let mut gate_logged = false;
    let mut gate_said = h.now();
    let mut gate_wait_start: Option<Instant> = None;
    let mut gate_waited = Duration::ZERO;
    loop {
        if over_queue_limit() {
            return Err(queue_limit("taken"));
        }
        if let Some(code) = stop() {
            h.log.warn(
                "throttle.run",
                "stopped while waiting for the lock",
                &[("ExitCode", json!(code))],
            );
            ci_error(
                h,
                code,
                "stopped by a signal while waiting for the lock",
                "run it again when you want the step",
            );
            return Err(code);
        }
        if gate_on && let Some(msg) = h.gate_closed() {
            gate_wait_start.get_or_insert_with(Instant::now);
            if let Some(sc) = stall.as_mut() {
                let view = acq.queue_view(opts.run_max);
                if let Some(quiet) = sc.observe(&view, h.clock.now_micros()) {
                    acq.release();
                    return Err(queue_stalled(quiet, &format!("the load gate ({msg})")));
                }
            }
            if !gate_logged {
                gate_logged = true;
                h.log.info(
                    "throttle.gate",
                    &format!("waiting: {msg}"),
                    &[
                        ("Load", json!(h.load_now())),
                        ("Threshold", json!(h.cfg.gate_load)),
                    ],
                );
            }
            if h.now().saturating_sub(gate_said) as f64 >= h.cfg.wait_log.as_secs_f64() {
                let w = h.now().saturating_sub(acq.t0());
                h.note_wait(&name, w, &msg);
                h.log.info(
                    "throttle.gate",
                    &format!("still waiting: {msg}"),
                    &[("Load", json!(h.load_now()))],
                );
                gate_said = h.now();
            }
            if !h.nap(h.cfg.gate_poll, &|| stop().is_some()) {
                let code = stop().unwrap_or(130);
                h.log.warn(
                    "throttle.run",
                    "stopped at the load gate",
                    &[("ExitCode", json!(code))],
                );
                acq.release();
                ci_error(
                    h,
                    code,
                    "stopped by a signal at the load gate",
                    "run it again when the load is lower",
                );
                return Err(code);
            }
            continue;
        }
        if let Some(t) = gate_wait_start.take() {
            gate_waited += t.elapsed();
            h.log.info(
                "throttle.phase",
                "gate over",
                &[
                    ("Phase", json!("gate_wait")),
                    ("DurationMs", json!(gate_waited.as_millis() as u64)),
                ],
            );
        }
        match acq.poll() {
            Poll::Held => {
                if gate_on && h.gate_closed().is_some() {
                    // The load rose between the check and the take: the slot goes back, the run waits for the load again.
                    acq.step_back();
                    h.log.info(
                        "throttle.gate",
                        "took a slot as the load rose; gave it back",
                        &[],
                    );
                    continue;
                }
                break;
            }
            Poll::GaveUp(e) => {
                (h.say)(&e);
                h.log
                    .event(crate::log::Level::Error, "throttle.run", &e, &[]);
                ci_error(
                    h,
                    exit::LOCK_NOT_TAKEN,
                    &e,
                    &format!(
                        "check free disk and permissions of {}",
                        h.cfg.root.display()
                    ),
                );
                return Err(exit::LOCK_NOT_TAKEN);
            }
            Poll::Waiting => {
                if let Some(sc) = stall.as_mut() {
                    let view = acq.queue_view(opts.run_max);
                    if let Some(quiet) = sc.observe(&view, h.clock.now_micros()) {
                        return Err(queue_stalled(quiet, &view.holders_text()));
                    }
                }
                if !h.nap(h.cfg.poll, &|| stop().is_some()) {
                    let code = stop().unwrap_or(130);
                    h.log.warn(
                        "throttle.run",
                        "stopped while waiting for the lock",
                        &[("ExitCode", json!(code))],
                    );
                    ci_error(
                        h,
                        code,
                        "stopped by a signal while waiting for the lock",
                        "run it again when you want the step",
                    );
                    return Err(code);
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
    h.alog(&format!(
        "{name} took the lock after {waited}s: {} (cwd {})",
        req.cmdline, req.cwd
    ));
    if gate_on {
        (h.say)(&format!("gate ok, load {}", h.load_now()));
    }
    if over_queue_limit() {
        // The slot came, but only after the limit (the load gate took the time): the limit is about waiting, not about running.
        acq.release();
        return Err(queue_limit("taken before the load gate opened"));
    }
    let queued = queued_secs();
    Ok(Admitted {
        acq,
        queued,
        waited,
        held_from: t1,
    })
}

/// Runs `args` under the lock. Returns the exit code.
pub fn run(h: &Throttle, req: Request, args: &[String], opts: RunOpts, stop: Stop<'_>) -> i32 {
    let name = req.name.clone();
    h.log.info(
        "throttle.run",
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
    let Admitted {
        mut acq,
        queued,
        waited,
        held_from: t1,
    } = match admit(h, &req, &opts, stop) {
        Ok(a) => a,
        Err(code) => return code,
    };
    let timing = |q: f64, r: f64| (h.say)(&format!("{name} timing queued {q:.1} s, ran {r:.1} s"));

    let t2 = h.now();
    let run_log = open_run_log(h, &name, args);
    let t_run = Instant::now();
    let Ended {
        code,
        desc,
        tail,
        hung,
    } = exec(h, args, opts, stop, Some(&mut acq), run_log.clone());
    let ran = h.now().saturating_sub(t2);
    timing(queued, t_run.elapsed().as_secs_f64());
    let summary = format!(
        "{name} summary: exit {code}, {:.1} s, {}{}",
        t_run.elapsed().as_secs_f64(),
        if hung { "hung" } else { "ran" },
        run_log
            .as_ref()
            .map_or(String::new(), |l| format!("; log {}", l.path().display()))
    );
    (h.say)(&summary);
    if let Some(l) = &run_log {
        l.note(&format!("throttle: {summary}"));
    }
    // A row that passed teaches the history how long it takes (a failure is not its usual time).
    if code == 0
        && let Some(row) = &req.row
        && let Err(e) =
            crate::history::History::of(&h.cfg).record(row, t_run.elapsed().as_secs_f64(), h.now())
    {
        h.log.warn(
            "throttle.history",
            "could not record the run time; the next run has no history for it",
            &[("Error", json!(e.to_string()))],
        );
    }
    let t3 = h.now();
    h.log.info(
        "throttle.phase",
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
        "throttle.phase",
        "released",
        &[
            ("Phase", json!("release")),
            ("DurationMs", json!(ms(t_rel))),
        ],
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
        h.log.info(
            "throttle.run",
            "run ends",
            &[("ExitCode", json!(0)), ("RanSeconds", json!(ran))],
        );
    } else {
        let tail_s = tail.unwrap_or_default();
        h.log.event(
            crate::log::Level::Error,
            "throttle.run",
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

/// Opens the run's log in `<runs>/<agent>/` after evicting old ones ([`crate::capture`]) and says where it is. `None` (said, never a
/// failure of the run) when it cannot be written: the output still reaches the terminal.
fn open_run_log(h: &Throttle, name: &str, args: &[String]) -> Option<Arc<RunLog>> {
    let runs = h.cfg.runs_dir();
    let agent = capture::clean_name(name, 64);
    let now = std::time::UNIX_EPOCH + Duration::from_micros(h.clock.now_micros());
    let pruned = capture::prune(
        &runs,
        &agent,
        capture::KEEP_PER_AGENT,
        capture::MAX_AGE,
        now,
    );
    if !pruned.removed.is_empty() || !pruned.failed.is_empty() {
        h.log.info(
            "throttle.capture",
            "old run logs removed",
            &[
                ("Removed", json!(pruned.removed.len())),
                (
                    "Failed",
                    json!(
                        pruned
                            .failed
                            .iter()
                            .map(|(p, e)| format!("{}: {e}", p.display()))
                            .collect::<Vec<_>>()
                    ),
                ),
            ],
        );
    }
    let stamp = crate::timefmt::stamp_text(h.clock.now_micros());
    match RunLog::create(
        &runs,
        &agent,
        &stamp,
        &capture::short_name(args),
        capture::MAX_LOG_BYTES,
    ) {
        Ok(l) => {
            (h.say)(&format!("{name} run log {}", l.path().display()));
            h.log.info(
                "throttle.capture",
                "run log opened",
                &[("Path", json!(l.path().to_string_lossy()))],
            );
            Some(Arc::new(l))
        }
        Err(e) => {
            let msg = format!(
                "no run log in {}: {e}; the output goes to the terminal only. Next: set PADDOCK_DEV_TARGETS to a writable folder",
                runs.join(&agent).display()
            );
            (h.say)(&msg);
            h.log.warn("throttle.capture", &msg, &[]);
            None
        }
    }
}

/// How the command ended.
struct Ended {
    code: i32,
    desc: String,
    tail: Option<String>,
    /// Ended by the hang detector.
    hung: bool,
}

/// Copies one output pipe of the command to throttle's own stream and the run log, counting the bytes (the hang detector's output
/// signal) and keeping the stderr tail when asked.
fn pump(
    mut from: impl Read + Send + 'static,
    to_stderr: bool,
    log: Option<Arc<RunLog>>,
    bytes: Arc<AtomicU64>,
    tail: Option<Arc<Mutex<Vec<u8>>>>,
    open: Arc<AtomicUsize>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = from.read(&mut buf) {
            if n == 0 {
                break;
            }
            let chunk = &buf[..n];
            if to_stderr {
                let mut out = std::io::stderr().lock();
                let _ = out.write_all(chunk);
                let _ = out.flush();
            } else {
                let mut out = std::io::stdout().lock();
                let _ = out.write_all(chunk);
                let _ = out.flush();
            }
            if let Some(l) = &log {
                l.write(chunk);
            }
            bytes.fetch_add(n as u64, Ordering::SeqCst);
            if let Some(t) = &tail
                && let Ok(mut t) = t.lock()
            {
                t.extend_from_slice(chunk);
                let over = t.len().saturating_sub(TAIL_BYTES);
                t.drain(..over);
            }
        }
        open.fetch_sub(1, Ordering::SeqCst);
    })
}

/// Starts the command and waits for it.
fn exec(
    h: &Throttle,
    args: &[String],
    opts: RunOpts,
    stop: Stop<'_>,
    acq: Option<&mut Acquirer<'_>>,
    run_log: Option<Arc<RunLog>>,
) -> Ended {
    let ended = |code: i32, desc: String, tail: Option<String>| Ended {
        code,
        desc,
        tail,
        hung: false,
    };
    let mut cmd = command(&args[0]);
    cmd.args(&args[1..]);
    // The correlation id travels to the command, so its own logs can be matched to this run.
    cmd.env("DEVKIT_RUN_ID", h.log.run_id());
    // A heavy step of this command runs inside this slot (run_nested); a nested run passes on the marker it was given.
    if acq.is_some() {
        let held = format!("{}:{}", h.log.run_id(), h.pid);
        cmd.env(HELD_VAR, &held);
        // A binary of the earlier name (`heavy`) inside this command still finds its parent's slot.
        cmd.env("HEAVY_HELD", &held);
    }
    // ER-48: the threads a step may use are its share of the cores: a step admitted while `n` may run gets `1/n` of them, so steps at
    // once times threads stay within the cores. The caller's own values win (ER-47: children inherit the environment; a caller that set
    // one chose it).
    // A nested run (no slot of its own) inherits its holder's share: the variables its parent set stay as they are.
    let share = acq.as_ref().map_or(1, |a| a.share());
    let env = crate::gate::step_env(h.cfg.cpus, share, &|k| {
        std::env::var_os(k).map(|v| v.to_string_lossy().into_owned())
    });
    if !env.is_empty() {
        let set: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        h.log.info(
            "throttle.run",
            "threads set for the step",
            &[
                ("Share", json!(share)),
                ("Cpus", json!(h.cfg.cpus)),
                ("Set", json!(set)),
            ],
        );
    }
    cmd.envs(env);
    // The prompt policy (ER-47, named: paddock_env::command::no_prompts): no standard input and no toolchain auto-install, so a
    // tool that asks "install X? [Y/n]" cannot hold the slot for hours (cargo llvm-cov did, 2026-11-20). THROTTLE_STDIN=inherit opts in.
    paddock_env::command::no_prompts(&mut cmd);
    if h.cfg.stdin_inherit {
        cmd.stdin(Stdio::inherit());
    }
    if opts.low {
        lower_priority(&mut cmd);
    }
    #[cfg(unix)]
    if !std::io::stdin().is_terminal() {
        use std::os::unix::process::CommandExt;
        // Its own process group, so a stop reaches the whole tree (a terminal user keeps Ctrl-C going straight to the command).
        cmd.process_group(0);
    }
    // Both streams are pipes: throttle copies them to its own and to the run log, and counts them for the hang detector.
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let code = if e.kind() == std::io::ErrorKind::NotFound {
                exit::NOT_FOUND
            } else {
                exit::NOT_RUNNABLE
            };
            let msg = format!("cannot run '{}': {e}", args[0]);
            (h.say)(&format!("throttle: {msg}"));
            if let Some(l) = &run_log {
                l.note(&format!("throttle: {msg}"));
            }
            return ended(code, msg, None);
        }
    };
    let tail = Arc::new(Mutex::new(Vec::<u8>::new()));
    let bytes = Arc::new(AtomicU64::new(0));
    let open = Arc::new(AtomicUsize::new(0));
    let mut pumps = Vec::new();
    let log_arc = run_log.clone();
    if let Some(out) = child.stdout.take() {
        open.fetch_add(1, Ordering::SeqCst);
        pumps.push(pump(
            out,
            false,
            log_arc.clone(),
            Arc::clone(&bytes),
            None,
            Arc::clone(&open),
        ));
    }
    if let Some(err) = child.stderr.take() {
        open.fetch_add(1, Ordering::SeqCst);
        let t = h.cfg.stderr_tail.then(|| Arc::clone(&tail));
        pumps.push(pump(
            err,
            true,
            log_arc,
            Arc::clone(&bytes),
            t,
            Arc::clone(&open),
        ));
    }
    // The slot is held by this throttle's own file handle, which the command does not inherit: a command that outlives this throttle
    // never keeps the slot (the kernel frees it when throttle ends, however it ends).
    let id = ProcessId::of(child.id());
    let slot = acq.and_then(|a| a.slot());
    h.log.info(
        "throttle.run",
        "command started",
        &[("ChildPid", json!(child.id())), ("Slot", json!(slot))],
    );
    let mut stopped: Option<i32> = None;
    let mut deadline: Option<Instant> = None;
    let run_end = opts.run_max.map(|m| Instant::now() + m);
    let mut over_run = false;
    let t_start = Instant::now();
    let since_start = || u64::try_from(t_start.elapsed().as_micros()).unwrap_or(u64::MAX);
    let mut hang = opts.stall.map(|d| HangWatch::new(d, since_start()));
    let mut hung = false;
    let mut hang_noted = false;
    if hang.is_some() && id.is_none() {
        // No identity, no CPU reading (and nothing to end by): the detector is not measured, never a silent pass (ER-33).
        hang = None;
        let msg = "--stall: the command's start time cannot be read, so its CPU time cannot be either; the hang detector is off for this run";
        (h.say)(msg);
        h.log.warn("throttle.hang", msg, &[]);
    }
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
                "throttle.run",
                "stop requested: ending the command and everything it started",
                &[("ExitCode", json!(code))],
            );
            if let Some(id) = id {
                let _ = kill_tree(id);
            }
            deadline = Some(Instant::now() + Duration::from_secs(10));
        }
        if !over_run && stopped.is_none() && run_end.is_some_and(|e| Instant::now() >= e) {
            over_run = true;
            let msg = format!(
                "run limit: the command ran {} s; ending it and everything it started",
                opts.run_max.map_or(0, |m| m.as_secs())
            );
            (h.say)(&msg);
            h.log.warn("throttle.run", &msg, &[]);
            if let Some(id) = id {
                let _ = kill_tree(id);
            }
            deadline = Some(Instant::now() + Duration::from_secs(10));
        }
        if !hung
            && !over_run
            && stopped.is_none()
            && let (Some(w), Some(id)) = (hang.as_mut(), id)
        {
            let now = since_start();
            match w.observe(now, bytes.load(Ordering::SeqCst), || {
                tree_cpu_time(id).map(|d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
            }) {
                Verdict::Alive => {}
                Verdict::NotMeasured => {
                    if !hang_noted {
                        hang_noted = true;
                        let msg = "--stall: the CPU time of the command's processes cannot be read now; \
                                   the hang detector waits (unknown is never idle)";
                        (h.say)(msg);
                        h.log.warn("throttle.hang", msg, &[]);
                    }
                }
                #[expect(
                    clippy::disallowed_methods,
                    reason = "a production read after the event that settles it: the hang verdict means the whole --stall window passed with no output and no CPU, and a stop lasts until the command is resumed, so the read follows the window instead of racing a change; it only excuses a paused command (App Tamer) and any other state ends it, as before"
                )]
                Verdict::Hung(quiet) => {
                    if run_state(id) == RunState::Stopped {
                        // Paused by a signal or the OS (App Tamer): not stuck, it goes on when resumed.
                        w.touch(now);
                        h.log.info(
                            "throttle.hang",
                            "quiet, but the command is stopped (paused), not hung",
                            &[],
                        );
                    } else {
                        hung = true;
                        let limit = opts.stall.map_or(0.0, |d| d.as_secs_f64());
                        let msg = format!(
                            "hung: no output and no CPU for {limit} s; ending the command and everything it started"
                        );
                        (h.say)(&msg);
                        h.log.warn(
                            "throttle.hang",
                            &msg,
                            &[
                                ("QuietSeconds", json!(quiet.as_secs_f64())),
                                ("StallSeconds", json!(limit)),
                                ("OutputBytes", json!(bytes.load(Ordering::SeqCst))),
                            ],
                        );
                        if let Some(l) = &run_log {
                            l.note(&format!("throttle: {msg}"));
                        }
                        // By the identity throttle holds (pid plus start time, rule 53): its group, then each process of its tree.
                        let _ = kill_tree(id);
                        deadline = Some(Instant::now() + Duration::from_secs(10));
                    }
                }
            }
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            let _ = child.kill();
            deadline = None;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    // The command ended, but a background process it started may still hold an output pipe open. Never wait for that
    // for long: give the pipes `drain` to run dry, keep watching for a stop request, then leave the readers behind.
    let closed = || open.load(Ordering::SeqCst) == 0;
    if !pumps.is_empty() {
        if !closed() {
            h.log.info(
                "throttle.run",
                "command exited; draining its output",
                &[("DrainMs", json!(h.cfg.drain.as_millis() as u64))],
            );
        }
        let end = Instant::now() + h.cfg.drain;
        while !closed() && Instant::now() < end {
            if stopped.is_none()
                && let Some(code) = stop()
            {
                stopped = Some(code);
                h.log.warn(
                    "throttle.run",
                    "stop requested while draining the command's output",
                    &[("ExitCode", json!(code))],
                );
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if closed() {
            for p in pumps {
                let _ = p.join();
            }
        } else {
            h.log.warn(
                "throttle.run",
                "the output is still held open by a process the command left running; not waiting for it",
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
        return ended(code, "stopped by a signal to throttle".into(), tail_str);
    }
    if hung {
        let limit = opts.stall.map_or(0.0, |d| d.as_secs_f64());
        return Ended {
            code: exit::HUNG,
            desc: format!("hung: no output and no CPU for {limit} s, so it was ended"),
            tail: tail_str,
            hung: true,
        };
    }
    if over_run {
        let m = opts.run_max.map_or(0, |m| m.as_secs());
        return ended(
            exit::RUN_LIMIT,
            format!("ended after the run limit of {m} s"),
            tail_str,
        );
    }
    let Some(status) = status else {
        return ended(1, "could not wait for the command".into(), tail_str);
    };
    #[cfg(unix)]
    let sig = std::os::unix::process::ExitStatusExt::signal(&status);
    #[cfg(not(unix))]
    let sig = None;
    let (code, desc) = describe_exit(status.code(), sig);
    ended(code, desc, if code == 0 { None } else { tail_str })
}
