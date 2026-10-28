//! Validation of untrusted values against a section (parse, don't validate): [`validate`] turns a JSON object from the UI
//! or the settings file into [`ValidValues`]: unknown keys are dropped, missing keys take their default, and every type
//! and range problem is reported per field with a message the user can act on. [`ValidValues::decode`] then gives the
//! app its own typed struct.

use crate::format::format_ms;
use crate::schema::{
    FieldKind, MAX_ITEM_LEN, MAX_PATH_LEN, SettingField, SettingsSection, anchored,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::time::Duration;

/// Values that passed [`validate`] (or [`sanitize`]): every field of the section is present, of its kind and within its
/// limits.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ValidValues(BTreeMap<String, Value>);

impl ValidValues {
    /// The raw value of `key`.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        self.get(key)?.as_bool()
    }

    pub fn int(&self, key: &str) -> Option<i64> {
        self.get(key)?.as_i64()
    }

    pub fn float(&self, key: &str) -> Option<f64> {
        self.get(key)?.as_f64()
    }

    /// Text, enum and path values.
    pub fn text(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_str()
    }

    /// A duration (stored as milliseconds).
    pub fn duration(&self, key: &str) -> Option<Duration> {
        self.get(key)?.as_u64().map(Duration::from_millis)
    }

    /// A size, in bytes.
    pub fn bytes(&self, key: &str) -> Option<u64> {
        self.get(key)?.as_u64()
    }

    /// A list of text.
    pub fn strings(&self, key: &str) -> Option<Vec<String>> {
        self.get(key)?
            .as_array()?
            .iter()
            .map(|v| v.as_str().map(String::from))
            .collect()
    }

    /// Every key and value, ordered by key.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.0.iter()
    }

    /// The values as a JSON object.
    pub fn to_json(&self) -> Value {
        Value::Object(
            self.0
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Map<_, _>>(),
        )
    }

    /// The values as the app's own type (`#[derive(Deserialize)] #[serde(rename_all = "camelCase")] struct ...`), whose
    /// fields are the section's keys. An error means the struct and the section disagree: a bug, not bad input.
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.to_json())
    }
}

/// Why a value was refused; `key` is empty for a problem with the whole object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FieldError {
    pub key: String,
    /// What is wrong, for the user.
    pub message: String,
}

impl FieldError {
    pub fn new(key: impl Into<String>, message: impl Into<String>) -> Self {
        FieldError {
            key: key.into(),
            message: message.into(),
        }
    }
}

/// Validates `values` (a JSON object) against `section`. Unknown keys are dropped, missing or `null` keys take their
/// default; any invalid field refuses the whole set, with one error per field.
pub fn validate(section: &SettingsSection, values: &Value) -> Result<ValidValues, Vec<FieldError>> {
    let (valid, errors) = sanitize(section, values);
    if errors.is_empty() {
        Ok(valid)
    } else {
        Err(errors)
    }
}

/// Like [`validate`], but replaces each invalid field by its default and returns the problems alongside (for stored
/// values, which must never stop the app).
pub fn sanitize(section: &SettingsSection, values: &Value) -> (ValidValues, Vec<FieldError>) {
    let empty = Map::new();
    let mut errors = Vec::new();
    let object = match values {
        Value::Object(o) => o,
        Value::Null => &empty,
        _ => {
            errors.push(FieldError::new(
                "",
                "Settings must be an object of key/value pairs.",
            ));
            &empty
        }
    };
    let mut out = BTreeMap::new();
    for field in &section.fields {
        let value = match object.get(&field.key) {
            None | Some(Value::Null) => field.default.clone(),
            Some(v) => validate_field(field, v).unwrap_or_else(|message| {
                errors.push(FieldError::new(field.key.clone(), message));
                field.default.clone()
            }),
        };
        out.insert(field.key.clone(), value);
    }
    (ValidValues(out), errors)
}

