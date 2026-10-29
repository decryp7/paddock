//! Ids that survive renames: [`Fingerprint`], [`FileId`], [`SymbolId`] and [`ChunkId`] (ADR 0002).
//!
//! A [`SymbolId`] derives from the file's [`FileId`] and the symbol's identity (language, kind, qualified name,
//! disambiguator), never from the file's path. The store issues one `FileId` per file from a counter
//! ([`FileIdAllocator`]) and re-binds it to the new path when a rename is matched by [`Fingerprint`], so a renamed file
//! keeps every symbol id. The *qualified name* is the chain of containers inside the file (`Outer::Inner::method`),
//! never a module path derived from where the file lives (ADR 0002); a module path is a separate, mutable column.
//!
//! TODO(shared-later, ADR 0002 "Shared later"): [`Fingerprint`] is one of several content hashes in the family; a single
//! canonical one is chosen when shared code unfreezes.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::language::Language;

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
    /// The id for the `sequence`-th file the store has ever seen. The store issues sequence numbers from a persistent
    /// counter ([`FileIdAllocator`]), so two files never share an id, even when one is moved away and another file
    /// later appears at its old path with the same content. The id is opaque; it is never recomputed from a path.
    pub fn from_sequence(sequence: u64) -> FileId {
        let mut f = Fields::new("pacenotes/file/v2");
        f.bytes(&sequence.to_le_bytes());
        FileId(f.finish16())
    }
}

/// Issues [`FileId`]s from a counter. The store persists [`FileIdAllocator::next_sequence`] with its data and restores it
/// with [`FileIdAllocator::resume`]; ids are never reused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIdAllocator {
    next: u64,
}

impl FileIdAllocator {
    /// A new allocator whose first id is sequence 0.
    pub fn new() -> FileIdAllocator {
        FileIdAllocator { next: 0 }
    }

    /// An allocator that continues from a stored `next_sequence`.
    pub fn resume(next_sequence: u64) -> FileIdAllocator {
        FileIdAllocator { next: next_sequence }
    }

    /// The id for a file seen for the first time.
    pub fn issue(&mut self) -> FileId {
        let id = FileId::from_sequence(self.next);
        self.next = self.next.saturating_add(1);
        id
    }

    /// The counter to store.
    pub fn next_sequence(&self) -> u64 {
        self.next
    }
}

impl Default for FileIdAllocator {
    fn default() -> Self {
        FileIdAllocator::new()
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

    fn rust() -> Language {
        Language::Known(KnownLanguage::Rust)
    }

    /// What this proves: derivation is deterministic and has no path input, so a store that keeps the stored `FileId` on
    /// a rename keeps every symbol id. That the store does keep it (a rename matched by fingerprint) is A3a's test.
    #[test]
    fn symbol_ids_are_deterministic_and_take_no_path() {
        let file = FileIdAllocator::new().issue();
        let a = SymbolId::derive(file, &rust(), SymbolKind::Function, "Outer::parse", 0);
        let b = SymbolId::derive(file, &rust(), SymbolKind::Function, "Outer::parse", 0);
        assert_eq!(a, b);
        assert_eq!(
            Fingerprint::of_content(b"pub fn parse() {}\n"),
            Fingerprint::of_content(b"pub fn parse() {}\r\n")
        );
    }

    /// `git mv a b`, then a new file appears at `a` with the same content: the old scheme (path + fingerprint) gave both
    /// the same id. Issued ids cannot collide.
    #[test]
    fn moving_a_file_away_and_restoring_the_path_gives_two_different_ids() {
        let mut ids = FileIdAllocator::new();
        let at_a = ids.issue();
        // a moves to b: the store keeps `at_a` for the file now at b. A new file at a gets a new id.
        let restored_a = ids.issue();
        assert_ne!(at_a, restored_a);
        let sym = |f| SymbolId::derive(f, &rust(), SymbolKind::Function, "parse", 0);
        assert_ne!(sym(at_a), sym(restored_a));
    }

    #[test]
    fn the_allocator_resumes_without_reusing_an_id() {
        let mut first = FileIdAllocator::new();
        let ids: Vec<_> = (0..3).map(|_| first.issue()).collect();
        let mut again = FileIdAllocator::resume(first.next_sequence());
        let next = again.issue();
        assert!(!ids.contains(&next));
        assert_eq!(FileIdAllocator::default().next_sequence(), 0);
        assert_eq!(FileId::from_sequence(1), ids[1]);
    }

    #[test]
    fn a_changed_identity_is_a_new_id() {
        let file = FileId::from_sequence(1);
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
        let other_file = FileId::from_sequence(3);
        assert_ne!(base, SymbolId::derive(other_file, &rust(), SymbolKind::Function, "a::f", 0));
    }

    #[test]
    fn fields_cannot_be_shifted_into_each_other() {
        let file = FileId::from_sequence(2);
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
        let file = FileId::from_sequence(1);
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
