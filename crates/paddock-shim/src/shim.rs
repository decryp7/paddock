//! One shim run: decide, take a slot, run the real tool with the same arguments and streams, report events.

use crate::event::{RunEvent, Sinks};
use crate::project::{self, Project};
use crate::resolve::{Own, find_real};
use crate::sinks::{RunLogSink, StallReport};
use paddock_proc::{ProcessId, RunState, command, kill_tree, run_state, tree_cpu_time};
use paddock_throttle::boot::{self, env_get};
use paddock_throttle::clock::{Clock, SystemClock};
use paddock_throttle::exit::{self, describe_exit};
use paddock_throttle::gate::step_env;
use paddock_throttle::hang::{HangWatch, Verdict};
use paddock_throttle::lock::{Request, Throttle, WaitNote};
use paddock_throttle::procs::OsProcs;
use paddock_throttle::runner::{HELD_VAR, RunOpts, Stop, admit, held_by_procs};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The tools that have a shim. Adding one is adding its name here (open/closed).
pub const SHIMMED: &[&str] = &["cargo", "npm", "npx", "node", "playwright", "ng"];

/// The default stall limit: a quarter of an hour of a tool tree using no CPU is unusual for a build; a long link or a network wait is
/// reported, never ended (observe-by-default).
const DEFAULT_STALL_SECS: u64 = 600;

/// The tool a program was called as (`/x/shims/cargo`, `cargo.exe`, `cargo`), when it is one that has a shim.
pub fn called_as(argv0: Option<&OsStr>) -> Option<&'static str> {
    let stem = Path::new(argv0?).file_stem()?.to_str()?;
    SHIMMED
        .iter()
        .copied()
        .find(|n| n.eq_ignore_ascii_case(stem))
}

/// The folder a program was started from when its path was explicit (a bare name came through `PATH`: unknown).
fn explicit_dir(argv0: &OsStr) -> Option<PathBuf> {
    let p = Path::new(argv0);
    let parent = p.parent().filter(|d| !d.as_os_str().is_empty())?;
    std::path::absolute(parent).ok()
}

fn off(get: &dyn Fn(&str) -> Option<String>) -> bool {
    get("PADDOCK_SHIM").is_some_and(|v| {
        matches!(
            v.to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "no"
        )
    })
}

/// What the shim says on standard error when a run waits for a slot: its own sentence, built from the typed note.
pub fn wait_line(tool: &str, w: &WaitNote) -> String {
    format!(
        "{tool} is waiting for a slot ({} s so far): {}",
        w.secs, w.cause
    )
}

/// What the run does about the shim itself.
#[derive(Debug, PartialEq, Eq)]
enum Plan {
    /// Run the real tool at once: nothing is taken or recorded.
    Straight,
    /// Take a slot, run it supervised, report it.
    Supervise,
}

fn plan(opted_out: bool, switched_off: bool, nested: Option<&str>) -> Plan {
    // Three reasons, one outcome: the real tool runs at once with nothing taken or recorded.
    if switched_off || opted_out || nested.is_some() {
        Plan::Straight
    } else {
        Plan::Supervise
    }
}

/// Runs the shim for `tool` (called as `argv0`) with `args`: the exit code to leave with. `stop` is the caller's signal flags.
pub fn run(tool: &'static str, argv0: &OsStr, args: &[OsString], stop: Stop<'_>) -> i32 {
    let own = Own {
        exe: std::env::current_exe().unwrap_or_default(),
        invoked_dir: explicit_dir(argv0),
    };
    let path = std::env::var_os("PATH").unwrap_or_default();
    let Some(real) = find_real(tool, &path, &own) else {
        eprintln!(
            "paddock-shim: no real `{tool}` found on PATH past the shim folder. Next: install {tool}, or put its folder on PATH after the shims folder"
        );
        return exit::NOT_FOUND;
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let project = project::detect(&cwd, &|p| p.exists());
    let nested = held_by_procs(&OsProcs, env_get(HELD_VAR).as_deref());
    match plan(project.opted_out, off(&env_get), nested.as_deref()) {
        Plan::Straight => straight(&real, args),
        Plan::Supervise => supervised(tool, &real, args, &cwd, &project, stop),
    }
}

/// The real tool with nothing in between. On Unix the shim becomes the tool (`exec`): the same process, signals and exit status.
fn straight(real: &Path, args: &[OsString]) -> i32 {
    let mut cmd = command(real);
    cmd.args(args);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let e = cmd.exec();
        eprintln!(
            "paddock-shim: cannot run {}: {e}. Next: check that the file is executable",
            real.display()
        );
        exit::NOT_RUNNABLE
    }
    #[cfg(not(unix))]
    {
        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        match cmd.status() {
            Ok(s) => describe_exit(s.code(), None).0,
            Err(e) => {
                eprintln!(
                    "paddock-shim: cannot run {}: {e}. Next: check that the file is executable",
                    real.display()
                );
                exit::NOT_RUNNABLE
            }
        }
    }
}