/// Checks one value, returning it normalised (`5.0` becomes `5` for an int).
pub fn validate_field(field: &SettingField, value: &Value) -> Result<Value, String> {
    match &field.kind {
        FieldKind::Bool => value
            .as_bool()
            .map(Value::from)
            .ok_or_else(|| "Must be on or off (true or false).".into()),
        FieldKind::Int {
            min,
            max,
            step,
            unit,
        } => {
            let n = whole(value).ok_or("Must be a whole number.")?;
            if n < *min || n > *max {
                return Err(format!(
                    "Must be between {} and {}.",
                    with_unit(*min, unit),
                    with_unit(*max, unit)
                ));
            }
            if (n - min) % step != 0 {
                return Err(format!("Must be {min} plus a multiple of {step}."));
            }
            Ok(Value::from(n))
        }
        FieldKind::Float { min, max, unit, .. } => {
            let n = value
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or("Must be a number.")?;
            if n < *min || n > *max {
                return Err(format!(
                    "Must be between {} and {}.",
                    with_unit(*min, unit),
                    with_unit(*max, unit)
                ));
            }
            Ok(Value::from(n))
        }
        FieldKind::Text { max_len, pattern } => {
            let s = value.as_str().ok_or("Must be text.")?;
            if s.chars().count() > *max_len {
                return Err(format!("Must be at most {max_len} characters."));
            }
            if let Some(p) = pattern {
                let re = anchored(p).map_err(|e| format!("The setting's {e}"))?;
                if !re.is_match(s) {
                    return Err(format!("Does not have the expected format ({p})."));
                }
            }
            Ok(Value::from(s))
        }
        FieldKind::Enum { options } => value
            .as_str()
            .filter(|s| options.iter().any(|o| o.value == *s))
            .map(Value::from)
            .ok_or_else(|| {
                let names: Vec<&str> = options.iter().map(|o| o.value.as_str()).collect();
                format!("Must be one of: {}.", names.join(", "))
            }),
        FieldKind::Path { .. } => {
            let s = value.as_str().ok_or("Must be a path.")?;
            if s.chars().count() > MAX_PATH_LEN {
                return Err(format!("Must be at most {MAX_PATH_LEN} characters."));
            }
            if s.contains('\0') {
                return Err("Must not contain NUL characters.".into());
            }
            if !s.is_empty() && !is_absolute(s) {
                return Err("Must be an absolute path (or empty).".into());
            }
            Ok(Value::from(s))
        }
        FieldKind::Duration { min_ms, max_ms } => {
            let n = unsigned(value).ok_or("Must be a duration in whole milliseconds.")?;
            if n < *min_ms || n > *max_ms {
                return Err(format!(
                    "Must be between {} and {}.",
                    format_ms(*min_ms),
                    format_ms(*max_ms)
                ));
            }
            Ok(Value::from(n))
        }
        FieldKind::Size {
            kind,
            min_bytes,
            max_bytes,
        } => {
            let n = unsigned(value).ok_or("Must be a size in whole bytes.")?;
            if n < *min_bytes || n > *max_bytes {
                return Err(format!(
                    "Must be between {} and {}.",
                    paddock_units::format_whole(*min_bytes, (*kind).into()),
                    paddock_units::format_whole(*max_bytes, (*kind).into())
                ));
            }
            Ok(Value::from(n))
        }
        FieldKind::StringList { max_items } => {
            let items = value.as_array().ok_or("Must be a list of text.")?;
            if items.len() > *max_items {
                return Err(format!("Must have at most {max_items} items."));
            }
            let mut out = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                let s = item
                    .as_str()
                    .ok_or_else(|| format!("Item {} must be text.", i + 1))?;
                if s.chars().count() > MAX_ITEM_LEN {
                    return Err(format!(
                        "Item {} must be at most {MAX_ITEM_LEN} characters.",
                        i + 1
                    ));
                }
                out.push(Value::from(s));
            }
            Ok(Value::Array(out))
        }
    }
}

/// An absolute path on this platform, or a Windows one (drive or UNC) anywhere: a settings file moves between machines,
/// and the UI may run on another OS than the one that checks.
fn is_absolute(s: &str) -> bool {
    let b = s.as_bytes();
    let drive = b.len() >= 3
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (b[2] == b'\\' || b[2] == b'/');
    std::path::Path::new(s).is_absolute() || s.starts_with('/') || s.starts_with("\\\\") || drive
}

/// An integer, also from a float without a fractional part (JavaScript numbers).
fn whole(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| {
        value
            .as_f64()
            .filter(|f| f.fract() == 0.0 && f.abs() < 9.0e15)
            .map(|f| f as i64)
    })
}

fn unsigned(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| whole(value).filter(|n| *n >= 0).map(i64::unsigned_abs))
}

