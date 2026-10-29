//! The benchmark harness skeleton (rule 48: every quoted benchmark is recorded in `docs/performance.md`).
//!
//! It needs no model: it times any closure with an injected clock, computes percentiles the way `performance.md`
//! reports them, judges them against budgets and renders the results block of the family template
//! (`docs/benchmarks.md`). **Runtime code never reads the budgets** (ADR 0031): they are benchmark constants only.

use std::cell::Cell;
use std::fmt::Write as _;
use std::time::{Duration, Instant};

/// The budgets of ADR 0001 and the design review, as named constants (benchmark use only; mirrored in
/// `docs/performance.md`). Changing a number is a one-line change.
pub mod budgets {
    use std::time::Duration;

    /// Gate: full-frame locate p95 on a GPU (CoreML or DirectML) at 1920 x 1200 (or 4K at 200 %).
    pub const LOCATE_GATE_GPU_P95: Duration = Duration::from_millis(500);
    /// Gate: full-frame locate p95 on the CPU at 1920 x 1200.
    pub const LOCATE_GATE_CPU_P95: Duration = Duration::from_secs(2);
    /// Gate: full-frame locate p95 on a GPU at 3840 x 2160 and 100 %.
    pub const LOCATE_GATE_GPU_P95_4K: Duration = Duration::from_secs(1);
    /// Gate: full-frame locate p95 on the CPU at 3840 x 2160 and 100 %.
    pub const LOCATE_GATE_CPU_P95_4K: Duration = Duration::from_secs(4);
    /// Target (not a gate): GPU locate p50.
    pub const LOCATE_TARGET_GPU_P50: Duration = Duration::from_millis(80);
    /// Target (not a gate): GPU locate p95.
    pub const LOCATE_TARGET_GPU_P95: Duration = Duration::from_millis(250);
    /// Target (not a gate): CPU locate p95.
    pub const LOCATE_TARGET_CPU_P95: Duration = Duration::from_millis(600);
    /// Model cold load.
    pub const MODEL_COLD_LOAD: Duration = Duration::from_millis(1500);
    /// HUD overhead on the server, per frame.
    pub const HUD_FRAME_OVERHEAD: Duration = Duration::from_millis(2);
    /// Largest change of step timing, in percent, that switching the trace on may cause.
    pub const HUD_STEP_TIMING_CHANGE_PERCENT: f64 = 5.0;
    /// Healthy 1-minute load for a benchmark run (the family guide says "below about 12"; ADR runs skipped above 25).
    pub const RUN_LOAD_LIMIT: f64 = 25.0;
}

/// Why a measurement could not run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BenchError {
    /// Zero timed runs requested.
    #[error("a measurement needs at least one timed run")]
    NoRuns,
    /// More runs than allowed.
    #[error("{got} runs is more than the limit of {max}")]
    TooManyRuns {
        /// Runs asked for.
        got: usize,
        /// Limit.
        max: usize,
    },
}

/// Most timed runs one measurement takes.
pub const MAX_RUNS: usize = 1_000_000;

/// A monotonic time source; tests inject a fake one.
pub trait Clock {
    /// Time since an arbitrary fixed start.
    fn now(&self) -> Duration;
}

/// The real clock.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock(Instant);

impl SystemClock {
    /// Starts a clock now.
    pub fn new() -> Self {
        Self(Instant::now())
    }
}
impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}
impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}

/// A clock that moves only when told to (for deterministic tests).
#[derive(Debug, Default)]
pub struct ManualClock(Cell<Duration>);

impl ManualClock {
    /// Moves the clock forward.
    pub fn advance(&self, d: Duration) {
        self.0.set(self.0.get() + d);
    }
}
impl Clock for ManualClock {
    fn now(&self) -> Duration {
        self.0.get()
    }
}

/// Timed samples of one measurement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Samples(Vec<Duration>);

impl Samples {
    /// Wraps samples; at least one is required.
    pub fn new(mut v: Vec<Duration>) -> Result<Self, BenchError> {
        if v.is_empty() {
            return Err(BenchError::NoRuns);
        }
        v.sort();
        Ok(Self(v))
    }
    /// Number of samples.
    pub fn len(&self) -> usize {
        self.0.len()
    }
    /// Never empty (kept for the usual pairing with `len`).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// The percentile `p` in `[0, 1]` as `performance.md` reports it: `sorted[floor(p * (n - 1))]`.
    pub fn percentile(&self, p: f64) -> Duration {
        let p = if p.is_nan() { 1.0 } else { p.clamp(0.0, 1.0) };
        self.0[(p * (self.0.len() - 1) as f64).floor() as usize]
    }
    /// The median.
    pub fn p50(&self) -> Duration {
        self.percentile(0.5)
    }
    /// The 95th percentile.
    pub fn p95(&self) -> Duration {
        self.percentile(0.95)
    }
    /// The slowest run.
    pub fn max(&self) -> Duration {
        self.0[self.0.len() - 1]
    }
    /// The mean.
    pub fn mean(&self) -> Duration {
        self.0.iter().sum::<Duration>() / self.0.len() as u32
    }
}

