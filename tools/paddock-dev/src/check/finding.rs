//! One finding, in the shape of `conformance/src/finding.mjs` and the waiver engine's own findings, so the report JSON of the
//! two hosts can be merged and diffed.

use super::outcome::Outcome;
use serde_json::{Map, Value, json};

/// How a finding counts: an error fails the run, a warning does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// Fails the run.
    Error,
    /// Is reported only.
    Warn,
}

impl Severity {
    /// The name in the report.
    pub fn name(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warn => "warn",
        }
    }
}

/// One finding of a check.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    /// The check id (`C39`), or a waiver problem (`W001`..`W004`).
    pub check: String,
    /// Error or warning.
    pub severity: Severity,
    /// The rule number the check enforces.
    pub rule: u32,
    /// The repository path, POSIX style.
    pub file: String,
    /// The 1-based line.
    pub line: usize,
    /// What is wrong.
    pub message: String,
    /// The fix in one sentence.
    pub fix: String,
    /// An error no waiver can cover: the check could not look ("check broken", "not measured"), ER-33.
    pub unwaivable: bool,
    /// Why it is unwaivable: the check is broken or did not look (the exit status of the run, ADR 0089). `None` for an ordinary finding.
    pub outcome: Option<Outcome>,
    /// The JSON key order of the JavaScript object this mirrors: waiver findings put `file` and `line` before `rule`.
    where_first: bool,
    /// The reason of a comment at the finding's site that waives it: the host counts it as waived (with `expires` "inline"), not as a finding.
    pub inline_waiver: Option<String>,
}

impl Finding {
    /// An error finding.
    pub fn error(check: &str, rule: u32, file: &str, line: usize, message: impl Into<String>, fix: impl Into<String>) -> Finding {
        Finding {
            check: check.into(),
            severity: Severity::Error,
            rule,
            file: file.into(),
            line,
            message: message.into(),
            fix: fix.into(),
            unwaivable: false,
            outcome: None,
            where_first: false,
            inline_waiver: None,
        }
    }

    /// A warning finding: reported and counted, never fails the run.
    pub fn warn(check: &str, rule: u32, file: &str, line: usize, message: impl Into<String>, fix: impl Into<String>) -> Finding {
        Finding {
            severity: Severity::Warn,
            ..Finding::error(check, rule, file, line, message, fix)
        }
    }

    /// The same, as the waiver engine builds it (`W001`, `W003`, `W004`): `file` and `line` come before `rule` in the JSON.
    pub fn waiver_problem(
        check: &str,
        severity: Severity,
        file: &str,
        line: usize,
        message: impl Into<String>,
        fix: impl Into<String>,
    ) -> Finding {
        Finding {
            severity,
            where_first: true,
            ..Finding::error(check, 5, file, line, message, fix)
        }
    }

    /// Marks the finding as waived by a comment at its site, with the comment's reason.
    pub fn inline_waived(mut self, reason: impl Into<String>) -> Finding {
        self.inline_waiver = Some(reason.into());
        self
    }

    /// Marks the finding as one no waiver covers.
    pub fn unwaivable(mut self) -> Finding {
        self.unwaivable = true;
        self
    }

    /// Names why the finding is unwaivable (`outcome`), as `unwaivable(f, outcome)` does in the JavaScript host.
    pub fn with_outcome(mut self, outcome: Outcome) -> Finding {
        self.outcome = Some(outcome);
        self
    }

    /// The report JSON of the finding, with the key order of the JavaScript host.
    pub fn json(&self) -> Value {
        let mut m = Map::new();
        m.insert("check".into(), json!(self.check));
        m.insert("severity".into(), json!(self.severity.name()));
        if self.where_first {
            m.insert("file".into(), json!(self.file));
            m.insert("line".into(), json!(self.line));
            m.insert("rule".into(), json!(self.rule));
        } else {
            m.insert("rule".into(), json!(self.rule));
            m.insert("file".into(), json!(self.file));
            m.insert("line".into(), json!(self.line));
        }
        m.insert("message".into(), json!(self.message));
        m.insert("fix".into(), json!(self.fix));
        if self.unwaivable {
            m.insert("unwaivable".into(), json!(true));
        }
        if let Some(o) = self.outcome {
            m.insert("outcome".into(), json!(o.name()));
        }
        Value::Object(m)
    }
}

/// A finding a waiver covered.
#[derive(Clone, Debug, PartialEq)]
pub struct Waived {
    /// The covered finding.
    pub finding: Finding,
    /// The waiver's reason.
    pub reason: String,
    /// The waiver's last day, `YYYY-MM-DD`.
    pub expires: String,
}

impl Waived {
    /// The finding's JSON plus `waiver: { reason, expires }`.
    pub fn json(&self) -> Value {
        let mut v = self.finding.json();
        if let Value::Object(m) = &mut v {
            m.insert("waiver".into(), json!({"reason": self.reason, "expires": self.expires}));
        }
        v
    }
}
