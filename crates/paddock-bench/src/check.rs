use crate::budget::Budgets;
use crate::kind::{Direction, Kind};
use crate::results::Results;
use paddock_cli::{
    CheckId, Finding, Outcome, Reason, Subject, ToolVersion, UnixSeconds, Unmeasured, Verdict,
    measure,
};
use serde_json::{Value, json};

/// How a measurement stands against its budget. Only a description of one row: the check's result is an
/// [`paddock_cli::Outcome`] built from the rows by [`check`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// Inside the limit and no worse than the previous run allows.
    WithinBudget,
    /// Past the limit but inside the tolerance: passes, with a warning.
    Tolerated,
    /// Past the limit and the tolerance. Fails.
    OverBudget,
    /// Inside the budget but much worse than the previous run. Fails.
    Regressed,
    /// A budget exists but the run did not record the measurement. Fails (a benchmark that stopped running is not a pass).
    NoMeasurement,
    /// The measurement exists with another kind than the budget names. Fails.
    WrongKind,
    /// A measurement without a budget: reported, never fails.
    Unbudgeted,
}

impl Standing {
    /// A short word for tables and JSON.
    pub fn word(self) -> &'static str {
        match self {
            Standing::WithinBudget => "pass",
            Standing::Tolerated => "tolerated",
            Standing::OverBudget => "over-budget",
            Standing::Regressed => "regressed",
            Standing::NoMeasurement => "missing",
            Standing::WrongKind => "wrong-kind",
            Standing::Unbudgeted => "unbudgeted",
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
    /// How it stands.
    pub verdict: Standing,
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

/// The whole check: the rows, and its outcomes (ADR 0089), which are what decide the exit.
#[derive(Debug, Clone)]
pub struct Report {
    /// The app.
    pub app: String,
    /// One row per budget, in file order, then the unbudgeted measurements by name.
    pub rows: Vec<Row>,
    /// What the check measured, and (second, when a budgeted measurement is missing) what it could not.
    pub outcomes: Vec<(String, Outcome)>,
}

impl Report {
    /// The exit: failing findings 5, a missing measurement or an empty run 6, otherwise 0. Only the outcomes decide it.
    pub fn exit(&self) -> paddock_cli::Exit {
        Verdict::strict(self.outcomes.iter().map(|(id, o)| (id.clone(), o.clone()))).exit()
    }

    /// The counts.
    pub fn summary(&self) -> Summary {
        let mut s = Summary::default();
        for r in &self.rows {
            match r.verdict {
                Standing::Unbudgeted => s.unbudgeted += 1,
                Standing::OverBudget
                | Standing::Regressed
                | Standing::NoMeasurement
                | Standing::WrongKind => s.failed += 1,
                Standing::WithinBudget | Standing::Tolerated => s.passed += 1,
            }
        }
        s
    }

    /// The rows that did not meet their budget (over it, regressed, of the wrong kind) or have no measurement.
    pub fn failures(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter().filter(|r| {
            matches!(
                r.verdict,
                Standing::OverBudget
                    | Standing::Regressed
                    | Standing::NoMeasurement
                    | Standing::WrongKind
            )
        })
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
        json!({"withinBudget": self.exit() == paddock_cli::Exit::Ok, "app": self.app, "outcomes": self.outcomes.iter().map(|(id, o)| { let mut v = o.json(); v["id"] = json!(id); v }).collect::<Vec<_>>(), "passed": s.passed, "failed": s.failed, "unbudgeted": s.unbudgeted, "rows": rows})
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
pub fn check(
    budgets: &Budgets,
    results: &Results,
    previous: Option<&Results>,
    subject: Subject,
) -> Report {
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
                verdict: Standing::NoMeasurement,
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
                Standing::WrongKind,
                format!(
                    "the budget is for a {:?} measurement but this is {:?}; fix the budget's `kind`",
                    b.kind.unwrap_or(m.kind),
                    m.kind
                ),
            )
        } else if past_allowed {
            (
                Standing::OverBudget,
                format!(
                    "{} {unit} is past the budget ({bound} {} {unit}, tolerance {}%); fix the slowdown or, if it is intended, change the budget with a reason",
                    fmt_num(m.value),
                    fmt_num(b.limit),
                    b.tolerance_pct
                ),
            )
        } else if worse_pct.is_some_and(|w| w > b.regression_pct) {
            (
                Standing::Regressed,
                format!(
                    "{:.0}% worse than the previous run ({} {unit} against {} {unit}); find what changed",
                    worse_pct.unwrap_or(0.0),
                    fmt_num(m.value),
                    fmt_num(prev.unwrap_or(0.0))
                ),
            )
        } else if past_limit {
            (
                Standing::Tolerated,
                format!(
                    "{} {unit} is past the limit of {} {unit} but inside the {}% tolerance",
                    fmt_num(m.value),
                    fmt_num(b.limit),
                    b.tolerance_pct
                ),
            )
        } else {
            (
                Standing::WithinBudget,
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
            verdict: Standing::Unbudgeted,
            message: format!(
                "{} {} has no budget; `paddock-bench propose` writes one from this run",
                fmt_num(m.value),
                m.kind.unit()
            ),
        });
    }
    let outcomes = outcomes_of(&rows, &results.app, subject);
    Report {
        app: results.app.clone(),
        rows,
        outcomes,
    }
}

/// What a check of `results` against `budgets` is about: the commit the run names (or that it names none) and a SHA-256 of the
/// two files' contents as read.
pub fn subject_of(results: &Results, budgets: &Budgets) -> Subject {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(serde_json::to_vec(results).unwrap_or_default());
    h.update(budgets.to_toml().unwrap_or_default());
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Subject::Files {
        head: paddock_cli::outcome::Sha::new(
            results.commit.clone().unwrap_or_else(|| "unknown".into()),
        ),
        digest: paddock_cli::outcome::Sha256::new(hex),
    }
}

/// The rows as outcomes (ADR 0089 amendment section 1): within budget is clean through `measure`; tolerated and unbudgeted are
/// report-only findings; over budget, regressed and wrong kind are failing findings; a budgeted measurement the run did not
/// record is a second outcome, not measured (it fails today too, with exit 6). No row at all measured nothing.
fn outcomes_of(rows: &[Row], app: &str, subject: Subject) -> Vec<(String, Outcome)> {
    let id = format!("bench:{app}");
    let tool = ToolVersion::new(concat!("paddock-bench ", env!("CARGO_PKG_VERSION")));
    let said = |r: &Row| format!("{}: {}", r.name, r.message);
    let missing: Vec<&str> = rows
        .iter()
        .filter(|r| r.verdict == Standing::NoMeasurement)
        .map(|r| r.name.as_str())
        .collect();
    let checked = measure(
        CheckId::new(id.clone()),
        subject,
        tool,
        UnixSeconds::now(),
        || {
            if rows.is_empty() {
                return Err(Unmeasured::NotMeasured(Reason::new(
                    "there is no budget and no measurement to compare",
                )));
            }
            Ok(rows
                .iter()
                .filter_map(|r| match r.verdict {
                    Standing::WithinBudget | Standing::NoMeasurement => None,
                    Standing::Tolerated | Standing::Unbudgeted => {
                        Some(Finding::report_only(said(r)))
                    }
                    Standing::OverBudget | Standing::Regressed | Standing::WrongKind => {
                        Some(Finding::failing(said(r)))
                    }
                })
                .collect())
        },
    );
    let mut out = vec![(id.clone(), checked)];
    if !missing.is_empty() {
        out.push((
            format!("{id}/missing"),
            Outcome::NotMeasured(Reason::new(format!(
                "the run did not record {}; make the benchmark run again or remove the budget",
                missing.join(", ")
            ))),
        ));
    }
    out
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

    fn subject() -> Subject {
        Subject::Files {
            head: paddock_cli::outcome::Sha::new("h"),
            digest: paddock_cli::outcome::Sha256::new("d"),
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

    fn verdict(b: Budget, kind: Kind, value: f64, prev: Option<f64>) -> Standing {
        let name = b.name.clone();
        let budgets = Budgets { budget: vec![b] };
        let p = prev.map(|v| run(&[(&name, kind, v)]));
        check(
            &budgets,
            &run(&[(&name, kind, value)]),
            p.as_ref(),
            subject(),
        )
        .rows[0]
            .verdict
    }

    #[test]
    fn lower_is_better_passes_tolerates_and_fails() {
        let b = || budget("q", 100.0);
        assert_eq!(
            verdict(b(), Kind::QueryLatency, 80.0, None),
            Standing::WithinBudget
        );
        assert_eq!(
            verdict(b(), Kind::QueryLatency, 100.0, None),
            Standing::WithinBudget,
            "exactly at the limit"
        );
        assert_eq!(
            verdict(b(), Kind::QueryLatency, 105.0, None),
            Standing::Tolerated
        );
        assert_eq!(
            verdict(b(), Kind::QueryLatency, 110.0, None),
            Standing::Tolerated,
            "exactly at the tolerance"
        );
        assert_eq!(
            verdict(b(), Kind::QueryLatency, 110.1, None),
            Standing::OverBudget
        );
    }

    #[test]
    fn higher_is_better_is_mirrored() {
        let b = || budget("t", 1000.0);
        assert_eq!(
            verdict(b(), Kind::Throughput, 5000.0, None),
            Standing::WithinBudget
        );
        assert_eq!(
            verdict(b(), Kind::Throughput, 950.0, None),
            Standing::Tolerated
        );
        assert_eq!(
            verdict(b(), Kind::Throughput, 899.0, None),
            Standing::OverBudget
        );
    }

    #[test]
    fn a_regression_against_the_previous_run_fails_inside_the_limit() {
        let b = || budget("q", 1000.0);
        assert_eq!(
            verdict(b(), Kind::QueryLatency, 130.0, Some(100.0)),
            Standing::Regressed
        );
        assert_eq!(
            verdict(b(), Kind::QueryLatency, 119.0, Some(100.0)),
            Standing::WithinBudget,
            "inside regression_pct"
        );
        assert_eq!(
            verdict(b(), Kind::QueryLatency, 60.0, Some(100.0)),
            Standing::WithinBudget,
            "faster is never a regression"
        );
        let t = || budget("t", 10.0);
        assert_eq!(
            verdict(t(), Kind::Throughput, 700.0, Some(1000.0)),
            Standing::Regressed,
            "30% less throughput"
        );
        assert_eq!(
            verdict(t(), Kind::Throughput, 900.0, Some(1000.0)),
            Standing::WithinBudget
        );
        assert_eq!(
            verdict(b(), Kind::QueryLatency, 50.0, Some(0.0)),
            Standing::WithinBudget,
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
            check(&budgets, &now, Some(&other), subject()).rows[0].verdict,
            Standing::WithinBudget
        );
        let kind_changed = run(&[("q", Kind::Memory, 100.0)]);
        let o = check(&budgets, &now, Some(&kind_changed), subject());
        assert_eq!(
            (o.rows[0].verdict, o.rows[0].previous),
            (Standing::WithinBudget, None)
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
            subject(),
        );
        let v: Vec<Standing> = o.rows.iter().map(|r| r.verdict).collect();
        assert_eq!(
            v,
            [
                Standing::NoMeasurement,
                Standing::WrongKind,
                Standing::Unbudgeted,
                Standing::Unbudgeted
            ]
        );
        assert_eq!(o.rows[2].name, "a", "extras are sorted by name");
        assert_eq!(
            o.exit(),
            paddock_cli::Exit::Findings,
            "a failing finding (5) beats the missing measurement (6)"
        );
        assert_eq!(
            o.outcomes.len(),
            2,
            "the missing measurement is an outcome of its own"
        );
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
    fn every_verdict_has_its_own_word() {
        let all = [
            Standing::WithinBudget,
            Standing::Tolerated,
            Standing::OverBudget,
            Standing::Regressed,
            Standing::NoMeasurement,
            Standing::WrongKind,
            Standing::Unbudgeted,
        ];
        let mut words: Vec<&str> = all.iter().map(|v| v.word()).collect();
        words.sort();
        words.dedup();
        assert_eq!(words.len(), all.len());
    }

    #[test]
    fn unbudgeted_measurements_alone_do_not_fail() {
        let o = check(
            &Budgets::default(),
            &run(&[("a", Kind::Memory, 2.0)]),
            None,
            subject(),
        );
        assert_eq!(o.exit(), paddock_cli::Exit::Ok);
        assert!(
            matches!(o.outcomes[0].1, Outcome::Findings(_)),
            "listed, with evidence"
        );
    }

    #[test]
    fn a_missing_measurement_alone_is_exit_6_and_a_pass_carries_evidence() {
        let budgets = Budgets {
            budget: vec![budget("q", 100.0)],
        };
        let o = check(&budgets, &run(&[]), None, subject());
        assert_eq!(o.exit(), paddock_cli::Exit::NotMeasured);
        let ok = check(
            &budgets,
            &run(&[("q", Kind::QueryLatency, 50.0)]),
            None,
            subject(),
        );
        assert_eq!(ok.exit(), paddock_cli::Exit::Ok);
        assert!(
            matches!(&ok.outcomes[0].1, Outcome::Clean(p) if p.evidence().check().as_str() == "bench:demo")
        );
        assert_eq!(
            ok.to_json()["outcomes"][0]["evidence"]["tool"]
                .as_str()
                .map(|t| t.starts_with("paddock-bench")),
            Some(true)
        );
    }

    /// PRINCIPLES "delete the check": no budget and no measurement (nothing was compared) is exit 6, never 0.
    #[test]
    fn deleting_every_budget_and_measurement_is_not_a_pass() {
        let o = check(&Budgets::default(), &run(&[]), None, subject());
        assert_eq!(o.exit(), paddock_cli::Exit::NotMeasured);
        assert!(
            Report {
                app: "demo".into(),
                rows: Vec::new(),
                outcomes: Vec::new()
            }
            .exit()
                == paddock_cli::Exit::NotMeasured
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
            subject(),
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
        assert_eq!(j["withinBudget"], false);
        assert_eq!(j["failed"], 1);
        assert_eq!(j["rows"][0]["verdict"], "over-budget");
        assert_eq!(j["rows"][2]["limit"], Value::Null);
        assert_eq!(fmt_num(1.23456), "1.23");
        let missing = check(&budgets, &run(&[]), None, subject()).render_markdown();
        assert!(
            missing.contains("| q | - | 100.0 | missing |") || missing.contains("| q | - |"),
            "{missing}"
        );
    }
}