/// Runs `f` `warmup` times unmeasured, then `runs` times timed with `clock`.
pub fn measure<C: Clock>(clock: &C, warmup: usize, runs: usize, mut f: impl FnMut()) -> Result<Samples, BenchError> {
    if runs == 0 {
        return Err(BenchError::NoRuns);
    }
    if runs > MAX_RUNS {
        return Err(BenchError::TooManyRuns { got: runs, max: MAX_RUNS });
    }
    for _ in 0..warmup {
        f();
    }
    let mut out = Vec::with_capacity(runs);
    for _ in 0..runs {
        let t0 = clock.now();
        f();
        out.push(clock.now().saturating_sub(t0));
    }
    Samples::new(out)
}

/// Interleaved A/B timing: `pairs` times, run `a` then `b` (each `runs` timed runs after `warmup`), so drift cancels
/// between the two series (the family guide's rule). Returns the samples of A and of B.
pub fn measure_ab<C: Clock>(
    clock: &C,
    pairs: usize,
    warmup: usize,
    runs: usize,
    mut a: impl FnMut(),
    mut b: impl FnMut(),
) -> Result<(Samples, Samples), BenchError> {
    if pairs == 0 {
        return Err(BenchError::NoRuns);
    }
    let (mut sa, mut sb) = (Vec::new(), Vec::new());
    for _ in 0..pairs {
        sa.extend_from_slice(&measure(clock, warmup, runs, &mut a)?.0);
        sb.extend_from_slice(&measure(clock, warmup, runs, &mut b)?.0);
    }
    Ok((Samples::new(sa)?, Samples::new(sb)?))
}

/// Which statistic a budget limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stat {
    /// Median.
    P50,
    /// 95th percentile.
    P95,
    /// Worst run.
    Max,
}

/// A named latency budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Shown in the results table.
    pub name: &'static str,
    /// The limit.
    pub limit: Duration,
    /// The statistic compared with the limit.
    pub stat: Stat,
}

/// Whether a result meets its budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// At or under the limit.
    Within,
    /// Over the limit.
    Over,
}

impl Budget {
    /// The measured value this budget looks at.
    pub fn value(&self, s: &Samples) -> Duration {
        match self.stat {
            Stat::P50 => s.p50(),
            Stat::P95 => s.p95(),
            Stat::Max => s.max(),
        }
    }
    /// Judges samples against the budget.
    pub fn judge(&self, s: &Samples) -> Verdict {
        if self.value(s) <= self.limit {
            Verdict::Within
        } else {
            Verdict::Over
        }
    }
}

/// Relative change of `with` against `without` in percent (positive means slower). Zero when `without` is zero and
/// `with` is too; infinite when only `without` is zero.
pub fn change_percent(without: Duration, with: Duration) -> f64 {
    let (a, b) = (without.as_secs_f64(), with.as_secs_f64());
    if a == 0.0 {
        if b == 0.0 { 0.0 } else { f64::INFINITY }
    } else {
        (b - a) / a * 100.0
    }
}

/// Whether a run may start at this 1-minute load (benchmarks on a loaded machine are indications only).
pub fn load_allows_run(load_1min: f64) -> bool {
    load_1min.is_finite() && load_1min <= budgets::RUN_LOAD_LIMIT
}

/// The metadata of the family results template (`docs/benchmarks.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunInfo {
    /// `YYYY-MM-DD`.
    pub date: String,
    /// What was measured.
    pub what: String,
    /// Branch or tag.
    pub branch: String,
    /// Short commit hash.
    pub commit: String,
    /// Host, CPU, cores, RAM, OS.
    pub machine: String,
    /// 1-minute load range and other jobs running.
    pub load: String,
    /// Build kind, allocator, features, library versions.
    pub build: String,
    /// Generator and arguments; must be synthetic.
    pub corpus: String,
    /// The exact command with every env var.
    pub command: String,
    /// Run count and what is reported.
    pub runs: String,
    /// Footprint, PSS, private working set, or RSS.
    pub memory_measure: String,
    /// Noise, anomalies, what is not measured.
    pub notes: String,
}

/// One row of the numbers table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// What was measured.
    pub measure: String,
    /// The measured value, formatted.
    pub value: String,
    /// The budget, formatted ("none" for an indication).
    pub budget: String,
    /// Verdict text.
    pub verdict: String,
}

