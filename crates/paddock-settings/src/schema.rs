//! The section schema: what a settings page is generated from. A [`SettingsSection`] lists [`SettingField`]s, each with a
//! [`FieldKind`] (bool, ranged int and float, text, enum, path, duration, size, string list) and a default.
//! [`SettingsSection::check`] validates the schema itself once, when the section is registered, so a defect in an app's
//! or the shell's section is refused with a message instead of producing a page that cannot be saved.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

/// Longest text value (and `Text::max_len`) accepted, in characters.
pub const MAX_TEXT_LEN: usize = 64 * 1024;
/// Most items a `StringList` may allow.
pub const MAX_LIST_ITEMS: usize = 1000;
/// Longest single item of a `StringList`, in characters.
pub const MAX_ITEM_LEN: usize = 4096;
/// Longest path accepted, in characters.
pub const MAX_PATH_LEN: usize = 4096;
/// Most fields one section may declare.
pub const MAX_FIELDS: usize = 200;
/// Most options one enum may declare.
pub const MAX_OPTIONS: usize = 200;
/// Longest section id or field key.
pub const MAX_ID_LEN: usize = 128;
/// Longest label, title or option label, in characters.
pub const MAX_LABEL_LEN: usize = 200;
/// Longest description, in characters.
pub const MAX_DESCRIPTION_LEN: usize = 2000;
/// Longest `Text::pattern`, in characters.
pub const MAX_PATTERN_LEN: usize = 1000;
/// Largest compiled `Text::pattern`, in bytes (the regex engine's size limit).
pub const MAX_PATTERN_BYTES: usize = 256 * 1024;

/// Who registers a section: the shell (appearance, updates, ...) or the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum SectionOwner {
    /// A section of the Paddock shell.
    Shell,
    /// A section of the app.
    App,
}

/// One choice of an [`FieldKind::Enum`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct EnumOption {
    /// The stored value.
    pub value: String,
    /// Text shown to the user.
    pub label: String,
}

impl EnumOption {
    /// An option with the stored `value` and its `label`.
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        EnumOption {
            value: value.into(),
            label: label.into(),
        }
    }
}

/// The type of a field and its limits. Durations are stored as milliseconds and sizes as bytes, so values are plain JSON
/// integers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub enum FieldKind {
    /// On or off.
    Bool,
    /// A whole number in `min..=max`, reachable from `min` in `step`s.
    Int {
        #[cfg_attr(test, ts(type = "number"))]
        min: i64,
        #[cfg_attr(test, ts(type = "number"))]
        max: i64,
        /// Granularity; at least 1.
        #[cfg_attr(test, ts(type = "number"))]
        step: i64,
        /// Unit label shown beside the value (`"lines"`).
        unit: Option<String>,
    },
    /// A finite number in `min..=max`; `step` is a hint for the control only.
    Float {
        min: f64,
        max: f64,
        /// Greater than 0.
        step: f64,
        unit: Option<String>,
    },
    /// Up to `max_len` characters, fully matching `pattern` when given.
    Text {
        /// 1 to [`MAX_TEXT_LEN`].
        #[cfg_attr(test, ts(type = "number"))]
        max_len: usize,
        /// A regular expression (Rust `regex` syntax) the whole text must match.
        pattern: Option<String>,
    },
    /// One of a fixed set of choices.
    Enum {
        /// At least one, with unique values.
        options: Vec<EnumOption>,
    },
    /// An absolute path, or empty for "not set"; `dir` asks for a folder.
    Path { dir: bool },
    /// A time span, stored as milliseconds.
    Duration {
        #[cfg_attr(test, ts(type = "number"))]
        min_ms: u64,
        #[cfg_attr(test, ts(type = "number"))]
        max_ms: u64,
    },
    /// A size, stored as bytes.
    Size {
        #[cfg_attr(test, ts(type = "number"))]
        min_bytes: u64,
        #[cfg_attr(test, ts(type = "number"))]
        max_bytes: u64,
    },
    /// A list of text items.
    StringList {
        /// 1 to [`MAX_LIST_ITEMS`].
        #[cfg_attr(test, ts(type = "number"))]
        max_items: usize,
    },
}

