//! The `heavy` program: arguments and environment in, ports wired to the real clock, process table and load.

use heavy::cli::{self, Cli, USAGE};
use heavy::clock::{Clock, SystemClock};
use heavy::config::{Config, LEGACY_ROOT};
use heavy::exit::{self, process_exit};
use heavy::gate::SysLoad;
use heavy::lock::{Heavy, Request};
use heavy::log::Logger;
use heavy::procs::OsProcs;
use heavy::runner::{RunOpts, announce, run};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

fn env_get(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
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

fn main() {
    let code = real_main();
    // The full 32 bits on Windows (NTSTATUS), the low byte (never a false success) elsewhere.
    std::process::exit(process_exit(code, cfg!(windows)));
}

fn real_main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut cfg, warnings) = Config::from_env(&env_get, Path::new(LEGACY_ROOT).is_dir());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let pid = std::process::id();
    // One correlation id per run; an id handed in by a caller (a dev tool, CI) is reused and passed on to the command.
    let run_id = env_get("DEVKIT_RUN_ID").unwrap_or_else(|| format!("{:x}-{:x}", clock.now_micros(), pid));
    let parsed = match cli::parse(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("heavy: {e}\n{USAGE}");
            announce(&run_id, None, exit::USAGE, &e, "fix the command line (heavy --help)");
            return exit::USAGE;
        }
    };
    match &parsed {
        Cli::Help => {
            print!("{USAGE}");
            return 0;
        }
        Cli::Version => {
            println!("heavy {}", env!("CARGO_PKG_VERSION"));
            return 0;
        }
        _ => {}
    }
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
    let log = Arc::new(Logger::new(
        run_id.clone(),
        pid,
        Arc::clone(&clock),
        Some(cfg.log_path(&heavy::timefmt::day_text(clock.now_secs()))),
        cfg.verbose,
    ));
    let say: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(|m: &str| eprintln!("heavy: {m}"));
    for w in &warnings {
        say(w);
        log.warn("heavy.config", w, &[]);
    }
    let h = Heavy::new(cfg, clock, Arc::new(OsProcs), Arc::new(SysLoad), log, pid, say);
    match parsed {
        Cli::Status => {
            print!("{}", h.status());
            0
        }
        Cli::Run(r) => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let home = env_get("HOME").or_else(|| env_get("USERPROFILE")).map(PathBuf::from);
            if !h.cfg.allow_home_target {
                let mut dirs = heavy::guard::build_dirs(
                    env_get("CARGO_TARGET_DIR").as_deref(),
                    env_get("CARGO_BUILD_TARGET_DIR").as_deref(),
                    &r.command,
                );
                let cargo_home = env_get("CARGO_HOME")
                    .map(PathBuf::from)
                    .or_else(|| home.as_ref().map(|p| p.join(".cargo")));
                dirs.extend(heavy::guard::config_target_dirs(&cwd, cargo_home.as_deref()));
                if let Some(v) = heavy::guard::violation(&dirs, home.as_deref(), &cwd) {
                    let hint = env_get("HEAVY_BUILD_ROOT_HINT").unwrap_or_else(|| "a folder outside your home, e.g. on a work disk".into());
                    eprintln!("ER-14: build dir inside ~ ({v}); use {hint} (HEAVY_ALLOW_HOME_TARGET=1 overrides)");
                    h.log.warn(
                        "heavy.guard",
                        "refused: build dir inside the home folder",
                        &[("Dir", serde_json::json!(v))],
                    );
                    announce(
                        &run_id,
                        h.log.file().map(PathBuf::as_path),
                        exit::REFUSED,
                        &format!("refused: the build folder {v} is inside the home folder (ER-14)"),
                        &format!("use {hint}, or set HEAVY_ALLOW_HOME_TARGET=1"),
                    );
                    return exit::REFUSED;
                }
            }
            let cwd_text = cwd.to_string_lossy().into_owned();
            let req = Request {
                name: heavy::timefmt::oneline(&r.name),
                cwd: heavy::timefmt::oneline(&cwd_text),
                cmdline: heavy::mask::mask_cmd(&r.command),
                high: r.high,
            };
            let (int, term, hup) = stop_flags();
            let stop = move || exit::stop_code(int.load(Ordering::SeqCst), term.load(Ordering::SeqCst), hup.load(Ordering::SeqCst));
            run(
                &h,
                req,
                &r.command,
                RunOpts {
                    gate: !r.no_gate,
                    low: r.low,
                    queue_max: r.queue_max.map(std::time::Duration::from_secs_f64),
                    run_max: r.run_max.map(std::time::Duration::from_secs_f64),
                },
                &stop,
            )
        }
        _ => exit::USAGE,
    }
}
