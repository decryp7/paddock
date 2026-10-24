//! The real probe reads this machine (read-only): plausible figures, nothing started, nothing written.

use paddock_jobs::time::SystemClock;
use paddock_metrics::{MachineProbe, Sampler, SystemProbe};
use std::sync::Arc;

#[test]
fn the_real_probe_reads_plausible_figures() {
    let mut probe = SystemProbe::default();
    assert!(probe.cpu_permille() <= 1000);
    let (used, total) = probe.memory();
    assert!(total > 0 && used <= total);
    assert!(!probe.mounts().is_empty());
    assert!(probe.process_memory(std::process::id()) > 0);
}

#[test]
fn a_sampler_on_the_real_probe_stamps_a_recent_wall_time() {
    let mut sampler =
        Sampler::new(SystemProbe::new(), Arc::new(SystemClock)).watch_process(std::process::id());
    let s = sampler.sample();
    assert!(s.at > 1_700_000_000_000, "stamped with the wall clock");
    assert!(s.own_mem_bytes > 0);
}
