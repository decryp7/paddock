//! A report, its counts, and the exit code.

use pacenotes_core::Severity;
use std::collections::BTreeMap;

use super::{Finding, Hidden, Rule, RuleId};

/// A result read from SARIF or rdjson, with the reason that hid it when the source had suppressed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    /// The finding.
    pub finding: Finding,
    /// Why the source suppressed it, if it did.
    pub suppressed_by: Option<super::Reason>,
}

/// A rule that did not run, and why (so a green report never means "everything ran").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// The rule.
    pub rule: RuleId,
    /// Why, in words.
    pub why: String,
}

/// Findings by severity, and how many were hidden.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Counts {
    /// Kept findings by severity.
    pub by_severity: BTreeMap<Severity, usize>,
    /// Findings a waiver or suppression hid.
    pub hidden: usize,
}

/// What a run found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// What the report is about: a repository, or conformance's app name.
    pub scope: String,
    /// The tool that made it.
    pub tool: String,
    /// Its version.
    pub tool_version: String,
    /// The rules that ran.
    pub rules: Vec<Rule>,
    /// Findings nothing hid.
    pub findings: Vec<Finding>,
    /// Findings that were hidden, with what hid them.
    pub hidden: Vec<Hidden>,
    /// Rules that did not run.
    pub skipped: Vec<Skipped>,
}

impl Report {
    /// The counts.
    pub fn counts(&self) -> Counts {
        let mut by_severity = BTreeMap::new();
        for f in &self.findings {
            *by_severity.entry(f.severity).or_insert(0) += 1;
        }
        Counts {
            by_severity,
            hidden: self.hidden.len(),
        }
    }
}

/// The process exit code: 2 when the run itself failed; else 1 when a kept finding is at or above `threshold`; else 3 when the run was
/// partial or ran out of budget (so "clean" is never claimed for work that was cut short); else 0.
pub fn exit_code(report: &Report, threshold: Severity, partial: bool, errored: bool) -> u8 {
    if errored {
        2
    } else if report.findings.iter().any(|f| f.severity >= threshold) {
        1
    } else if partial || !report.skipped.is_empty() {
        3
    } else {
        0
    }
}