/// One setting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SettingField {
    /// Stable key within the section (`[A-Za-z0-9._-]`); the key in the settings file.
    pub key: String,
    /// Name shown to the user.
    pub label: String,
    /// Help text shown under the control.
    pub description: String,
    pub kind: FieldKind,
    /// The value until the user changes it; it passes the field's own validation.
    #[cfg_attr(test, ts(type = "SettingValue"))]
    pub default: Value,
    /// A change takes effect only after the app restarts.
    pub restart_required: bool,
    /// Hidden unless the user shows advanced settings.
    pub advanced: bool,
}

impl SettingField {
    /// A field with a `key`, `label`, kind and default; the rest take their defaults.
    pub fn new(
        key: impl Into<String>,
        label: impl Into<String>,
        kind: FieldKind,
        default: impl Into<Value>,
    ) -> Self {
        SettingField {
            key: key.into(),
            label: label.into(),
            description: String::new(),
            kind,
            default: default.into(),
            restart_required: false,
            advanced: false,
        }
    }

    /// Sets the help text.
    pub fn describe(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Marks the field as taking effect only after a restart.
    pub fn needs_restart(mut self) -> Self {
        self.restart_required = true;
        self
    }

    /// Hides the field unless the user shows advanced settings.
    pub fn as_advanced(mut self) -> Self {
        self.advanced = true;
        self
    }
}

/// A group of fields stored together: one key of the settings file and, in the UI, one settings page (its id is the
/// page's `defineSettingsPage` id).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SettingsSection {
    /// Stable id (`pd.appearance`, `pw.models`); the key in the settings file.
    pub id: String,
    /// Title shown for the section.
    pub title: String,
    /// Help text shown under the title.
    pub description: String,
    pub owner: SectionOwner,
    /// The fields, in display order.
    pub fields: Vec<SettingField>,
}

impl SettingsSection {
    /// An empty section.
    pub fn new(id: impl Into<String>, title: impl Into<String>, owner: SectionOwner) -> Self {
        SettingsSection {
            id: id.into(),
            title: title.into(),
            description: String::new(),
            owner,
            fields: Vec::new(),
        }
    }

    /// Sets the description.
    pub fn describe(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Appends a field.
    pub fn field(mut self, field: SettingField) -> Self {
        self.fields.push(field);
        self
    }

    /// The field with `key`, if any.
    pub fn find(&self, key: &str) -> Option<&SettingField> {
        self.fields.iter().find(|f| f.key == key)
    }

    /// Checks the schema: ids and keys, lengths, consistent limits, compilable patterns, and defaults that pass their own
    /// validation.
    pub fn check(&self) -> Result<(), SchemaError> {
        check_id("section id", &self.id)?;
        check_len("title", &self.title, MAX_LABEL_LEN, true)?;
        check_len("description", &self.description, MAX_DESCRIPTION_LEN, false)?;
        if self.fields.len() > MAX_FIELDS {
            return Err(SchemaError(format!(
                "section \"{}\" has {} fields; the limit is {MAX_FIELDS}",
                self.id,
                self.fields.len()
            )));
        }
        let mut seen = HashSet::new();
        for f in &self.fields {
            check_id("field key", &f.key)?;
            let at = |e: SchemaError| SchemaError(format!("field \"{}\": {}", f.key, e.0));
            if !seen.insert(f.key.as_str()) {
                return Err(SchemaError(format!(
                    "field \"{}\" is declared twice",
                    f.key
                )));
            }
            check_len("label", &f.label, MAX_LABEL_LEN, true).map_err(at)?;
            check_len("description", &f.description, MAX_DESCRIPTION_LEN, false).map_err(at)?;
            check_kind(&f.kind).map_err(at)?;
            crate::values::validate_field(f, &f.default).map_err(|e| {
                SchemaError(format!("field \"{}\": default {}: {e}", f.key, f.default))
            })?;
        }
        Ok(())
    }
}

/// A defect in a section's schema (a bug in the app or the shell), with what to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaError(pub String);

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SchemaError {}

/// Whether `id` is a section id or field key: 1 to [`MAX_ID_LEN`] characters of `A-Z a-z 0-9 . _ -`.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn check_id(what: &str, id: &str) -> Result<(), SchemaError> {
    if valid_id(id) {
        Ok(())
    } else {
        Err(SchemaError(format!(
            "{what} \"{}\" must be 1-{MAX_ID_LEN} characters of A-Z, a-z, 0-9, '.', '_' or '-'",
            id.chars().take(MAX_ID_LEN).collect::<String>()
        )))
    }
}

