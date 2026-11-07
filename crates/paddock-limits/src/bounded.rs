//! `Bounded<T>`: a value plus what was left out of it (Debrief ADR 0065 section 1).
//!
//! A [`Cut`] is made only by the constructors and deserialized only through [`CutWire`]'s checks, so the illegal states
//! (a cut with nothing left out, `from + shown` past the total, a path that is not a field path) cannot be built. A
//! constructor takes a [`CutRow`], a handle on a registry row that declares the selection it is used for, so passing a stop
//! row or a row of another selection does not compile (a `const` handle of a wrong row fails the build).

use crate::id::{LimitId, grouped};
use crate::row::{BOUNDED_CUTS, LimitRow, Remedy, RowKind, RowValue, SelectionKind};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;

/// The most cuts one value may carry, the aggregate included.
pub const MAX_CUTS: usize = 32;
/// The longest [`FieldPath`].
pub const MAX_FIELD_PATH: usize = 64;

/// Why a bounded value or one of its parts was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundedError {
    /// Not a field path.
    Path,
    /// The numbers do not describe a cut: nothing left out, or `from + shown` past the total.
    Arithmetic,
    /// More than 32 cuts.
    TooManyCuts,
    /// A `bounded.cuts` aggregate that is not last, or appears twice.
    Aggregate,
    /// Two cuts with the same list and limit.
    Duplicate,
}

impl fmt::Display for BoundedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Path => "not a field path (parts of [a-z][a-zA-Z0-9]* with an optional [index], at most 64 bytes)",
            Self::Arithmetic => "a cut must leave something out, and from + shown must fit the total",
            Self::TooManyCuts => "more than 32 cuts on one value",
            Self::Aggregate => "the bounded.cuts aggregate must be the last cut and appear once",
            Self::Duplicate => "two cuts name the same list and limit",
        })
    }
}

impl std::error::Error for BoundedError {}

// ---- FieldPath -------------------------------------------------------------------------------------------------

/// A dotted path of field names in the reply's camelCase (`durationMs`, `callees.selfMs`), where a part may carry one
/// index into a list (`groups[3].ids`). Parsed once, at most 64 bytes; never shown to a person (the words come from the
/// message), never starting with `result`. The empty path means the value itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FieldPath(Cow<'static, str>);

const fn valid_path(b: &[u8]) -> bool {
    if b.is_empty() {
        return true;
    }
    if b.len() > MAX_FIELD_PATH {
        return false;
    }
    let mut i = 0;
    let n = b.len();
    // `result` as the first part would restate the wrapper.
    if n >= 6
        && b[0] == b'r'
        && b[1] == b'e'
        && b[2] == b's'
        && b[3] == b'u'
        && b[4] == b'l'
        && b[5] == b't'
        && (n == 6 || b[6] == b'.' || b[6] == b'[')
    {
        return false;
    }
    loop {
        if i >= n || !b[i].is_ascii_lowercase() {
            return false;
        }
        i += 1;
        while i < n && (b[i].is_ascii_alphanumeric()) {
            i += 1;
        }
        if i < n && b[i] == b'[' {
            i += 1;
            let start = i;
            let mut value: u64 = 0;
            while i < n && b[i].is_ascii_digit() {
                value = value * 10 + (b[i] - b'0') as u64;
                if value > u32::MAX as u64 {
                    return false;
                }
                i += 1;
            }
            if i == start || i >= n || b[i] != b']' {
                return false;
            }
            i += 1;
        }
        if i == n {
            return true;
        }
        if b[i] != b'.' {
            return false;
        }
        i += 1;
    }
}

impl FieldPath {
    /// A path from a literal; in a `const` a bad one fails the build.
    ///
    /// # Panics
    /// If `path` is not a field path. Paths are literals chosen by the programmer, never input.
    #[must_use]
    pub const fn lit(path: &'static str) -> Self {
        assert!(valid_path(path.as_bytes()), "not a field path");
        Self(Cow::Borrowed(path))
    }

    /// The value itself.
    #[must_use]
    pub const fn root() -> Self {
        Self(Cow::Borrowed(""))
    }

