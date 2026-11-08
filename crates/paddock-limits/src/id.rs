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
const fn is_key(s: &str) -> bool {
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
        } else if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_') {
            return false;
        }
        i += 1;
    }
    !at_start
}

/// The longest [`SettingPath`] (`paddock_settings::schema::MAX_ID_LEN`).
const MAX_SETTING_PATH: usize = 128;

/// The grammar of `paddock_settings::valid_id`: 1 to 128 characters of `A-Z a-z 0-9 . _ -`.
const fn is_setting_path(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > MAX_SETTING_PATH {
        return false;
    }
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if !(c.is_ascii_alphanumeric() || c == b'.' || c == b'_' || c == b'-') {
            return false;
        }
        i += 1;
    }
    true
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
            is_key(key),
            "a LimitId is dotted lower-case: parts of [a-z][a-z0-9_]*, at most 64 bytes"
        );
        Self(key)
    }

    /// The key.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }

    /// A key read from the wire. A key of a registered registry ([`LimitId::register`]) resolves to its `'static` text and
    /// can never be refused; any other key is validated and interned once (bounded at 4 096 distinct unknown keys), so
    /// equal keys compare equal and a flood of junk keys cannot make a registered key undecodable.
    ///
    /// # Errors
    /// [`BadKey`] for a malformed key, or for an unknown key when the table of unknown keys is full.
    pub fn parse(key: &str) -> Result<Self, BadKey> {
        if !is_key(key) {
            return Err(BadKey::Syntax);
        }
        with_table(|t| t.parse(key)).map(Self)
    }

    /// Whether `key` is the id of a registered row (see [`LimitId::register`]): such an id always decodes, and is not
    /// counted against the bound on unknown ones.
    #[must_use]
    pub fn is_registered(key: &str) -> bool {
        with_table(|t| t.known.contains(key))
    }

    /// Makes the ids of `registry` known to [`LimitId::parse`] (idempotent). An app or crate calls it once at start-up, before
    /// it decodes anything; the built-in `bounded.cuts` row is always known.
    pub fn register(registry: &crate::Registry) {
        with_table(|t| {
            for row in registry.rows() {
                t.known.insert(row.id.0);
            }
        });
    }
}

/// The keys [`LimitId::parse`] resolves: the registered ones without a bound (they are `'static` rows), and the unknown
/// ones it interned, at most `cap`.
pub(crate) struct Interner {
    known: HashSet<&'static str>,
    unknown: HashSet<&'static str>,
    cap: usize,
    reported: bool,
}

impl Interner {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            known: HashSet::new(),
            unknown: HashSet::new(),
            cap,
            reported: false,
        }
    }

    pub(crate) fn parse(&mut self, key: &str) -> Result<&'static str, BadKey> {
        if let Some(found) = self.known.get(key).or_else(|| self.unknown.get(key)) {
            return Ok(found);
        }
        if self.unknown.len() >= self.cap {
            // Once: a flood is one problem, not one line per key. A registered key is unaffected.
            if !self.reported {
                self.reported = true;
                log::error!(
                    "paddock-limits: {} unknown limit ids are known; later unknown ids are refused (register the registries with LimitId::register at start-up)",
                    self.cap
                );
            }
            return Err(BadKey::Full);
        }
        let leaked: &'static str = Box::leak(key.to_owned().into_boxed_str());
        self.unknown.insert(leaked);
        Ok(leaked)
    }
}

