//! [`RuleSpec`]: the shape of a review rule (proposal 24.2), validated at the edge.
//!
//! The core checks shape and bounds. A structural pattern is compiled by `pacenotes-rules`; a Markdown rule's text is
//! untrusted data that is never executed.

use serde::{Deserialize, Serialize};

use crate::language::Language;
use crate::tier::Severity;

/// The most examples a rule may carry, per side.
pub const MAX_EXAMPLES: usize = 32;
/// The most globs per list.
pub const MAX_GLOBS: usize = 64;
/// The longest example, pattern or Markdown text, in bytes.
pub const MAX_TEXT_BYTES: usize = 16 * 1024;

/// Why a rule is refused. `rule` names it (or `?` when its id is the problem).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleError {
    /// The text is not valid TOML for a rule.
    #[error("the rule file is not valid: {0}")]
    Syntax(String),
    /// A field is invalid.
    #[error("rule `{rule}`, field `{field}`: {problem}. {fix}")]
    Invalid {
        /// The rule id.
        rule: String,
        /// The field.
        field: &'static str,
        /// What is wrong.
        problem: String,
        /// What to do.
        fix: &'static str,
    },
}

/// A validated rule id: `[a-z0-9][a-z0-9._/-]{0,95}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RuleId(String);

impl RuleId {
    /// Parses an id.
    pub fn parse(text: &str) -> Result<RuleId, RuleError> {
        let mut bytes = text.bytes();
        let ok = text.len() <= 96
            && bytes.next().is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'/' | b'-'));
        if ok {
            Ok(RuleId(text.to_owned()))
        } else {
            Err(RuleError::Invalid {
                rule: "?".to_owned(),
                field: "id",
                problem: format!("{:?} is not a rule id", text.chars().take(40).collect::<String>()),
                fix: "Use lowercase letters, digits and `._/-`, starting with a letter or digit, at most 96 characters.",
            })
        }
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RuleId {
    type Error = RuleError;
    fn try_from(value: String) -> Result<Self, RuleError> {
        RuleId::parse(&value)
    }
}

impl From<RuleId> for String {
    fn from(id: RuleId) -> String {
        id.0
    }
}

/// What a rule checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Check {
    /// A structural, ast-grep-style pattern. Deterministic; needs no model.
    Pattern {
        /// The pattern text (compiled by `pacenotes-rules`).
        pattern: String,
    },
    /// A judgement rule in prose, checked by the local model through Pitwall.
    Markdown {
        /// The rule text.
        text: String,
    },
}

/// A review rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSpec {
    /// The rule id.
    pub id: RuleId,
    /// A short title.
    pub title: String,
    /// How serious a violation is.
    pub severity: Severity,
    /// Files the rule applies to (empty: all).
    #[serde(default)]
    pub include: Vec<String>,
    /// Files it does not apply to.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Languages it applies to (empty: all).
    #[serde(default)]
    pub languages: Vec<Language>,
    /// The check.
    pub check: Check,
    /// A hint on how to fix a violation.
    pub fix: Option<String>,
    /// Snippets the rule must not flag; with `bad` they are the rule's tests.
    #[serde(default)]
    pub good: Vec<String>,
    /// Snippets the rule must flag.
    #[serde(default)]
    pub bad: Vec<String>,
}

impl RuleSpec {
    /// Parses the TOML text of one rule and validates it.
    pub fn from_toml(text: &str) -> Result<RuleSpec, RuleError> {
        if text.len() > 4 * MAX_TEXT_BYTES {
            return Err(RuleError::Syntax(format!(
                "the rule file is {} bytes; at most {} are accepted",
                text.len(),
                4 * MAX_TEXT_BYTES
            )));
        }
        let rule: RuleSpec = toml::from_str(text).map_err(|e| RuleError::Syntax(e.message().to_owned()))?;
        rule.validate()?;
        Ok(rule)
    }