fn check_len(what: &str, text: &str, max: usize, required: bool) -> Result<(), SchemaError> {
    if required && text.trim().is_empty() {
        return Err(SchemaError(format!("the {what} is empty")));
    }
    if text.chars().count() > max {
        return Err(SchemaError(format!(
            "the {what} is longer than {max} characters"
        )));
    }
    Ok(())
}

fn check_kind(kind: &FieldKind) -> Result<(), SchemaError> {
    let fail = |m: String| Err(SchemaError(m));
    match kind {
        FieldKind::Int { min, max, step, .. } if min > max || *step < 1 => {
            fail("needs min <= max and step >= 1".into())
        }
        FieldKind::Float { min, max, step, .. }
            if !(min.is_finite() && max.is_finite() && step.is_finite())
                || min > max
                || *step <= 0.0 =>
        {
            fail("needs finite min <= max and step > 0".into())
        }
        FieldKind::Text { max_len, .. } if *max_len == 0 || *max_len > MAX_TEXT_LEN => {
            fail(format!("max_len must be 1-{MAX_TEXT_LEN}"))
        }
        FieldKind::Text {
            pattern: Some(p), ..
        } => anchored(p).map(|_| ()).map_err(SchemaError),
        FieldKind::Enum { options } => {
            let mut seen = HashSet::new();
            if options.is_empty() || options.len() > MAX_OPTIONS {
                return fail(format!("needs 1-{MAX_OPTIONS} options"));
            }
            for o in options {
                if !seen.insert(o.value.as_str()) {
                    return fail(format!("option \"{}\" is declared twice", o.value));
                }
                check_len("option value", &o.value, MAX_LABEL_LEN, false)?;
                check_len("option label", &o.label, MAX_LABEL_LEN, true)?;
            }
            Ok(())
        }
        FieldKind::Duration { min_ms, max_ms } if min_ms > max_ms => {
            fail("needs min_ms <= max_ms".into())
        }
        FieldKind::Size {
            min_bytes,
            max_bytes,
        } if min_bytes > max_bytes => fail("needs min_bytes <= max_bytes".into()),
        FieldKind::StringList { max_items } if *max_items == 0 || *max_items > MAX_LIST_ITEMS => {
            fail(format!("max_items must be 1-{MAX_LIST_ITEMS}"))
        }
        _ => Ok(()),
    }
}