    /// A path from text outside the program.
    ///
    /// # Errors
    /// [`BoundedError::Path`].
    pub fn parse(path: &str) -> Result<Self, BoundedError> {
        if valid_path(path.as_bytes()) {
            Ok(Self(Cow::Owned(path.to_owned())))
        } else {
            Err(BoundedError::Path)
        }
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for FieldPath {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for FieldPath {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = <Cow<'de, str>>::deserialize(d)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "ts")]
impl ts_rs::TS for FieldPath {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;
    fn name() -> String {
        "FieldPath".into()
    }
    fn inline() -> String {
        "string".into()
    }
    fn inline_flattened() -> String {
        "string".into()
    }
    fn decl() -> String {
        "type FieldPath = string;".into()
    }
    fn decl_concrete() -> String {
        "type FieldPath = string;".into()
    }
}

// ---- the parts of a cut ----------------------------------------------------------------------------------------

/// How many items there were.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Total {
    /// Exactly `n`.
    Exact {
        /// The count.
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        n: u64,
    },
    /// More than `n - 1`: `n` is one past what was looked at, never counted further (rule 51).
    AtLeast {
        /// One past the limit.
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        n: u64,
    },
    /// Something may have been left out, and nobody knows how much. Only [`Cut::maybe`] makes it.
    Unknown,
}

/// Which items were kept.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Selection {
    /// Items `from .. from + shown` in the source's own order (a page, or the first ones from 0).
    First {
        /// Where the shown part starts.
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        from: u64,
    },
    /// The last `shown`, such as the newest lines of a tail.
    Last,
    /// Ranked by `key`; ranks `from + 1 ..= from + shown`.
    TopBy {
        /// What the ranking sorts by.
        key: SortKey,
        /// Ranks skipped.
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        from: u64,
    },
    /// A sample.
    Sample,
}

impl Selection {
    /// Which kind of selection this is, for matching against a registry row.
    #[must_use]
    pub const fn kind(&self) -> SelectionKind {
        match self {
            Self::First { .. } => SelectionKind::First,
            Self::Last => SelectionKind::Last,
            Self::TopBy { .. } => SelectionKind::TopBy,
            Self::Sample => SelectionKind::Sample,
        }
    }

    const fn from(&self) -> u64 {
        match self {
            Self::First { from } | Self::TopBy { from, .. } => *from,
            Self::Last | Self::Sample => 0,
        }
    }
}

/// What a ranking sorts by.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SortKey {
    /// The field of the listed items.
    pub field: FieldPath,
    /// Largest first or smallest first.
    pub order: Order,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Order {
    /// Largest first.
    Desc,
    /// Smallest first.
    Asc,
}

/// What the person can do about a cut; derived from the registry row, never chosen at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Next {
    /// Fetch the next page.
    Page,
    /// Show more.
    ShowMore,
    /// Raise the Setting.
    RaiseSetting,
    /// Narrow the query (words only).
    Narrow,
    /// Nothing.
    None,
}

// ---- row handles -----------------------------------------------------------------------------------------------

/// Marks a [`CutRow`] declared for [`Selection::First`].
#[derive(Debug, Clone, Copy)]
pub struct First;
/// Marks a [`CutRow`] declared for [`Selection::Last`].
#[derive(Debug, Clone, Copy)]
pub struct Last;
/// Marks a [`CutRow`] declared for [`Selection::TopBy`].
#[derive(Debug, Clone, Copy)]
pub struct TopBy;
/// Marks a [`CutRow`] declared for [`Selection::Sample`].
#[derive(Debug, Clone, Copy)]
pub struct Sample;
/// Marks a [`CutRow`] declared for [`Cut::maybe`].
#[derive(Debug, Clone, Copy)]
pub struct Maybe;

/// The selection a marker stands for.
pub trait SelectionMarker {
    /// Its kind.
    const KIND: SelectionKind;
}
macro_rules! marker {
    ($($t:ident => $k:ident),*) => {$(impl SelectionMarker for $t { const KIND: SelectionKind = SelectionKind::$k; })*};
}
marker!(First => First, Last => Last, TopBy => TopBy, Sample => Sample, Maybe => Maybe);

/// A handle on a registry row of `kind` cut or both that declares selection `S`. Made in a `const` (or `static`) so a row
/// that cannot cut, or does not declare `S`, fails the build.
#[derive(Debug)]
pub struct CutRow<S> {
    row: &'static LimitRow,
    selection: PhantomData<fn() -> S>,
}

impl<S> Clone for CutRow<S> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<S> Copy for CutRow<S> {}

impl<S: SelectionMarker> CutRow<S> {
    /// The handle for `row`.
    ///
    /// # Panics
    /// In a `const`, a compile error: if the row is a stop row, does not declare selection `S`, or does not count items.
    #[must_use]
    pub const fn of(row: &'static LimitRow) -> Self {
        assert!(
            !matches!(row.kind, RowKind::Stop),
            "a stop row cannot be used for a cut"
        );
        let mut declared = false;
        let mut i = 0;
        while i < row.selections.len() {
            if row.selections[i] as u8 == S::KIND as u8 {
                declared = true;
            }
            i += 1;
        }
        assert!(declared, "the row does not declare this selection");
        assert!(
            matches!(row.default, crate::Quantity::Count(_)),
            "a cut row counts items"
        );
        Self {
            row,
            selection: PhantomData,
        }
    }
}

impl<S> CutRow<S> {
    /// The row.
    #[must_use]
    pub const fn row(&self) -> &'static LimitRow {
        self.row
    }

