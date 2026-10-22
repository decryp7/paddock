//! The configuration schema of a source kind: which fields its form has, and the values a user entered.
//!
//! Built-in sources and source packs describe their settings with [`FieldSpec`]s; the app renders the form from
//! them and checks entered values with [`FieldSpec::check`] before it saves a watch (the engine checks again).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Longest value of one field, in bytes.
pub const MAX_VALUE_LEN: usize = 4096;
/// Most fields in one schema or values in one [`Config`].
pub const MAX_FIELDS: usize = 64;

/// One field of a source kind's form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldSpec {
    /// The key the value is stored under (`[a-z][a-zA-Z0-9_]*`).
    pub key: String,
    /// The label shown next to the input.
    pub label: String,
    /// One line of help under the input (may be empty).
    pub help: String,
    /// What kind of value it takes.
    pub kind: FieldKind,
    /// Whether a value must be given.
    pub required: bool,
    /// The value used when none is given (empty for none).
    pub default: String,
    /// An example shown in the empty input (may be empty).
    pub placeholder: String,
}

/// What kind of value a field takes. Secrets are never stored in the configuration: the app keeps them in the
/// operating system's credential store and passes them to the source only when it checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldKind {
    /// One line of text.
    Text,
    /// An `http://` or `https://` address.
    Url,
    /// A folder (local, mounted or a UNC share).
    Folder,
    /// A token or password (kept in the credential store).
    Secret,
    /// A whole number in `min..=max`.
    Integer {
        /// Smallest value.
        min: i64,
        /// Largest value.
        max: i64,
    },
    /// On or off (`true` / `false`).
    Toggle,
    /// One of a fixed set of values.
    Choice(Vec<ChoiceOption>),
}

/// One value of a [`FieldKind::Choice`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoiceOption {
    /// The stored value.
    pub value: String,
    /// What the user sees.
    pub label: String,
}

impl ChoiceOption {
    /// An option.
    pub fn new(value: &str, label: &str) -> ChoiceOption {
        ChoiceOption {
            value: value.into(),
            label: label.into(),
        }
    }
}

impl FieldSpec {
    /// A field with no help, default or placeholder.
    pub fn new(key: &str, label: &str, kind: FieldKind) -> FieldSpec {
        FieldSpec {
            key: key.into(),
            label: label.into(),
            help: String::new(),
            kind,
            required: false,
            default: String::new(),
            placeholder: String::new(),
        }
    }

    /// The same field, required.
    pub fn required(mut self) -> FieldSpec {
        self.required = true;
        self
    }

    /// The same field with help text.
    pub fn help(mut self, help: &str) -> FieldSpec {
        self.help = help.into();
        self
    }

    /// The same field with a default value.
    pub fn default_value(mut self, value: &str) -> FieldSpec {
        self.default = value.into();
        self
    }

    /// The same field with a placeholder.
    pub fn placeholder(mut self, placeholder: &str) -> FieldSpec {
        self.placeholder = placeholder.into();
        self
    }

    /// Checks one entered value (`None` or empty: not given). The message says what to fix.
    pub fn check(&self, value: Option<&str>) -> Result<(), String> {
        let value = value.map(str::trim).filter(|v| !v.is_empty());
        let Some(v) = value else {
            return if self.required && self.default.is_empty() {
                Err(format!("{} is required.", self.label))
            } else {
                Ok(())
            };
        };
        if v.len() > MAX_VALUE_LEN {
            return Err(format!("{} is too long (at most {MAX_VALUE_LEN} characters).", self.label));
        }
        match &self.kind {
            FieldKind::Text | FieldKind::Folder | FieldKind::Secret => Ok(()),
            FieldKind::Url => {
                let lower = v.to_ascii_lowercase();
                if (lower.starts_with("https://") || lower.starts_with("http://")) && v.len() > "http://".len() + 1 {
                    Ok(())
                } else {
                    Err(format!("{} must start with https:// or http://.", self.label))
                }
            }
            FieldKind::Integer { min, max } => match v.parse::<i64>() {
                Ok(n) if (*min..=*max).contains(&n) => Ok(()),
                _ => Err(format!("{} must be a whole number from {min} to {max}.", self.label)),
            },
            FieldKind::Toggle => match v {
                "true" | "false" => Ok(()),
                _ => Err(format!("{} must be true or false.", self.label)),
            },
            FieldKind::Choice(options) => {
                if options.iter().any(|o| o.value == v) {
                    Ok(())
                } else {
                    Err(format!("{} must be one of the listed choices.", self.label))
                }
            }
        }
    }
}

