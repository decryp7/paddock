//! Idle cost of the built-in plugin processes (research note `docs/research/2026-10-28-cpu-memory.md`): warms every
//! built-in up as the app does at start, then idles for `DEBRIEF_PERF_IDLE_S` seconds (default 60), printing
//! `PHASE <name> <unix ms>` lines so an outside sampler (physical footprint, CPU time and wake-ups of this process and
//! its children) can attribute its samples. This process stands in for the app: it runs the host's memory watchers.
//! Does nothing without `DEBRIEF_PERF_PLUGINS`.

use crate::common;
use debrief_plugin_host::Builtins;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn phase(name: &str) {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    eprintln!("PHASE {name} {ms}");
}

#[test]
fn idle_plugins() {
    if std::env::var_os("DEBRIEF_PERF_PLUGINS").is_none() {
        eprintln!("skipped: set DEBRIEF_PERF_PLUGINS=1 to measure");
        return;
    }
    let idle = Duration::from_secs(std::env::var("DEBRIEF_PERF_IDLE_S").ok().and_then(|v| v.parse().ok()).unwrap_or(60));
    eprintln!("PID {}", std::process::id());
    phase("warm-up");
    let builtins = Builtins::load(common::binary());
    builtins.warm_up();
    let started = Instant::now();
    while builtins.processes().iter().any(|p| p.status().pid.is_none()) && started.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(50));
    }
    eprintln!("{} plugin processes up in {:?}", builtins.processes().len(), started.elapsed());
    phase("idle");
    std::thread::sleep(idle);
    phase("idle-end");
    builtins.shutdown();
}
