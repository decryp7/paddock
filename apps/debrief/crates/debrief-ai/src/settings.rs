//! The AI settings section: one serde struct with defaults and clamping, a versioned file that tolerates corruption, and
//! a JSON description of the fields for the Settings screen. Pitwall is the only engine (ADR 0070), so only what shapes
//! Debrief's own requests is kept: the context it needs, the longest answer and the creativity. The model, its runtime
//! and its downloads are Pitwall's.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

pub const SETTINGS_VERSION: u32 = 2;

/// The keys of a 1.1 `ai.json` that mean nothing any more (the built-in engine's). The file still loads, since the
/// unknown keys are ignored; the next save drops them (ADR 0070 D4).
pub const REMOVED_KEYS: &[&str] = &[
    "active_model",
    "autoload",
    "idle_unload_minutes",
    "gpu_offload",
    "threads",
    "hf_endpoint",
    "download_proxy",
    "max_download_gb",
    "download_connections",
    "models_dir",
    "inference_build",
    "llama_server_path",
    "runtime_updates",
    "model_updates",
    "update_check_hours",
    "name_aliases",
    "engine",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    pub version: u32,
    /// The context Debrief's prompts need, in tokens (Pitwall is told, and picks a model that offers it).
    pub context_length: usize,
    pub max_tokens: usize,
    pub temperature: f32,
}

impl Default for AiSettings {
    fn default() -> Self {
        AiSettings {
            version: SETTINGS_VERSION,
            context_length: 8192,
            max_tokens: 1024,
            temperature: 0.2,
        }
    }
}

impl AiSettings {
    /// Brings every value into its allowed range (called after loading and before saving).
    pub fn clamp(mut self) -> Self {
        self.version = SETTINGS_VERSION;
        self.context_length = self.context_length.clamp(1024, 131_072);
        self.max_tokens = self.max_tokens.clamp(64, 16_384).min(self.context_length / 2);
        self.temperature = if self.temperature.is_finite() {
            self.temperature.clamp(0.0, 2.0)
        } else {
            0.2
        };
        self
    }

    /// Loads settings; a missing file gives defaults, a damaged or newer one gives defaults plus a warning. The keys of
    /// an older file that mean nothing any more are named in one log line and dropped at the next save.
    pub fn load(path: &Path) -> (AiSettings, Option<String>) {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (AiSettings::default(), None),
            Err(e) => {
                return (
                    AiSettings::default(),
                    Some(format!("cannot read the AI settings ({e}); using defaults")),
                )
            }
        };
        match serde_json::from_str::<AiSettings>(&text) {
            Ok(s) if s.version <= SETTINGS_VERSION => {
                if let Some(note) = removed_keys_in(&text) {
                    log::info!("{note}");
                }
                (s.clamp(), None)
            }
            Ok(_) => (
                AiSettings::default(),
                Some("the AI settings were saved by a newer Debrief; using defaults".into()),
            ),
            Err(_) => (
                AiSettings::default(),
                Some("the AI settings file is damaged; using defaults".into()),
            ),
        }
    }

    /// Writes atomically (temp file, then rename).
    pub fn save(&self, path: &Path) -> crate::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(&self.clone().clamp()).map_err(|e| crate::AiError::Io(e.to_string()))?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

/// The removed keys `text` (an `ai.json`) still holds, as one log line; none when there are none.
fn removed_keys_in(text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    let found: Vec<&str> = REMOVED_KEYS.iter().copied().filter(|k| v.get(*k).is_some()).collect();
    (!found.is_empty()).then(|| {
        format!(
            "AI settings: {} setting(s) of the built-in engine are no longer used and will be dropped ({}); Pitwall is the AI engine",
            found.len(),
            found.join(", ")
        )
    })
}

/// [`removed_keys_in`] for a file, or `None` when it is missing or holds none.
pub fn removed_keys_note(path: &Path) -> Option<String> {
    removed_keys_in(&std::fs::read_to_string(path).ok()?)
}

