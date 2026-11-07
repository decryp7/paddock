//! The vocabulary: ids, quantities with units, scopes and Setting paths.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::sync::Mutex;

/// The longest `LimitId` or `SettingPath` text.
const MAX_KEY: usize = 64;
/// How many distinct ids deserializing may intern. A registry has a few hundred rows; the bound keeps hostile input from
/// growing memory.
const MAX_INTERNED: usize = 4096;

/// Whether `s` is a dotted, lower-case key: parts of `[a-z][a-z0-9_]*` joined by `.`, at most 64 bytes.
const fn is_key(s: &str, camel: bool) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > MAX_KEY {
        return false;
    }
    let mut at_start = true;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'.' {
            if at_start {
                return false;
            }
            at_start = true;
        } else if at_start {
            if !c.is_ascii_lowercase() {
                return false;
            }
            at_start = false;
        } else if !(c.is_ascii_lowercase()
            || c.is_ascii_digit()
            || c == b'_'
            || (camel && c.is_ascii_uppercase()))
        {
            return false;
        }
        i += 1;
    }
    !at_start
}

/// A stable key for one limit: dotted, lower-case, such as `"import.record_bytes"` or `"plugin.memory"`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LimitId(&'static str);

impl LimitId {
    /// A key from a literal. In a `const` or `static` a bad key fails the build.
    ///
    /// # Panics
    /// If `key` is not dotted lower-case (`[a-z][a-z0-9_]*` parts, at most 64 bytes). Keys are literals chosen by the
    /// programmer, never input.
    #[must_use]
    pub const fn new(key: &'static str) -> Self {
        assert!(
            is_key(key, false),
            "a LimitId is dotted lower-case: parts of [a-z][a-z0-9_]*, at most 64 bytes"
        );
        Self(key)
    }

    /// The key.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }

    /// A key read from the wire. Registry ids are `'static`; a key from outside is validated and interned once
    /// (bounded at 4 096 distinct keys), so equal keys compare equal.
    ///
    /// # Errors
    /// [`BadKey`] for a malformed key or when the intern table is full.
    pub fn parse(key: &str) -> Result<Self, BadKey> {
        if !is_key(key, false) {
            return Err(BadKey::Syntax);
        }
        static TABLE: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
        let mut guard = TABLE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let table = guard.get_or_insert_with(HashSet::new);
        if let Some(found) = table.get(key) {
            return Ok(Self(found));
        }
        if table.len() >= MAX_INTERNED {
            return Err(BadKey::Full);
        }
        let leaked: &'static str = Box::leak(key.to_owned().into_boxed_str());
        table.insert(leaked);
        Ok(Self(leaked))
    }
}

impl fmt::Debug for LimitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LimitId({})", self.0)
    }
}

impl fmt::Display for LimitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl Serialize for LimitId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.0)
    }
}

impl<'de> Deserialize<'de> for LimitId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "ts")]
impl ts_rs::TS for LimitId {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;
    fn name() -> String {
        "LimitId".into()
    }
    fn inline() -> String {
        "string".into()
    }
    fn inline_flattened() -> String {
        "string".into()
    }
    fn decl() -> String {
        "type LimitId = string;".into()
    }
    fn decl_concrete() -> String {
        "type LimitId = string;".into()
    }
}

/// Why a key was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadKey {
    /// Not dotted lower-case, empty or longer than 64 bytes.
    Syntax,
    /// 4 096 distinct keys are already known.
    Full,
}

impl fmt::Display for BadKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax => {
                f.write_str("a limit key is dotted lower-case (a-z, 0-9, _) and at most 64 bytes")
            }
            Self::Full => f.write_str("too many distinct limit keys"),
        }
    }
}

impl std::error::Error for BadKey {}

/// The path of a Setting that raises a limit, as the Settings schema names it (`"performance.pluginMemoryMb"`): dotted
/// parts of letters, digits and `_`, starting with a lower-case letter, at most 64 bytes. Registry tests resolve it
/// against the schema.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SettingPath(&'static str);

impl SettingPath {
    /// A path from a literal; a bad one fails the build in a `const`.
    ///
    /// # Panics
    /// If `path` is malformed (see the type).
    #[must_use]
    pub const fn new(path: &'static str) -> Self {
        assert!(
            is_key(path, true),
            "a SettingPath is dotted: parts of [a-z][a-zA-Z0-9_]*, at most 64 bytes"
        );
        Self(path)
    }

    /// The path.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl Serialize for SettingPath {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.0)
    }
}

