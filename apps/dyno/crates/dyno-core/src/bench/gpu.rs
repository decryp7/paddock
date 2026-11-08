//! The GPU test's method (ADR 0030 section 1): every submission is sized to take about 100 ms and never more than 250 ms, found
//! from a short calibration and halved whenever one runs long (Windows resets a GPU whose work runs past the driver timeout, about
//! 2 s, and a reset takes the display with it). Pure: the durations come in, the next amount of work goes out. Also what counts as
//! a software renderer, which is reported as such and never compared with hardware.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// What a submission is sized for, ms.
pub const TARGET_MS: u64 = 100;
/// No submission is planned to take longer than this, ms.
pub const MAX_MS: u64 = 250;
/// Submissions before the counted ones (shader compile, clocks ramping).
pub const WARMUP_SUBMISSIONS: u32 = 2;
/// A measurement keeps submitting until at least this much work time is counted, ms.
pub const MEASURE_MIN_MS: u64 = 1_000;
/// Calibration stops after this many tries.
pub const CALIBRATION_TRIES: u32 = 12;
/// Repetitions of every GPU metric.
pub const REPETITIONS: u32 = 5;
/// The amount of work grows by at most this factor from one submission to the next.
pub const MAX_GROWTH: u64 = 16;

/// The kind of adapter, as the graphics API reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AdapterKind {
    /// A discrete GPU.
    Discrete,
    /// A GPU that shares the CPU's memory.
    Integrated,
    /// A virtual GPU of a virtual machine.
    Virtual,
    /// A software renderer.
    Cpu,
    /// The API does not say.
    Other,
}

/// One graphics adapter, as the picker lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuAdapter {
    /// Its position in the runner's list (what `--adapter` takes).
    pub index: u32,
    /// The name the driver gives.
    pub name: String,
    /// Its kind.
    pub kind: AdapterKind,
    /// The API: Metal, Dx12 or Vulkan.
    pub backend: String,
    /// The driver, when it says.
    pub driver: String,
}

/// Whether the adapter is a software renderer (llvmpipe, WARP, SwiftShader and any adapter of kind Cpu).
pub fn is_software(kind: AdapterKind, name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    kind == AdapterKind::Cpu
        || ["llvmpipe", "softpipe", "swiftshader", "warp", "microsoft basic render", "software"]
            .iter()
            .any(|s| n.contains(s))
}

/// The words for a software adapter.
pub const SOFTWARE_NOTE: &str = "Software renderer, not a GPU. Its numbers are not comparable with a graphics card's.";

/// The default adapter: the discrete one, else an integrated one, else the first that is not software; `None` if all are software
/// or there are none (the first software one is then offered for information, but not chosen for a comparison).
pub fn default_adapter(list: &[GpuAdapter]) -> Option<&GpuAdapter> {
    let hw = |a: &&GpuAdapter| !is_software(a.kind, &a.name);
    list.iter()
        .filter(hw)
        .find(|a| a.kind == AdapterKind::Discrete)
        .or_else(|| list.iter().filter(hw).find(|a| a.kind == AdapterKind::Integrated))
        .or_else(|| list.iter().find(hw))
}

/// The amount of work (in the kernel's own unit) for the next submission, from the work just done and how long it took. It aims at
/// [`TARGET_MS`], grows by at most [`MAX_GROWTH`], and a submission that ran past [`MAX_MS`] is halved at least, so the next one is
/// planned for less than the cap.
pub fn next_work(work: u64, took: Duration) -> u64 {
    let ms = took.as_secs_f64() * 1000.0;
    let ideal = if ms <= 0.0 {
        work.saturating_mul(MAX_GROWTH)
    } else {
        (work as f64 * TARGET_MS as f64 / ms) as u64
    };
    let mut next = ideal.clamp(1, work.saturating_mul(MAX_GROWTH).max(1));
    if ms > MAX_MS as f64 {
        next = next.min((work / 2).max(1));
    }
    next
}

/// Whether the calibration may stop: the last submission was close to the target.
pub fn calibrated(took: Duration) -> bool {
    let ms = took.as_millis() as u64;
    (TARGET_MS * 6 / 10..=TARGET_MS * 15 / 10).contains(&ms)
}