/// Description of every field for the Settings screen: key, label, description, type, default,
/// min, max, options, restartRequired.
pub fn schema() -> Value {
    let d = AiSettings::default();
    let f = |key: &str, label: &str, desc: &str, ty: &str, default: Value, extra: Value| {
        let mut o = json!({"key": key, "label": label, "description": desc, "type": ty, "default": default, "restartRequired": false});
        if let (Some(m), Some(e)) = (o.as_object_mut(), extra.as_object()) {
            m.extend(e.clone());
        }
        o
    };
    json!([
        f("context_length", "Context length (tokens)", "How much text Debrief's requests need the model to consider at once. Pitwall picks a model that offers it; larger needs more memory there.", "int", json!(d.context_length), json!({"min": 1024, "max": 131072})),
        f("max_tokens", "Longest answer (tokens)", "Hard cap on the length of one answer.", "int", json!(d.max_tokens), json!({"min": 64, "max": 16384})),
        f("temperature", "Creativity", "0 is the most predictable. Low values suit analysis.", "float", json!(d.temperature), json!({"min": 0.0, "max": 2.0})),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ADR 0070 D7.3: a 1.1 `ai.json` with the removed engine keys still loads; the kept keys keep their values, the
    /// removed keys are named in the log note and the saved file has none of them.
    #[test]
    fn a_1_1_file_with_engine_keys_loads_without_them() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ai.json");
        std::fs::write(
            &p,
            r#"{"version":1,"engine":"builtIn","active_model":"m.gguf","inference_build":"vulkan","models_dir":"/x/models",
                "name_aliases":false,"context_length":4096,"max_tokens":300,"temperature":0.5}"#,
        )
        .unwrap();
        let (s, warning) = AiSettings::load(&p);
        assert_eq!((s.context_length, s.max_tokens, s.temperature), (4096, 300, 0.5));
        assert_eq!(s.version, SETTINGS_VERSION);
        assert!(warning.is_none(), "an old file is not a problem: {warning:?}");
        let note = removed_keys_note(&p).expect("the removed keys are named");
        for k in ["engine", "active_model", "inference_build", "models_dir", "name_aliases"] {
            assert!(note.contains(k), "{k} not named in: {note}");
        }
        assert!(!note.contains("context_length"), "kept keys are not named: {note}");
        s.save(&p).unwrap();
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        let mut keys: Vec<&str> = saved.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["context_length", "max_tokens", "temperature", "version"]);
        assert_eq!(removed_keys_note(&p), None, "nothing left to drop");
    }

    #[test]
    fn defaults_roundtrip_and_unknown_keys_are_ignored() {
        let s = AiSettings::default();
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<AiSettings>(&text).unwrap(), s);
        let p: AiSettings = serde_json::from_str("{\"max_tokens\": 300, \"future\": 1}").unwrap();
        assert!(p.max_tokens == 300 && p.context_length == 8192);
    }

    #[test]
    fn clamping() {
        let s = AiSettings {
            context_length: 10,
            max_tokens: 99_999,
            temperature: f32::NAN,
            ..AiSettings::default()
        }
        .clamp();
        assert_eq!((s.context_length, s.max_tokens), (1024, 512));
        assert_eq!(s.temperature, 0.2);
        assert_eq!(
            AiSettings {
                temperature: 9.0,
                ..AiSettings::default()
            }
            .clamp()
            .temperature,
            2.0
        );
    }

    #[test]
    fn load_and_save_tolerate_problems() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub/ai.json");
        assert_eq!(AiSettings::load(&p), (AiSettings::default(), None));
        let s = AiSettings {
            temperature: 0.7,
            ..AiSettings::default()
        };
        s.save(&p).unwrap();
        assert_eq!(AiSettings::load(&p), (s, None));
        std::fs::write(&p, "{ broken").unwrap();
        assert!(AiSettings::load(&p).1.unwrap().contains("damaged"));
        std::fs::write(&p, "{\"version\": 99}").unwrap();
        assert!(AiSettings::load(&p).1.unwrap().contains("newer"));
        assert!(AiSettings::load(dir.path()).1.unwrap().contains("cannot read"));
    }

    #[test]
    fn schema_describes_every_field() {
        let v = schema();
        let keys: Vec<&str> = v.as_array().unwrap().iter().map(|f| f["key"].as_str().unwrap()).collect();
        let settings = serde_json::to_value(AiSettings::default()).unwrap();
        for k in settings.as_object().unwrap().keys().filter(|k| *k != "version") {
            assert!(keys.contains(&k.as_str()), "{k} missing from the schema");
        }
        for f in v.as_array().unwrap() {
            for k in ["key", "label", "description", "type", "default", "restartRequired"] {
                assert!(f.get(k).is_some(), "{f}");
            }
        }
    }
}
