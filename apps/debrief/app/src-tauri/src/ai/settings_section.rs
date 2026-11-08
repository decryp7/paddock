//! Settings → AI: the fields of `debrief_ai::settings::schema()` as a section of the app's settings system.
//! Pitwall is the only engine (ADR 0070): the model, its runtime and its downloads are Pitwall's settings, so only what
//! shapes Debrief's own requests is a field here (context, answer length, creativity).
//! The section's values live in `settings.json` like every other section and are applied to the runtime
//! (and written to `ai.json`) at startup and on every change, so `settings.json` is their source of truth.

use super::core::AiCore;
use crate::settings::AI_SECTION;
use debrief_ai::settings::AiSettings;
use debrief_core::settings::{Configurable, EnumOption, FieldKind, SectionOwner, SettingField, SettingsSection, ValidValues};
use serde_json::Value;
use std::sync::Arc;

const MAX_TEXT: usize = 2048;

pub struct AiSettingsSection {
    pub core: Arc<AiCore>,
}

fn num(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64)
}

/// One schema entry as a field, or `None` for entries that are not settings fields.
fn field_of(f: &Value) -> Option<SettingField> {
    let key = f.get("key")?.as_str()?;
    let label = f.get("label").and_then(Value::as_str).unwrap_or(key);
    let desc = f.get("description").and_then(Value::as_str).unwrap_or("");
    let default = f.get("default").cloned().unwrap_or(Value::Null);
    let (kind, default) = match f.get("type")?.as_str()? {
        "bool" => (FieldKind::Bool, default),
        "int" => (
            FieldKind::Int {
                min: num(f, "min")? as i64,
                max: num(f, "max")? as i64,
                step: 1,
                unit: None,
            },
            default,
        ),
        "float" => (
            FieldKind::Float {
                min: num(f, "min")?,
                max: num(f, "max")?,
                step: 0.1,
                unit: None,
            },
            default,
        ),
        "enum" => {
            let options = f
                .get("options")?
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .map(|o| EnumOption::new(o, o))
                .collect();
            (FieldKind::Enum { options }, default)
        }
        "string" => (
            FieldKind::Text {
                max_len: MAX_TEXT,
                pattern: None,
            },
            Value::from(default.as_str().unwrap_or("")),
        ),
        _ => return None,
    };
    let mut field = SettingField::new(key, label, kind, default).describe(desc);
    if key == "temperature" {
        field = field.as_advanced();
    }
    Some(field)
}

/// The section, from the library's schema.
pub fn section() -> SettingsSection {
    let mut s = SettingsSection::new(AI_SECTION, "AI assistant", SectionOwner::App).describe(
        "The assistant runs on Pitwall, the family's local model server, on this computer; nothing about your logs is sent anywhere. \
Web pages reach it only when you hand them over from the internal browser. The model, its memory and its downloads are chosen in Pitwall.",
    );
    if let Some(fields) = debrief_ai::settings::schema().as_array() {
        for f in fields.iter().filter_map(field_of) {
            s = s.field(f);
        }
    }
    s
}

/// Builds the settings the section's values describe, over `base`.
pub fn to_settings(values: &ValidValues, base: &AiSettings) -> AiSettings {
    let mut s = base.clone();
    let int = |k: &str, d: i64| values.int(k).unwrap_or(d);
    s.context_length = int("context_length", s.context_length as i64) as usize;
    s.max_tokens = int("max_tokens", s.max_tokens as i64) as usize;
    s.temperature = values.float("temperature").map_or(s.temperature, |t| t as f32);
    s.clamp()
}

impl Configurable for AiSettingsSection {
    fn settings_section(&self) -> SettingsSection {
        section()
    }

    fn apply_settings(&self, values: &ValidValues) -> anyhow::Result<()> {
        let next = to_settings(values, &self.core.settings());
        self.core.apply_settings(next).map_err(|e| anyhow::anyhow!(e.message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use debrief_core::settings::sanitize;
    use serde_json::json;

    #[test]
    fn the_section_is_valid_and_has_only_what_shapes_the_requests() {
        let s = section();
        s.check().unwrap();
        assert_eq!(s.id, AI_SECTION);
        let keys: Vec<&str> = s.fields.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, ["context_length", "max_tokens", "temperature"]);
        assert!(s.description.contains("Pitwall"));
        assert!(field_of(&json!({"key": "x", "type": "secret"})).is_none());
        assert!(
            field_of(&json!({"key": "x", "type": "int"})).is_none(),
            "an int without limits is skipped"
        );
    }

    #[test]
    fn defaults_map_back_to_default_settings() {
        let s = section();
        let (values, errors) = sanitize(&s, &Value::Null);
        assert!(errors.is_empty());
        assert_eq!(to_settings(&values, &AiSettings::default()), AiSettings::default());
    }

    #[test]
    fn values_become_settings() {
        let s = section();
        let (values, errors) = sanitize(&s, &json!({"context_length": 4096, "max_tokens": 512, "temperature": 0.5}));
        assert!(errors.is_empty(), "{errors:?}");
        let a = to_settings(&values, &AiSettings::default());
        assert_eq!((a.context_length, a.max_tokens), (4096, 512));
        assert!((a.temperature - 0.5).abs() < 1e-6);
    }
}
