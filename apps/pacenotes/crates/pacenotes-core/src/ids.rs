//! Ids that survive renames: [`Fingerprint`], [`FileId`], [`SymbolId`] and [`ChunkId`] (ADR 0002).
//!
//! A [`SymbolId`] derives from the file's [`FileId`] and the symbol's identity (language, kind, qualified name,
//! disambiguator), never from the file's path. The index keeps one `FileId` per file for the file's whole life and
//! re-binds it to the new path when a rename is matched by [`Fingerprint`], so a renamed file keeps every symbol id.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::language::Language;
use crate::path::RepoPath;

/// Why a string is not an id.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdError {
    /// The prefix or the digits are wrong.
    #[error("expected `{prefix}` followed by {digits} hexadecimal digits, got {got:?}")]
    Malformed {
        /// The expected prefix.
        prefix: &'static str,
        /// The expected number of hex digits.
        digits: usize,
        /// What was given.
        got: String,
    },
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn from_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != N * 2 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// A hasher that length-prefixes every field, so `("ab", "c")` and `("a", "bc")` hash differently.
struct Fields(Sha256);

impl Fields {
    fn new(domain: &str) -> Fields {
        let mut f = Fields(Sha256::new());
        f.text(domain);
        f
    }

    fn text(&mut self, s: &str) -> &mut Self {
        self.bytes(s.as_bytes())
    }

    fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.0.update((b.len() as u64).to_le_bytes());
        self.0.update(b);
        self
    }

    fn finish16(self) -> [u8; 16] {
        let full = self.0.finalize();
        let mut out = [0u8; 16];
        out.copy_from_slice(&full[..16]);
        out
    }
}

/// SHA-256 of a file's content with line endings normalised to `\n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    /// The fingerprint of `content`; `\r\n` and lone `\r` count as `\n`, so a checkout with other line endings matches.
    pub fn of_content(content: &[u8]) -> Fingerprint {
        let mut hasher = Sha256::new();
        let mut i = 0;
        let mut start = 0;
        while i < content.len() {
            if content[i] == b'\r' {
                hasher.update(&content[start..i]);
                hasher.update(b"\n");
                if content.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
                start = i + 1;
            }
            i += 1;
        }
        hasher.update(&content[start..]);
        Fingerprint(hasher.finalize().into())
    }

    /// The fingerprint as 64 lowercase hex digits.
    pub fn to_hex(&self) -> String {
        to_hex(&self.0)
    }

    /// Parses 64 hex digits.
    pub fn from_hex(text: &str) -> Option<Fingerprint> {
        from_hex::<32>(text).map(Fingerprint)
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident, $prefix:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name([u8; 16]);

        impl $name {
            /// The id from its 16 raw bytes (for storage that keeps them).
            pub fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(bytes)
            }

            /// The 16 raw bytes.
            pub fn to_bytes(self) -> [u8; 16] {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}{}", $prefix, to_hex(&self.0))
            }
        }

        impl FromStr for $name {
            type Err = IdError;
            fn from_str(s: &str) -> Result<Self, IdError> {
                s.strip_prefix($prefix)
                    .and_then(from_hex::<16>)
                    .map(Self)
                    .ok_or_else(|| IdError::Malformed { prefix: $prefix, digits: 32, got: s.chars().take(80).collect() })
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

id_type!(
    /// The identity of a file for its whole life in the index. It is minted once, when the index first sees the file,
    /// and re-bound to the new path on a rename matched by [`Fingerprint`]; it is never recomputed from the path.
    FileId,
    "file:"
);
id_type!(
    /// The identity of a symbol: derived from its file's [`FileId`] and its own identity, never from a path or an order.
    SymbolId,
    "sym:"
);
id_type!(
    /// The identity of a text chunk used for search.
    ChunkId,
    "chk:"
);

impl FileId {
    /// Mints the id of a file first seen at `path` with `fingerprint`. Call it once per new file and store the result;
    /// a renamed file keeps the stored id.
    pub fn mint(path: &RepoPath, fingerprint: &Fingerprint) -> FileId {
        let mut f = Fields::new("pacenotes/file/v1");
        f.text(path.as_str()).bytes(&fingerprint.0);
        FileId(f.finish16())
    }
}

/// The kind of a symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    /// A module, package or namespace.
    Module,
    /// A class.
    Class,
    /// A struct or record.
    Struct,
    /// An interface or protocol.
    Interface,
    /// A trait.
    Trait,
    /// An enum.
    Enum,
    /// A free function.
    Function,
    /// A method.
    Method,
    /// A constructor.
    Constructor,
    /// A field.
    Field,
    /// A property.
    Property,
    /// A variable.
    Variable,
    /// A constant.
    Constant,
    /// A type alias.
    TypeAlias,
    /// A macro.
    Macro,
    /// Anything else the grammar names.
    Other,
}

impl SymbolKind {
    /// A stable name, part of the id derivation: renaming a variant changes every id of that kind.
    pub fn name(self) -> &'static str {
        match self {
            SymbolKind::Module => "module",
            SymbolKind::Class => "class",
            SymbolKind::Struct => "struct",
            SymbolKind::Interface => "interface",
            SymbolKind::Trait => "trait",
            SymbolKind::Enum => "enum",
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Constructor => "constructor",
            SymbolKind::Field => "field",
            SymbolKind::Property => "property",
            SymbolKind::Variable => "variable",
            SymbolKind::Constant => "constant",
            SymbolKind::TypeAlias => "type_alias",
            SymbolKind::Macro => "macro",
            SymbolKind::Other => "other",
        }
    }
}

