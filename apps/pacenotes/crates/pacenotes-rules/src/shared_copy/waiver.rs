//! Waivers (a file entry hiding a rule over a path) and suppressions (the in-source or per-finding form), and the file that holds them.

use serde::Deserialize;

use super::{IsoDate, Reason, RuleId};

/// Where a waiver comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Origin {
    /// A waiver file; it must expire.
    File,
    /// A check's own exemption list (`untimedCommands`, `a11y.skip`, `ownTestBinaries`): no expiry, but a reason and an owner.
    Config,
}

/// An entry that hides the findings of a rule over a path glob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiver {
    /// The rule (or one of its deprecated ids).
    pub rule: RuleId,
    /// A path glob (`path_matches`).
    pub path: String,
    /// Why.
    pub reason: Reason,
    /// The last day it holds (inclusive, UTC). Required for [`Origin::File`].
    pub expires: Option<IsoDate>,
    /// The most findings it may hide; above that it hides nothing (ADR 0021).
    pub max_count: Option<u32>,
    /// Who to ask; absent, the owner of the file in CODEOWNERS.
    pub owner: Option<String>,
    /// Where it was written.
    pub origin: Origin,
}

/// What a suppression covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuppressionScope {
    /// One line of one file.
    Line {
        /// The file.
        path: String,
        /// The 1-based line.
        line: u32,
    },
    /// A whole file.
    File {
        /// The file.
        path: String,
    },
    /// A path glob.
    Path {
        /// The glob.
        glob: String,
    },
    /// Every place (needs a rule).
    Everywhere,
}

/// A suppression: a scope, an optional rule, a required reason and an optional expiry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suppression {
    /// What it covers.
    pub scope: SuppressionScope,
    /// Only this rule; required for [`SuppressionScope::Everywhere`].
    pub rule: Option<RuleId>,
    /// Why.
    pub reason: Reason,
    /// The last day it holds (inclusive, UTC).
    pub expires: Option<IsoDate>,
}

/// Why a waiver file was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WaiverError {
    /// Not TOML, too big, or an unknown key.
    #[error("the suppression file cannot be read: {0}")]
    Syntax(String),
    /// A wrong or missing `version`.
    #[error("the suppression file says version {0}; this program reads version 1")]
    Version(u32),
    /// An entry is incomplete or wrong.
    #[error("{table} number {number}: {problem}")]
    Entry {
        /// `waiver` or `suppression`.
        table: &'static str,
        /// 1-based position in the file.
        number: usize,
        /// What to fix.
        problem: String,
    },
}

/// The largest suppression file read.
pub const MAX_FILE_BYTES: usize = 256 * 1024;
/// The most entries in one file.
pub const MAX_ENTRIES: usize = 5_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    version: Option<u32>,
    #[serde(default)]
    waiver: Vec<RawWaiver>,
    #[serde(default)]
    suppression: Vec<RawSuppression>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWaiver {
    rule: String,
    path: String,
    reason: String,
    expires: Option<String>,
    max_count: Option<u32>,
    owner: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSuppression {
    scope: String,
    path: Option<String>,
    line: Option<u32>,
    rule: Option<String>,
    reason: String,
    expires: Option<String>,
}

/// The waivers and suppressions of one file (`paddock.waivers.toml` has only waivers; `.pacenotes/suppressions.toml` has both).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SuppressionFile {
    /// The `[[waiver]]` tables.
    pub waivers: Vec<Waiver>,
    /// The `[[suppression]]` tables.
    pub suppressions: Vec<Suppression>,
}

fn rule_id(text: &str) -> Result<RuleId, String> {
    match text.split_once(':') {
        Some((tool, id)) => RuleId::external(tool, id).map_err(|e| e.to_string()),
        None => RuleId::local(text).map_err(|e| e.to_string()),
    }
}

fn date(text: Option<&str>) -> Result<Option<IsoDate>, String> {
    text.map(|t| IsoDate::parse(t).map_err(|e| e.to_string())).transpose()
}

