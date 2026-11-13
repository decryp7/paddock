//! A pass carries its evidence (ADR 0089, amendment 2026-11-13).
//!
//! `Clean`, `Findings` and `KnownCommit` hold a private-field payload whose [`Evidence`] only this module can build:
//!
//! * [`measure`] is the only constructor of a pass or of findings: the check runs inside it, a panic is
//!   [`Outcome::CheckBroken`], an input it could not read is [`Outcome::NotMeasured`];
//! * [`Evidence::adopt`] is the only constructor of [`Outcome::KnownCommit`]: it reads a stored record and re-reads the
//!   commit's tree live, and a mismatch is not measured;
//! * [`Outcome::from_wire`] is the consumer's parser: a result with no evidence, evidence for another subject, or a known
//!   commit the caller did not accept is not measured, never a pass. An exit code alone is never read as a pass.
//!
//! This closes code paths, not a malicious writer of a record file (single-user tooling).

use super::{Finding, Findings, Outcome, Reason, Severity};
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, catch_unwind};

macro_rules! text_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name(String);

        impl $name {
            /// Wraps the text (trimmed).
            pub fn new(text: impl Into<String>) -> $name {
                $name(text.into().trim().to_string())
            }

            /// The text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

text_id!(
    /// What was checked, such as `check:C99` or `gate:pre-push/fmt`.
    CheckId
);
text_id!(
    /// A commit id.
    Sha
);
text_id!(
    /// A tree id.
    TreeId
);
text_id!(
    /// A digest of the files a working-tree run read.
    Sha256
);
text_id!(
    /// The tool that measured, with its version.
    ToolVersion
);

/// Seconds since the Unix epoch, passed in (nothing here reads the clock).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnixSeconds(pub u64);

impl UnixSeconds {
    /// The wall clock now (the one place a caller that has no clock of its own asks for it).
    pub fn now() -> UnixSeconds {
        UnixSeconds(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        )
    }
}

/// A CI run's id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunId(pub u64);

/// What a measurement was about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    /// A committed tree (pre-push on a pushed commit, merge check, CI conformance).
    Commit {
        /// The commit.
        sha: Sha,
        /// Its tree.
        tree: TreeId,
    },
    /// A working-tree or `--files` run: the head plus a digest of the files read.
    Files {
        /// The head commit.
        head: Sha,
        /// The digest of the files read.
        digest: Sha256,
    },
    /// A complete CI run on a commit.
    CiRun {
        /// The workflow file.
        workflow: String,
        /// The run.
        run: RunId,
        /// The commit it tested.
        sha: Sha,
    },
}

impl Subject {
    /// Whether a result about `self` answers a question about `asked`: the same subject, or a CI run on the asked commit.
    fn covers(&self, asked: &Subject) -> bool {
        match (self, asked) {
            (Subject::CiRun { sha, .. }, Subject::Commit { sha: want, .. }) => sha == want,
            _ => self == asked,
        }
    }

    fn json(&self) -> Value {
        match self {
            Subject::Commit { sha, tree } => {
                json!({"kind": "commit", "sha": sha.as_str(), "tree": tree.as_str()})
            }
            Subject::Files { head, digest } => {
                json!({"kind": "files", "head": head.as_str(), "digest": digest.as_str()})
            }
            Subject::CiRun { workflow, run, sha } => {
                json!({"kind": "ci-run", "workflow": workflow, "run": run.0, "sha": sha.as_str()})
            }
        }
    }

    fn parse(v: &Value) -> Result<Subject, String> {
        let text = |k: &str| {
            v.get(k)
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string)
                .ok_or_else(|| format!("the subject has no `{k}`"))
        };
        match v.get("kind").and_then(Value::as_str) {
            Some("commit") => Ok(Subject::Commit {
                sha: Sha::new(text("sha")?),
                tree: TreeId::new(text("tree")?),
            }),
            Some("files") => Ok(Subject::Files {
                head: Sha::new(text("head")?),
                digest: Sha256::new(text("digest")?),
            }),
            Some("ci-run") => Ok(Subject::CiRun {
                workflow: text("workflow")?,
                run: RunId(
                    v.get("run")
                        .and_then(Value::as_u64)
                        .ok_or("the subject has no `run`")?,
                ),
                sha: Sha::new(text("sha")?),
            }),
            other => Err(format!("the subject kind {other:?} is unknown")),
        }
    }
}

