//! The benchmark side of the pure core (rule 48: every quoted benchmark is recorded in `docs/performance.md`).
//!
//! What is Sighter's own lives here: the budgets as named constants, interleaved A/B timing over an injected `now`
//! function, and the renderer of the family results block (`docs/benchmarks.md`). The clock, the sample statistics and
//! the results files are the shared `paddock-bench` crate's: the programs of `sighter-bench` use it as a
//! dependency and pass `|| clock.now()` here, so this module needs no clock type of its own.
//!
//! TODO(shared follow-up, after the Dyno and Spotter tags freeze lifts): the results-block renderer (`RunInfo`,
//! `ResultRow`, `render_results_block`) and the interleaved A/B timer are generic; move them to `paddock-bench`.
//!
//! **Runtime code never reads the budgets** (ADR 0031): they are benchmark constants only.

use std::fmt::Write as _;
use std::time::Duration;

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
    /// 1-minute load up to which a number counts as measured on a quiet machine (the family guide: below about 12). Above
    /// it, up to [`RUN_LOAD_LIMIT`], a run is only an indication.
    pub const QUIET_LOAD: f64 = 12.0;
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

/// Most timed runs one measurement takes (all pairs of an A/B measurement together).
pub const MAX_RUNS: usize = 1_000_000;

/// Runs `f` `warmup` times unmeasured, then `runs` times timed with `now` (a monotonic time since some start).
pub fn time_runs(now: &dyn Fn() -> Duration, warmup: usize, runs: usize, mut f: impl FnMut()) -> Result<Vec<Duration>, BenchError> {
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
        let t0 = now();
        f();
        out.push(now().saturating_sub(t0));
    }
    Ok(out)
}

/// Interleaved A/B timing: `pairs` times, run `a` then `b` (each `runs` timed runs after `warmup`), so drift cancels
/// between the two series (the family guide's rule). Returns the pooled samples of A and of B. `pairs * runs` is bounded
/// by [`MAX_RUNS`].
pub fn time_ab(
    now: &dyn Fn() -> Duration,
    pairs: usize,
    warmup: usize,
    runs: usize,
    mut a: impl FnMut(),
    mut b: impl FnMut(),
) -> Result<(Vec<Duration>, Vec<Duration>), BenchError> {
    if pairs == 0 || runs == 0 {
        return Err(BenchError::NoRuns);
    }
    let total = pairs.saturating_mul(runs);
    if total > MAX_RUNS {
        return Err(BenchError::TooManyRuns { got: total, max: MAX_RUNS });
    }
    let (mut sa, mut sb) = (Vec::with_capacity(total), Vec::with_capacity(total));
    for _ in 0..pairs {
        sa.extend(time_runs(now, warmup, runs, &mut a)?);
        sb.extend(time_runs(now, warmup, runs, &mut b)?);
    }
    Ok((sa, sb))
}

/// Relative change of `with` against `without` in percent (positive means slower). Zero when both are zero; infinite when
/// only `without` is zero.
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
    /// Footprint, PSS, private working set, or RSS, with the figure.
    pub memory_measure: String,
    /// Noise, anomalies, what is not measured.
    pub notes: String,
}

/// One row of the numbers table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultRow {
    /// What was measured.
    pub measure: String,
    /// The measured value, formatted.
    pub value: String,
    /// The budget, formatted ("none" for an indication).
    pub budget: String,
    /// Verdict text.
    pub verdict: String,
}

