//! Booting a participant from the real environment: the one place the program wiring lives, so the `throttle` binary and the shim
//! (`paddock-shim`) start the same way (ER-22). Real clock, process table and load; the environment read through the old-name aliases.

use crate::clock::{Clock, SystemClock};
use crate::config::Config;
use crate::gate::SysLoad;
use crate::lock::Throttle;
use crate::log::Logger;
use crate::places::Host;
use crate::procs::OsProcs;
use std::path::PathBuf;
use std::sync::Arc;

/// One variable of the environment; `HEAVY_X` stands in for `THROTTLE_X` until the old names are retired (`config::aliased`).
pub fn env_get(k: &str) -> Option<String> {
    let raw = |k: &str| std::env::var(k).ok();
    crate::config::aliased(&raw, k).filter(|v| !v.is_empty())
}

/// The dev-targets folder by the tools' rule (docs/dev-setup.md, "Where things live"; `crate::capture::dev_targets_of`):
/// `PADDOCK_DEV_TARGETS`, else the parent of `git config paddock.worktreeRoot` read from the current folder, else of
/// `PADDOCK_WORKTREE_ROOT`. `None` when none gives one: the run logs then go under the lock folder.
pub fn dev_targets() -> Option<std::path::PathBuf> {
    use crate::capture::{DEV_TARGETS_ENV, dev_targets_of};
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

/// The configuration of this run from the environment, with the dev-targets folder it was derived from and the settings warnings.
/// `with_dev_targets` is false for a command that only prints the lock folder.
pub fn config(with_dev_targets: bool) -> (Config, Vec<String>, Option<PathBuf>) {
    let dev_targets = if with_dev_targets {
        dev_targets()
    } else {
        None
    };
    let host = Host::real(dev_targets.clone());
    let (cfg, warnings) = Config::from_env(&env_get, &host);
    (cfg, warnings, dev_targets)
}

/// A correlation id for this run: the caller's `DEVKIT_RUN_ID` when one was handed in, else a fresh one.
pub fn run_id(clock: &dyn Clock, pid: u32) -> String {
    env_get("DEVKIT_RUN_ID").unwrap_or_else(|| format!("{:x}-{:x}", clock.now_micros(), pid))
}

/// A participant over the real ports: its troubleshooting log under the lock folder, `warnings` said and logged, and its lines on stderr
/// prefixed `who`.
pub fn throttle(cfg: Config, warnings: &[String], run_id: &str, who: &'static str) -> Throttle {
    throttle_with(
        cfg,
        warnings,
        run_id,
        Arc::new(move |m: &str| eprintln!("{who}: {m}")),
    )
}

/// [`throttle`] with the human-readable lines handed to `say` (a participant that stays quiet unless something is worth saying).
pub fn throttle_with(
    cfg: Config,
    warnings: &[String],
    run_id: &str,
    say: Arc<dyn Fn(&str) + Send + Sync>,
) -> Throttle {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let pid = std::process::id();
    let log = Arc::new(Logger::new(
        run_id.to_string(),
        pid,
        Arc::clone(&clock),
        Some(cfg.log_path(&crate::timefmt::day_text(clock.now_secs()))),
        cfg.verbose,
    ));
    for w in warnings {
        say(w);
        log.warn("throttle.config", w, &[]);
    }
    Throttle::new(
        cfg,
        clock,
        Arc::new(OsProcs),
        Arc::new(SysLoad::default()),
        log,
        pid,
        say,
    )
}
