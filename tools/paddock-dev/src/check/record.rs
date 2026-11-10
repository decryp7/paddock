//! The run record of one check: what it ran over and whether that counts (ER-33). A check that did not run, did not complete,
//! had no precondition, failed its own control or looked at nothing is `not measured` (or `check broken`), never `clean`.
//! ADR 0088's stale-entry engine reads these records to decide which ratchet entries a run covered (§2): the fields are the ones
//! it names (ran, completed, precondition, probe count, scope, OS), so it extends this type instead of replacing it.

use serde_json::{Value, json};

/// How far a run looked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    /// The whole tree under the root.
    Root,
    /// Only the listed files (`--files`): those of them the tree knows, sorted. ADR 0088 decides coverage of an entry by them.
    Files(Vec<String>),
}

impl Scope {
    /// The name in the report.
    pub fn name(&self) -> &'static str {
        match self {
            Scope::Root => "root",
            Scope::Files(_) => "files",
        }
    }
}

/// What a check reports about its own run.
#[derive(Clone, Debug, PartialEq)]
pub struct RunRecord {
    /// The check id.
    pub check: String,
    /// The check was started.
    pub ran: bool,
    /// It read everything it meant to read (a file it could not read makes this false).
    pub completed: bool,
    /// The panic message when the check crashed.
    pub crash: Option<String>,
    /// Every folder of the tree was listed (the tree is completely known).
    pub listing_complete: bool,
    /// What its input had to be for the run to mean anything, and whether it was.
    pub precondition: Precondition,
    /// Its built-in positive control saw what it is built to see.
    pub control: bool,
    /// How many things it looked at (files scanned, sites read).
    pub probes: usize,
    /// How far it looked.
    pub scope: Scope,
    /// The OS the run was on.
    pub os: &'static str,
    /// The keys it matched (`file:line` of each raw finding, before waivers), for ADR 0088's stale-entry engine.
    pub matched: Vec<String>,
}

/// A precondition and whether it held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Precondition {
    /// Whether it held.
    pub met: bool,
    /// What it is, or why it failed.
    pub detail: String,
}

/// What the record says about the run's result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Measured, nothing found.
    Clean,
    /// Measured, something found.
    Findings,
    /// Could not look: no input, no probes on a whole-tree run, a crash, or not completed. Always comes with an unwaivable error.
    NotMeasured,
    /// A scoped (`--files`) run whose files the check does not read: nothing to measure, nothing wrong.
    NotApplicable,
    /// The probe does not see what it is built to see.
    CheckBroken,
}

impl Status {
    /// The name in the report and the log.
    pub fn name(self) -> &'static str {
        match self {
            Status::Clean => "clean",
            Status::Findings => "findings",
            Status::NotMeasured => "not measured",
            Status::NotApplicable => "not applicable",
            Status::CheckBroken => "check broken",
        }
    }
}