/// What a pass is a pass of: the check, its subject, the tool and when. The fields are private.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    check: CheckId,
    subject: Subject,
    tool: ToolVersion,
    at: UnixSeconds,
}

/// A source for [`Evidence::adopt`]: the repository as it is now.
pub trait LiveRepo {
    /// The tree of `sha` as the repository reads it now, or `None` when the commit is not there.
    fn tree_of(&self, sha: &Sha) -> Option<TreeId>;
}

impl Evidence {
    /// The check.
    pub fn check(&self) -> &CheckId {
        &self.check
    }

    /// What it was about.
    pub fn subject(&self) -> &Subject {
        &self.subject
    }

    /// The tool and its version.
    pub fn tool(&self) -> &ToolVersion {
        &self.tool
    }

    /// When it was measured.
    pub fn at(&self) -> UnixSeconds {
        self.at
    }

    /// Evidence of a complete CI run on `sha`. Public, with one allowed caller (`ci_runs::verdict`), held by check C124.
    pub fn ci_run(
        check: CheckId,
        workflow: &str,
        run: RunId,
        sha: Sha,
        tool: ToolVersion,
        at: UnixSeconds,
    ) -> Evidence {
        Evidence {
            check,
            subject: Subject::CiRun {
                workflow: workflow.to_string(),
                run,
                sha,
            },
            tool,
            at,
        }
    }

    /// The record as JSON (what a pass record file or a result holds).
    pub fn to_json(&self) -> Value {
        json!({"check": self.check.as_str(), "subject": self.subject.json(), "tool": self.tool.as_str(), "at": self.at.0})
    }

    /// Reads a stored record. It proves nothing by itself: only [`Evidence::adopt`] turns it into a (known-commit) outcome.
    pub fn from_json(v: &Value) -> Result<Evidence, String> {
        let text = |k: &str| {
            v.get(k)
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string)
                .ok_or_else(|| format!("the evidence has no `{k}`"))
        };
        Ok(Evidence {
            check: CheckId::new(text("check")?),
            subject: Subject::parse(v.get("subject").ok_or("the evidence has no `subject`")?)?,
            tool: ToolVersion::new(text("tool")?),
            at: UnixSeconds(
                v.get("at")
                    .and_then(Value::as_u64)
                    .ok_or("the evidence has no `at`")?,
            ),
        })
    }

    /// A stored record for a commit, re-read live: [`Outcome::KnownCommit`] only when the commit is still there and (for a
    /// commit subject) its tree is the recorded one. A files subject cannot be adopted: it names no commit.
    pub fn adopt(stored: Evidence, live: &dyn LiveRepo) -> Outcome {
        let (sha, tree) = match &stored.subject {
            Subject::Commit { sha, tree } => (sha, Some(tree)),
            Subject::CiRun { sha, .. } => (sha, None),
            Subject::Files { .. } => {
                return Outcome::NotMeasured(Reason::new(
                    "a record of a working-tree run names no commit, so it cannot stand for one; run the check",
                ));
            }
        };
        match live.tree_of(sha) {
            None => Outcome::NotMeasured(Reason::new(format!(
                "the recorded commit {} is not in this repository; run the check",
                sha.as_str()
            ))),
            Some(now) if tree.is_some_and(|t| *t != now) => {
                Outcome::NotMeasured(Reason::new(format!(
                    "the tree of {} is {} now, not the {} the record names; run the check",
                    sha.as_str(),
                    now.as_str(),
                    tree.map(TreeId::as_str).unwrap_or_default()
                )))
            }
            Some(_) => Outcome::KnownCommit(Known { evidence: stored }),
        }
    }
}

/// Why a probe could not give a result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unmeasured {
    /// It did not look: a tool is missing, a precondition is false, its input was incomplete.
    NotMeasured(Reason),
    /// It looked, but its positive control failed.
    Broken(Reason),
}

/// A pass: measured, nothing found. Only [`measure`] builds one.
#[derive(Debug, Clone)]
pub struct Passed {
    evidence: Evidence,
}

impl Passed {
    /// What the pass is a pass of.
    pub fn evidence(&self) -> &Evidence {
        &self.evidence
    }
}