fn stall_limit(get: &dyn Fn(&str) -> Option<String>) -> Option<Duration> {
    let secs = get("PADDOCK_SHIM_STALL")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_STALL_SECS);
    (secs > 0).then(|| Duration::from_secs(secs))
}

fn supervised(
    tool: &'static str,
    real: &Path,
    args: &[OsString],
    cwd: &Path,
    project: &Project,
    stop: Stop<'_>,
) -> i32 {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let run_id = boot::run_id(clock.as_ref(), std::process::id());
    let (mut cfg, warnings, dev_targets) = boot::config(true);
    if cfg.runs.is_none() {
        cfg.runs = dev_targets.map(|d| d.join("runs"));
    }
    let runs = cfg.runs_dir();
    // The shim is silent while the run is admitted at once: the tool's own output is all the user sees. It speaks only about a wait,
    // from the typed note, never from the wording of throttle's line (ER-42).
    let say: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(|_: &str| {});
    let mut h: Throttle = boot::throttle_with(cfg, &warnings, &run_id, say);
    h.wait_hook = Some(Arc::new(|w: &WaitNote| {
        eprintln!("paddock-shim: {}", wait_line(tool, w))
    }));
    let train = env_get("THROTTLE_PRIORITY").is_some_and(|v| v.eq_ignore_ascii_case("train"));
    // The record names the tool and the project, never the arguments (ER-27).
    let req = Request {
        name: tool.to_string(),
        cwd: paddock_throttle::timefmt::oneline(&cwd.to_string_lossy()),
        cmdline: tool.to_string(),
        high: train,
        expected_secs: None,
        row: None,
    };
    let opts = RunOpts {
        gate: true,
        low: false,
        queue_max: None,
        queue_stall: None,
        run_max: None,
        stall: None,
    };
    let admitted = match admit(&h, &req, &opts, stop) {
        Ok(a) => Some(a),
        Err(code) => {
            if let Some(stopped) = stop() {
                return stopped;
            }
            // Degrade, don't fail: the tool still runs, without a slot, and the cause is in the log.
            h.log.warn(
                "throttle.shim",
                "no slot could be taken; running the tool without one",
                &[("ExitCode", serde_json::json!(code))],
            );
            None
        }
    };
    let mut sinks = Sinks::new();
    sinks.add(Box::new(RunLogSink::new(
        &runs,
        Arc::clone(&clock),
        Arc::clone(&h.log),
    )));
    sinks.add(Box::new(StallReport::new(
        Box::new(|m| eprintln!("paddock-shim: {m}")),
        Arc::clone(&h.log),
    )));

    let share = admitted.as_ref().map_or(1, |a| a.acq.share());
    let mut cmd = command(real);
    cmd.args(args);
    cmd.env("DEVKIT_RUN_ID", &run_id);
    if admitted.is_some() {
        let held = format!("{run_id}:{}", h.pid);
        cmd.env(HELD_VAR, &held).env("HEAVY_HELD", &held);
    }
    // ER-48: the tool's threads are its share of the cores; a variable the caller set wins (ER-47).
    cmd.envs(step_env(h.cfg.cpus, share, &|k| {
        std::env::var_os(k).map(|v| v.to_string_lossy().into_owned())
    }));
    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    #[cfg(unix)]
    {
        use std::io::IsTerminal;
        use std::os::unix::process::CommandExt;
        // A terminal user keeps Ctrl-C going straight to the tool; otherwise it has its own group so a stop reaches its whole tree.
        if !std::io::stdin().is_terminal() {
            cmd.process_group(0);
        }
    }
    let start_us = clock.now_micros();
    let t0 = Instant::now();
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "paddock-shim: cannot run {}: {e}. Next: check that the file is executable",
                real.display()
            );
            return if e.kind() == std::io::ErrorKind::NotFound {
                exit::NOT_FOUND
            } else {
                exit::NOT_RUNNABLE
            };
        }
    };
    sinks.emit(&RunEvent::Started {
        tool,
        project: &project.name,
        start_us,
    });
    h.log.info(
        "throttle.shim",
        "tool started",
        &[("ChildPid", serde_json::json!(child.id()))],
    );
    let id = ProcessId::of(child.id());
    let end_on_stall =
        env_get("PADDOCK_SHIM_STALL_ACTION").is_some_and(|v| v.eq_ignore_ascii_case("end"));
    let limit = stall_limit(&env_get);
    let since = || u64::try_from(t0.elapsed().as_micros()).unwrap_or(u64::MAX);
    let mut watch = limit.zip(id).map(|(l, _)| HangWatch::new(l, since()));
    let (mut stopped, mut ended_by_stall, mut deadline) = (None, false, None::<Instant>);
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
            if let Some(id) = id {
                let _ = kill_tree(id);
            }
            deadline = Some(Instant::now() + Duration::from_secs(10));
        }
        if stopped.is_none()
            && !ended_by_stall
            && let (Some(w), Some(id)) = (watch.as_mut(), id)
        {
            let now = since();
            // Nothing is piped, so the byte count stays 0: only the CPU time of the tool's tree can show life.
            if let Verdict::Hung(quiet) = w.observe(now, 0, || {
                tree_cpu_time(id).map(|d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
            }) {
                #[expect(
                    clippy::disallowed_methods,
                    reason = "a production read after the event that settles it: the hang verdict means the whole stall window passed with no CPU, and a stop lasts until the tool is resumed, so the read follows the window instead of racing a change; it only excuses a paused tool (App Tamer) and any other state ends it, as before"
                )]
                let paused = run_state(id) == RunState::Stopped;
                if paused {
                    w.touch(now);
                } else {
                    sinks.emit(&RunEvent::Stalled {
                        tool,
                        project: &project.name,
                        quiet,
                        pid: child.id(),
                        ends: end_on_stall,
                    });
                    if end_on_stall {
                        ended_by_stall = true;
                        let _ = kill_tree(id);
                        deadline = Some(Instant::now() + Duration::from_secs(10));
                    } else {
                        // Reported once per limit, not once per poll.
                        w.touch(now);
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
    let code = if let Some(c) = stopped {
        c
    } else if ended_by_stall {
        exit::HUNG
    } else if let Some(s) = status {
        #[cfg(unix)]
        let sig = std::os::unix::process::ExitStatusExt::signal(&s);
        #[cfg(not(unix))]
        let sig = None;
        describe_exit(s.code(), sig).0
    } else {
        1
    };
    sinks.emit(&RunEvent::Ended {
        tool,
        project: &project.name,
        start_us,
        duration: t0.elapsed(),
        code,
    });
    drop(admitted);
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_is_a_shim_by_the_name_it_was_called_as() {
        assert_eq!(called_as(Some(OsStr::new("/x/shims/cargo"))), Some("cargo"));
        assert_eq!(called_as(Some(OsStr::new("npm.exe"))), Some("npm"));
        assert_eq!(called_as(Some(OsStr::new("NPX.EXE"))), Some("npx"));
        assert_eq!(called_as(Some(OsStr::new("throttle"))), None);
        assert_eq!(called_as(Some(OsStr::new("cargo-nextest"))), None);
        assert_eq!(called_as(None), None);
    }

    #[test]
    fn the_plan_passes_straight_through_when_off_opted_out_or_nested() {
        assert_eq!(plan(false, false, None), Plan::Supervise);
        assert!(plan(true, false, None) == Plan::Straight);
        assert!(plan(false, true, None) == Plan::Straight);
        assert!(plan(false, false, Some("r:1")) == Plan::Straight);
    }

    #[test]
    fn the_stall_limit_has_a_default_and_zero_turns_it_off() {
        assert_eq!(stall_limit(&|_| None), Some(Duration::from_secs(600)));
        assert_eq!(stall_limit(&|_| Some("0".into())), None);
        assert_eq!(
            stall_limit(&|_| Some("5".into())),
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            stall_limit(&|_| Some("x".into())),
            Some(Duration::from_secs(600))
        );
    }

    #[test]
    fn the_wait_line_is_built_from_the_note_and_not_from_throttles_wording() {
        let w = WaitNote {
            who: "cargo".into(),
            secs: 7,
            cause: "held by someone".into(),
        };
        assert_eq!(
            wait_line("cargo", &w),
            "cargo is waiting for a slot (7 s so far): held by someone"
        );
    }

    #[test]
    fn the_switch_reads_off_in_any_case() {
        assert!(off(&|_| Some("OFF".into())));
        assert!(!off(&|_| Some("on".into())));
        assert!(!off(&|_| None));
    }

    #[test]
    fn an_explicit_path_names_the_shim_folder_and_a_bare_name_does_not() {
        assert!(explicit_dir(OsStr::new("cargo")).is_none());
        assert!(explicit_dir(OsStr::new("/x/shims/cargo")).is_some_and(|d| d.ends_with("shims")));
    }
}