/// Checks every value against a schema: required fields, kinds, and no unknown keys. Returns every problem.
pub fn check_all(fields: &[FieldSpec], config: &Config) -> Result<(), Vec<String>> {
    let mut problems: Vec<String> = fields.iter().filter_map(|f| f.check(config.get(&f.key)).err()).collect();
    for key in config.0.keys() {
        if !fields.iter().any(|f| &f.key == key) {
            problems.push(format!("\"{key}\" is not a setting of this source."));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// A source kind: an id, how it is shown and its configuration form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceKindSpec {
    /// Stable id (`[a-z0-9-]+`).
    pub id: String,
    /// Display name.
    pub label: String,
    /// One sentence about it.
    pub description: String,
    /// The form.
    pub fields: Vec<FieldSpec>,
}

impl SourceKindSpec {
    /// A kind without fields.
    pub fn new(id: &str, label: &str, description: &str) -> SourceKindSpec {
        SourceKindSpec { id: id.into(), label: label.into(), description: description.into(), fields: Vec::new() }
    }

    /// The same kind with one more field.
    pub fn with_field(mut self, field: FieldSpec) -> SourceKindSpec {
        self.fields.push(field);
        self
    }
}

/// The values a user entered for one source, by field key. Values are text; the typed getters parse them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config(pub BTreeMap<String, String>);

impl Config {
    /// An empty configuration.
    pub fn new() -> Config {
        Config::default()
    }

    /// The same configuration with one more value.
    pub fn with(mut self, key: &str, value: &str) -> Config {
        self.0.insert(key.into(), value.into());
        self
    }

    /// A value, trimmed; `None` when missing or empty.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|v| v.trim()).filter(|v| !v.is_empty())
    }

    /// A value that must be given.
    pub fn require(&self, key: &str) -> Result<&str, String> {
        self.get(key).ok_or_else(|| format!("the setting \"{key}\" is missing"))
    }

    /// A toggle (`true` when the value is `true`).
    pub fn flag(&self, key: &str) -> bool {
        self.get(key) == Some("true")
    }

    /// A whole number, `default` when missing; an error when not a number.
    pub fn integer(&self, key: &str, default: i64) -> Result<i64, String> {
        match self.get(key) {
            None => Ok(default),
            Some(v) => v.parse().map_err(|_| format!("the setting \"{key}\" must be a whole number")),
        }
    }

    /// Whether the configuration is within the limits ([`MAX_FIELDS`], [`MAX_VALUE_LEN`]).
    pub fn within_limits(&self) -> bool {
        self.0.len() <= MAX_FIELDS && self.0.iter().all(|(k, v)| k.len() <= 64 && v.len() <= MAX_VALUE_LEN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int() -> FieldSpec {
        FieldSpec::new("n", "Count", FieldKind::Integer { min: 1, max: 10 })
    }

    #[test]
    fn fields_check_their_values() {
        let url = FieldSpec::new("url", "Address", FieldKind::Url).required();
        assert_eq!(url.check(None).unwrap_err(), "Address is required.");
        assert!(url.check(Some("  ")).is_err());
        assert!(url.check(Some("https://example.org/x")).is_ok());
        assert!(url.check(Some("HTTP://example.org")).is_ok());
        assert!(url.check(Some("ftp://example.org")).unwrap_err().contains("https://"));
        assert!(url.check(Some("https://")).is_err());
        assert!(url.check(Some(&"h".repeat(MAX_VALUE_LEN + 1))).unwrap_err().contains("too long"));

        assert!(int().check(Some("10")).is_ok());
        assert!(int().check(Some("0")).unwrap_err().contains("from 1 to 10"));
        assert!(int().check(Some("x")).is_err());
        assert!(int().check(None).is_ok(), "optional");
        assert!(
            int().required().default_value("3").check(None).is_ok(),
            "a default fills a required field"
        );

        let toggle = FieldSpec::new("t", "On", FieldKind::Toggle);
        assert!(toggle.check(Some("true")).is_ok() && toggle.check(Some("false")).is_ok());
        assert!(toggle.check(Some("yes")).is_err());

        let choice = FieldSpec::new("c", "Pick", FieldKind::Choice(vec![ChoiceOption::new("a", "A")]));
        assert!(choice.check(Some("a")).is_ok());
        assert!(choice.check(Some("b")).is_err());
        for kind in [FieldKind::Text, FieldKind::Folder, FieldKind::Secret] {
            assert!(FieldSpec::new("x", "X", kind)
                .help("h")
                .placeholder("p")
                .check(Some("anything"))
                .is_ok());
        }
    }

    #[test]
    fn whole_configs_are_checked_against_the_schema() {
        let fields = vec![FieldSpec::new("url", "Address", FieldKind::Url).required(), int()];
        assert!(check_all(&fields, &Config::new().with("url", "https://a.b")).is_ok());
        let problems = check_all(&fields, &Config::new().with("n", "99").with("extra", "1")).unwrap_err();
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("\"extra\"")));
    }

    #[test]
    fn config_getters_parse_values() {
        let c = Config::new()
            .with("a", " x ")
            .with("b", "")
            .with("on", "true")
            .with("n", "42")
            .with("bad", "4x");
        assert_eq!(c.get("a"), Some("x"));
        assert_eq!(c.get("b"), None);
        assert_eq!(c.require("a"), Ok("x"));
        assert!(c.require("missing").unwrap_err().contains("missing"));
        assert!(c.flag("on") && !c.flag("a"));
        assert_eq!(c.integer("n", 1), Ok(42));
        assert_eq!(c.integer("missing", 7), Ok(7));
        assert!(c.integer("bad", 1).is_err());
        assert!(c.within_limits());
        assert!(!Config::new().with("k", &"v".repeat(MAX_VALUE_LEN + 1)).within_limits());
        assert!(!Config::new().with(&"k".repeat(65), "v").within_limits());
    }
}