impl ResultRow {
    /// A measurement with no budget: an indication, not a verdict.
    pub fn indication(measure: &str, value: Duration) -> Self {
        Self {
            measure: measure.to_owned(),
            value: format_duration(value),
            budget: "none".into(),
            verdict: "indication".into(),
        }
    }
    /// A latency against the budget that belongs to it. Measured above [`budgets::QUIET_LOAD`] the verdict says
    /// "(indication)": a plain "within" is for a quiet machine only.
    pub fn against(measure: &str, value: Duration, budget_name: &str, limit: Duration, load_1min: f64) -> Self {
        Self::with_verdict(
            measure,
            format_duration(value),
            format!("{budget_name} <= {}", format_duration(limit)),
            value <= limit,
            load_1min,
        )
    }
    /// A row with a ready value and budget text and a pass flag; the load decides whether the verdict is an indication.
    pub fn with_verdict(measure: &str, value: String, budget: String, within: bool, load_1min: f64) -> Self {
        let word = if within { "within" } else { "over" };
        let quiet = load_1min <= budgets::QUIET_LOAD;
        Self {
            measure: measure.to_owned(),
            value,
            budget,
            verdict: if quiet { word.to_owned() } else { format!("{word} (indication)") },
        }
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
pub fn render_results_block(info: &RunInfo, rows: &[ResultRow]) -> String {
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
    use std::cell::Cell;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn time_runs_uses_the_injected_clock_and_skips_warmup() {
        let t = Cell::new(Duration::ZERO);
        let now = || t.get();
        let mut calls = 0u64;
        let s = time_runs(&now, 2, 5, || {
            calls += 1;
            t.set(t.get() + ms(calls));
        })
        .unwrap();
        assert_eq!(calls, 7);
        assert_eq!(s, vec![ms(3), ms(4), ms(5), ms(6), ms(7)]);
    }

    #[test]
    fn bad_run_counts_are_typed_errors() {
        let now = || Duration::ZERO;
        assert_eq!(time_runs(&now, 0, 0, || {}), Err(BenchError::NoRuns));
        assert_eq!(
            time_runs(&now, 0, MAX_RUNS + 1, || {}),
            Err(BenchError::TooManyRuns {
                got: MAX_RUNS + 1,
                max: MAX_RUNS
            })
        );
        assert_eq!(time_ab(&now, 0, 0, 1, || {}, || {}), Err(BenchError::NoRuns));
        assert_eq!(time_ab(&now, 1, 0, 0, || {}, || {}), Err(BenchError::NoRuns));
        // pairs x runs is what is bounded, so no pooled series can exceed the limit or be empty.
        assert_eq!(
            time_ab(&now, 2, 0, MAX_RUNS, || {}, || {}),
            Err(BenchError::TooManyRuns {
                got: 2 * MAX_RUNS,
                max: MAX_RUNS
            })
        );
        assert_eq!(
            time_ab(&now, usize::MAX, 0, 2, || {}, || {}),
            Err(BenchError::TooManyRuns {
                got: usize::MAX,
                max: MAX_RUNS
            })
        );
    }

    #[test]
    fn interleaved_ab_alternates_and_pools() {
        let t = Cell::new(Duration::ZERO);
        let now = || t.get();
        let order = std::cell::RefCell::new(String::new());
        let (a, b) = time_ab(
            &now,
            2,
            0,
            1,
            || {
                order.borrow_mut().push('a');
                t.set(t.get() + ms(1));
            },
            || {
                order.borrow_mut().push('b');
                t.set(t.get() + ms(2));
            },
        )
        .unwrap();
        assert_eq!(order.borrow().as_str(), "abab");
        assert_eq!((a, b), (vec![ms(1), ms(1)], vec![ms(2), ms(2)]));
    }

    #[test]
    fn budgets_are_named_after_the_adr() {
        assert_eq!(budgets::LOCATE_GATE_CPU_P95, Duration::from_secs(2));
        assert_eq!(budgets::HUD_FRAME_OVERHEAD, ms(2));
        assert_eq!(budgets::HUD_STEP_TIMING_CHANGE_PERCENT, 5.0);
        assert!(budgets::LOCATE_TARGET_GPU_P95 < budgets::LOCATE_GATE_GPU_P95);
        assert!(budgets::LOCATE_GATE_CPU_P95_4K > budgets::LOCATE_GATE_GPU_P95_4K);
        assert!(budgets::MODEL_COLD_LOAD > budgets::LOCATE_TARGET_GPU_P50);
        assert!(budgets::LOCATE_TARGET_CPU_P95 < budgets::LOCATE_GATE_CPU_P95);
        assert!(budgets::LOCATE_TARGET_GPU_P50 < budgets::LOCATE_TARGET_GPU_P95);
        assert!(budgets::LOCATE_GATE_GPU_P95_4K > budgets::LOCATE_GATE_GPU_P95);
    }

    #[test]
    fn change_percent_and_load_gate() {
        assert!((change_percent(ms(100), ms(104)) - 4.0).abs() < 1e-9);
        assert_eq!(change_percent(ms(0), ms(0)), 0.0);
        assert!(change_percent(ms(0), ms(1)).is_infinite());
        assert!(load_allows_run(24.9) && !load_allows_run(25.1) && !load_allows_run(f64::NAN));
    }

    #[test]
    fn rows_judge_only_against_their_own_budget() {
        let within = ResultRow::against("HUD", ms(1), "HUD per frame p95", budgets::HUD_FRAME_OVERHEAD, 8.0);
        assert_eq!(
            (within.verdict.as_str(), within.budget.as_str()),
            ("within", "HUD per frame p95 <= 2.00 ms")
        );
        let q = |value, load| ResultRow::against("HUD", value, "x", budgets::HUD_FRAME_OVERHEAD, load).verdict;
        assert_eq!(q(ms(3), 8.0), "over");
        // Between the quiet threshold and the run limit a verdict is only an indication; a plain "within" is for quiet.
        assert_eq!(q(ms(1), 18.5), "within (indication)");
        assert_eq!(q(ms(3), 18.5), "over (indication)");
        assert_eq!(q(ms(1), budgets::QUIET_LOAD), "within");
        let ind = ResultRow::indication("NMS", Duration::from_micros(65));
        assert_eq!(
            (ind.budget.as_str(), ind.verdict.as_str(), ind.value.as_str()),
            ("none", "indication", "65.00 us")
        );
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
        let row = ResultRow::indication("map 1k boxes | p95", ms(1));
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
}
