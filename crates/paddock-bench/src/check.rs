use crate::budget::Budgets;
use crate::kind::{Direction, Kind};
use crate::results::Results;
use serde_json::{Value, json};

/// What a measurement came to against its budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Inside the limit and no worse than the previous run allows.
    Pass,
    /// Past the limit but inside the tolerance: passes, with a warning.
    Tolerated,
    /// Past the limit and the tolerance. Fails.
    OverBudget,
    /// Inside the budget but much worse than the previous run. Fails.
    Regressed,
    /// A budget exists but the run did not record the measurement. Fails (a benchmark that stopped running is not a pass).
    Missing,
    /// The measurement exists with another kind than the budget names. Fails.
    WrongKind,
    /// A measurement without a budget: reported, never fails.
    Unbudgeted,
}

impl Verdict {
    /// Whether this verdict fails the check.
    pub fn fails(self) -> bool {
        matches!(
            self,
            Verdict::OverBudget | Verdict::Regressed | Verdict::Missing | Verdict::WrongKind
        )
    }

    /// A short word for tables and JSON.
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Tolerated => "tolerated",
            Verdict::OverBudget => "over-budget",
            Verdict::Regressed => "regressed",
            Verdict::Missing => "missing",
            Verdict::WrongKind => "wrong-kind",
            Verdict::Unbudgeted => "unbudgeted",
        }
    }
}

/// One line of the report.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The measurement's name.
    pub name: String,
    /// Its kind (the budget's when the measurement is missing and the budget names one).
    pub kind: Option<Kind>,
    /// The measured value.
    pub value: Option<f64>,
    /// The budget's limit.
    pub limit: Option<f64>,
    /// The previous run's value.
    pub previous: Option<f64>,
    /// How much worse (positive) or better (negative) than the previous run, in percent.
    pub worse_pct: Option<f64>,
    /// The verdict.
    pub verdict: Verdict,
    /// One sentence saying what happened and what to do.
    pub message: String,
}

/// How many rows ended in each way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    /// Rows that passed (including tolerated ones).
    pub passed: usize,
    /// Rows that failed.
    pub failed: usize,
    /// Rows without a budget.
    pub unbudgeted: usize,
}

/// The whole check.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// The app.
    pub app: String,
    /// One row per budget, in file order, then the unbudgeted measurements by name.
    pub rows: Vec<Row>,
}

impl Outcome {
    /// Whether anything failed.
    pub fn failed(&self) -> bool {
        self.rows.iter().any(|r| r.verdict.fails())
    }

    /// The counts.
    pub fn summary(&self) -> Summary {
        let mut s = Summary::default();
        for r in &self.rows {
            match r.verdict {
                Verdict::Unbudgeted => s.unbudgeted += 1,
                v if v.fails() => s.failed += 1,
                _ => s.passed += 1,
            }
        }
        s
    }

    /// The failing rows.
    pub fn failures(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter().filter(|r| r.verdict.fails())
    }

    /// Plain text for the job log.
    pub fn render_text(&self) -> String {
        let mut out = format!("{} benchmarks\n", self.app);
        let w = self
            .rows
            .iter()
            .map(|r| r.name.chars().count())
            .max()
            .unwrap_or(0);
        for r in &self.rows {
            out.push_str(&format!(
                "{:<w$}  {:<11}  {}\n",
                r.name,
                r.verdict.word(),
                r.message
            ));
        }
        out.push_str(&self.summary_line());
        out
    }

