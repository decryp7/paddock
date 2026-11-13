//! Folding a run's outcomes into one exit code (ADR 0089 section 2).

use super::{Date, Optional, Outcome, Run, RunKind, Scope};
use crate::{Exit, Table};
use serde_json::{Value, json};

/// One measurement of the run: what it was, how it ended, and the optional entry that excused it, if any.
#[derive(Debug, Clone)]
struct Item {
    id: String,
    outcome: Outcome,
    /// What excuses this not-measured item, if anything.
    excused_by: Option<Excuse>,
}

/// What excuses a not-measured item: reviewed data, or a person's override. A person's override is an excuse, never a
/// state of its own (ADR 0089 amendment 2026-11-13, pre-push's former `Accepted`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Excuse {
    /// A live `[[optional]]` entry, by id.
    Register(String),
    /// A person's override, by who gave it.
    Override(String),
}

impl Item {
    /// A whole-tree run has no "not applicable": a check that read nothing there did not measure.
    fn counts_as_not_measured(&self, scope: Scope) -> bool {
        match self.outcome {
            Outcome::NotMeasured(_) => self.excused_by.is_none(),
            Outcome::NotApplicable(_) => scope == Scope::Root,
            _ => false,
        }
    }
}

/// A run's outcomes folded into one exit code. Built only by [`Verdict::of`].
#[must_use = "a verdict is turned into the process exit with Verdict::exit"]
#[derive(Debug, Clone)]
pub struct Verdict {
    items: Vec<Item>,
    scope: Scope,
    /// Optional entries past their date: errors.
    problems: Vec<String>,
}

impl Verdict {
    /// Folds `outcomes` (each with the id of what was measured, such as `check:C99/graph`) for `run`, with the reviewed
    /// `optional` entries. No outcome at all is a run that measured nothing: [`Exit::NotMeasured`]. Only a not-measured item
    /// can be excused, and only by a live entry that applies to this leg and OS (and in a release run, to one measured elsewhere).
    pub fn of<S: Into<String>>(
        outcomes: impl IntoIterator<Item = (S, Outcome)>,
        optional: &Optional,
        run: &Run,
    ) -> Verdict {
        Verdict::of_overridden(
            outcomes.into_iter().map(|(id, o)| (id, o, None)),
            optional,
            run,
        )
    }

    /// Like [`Verdict::of`], with a person's override per item (the name of who gave it): it excuses a not-measured item
    /// like an `[[optional]]` entry does, is named in the JSON (`override`), and excuses nothing else.
    pub fn of_overridden<S: Into<String>>(
        outcomes: impl IntoIterator<Item = (S, Outcome, Option<String>)>,
        optional: &Optional,
        run: &Run,
    ) -> Verdict {
        let items = outcomes
            .into_iter()
            .map(|(id, outcome, who)| {
                let id = id.into();
                let excused_by = match outcome {
                    Outcome::NotMeasured(_) => who.map(Excuse::Override).or_else(|| {
                        optional
                            .covering(&id, run)
                            .map(|e| Excuse::Register(e.id.clone()))
                    }),
                    _ => None,
                };
                Item {
                    id,
                    outcome,
                    excused_by,
                }
            })
            .collect();
        let problems = optional
            .expired(run.today)
            .iter()
            .map(|e| {
                format!(
                    "the optional entry {} expired on {}: renew it with a reason, or remove it",
                    e.id, e.expires
                )
            })
            .collect();
        Verdict {
            items,
            scope: run.scope,
            problems,
        }
    }

    /// Folds `outcomes` for a tool with no `[[optional]]` register: an ordinary whole-tree run, so nothing is excused, a
    /// not-applicable item counts as not measured, and no outcome at all is [`Exit::NotMeasured`].
    pub fn strict<S: Into<String>>(outcomes: impl IntoIterator<Item = (S, Outcome)>) -> Verdict {
        let today =
            Date::parse("2000-01-01").unwrap_or_else(|_| unreachable!("a fixed valid date"));
        Verdict::of(
            outcomes,
            &Optional::none(),
            &Run::new(RunKind::Ordinary, Scope::Root, "any", today),
        )
    }

