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
/// The longest fix sentence, in bytes (measured against conformance's longest, 346).
pub const MAX_FIX_BYTES: usize = 512;
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

/// What a rule checks. Externally tagged, so it crosses postcard: `check = { pattern = "..." }` in TOML.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    /// A structural, ast-grep-style pattern (compiled by `pacenotes-rules`). Deterministic; needs no model.
    Pattern(String),
    /// A judgement rule in prose, checked by the local model through Pitwall.
    Markdown(String),
}

/// The wire form of a [`RuleSpec`]. Reading a `RuleSpec` from any serde format reads this and runs the same validation as
/// [`RuleSpec::from_toml`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuleWire {
    pub(crate) id: RuleId,
    pub(crate) title: String,
    pub(crate) severity: Severity,
    #[serde(default)]
    pub(crate) include: Vec<String>,
    #[serde(default)]
    pub(crate) exclude: Vec<String>,
    #[serde(default)]
    pub(crate) languages: Vec<Language>,
    pub(crate) check: Check,
    #[serde(default)]
    pub(crate) fix: String,
    #[serde(default)]
    pub(crate) good: Vec<String>,
    #[serde(default)]
    pub(crate) bad: Vec<String>,
}

impl TryFrom<RuleWire> for RuleSpec {
    type Error = RuleError;
    fn try_from(w: RuleWire) -> Result<RuleSpec, RuleError> {
        RuleSpec::from_wire(w)
    }
}

