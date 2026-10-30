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

/// Who to ask about a waiver: a name or a CODEOWNERS entry. Never blank.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Owner(String);

impl Owner {
    /// Parses an owner; a blank one is refused.
    pub fn parse(text: &str) -> Result<Owner, super::TextError> {
        let t = text.trim();
        if t.is_empty() {
            return Err(super::TextError::Empty { what: "the owner" });
        }
        if t.len() > 200 || t.chars().any(char::is_control) {
            return Err(super::TextError::NotOneLine { what: "the owner" });
        }
        Ok(Owner(t.to_owned()))
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An entry that hides the findings of a rule over a path glob. Built only by [`Waiver::file`] and [`Waiver::config`], which enforce
/// what each origin must carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiver {
    pub(crate) rule: RuleId,
    pub(crate) path: String,
    pub(crate) reason: Reason,
    pub(crate) expires: Option<IsoDate>,
    pub(crate) max_count: Option<u32>,
    pub(crate) owner: Option<Owner>,
    pub(crate) origin: Origin,
}

impl Waiver {
    /// A waiver written in a waiver file: it must expire (the last day it holds, inclusive, UTC) and its glob must be within bounds.
    pub fn file(
        rule: RuleId,
        path: &str,
        reason: Reason,
        expires: IsoDate,
        max_count: Option<u32>,
        owner: Option<Owner>,
    ) -> Result<Waiver, String> {
        super::validate_glob(path)?;
        Ok(Waiver {
            rule,
            path: path.to_owned(),
            reason,
            expires: Some(expires),
            max_count,
            owner,
            origin: Origin::File,
        })
    }

    /// A waiver built from a check's own exemption list: no expiry, but a reason and an owner are required (the owner may be the
    /// CODEOWNERS owner of the config file).
    pub fn config(rule: RuleId, path: &str, reason: Reason, owner: Owner, max_count: Option<u32>) -> Result<Waiver, String> {
        super::validate_glob(path)?;
        Ok(Waiver {
            rule,
            path: path.to_owned(),
            reason,
            expires: None,
            max_count,
            owner: Some(owner),
            origin: Origin::Config,
        })
    }

    /// The rule.
    pub fn rule(&self) -> &RuleId {
        &self.rule
    }

    /// The path glob.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Why.
    pub fn reason(&self) -> &Reason {
        &self.reason
    }

    /// The last day it holds.
    pub fn expires(&self) -> Option<IsoDate> {
        self.expires
    }

    /// The most findings it may hide.
    pub fn max_count(&self) -> Option<u32> {
        self.max_count
    }

    /// Who to ask.
    pub fn owner(&self) -> Option<&Owner> {
        self.owner.as_ref()
    }

    /// Where it was written.
    pub fn origin(&self) -> Origin {
        self.origin
    }
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

impl std::fmt::Display for SuppressionScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SuppressionScope::Line { path, line } => write!(f, "line {line} of {path}"),
            SuppressionScope::File { path } => write!(f, "the file {path}"),
            SuppressionScope::Path { glob } => write!(f, "the paths {glob}"),
            SuppressionScope::Everywhere => f.write_str("everywhere"),
        }
    }
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
    /// True when it is written in the source (an annotation comment), false when it comes from a file.
    pub in_source: bool,
}