impl RunRecord {
    /// The OS name as the records print it.
    pub fn this_os() -> &'static str {
        std::env::consts::OS
    }

    /// Whether the run counts as a measurement (what the stale-entry engine asks): it ran, did not crash, completed, listed the
    /// whole tree, had its precondition and control, and looked at something.
    pub fn measured(&self) -> bool {
        self.ran
            && self.crash.is_none()
            && self.completed
            && self.listing_complete
            && self.precondition.met
            && self.control
            && self.probes > 0
    }

    /// The one decision about a run (ER-33): its status given whether findings are left after waivers, and the error no waiver
    /// covers when the run did not look. The error is `Some` exactly when the status is `not measured` or `check broken`; the host
    /// adds it to the findings, so the record and the exit code cannot disagree.
    pub fn judge(&self, kept: bool) -> (Status, Option<String>) {
        let mut reasons: Vec<String> = Vec::new();
        if !self.ran {
            reasons.push("it did not run".into());
        }
        if let Some(why) = &self.crash {
            reasons.push(format!("it crashed: {why}"));
        } else if !self.completed {
            reasons.push("it did not complete (a file could not be read)".into());
        }
        if !self.listing_complete {
            reasons.push("the tree could not be listed completely (a folder could not be read)".into());
        }
        if !self.control {
            reasons.push("its positive control no longer sees what the check is built to see".into());
        }
        if !self.precondition.met {
            reasons.push(format!("its precondition failed: {}", self.precondition.detail));
        }
        let mut applicable = true;
        if self.probes == 0 && self.crash.is_none() && self.completed {
            match &self.scope {
                Scope::Root => reasons.push("it looked at no file on a whole-tree run".into()),
                Scope::Files(known) if known.is_empty() => {
                    reasons.push("none of the --files entries is a file of the tree, so it looked at nothing".into());
                }
                Scope::Files(_) => applicable = false,
            }
        }
        let status = if !self.control {
            Status::CheckBroken
        } else if !reasons.is_empty() {
            Status::NotMeasured
        } else if kept {
            Status::Findings
        } else if !applicable {
            Status::NotApplicable
        } else {
            Status::Clean
        };
        let error = (!reasons.is_empty()).then(|| format!("{}: {} {}", status.name(), self.check, reasons.join("; ")));
        (status, error)
    }

    /// The status given whether findings are left after waivers.
    pub fn status(&self, kept: bool) -> Status {
        self.judge(kept).0
    }

    /// The record as JSON: the raw findings (before waivers) and the kept ones (after), and the status from the kept ones.
    pub fn json(&self, raw: usize, kept: usize) -> Value {
        json!({
            "check": self.check,
            "ran": self.ran,
            "completed": self.completed,
            "listingComplete": self.listing_complete,
            "precondition": {"met": self.precondition.met, "detail": self.precondition.detail},
            "control": self.control,
            "probes": self.probes,
            "scope": self.scope.name(),
            "files": match &self.scope { Scope::Files(f) => json!(f), Scope::Root => Value::Null },
            "os": self.os,
            "found": {"raw": raw, "kept": kept},
            "matched": self.matched,
            "status": self.status(kept > 0).name(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec() -> RunRecord {
        RunRecord {
            check: "C39".into(),
            ran: true,
            completed: true,
            crash: None,
            listing_complete: true,
            precondition: Precondition {
                met: true,
                detail: "the tree lists files".into(),
            },
            control: true,
            probes: 3,
            scope: Scope::Root,
            os: "linux",
            matched: vec![],
        }
    }

    #[test]
    fn clean_needs_every_field_to_hold() {
        assert_eq!(rec().status(false), Status::Clean);
        assert_eq!(rec().status(true), Status::Findings);
        for (name, r) in [
            ("probes", RunRecord { probes: 0, ..rec() }),
            ("completed", RunRecord { completed: false, ..rec() }),
            ("ran", RunRecord { ran: false, ..rec() }),
            (
                "precondition",
                RunRecord {
                    precondition: Precondition {
                        met: false,
                        detail: String::new(),
                    },
                    ..rec()
                },
            ),
        ] {
            assert_eq!(r.status(false), Status::NotMeasured, "{name}");
            assert!(!r.measured(), "{name}");
        }
        assert_eq!(RunRecord { control: false, ..rec() }.status(false), Status::CheckBroken);
    }

    /// Every scope times every way of not looking: the status and the unwaivable error come from one decision and agree.
    #[test]
    fn status_and_error_agree_for_every_scope_and_way_of_not_looking() {
        type Way = (&'static str, fn(RunRecord) -> RunRecord);
        let ways: [Way; 7] = [
            ("crash", |r| RunRecord {
                crash: Some("boom".into()),
                completed: false,
                probes: 0,
                ..r
            }),
            ("incomplete", |r| RunRecord { completed: false, ..r }),
            ("unlistable folder", |r| RunRecord {
                listing_complete: false,
                ..r
            }),
            ("failed control", |r| RunRecord { control: false, ..r }),
            ("unmet precondition", |r| RunRecord {
                precondition: Precondition {
                    met: false,
                    detail: "no input".into(),
                },
                ..r
            }),
            ("zero probes", |r| RunRecord { probes: 0, ..r }),
            ("fine", |r| r),
        ];
        let scopes = [
            ("root", Scope::Root),
            ("files, known", Scope::Files(vec!["notes.md".into()])),
            ("files, all unknown", Scope::Files(vec![])),
        ];
        for (sname, scope) in scopes {
            for (wname, way) in &ways {
                let r = way(RunRecord {
                    scope: scope.clone(),
                    ..rec()
                });
                for kept in [false, true] {
                    let (status, error) = r.judge(kept);
                    let must_fail = matches!(status, Status::NotMeasured | Status::CheckBroken);
                    assert_eq!(error.is_some(), must_fail, "{sname} / {wname} / kept {kept}: {status:?} {error:?}");
                    assert_eq!(r.status(kept), status);
                }
                let expect = match (*wname, &scope) {
                    ("failed control", _) => Status::CheckBroken,
                    ("fine", _) => Status::Clean,
                    ("zero probes", Scope::Files(f)) if !f.is_empty() => Status::NotApplicable,
                    _ => Status::NotMeasured,
                };
                assert_eq!(r.status(false), expect, "{sname} / {wname}");
            }
        }
    }

    #[test]
    fn json_names_every_field_the_stale_entry_engine_reads() {
        let v = RunRecord {
            scope: Scope::Files(vec!["a.rs".into(), "b.rs".into()]),
            matched: vec!["a.rs:3".into()],
            ..rec()
        }
        .json(2, 0);
        for k in [
            "check",
            "ran",
            "completed",
            "precondition",
            "probes",
            "scope",
            "os",
            "status",
            "control",
            "files",
            "found",
            "matched",
        ] {
            assert!(v.get(k).is_some(), "{k}");
        }
        assert_eq!(v["scope"], "files");
        assert_eq!(v["files"], json!(["a.rs", "b.rs"]));
        assert_eq!(
            v["status"], "clean",
            "every raw finding was waived: the status follows the kept ones"
        );
        assert_eq!(v["found"], json!({"raw": 2, "kept": 0}));
    }
}