/// A review rule. Fields are private: a rule exists only if it passed [`RuleSpec::from_toml`] or the same validation on the
/// wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RuleWire")]
pub struct RuleSpec {
    id: RuleId,
    title: String,
    severity: Severity,
    include: Vec<String>,
    exclude: Vec<String>,
    languages: Vec<Language>,
    check: Check,
    fix: String,
    good: Vec<String>,
    bad: Vec<String>,
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
        let wire: RuleWire = toml::from_str(text).map_err(|e| RuleError::Syntax(e.message().to_owned()))?;
        RuleSpec::from_wire(wire)
    }

    /// The one validation: shape and bounds.
    pub(crate) fn from_wire(w: RuleWire) -> Result<RuleSpec, RuleError> {
        let err = |field: &'static str, problem: String, fix: &'static str| RuleError::Invalid {
            rule: w.id.as_str().to_owned(),
            field,
            problem,
            fix,
        };
        if w.title.trim().is_empty() || w.title.len() > 200 {
            return Err(err(
                "title",
                "it is empty or longer than 200 characters".into(),
                "Give the rule a short title.",
            ));
        }
        for (field, globs) in [("include", &w.include), ("exclude", &w.exclude)] {
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
        if w.languages.len() > MAX_GLOBS {
            return Err(err(
                "languages",
                "too many languages".into(),
                "List the languages the rule applies to, or none for all.",
            ));
        }
        let fix = w.fix.trim();
        if fix.is_empty() {
            return Err(err(
                "fix",
                "it is missing or blank".into(),
                "Add `fix = \"...\"`: one sentence that says what to do about a finding.",
            ));
        }
        if fix.len() > MAX_FIX_BYTES || fix.chars().any(char::is_control) {
            return Err(err(
                "fix",
                format!("it is longer than {MAX_FIX_BYTES} bytes or has a line break"),
                "Write the fix as one short sentence on one line.",
            ));
        }
        for (field, list) in [("good", &w.good), ("bad", &w.bad)] {
            if list.len() > MAX_EXAMPLES || list.iter().any(|e| e.len() > MAX_TEXT_BYTES || e.contains('\0')) {
                return Err(err(
                    field,
                    format!("more than {MAX_EXAMPLES} examples, or one is too long"),
                    "Keep a few short examples.",
                ));
            }
        }
        match &w.check {
            Check::Pattern(pattern) => {
                if pattern.trim().is_empty() || pattern.len() > MAX_TEXT_BYTES {
                    return Err(err(
                        "check.pattern",
                        "it is empty or too long".into(),
                        "Give the pattern, at most 16 KiB.",
                    ));
                }
                if w.good.is_empty() || w.bad.is_empty() {
                    return Err(err(
                        "good/bad",
                        "a structural rule needs at least one good and one bad example".into(),
                        "Add examples: they are the rule's tests (`pacenotes rules test`).",
                    ));
                }
            }
            Check::Markdown(text) => {
                if text.trim().is_empty() || text.len() > MAX_TEXT_BYTES {
                    return Err(err(
                        "check.text",
                        "it is empty or too long".into(),
                        "Write the rule in a sentence or two, at most 16 KiB.",
                    ));
                }
            }
        }
        Ok(RuleSpec {
            id: w.id,
            title: w.title,
            severity: w.severity,
            include: w.include,
            exclude: w.exclude,
            languages: w.languages,
            check: w.check,
            fix: fix.to_owned(),
            good: w.good,
            bad: w.bad,
        })
    }

    /// The rule id.
    pub fn id(&self) -> &RuleId {
        &self.id
    }

    /// A short title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// How serious a violation is.
    pub fn severity(&self) -> Severity {
        self.severity
    }

    /// Files the rule applies to (empty: all).
    pub fn include(&self) -> &[String] {
        &self.include
    }

    /// Files it does not apply to.
    pub fn exclude(&self) -> &[String] {
        &self.exclude
    }

    /// Languages it applies to (empty: all).
    pub fn languages(&self) -> &[Language] {
        &self.languages
    }

    /// The check.
    pub fn check(&self) -> &Check {
        &self.check
    }

    /// What to do about a violation, one sentence (required).
    pub fn fix(&self) -> &str {
        &self.fix
    }

    /// Snippets the rule must not flag; with `bad` they are the rule's tests.
    pub fn good(&self) -> &[String] {
        &self.good
    }

    /// Snippets the rule must flag.
    pub fn bad(&self) -> &[String] {
        &self.bad
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
pattern = "$X.unwrap()"
"#;

    fn msg(text: &str) -> String {
        RuleSpec::from_toml(text).unwrap_err().to_string()
    }

    #[test]
    fn a_good_structural_rule_parses() {
        let r = RuleSpec::from_toml(GOOD).unwrap();
        assert_eq!((r.id().as_str(), r.severity()), ("no-unwrap-on-io", Severity::High));
        assert!(matches!(r.check(), Check::Pattern(_)));
    }

    #[test]
    fn a_structural_rule_without_examples_is_refused() {
        let m = msg(&GOOD.replace("bad = [\"let f = File::open(p).unwrap();\"]\n", ""));
        assert!(m.contains("good and one bad example") && m.contains("no-unwrap-on-io"), "{m}");
    }

    #[test]
    fn a_markdown_rule_needs_text_not_examples() {
        let md = "id = \"validate-body\"\ntitle = \"Validate the body\"\nseverity = \"medium\"\nfix = \"Validate it.\"\n[check]\nmarkdown = \"Handlers validate the request body before using it.\"\n";
        assert!(RuleSpec::from_toml(md).is_ok());
        assert!(msg(&md.replace("Handlers validate the request body before using it.", "  ")).contains("check.text"));
    }

    /// D-rules decision 2 (2026-10-30): every rule says in one sentence what to do.
    #[test]
    fn a_rule_without_a_usable_fix_sentence_is_refused_with_a_message_that_says_to_add_one() {
        let without = GOOD.replace("fix = \"Return the error with `?`.\"\n", "");
        let msg = |t: &str| RuleSpec::from_toml(t).unwrap_err().to_string();
        assert!(
            msg(&without).contains("field `fix`") && msg(&without).contains("Add"),
            "{}",
            msg(&without)
        );
        for bad in ["fix = \"   \"", "fix = \"two\\nlines\"", &format!("fix = \"{}\"", "w".repeat(513))] {
            let text = GOOD.replace("fix = \"Return the error with `?`.\"", bad);
            assert!(msg(&text).contains("field `fix`"), "{bad}: {}", msg(&text));
        }
        assert!(RuleSpec::from_toml(&GOOD.replace("Return the error with `?`.", &"w".repeat(512))).is_ok());
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

    #[test]
    fn every_getter_returns_what_was_parsed() {
        let r = RuleSpec::from_toml(GOOD).unwrap();
        assert_eq!(r.title(), "No unwrap on I/O");
        assert_eq!(r.include(), ["src/**/*.rs"]);
        assert_eq!(r.exclude(), ["tests/**"]);
        assert_eq!(r.languages().len(), 1);
        assert_eq!(r.fix(), "Return the error with `?`.");
        assert_eq!((r.good().len(), r.bad().len()), (1, 1));
        assert_eq!(r.check(), &Check::Pattern("$X.unwrap()".into()));
    }
}