/// Why a waiver file, or one entry of it, was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WaiverError {
    /// Not TOML, too big, or the top level is wrong.
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
struct RawWaiver {
    rule: String,
    path: String,
    reason: String,
    expires: Option<String>,
    /// ADR 0021's key; the model field is `max_count`.
    count: Option<u32>,
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

fn waiver_of(w: &RawWaiver) -> Result<Waiver, String> {
    let expires = date(w.expires.as_deref())?.ok_or_else(|| "a waiver in a file needs `expires = \"YYYY-MM-DD\"`".to_owned())?;
    let owner = w.owner.as_deref().map(Owner::parse).transpose().map_err(|e| e.to_string())?;
    Waiver::file(
        rule_id(&w.rule)?,
        &w.path,
        Reason::parse(&w.reason).map_err(|e| e.to_string())?,
        expires,
        w.count,
        owner,
    )
}

fn suppression_of(s: &RawSuppression) -> Result<Suppression, String> {
    let need = |what: &str, v: Option<&String>| v.cloned().ok_or_else(|| format!("scope {:?} needs `{what}`", s.scope));
    let scope = match s.scope.as_str() {
        "line" => SuppressionScope::Line {
            path: need("path", s.path.as_ref())?,
            line: s
                .line
                .filter(|l| *l > 0)
                .ok_or_else(|| "scope \"line\" needs `line` (1 or more)".to_owned())?,
        },
        "file" => SuppressionScope::File {
            path: need("path", s.path.as_ref())?,
        },
        "path" => {
            let glob = need("path", s.path.as_ref())?;
            super::validate_glob(&glob)?;
            SuppressionScope::Path { glob }
        }
        "rule" => SuppressionScope::Everywhere,
        other => return Err(format!("scope {other:?} is not line, file, path or rule")),
    };
    let rule = s.rule.as_deref().map(rule_id).transpose()?;
    if scope == SuppressionScope::Everywhere && rule.is_none() {
        return Err("scope \"rule\" needs `rule`".to_owned());
    }
    Ok(Suppression {
        scope,
        rule,
        reason: Reason::parse(&s.reason).map_err(|e| e.to_string())?,
        expires: date(s.expires.as_deref())?,
        in_source: false,
    })
}

impl SuppressionFile {
    /// Reads a file leniently: every good entry is kept and every bad one comes back as an error naming it, so one typo does not switch
    /// off the rest of the file. A file that cannot be read at all (not TOML, too big, wrong version) has no entries and one error.
    pub fn parse_lenient(text: &str) -> (SuppressionFile, Vec<WaiverError>) {
        let mut out = SuppressionFile::default();
        let whole = |e: WaiverError| (SuppressionFile::default(), vec![e]);
        if text.len() > MAX_FILE_BYTES {
            return whole(WaiverError::Syntax(format!(
                "the file is {} bytes; at most {MAX_FILE_BYTES} are read",
                text.len()
            )));
        }
        let table: toml::Table = match toml::from_str(text) {
            Ok(t) => t,
            Err(e) => return whole(WaiverError::Syntax(e.message().to_owned())),
        };
        match table.get("version").and_then(toml::Value::as_integer) {
            Some(1) => {}
            Some(other) => return whole(WaiverError::Version(u32::try_from(other).unwrap_or(u32::MAX))),
            None => return whole(WaiverError::Syntax("`version = 1` is missing at the top".to_owned())),
        }
        let mut errors = Vec::new();
        for key in table.keys().filter(|k| !matches!(k.as_str(), "version" | "waiver" | "suppression")) {
            errors.push(WaiverError::Syntax(format!("unknown key {key:?} at the top")));
        }
        let list = |name: &str| table.get(name).and_then(toml::Value::as_array).cloned().unwrap_or_default();
        let (waivers, suppressions) = (list("waiver"), list("suppression"));
        if waivers.len() + suppressions.len() > MAX_ENTRIES {
            return whole(WaiverError::Syntax(format!("more than {MAX_ENTRIES} entries")));
        }
        for (i, v) in waivers.into_iter().enumerate() {
            let bad = |problem: String| WaiverError::Entry {
                table: "waiver",
                number: i + 1,
                problem,
            };
            match v
                .try_into::<RawWaiver>()
                .map_err(|e| e.message().to_owned())
                .and_then(|r| waiver_of(&r))
            {
                Ok(w) => out.waivers.push(w),
                Err(problem) => errors.push(bad(problem)),
            }
        }
        for (i, v) in suppressions.into_iter().enumerate() {
            let bad = |problem: String| WaiverError::Entry {
                table: "suppression",
                number: i + 1,
                problem,
            };
            match v
                .try_into::<RawSuppression>()
                .map_err(|e| e.message().to_owned())
                .and_then(|r| suppression_of(&r))
            {
                Ok(s) => out.suppressions.push(s),
                Err(problem) => errors.push(bad(problem)),
            }
        }
        (out, errors)
    }

    /// Reads a file strictly: the first problem is the error.
    pub fn parse(text: &str) -> Result<SuppressionFile, WaiverError> {
        match SuppressionFile::parse_lenient(text) {
            (file, errors) if errors.is_empty() => Ok(file),
            (_, mut errors) => Err(errors.remove(0)),
        }
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
count = 3
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
        assert_eq!(f.waivers[0].max_count(), Some(3));
        assert_eq!(f.waivers[0].origin(), Origin::File);
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

    #[test]
    fn one_bad_entry_does_not_switch_off_the_rest_of_the_file() {
        let text = format!(
            "{GOOD}\n[[waiver]]\nrule = \"x\"\npath = \"a\"\nreason = \"r\"\n\n[[suppression]]\nscope = \"galaxy\"\nreason = \"r\"\n"
        );
        let (file, errors) = SuppressionFile::parse_lenient(&text);
        assert_eq!((file.waivers.len(), file.suppressions.len(), errors.len()), (1, 2, 2));
        assert!(errors[0].to_string().contains("waiver number 2") && errors[1].to_string().contains("suppression number 3"));
        let (empty, errors) = SuppressionFile::parse_lenient("not toml [");
        assert_eq!((empty, errors.len()), (SuppressionFile::default(), 1));
        assert!(
            SuppressionFile::parse_lenient("version = 1\nsurprise = 2\n").1[0]
                .to_string()
                .contains("unknown key")
        );
    }

    #[test]
    fn globs_in_a_file_are_bounded_and_constructors_enforce_what_each_origin_needs() {
        let long = format!(
            "version = 1\n[[waiver]]\nrule=\"a\"\npath=\"{}\"\nreason=\"r\"\nexpires=\"2026-01-01\"\n",
            "*a".repeat(17)
        );
        assert!(SuppressionFile::parse(&long).unwrap_err().to_string().contains("17 `*`"));
        let reason = Reason::parse("standing decision").unwrap();
        let rule = RuleId::local("a").unwrap();
        let c = Waiver::config(rule.clone(), "src/**", reason.clone(), Owner::parse("@decryp7").unwrap(), None).unwrap();
        assert_eq!(
            (c.origin(), c.expires(), c.owner().map(Owner::as_str)),
            (Origin::Config, None, Some("@decryp7"))
        );
        assert!(Owner::parse("  ").is_err() && Owner::parse("a\nb").is_err());
        assert!(Waiver::config(rule.clone(), &"*".repeat(300), reason.clone(), Owner::parse("o").unwrap(), None).is_err());
        let f = Waiver::file(rule, "src/**", reason, IsoDate::parse("2026-12-31").unwrap(), Some(2), None).unwrap();
        assert_eq!(
            (f.origin(), f.max_count(), f.path(), f.rule().to_string(), f.reason().as_str()),
            (Origin::File, Some(2), "src/**", "a".to_owned(), "standing decision")
        );
        assert_eq!(
            SuppressionScope::Line {
                path: "a.rs".into(),
                line: 3
            }
            .to_string(),
            "line 3 of a.rs"
        );
        assert_eq!(SuppressionScope::Everywhere.to_string(), "everywhere");
    }
}