    /// The row's default limit, in items.
    #[must_use]
    pub const fn limit(&self) -> u64 {
        self.row.default.amount()
    }

    /// What the person can do, from the row: a Setting gives `RaiseSetting`, otherwise the row's remedy.
    const fn next(&self) -> Next {
        match (self.row.value, self.row.remedy) {
            (RowValue::Setting { .. }, _) | (_, Remedy::RaiseSetting { .. }) => Next::RaiseSetting,
            (_, Remedy::ShowMore) => Next::ShowMore,
            (_, Remedy::Narrow) => Next::Narrow,
            (_, Remedy::Retry | Remedy::None) => Next::None,
        }
    }
}

// ---- Cut -------------------------------------------------------------------------------------------------------

/// The wire form of a [`Cut`]; deserializing a `Cut` runs the same checks as the constructors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "Cut"))]
pub struct CutWire {
    /// The limit.
    pub limit: LimitId,
    /// The list that was cut.
    pub of: FieldPath,
    /// Items in the reply, or looked at.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub shown: u64,
    /// How many there were.
    pub total: Total,
    /// Which were kept.
    pub selection: Selection,
    /// What the person can do.
    pub next: Next,
}

/// One list that was cut, and how.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CutWire", into = "CutWire")]
pub struct Cut {
    limit: LimitId,
    of: FieldPath,
    shown: u64,
    total: Total,
    selection: Selection,
    next: Next,
}

fn check(shown: u64, total: Total, selection: &Selection) -> Result<(), BoundedError> {
    let end = selection
        .from()
        .checked_add(shown)
        .ok_or(BoundedError::Arithmetic)?;
    let fits = match (total, &selection) {
        (Total::Exact { n }, _) => shown < n && end <= n,
        (Total::AtLeast { n }, Selection::First { .. }) => shown < n && end < n,
        (Total::AtLeast { n }, _) => shown < n && end <= n,
        (Total::Unknown, _) => true,
    };
    if fits {
        Ok(())
    } else {
        Err(BoundedError::Arithmetic)
    }
}

impl TryFrom<CutWire> for Cut {
    type Error = BoundedError;
    fn try_from(w: CutWire) -> Result<Self, BoundedError> {
        check(w.shown, w.total, &w.selection)?;
        Ok(Self {
            limit: w.limit,
            of: w.of,
            shown: w.shown,
            total: w.total,
            selection: w.selection,
            next: w.next,
        })
    }
}

impl From<Cut> for CutWire {
    fn from(c: Cut) -> Self {
        Self {
            limit: c.limit,
            of: c.of,
            shown: c.shown,
            total: c.total,
            selection: c.selection,
            next: c.next,
        }
    }
}