    /// Like [`Verdict::strict`] for a run scoped to a change (the push gate: a gate the change does not reach is not applicable, not
    /// missing), with a person's override per item (see [`Verdict::of_overridden`]).
    pub fn scoped<S: Into<String>>(
        outcomes: impl IntoIterator<Item = (S, Outcome, Option<String>)>,
    ) -> Verdict {
        let today =
            Date::parse("2000-01-01").unwrap_or_else(|_| unreachable!("a fixed valid date"));
        Verdict::of_overridden(
            outcomes,
            &Optional::none(),
            &Run::new(RunKind::Ordinary, Scope::Scoped, "any", today),
        )
    }

    /// The exit code, by the precedence in the module doc.
    pub fn exit(&self) -> Exit {
        if self
            .items
            .iter()
            .any(|i| matches!(i.outcome, Outcome::CheckBroken(_)))
        {
            Exit::CheckBroken
        } else if self.items.iter().any(|i| i.outcome.failing()) || !self.problems.is_empty() {
            Exit::Findings
        } else if self.items.is_empty()
            || self
                .items
                .iter()
                .any(|i| i.counts_as_not_measured(self.scope))
        {
            Exit::NotMeasured
        } else {
            Exit::Ok
        }
    }

    /// The report: the exit, its name, every item with its reason or findings (nothing is dropped, whatever the exit), the
    /// optional entries used and the problems with the entries.
    pub fn json(&self) -> Value {
        let exit = self.exit();
        let items: Vec<Value> = self
            .items
            .iter()
            .map(|i| {
                let mut v = i.outcome.json();
                v["id"] = json!(i.id);
                match &i.excused_by {
                    Some(Excuse::Register(e)) => v["optional"] = json!(e),
                    Some(Excuse::Override(who)) => v["override"] = json!(who),
                    None => {}
                }
                if i.counts_as_not_measured(self.scope) && matches!(i.outcome, Outcome::NotApplicable(_)) {
                    v["note"] = json!("not applicable only in a scoped run; this was a whole-tree run, so it counts as not measured");
                }
                v
            })
            .collect();
        let mut used: Vec<&str> = self
            .items
            .iter()
            .filter_map(|i| match &i.excused_by {
                Some(Excuse::Register(e)) => Some(e.as_str()),
                _ => None,
            })
            .collect();
        used.sort_unstable();
        used.dedup();
        let mut v = json!({
            "exit": exit.code(),
            "status": exit.name(),
            "outcomes": items,
            "optional_used": used,
            "problems": self.problems,
        });
        if self.items.is_empty() {
            v["note"] = json!("no outcome was recorded: nothing was measured");
        }
        v
    }