impl SuppressionFile {
    /// Reads and checks a file. Every refusal names the entry and what to fix; nothing panics on any input.
    pub fn parse(text: &str) -> Result<SuppressionFile, WaiverError> {
        if text.len() > MAX_FILE_BYTES {
            return Err(WaiverError::Syntax(format!(
                "the file is {} bytes; at most {MAX_FILE_BYTES} are read",
                text.len()
            )));
        }
        let raw: RawFile = toml::from_str(text).map_err(|e| WaiverError::Syntax(e.message().to_owned()))?;
        match raw.version {
            Some(1) => {}
            Some(other) => return Err(WaiverError::Version(other)),
            None => return Err(WaiverError::Syntax("`version = 1` is missing at the top".to_owned())),
        }
        if raw.waiver.len() + raw.suppression.len() > MAX_ENTRIES {
            return Err(WaiverError::Syntax(format!("more than {MAX_ENTRIES} entries")));
        }
        let mut out = SuppressionFile::default();
        for (i, w) in raw.waiver.iter().enumerate() {
            let bad = |problem: String| WaiverError::Entry {
                table: "waiver",
                number: i + 1,
                problem,
            };
            let expires = date(w.expires.as_deref()).map_err(bad)?;
            if expires.is_none() {
                return Err(bad("a waiver in a file needs `expires = \"YYYY-MM-DD\"`".to_owned()));
            }
            out.waivers.push(Waiver {
                rule: rule_id(&w.rule).map_err(bad)?,
                path: w.path.clone(),
                reason: Reason::parse(&w.reason).map_err(|e| bad(e.to_string()))?,
                expires,
                max_count: w.max_count,
                owner: w.owner.clone(),
                origin: Origin::File,
            });
        }
        for (i, s) in raw.suppression.iter().enumerate() {
            let bad = |problem: String| WaiverError::Entry {
                table: "suppression",
                number: i + 1,
                problem,
            };
            let need = |what: &str, v: Option<&String>| v.cloned().ok_or_else(|| bad(format!("scope {:?} needs `{what}`", s.scope)));
            let scope = match s.scope.as_str() {
                "line" => SuppressionScope::Line {
                    path: need("path", s.path.as_ref())?,
                    line: s
                        .line
                        .filter(|l| *l > 0)
                        .ok_or_else(|| bad("scope \"line\" needs `line` (1 or more)".to_owned()))?,
                },
                "file" => SuppressionScope::File {
                    path: need("path", s.path.as_ref())?,
                },
                "path" => SuppressionScope::Path {
                    glob: need("path", s.path.as_ref())?,
                },
                "rule" => SuppressionScope::Everywhere,
                other => return Err(bad(format!("scope {other:?} is not line, file, path or rule"))),
            };
            let rule = s.rule.as_deref().map(rule_id).transpose().map_err(bad)?;
            if scope == SuppressionScope::Everywhere && rule.is_none() {
                return Err(bad("scope \"rule\" needs `rule`".to_owned()));
            }
            out.suppressions.push(Suppression {
                scope,
                rule,
                reason: Reason::parse(&s.reason).map_err(|e| bad(e.to_string()))?,
                expires: date(s.expires.as_deref()).map_err(bad)?,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
version = 1
[[waiver]]
rule = "no-unwrap"
path = "src/legacy/**"
reason = "old code, scheduled for removal"
expires = "2026-12-31"
max_count = 3
owner = "decryp7"

[[suppression]]
scope = "line"
path = "src/a.rs"
line = 12
rule = "no-unwrap"
reason = "the value was checked on the line above"

[[suppression]]
scope = "rule"
rule = "clippy:needless_return"
reason = "generated code"
expires = "2027-01-01"
"#;

    #[test]
    fn a_good_file_is_read_with_both_table_kinds() {
        let f = SuppressionFile::parse(GOOD).unwrap();
        assert_eq!((f.waivers.len(), f.suppressions.len()), (1, 2));
        assert_eq!(f.waivers[0].max_count, Some(3));
        assert_eq!(f.waivers[0].origin, Origin::File);
        assert!(matches!(f.suppressions[0].scope, SuppressionScope::Line { line: 12, .. }));
        assert!(matches!(f.suppressions[1].rule, Some(RuleId::External(_))));
    }

    #[test]
    fn every_refusal_names_the_entry_and_what_to_fix() {
        let cases = [
            ("not toml [", "cannot be read"),
            (
                "[[waiver]]\nrule=\"a\"\npath=\"x\"\nreason=\"r\"\nexpires=\"2026-01-01\"\n",
                "`version = 1` is missing",
            ),
            ("version = 2\n", "version 2"),
            ("version = 1\n[[waiver]]\nrule=\"a\"\npath=\"x\"\nreason=\"r\"\n", "needs `expires"),
            (
                "version = 1\n[[waiver]]\nrule=\"a\"\npath=\"x\"\nreason=\"  \"\nexpires=\"2026-01-01\"\n",
                "reason is empty",
            ),
            (
                "version = 1\n[[waiver]]\nrule=\"A B\"\npath=\"x\"\nreason=\"r\"\nexpires=\"2026-01-01\"\n",
                "not a rule id",
            ),
            (
                "version = 1\n[[waiver]]\nrule=\"a\"\npath=\"x\"\nreason=\"r\"\nexpires=\"2026-02-31\"\n",
                "not a date",
            ),
            (
                "version = 1\n[[waiver]]\nrule=\"a\"\npath=\"x\"\nreason=\"r\"\nexpires=\"2026-01-01\"\nextra=1\n",
                "unknown field",
            ),
            (
                "version = 1\n[[suppression]]\nscope=\"line\"\npath=\"a\"\nreason=\"r\"\n",
                "needs `line`",
            ),
            ("version = 1\n[[suppression]]\nscope=\"file\"\nreason=\"r\"\n", "needs `path`"),
            ("version = 1\n[[suppression]]\nscope=\"rule\"\nreason=\"r\"\n", "needs `rule`"),
            (
                "version = 1\n[[suppression]]\nscope=\"galaxy\"\nreason=\"r\"\n",
                "is not line, file, path or rule",
            ),
        ];
        for (text, expect) in cases {
            let err = SuppressionFile::parse(text).unwrap_err().to_string();
            assert!(err.contains(expect), "{text:?} -> {err}");
        }
        assert!(SuppressionFile::parse(&"#".repeat(MAX_FILE_BYTES + 1)).is_err());
    }
}