impl Cut {
    fn make<S: SelectionMarker>(
        row: &CutRow<S>,
        of: FieldPath,
        shown: u64,
        total: Total,
        selection: Selection,
        next: Next,
    ) -> Self {
        // A mismatch here is a programmer error, found by the first test that runs the call.
        assert!(
            S::KIND == selection.kind() || matches!(S::KIND, SelectionKind::Maybe),
            "selection does not match the row's handle"
        );
        if let Err(e) = check(shown, total, &selection) {
            panic!("limit {}: {e}", row.row.id);
        }
        Self {
            limit: row.row.id,
            of,
            shown,
            total,
            selection,
            next,
        }
    }

    /// A cut for a total the caller already knows. `None` when nothing was left out (`shown >= total`).
    ///
    /// # Panics
    /// If `selection` is not the kind the handle declares, or `from + shown` is past `total`.
    /// The panics are programmer-error assertions: every argument is a literal, a registry row or a count the caller computed from its own data, so no user or I/O data can reach them (a reply that is merely wrong in size yields `None`, never a panic).
    #[must_use]
    pub fn counted<S: SelectionMarker>(
        row: &CutRow<S>,
        of: FieldPath,
        shown: u64,
        total: u64,
        selection: Selection,
    ) -> Option<Self> {
        (shown < total).then(|| {
            Self::make(
                row,
                of,
                shown,
                Total::Exact { n: total },
                selection,
                row.next(),
            )
        })
    }

    /// A page `offset .. offset + shown` of `total`, with a way to the rest. `None` when the page is everything.
    ///
    /// # Panics
    /// If `offset + shown` is past `total`.
    /// The panics are programmer-error assertions: every argument is a literal, a registry row or a count the caller computed from its own data, so no user or I/O data can reach them (a reply that is merely wrong in size yields `None`, never a panic).
    #[must_use]
    pub fn page(
        row: &CutRow<First>,
        of: FieldPath,
        offset: u64,
        shown: u64,
        total: u64,
    ) -> Option<Self> {
        (shown < total).then(|| {
            Self::make(
                row,
                of,
                shown,
                Total::Exact { n: total },
                Selection::First { from: offset },
                Next::Page,
            )
        })
    }

    /// The first `shown` of more than `n - 1` (`n` is one past what was looked at). `None` when `shown >= n`.
    #[must_use]
    pub fn at_least(row: &CutRow<First>, of: FieldPath, shown: u64, n: u64) -> Option<Self> {
        (shown < n).then(|| {
            Self::make(
                row,
                of,
                shown,
                Total::AtLeast { n },
                Selection::First { from: 0 },
                row.next(),
            )
        })
    }

    /// Something may have been left out of this list, and the producer does not know how much.
    #[must_use]
    pub fn maybe(row: &CutRow<Maybe>, of: FieldPath, shown: u64, selection: Selection) -> Self {
        Self::make(row, of, shown, Total::Unknown, selection, row.next())
    }

    /// The limit.
    #[must_use]
    pub const fn limit(&self) -> LimitId {
        self.limit
    }
    /// Which list was cut.
    #[must_use]
    pub const fn of(&self) -> &FieldPath {
        &self.of
    }
    /// Items shown.
    #[must_use]
    pub const fn shown(&self) -> u64 {
        self.shown
    }
    /// How many there were.
    #[must_use]
    pub const fn total(&self) -> Total {
        self.total
    }
    /// Which were kept.
    #[must_use]
    pub const fn selection(&self) -> &Selection {
        &self.selection
    }
    /// What the person can do.
    #[must_use]
    pub const fn next(&self) -> Next {
        self.next
    }