fn with_table<R>(f: impl FnOnce(&mut Interner) -> R) -> R {
    static TABLE: Mutex<Option<Interner>> = Mutex::new(None);
    let mut guard = TABLE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let table = guard.get_or_insert_with(|| {
        let mut t = Interner::new(MAX_INTERNED);
        for row in crate::BUILTIN.rows() {
            t.known.insert(row.id.0);
        }
        t
    });
    f(table)
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

/// The path of a Setting that raises a limit, as the Settings schema names it (`"performance.pluginMemoryMb"`), with the
/// grammar of a Settings id. Registry tests resolve it against the schema.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SettingPath(&'static str);

impl SettingPath {
    /// A path from a literal; a bad one fails the build in a `const`.
    ///
    /// # Panics
    /// If `path` is not a setting path (see [`SettingPath::is_valid`]). Paths are literals, never input.
    #[must_use]
    pub const fn new(path: &'static str) -> Self {
        assert!(
            is_setting_path(path),
            "a SettingPath is 1 to 128 characters of A-Z, a-z, 0-9, '.', '_' or '-'"
        );
        Self(path)
    }

    /// Whether `path` has the grammar of a Setting id: the same as `paddock_settings::valid_id` (a test in that crate keeps
    /// them equal).
    #[must_use]
    pub const fn is_valid(path: &str) -> bool {
        is_setting_path(path)
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
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

/// The tag of a [`Quantity`] on the wire: the name in JSON, the variant index in a compact format (the derive reads both).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum Kind {
    Bytes,
    Count,
    Ratio,
    Millis,
    Percent,
}

impl<'de> Deserialize<'de> for Quantity {
    /// Reads `{"kind": "bytes", "n": 5}` from JSON, and a compact format's fields in order (MessagePack, postcard, bincode)
    /// with the number at the width the derived `Serialize` wrote (u32 for a ratio, u8 for a percentage, u64 otherwise). A
    /// percentage over 100 is refused, and so is a key that repeats. A sequence is also accepted from JSON (`["bytes", 5]`):
    /// compact formats write a struct as one, and telling them apart costs more than the harmless leniency.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{Error, IgnoredAny, MapAccess, SeqAccess, Visitor};

        fn percent<E: Error>(n: u64) -> Result<Quantity, E> {
            if n <= 100 {
                Ok(Quantity::Percent(n as u8))
            } else {
                Err(E::custom(format!("a percentage is 0 to 100, not {n}")))
            }
        }

        /// A number read before its kind was known (a map's keys come in any order).
        fn from_wide<E: Error>(kind: Kind, n: u64) -> Result<Quantity, E> {
            match kind {
                Kind::Bytes => Ok(Quantity::Bytes(n)),
                Kind::Count => Ok(Quantity::Count(n)),
                Kind::Millis => Ok(Quantity::Millis(n)),
                Kind::Ratio => u32::try_from(n)
                    .map(Quantity::Ratio)
                    .map_err(|_| E::custom("a ratio does not fit 32 bits")),
                Kind::Percent => percent(n),
            }
        }

        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Quantity;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a quantity: a kind and a number")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Quantity, A::Error> {
                let kind: Kind = seq
                    .next_element()?
                    .ok_or_else(|| Error::invalid_length(0, &self))?;
                let missing = || Error::invalid_length(1, &self);
                Ok(match kind {
                    Kind::Bytes => Quantity::Bytes(seq.next_element()?.ok_or_else(missing)?),
                    Kind::Count => Quantity::Count(seq.next_element()?.ok_or_else(missing)?),
                    Kind::Millis => Quantity::Millis(seq.next_element()?.ok_or_else(missing)?),
                    Kind::Ratio => Quantity::Ratio(seq.next_element()?.ok_or_else(missing)?),
                    Kind::Percent => {
                        percent(u64::from(seq.next_element::<u8>()?.ok_or_else(missing)?))?
                    }
                })
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Quantity, A::Error> {
                let (mut kind, mut n): (Option<Kind>, Option<u64>) = (None, None);
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "kind" if kind.is_some() => return Err(Error::duplicate_field("kind")),
                        "kind" => kind = Some(map.next_value()?),
                        "n" if n.is_some() => return Err(Error::duplicate_field("n")),
                        "n" => n = Some(map.next_value()?),
                        _ => {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                from_wide(
                    kind.ok_or_else(|| Error::missing_field("kind"))?,
                    n.ok_or_else(|| Error::missing_field("n"))?,
                )
            }
        }
        d.deserialize_struct("Quantity", &["kind", "n"], V)
    }
}

impl Quantity {
    /// The text for a value that was *measured* (the amount a limit was reached with): bytes and times are rounded to one
    /// decimal of the largest unit (`1.2 GiB`, `1.5 s`), because 1,210,123,456 bytes is not information. A limit itself
    /// reads exact through `Display`.
    #[must_use]
    pub fn approx(self) -> String {
        match self {
            Self::Bytes(n) => {
                const UNITS: [(&str, u64); 3] =
                    [("GiB", 1 << 30), ("MiB", 1 << 20), ("KiB", 1 << 10)];
                match UNITS.iter().find(|&&(_, size)| n >= size) {
                    Some(&(name, size)) => format!("{} {name}", tenths(n, size)),
                    None => format!("{n} B"),
                }
            }
            Self::Millis(n) if n >= 1000 => format!("{} s", tenths(n, 1000)),
            other => other.to_string(),
        }
    }
}

/// `n / size` rounded to one decimal, without a trailing `.0`.
fn tenths(n: u64, size: u64) -> String {
    let t = (u128::from(n) * 10 + u128::from(size) / 2) / u128::from(size);
    let (whole, frac) = (t / 10, t % 10);
    if frac == 0 {
        grouped(u64::try_from(whole).unwrap_or(u64::MAX))
    } else {
        format!(
            "{}.{frac}",
            grouped(u64::try_from(whole).unwrap_or(u64::MAX))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Review P1-3: a flood of junk keys fills only the table of unknown keys; the registered ones stay decodable. (A local
    /// table: the process-wide one is shared by every test of the binary.)
    #[test]
    fn a_flood_of_unknown_keys_never_refuses_a_registered_one() {
        let mut t = Interner::new(8);
        t.known.insert("registered.id");
        let mut refused = 0;
        for i in 0..100 {
            if t.parse(&format!("junk.k{i}")) == Err(BadKey::Full) {
                refused += 1;
            }
        }
        assert_eq!(refused, 92, "8 unknown keys fit, the rest are refused");
        assert_eq!(t.parse("registered.id"), Ok("registered.id"));
        // Keys already interned stay decodable too.
        assert_eq!(t.parse("junk.k0"), Ok("junk.k0"));
        assert_eq!(t.parse("junk.new"), Err(BadKey::Full));
        assert!(BadKey::Full.to_string().contains("too many"));
        assert!(t.reported, "the flood is logged once");
    }

    #[test]
    fn the_process_table_knows_the_built_in_row_from_the_start() {
        assert_eq!(with_table(|t| t.parse("bounded.cuts")), Ok("bounded.cuts"));
    }
}