/// Measured, found something. Only [`measure`] builds one.
#[derive(Debug, Clone)]
pub struct Found {
    evidence: Evidence,
    findings: Findings,
}

impl Found {
    /// What was measured.
    pub fn evidence(&self) -> &Evidence {
        &self.evidence
    }

    /// The findings.
    pub fn findings(&self) -> &Findings {
        &self.findings
    }
}

/// A commit passed earlier and is not checked again. Only [`Evidence::adopt`] (and [`Outcome::from_wire`], for a consumer
/// that accepts it) builds one.
#[derive(Debug, Clone)]
pub struct Known {
    evidence: Evidence,
}

impl Known {
    /// The record it stands on.
    pub fn evidence(&self) -> &Evidence {
        &self.evidence
    }
}

/// Runs a check and gives its outcome: the only way to a pass or to findings.
///
/// `f` returns the findings (an empty list is a pass), or why it could not look. A panic inside `f` is
/// [`Outcome::CheckBroken`]. A pass about nothing is impossible: `subject` and `tool` are required.
pub fn measure<F>(
    check: CheckId,
    subject: Subject,
    tool: ToolVersion,
    now: UnixSeconds,
    f: F,
) -> Outcome
where
    F: FnOnce() -> Result<Vec<Finding>, Unmeasured>,
{
    match catch_unwind(AssertUnwindSafe(f)) {
        Err(panic) => {
            let text = panic
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "no message".into());
            Outcome::CheckBroken(Reason::new(format!(
                "{} panicked ({text}): a bug in the check, not a finding in your change",
                check.as_str()
            )))
        }
        Ok(Err(Unmeasured::NotMeasured(r))) => Outcome::NotMeasured(r),
        Ok(Err(Unmeasured::Broken(r))) => Outcome::CheckBroken(r),
        Ok(Ok(list)) => {
            let evidence = Evidence {
                check,
                subject,
                tool,
                at: now,
            };
            match Findings::new(list) {
                None => Outcome::Clean(Passed { evidence }),
                Some(findings) => Outcome::Findings(Found { evidence, findings }),
            }
        }
    }
}

impl Outcome {
    /// Reads a result that crossed a process boundary (one outcome object, as [`Outcome::json`] writes it), for a consumer
    /// that asks about `expect`. A pass without evidence, evidence for another subject, a `known-commit` the caller did not
    /// accept (`accept_known`), and anything unparsable are [`Outcome::NotMeasured`]; this is the only reader of a gate's
    /// result, so an exit code alone is never a pass.
    pub fn from_wire(wire: &Value, expect: &Subject, accept_known: bool) -> Outcome {
        match parse_wire(wire, expect, accept_known) {
            Ok(o) => o,
            Err(why) => Outcome::NotMeasured(Reason::new(format!(
                "the gate's result was not usable ({why}); it counts as not measured, never as a pass"
            ))),
        }
    }
}

fn parse_wire(wire: &Value, expect: &Subject, accept_known: bool) -> Result<Outcome, String> {
    let status = wire
        .get("status")
        .and_then(Value::as_str)
        .ok_or("no `status`")?;
    let reason = || {
        wire.get("reason")
            .and_then(Value::as_str)
            .map(Reason::new)
            .ok_or_else(|| format!("`{status}` has no `reason`"))
    };
    let evidence = || -> Result<Evidence, String> {
        let e = Evidence::from_json(
            wire.get("evidence")
                .ok_or_else(|| format!("`{status}` has no evidence"))?,
        )?;
        Ok(e)
    };
    match status {
        "not-applicable" => Ok(Outcome::NotApplicable(reason()?)),
        "not-measured" => Ok(Outcome::NotMeasured(reason()?)),
        "check-broken" => Ok(Outcome::CheckBroken(reason()?)),
        "clean" | "findings" => {
            let evidence = evidence()?;
            if evidence.subject != *expect {
                return Err(format!(
                    "the evidence is for {}, not {}",
                    describe(&evidence.subject),
                    describe(expect)
                ));
            }
            if status == "clean" {
                return Ok(Outcome::Clean(Passed { evidence }));
            }
            let list = wire
                .get("findings")
                .and_then(Value::as_array)
                .ok_or("`findings` has no list")?;
            let list: Vec<Finding> = list
                .iter()
                .map(|f| {
                    let message = f
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("(no message)");
                    let failing = f.get("failing").and_then(Value::as_bool).unwrap_or(true);
                    Finding {
                        message: message.to_string(),
                        severity: if failing {
                            Severity::Failing
                        } else {
                            Severity::ReportOnly
                        },
                    }
                })
                .collect();
            let findings = Findings::new(list).ok_or("`findings` holds no finding")?;
            Ok(Outcome::Findings(Found { evidence, findings }))
        }
        "known-commit" => {
            if !accept_known {
                return Err(
                    "this caller asks about another tree and does not accept a known commit".into(),
                );
            }
            let evidence = evidence()?;
            if !evidence.subject.covers(expect) {
                return Err(format!(
                    "the evidence is for {}, not {}",
                    describe(&evidence.subject),
                    describe(expect)
                ));
            }
            Ok(Outcome::KnownCommit(Known { evidence }))
        }
        other => Err(format!("the status `{other}` is unknown")),
    }
}