fn with_unit<T: std::fmt::Display>(n: T, unit: &Option<String>) -> String {
    match unit {
        Some(u) if !u.is_empty() => format!("{n} {u}"),
        _ => n.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ByteKind, EnumOption, SectionOwner};
    use serde_json::json;

    fn field(kind: FieldKind, default: Value) -> SettingField {
        SettingField::new("k", "K", kind, default)
    }

    fn check(kind: FieldKind, value: Value) -> Result<Value, String> {
        validate_field(&field(kind, Value::Null), &value)
    }

    #[test]
    fn every_field_kind_accepts_its_values_and_refuses_others_with_a_message() {
        assert_eq!(check(FieldKind::Bool, json!(true)), Ok(json!(true)));
        assert!(
            check(FieldKind::Bool, json!("yes"))
                .unwrap_err()
                .contains("on or off")
        );

        let int = FieldKind::Int {
            min: 0,
            max: 10,
            step: 2,
            unit: Some("lines".into()),
        };
        assert_eq!(
            check(int.clone(), json!(6.0)),
            Ok(json!(6)),
            "an integral float is a whole number"
        );
        assert!(
            check(int.clone(), json!(5.5))
                .unwrap_err()
                .contains("whole number")
        );
        assert_eq!(
            check(int.clone(), json!(12)).unwrap_err(),
            "Must be between 0 lines and 10 lines."
        );
        assert_eq!(
            check(int.clone(), json!(3)).unwrap_err(),
            "Must be 0 plus a multiple of 2."
        );
        assert!(check(int, json!(1e300)).is_err());

        let float = FieldKind::Float {
            min: 0.5,
            max: 2.0,
            step: 0.1,
            unit: Some(String::new()),
        };
        assert_eq!(check(float.clone(), json!(1.5)), Ok(json!(1.5)));
        assert_eq!(
            check(float.clone(), json!(3)).unwrap_err(),
            "Must be between 0.5 and 2."
        );
        assert!(check(float, json!("1")).unwrap_err().contains("a number"));

        let text = FieldKind::Text {
            max_len: 5,
            pattern: Some("[a-z]+".into()),
        };
        assert_eq!(check(text.clone(), json!("abc")), Ok(json!("abc")));
        assert!(
            check(text.clone(), json!("abcdef"))
                .unwrap_err()
                .contains("at most 5")
        );
        assert!(
            check(text.clone(), json!("ab1"))
                .unwrap_err()
                .contains("expected format ([a-z]+)")
        );
        assert!(check(text, json!(1)).unwrap_err().contains("text"));
        let broken = FieldKind::Text {
            max_len: 5,
            pattern: Some("(".into()),
        };
        assert!(
            check(broken, json!("a"))
                .unwrap_err()
                .contains("The setting's pattern")
        );

        let options = FieldKind::Enum {
            options: vec![
                EnumOption::new("light", "Light"),
                EnumOption::new("dark", "Dark"),
            ],
        };
        assert_eq!(check(options.clone(), json!("dark")), Ok(json!("dark")));
        assert_eq!(
            check(options, json!("blue")).unwrap_err(),
            "Must be one of: light, dark."
        );

        let path = FieldKind::Path { dir: true };
        assert_eq!(
            check(path.clone(), json!("")),
            Ok(json!("")),
            "empty is not set"
        );
        assert_eq!(
            check(path.clone(), json!("/var/log")),
            Ok(json!("/var/log"))
        );
        assert_eq!(
            check(path.clone(), json!("C:\\Logs")),
            Ok(json!("C:\\Logs"))
        );
        assert_eq!(
            check(path.clone(), json!("\\\\server\\share")),
            Ok(json!("\\\\server\\share"))
        );
        assert!(
            check(path.clone(), json!("logs/today"))
                .unwrap_err()
                .contains("absolute")
        );
        assert!(
            check(path.clone(), json!("/a\0b"))
                .unwrap_err()
                .contains("NUL")
        );
        assert!(
            check(
                path.clone(),
                json!(format!("/{}", "a".repeat(MAX_PATH_LEN)))
            )
            .unwrap_err()
            .contains("at most")
        );
        assert!(check(path, json!(3)).unwrap_err().contains("a path"));

        let duration = FieldKind::Duration {
            min_ms: 1000,
            max_ms: 3_600_000,
        };
        assert_eq!(check(duration.clone(), json!(90_000)), Ok(json!(90_000)));
        assert_eq!(
            check(duration.clone(), json!(500)).unwrap_err(),
            "Must be between 1 s and 1 h."
        );
        assert!(
            check(duration, json!(-1))
                .unwrap_err()
                .contains("whole milliseconds")
        );

        let size = FieldKind::Size {
            kind: ByteKind::Storage,
            min_bytes: 1000,
            max_bytes: 1_000_000_000,
        };
        assert_eq!(check(size.clone(), json!(2000.0)), Ok(json!(2000)));
        assert_eq!(
            check(size.clone(), json!(10)).unwrap_err(),
            "Must be between 1 KB and 1 GB."
        );
        let memory = FieldKind::Size {
            kind: ByteKind::Memory,
            min_bytes: 1024,
            max_bytes: 1 << 30,
        };
        assert_eq!(
            check(memory, json!(10)).unwrap_err(),
            "Must be between 1 KB and 1 GB."
        );
        assert!(
            check(size, json!("1 KB"))
                .unwrap_err()
                .contains("whole bytes")
        );

        let list = FieldKind::StringList { max_items: 2 };
        assert_eq!(
            check(list.clone(), json!(["a", "b"])),
            Ok(json!(["a", "b"]))
        );
        assert!(
            check(list.clone(), json!(["a", "b", "c"]))
                .unwrap_err()
                .contains("at most 2 items")
        );
        assert_eq!(
            check(list.clone(), json!(["a", 1])).unwrap_err(),
            "Item 2 must be text."
        );
        assert!(
            check(list.clone(), json!(["x".repeat(MAX_ITEM_LEN + 1)]))
                .unwrap_err()
                .contains("Item 1 must be at most")
        );
        assert!(check(list, json!("a")).unwrap_err().contains("list"));
    }

    fn section() -> SettingsSection {
        SettingsSection::new("sd.s", "S", SectionOwner::App)
            .field(field(FieldKind::Bool, json!(false)))
            .field(SettingField::new(
                "n",
                "N",
                FieldKind::Int {
                    min: 0,
                    max: 9,
                    step: 1,
                    unit: None,
                },
                3,
            ))
    }

    #[test]
    fn validate_drops_unknown_keys_fills_defaults_and_refuses_the_whole_set_on_any_error() {
        let s = section();
        let v = validate(&s, &json!({ "k": true, "other": 1 })).unwrap();
        assert_eq!(v.to_json(), json!({ "k": true, "n": 3 }));
        assert_eq!(
            validate(&s, &Value::Null).unwrap().to_json(),
            json!({ "k": false, "n": 3 })
        );
        assert_eq!(
            validate(&s, &json!({ "n": null })).unwrap().int("n"),
            Some(3)
        );
        let errors = validate(&s, &json!({ "k": 1, "n": 99 })).unwrap_err();
        assert_eq!(
            errors.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
            ["k", "n"]
        );
        let whole = validate(&s, &json!([1])).unwrap_err();
        assert_eq!(
            whole,
            [FieldError::new(
                "",
                "Settings must be an object of key/value pairs."
            )]
        );
    }

    #[test]
    fn sanitize_replaces_each_bad_value_by_its_default() {
        let (v, errors) = sanitize(&section(), &json!({ "k": true, "n": "x" }));
        assert_eq!(v.to_json(), json!({ "k": true, "n": 3 }));
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn typed_accessors_and_decode() {
        let s = SettingsSection::new("sd.s", "S", SectionOwner::App)
            .field(SettingField::new("on", "On", FieldKind::Bool, true))
            .field(SettingField::new(
                "ratio",
                "R",
                FieldKind::Float {
                    min: 0.0,
                    max: 1.0,
                    step: 0.1,
                    unit: None,
                },
                0.5,
            ))
            .field(SettingField::new(
                "name",
                "N",
                FieldKind::Text {
                    max_len: 9,
                    pattern: None,
                },
                "x",
            ))
            .field(SettingField::new(
                "wait",
                "W",
                FieldKind::Duration {
                    min_ms: 0,
                    max_ms: 9000,
                },
                1500,
            ))
            .field(SettingField::new(
                "cap",
                "C",
                FieldKind::Size {
                    kind: ByteKind::Storage,
                    min_bytes: 0,
                    max_bytes: 9000,
                },
                512,
            ))
            .field(SettingField::new(
                "tags",
                "T",
                FieldKind::StringList { max_items: 3 },
                json!(["a"]),
            ))
            .field(SettingField::new(
                "count",
                "C",
                FieldKind::Int {
                    min: 0,
                    max: 9,
                    step: 1,
                    unit: None,
                },
                2,
            ));
        let v = validate(&s, &json!({})).unwrap();
        assert_eq!(v.bool("on"), Some(true));
        assert_eq!(v.float("ratio"), Some(0.5));
        assert_eq!(v.text("name"), Some("x"));
        assert_eq!(v.duration("wait"), Some(Duration::from_millis(1500)));
        assert_eq!(v.bytes("cap"), Some(512));
        assert_eq!(v.strings("tags"), Some(vec!["a".to_string()]));
        assert_eq!(v.int("count"), Some(2));
        assert_eq!(v.iter().count(), 7);
        assert!(v.get("missing").is_none() && v.bool("name").is_none());

        #[derive(serde::Deserialize, Debug, PartialEq)]
        struct Typed {
            on: bool,
            count: u8,
            tags: Vec<String>,
        }
        assert_eq!(
            v.decode::<Typed>().unwrap(),
            Typed {
                on: true,
                count: 2,
                tags: vec!["a".into()]
            }
        );
        #[derive(serde::Deserialize, Debug)]
        #[allow(dead_code)]
        struct Wrong {
            absent: bool,
        }
        assert!(
            v.decode::<Wrong>().is_err(),
            "a struct that disagrees with the section is an error"
        );
        assert_eq!(ValidValues::default().iter().count(), 0);
    }
}
