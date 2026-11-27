//! The `throttle` program: arguments and environment in, ports wired to the real clock, process table and load.

use paddock_throttle::cli::{self, Cli, USAGE};
use paddock_throttle::clock::{Clock, SystemClock};
use paddock_throttle::config::Config;
use paddock_throttle::exit::{self, process_exit};
use paddock_throttle::gate::SysLoad;
use paddock_throttle::lock::{Request, Throttle};
use paddock_throttle::log::Logger;
use paddock_throttle::procs::OsProcs;
use paddock_throttle::runner::{RunOpts, announce, run};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// One variable of the environment; `HEAVY_X` stands in for `THROTTLE_X` until the old names are retired (`config::aliased`).
fn env_get(k: &str) -> Option<String> {
    let raw = |k: &str| std::env::var(k).ok();
    paddock_throttle::config::aliased(&raw, k).filter(|v| !v.is_empty())
}

/// Stop flags: (interrupt, terminate, hangup).
fn stop_flags() -> (Arc<AtomicBool>, Arc<AtomicBool>, Arc<AtomicBool>) {
    let (int, term, hup) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    #[cfg(unix)]
    {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
        let _ = signal_hook::flag::register(SIGINT, Arc::clone(&int));
        let _ = signal_hook::flag::register(SIGTERM, Arc::clone(&term));
        let _ = signal_hook::flag::register(SIGHUP, Arc::clone(&hup));
    }
    #[cfg(windows)]
    {
        let i = Arc::clone(&int);
        let _ = ctrlc::set_handler(move || i.store(true, Ordering::SeqCst));
    }
    (int, term, hup)
}

/// The dev-targets folder by the tools' rule (docs/dev-setup.md, "Where things live"; `paddock_throttle::capture::dev_targets_of`):
/// `PADDOCK_DEV_TARGETS`, else the parent of `git config paddock.worktreeRoot` read from the current folder, else of
/// `PADDOCK_WORKTREE_ROOT`. `None` when none gives one: the run logs then go under the lock folder.
fn dev_targets() -> Option<std::path::PathBuf> {
    use paddock_throttle::capture::{DEV_TARGETS_ENV, dev_targets_of};
    if let Some(v) = env_get(DEV_TARGETS_ENV) {
        return dev_targets_of(Some(&v), None);
    }
    let mut git = paddock_proc::command("git");
    git.args(["config", "--get", "paddock.worktreeRoot"]);
    let from_git = paddock_proc::run_bounded(
        git,
        paddock_proc::Limits::new(std::time::Duration::from_secs(10)).max_output(1 << 16),
    )
    .ok()
    .filter(paddock_proc::Output::success)
    .map(|o| o.stdout_text().trim().to_string())
    .filter(|v| !v.is_empty());
    let root = from_git.or_else(|| env_get("PADDOCK_WORKTREE_ROOT"));
    dev_targets_of(None, root.as_deref())
}

fn main() {
    let code = real_main();
    // The full 32 bits on Windows (NTSTATUS), the low byte (never a false success) elsewhere.
    std::process::exit(process_exit(code, cfg!(windows)));
}

