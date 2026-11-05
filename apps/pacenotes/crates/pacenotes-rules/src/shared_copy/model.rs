//! A rule, a finding, and where a fix sentence came from.

use pacenotes_core::{Confidence, Location, Severity};
use serde::{Deserialize, Serialize};

use super::{FindingFingerprint, FixSentence, RuleId};

/// Where a fix sentence came from; a `Generated` one is never written back out as a tool's help.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixSource {
    /// A rule of ours.
    Rule,
    /// Text a tool gave.
    Tool,
    /// The fixed sentence for "nothing available".
    Generated,
}

/// Which paths a rule looks at.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    /// Path globs the rule applies to (empty: everywhere).
    pub include: Vec<String>,
    /// Path globs it skips.
    pub exclude: Vec<String>,
}

/// A rule of the shared model: the part conformance's checks and Pacenotes' rules have in common.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    /// Identity.
    pub id: RuleId,
    /// Short title.
    pub title: String,
    /// Severity unless a finding says otherwise.
    pub default_severity: Severity,
    /// What to do about it, one sentence.
    pub fix: FixSentence,
    /// A citation shown in reports (a rule number, a document anchor); not a URL.
    pub policy_ref: Option<String>,
    /// Raised when what the rule flags changes.
    pub version: u32,
    /// Earlier ids, so a baseline or waiver written for one still matches.
    pub deprecated_ids: Vec<RuleId>,
    /// Where it applies.
    pub scope: Scope,
}

/// One result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// The rule.
    pub rule: RuleId,
    /// How serious.
    pub severity: Severity,
    /// Where; `None` for a finding about the repository as a whole.
    pub location: Option<Location>,
    /// What is wrong.
    pub message: String,
    /// What to do, one sentence (required).
    pub fix: FixSentence,
    /// Where `fix` came from.
    pub fix_source: FixSource,
    /// Why it matters.
    pub explanation: Option<String>,
    /// Excerpts that show it.
    pub evidence: Vec<String>,
    /// A suggested change as a unified diff; never applied.
    pub fix_diff: Option<String>,
    /// Stable identity for baselines.
    pub fingerprint: Option<FindingFingerprint>,
    /// How sure the finder is.
    pub confidence: Option<Confidence>,
}