    /// A Markdown table for the job summary or an issue.
    pub fn render_markdown(&self) -> String {
        let mut out = format!(
            "### {} benchmarks\n\n| Measurement | Value | Limit | Verdict |\n|---|---:|---:|---|\n",
            self.app
        );
        for r in &self.rows {
            let unit = r.kind.map_or("", Kind::unit);
            let num =
                |v: Option<f64>| v.map_or("-".to_string(), |v| format!("{} {unit}", fmt_num(v)));
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                r.name,
                num(r.value),
                num(r.limit),
                r.verdict.word()
            ));
        }
        out.push('\n');
        out.push_str(&self.summary_line());
        out
    }

    fn summary_line(&self) -> String {
        let s = self.summary();
        format!(
            "{} passed, {} failed, {} without a budget\n",
            s.passed, s.failed, s.unbudgeted
        )
    }

    /// The JSON object `paddock-bench check --json` prints.
    pub fn to_json(&self) -> Value {
        let rows: Vec<Value> = self
            .rows
            .iter()
            .map(|r| {
                json!({"name": r.name, "kind": r.kind, "value": r.value, "limit": r.limit, "previous": r.previous,
                       "worsePct": r.worse_pct, "verdict": r.verdict.word(), "message": r.message})
            })
            .collect();
        let s = self.summary();
        json!({"ok": !self.failed(), "app": self.app, "passed": s.passed, "failed": s.failed, "unbudgeted": s.unbudgeted, "rows": rows})
    }
}

/// A number for people: no decimals from 1000 up, else two significant decimals.
pub(crate) fn fmt_num(v: f64) -> String {
    if v.abs() >= 1000.0 {
        format!("{v:.0}")
    } else if v.abs() >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}

