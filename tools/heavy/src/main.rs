//! The `heavy` program: arguments and environment in, ports wired to the real clock, process table and load.

use heavy::cli::{self, Cli, RunArgs, USAGE};
use heavy::clock::{Clock, SystemClock};
use heavy::config::{Config, LEGACY_ROOT};
use heavy::gate::SysLoad;
use heavy::lock::{Heavy, Request};
use heavy::log::Logger;
use heavy::procs::OsProcs;
use heavy::runner::{RunOpts, run};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

fn env_get(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

/// Stop flags: (interrupt, terminate).
fn stop_flags() -> (Arc<AtomicBool>, Arc<AtomicBool>) {
    let (int, term) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    #[cfg(unix)]
    {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
        let _ = signal_hook::flag::register(SIGINT, Arc::clone(&int));
        let _ = signal_hook::flag::register(SIGTERM, Arc::clone(&term));
        let _ = signal_hook::flag::register(SIGHUP, Arc::clone(&term));
    }
    #[cfg(windows)]
    {
        let i = Arc::clone(&int);
        let _ = ctrlc::set_handler(move || i.store(true, Ordering::SeqCst));
    }
    (int, term)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let parsed = match cli::parse(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("heavy: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let (mut cfg, warnings) = Config::from_env(&env_get, std::path::Path::new(LEGACY_ROOT).is_dir());
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let run_args: Option<&RunArgs> = if let Cli::Run(r) = &parsed { Some(r) } else { None };
    if let Some(r) = run_args {
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
    match &parsed {
        Cli::Help => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Cli::Version => {
            println!("heavy {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        _ => {}
    }
    let pid = std::process::id();
    let run_id = format!("{:x}-{:x}", clock.now_micros(), pid);
    let log = Arc::new(Logger::new(
        run_id,
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
            ExitCode::SUCCESS
        }
        Cli::Run(r) => {
            let cwd = std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let home = env_get("HOME").or_else(|| env_get("USERPROFILE")).map(std::path::PathBuf::from);
            if !h.cfg.allow_home_target {
                let dirs = heavy::guard::build_dirs(env_get("CARGO_TARGET_DIR").as_deref(), &r.command);
                if let Some(v) = heavy::guard::violation(&dirs, home.as_deref(), std::path::Path::new(&cwd)) {
                    let hint = env_get("HEAVY_BUILD_ROOT_HINT").unwrap_or_else(|| "a folder outside your home, e.g. on a work disk".into());
                    eprintln!("ER-14: build dir inside ~ ({v}); use {hint} (HEAVY_ALLOW_HOME_TARGET=1 overrides)");
                    h.log.warn(
                        "heavy.guard",
                        "refused: build dir inside the home folder",
                        &[("Dir", serde_json::json!(v))],
                    );
                    return ExitCode::from(2);
                }
            }
            let req = Request {
                name: heavy::timefmt::oneline(&r.name),
                cwd: heavy::timefmt::oneline(&cwd),
                cmdline: heavy::mask::mask_cmd(&r.command),
                high: r.high,
            };
            let (int, term) = stop_flags();
            let stop = move || {
                if term.load(Ordering::SeqCst) {
                    Some(143)
                } else if int.load(Ordering::SeqCst) {
                    Some(130)
                } else {
                    None
                }
            };
            let code = run(
                &h,
                req,
                &r.command,
                RunOpts {
                    gate: !r.no_gate,
                    low: r.low,
                },
                &stop,
            );
            ExitCode::from(u8::try_from(code & 0xff).unwrap_or(1))
        }
        _ => ExitCode::from(2),
    }
}