fn real_main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let pid = std::process::id();
    // One correlation id per run; an id handed in by a caller (a dev tool, CI) is reused and passed on to the command.
    let run_id = env_get("DEVKIT_RUN_ID").unwrap_or_else(|| format!("{:x}-{:x}", clock.now_micros(), pid));
    let parsed = match cli::parse(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("throttle: {e}\n{USAGE}");
            announce(&run_id, None, exit::USAGE, &e, "fix the command line (throttle --help)");
            return exit::USAGE;
        }
    };
    match &parsed {
        Cli::Help => {
            print!("{USAGE}");
            return 0;
        }
        Cli::Version => {
            println!("throttle {}", env!("CARGO_PKG_VERSION"));
            return 0;
        }
        _ => {}
    }
    // The dev-targets folder is derived once: the run logs go under it, and `<dev-targets>/throttle` is an old lock folder.
    let dev_targets = if matches!(parsed, Cli::Dir) { None } else { dev_targets() };
    let host = paddock_throttle::places::Host::real(dev_targets.clone());
    let (mut cfg, warnings) = Config::from_env(&env_get, &host);
    if let Cli::Run(r) = &parsed {
        if let Some(g) = r.gate_load {
            cfg.gate_load = g;
        }
        if let Some(s) = r.slots {
            cfg.slots = s;
        }
        cfg.verbose |= r.verbose;
        if let Some(f) = &r.log_file {
            cfg.log_file = Some(f.into());
        }
    }
    if matches!(parsed, Cli::Dir) {
        // Only the folder on stdout (a caller reads it); what is wrong with a settings file goes to stderr.
        println!("{}", cfg.root.display());
        for w in &warnings {
            eprintln!("throttle: {w}");
        }
        return 0;
    }
    if matches!(parsed, Cli::Run(_)) && cfg.runs.is_none() {
        cfg.runs = dev_targets.map(|d| d.join("runs"));
    }
    let log = Arc::new(Logger::new(
        run_id.clone(),
        pid,
        Arc::clone(&clock),
        Some(cfg.log_path(&paddock_throttle::timefmt::day_text(clock.now_secs()))),
        cfg.verbose,
    ));
    let say: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(|m: &str| eprintln!("throttle: {m}"));
    for w in &warnings {
        say(w);
        log.warn("throttle.config", w, &[]);
    }
    let h = Throttle::new(cfg, clock, Arc::new(OsProcs), Arc::new(SysLoad::default()), log, pid, say);
    match parsed {
        Cli::Status => {
            print!("{}", h.status());
            0
        }
        Cli::Mark { name, pid, remove } => {
            if remove {
                h.unmark(&name, pid);
                0
            } else {
                match h.mark(&name, pid) {
                    Ok(_) => 0,
                    Err(e) => {
                        eprintln!("throttle: {e}");
                        announce(
                            &run_id,
                            h.log.file().map(PathBuf::as_path),
                            exit::LOCK_NOT_TAKEN,
                            &e,
                            "free space or fix the folder's rights, then run it again",
                        );
                        exit::LOCK_NOT_TAKEN
                    }
                }
            }
        }
        Cli::Run(r) => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let home = env_get("HOME").or_else(|| env_get("USERPROFILE")).map(PathBuf::from);
            if !h.cfg.allow_home_target {
                let mut dirs = paddock_throttle::guard::build_dirs(
                    env_get("CARGO_TARGET_DIR").as_deref(),
                    env_get("CARGO_BUILD_TARGET_DIR").as_deref(),
                    &r.command,
                );
                let cargo_home = env_get("CARGO_HOME")
                    .map(PathBuf::from)
                    .or_else(|| home.as_ref().map(|p| p.join(".cargo")));
                dirs.extend(paddock_throttle::guard::config_target_dirs(&cwd, cargo_home.as_deref()));
                if let Some(v) = paddock_throttle::guard::violation(&dirs, home.as_deref(), &cwd) {
                    let hint =
                        env_get("THROTTLE_BUILD_ROOT_HINT").unwrap_or_else(|| "a folder outside your home, e.g. on a work disk".into());
                    eprintln!("ER-14: build dir inside ~ ({v}); use {hint} (THROTTLE_ALLOW_HOME_TARGET=1 overrides)");
                    h.log.warn(
                        "throttle.guard",
                        "refused: build dir inside the home folder",
                        &[("Dir", serde_json::json!(v))],
                    );
                    announce(
                        &run_id,
                        h.log.file().map(PathBuf::as_path),
                        exit::REFUSED,
                        &format!("refused: the build folder {v} is inside the home folder (ER-14)"),
                        &format!("use {hint}, or set THROTTLE_ALLOW_HOME_TARGET=1"),
                    );
                    return exit::REFUSED;
                }
            }
            let cwd_text = cwd.to_string_lossy().into_owned();
            let row = env_get("THROTTLE_ROW")
                .map(|k| paddock_throttle::history::clean_key(&k))
                .filter(|k| !k.is_empty());
            let req = Request {
                name: paddock_throttle::timefmt::oneline(&r.name),
                cwd: paddock_throttle::timefmt::oneline(&cwd_text),
                cmdline: paddock_throttle::mask::mask_cmd(&r.command),
                high: r.high,
                expected_secs: row
                    .as_deref()
                    .and_then(|k| paddock_throttle::history::History::of(&h.cfg).expected_secs(k)),
                row,
            };
            let (int, term, hup) = stop_flags();
            let stop = move || exit::stop_code(int.load(Ordering::SeqCst), term.load(Ordering::SeqCst), hup.load(Ordering::SeqCst));
            let opts = RunOpts {
                gate: !r.no_gate,
                low: r.low,
                queue_max: r.queue_max.map(std::time::Duration::from_secs_f64),
                queue_stall: r.queue_stall.map(std::time::Duration::from_secs_f64),
                run_max: r.run_max.map(std::time::Duration::from_secs_f64),
                stall: r.stall.map(std::time::Duration::from_secs_f64).or(h.cfg.stall),
            };
            // Inside a run that holds a slot (THROTTLE_HELD from a live holder): run in that slot, never queue for it (ER-43).
            if let Some(outer) = paddock_throttle::runner::held_by(&h, env_get(paddock_throttle::runner::HELD_VAR).as_deref()) {
                return paddock_throttle::runner::run_nested(&h, &req.name, &r.command, opts, &stop, &outer);
            }
            run(&h, req, &r.command, opts, &stop)
        }
        _ => exit::USAGE,
    }
}
