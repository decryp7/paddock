//! Identifiers parsed once at the edge: model ids and app ids. Each is a newtype over a validated string, so code
//! that holds one never re-checks it.

use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

/// Why an identifier was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct IdError(String);

fn check(raw: &str, max: usize, what: &str, extra: &[char]) -> Result<String, IdError> {
    let s = raw.trim().to_ascii_lowercase();
    let ok = !s.is_empty()
        && s.len() <= max
        && s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || extra.contains(&c))
        && !s.contains("..");
    if ok {
        Ok(s)
    } else {
        let shown: String = raw.chars().take(80).collect();
        let punct: String = extra.iter().collect();
        Err(IdError(format!(
            "{shown:?} is not a valid {what}: use up to {max} letters, digits and the characters {punct:?}, starting with a letter or digit"
        )))
    }
}

macro_rules! slug_id {
    ($(#[$doc:meta])* $name:ident, $max:expr, $what:literal, $extra:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Parses an id: trimmed and lowercased; letters, digits and the allowed punctuation.
            pub fn parse(raw: &str) -> Result<Self, IdError> {
                check(raw, $max, $what, $extra).map(Self)
            }

            /// The id as text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                Self::parse(&s).map_err(serde::de::Error::custom)
            }
        }
    };
}

slug_id!(
    /// A model in the library, e.g. `qwen3-8b-instruct-q4_k_m`: its folder name on disk and the `model` clients send.
    ModelId,
    96,
    "model id",
    &['-', '_', '.', ':']
);

slug_id!(
    /// An app that talks to Pitwall, e.g. `imt`, `pitcrew`, `continue`.
    AppId,
    32,
    "app id",
    &['-', '_']
);

impl ModelId {
    /// A model id from a GGUF file name: the stem without the split suffix (`-00001-of-00003`), lowercased, every
    /// other character mapped to `-`.
    pub fn from_file_name(file: &str) -> Result<ModelId, IdError> {
        let base = file.rsplit(['/', '\\']).next().unwrap_or(file);
        let stem = base.strip_suffix(".gguf").unwrap_or(base);
        let stem = strip_split_suffix(stem);
        let mapped: String = stem
            .chars()
            .map(|c| c.to_ascii_lowercase())
            .map(|c| {
                if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.') {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        let mut cleaned: String = mapped.trim_matches(|c: char| !c.is_ascii_alphanumeric()).chars().take(96).collect();
        while cleaned.contains("..") {
            cleaned = cleaned.replace("..", ".");
        }
        ModelId::parse(&cleaned)
    }
}

impl AppId {
    /// An app id from a display name (`"Continue (VS Code)"` → `continue-vs-code`).
    pub fn from_name(name: &str) -> Result<AppId, IdError> {
        let mut out = String::new();
        for c in name.chars().map(|c| c.to_ascii_lowercase()) {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                out.push(c);
            } else if !out.ends_with('-') && !out.is_empty() {
                out.push('-');
            }
        }
        let out: String = out.trim_end_matches('-').chars().take(32).collect();
        AppId::parse(out.trim_end_matches('-'))
    }
}

/// `name-00001-of-00003` → `name`; anything else unchanged.
pub fn strip_split_suffix(stem: &str) -> &str {
    split_part(stem).map_or(stem, |(base, _, _)| base)
}

/// `(base, part, total)` of a split GGUF stem like `name-00002-of-00005`.
pub fn split_part(stem: &str) -> Option<(&str, u32, u32)> {
    let (rest, total) = stem.rsplit_once("-of-")?;
    let (base, part) = rest.rsplit_once('-')?;
    let digits = |s: &str| s.len() == 5 && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(part) || !digits(total) {
        return None;
    }
    let (p, t): (u32, u32) = (part.parse().ok()?, total.parse().ok()?);
    (p >= 1 && p <= t).then_some((base, p, t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_parsed_once() {
        assert_eq!(AppId::parse(" IMT ").unwrap().as_str(), "imt");
        assert_eq!(AppId::parse("continue-vscode").unwrap().to_string(), "continue-vscode");
        for bad in ["", "-x", "a b", "a/b", &"a".repeat(33), "ä"] {
            assert!(AppId::parse(bad).is_err(), "{bad}");
        }
        assert!(ModelId::parse("qwen3:8b").is_ok());
        assert!(ModelId::parse("a..b").is_err());
        let e = ModelId::parse("../etc").unwrap_err().to_string();
        assert!(e.contains("model id"), "{e}");
        let id: ModelId = serde_json::from_str("\"Qwen3-8B\"").unwrap();
        assert_eq!(id.as_str(), "qwen3-8b");
        assert_eq!(id.to_string(), "qwen3-8b");
        assert!(serde_json::from_str::<ModelId>("\"bad id\"").is_err());
        assert!(serde_json::from_str::<ModelId>("5").is_err());
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"qwen3-8b\"");
    }

    #[test]
    fn model_ids_from_files() {
        assert_eq!(ModelId::from_file_name("Qwen3-8B-Q4_K_M.gguf").unwrap().as_str(), "qwen3-8b-q4_k_m");
        assert_eq!(
            ModelId::from_file_name("dir/Big Model-00001-of-00003.gguf").unwrap().as_str(),
            "big-model"
        );
        assert_eq!(ModelId::from_file_name("x...y.gguf").unwrap().as_str(), "x.y");
        assert!(ModelId::from_file_name("---.gguf").is_err());
    }

    #[test]
    fn app_ids_from_names() {
        assert_eq!(AppId::from_name("Continue (VS Code)").unwrap().as_str(), "continue-vs-code");
        assert_eq!(AppId::from_name("IMT").unwrap().as_str(), "imt");
        assert!(AppId::from_name("!!!").is_err());
    }

    #[test]
    fn split_parts() {
        assert_eq!(split_part("m-00002-of-00005"), Some(("m", 2, 5)));
        assert_eq!(split_part("m-00006-of-00005"), None);
        assert_eq!(split_part("m-2-of-5"), None);
        assert_eq!(split_part("m-00002-of-0005x"), None);
        assert_eq!(split_part("m"), None);
        assert_eq!(split_part("nodash-of-00005"), None);
        assert_eq!(strip_split_suffix("m-00001-of-00001"), "m");
    }
}