    /// One English line for logs and the AI (no user content: ids, field paths and counts). The UI has its own words.
    #[must_use]
    pub fn describe(&self) -> String {
        let total = match self.total {
            Total::Exact { n } => format!("of {}", grouped(n)),
            Total::AtLeast { n } => format!("of more than {}", grouped(n.saturating_sub(1))),
            Total::Unknown => "of an unknown number".to_owned(),
        };
        let how = match &self.selection {
            Selection::First { from: 0 } => "the first".to_owned(),
            Selection::First { from } => format!("from {}", grouped(*from + 1)),
            Selection::Last => "the last".to_owned(),
            Selection::TopBy { key, from } => {
                format!(
                    "ranked by {} {}{}",
                    key.field.as_str(),
                    if key.order == Order::Desc {
                        "desc"
                    } else {
                        "asc"
                    },
                    if *from > 0 {
                        format!(" from rank {}", from + 1)
                    } else {
                        String::new()
                    }
                )
            }
            Selection::Sample => "a sample".to_owned(),
        };
        let of = if self.of.as_str().is_empty() {
            "the value"
        } else {
            self.of.as_str()
        };
        format!(
            "{}: {} {} shown {} in {}",
            self.limit,
            how,
            grouped(self.shown),
            total,
            of
        )
    }

    fn same_list(&self, other: &Self) -> bool {
        self.limit == other.limit && self.of == other.of
    }

    fn is_aggregate(&self) -> bool {
        self.limit == BOUNDED_CUTS.id
    }
}

// ---- Bounded ---------------------------------------------------------------------------------------------------

/// The wire form of [`Bounded`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "Bounded"))]
pub struct BoundedWire<T> {
    /// The reply.
    pub result: T,
    /// What was left out of it; empty means complete.
    #[cfg_attr(feature = "ts", ts(as = "Vec<CutWire>"))]
    pub cuts: Vec<Cut>,
}

/// A value and what was left out of it. An empty `cuts` means complete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "BoundedWire<T>", into = "BoundedWire<T>")]
#[serde(bound(
    serialize = "T: Serialize + Clone",
    deserialize = "T: Deserialize<'de>"
))]
pub struct Bounded<T> {
    result: T,
    cuts: Vec<Cut>,
}

impl<T> TryFrom<BoundedWire<T>> for Bounded<T> {
    type Error = BoundedError;
    fn try_from(w: BoundedWire<T>) -> Result<Self, BoundedError> {
        if w.cuts.len() > MAX_CUTS {
            return Err(BoundedError::TooManyCuts);
        }
        for (i, cut) in w.cuts.iter().enumerate() {
            if cut.is_aggregate() && i + 1 != w.cuts.len() {
                return Err(BoundedError::Aggregate);
            }
            if w.cuts[..i].iter().any(|c| c.same_list(cut)) {
                return Err(BoundedError::Duplicate);
            }
        }
        Ok(Self {
            result: w.result,
            cuts: w.cuts,
        })
    }
}

impl<T> From<Bounded<T>> for BoundedWire<T> {
    fn from(b: Bounded<T>) -> Self {
        Self {
            result: b.result,
            cuts: b.cuts,
        }
    }
}

impl<T> Bounded<T> {
    /// A value nothing was left out of.
    #[must_use]
    pub const fn complete(result: T) -> Self {
        Self {
            result,
            cuts: Vec::new(),
        }
    }

    /// A value with at most one cut (`None` is complete).
    #[must_use]
    pub fn with(result: T, cut: Option<Cut>) -> Self {
        Self::complete(result).and(cut)
    }

    /// Adds a cut (`None` adds nothing). A cut of a list and limit already cut replaces the earlier one. Past 32 cuts the
    /// first 31 stay in the order they were added and one aggregate on the `bounded.cuts` row says how many more there were;
    /// `.and` on a value that has an aggregate re-aggregates, and the aggregate stays last.
    #[must_use]
    pub fn and(mut self, cut: Option<Cut>) -> Self {
        let Some(cut) = cut else { return self };
        let mut extra = match self.cuts.last() {
            Some(last) if last.is_aggregate() => match self.cuts.pop().map(|a| a.total) {
                Some(Total::Exact { n }) => n.saturating_sub(MAX_CUTS as u64 - 1),
                _ => 0,
            },
            _ => 0,
        };
        if let Some(i) = self.cuts.iter().position(|c| c.same_list(&cut)) {
            self.cuts[i] = cut;
        } else if extra == 0 && self.cuts.len() < MAX_CUTS {
            self.cuts.push(cut);
        } else {
            if extra == 0 {
                extra = (self.cuts.len() - (MAX_CUTS - 1)) as u64;
                self.cuts.truncate(MAX_CUTS - 1);
            }
            extra += 1;
        }
        if extra > 0 {
            let row = CutRow::<First>::of(&BOUNDED_CUTS);
            let shown = MAX_CUTS as u64 - 1;
            self.cuts.push(Cut {
                limit: row.row.id,
                of: FieldPath::root(),
                shown,
                total: Total::Exact { n: shown + extra },
                selection: Selection::First { from: 0 },
                next: Next::None,
            });
        }
        self
    }