/// Compares `results` with `budgets`, and with the `previous` run when there is one (a regression is judged on the
/// measurements both runs have, and only when they ran on the same OS and architecture: other machines are not compared).
pub fn check(budgets: &Budgets, results: &Results, previous: Option<&Results>) -> Outcome {
    let previous =
        previous.filter(|p| p.host.os == results.host.os && p.host.arch == results.host.arch);
    let mut rows = Vec::with_capacity(budgets.budget.len() + results.measurements.len());
    for b in &budgets.budget {
        let Some(m) = results.get(&b.name) else {
            rows.push(Row {
                name: b.name.clone(),
                kind: b.kind,
                value: None,
                limit: Some(b.limit),
                previous: None,
                worse_pct: None,
                verdict: Verdict::Missing,
                message: "the run did not record this measurement; make the benchmark run again or remove the budget".into(),
            });
            continue;
        };
        let unit = m.kind.unit();
        let dir = m.kind.direction();
        let prev = previous
            .and_then(|p| p.get(&b.name))
            .filter(|p| p.kind == m.kind)
            .map(|p| p.value);
        let worse_pct = prev.filter(|p| *p > 0.0).map(|p| match dir {
            Direction::LowerIsBetter => (m.value - p) / p * 100.0,
            Direction::HigherIsBetter => (p - m.value) / p * 100.0,
        });
        let allowed = b.allowed(dir);
        let past_limit = match dir {
            Direction::LowerIsBetter => m.value > b.limit,
            Direction::HigherIsBetter => m.value < b.limit,
        };
        let past_allowed = match dir {
            Direction::LowerIsBetter => m.value > allowed,
            Direction::HigherIsBetter => m.value < allowed,
        };
        let bound = if dir == Direction::LowerIsBetter {
            "at most"
        } else {
            "at least"
        };
        let (verdict, message) = if b.kind.is_some_and(|k| k != m.kind) {
            (
                Verdict::WrongKind,
                format!(
                    "the budget is for a {:?} measurement but this is {:?}; fix the budget's `kind`",
                    b.kind.unwrap_or(m.kind),
                    m.kind
                ),
            )
        } else if past_allowed {
            (
                Verdict::OverBudget,
                format!(
                    "{} {unit} is past the budget ({bound} {} {unit}, tolerance {}%); fix the slowdown or, if it is intended, change the budget with a reason",
                    fmt_num(m.value),
                    fmt_num(b.limit),
                    b.tolerance_pct
                ),
            )
        } else if worse_pct.is_some_and(|w| w > b.regression_pct) {
            (
                Verdict::Regressed,
                format!(
                    "{:.0}% worse than the previous run ({} {unit} against {} {unit}); find what changed",
                    worse_pct.unwrap_or(0.0),
                    fmt_num(m.value),
                    fmt_num(prev.unwrap_or(0.0))
                ),
            )
        } else if past_limit {
            (
                Verdict::Tolerated,
                format!(
                    "{} {unit} is past the limit of {} {unit} but inside the {}% tolerance",
                    fmt_num(m.value),
                    fmt_num(b.limit),
                    b.tolerance_pct
                ),
            )
        } else {
            (
                Verdict::Pass,
                format!(
                    "{} {unit} ({bound} {} {unit})",
                    fmt_num(m.value),
                    fmt_num(b.limit)
                ),
            )
        };
        rows.push(Row {
            name: b.name.clone(),
            kind: Some(m.kind),
            value: Some(m.value),
            limit: Some(b.limit),
            previous: prev,
            worse_pct,
            verdict,
            message,
        });
    }
    let mut extra: Vec<&crate::results::Measurement> = results
        .measurements
        .iter()
        .filter(|m| budgets.get(&m.name).is_none())
        .collect();
    extra.sort_by(|a, b| a.name.cmp(&b.name));
    for m in extra {
        rows.push(Row {
            name: m.name.clone(),
            kind: Some(m.kind),
            value: Some(m.value),
            limit: None,
            previous: None,
            worse_pct: None,
            verdict: Verdict::Unbudgeted,
            message: format!(
                "{} {} has no budget; `paddock-bench propose` writes one from this run",
                fmt_num(m.value),
                m.kind.unit()
            ),
        });
    }
    Outcome {
        app: results.app.clone(),
        rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::Budget;
    use crate::results::Measurement;

    fn budget(name: &str, limit: f64) -> Budget {
        Budget {
            name: name.into(),
            kind: None,
            limit,
            tolerance_pct: 10.0,
            regression_pct: 20.0,
        }
    }

    fn run(items: &[(&str, Kind, f64)]) -> Results {
        let mut r = Results::new("demo", "1");
        r.measurements = items
            .iter()
            .map(|(n, k, v)| Measurement {
                name: (*n).into(),
                kind: *k,
                value: *v,
                runs: 3,
            })
            .collect();
        r
    }

    fn verdict(b: Budget, kind: Kind, value: f64, prev: Option<f64>) -> Verdict {
        let name = b.name.clone();
        let budgets = Budgets { budget: vec![b] };
        let p = prev.map(|v| run(&[(&name, kind, v)]));
        check(&budgets, &run(&[(&name, kind, value)]), p.as_ref()).rows[0].verdict
    }

    #[test]
    fn lower_is_better_passes_tolerates_and_fails() {
        let b = || budget("q", 100.0);
        assert_eq!(verdict(b(), Kind::QueryLatency, 80.0, None), Verdict::Pass);
        assert_eq!(
            verdict(b(), Kind::QueryLatency, 100.0, None),
            Verdict::Pass,
            "exactly at the limit"
        );
        assert_eq!(
            verdict(b(), Kind::QueryLatency, 105.0, None),
            Verdict::Tolerated
        );
        assert_eq!(
            verdict(b(), Kind::QueryLatency, 110.0, None),
            Verdict::Tolerated,
            "exactly at the tolerance"
        );
        assert_eq!(
            verdict(b(), Kind::QueryLatency, 110.1, None),
            Verdict::OverBudget
        );
    }

    #[test]
    fn higher_is_better_is_mirrored() {
        let b = || budget("t", 1000.0);
        assert_eq!(verdict(b(), Kind::Throughput, 5000.0, None), Verdict::Pass);
        assert_eq!(
            verdict(b(), Kind::Throughput, 950.0, None),
            Verdict::Tolerated
        );
        assert_eq!(
            verdict(b(), Kind::Throughput, 899.0, None),
            Verdict::OverBudget
        );
    }

    #[test]
    fn a_regression_against_the_previous_run_fails_inside_the_limit() {
        let b = || budget("q", 1000.0);
        assert_eq!(
            verdict(b(), Kind::QueryLatency, 130.0, Some(100.0)),
            Verdict::Regressed
        );
        assert_eq!(
            verdict(b(), Kind::QueryLatency, 119.0, Some(100.0)),
            Verdict::Pass,
            "inside regression_pct"
        );
        assert_eq!(
            verdict(b(), Kind::QueryLatency, 60.0, Some(100.0)),
            Verdict::Pass,
            "faster is never a regression"
        );
        let t = || budget("t", 10.0);
        assert_eq!(
            verdict(t(), Kind::Throughput, 700.0, Some(1000.0)),
            Verdict::Regressed,
            "30% less throughput"
        );
        assert_eq!(
            verdict(t(), Kind::Throughput, 900.0, Some(1000.0)),
            Verdict::Pass
        );
        assert_eq!(
            verdict(b(), Kind::QueryLatency, 50.0, Some(0.0)),
            Verdict::Pass,
            "a zero baseline is not compared"
        );
    }

    #[test]
    fn previous_runs_from_another_machine_class_or_kind_are_not_compared() {
        let budgets = Budgets {
            budget: vec![budget("q", 1000.0)],
        };
        let now = run(&[("q", Kind::QueryLatency, 500.0)]);
        let mut other = run(&[("q", Kind::QueryLatency, 100.0)]);
        other.host.os = "somewhere-else".into();
        assert_eq!(
            check(&budgets, &now, Some(&other)).rows[0].verdict,
            Verdict::Pass
        );
        let kind_changed = run(&[("q", Kind::Memory, 100.0)]);
        let o = check(&budgets, &now, Some(&kind_changed));
        assert_eq!(
            (o.rows[0].verdict, o.rows[0].previous),
            (Verdict::Pass, None)
        );
    }

    #[test]
    fn a_missing_measurement_and_a_wrong_kind_fail_and_extras_are_reported() {
        let budgets = Budgets {
            budget: vec![
                budget("gone", 1.0),
                Budget {
                    kind: Some(Kind::Memory),
                    ..budget("m", 100.0)
                },
            ],
        };
        let o = check(
            &budgets,
            &run(&[
                ("m", Kind::Startup, 5.0),
                ("z", Kind::Memory, 1.0),
                ("a", Kind::Memory, 2.0),
            ]),
            None,
        );
        let v: Vec<Verdict> = o.rows.iter().map(|r| r.verdict).collect();
        assert_eq!(
            v,
            [
                Verdict::Missing,
                Verdict::WrongKind,
                Verdict::Unbudgeted,
                Verdict::Unbudgeted
            ]
        );
        assert_eq!(o.rows[2].name, "a", "extras are sorted by name");
        assert!(o.failed());
        assert_eq!(o.failures().count(), 2);
        assert_eq!(
            o.summary(),
            Summary {
                passed: 0,
                failed: 2,
                unbudgeted: 2
            }
        );
    }

    #[test]
    fn unbudgeted_measurements_alone_do_not_fail() {
        let o = check(&Budgets::default(), &run(&[("a", Kind::Memory, 2.0)]), None);
        assert!(!o.failed());
        assert!(
            !Verdict::Pass.fails() && !Verdict::Tolerated.fails() && !Verdict::Unbudgeted.fails()
        );
    }

    #[test]
    fn reports_render_in_text_markdown_and_json() {
        let budgets = Budgets {
            budget: vec![budget("q", 100.0), budget("big", 5000.0)],
        };
        let o = check(
            &budgets,
            &run(&[
                ("q", Kind::QueryLatency, 250.0),
                ("big", Kind::Memory, 4200.0),
                ("x", Kind::Startup, 12.34),
            ]),
            None,
        );
        let text = o.render_text();
        assert!(text.starts_with("demo benchmarks\n"), "{text}");
        assert!(
            text.contains("q    over-budget")
                && text.contains("1 passed, 1 failed, 1 without a budget"),
            "{text}"
        );
        let md = o.render_markdown();
        assert!(
            md.contains("| q | 250.0 ms | 100.0 ms | over-budget |")
                && md.contains("| big | 4200 MB | 5000 MB | pass |"),
            "{md}"
        );
        let j = o.to_json();
        assert_eq!(j["ok"], false);
        assert_eq!(j["failed"], 1);
        assert_eq!(j["rows"][0]["verdict"], "over-budget");
        assert_eq!(j["rows"][2]["limit"], Value::Null);
        assert_eq!(fmt_num(1.23456), "1.23");
        let missing = check(&budgets, &run(&[]), None).render_markdown();
        assert!(
            missing.contains("| q | - | 100.0 | missing |") || missing.contains("| q | - |"),
            "{missing}"
        );
    }
}