    /// Checks shape and bounds.
    pub fn validate(&self) -> Result<(), RuleError> {
        let err = |field: &'static str, problem: String, fix: &'static str| RuleError::Invalid {
            rule: self.id.as_str().to_owned(),
            field,
            problem,
            fix,
        };
        if self.title.trim().is_empty() || self.title.len() > 200 {
            return Err(err(
                "title",
                "it is empty or longer than 200 characters".into(),
                "Give the rule a short title.",
            ));
        }
        for (field, globs) in [("include", &self.include), ("exclude", &self.exclude)] {
            if globs.len() > MAX_GLOBS {
                return Err(err(field, format!("more than {MAX_GLOBS} globs"), "Merge globs."));
            }
            if let Some(g) = globs
                .iter()
                .find(|g| g.is_empty() || g.len() > 256 || g.contains('\0') || g.starts_with('/') || g.split('/').any(|s| s == ".."))
            {
                return Err(err(
                    field,
                    format!("{g:?} is not a workspace-relative glob"),
                    "Use relative globs without `..`, for example \"src/**/*.ts\".",
                ));
            }
        }
        if self.languages.len() > MAX_GLOBS {
            return Err(err(
                "languages",
                "too many languages".into(),
                "List the languages the rule applies to, or none for all.",
            ));
        }
        if self.fix.as_deref().is_some_and(|f| f.len() > MAX_TEXT_BYTES) {
            return Err(err("fix", "it is too long".into(), "Keep the fix hint short."));
        }
        for (field, list) in [("good", &self.good), ("bad", &self.bad)] {
            if list.len() > MAX_EXAMPLES || list.iter().any(|e| e.len() > MAX_TEXT_BYTES || e.contains('\0')) {
                return Err(err(
                    field,
                    format!("more than {MAX_EXAMPLES} examples, or one is too long"),
                    "Keep a few short examples.",
                ));
            }
        }
        match &self.check {
            Check::Pattern { pattern } => {
                if pattern.trim().is_empty() || pattern.len() > MAX_TEXT_BYTES {
                    return Err(err(
                        "check.pattern",
                        "it is empty or too long".into(),
                        "Give the pattern, at most 16 KiB.",
                    ));
                }
                if self.good.is_empty() || self.bad.is_empty() {
                    return Err(err(
                        "good/bad",
                        "a structural rule needs at least one good and one bad example".into(),
                        "Add examples: they are the rule's tests (`pacenotes rules test`).",
                    ));
                }
            }
            Check::Markdown { text } => {
                if text.trim().is_empty() || text.len() > MAX_TEXT_BYTES {
                    return Err(err(
                        "check.text",
                        "it is empty or too long".into(),
                        "Write the rule in a sentence or two, at most 16 KiB.",
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
id = "no-unwrap-on-io"
title = "No unwrap on I/O"
severity = "high"
include = ["src/**/*.rs"]
exclude = ["tests/**"]
languages = ["rust"]
fix = "Return the error with `?`."
good = ["let f = File::open(p)?;"]
bad = ["let f = File::open(p).unwrap();"]

[check]
kind = "pattern"
pattern = "$X.unwrap()"
"#;

    fn msg(text: &str) -> String {
        RuleSpec::from_toml(text).unwrap_err().to_string()
    }

    #[test]
    fn a_good_structural_rule_parses() {
        let r = RuleSpec::from_toml(GOOD).unwrap();
        assert_eq!((r.id.as_str(), r.severity), ("no-unwrap-on-io", Severity::High));
        assert!(matches!(r.check, Check::Pattern { .. }));
    }

    #[test]
    fn a_structural_rule_without_examples_is_refused() {
        let m = msg(&GOOD.replace("bad = [\"let f = File::open(p).unwrap();\"]\n", ""));
        assert!(m.contains("good and one bad example") && m.contains("no-unwrap-on-io"), "{m}");
    }

    #[test]
    fn a_markdown_rule_needs_text_not_examples() {
        let md = "id = \"validate-body\"\ntitle = \"Validate the body\"\nseverity = \"medium\"\n[check]\nkind = \"markdown\"\ntext = \"Handlers validate the request body before using it.\"\n";
        assert!(RuleSpec::from_toml(md).is_ok());
        assert!(msg(&md.replace("Handlers validate the request body before using it.", "  ")).contains("check.text"));
    }

    #[test]
    fn bad_fields_say_what_to_fix() {
        assert!(msg(&GOOD.replace("id = \"no-unwrap-on-io\"", "id = \"No Unwrap\"")).contains("not a rule id"));
        assert!(msg(&GOOD.replace("\"high\"", "\"critical\"")).contains("unknown variant"));
        assert!(msg(&GOOD.replace("title = \"No unwrap on I/O\"", "title = \" \"")).contains("title"));
        assert!(msg(&GOOD.replace("\"src/**/*.rs\"", "\"../x\"")).contains("workspace-relative"));
        assert!(msg(&GOOD.replace("severity", "colour = 1\nseverity")).contains("unknown field"));
        assert!(msg(&GOOD.replace("pattern = \"$X.unwrap()\"", "pattern = \"\"")).contains("check.pattern"));
    }

    #[test]
    fn examples_are_bounded() {
        let many = format!("[{}]", vec!["\"x\""; MAX_EXAMPLES + 1].join(","));
        assert!(msg(&GOOD.replace("good = [\"let f = File::open(p)?;\"]", &format!("good = {many}"))).contains("examples"));
    }

    #[test]
    fn rule_ids_are_slugs() {
        assert!(RuleId::parse("a.b/c-d_1").is_ok());
        for bad in ["", "-a", "A", "a b", &"a".repeat(97)] {
            assert!(RuleId::parse(bad).is_err(), "{bad:?}");
        }
        assert!(serde_json::from_str::<RuleId>("\"Bad Id\"").is_err());
    }

    #[test]
    fn parsing_never_panics_on_hostile_input() {
        paddock_testkit::assert_never_panics(&[GOOD.as_bytes()], |b| RuleSpec::from_toml(&String::from_utf8_lossy(b)).is_ok());
    }

    #[test]
    fn size_and_list_bounds_are_enforced() {
        assert!(msg(&" ".repeat(4 * MAX_TEXT_BYTES + 1)).contains("bytes"));
        let many = format!("[{}]", vec!["\"a\""; MAX_GLOBS + 1].join(","));
        assert!(msg(&GOOD.replace("include = [\"src/**/*.rs\"]", &format!("include = {many}"))).contains("more than 64 globs"));
        assert!(msg(&GOOD.replace("languages = [\"rust\"]", &format!("languages = {many}"))).contains("too many languages"));
        let long = "x".repeat(MAX_TEXT_BYTES + 1);
        assert!(msg(&GOOD.replace("fix = \"Return the error with `?`.\"", &format!("fix = \"{long}\""))).contains("fix"));
    }

    #[test]
    fn rule_ids_convert_to_and_from_strings() {
        let id = RuleId::try_from("a-b".to_owned()).unwrap();
        assert_eq!(String::from(id), "a-b");
    }
}