impl SymbolId {
    /// The id of a symbol. `disambiguator` tells apart symbols that share a kind and a qualified name in one file
    /// (overloads, a function defined twice under `#ifdef`); use 0 for the first.
    pub fn derive(file: FileId, language: &Language, kind: SymbolKind, qualified_name: &str, disambiguator: u32) -> SymbolId {
        let mut f = Fields::new("pacenotes/symbol/v1");
        f.bytes(&file.0)
            .text(language.id())
            .text(kind.name())
            .text(qualified_name)
            .bytes(&disambiguator.to_le_bytes());
        SymbolId(f.finish16())
    }
}

impl ChunkId {
    /// The id of the `ordinal`-th chunk of a file, or of a symbol when the chunk is that symbol's text.
    pub fn derive(file: FileId, within: Option<SymbolId>, ordinal: u32) -> ChunkId {
        let mut f = Fields::new("pacenotes/chunk/v1");
        f.bytes(&file.0)
            .bytes(&within.map_or([0u8; 16], |s| s.0))
            .bytes(&ordinal.to_le_bytes());
        ChunkId(f.finish16())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::KnownLanguage;

    fn path(p: &str) -> RepoPath {
        RepoPath::parse(p).unwrap()
    }

    fn rust() -> Language {
        Language::Known(KnownLanguage::Rust)
    }

    /// The regression test of the plan (A1): ids stay stable after a rename matched by fingerprint.
    #[test]
    fn symbol_ids_survive_a_rename_matched_by_fingerprint() {
        let content = b"pub fn parse() {}\n";
        let fp_before = Fingerprint::of_content(content);
        let file = FileId::mint(&path("src/old_name.rs"), &fp_before);
        let before = SymbolId::derive(file, &rust(), SymbolKind::Function, "crate::parse", 0);

        // The file moves; its content is identical, so its fingerprint matches and the index keeps the stored FileId.
        let fp_after = Fingerprint::of_content(b"pub fn parse() {}\r\n");
        assert_eq!(fp_before, fp_after, "line endings do not change the fingerprint");
        let after = SymbolId::derive(file, &rust(), SymbolKind::Function, "crate::parse", 0);
        assert_eq!(before, after);
    }

    #[test]
    fn a_changed_identity_is_a_new_id() {
        let file = FileId::mint(&path("a.rs"), &Fingerprint::of_content(b"x"));
        let base = SymbolId::derive(file, &rust(), SymbolKind::Function, "a::f", 0);
        assert_ne!(
            base,
            SymbolId::derive(file, &rust(), SymbolKind::Function, "a::g", 0),
            "renamed symbol"
        );
        assert_ne!(base, SymbolId::derive(file, &rust(), SymbolKind::Method, "a::f", 0), "other kind");
        assert_ne!(base, SymbolId::derive(file, &rust(), SymbolKind::Function, "a::f", 1), "overload");
        assert_ne!(
            base,
            SymbolId::derive(file, &Language::Known(KnownLanguage::C), SymbolKind::Function, "a::f", 0)
        );
        let other_file = FileId::mint(&path("b.rs"), &Fingerprint::of_content(b"y"));
        assert_ne!(base, SymbolId::derive(other_file, &rust(), SymbolKind::Function, "a::f", 0));
    }

    #[test]
    fn fields_cannot_be_shifted_into_each_other() {
        let file = FileId::mint(&path("a.rs"), &Fingerprint::of_content(b""));
        let a = SymbolId::derive(file, &Language::parse("zig").unwrap(), SymbolKind::Other, "x", 0);
        let b = SymbolId::derive(file, &Language::parse("zi").unwrap(), SymbolKind::Other, "gx", 0);
        assert_ne!(a, b);
    }

    #[test]
    fn fingerprints_normalise_line_endings_only() {
        let lf = Fingerprint::of_content(b"a\nb\n");
        assert_eq!(lf, Fingerprint::of_content(b"a\r\nb\r\n"));
        assert_eq!(lf, Fingerprint::of_content(b"a\rb\r"));
        assert_ne!(lf, Fingerprint::of_content(b"a\nb"));
        assert_ne!(lf, Fingerprint::of_content(b"a\n\nb\n"));
        assert_eq!(Fingerprint::from_hex(&lf.to_hex()), Some(lf));
        assert_eq!(Fingerprint::from_hex("zz"), None);
    }

    #[test]
    fn ids_display_parse_and_serialise_the_same_way() {
        let file = FileId::mint(&path("a.rs"), &Fingerprint::of_content(b"x"));
        let sym = SymbolId::derive(file, &rust(), SymbolKind::Class, "A", 0);
        let chunk = ChunkId::derive(file, Some(sym), 3);
        assert!(file.to_string().starts_with("file:") && sym.to_string().starts_with("sym:") && chunk.to_string().starts_with("chk:"));
        assert_eq!(sym.to_string().len(), 4 + 32);
        assert_eq!(sym.to_string().parse::<SymbolId>().unwrap(), sym);
        assert_eq!(
            serde_json::from_str::<ChunkId>(&serde_json::to_string(&chunk).unwrap()).unwrap(),
            chunk
        );
        assert_ne!(chunk, ChunkId::derive(file, None, 3));
        assert_ne!(chunk, ChunkId::derive(file, Some(sym), 4));
        assert_eq!(SymbolId::from_bytes(sym.to_bytes()), sym);
    }

    #[test]
    fn malformed_ids_are_refused() {
        for bad in [
            "",
            "sym:",
            "sym:zz",
            "chk:0123456789abcdef0123456789abcdef",
            &format!("sym:{}", "0".repeat(33)),
        ] {
            assert!("sym:00".parse::<SymbolId>().is_err());
            assert!(bad.parse::<SymbolId>().is_err(), "{bad}");
        }
        assert!(serde_json::from_str::<SymbolId>("\"nope\"").is_err());
    }

    #[test]
    fn display_and_names_cover_every_kind() {
        let fp = Fingerprint::of_content(b"x");
        assert_eq!(fp.to_string(), fp.to_hex());
        use SymbolKind::*;
        let kinds = [
            Module,
            Class,
            Struct,
            Interface,
            Trait,
            Enum,
            Function,
            Method,
            Constructor,
            Field,
            Property,
            Variable,
            Constant,
            TypeAlias,
            Macro,
            Other,
        ];
        let names: std::collections::BTreeSet<_> = kinds.iter().map(|k| k.name()).collect();
        assert_eq!(names.len(), kinds.len(), "every kind has its own name");
        assert_eq!(serde_json::to_string(&TypeAlias).unwrap(), "\"type_alias\"");
    }
}
