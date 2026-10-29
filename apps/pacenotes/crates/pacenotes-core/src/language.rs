//! [`Language`]: the languages Pacenotes knows by name, and a validated id for those a pack adds.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Why a string is not a language.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LanguageError {
    /// The id does not match `[a-z][a-z0-9+#-]{0,31}` after lowering.
    #[error("{0:?} is not a language id (lowercase letters, digits, `+`, `#` or `-`, starting with a letter, at most 32 characters)")]
    BadId(String),
    /// The id is one of the languages Pacenotes knows; use [`Language::parse`], which gives the known one.
    #[error("{0:?} is a language Pacenotes already knows; it cannot be added as another")]
    KnownAlias(String),
}

/// The languages compiled in (proposal 7.1): the eight tier-3 languages, Kotlin, and the config and data formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KnownLanguage {
    /// C.
    C,
    /// C++.
    Cpp,
    /// C#.
    CSharp,
    /// Go.
    Go,
    /// Java.
    Java,
    /// JavaScript, including JSX.
    JavaScript,
    /// Kotlin.
    Kotlin,
    /// Python.
    Python,
    /// Rust.
    Rust,
    /// TypeScript, including TSX.
    TypeScript,
    /// JSON.
    Json,
    /// YAML.
    Yaml,
    /// TOML.
    Toml,
    /// XML, including project files.
    Xml,
    /// INI.
    Ini,
    /// SQL.
    Sql,
    /// Markdown.
    Markdown,
}

impl KnownLanguage {
    /// Every known language, in a fixed order.
    pub const ALL: [KnownLanguage; 17] = [
        KnownLanguage::C,
        KnownLanguage::Cpp,
        KnownLanguage::CSharp,
        KnownLanguage::Go,
        KnownLanguage::Java,
        KnownLanguage::JavaScript,
        KnownLanguage::Kotlin,
        KnownLanguage::Python,
        KnownLanguage::Rust,
        KnownLanguage::TypeScript,
        KnownLanguage::Json,
        KnownLanguage::Yaml,
        KnownLanguage::Toml,
        KnownLanguage::Xml,
        KnownLanguage::Ini,
        KnownLanguage::Sql,
        KnownLanguage::Markdown,
    ];

    /// The canonical id (`cpp`, `csharp`, ...).
    pub fn id(self) -> &'static str {
        match self {
            KnownLanguage::C => "c",
            KnownLanguage::Cpp => "cpp",
            KnownLanguage::CSharp => "csharp",
            KnownLanguage::Go => "go",
            KnownLanguage::Java => "java",
            KnownLanguage::JavaScript => "javascript",
            KnownLanguage::Kotlin => "kotlin",
            KnownLanguage::Python => "python",
            KnownLanguage::Rust => "rust",
            KnownLanguage::TypeScript => "typescript",
            KnownLanguage::Json => "json",
            KnownLanguage::Yaml => "yaml",
            KnownLanguage::Toml => "toml",
            KnownLanguage::Xml => "xml",
            KnownLanguage::Ini => "ini",
            KnownLanguage::Sql => "sql",
            KnownLanguage::Markdown => "markdown",
        }
    }

    /// The language for a canonical id or a common alias (`c++`, `c#`, `cs`, `js`, `ts`, `py`, `rs`, `golang`, `md`, `yml`).
    pub fn from_alias(text: &str) -> Option<KnownLanguage> {
        Some(match text {
            "c" => KnownLanguage::C,
            "cpp" | "c++" | "cxx" => KnownLanguage::Cpp,
            "csharp" | "c#" | "cs" => KnownLanguage::CSharp,
            "go" | "golang" => KnownLanguage::Go,
            "java" => KnownLanguage::Java,
            "javascript" | "js" => KnownLanguage::JavaScript,
            "kotlin" | "kt" => KnownLanguage::Kotlin,
            "python" | "py" => KnownLanguage::Python,
            "rust" | "rs" => KnownLanguage::Rust,
            "typescript" | "ts" => KnownLanguage::TypeScript,
            "json" => KnownLanguage::Json,
            "yaml" | "yml" => KnownLanguage::Yaml,
            "toml" => KnownLanguage::Toml,
            "xml" => KnownLanguage::Xml,
            "ini" => KnownLanguage::Ini,
            "sql" => KnownLanguage::Sql,
            "markdown" | "md" => KnownLanguage::Markdown,
            _ => return None,
        })
    }
}

/// A validated id of a language that only a pack adds.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LanguageId(String);

impl LanguageId {
    /// Parses and lowers `text`. A name or alias of a [`KnownLanguage`] is refused, so one language has one value.
    pub fn parse(text: &str) -> Result<LanguageId, LanguageError> {
        let id = text.to_ascii_lowercase();
        if KnownLanguage::from_alias(&id).is_some() {
            return Err(LanguageError::KnownAlias(text.to_owned()));
        }
        let mut chars = id.chars();
        let ok = id.len() <= 32
            && chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '+' | '#' | '-'));
        if ok {
            Ok(LanguageId(id))
        } else {
            Err(LanguageError::BadId(text.to_owned()))
        }
    }

    /// The id.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A language: one Pacenotes knows by name, or one a pack adds.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Language {
    /// Compiled in.
    Known(KnownLanguage),
    /// Added by a pack.
    Other(LanguageId),
}

impl Language {
    /// Parses an id or alias; `"C++"` and `"cpp"` give the same value.
    pub fn parse(text: &str) -> Result<Language, LanguageError> {
        let lower = text.to_ascii_lowercase();
        if let Some(k) = KnownLanguage::from_alias(&lower) {
            return Ok(Language::Known(k));
        }
        LanguageId::parse(&lower).map(Language::Other)
    }

    /// The canonical id.
    pub fn id(&self) -> &str {
        match self {
            Language::Known(k) => k.id(),
            Language::Other(o) => o.as_str(),
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

impl Serialize for Language {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.id())
    }
}

impl<'de> Deserialize<'de> for Language {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Language::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_give_one_value() {
        for (a, b) in [
            ("C++", "cpp"),
            ("c#", "csharp"),
            ("CS", "csharp"),
            ("Golang", "go"),
            ("YML", "yaml"),
            ("md", "markdown"),
        ] {
            assert_eq!(Language::parse(a).unwrap(), Language::parse(b).unwrap(), "{a}");
        }
    }

    #[test]
    fn every_known_language_round_trips_through_its_id() {
        for k in KnownLanguage::ALL {
            assert_eq!(Language::parse(k.id()).unwrap(), Language::Known(k));
        }
    }

    #[test]
    fn pack_languages_are_validated_ids() {
        assert_eq!(Language::parse("Zig").unwrap().id(), "zig");
        assert!(Language::parse("").is_err());
        assert!(Language::parse("1st").is_err());
        assert!(Language::parse("has space").is_err());
        assert!(Language::parse(&"a".repeat(33)).is_err());
        assert_eq!(LanguageId::parse("cpp"), Err(LanguageError::KnownAlias("cpp".into())));
        assert!(LanguageId::parse("c#").is_err());
    }

    #[test]
    fn serde_uses_the_id_and_validates() {
        let l: Language = serde_json::from_str("\"C++\"").unwrap();
        assert_eq!(serde_json::to_string(&l).unwrap(), "\"cpp\"");
        assert!(serde_json::from_str::<Language>("\"no way\"").is_err());
    }

    #[test]
    fn display_is_the_id() {
        assert_eq!(Language::parse("c#").unwrap().to_string(), "csharp");
        assert_eq!(Language::parse("zig").unwrap().to_string(), "zig");
    }
}