fn describe(s: &Subject) -> String {
    match s {
        Subject::Commit { sha, tree } => {
            format!("commit {} (tree {})", sha.as_str(), tree.as_str())
        }
        Subject::Files { head, digest } => {
            format!("files at {} ({})", head.as_str(), digest.as_str())
        }
        Subject::CiRun { run, sha, .. } => format!("CI run {} on {}", run.0, sha.as_str()),
    }
}

impl Passed {
    pub(super) fn json(&self) -> Value {
        json!({"status": "clean", "evidence": self.evidence.to_json()})
    }
}

impl Found {
    pub(super) fn json(&self) -> Value {
        let list: Vec<Value> = self
            .findings
            .as_slice()
            .iter()
            .map(|x| json!({"message": x.message, "failing": x.severity == Severity::Failing}))
            .collect();
        json!({"status": "findings", "findings": list, "evidence": self.evidence.to_json()})
    }
}

impl Known {
    pub(super) fn json(&self) -> Value {
        json!({"status": "known-commit", "not_rechecked": true, "evidence": self.evidence.to_json()})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Exit;
    use crate::outcome::test_support::{clean, measured, subject};
    use crate::outcome::{Date, Optional, Run, RunKind, Scope, Verdict};

    struct Repo(Option<&'static str>);
    impl LiveRepo for Repo {
        fn tree_of(&self, _: &Sha) -> Option<TreeId> {
            self.0.map(TreeId::new)
        }
    }

    fn run() -> Run {
        Run::new(
            RunKind::Ordinary,
            Scope::Root,
            "linux",
            Date::parse("2026-11-10").unwrap(),
        )
    }

    fn exit_of(items: Vec<(&str, Outcome)>) -> Exit {
        Verdict::of(items, &Optional::parse("").unwrap(), &run()).exit()
    }

    fn stored() -> Evidence {
        match clean() {
            Outcome::Clean(p) => p.evidence().clone(),
            _ => unreachable!(),
        }
    }

    fn tool() -> ToolVersion {
        ToolVersion::new("t 1")
    }

    #[test]
    fn measure_is_the_only_door_to_a_pass_and_a_panic_or_missing_input_is_never_one() {
        assert!(matches!(clean(), Outcome::Clean(p) if p.evidence().check().as_str() == "check:t"));
        assert!(matches!(
            measured(vec![Finding::failing("f")]),
            Outcome::Findings(_)
        ));
        let id = || CheckId::new("check:x");
        let boom = measure(
            id(),
            subject(),
            tool(),
            UnixSeconds(0),
            || -> Result<Vec<Finding>, Unmeasured> { panic!("boom") },
        );
        assert!(matches!(&boom, Outcome::CheckBroken(r) if r.as_str().contains("boom")));
        let nm = measure(id(), subject(), tool(), UnixSeconds(0), || {
            Err(Unmeasured::NotMeasured(Reason::new("no tool")))
        });
        assert!(matches!(nm, Outcome::NotMeasured(_)));
        let br = measure(id(), subject(), tool(), UnixSeconds(0), || {
            Err(Unmeasured::Broken(Reason::new("control")))
        });
        assert!(matches!(br, Outcome::CheckBroken(_)));
        let s = measure(
            id(),
            subject(),
            tool(),
            UnixSeconds(0),
            || -> Result<Vec<Finding>, Unmeasured> { std::panic::panic_any(5u8) },
        );
        assert!(matches!(s, Outcome::CheckBroken(_)));
    }

    /// PRINCIPLES "delete the check": the closure replaced by nothing at all (no outcome) gives 6, never 0.
    #[test]
    fn deleting_the_check_gives_not_measured_never_a_pass() {
        assert_eq!(exit_of(Vec::new()), Exit::NotMeasured);
        // The probe stripped to "no probe ran" is `Unmeasured`, also 6.
        let o = measure(CheckId::new("c"), subject(), tool(), UnixSeconds(0), || {
            Err(Unmeasured::NotMeasured(Reason::new("no probes")))
        });
        assert_eq!(exit_of(vec![("c", o)]), Exit::NotMeasured);
    }

    /// PRINCIPLES "skip the gate": a consumer handed a stand-in result that proves nothing gets 6 each time.
    #[test]
    fn a_stand_in_result_is_not_a_pass_for_any_consumer() {
        let ask = subject();
        let other = Subject::Commit {
            sha: Sha::new("a2"),
            tree: TreeId::new("t1"),
        };
        let good = clean().json();
        let mut other_ev = good.clone();
        other_ev["evidence"]["subject"]["sha"] = json!("a2");
        let standins = [
            Value::Null,
            json!({}),
            json!({"status": "clean"}),
            json!({"status": "passed"}),
            json!({"status": "clean", "evidence": {}}),
            other_ev,
            json!({"status": "known-commit", "evidence": stored().to_json()}),
        ];
        for w in &standins {
            let o = Outcome::from_wire(w, &ask, false);
            assert!(matches!(o, Outcome::NotMeasured(_)), "{w} became {o:?}");
            assert_eq!(exit_of(vec![("g", o)]), Exit::NotMeasured, "{w}");
        }
        assert!(matches!(
            Outcome::from_wire(&good, &ask, false),
            Outcome::Clean(_)
        ));
        assert!(matches!(
            Outcome::from_wire(&good, &other, false),
            Outcome::NotMeasured(_)
        ));
    }

    #[test]
    fn a_known_commit_is_named_exits_0_and_is_refused_by_a_consumer_about_another_tree() {
        let known = Evidence::adopt(stored(), &Repo(Some("t1")));
        assert!(matches!(known, Outcome::KnownCommit(_)));
        assert_eq!(exit_of(vec![("g", known.clone())]), Exit::Ok);
        let j = known.json();
        assert_eq!(j["status"], "known-commit");
        assert_eq!(j["not_rechecked"], true);
        assert!(matches!(
            Outcome::from_wire(&j, &subject(), true),
            Outcome::KnownCommit(_)
        ));
        assert!(matches!(
            Outcome::from_wire(&j, &subject(), false),
            Outcome::NotMeasured(_)
        ));
        let other = Subject::Commit {
            sha: Sha::new("zz"),
            tree: TreeId::new("t1"),
        };
        assert!(matches!(
            Outcome::from_wire(&j, &other, true),
            Outcome::NotMeasured(_)
        ));
    }

    #[test]
    fn adopt_rereads_the_tree_live() {
        assert!(
            matches!(Evidence::adopt(stored(), &Repo(Some("t2"))), Outcome::NotMeasured(r) if r.as_str().contains("t2"))
        );
        assert!(matches!(
            Evidence::adopt(stored(), &Repo(None)),
            Outcome::NotMeasured(_)
        ));
        let files = Evidence::from_json(&json!({"check": "c", "subject": {"kind": "files", "head": "h", "digest": "d"}, "tool": "t", "at": 1})).unwrap();
        assert!(matches!(
            Evidence::adopt(files, &Repo(Some("t1"))),
            Outcome::NotMeasured(_)
        ));
        let ci = Evidence::ci_run(
            CheckId::new("c"),
            "ci.yml",
            RunId(7),
            Sha::new("a1"),
            tool(),
            UnixSeconds(1),
        );
        assert!(matches!(
            Evidence::adopt(ci.clone(), &Repo(Some("anything"))),
            Outcome::KnownCommit(_)
        ));
        // A CI run on the asked commit answers a question about that commit.
        let j = Evidence::adopt(ci, &Repo(Some("t1"))).json();
        assert!(matches!(
            Outcome::from_wire(&j, &subject(), true),
            Outcome::KnownCommit(_)
        ));
    }

    #[test]
    fn evidence_round_trips_and_rejects_partial_records() {
        let e = stored();
        assert_eq!(Evidence::from_json(&e.to_json()).unwrap(), e);
        assert_eq!(e.at(), UnixSeconds(1));
        assert_eq!(e.tool().as_str(), "t 1");
        for broken in [
            json!({}),
            json!({"check": "c"}),
            json!({"check":"c","subject":{"kind":"x"},"tool":"t","at":1}),
            json!({"check":"c","subject":{"kind":"commit","sha":"a"},"tool":"t","at":1}),
            json!({"check":"c","subject":{"kind":"ci-run","workflow":"w","sha":"a"},"tool":"t","at":1}),
            json!({"check":"c","subject":{"kind":"files","head":"a"},"tool":"t","at":1}),
            json!({"check":"c","subject":{"kind":"commit","sha":"a","tree":"t"},"tool":"t"}),
        ] {
            assert!(Evidence::from_json(&broken).is_err(), "{broken}");
        }
    }

    #[test]
    fn findings_cross_the_wire_with_their_severity() {
        let o = measured(vec![Finding::report_only("r"), Finding::failing("f")]);
        let back = Outcome::from_wire(&o.json(), &subject(), false);
        assert_eq!(back.detail(), "(report only) r; f");
        assert!(back.failing());
        let mut empty = o.json();
        empty["findings"] = json!([]);
        assert!(matches!(
            Outcome::from_wire(&empty, &subject(), false),
            Outcome::NotMeasured(_)
        ));
        for status in ["not-applicable", "not-measured", "check-broken"] {
            let w = json!({"status": status, "reason": "why"});
            assert_eq!(Outcome::from_wire(&w, &subject(), false).name(), status);
            assert!(matches!(
                Outcome::from_wire(&json!({"status": status}), &subject(), false),
                Outcome::NotMeasured(_)
            ));
        }
    }

    /// The property of ADR 0089 amendment section 4.3: every outcome the public API can produce that passes carries evidence.
    #[test]
    fn every_pass_reachable_through_the_public_api_carries_evidence() {
        let all = [
            clean(),
            measured(vec![Finding::report_only("r")]),
            Evidence::adopt(stored(), &Repo(Some("t1"))),
            Outcome::from_wire(&clean().json(), &subject(), false),
            Outcome::from_wire(&json!({"status": "clean"}), &subject(), false),
        ];
        for o in all {
            let has = match &o {
                Outcome::Clean(p) => Some(p.evidence()),
                Outcome::Findings(f) => Some(f.evidence()),
                Outcome::KnownCommit(k) => Some(k.evidence()),
                _ => None,
            };
            if matches!(
                o,
                Outcome::Clean(_) | Outcome::Findings(_) | Outcome::KnownCommit(_)
            ) {
                assert!(has.is_some_and(
                    |e| !e.check().as_str().is_empty() && !e.tool().as_str().is_empty()
                ));
            }
        }
    }

    #[test]
    fn a_persons_override_excuses_only_a_not_measured_item_and_is_named() {
        let nm = || Outcome::NotMeasured(Reason::new("tool"));
        let v = Verdict::of_overridden(
            vec![
                ("a", nm(), Some("alex".to_string())),
                ("b", clean(), Some("alex".to_string())),
            ],
            &Optional::parse("").unwrap(),
            &run(),
        );
        assert_eq!(v.exit(), Exit::Ok);
        let j = v.json();
        assert_eq!(j["outcomes"][0]["override"], "alex");
        assert!(j["outcomes"][1].get("override").is_none());
        assert_eq!(v.table().rows[0][3], "override by alex");
        let failing = Verdict::of_overridden(
            vec![(
                "a",
                measured(vec![Finding::failing("f")]),
                Some("alex".to_string()),
            )],
            &Optional::parse("").unwrap(),
            &run(),
        );
        assert_eq!(failing.exit(), Exit::Findings);
    }
}