    /// The value.
    #[must_use]
    pub const fn result(&self) -> &T {
        &self.result
    }

    /// The value, dropping the cuts (the caller has shown them).
    #[must_use]
    pub fn into_result(self) -> T {
        self.result
    }

    /// What was left out.
    #[must_use]
    pub fn cuts(&self) -> &[Cut] {
        &self.cuts
    }

    /// Whether nothing was left out.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.cuts.is_empty()
    }

    /// Changes the value and keeps the cuts.
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Bounded<U> {
        Bounded {
            result: f(self.result),
            cuts: self.cuts,
        }
    }
}

fn to_usize(n: u64) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

impl<T> Bounded<Vec<T>> {
    /// The first items of `iter`, at most the row's limit. It reads one more item to know whether any were left out and
    /// answers `AtLeast { n: limit + 1 }`; it never counts further just to print a number (rule 51).
    #[must_use]
    pub fn first<I: IntoIterator<Item = T>>(iter: I, row: &CutRow<First>, of: FieldPath) -> Self {
        Self::first_with(iter, row, row.limit(), of)
    }

    /// As [`Bounded::first`] with the limit the Setting currently holds; the row names the limit and its remedy.
    #[must_use]
    pub fn first_with<I: IntoIterator<Item = T>>(
        iter: I,
        row: &CutRow<First>,
        limit: u64,
        of: FieldPath,
    ) -> Self {
        let mut items: Vec<T> = iter
            .into_iter()
            .take(to_usize(limit.saturating_add(1)))
            .collect();
        let cut = if items.len() as u64 > limit {
            items.truncate(to_usize(limit));
            Cut::at_least(row, of, limit, limit + 1)
        } else {
            None
        };
        Self::with(items, cut)
    }

    /// A page: `items` are `offset ..` of `total`. The cut says where the page starts and has `Next::Page`.
    #[must_use]
    pub fn page(
        items: Vec<T>,
        offset: u64,
        total: u64,
        row: &CutRow<First>,
        of: FieldPath,
    ) -> Self {
        let cut = Cut::page(row, of, offset, items.len() as u64, total);
        Self::with(items, cut)
    }

    /// `items` are ranked and all were seen: keeps the row's limit and answers `Exact` with `TopBy { from: 0 }`.
    #[must_use]
    pub fn ranked(mut items: Vec<T>, row: &CutRow<TopBy>, of: FieldPath, key: SortKey) -> Self {
        let seen = items.len() as u64;
        items.truncate(to_usize(row.limit()));
        let cut = Cut::counted(
            row,
            of,
            items.len() as u64,
            seen,
            Selection::TopBy { key, from: 0 },
        );
        Self::with(items, cut)
    }

    /// The first items (from 0) when the store already knows the `total`.
    #[must_use]
    pub fn counted(items: Vec<T>, total: u64, row: &CutRow<First>, of: FieldPath) -> Self {
        let cut = Cut::counted(
            row,
            of,
            items.len() as u64,
            total,
            Selection::First { from: 0 },
        );
        Self::with(items, cut)
    }

    /// The last items, out of `total`.
    #[must_use]
    pub fn last(items: Vec<T>, total: u64, row: &CutRow<Last>, of: FieldPath) -> Self {
        let cut = Cut::counted(row, of, items.len() as u64, total, Selection::Last);
        Self::with(items, cut)
    }

    /// A sample, out of `total`.
    #[must_use]
    pub fn sampled(items: Vec<T>, total: u64, row: &CutRow<Sample>, of: FieldPath) -> Self {
        let cut = Cut::counted(row, of, items.len() as u64, total, Selection::Sample);
        Self::with(items, cut)
    }
}