impl Row {
    /// A row for a latency statistic against a budget.
    pub fn latency(measure: &str, s: &Samples, b: &Budget) -> Self {
        let verdict = match b.judge(s) {
            Verdict::Within => "within",
            Verdict::Over => "over",
        };
        Self {
            measure: measure.to_owned(),
            value: format_duration(b.value(s)),
            budget: format!("{} {} <= {}", b.name, stat_name(b.stat), format_duration(b.limit)),
            verdict: verdict.to_owned(),
        }
    }
}

fn stat_name(s: Stat) -> &'static str {
    match s {
        Stat::P50 => "p50",
        Stat::P95 => "p95",
        Stat::Max => "max",
    }
}

/// Formats a duration with a unit that keeps three significant digits.
pub fn format_duration(d: Duration) -> String {
    let ns = d.as_nanos();
    if ns >= 1_000_000_000 {
        format!("{:.2} s", d.as_secs_f64())
    } else if ns >= 1_000_000 {
        format!("{:.2} ms", ns as f64 / 1e6)
    } else if ns >= 1_000 {
        format!("{:.2} us", ns as f64 / 1e3)
    } else {
        format!("{ns} ns")
    }
}

/// Escapes a cell so it cannot break the markdown table.
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\n', '\r'], " ")
}

/// Renders the results block of the family template, ready to paste at the top of `docs/performance.md`.
pub fn render_results_block(info: &RunInfo, rows: &[Row]) -> String {
    let mut o = String::new();
    let line = |o: &mut String, k: &str, v: &str| {
        let _ = writeln!(o, "- **{k}:** {}", v.replace(['\n', '\r'], " "));
    };
    let _ = writeln!(o, "### {}, {} ({})\n", info.date, info.what, info.branch);
    line(&mut o, "Commit", &info.commit);
    line(&mut o, "Machine", &info.machine);
    line(&mut o, "Load", &info.load);
    line(&mut o, "Build", &info.build);
    line(&mut o, "Corpus", &info.corpus);
    line(&mut o, "Command", &format!("`{}`", info.command.replace('`', "'")));
    line(&mut o, "Runs", &info.runs);
    let _ = writeln!(o, "- **Numbers:**\n\n  | Measure | Value | Budget | Verdict |\n  |---|---|---|---|");
    for r in rows {
        let _ = writeln!(
            o,
            "  | {} | {} | {} | {} |",
            cell(&r.measure),
            cell(&r.value),
            cell(&r.budget),
            cell(&r.verdict)
        );
    }
    let _ = writeln!(o);
    line(&mut o, "Memory measure", &info.memory_measure);
    line(&mut o, "Notes", &info.notes);
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn measure_uses_the_injected_clock_and_skips_warmup() {
        let clock = ManualClock::default();
        let mut calls = 0u64;
        let s = measure(&clock, 2, 5, || {
            calls += 1;
            clock.advance(ms(calls));
        })
        .unwrap();
        assert_eq!(calls, 7);
        assert_eq!(s.len(), 5);
        assert!(!s.is_empty());
        assert_eq!((s.p50(), s.max()), (ms(5), ms(7)));
        assert_eq!(s.mean(), ms(5));
        let sys = SystemClock::default();
        assert!(measure(&sys, 0, 2, || {}).is_ok());
        assert!(sys.now() >= Duration::ZERO);
    }

    #[test]
    fn interleaved_ab_alternates_and_pools() {
        let clock = ManualClock::default();
        let order = std::cell::RefCell::new(String::new());
        let (a, b) = measure_ab(
            &clock,
            2,
            0,
            1,
            || {
                order.borrow_mut().push('a');
                clock.advance(ms(1));
            },
            || {
                order.borrow_mut().push('b');
                clock.advance(ms(2));
            },
        )
        .unwrap();
        assert_eq!(order.borrow().as_str(), "abab");
        assert_eq!((a.len(), b.len(), a.max(), b.max()), (2, 2, ms(1), ms(2)));
        assert_eq!(measure_ab(&clock, 0, 0, 1, || {}, || {}), Err(BenchError::NoRuns));
    }

    #[test]
    fn percentile_follows_the_performance_md_rule() {
        // 30 samples: p95 is the 28th (index 27), as in the 2026-10-29 record.
        let s = Samples::new((1..=30).map(ms).collect()).unwrap();
        assert_eq!(s.p95(), ms(28));
        assert_eq!(s.p50(), ms(15));
        assert_eq!(s.percentile(f64::NAN), ms(30));
        assert_eq!(s.percentile(-1.0), ms(1));
    }

    #[test]
    fn bad_run_counts_are_typed_errors() {
        let c = ManualClock::default();
        assert_eq!(measure(&c, 0, 0, || {}), Err(BenchError::NoRuns));
        assert_eq!(
            measure(&c, 0, MAX_RUNS + 1, || {}),
            Err(BenchError::TooManyRuns {
                got: MAX_RUNS + 1,
                max: MAX_RUNS
            })
        );
        assert_eq!(Samples::new(vec![]), Err(BenchError::NoRuns));
    }

    #[test]
    fn budgets_are_judged_and_named_after_the_adr() {
        let s = Samples::new(vec![ms(100), ms(200), ms(900)]).unwrap();
        let gate = Budget {
            name: "locate gate",
            limit: budgets::LOCATE_GATE_GPU_P95,
            stat: Stat::Max,
        };
        assert_eq!(gate.judge(&s), Verdict::Over);
        let target = Budget {
            name: "target",
            limit: budgets::LOCATE_TARGET_GPU_P50,
            stat: Stat::P50,
        };
        assert_eq!(target.judge(&s), Verdict::Over);
        let p95 = Budget {
            name: "cpu",
            limit: budgets::LOCATE_GATE_CPU_P95,
            stat: Stat::P95,
        };
        assert_eq!(p95.judge(&s), Verdict::Within);
        assert_eq!(budgets::LOCATE_GATE_CPU_P95, Duration::from_secs(2));
        assert_eq!(budgets::HUD_FRAME_OVERHEAD, ms(2));
        assert!(
            budgets::LOCATE_TARGET_GPU_P95 < budgets::LOCATE_GATE_GPU_P95
                && budgets::LOCATE_GATE_CPU_P95_4K > budgets::LOCATE_GATE_GPU_P95_4K
        );
        assert!(budgets::MODEL_COLD_LOAD > ms(0) && budgets::LOCATE_TARGET_CPU_P95 < budgets::LOCATE_GATE_CPU_P95);
    }

    #[test]
    fn change_percent_and_load_gate() {
        assert!((change_percent(ms(100), ms(104)) - 4.0).abs() < 1e-9);
        assert_eq!(change_percent(ms(0), ms(0)), 0.0);
        assert!(change_percent(ms(0), ms(1)).is_infinite());
        assert!(load_allows_run(24.9) && !load_allows_run(25.1) && !load_allows_run(f64::NAN));
        assert_eq!(budgets::HUD_STEP_TIMING_CHANGE_PERCENT, 5.0);
    }

    #[test]
    fn results_block_follows_the_family_template() {
        let info = RunInfo {
            date: "2026-10-30".into(),
            what: "core geometry".into(),
            branch: "sighter/core-wave-0".into(),
            commit: "abc1234".into(),
            machine: "Mac mini M4 Pro, 14 cores, 64 GB, macOS 27.2".into(),
            load: "1-minute load 9-11".into(),
            build: "release".into(),
            corpus: "synthetic, seed 42".into(),
            command: "cargo run --release --example bench_core".into(),
            runs: "single, 200 timed runs, p50 and p95".into(),
            memory_measure: "not measured".into(),
            notes: "a | b\nsecond line".into(),
        };
        let s = Samples::new(vec![ms(1), ms(2)]).unwrap();
        let b = Budget {
            name: "HUD per frame",
            limit: budgets::HUD_FRAME_OVERHEAD,
            stat: Stat::P95,
        };
        let row = Row::latency("map 1k boxes | p95", &s, &b);
        assert_eq!(row.verdict, "within");
        let text = render_results_block(&info, &[row]);
        for needle in [
            "### 2026-10-30, core geometry (sighter/core-wave-0)",
            "- **Commit:** abc1234",
            "- **Numbers:**",
            "| Measure | Value | Budget | Verdict |",
            "- **Memory measure:**",
            "- **Notes:** a | b second line",
            "map 1k boxes \\| p95",
        ] {
            assert!(text.contains(needle), "missing {needle}\n{text}");
        }
        assert_eq!(format_duration(Duration::from_nanos(900)), "900 ns");
        assert_eq!(format_duration(Duration::from_micros(15)), "15.00 us");
        assert_eq!(format_duration(Duration::from_secs(3)), "3.00 s");
    }

    proptest! {
        #[test]
        fn percentiles_are_ordered_and_bounded(v in prop::collection::vec(0u64..10_000, 1..200)) {
            let s = Samples::new(v.iter().map(|&n| Duration::from_micros(n)).collect()).unwrap();
            prop_assert!(s.percentile(0.0) <= s.p50() && s.p50() <= s.p95() && s.p95() <= s.max());
            prop_assert!(s.mean() <= s.max());
        }
    }
}