/// Finds the amount of work that takes about [`TARGET_MS`]: `run(work)` does one submission and returns how long it took.
pub fn calibrate(start: u64, mut run: impl FnMut(u64) -> Duration) -> u64 {
    let mut work = start.max(1);
    for _ in 0..CALIBRATION_TRIES {
        let took = run(work);
        if calibrated(took) {
            return work;
        }
        work = next_work(work, took);
    }
    work
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GPU that does `rate` units of work per millisecond.
    fn gpu(rate: f64) -> impl FnMut(u64) -> Duration {
        move |w| Duration::from_secs_f64(w as f64 / rate / 1000.0)
    }

    #[test]
    fn calibration_finds_about_a_hundred_milliseconds_on_fast_and_slow_gpus() {
        for rate in [5.0, 500.0, 50_000.0] {
            let w = calibrate(1, gpu(rate));
            let ms = w as f64 / rate;
            assert!((60.0..=150.0).contains(&ms), "rate {rate}: {ms} ms");
        }
    }

    #[test]
    fn a_slow_submission_halves_the_work_and_the_next_is_never_planned_over_the_cap() {
        // 400 ms for 1000 units: the ideal is 250 units, and at least a halving applies.
        assert_eq!(next_work(1000, Duration::from_millis(400)), 250);
        assert_eq!(next_work(1000, Duration::from_millis(260)), 384);
        assert!(next_work(1000, Duration::from_millis(260)) as f64 * 0.26 <= MAX_MS as f64);
        // A 2 s submission (a driver reset is near) cuts the work to a twentieth.
        assert_eq!(next_work(1000, Duration::from_secs(2)), 50);
        let mut w = 1_000_000_u64;
        let rate = 1_000.0; // units per ms: 1000 ms for the first try
        for _ in 0..12 {
            w = next_work(w, Duration::from_secs_f64(w as f64 / rate / 1000.0));
        }
        assert!(w as f64 / rate <= MAX_MS as f64, "settled at {} ms", w as f64 / rate);
    }

    #[test]
    fn work_grows_by_at_most_sixteen_times_and_never_reaches_zero() {
        assert_eq!(next_work(100, Duration::from_micros(10)), 1600);
        assert_eq!(next_work(100, Duration::ZERO), 1600);
        assert_eq!(next_work(1, Duration::from_secs(10)), 1);
        assert_eq!(next_work(0, Duration::from_secs(1)), 1);
    }

    #[test]
    fn software_renderers_are_recognised_by_kind_and_by_name() {
        assert!(is_software(AdapterKind::Cpu, "anything"));
        assert!(is_software(AdapterKind::Other, "llvmpipe (LLVM 17.0.6, 256 bits)"));
        assert!(is_software(AdapterKind::Other, "Microsoft Basic Render Driver"));
        assert!(is_software(AdapterKind::Virtual, "SwiftShader Device"));
        assert!(!is_software(AdapterKind::Discrete, "NVIDIA GeForce RTX 4070"));
        assert!(!is_software(AdapterKind::Integrated, "Apple M4 Pro"));
    }

    fn ad(i: u32, name: &str, kind: AdapterKind) -> GpuAdapter {
        GpuAdapter {
            index: i,
            name: name.into(),
            kind,
            backend: "Vulkan".into(),
            driver: String::new(),
        }
    }

    #[test]
    fn the_default_adapter_is_discrete_then_integrated_and_never_software() {
        let both = [
            ad(0, "Intel UHD", AdapterKind::Integrated),
            ad(1, "NVIDIA", AdapterKind::Discrete),
            ad(2, "llvmpipe", AdapterKind::Cpu),
        ];
        assert_eq!(default_adapter(&both).map(|a| a.index), Some(1));
        assert_eq!(default_adapter(&both[..1]).map(|a| a.index), Some(0));
        assert!(default_adapter(&both[2..]).is_none(), "only software: no default");
        assert!(default_adapter(&[]).is_none());
    }
}