/// Compiles a text pattern matched against the whole value, within the length and size limits.
pub(crate) fn anchored(pattern: &str) -> Result<regex::Regex, String> {
    if pattern.chars().count() > MAX_PATTERN_LEN {
        return Err(format!("pattern: longer than {MAX_PATTERN_LEN} characters"));
    }
    regex::RegexBuilder::new(&format!("^(?:{pattern})$"))
        .size_limit(MAX_PATTERN_BYTES)
        .dfa_size_limit(MAX_PATTERN_BYTES)
        .build()
        .map_err(|e| format!("pattern: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn section(field: SettingField) -> SettingsSection {
        SettingsSection::new("sd.s", "S", SectionOwner::App).field(field)
    }

    fn problem(kind: FieldKind, default: Value) -> String {
        section(SettingField::new("k", "K", kind, default))
            .check()
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn a_well_formed_section_passes_and_round_trips_as_json() {
        let s = SettingsSection::new("pd.appearance", "Appearance", SectionOwner::Shell)
            .describe("d")
            .field(
                SettingField::new(
                    "n",
                    "N",
                    FieldKind::Int {
                        min: 0,
                        max: 10,
                        step: 2,
                        unit: None,
                    },
                    4,
                )
                .describe("x")
                .needs_restart()
                .as_advanced(),
            )
            .field(SettingField::new(
                "e",
                "E",
                FieldKind::Enum {
                    options: vec![EnumOption::new("a", "A")],
                },
                "a",
            ));
        s.check().unwrap();
        let back: SettingsSection =
            serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        assert_eq!(s.find("e").map(|f| f.label.as_str()), Some("E"));
        assert!(s.find("missing").is_none());
        assert_eq!(
            serde_json::to_value(SectionOwner::Shell).unwrap(),
            json!("shell")
        );
    }

    #[test]
    fn ids_lengths_and_duplicates_are_refused() {
        assert!(
            valid_id("pd.a_b-1")
                && !valid_id("")
                && !valid_id("a b")
                && !valid_id(&"a".repeat(129))
        );
        let bad_id = SettingsSection::new("bad id", "T", SectionOwner::App)
            .check()
            .unwrap_err();
        assert!(bad_id.0.contains("section id"), "{bad_id}");
        let empty_title = SettingsSection::new("ok", " ", SectionOwner::App)
            .check()
            .unwrap_err();
        assert!(empty_title.0.contains("title is empty"));
        let long = SettingsSection::new("ok", "T", SectionOwner::App)
            .describe("d".repeat(MAX_DESCRIPTION_LEN + 1));
        assert!(long.check().unwrap_err().0.contains("longer than"));
        let mut many = SettingsSection::new("ok", "T", SectionOwner::App);
        for i in 0..=MAX_FIELDS {
            many = many.field(SettingField::new(
                format!("f{i}"),
                "F",
                FieldKind::Bool,
                false,
            ));
        }
        assert!(many.check().unwrap_err().0.contains("limit is 200"));
        let twice = section(SettingField::new("k", "K", FieldKind::Bool, true))
            .field(SettingField::new("k", "K", FieldKind::Bool, true));
        assert!(twice.check().unwrap_err().0.contains("declared twice"));
        let bad_key = section(SettingField::new("k k", "K", FieldKind::Bool, true));
        assert!(bad_key.check().unwrap_err().0.contains("field key"));
        let no_label = section(SettingField::new("k", "", FieldKind::Bool, true));
        assert!(
            no_label
                .check()
                .unwrap_err()
                .0
                .contains("field \"k\": the label is empty")
        );
    }

    #[test]
    fn inconsistent_limits_and_bad_defaults_are_refused() {
        let int = |min, max, step| FieldKind::Int {
            min,
            max,
            step,
            unit: None,
        };
        assert!(problem(int(5, 1, 1), json!(1)).contains("min <= max"));
        assert!(problem(int(0, 5, 0), json!(1)).contains("step >= 1"));
        let float = |min: f64, max: f64, step: f64| FieldKind::Float {
            min,
            max,
            step,
            unit: None,
        };
        assert!(problem(float(0.0, f64::INFINITY, 1.0), json!(1)).contains("finite"));
        assert!(problem(float(2.0, 1.0, 1.0), json!(1)).contains("finite"));
        assert!(problem(float(0.0, 1.0, 0.0), json!(1)).contains("step > 0"));
        assert!(
            problem(
                FieldKind::Text {
                    max_len: 0,
                    pattern: None
                },
                json!("")
            )
            .contains("max_len")
        );
        assert!(
            problem(
                FieldKind::Text {
                    max_len: MAX_TEXT_LEN + 1,
                    pattern: None
                },
                json!("")
            )
            .contains("max_len")
        );
        assert!(
            problem(
                FieldKind::Text {
                    max_len: 9,
                    pattern: Some("(".into())
                },
                json!("")
            )
            .contains("pattern")
        );
        assert!(
            problem(
                FieldKind::Text {
                    max_len: 9,
                    pattern: Some("a".repeat(MAX_PATTERN_LEN + 1))
                },
                json!("")
            )
            .contains("longer")
        );
        assert!(
            problem(
                FieldKind::Text {
                    max_len: 9,
                    pattern: Some("a{1000}{1000}".into())
                },
                json!("")
            )
            .contains("pattern")
        );
        assert!(problem(FieldKind::Enum { options: vec![] }, json!("a")).contains("options"));
        let twice = vec![EnumOption::new("a", "A"), EnumOption::new("a", "B")];
        assert!(problem(FieldKind::Enum { options: twice }, json!("a")).contains("declared twice"));
        let unlabelled = vec![EnumOption::new("a", "")];
        assert!(
            problem(
                FieldKind::Enum {
                    options: unlabelled
                },
                json!("a")
            )
            .contains("option label")
        );
        assert!(
            problem(
                FieldKind::Duration {
                    min_ms: 2,
                    max_ms: 1
                },
                json!(1)
            )
            .contains("min_ms")
        );
        assert!(
            problem(
                FieldKind::Size {
                    min_bytes: 2,
                    max_bytes: 1
                },
                json!(1)
            )
            .contains("min_bytes")
        );
        assert!(problem(FieldKind::StringList { max_items: 0 }, json!([])).contains("max_items"));
        // A default that fails its own validation.
        let bad_default = problem(int(0, 5, 1), json!(9));
        assert!(
            bad_default.contains("default 9: Must be between 0 and 5."),
            "{bad_default}"
        );
        assert_eq!(SchemaError("x".into()).to_string(), "x");
    }
}