impl fmt::Debug for SettingPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SettingPath({})", self.0)
    }
}

impl fmt::Display for SettingPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

#[cfg(feature = "ts")]
impl ts_rs::TS for SettingPath {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;
    fn name() -> String {
        "SettingPath".into()
    }
    fn inline() -> String {
        "string".into()
    }
    fn inline_flattened() -> String {
        "string".into()
    }
    fn decl() -> String {
        "type SettingPath = string;".into()
    }
    fn decl_concrete() -> String {
        "type SettingPath = string;".into()
    }
}

/// The unit of a limit's [`Quantity`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Unit {
    /// Bytes.
    Bytes,
    /// A number of things.
    Count,
    /// A ratio, `n` to 1.
    Ratio,
    /// Milliseconds.
    Millis,
    /// A percentage, 0 to 100.
    Percent,
}

/// A value with its unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "n", rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Quantity {
    /// Bytes.
    Bytes(#[cfg_attr(feature = "ts", ts(type = "number"))] u64),
    /// A count.
    Count(#[cfg_attr(feature = "ts", ts(type = "number"))] u64),
    /// `n` to 1 (an archive's expansion ratio).
    Ratio(u32),
    /// Milliseconds.
    Millis(#[cfg_attr(feature = "ts", ts(type = "number"))] u64),
    /// A percentage, 0 to 100.
    Percent(u8),
}

impl Quantity {
    /// The unit.
    #[must_use]
    pub const fn unit(self) -> Unit {
        match self {
            Self::Bytes(_) => Unit::Bytes,
            Self::Count(_) => Unit::Count,
            Self::Ratio(_) => Unit::Ratio,
            Self::Millis(_) => Unit::Millis,
            Self::Percent(_) => Unit::Percent,
        }
    }

    /// The number, widened.
    #[must_use]
    pub const fn amount(self) -> u64 {
        match self {
            Self::Bytes(n) | Self::Count(n) | Self::Millis(n) => n,
            Self::Ratio(n) => n as u64,
            Self::Percent(n) => n as u64,
        }
    }

    /// Whether `self` and `other` have the same unit and `self <= other`.
    #[must_use]
    pub const fn at_most(self, other: Self) -> bool {
        self.unit_eq(other) && self.amount() <= other.amount()
    }

    /// Whether both have the same unit.
    #[must_use]
    pub const fn unit_eq(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Bytes(_), Self::Bytes(_))
                | (Self::Count(_), Self::Count(_))
                | (Self::Ratio(_), Self::Ratio(_))
                | (Self::Millis(_), Self::Millis(_))
                | (Self::Percent(_), Self::Percent(_))
        )
    }
}

/// Digits with a thin grouping by commas: `1204` becomes `1,204`.
pub(crate) fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

impl fmt::Display for Quantity {
    /// The text a person reads: `1,024 MiB`, `108 s`, `10,000`, `200:1`, `80 %`. Bytes use the largest binary unit that
    /// divides the value exactly, so a limit reads as it was written.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Bytes(n) => {
                const UNITS: [(&str, u64); 4] = [
                    ("GiB", 1 << 30),
                    ("MiB", 1 << 20),
                    ("KiB", 1 << 10),
                    ("B", 1),
                ];
                let (name, size) = UNITS
                    .iter()
                    .copied()
                    .find(|&(_, size)| n >= size && n.is_multiple_of(size))
                    .unwrap_or(("B", 1));
                if n == 0 {
                    f.write_str("0 B")
                } else {
                    write!(f, "{} {name}", grouped(n / size))
                }
            }
            Self::Count(n) => f.write_str(&grouped(n)),
            Self::Ratio(n) => write!(f, "{}:1", grouped(u64::from(n))),
            Self::Millis(n) if n >= 1000 && n.is_multiple_of(1000) => {
                write!(f, "{} s", grouped(n / 1000))
            }
            Self::Millis(n) => write!(f, "{} ms", grouped(n)),
            Self::Percent(n) => write!(f, "{n} %"),
        }
    }
}

/// What a limit applied to when it was hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Scope {
    /// One file.
    File,
    /// One source of events.
    Source,
    /// One import.
    Import,
    /// One plugin.
    Plugin,
    /// One query.
    Query,
    /// A whole process.
    Process,
}

impl Scope {
    /// The word used in messages.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Source => "source",
            Self::Import => "import",
            Self::Plugin => "plugin",
            Self::Query => "query",
            Self::Process => "process",
        }
    }
}