    /// The items as a [`Table`] (id, status, detail, optional), for `write_table`.
    pub fn table(&self) -> Table {
        let rows: Vec<Vec<Value>> = self
            .items
            .iter()
            .map(|i| {
                vec![
                    json!(i.id),
                    json!(i.outcome.name()),
                    json!(i.outcome.detail()),
                    json!(match &i.excused_by {
                        Some(Excuse::Register(e)) => e.clone(),
                        Some(Excuse::Override(who)) => format!("override by {who}"),
                        None => String::new(),
                    }),
                ]
            })
            .collect();
        Table {
            columns: ["id", "status", "detail", "optional"]
                .map(String::from)
                .to_vec(),
            total: rows.len(),
            rows,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outcome::test_support::{clean, measured};
    use crate::outcome::{Finding, Reason};
    use crate::{TableFormat, write_table};

    const FILE: &str = "[[optional]]\nid = \"test:cov\"\nwhere = { leg = [\"l\"] }\nreason = \"r\"\nexpires = \"2027-01-31\"\nmeasured_on = [\"m\"]\n";

    type Labelled = (&'static str, Outcome);

    fn d(text: &str) -> Date {
        Date::parse(text).unwrap()
    }

    fn reason() -> Reason {
        Reason::new("why")
    }

    /// An ordinary whole-tree run on leg `l`, before the entry expires.
    fn root() -> Run {
        Run::new(RunKind::Ordinary, Scope::Root, "linux", d("2026-11-10")).on_leg("l")
    }

    fn of(items: Vec<(&str, Outcome)>, run: &Run) -> Verdict {
        Verdict::of(items, &Optional::parse(FILE).unwrap(), run)
    }

    /// Every kind an outcome can take, with both severities, a mixed list, and a not-measured item the entry names.
    fn kinds() -> Vec<Labelled> {
        vec![
            ("clean", clean()),
            ("na", Outcome::NotApplicable(reason())),
            ("fail", measured(vec![Finding::failing("f")])),
            ("report", measured(vec![Finding::report_only("r")])),
            (
                "mixed",
                measured(vec![Finding::report_only("r"), Finding::failing("f")]),
            ),
            ("unc", Outcome::NotMeasured(reason())),
            ("test:cov", Outcome::NotMeasured(reason())),
            ("broken", Outcome::CheckBroken(reason())),
        ]
    }

    /// The run variants: scope x (ordinary | release with a green leg | release without) x (entry live | expired).
    fn runs() -> Vec<(Run, bool)> {
        let mut out = Vec::new();
        for scope in [Scope::Root, Scope::Scoped] {
            for variant in 0..3 {
                for today in ["2026-11-10", "2027-06-01"] {
                    let kind = if variant == 0 {
                        RunKind::Ordinary
                    } else {
                        RunKind::Release
                    };
                    let mut r = Run::new(kind, scope, "linux", d(today)).on_leg("l");
                    if variant == 1 {
                        r = r.with_green("test:cov", "m");
                    }
                    let covers = today == "2026-11-10" && variant != 2;
                    out.push((r, covers));
                }
            }
        }
        out
    }

    /// Calls `visit` with every sequence of up to `max` items drawn from `kinds()`.
    fn every_run(max: usize, visit: &mut dyn FnMut(&[Labelled])) {
        fn go(prefix: &mut Vec<Labelled>, left: usize, visit: &mut dyn FnMut(&[Labelled])) {
            visit(prefix);
            if left == 0 {
                return;
            }
            for k in kinds() {
                prefix.push(k);
                go(prefix, left - 1, visit);
                prefix.pop();
            }
        }
        go(&mut Vec::new(), max, visit);
    }

    /// A structurally different second implementation (ADR 0089 D3, D5): a rank per item, the run's rank is the maximum,
    /// and a rank is turned into an exit by a table. Broken 4 > failing 3 > not measured 2 > everything else 0; empty is 2.
    fn ranked(run: &[Labelled], r: &Run, entry_covers: bool) -> i32 {
        let rank = |(id, o): &Labelled| -> i32 {
            match o {
                Outcome::CheckBroken(_) => 4,
                Outcome::Findings(f)
                    if f.findings()
                        .as_slice()
                        .iter()
                        .any(|x| x.severity == crate::outcome::Severity::Failing) =>
                {
                    3
                }
                Outcome::NotMeasured(_) if *id == "test:cov" && entry_covers => 0,
                Outcome::NotMeasured(_) => 2,
                Outcome::NotApplicable(_) if r.scope == Scope::Root => 2,
                _ => 0,
            }
        };
        let expired = if r.today > d("2027-01-31") { 3 } else { 0 };
        let top = if run.is_empty() {
            2
        } else {
            run.iter().map(rank).max().unwrap()
        };
        match top.max(expired) {
            4 => 7,
            3 => 5,
            2 => 6,
            _ => 0,
        }
    }

    #[test]
    fn property_every_combination_of_outcomes_optional_entries_and_run_kinds_folds_by_the_precedence()
     {
        let opt = Optional::parse(FILE).unwrap();
        let (mut combos, mut zero) = (0, 0);
        for (run, covers) in runs() {
            every_run(4, &mut |items| {
                combos += 1;
                let v = Verdict::of(items.to_vec(), &opt, &run);
                let want = ranked(items, &run, covers);
                assert_eq!(v.exit().code(), want, "{items:?} {run:?}");
                let j = v.json();
                assert_eq!(j["exit"], want);
                assert_eq!(j["outcomes"].as_array().unwrap().len(), items.len());
                assert_eq!(v.table().rows.len(), items.len());
                let has = |id: &str| items.iter().any(|(i, _)| *i == id);
                // ADR 0089 section 6, stated on every combination.
                if has("broken") {
                    assert_eq!(v.exit(), Exit::CheckBroken, "never optional");
                }
                if has("unc") {
                    assert_ne!(
                        v.exit(),
                        Exit::Ok,
                        "an undeclared not-measured item never passes"
                    );
                }
                if has("test:cov") && !covers {
                    assert_ne!(
                        v.exit(),
                        Exit::Ok,
                        "an entry that does not cover this run excuses nothing"
                    );
                }
                if has("na") && run.scope == Scope::Root {
                    assert_ne!(v.exit(), Exit::Ok, "no not-applicable in a whole-tree run");
                }
                if want == 0 {
                    zero += 1;
                    assert!(!items.is_empty());
                }
                let mut rev = items.to_vec();
                rev.reverse();
                assert_eq!(
                    Verdict::of(rev, &opt, &run).exit(),
                    v.exit(),
                    "order never matters"
                );
            });
        }
        assert_eq!(combos, 12 * (1 + 8 + 64 + 512 + 4096));
        assert!(zero > 0, "the probe sees passing runs too");
    }

    #[test]
    fn positive_controls() {
        let report_only = || measured(vec![Finding::report_only("r")]);
        let failing = || measured(vec![Finding::failing("f")]);
        // D5: report-only findings plus not measured is 6 (never 0), plus check broken is 7.
        assert_eq!(
            of(
                vec![("a", report_only()), ("b", Outcome::NotMeasured(reason()))],
                &root()
            )
            .exit(),
            Exit::NotMeasured
        );
        assert_eq!(
            of(
                vec![("a", report_only()), ("b", Outcome::CheckBroken(reason()))],
                &root()
            )
            .exit(),
            Exit::CheckBroken
        );
        // Failing plus not measured is 5, with both listed.
        let v = of(
            vec![
                ("a", failing()),
                ("b", Outcome::NotMeasured(Reason::new("no ffmpeg"))),
            ],
            &root(),
        );
        assert_eq!(v.exit(), Exit::Findings);
        let text = v.json().to_string();
        assert!(
            text.contains("no ffmpeg") && text.contains("\"failing\":true"),
            "{text}"
        );
        // Broken beats failing.
        assert_eq!(
            of(
                vec![("a", failing()), ("b", Outcome::CheckBroken(reason()))],
                &root()
            )
            .exit(),
            Exit::CheckBroken
        );
        // The probe sees the clean cases: only these exit 0.
        assert_eq!(of(vec![("a", clean())], &root()).exit(), Exit::Ok);
        assert_eq!(of(vec![("a", report_only())], &root()).exit(), Exit::Ok);
        let scoped = Run::new(RunKind::Ordinary, Scope::Scoped, "linux", d("2026-11-10"));
        assert_eq!(
            of(vec![("a", Outcome::NotApplicable(reason()))], &scoped).exit(),
            Exit::Ok
        );
        // A whole-tree run has no not-applicable: [NotApplicable] is not measured.
        let v = of(vec![("a", Outcome::NotApplicable(reason()))], &root());
        assert_eq!(v.exit(), Exit::NotMeasured);
        assert!(v.json()["outcomes"][0]["note"].is_string());
        // Nothing recorded is nothing measured.
        let none = of(Vec::new(), &root());
        assert_eq!(none.exit(), Exit::NotMeasured);
        assert!(none.json()["note"].is_string());
    }

    #[test]
    fn an_optional_entry_excuses_one_not_measured_item_and_the_report_says_so() {
        let v = of(
            vec![
                ("test:cov", Outcome::NotMeasured(Reason::new("no ffmpeg"))),
                ("x", clean()),
            ],
            &root(),
        );
        assert_eq!(v.exit(), Exit::Ok);
        let j = v.json();
        assert_eq!(j["optional_used"][0], "test:cov");
        assert_eq!(j["outcomes"][0]["optional"], "test:cov");
        assert_eq!(j["outcomes"][0]["id"], "test:cov");
        // The same item on another leg is not excused.
        let other =
            Run::new(RunKind::Ordinary, Scope::Root, "linux", d("2026-11-10")).on_leg("elsewhere");
        assert_eq!(
            of(vec![("test:cov", Outcome::NotMeasured(reason()))], &other).exit(),
            Exit::NotMeasured
        );
        // A release run honours it only with a green leg of measured_on.
        let release = Run::new(RunKind::Release, Scope::Root, "linux", d("2026-11-10")).on_leg("l");
        assert_eq!(
            of(vec![("test:cov", Outcome::NotMeasured(reason()))], &release).exit(),
            Exit::NotMeasured
        );
        let green = release.with_green("test:cov", "m");
        assert_eq!(
            of(vec![("test:cov", Outcome::NotMeasured(reason()))], &green).exit(),
            Exit::Ok
        );
        // An expired entry is an error and excuses nothing.
        let late = Run::new(RunKind::Ordinary, Scope::Root, "linux", d("2027-06-01")).on_leg("l");
        let v = of(vec![("test:cov", Outcome::NotMeasured(reason()))], &late);
        assert_eq!(v.exit(), Exit::Findings);
        assert!(
            v.json()["problems"][0]
                .as_str()
                .unwrap()
                .contains("expired on 2027-01-31")
        );
    }

    #[test]
    fn the_json_gives_every_finding_its_message_and_failing_flag() {
        let mixed = measured(vec![
            Finding::report_only("only reported"),
            Finding::failing("really failing"),
        ]);
        let j = of(vec![("a", mixed)], &root()).json();
        let f = &j["outcomes"][0]["findings"];
        assert_eq!(
            (f[0]["message"].as_str(), f[0]["failing"].as_bool()),
            (Some("only reported"), Some(false))
        );
        assert_eq!(
            (f[1]["message"].as_str(), f[1]["failing"].as_bool()),
            (Some("really failing"), Some(true))
        );
        assert_eq!(j["status"], "findings");
    }

    #[test]
    fn the_table_lists_every_item_and_writes_in_each_format() {
        let v = of(
            vec![
                ("test:cov", Outcome::NotMeasured(Reason::new("no ffmpeg"))),
                ("t", measured(vec![Finding::failing("f")])),
            ],
            &root(),
        );
        let t = v.table();
        assert_eq!(t.columns, ["id", "status", "detail", "optional"]);
        assert_eq!(t.rows[0][1], "not-measured");
        assert_eq!(t.rows[0][3], "test:cov");
        assert_eq!(t.rows[1][2], "f");
        let (mut out, mut note) = (Vec::new(), Vec::new());
        for f in [TableFormat::Table, TableFormat::Csv, TableFormat::Json] {
            write_table(&t, f, "", &mut out, &mut note).unwrap();
        }
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("no ffmpeg") && text.contains("not-measured"));
        assert!(note.is_empty());
    }

    #[test]
    fn every_status_name_reaches_the_json_and_the_table() {
        let all = [
            ("a", clean(), "clean"),
            ("b", Outcome::NotApplicable(reason()), "not-applicable"),
            ("c", measured(vec![Finding::failing("f")]), "findings"),
            ("d", Outcome::NotMeasured(reason()), "not-measured"),
            ("e", Outcome::CheckBroken(reason()), "check-broken"),
        ];
        let v = of(
            all.iter().map(|(i, o, _)| (*i, o.clone())).collect(),
            &root(),
        );
        let (j, t) = (v.json(), v.table());
        for (n, (_, _, name)) in all.iter().enumerate() {
            assert_eq!(j["outcomes"][n]["status"], *name);
            assert_eq!(t.rows[n][1], *name);
        }
        for e in [
            Exit::Ok,
            Exit::Findings,
            Exit::NotMeasured,
            Exit::CheckBroken,
        ] {
            assert!(!e.name().is_empty());
        }
        assert_eq!(j["status"], "check-broken");
    }

    #[test]
    fn an_entry_is_still_live_on_its_last_day_and_dead_the_day_after() {
        let item = || vec![("test:cov", Outcome::NotMeasured(reason()))];
        let on = |day: &str| {
            of(
                item(),
                &Run::new(RunKind::Ordinary, Scope::Root, "linux", d(day)).on_leg("l"),
            )
        };
        assert_eq!(on("2027-01-31").exit(), Exit::Ok);
        assert_eq!(
            on("2027-01-31").json()["problems"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(on("2027-02-01").exit(), Exit::Findings);
    }

    #[test]
    fn strict_folds_without_a_register() {
        let one = |o: Outcome| Verdict::strict(vec![("x", o)]).exit();
        assert_eq!(one(clean()), Exit::Ok);
        assert_eq!(one(measured(vec![Finding::failing("f")])), Exit::Findings);
        assert_eq!(one(Outcome::NotMeasured(reason())), Exit::NotMeasured);
        assert_eq!(one(Outcome::NotApplicable(reason())), Exit::NotMeasured);
        assert_eq!(
            Verdict::strict(Vec::<(&str, Outcome)>::new()).exit(),
            Exit::NotMeasured
        );
    }
}
