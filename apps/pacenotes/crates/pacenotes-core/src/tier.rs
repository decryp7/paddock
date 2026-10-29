//! [`Tier`], [`Confidence`] and [`Severity`].

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Where a fact comes from (proposal 20.1). Ordered: a higher tier knows more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Files, text search, git: every language.
    Text,
    /// tree-sitter: symbols, outline, imports, approximate calls.
    Syntax,
    /// A language server or SCIP index: exact references and types.
    Semantic,
}

impl Tier {
    /// The tier number users see: 1, 2 or 3.
    pub fn number(self) -> u8 {
        match self {
            Tier::Text => 1,
            Tier::Syntax => 2,
            Tier::Semantic => 3,
        }
    }

    /// The tier for a number 1 to 3.
    pub fn from_number(n: u8) -> Option<Tier> {
        match n {
            1 => Some(Tier::Text),
            2 => Some(Tier::Syntax),
            3 => Some(Tier::Semantic),
            _ => None,
        }
    }
}

/// How sure a reference, call edge or link is. Ordered from the weakest to the strongest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// A name looks alike; several targets are possible.
    Guess,
    /// A heuristic picked the target (same name and an import in scope).
    Heuristic,
    /// The target is unique after scope and import resolution.
    Resolved,
    /// A language server or a SCIP index said so.
    Exact,
}

impl Confidence {
    /// True when a user may rely on it without a "check this" label.
    pub fn is_reliable(self) -> bool {
        self >= Confidence::Resolved
    }
}

/// Review severity (proposal section 8). Ordered from the least to the most severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// For information only.
    Info,
    /// Minor.
    Low,
    /// Should be fixed.
    Medium,
    /// Should be fixed before merging.
    High,
    /// Will break, or is a security or data-loss defect.
    Blocker,
}

impl Severity {
    /// The name used in rule files and reports.
    pub fn name(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Low => "low",
            Severity::Medium => "medium",
            Severity::High => "high",
            Severity::Blocker => "blocker",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A severity name that is not one of the five.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown severity {0:?} (use info, low, medium, high or blocker)")]
pub struct SeverityError(pub String);

impl FromStr for Severity {
    type Err = SeverityError;
    fn from_str(s: &str) -> Result<Self, SeverityError> {
        match s {
            "info" => Ok(Severity::Info),
            "low" => Ok(Severity::Low),
            "medium" => Ok(Severity::Medium),
            "high" => Ok(Severity::High),
            "blocker" => Ok(Severity::Blocker),
            other => Err(SeverityError(other.chars().take(40).collect())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_are_ordered_and_numbered() {
        assert!(Tier::Text < Tier::Syntax && Tier::Syntax < Tier::Semantic);
        for t in [Tier::Text, Tier::Syntax, Tier::Semantic] {
            assert_eq!(Tier::from_number(t.number()), Some(t));
        }
        assert_eq!(Tier::from_number(0), None);
        assert_eq!(Tier::from_number(4), None);
    }

    #[test]
    fn confidence_is_ordered_and_only_resolved_up_is_reliable() {
        assert!(Confidence::Guess < Confidence::Heuristic && Confidence::Resolved < Confidence::Exact);
        assert!(!Confidence::Heuristic.is_reliable() && Confidence::Resolved.is_reliable() && Confidence::Exact.is_reliable());
    }

    #[test]
    fn severity_names_round_trip() {
        for s in [Severity::Info, Severity::Low, Severity::Medium, Severity::High, Severity::Blocker] {
            assert_eq!(s.name().parse::<Severity>().unwrap(), s);
            assert_eq!(serde_json::to_string(&s).unwrap(), format!("\"{}\"", s.name()));
        }
        assert!("critical".parse::<Severity>().unwrap_err().to_string().contains("use info"));
        assert!(Severity::High < Severity::Blocker);
    }

    #[test]
    fn severity_displays_its_name() {
        assert_eq!(Severity::Blocker.to_string(), "blocker");
    }
}
