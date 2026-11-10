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
    /// Only the listed files (`--files`): the count of those the tree knows.
    Files(usize),
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
    /// It finished (it did not panic and read everything it meant to read).
    pub completed: bool,
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
    /// Could not look: no input, no probes, or not completed.
    NotMeasured,
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
            Status::CheckBroken => "check broken",
        }
    }
}

impl RunRecord {
    /// The OS name as the records print it.
    pub fn this_os() -> &'static str {
        std::env::consts::OS
    }

    /// Whether the run counts as a measurement (what the stale-entry engine asks): it ran, completed, had its precondition and
    /// control, and looked at something.
    pub fn measured(&self) -> bool {
        self.ran && self.completed && self.precondition.met && self.control && self.probes > 0
    }

    /// The status given whether the check produced findings.
    pub fn status(&self, found: bool) -> Status {
        if !self.control {
            Status::CheckBroken
        } else if !self.measured() {
            Status::NotMeasured
        } else if found {
            Status::Findings
        } else {
            Status::Clean
        }
    }

    /// The record as JSON.
    pub fn json(&self, found: bool) -> Value {
        json!({
            "check": self.check,
            "ran": self.ran,
            "completed": self.completed,
            "precondition": {"met": self.precondition.met, "detail": self.precondition.detail},
            "control": self.control,
            "probes": self.probes,
            "scope": self.scope.name(),
            "files": match self.scope { Scope::Files(n) => json!(n), Scope::Root => Value::Null },
            "os": self.os,
            "status": self.status(found).name(),
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
            precondition: Precondition {
                met: true,
                detail: "the tree lists files".into(),
            },
            control: true,
            probes: 3,
            scope: Scope::Root,
            os: "linux",
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

    #[test]
    fn json_names_every_field_the_stale_entry_engine_reads() {
        let v = RunRecord {
            scope: Scope::Files(2),
            ..rec()
        }
        .json(false);
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
        ] {
            assert!(v.get(k).is_some(), "{k}");
        }
        assert_eq!(v["scope"], "files");
        assert_eq!(v["files"], 2);
    }
}
