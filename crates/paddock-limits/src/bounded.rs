//! `Bounded<T>`: a value plus what was left out of it (Debrief ADR 0065 section 1).
//!
//! A [`Cut`] is made only by the constructors and deserialized only through [`CutWire`]'s checks, so the illegal states
//! (a cut with nothing left out, `from + shown` past the total, a path that is not a field path) cannot be built. A
//! constructor takes a [`CutRow`], a handle on a registry row that declares the selection it is used for, so passing a stop
//! row or a row of another selection does not compile (a `const` handle of a wrong row fails the build).

use crate::fieldpath::FieldPath;
use crate::id::{LimitId, grouped};
use crate::row::{BOUNDED_CUTS, LimitRow, Remedy, RowKind, RowValue, SelectionKind};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::marker::PhantomData;

/// The most cuts one value may carry, the aggregate included.
pub const MAX_CUTS: usize = 32;

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
/// that cannot cut, or does not declare `S`, fails the build. Each constructor takes the handle of the one selection it
/// reports, so a call with the wrong kind of row does not compile.
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
    /// Rows are literals chosen by the programmer, never input.
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

/// A limit value that was checked against its row's bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowLimit(u64);

impl RowLimit {
    /// The number of items.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A limit value outside the row's `min ..= max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitOutOfRange {
    /// The row.
    pub id: LimitId,
    /// What was given.
    pub value: u64,
    /// The row's lowest value.
    pub min: u64,
    /// The row's highest value.
    pub max: u64,
}

impl fmt::Display for LimitOutOfRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "limit {}: {} is outside {} to {}",
            self.id, self.value, self.min, self.max
        )
    }
}

impl std::error::Error for LimitOutOfRange {}

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

    /// `value` (what a Setting holds) if it lies within the row's `min ..= max`.
    ///
    /// # Errors
    /// [`LimitOutOfRange`].
    pub fn limit_of(&self, value: u64) -> Result<RowLimit, LimitOutOfRange> {
        let (min, max) = (self.row.min.amount(), self.row.max.amount());
        if (min..=max).contains(&value) {
            Ok(RowLimit(value))
        } else {
            Err(LimitOutOfRange {
                id: self.row.id,
                value,
                min,
                max,
            })
        }
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

/// What a cut can show on its own: something was left out and the numbers fit. `AtLeast` exists only for the first items
/// from 0 (a lower bound of what a scan looked at); every other window has an exact total or none.
fn check(shown: u64, total: Total, selection: &Selection) -> Result<(), BoundedError> {
    let fits = match total {
        Total::Unknown => true,
        Total::Exact { n } => {
            shown < n
                && selection
                    .from()
                    .checked_add(shown)
                    .is_some_and(|end| end <= n)
        }
        Total::AtLeast { n } => matches!(selection, Selection::First { from: 0 }) && shown < n,
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

/// How a window of `shown` items starting at `from` relates to `total` (`None`: the window is everything).
///
/// The numbers come from requests and from stores that count and read at different moments, so they may disagree. They
/// never panic and never hide missing rows: a window that does not start at 0 is a cut even when it ends at the total,
/// and one that ends past the total (the count is stale) has an unknown total.
fn window(shown: u64, total: u64, from: u64) -> Option<Total> {
    if from == 0 && shown >= total {
        None
    } else if from.saturating_add(shown) <= total {
        Some(Total::Exact { n: total })
    } else {
        Some(Total::Unknown)
    }
}

impl Cut {
    fn new(
        limit: LimitId,
        of: FieldPath,
        shown: u64,
        total: Total,
        selection: Selection,
        next: Next,
    ) -> Self {
        // `window` and the callers give numbers `check` accepts; if one ever did not, the total is "unknown" rather than a
        // value the wire would refuse.
        let total = if check(shown, total, &selection).is_ok() {
            total
        } else {
            Total::Unknown
        };
        Self {
            limit,
            of,
            shown,
            total,
            selection,
            next,
        }
    }

    /// The first `shown` items from `from` of a list whose store counted `total`. `None` when nothing was left out.
    #[must_use]
    pub fn counted_first(
        row: &CutRow<First>,
        of: FieldPath,
        shown: u64,
        total: u64,
        from: u64,
    ) -> Option<Self> {
        window(shown, total, from).map(|t| {
            Self::new(
                row.row.id,
                of,
                shown,
                t,
                Selection::First { from },
                row.next(),
            )
        })
    }

    /// A page `offset .. offset + shown` with a way to the rest (`Next::Page`). `None` when the page is everything.
    #[must_use]
    pub fn page(
        row: &CutRow<First>,
        of: FieldPath,
        offset: u64,
        shown: u64,
        total: u64,
    ) -> Option<Self> {
        window(shown, total, offset).map(|t| {
            Self::new(
                row.row.id,
                of,
                shown,
                t,
                Selection::First { from: offset },
                Next::Page,
            )
        })
    }

    /// Ranks `from + 1 ..= from + shown` by `key` out of `total`. `None` when nothing was left out.
    #[must_use]
    pub fn counted_top(
        row: &CutRow<TopBy>,
        of: FieldPath,
        shown: u64,
        total: u64,
        key: SortKey,
        from: u64,
    ) -> Option<Self> {
        window(shown, total, from).map(|t| {
            Self::new(
                row.row.id,
                of,
                shown,
                t,
                Selection::TopBy { key, from },
                row.next(),
            )
        })
    }

    /// The last `shown` items out of `total`. `None` when nothing was left out.
    #[must_use]
    pub fn counted_last(row: &CutRow<Last>, of: FieldPath, shown: u64, total: u64) -> Option<Self> {
        window(shown, total, 0)
            .map(|t| Self::new(row.row.id, of, shown, t, Selection::Last, row.next()))
    }

    /// A sample of `shown` items out of `total`. `None` when nothing was left out.
    #[must_use]
    pub fn counted_sample(
        row: &CutRow<Sample>,
        of: FieldPath,
        shown: u64,
        total: u64,
    ) -> Option<Self> {
        window(shown, total, 0)
            .map(|t| Self::new(row.row.id, of, shown, t, Selection::Sample, row.next()))
    }

    /// The first `shown` of more than `n - 1` (`n` is one past what was looked at). `None` when `shown >= n`.
    #[must_use]
    pub fn at_least(row: &CutRow<First>, of: FieldPath, shown: u64, n: u64) -> Option<Self> {
        (shown < n).then(|| {
            Self::new(
                row.row.id,
                of,
                shown,
                Total::AtLeast { n },
                Selection::First { from: 0 },
                row.next(),
            )
        })
    }

    /// Something may have been left out of this list, and the producer does not know how much. `selection` says which
    /// items were kept.
    #[must_use]
    pub fn maybe(row: &CutRow<Maybe>, of: FieldPath, shown: u64, selection: Selection) -> Self {
        Self::new(row.row.id, of, shown, Total::Unknown, selection, row.next())
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

    /// One English line for logs and the AI (ids, field paths and counts only; the UI has its own words):
    /// `rows: showing 501 to 503 of 1,204 (test.first)`.
    #[must_use]
    pub fn describe(&self) -> String {
        let total = match self.total {
            Total::Exact { n } => grouped(n),
            Total::AtLeast { n } => format!("more than {}", grouped(n.saturating_sub(1))),
            Total::Unknown => "an unknown number".to_owned(),
        };
        let shown = grouped(self.shown);
        let showing = match &self.selection {
            Selection::First { from: 0 } => format!("the first {shown}"),
            Selection::First { from } if self.shown == 0 => {
                format!("none from {}", grouped(from + 1))
            }
            Selection::First { from } => {
                format!("{} to {}", grouped(from + 1), grouped(from + self.shown))
            }
            Selection::Last => format!("the last {shown}"),
            Selection::TopBy { key, from } => {
                let order = if key.order == Order::Desc {
                    "descending"
                } else {
                    "ascending"
                };
                let skipped = if *from > 0 {
                    format!(", from rank {}", grouped(from + 1))
                } else {
                    String::new()
                };
                format!("the top {shown} by {} {order}{skipped}", key.field.as_str())
            }
            Selection::Sample => format!("a sample of {shown}"),
        };
        let of = if self.of.as_str().is_empty() {
            "the value"
        } else {
            self.of.as_str()
        };
        format!("{of}: showing {showing} of {total} ({})", self.limit)
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

/// What is serialized: the value and the cuts by reference, so a large reply is not copied to be written.
#[derive(Serialize)]
struct BoundedRef<'a, T> {
    result: &'a T,
    cuts: &'a [Cut],
}

/// A value and what was left out of it. An empty `cuts` means complete.
///
/// The wire is JSON or another self-describing format: the cut variants are internally tagged, which postcard cannot
/// decode. A command reply is JSON; a postcard boundary carries `cuts` as its own typed field instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bounded<T> {
    result: T,
    cuts: Vec<Cut>,
}

impl<T: Serialize> Serialize for Bounded<T> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        BoundedRef {
            result: &self.result,
            cuts: &self.cuts,
        }
        .serialize(s)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Bounded<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        BoundedWire::<T>::deserialize(d)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl<T> TryFrom<BoundedWire<T>> for Bounded<T> {
    type Error = BoundedError;
    fn try_from(w: BoundedWire<T>) -> Result<Self, BoundedError> {
        if w.cuts.len() > MAX_CUTS {
            return Err(BoundedError::TooManyCuts);
        }
        for (i, cut) in w.cuts.iter().enumerate() {
            if cut.is_aggregate() && !aggregate_is_well_formed(cut, i, w.cuts.len()) {
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

/// The aggregate (ADR 0065 section 1) is the last of exactly 32 cuts, of the value itself, shows the 31 before it, says
/// how many cuts there were (more than 31) and offers nothing.
fn aggregate_is_well_formed(cut: &Cut, index: usize, len: usize) -> bool {
    let kept = (MAX_CUTS - 1) as u64;
    len == MAX_CUTS
        && index == len - 1
        && cut.of.as_str().is_empty()
        && cut.shown == kept
        && matches!(cut.total, Total::Exact { n } if n > kept)
        && cut.selection == (Selection::First { from: 0 })
        && cut.next == Next::None
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

    /// Adds a cut (`None` adds nothing). A cut of a list and limit already cut replaces the earlier one. The cuts are kept
    /// in the structural order of their lists (a list before the lists inside it, siblings by name, entries by index), so
    /// the result does not depend on the order they were added in. Past 32 cuts the 31 first in that order stay and one
    /// aggregate on the `bounded.cuts` row says how many there were; `.and` on a value that has an aggregate
    /// re-aggregates, and the aggregate stays last.
    #[must_use]
    pub fn and(mut self, cut: Option<Cut>) -> Self {
        let Some(cut) = cut else { return self };
        let kept = (MAX_CUTS - 1) as u64;
        let mut extra = match self.cuts.last() {
            Some(last) if last.is_aggregate() => match self.cuts.pop().map(|a| a.total) {
                Some(Total::Exact { n }) => n.saturating_sub(kept),
                _ => 0,
            },
            _ => 0,
        };
        match self.cuts.iter().position(|c| c.same_list(&cut)) {
            Some(i) => self.cuts[i] = cut,
            None => self.cuts.push(cut),
        }
        self.cuts
            .sort_by(|a, b| a.of.cmp(&b.of).then(a.limit.cmp(&b.limit)));
        if extra == 0 && self.cuts.len() <= MAX_CUTS {
            return self;
        }
        let total = self.cuts.len() as u64 + extra;
        self.cuts.truncate(MAX_CUTS - 1);
        extra = total - kept;
        self.cuts.push(Cut {
            limit: BOUNDED_CUTS.id,
            of: FieldPath::root(),
            shown: kept,
            total: Total::Exact { n: kept + extra },
            selection: Selection::First { from: 0 },
            next: Next::None,
        });
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
        Self::first_with(iter, row, RowLimit(row.limit()), of)
    }

    /// As [`Bounded::first`] with the limit the Setting currently holds, checked against the row's bounds by
    /// [`CutRow::limit_of`].
    #[must_use]
    pub fn first_with<I: IntoIterator<Item = T>>(
        iter: I,
        row: &CutRow<First>,
        limit: RowLimit,
        of: FieldPath,
    ) -> Self {
        let limit = limit.get();
        let mut items: Vec<T> = iter
            .into_iter()
            .take(to_usize(limit.saturating_add(1)))
            .collect();
        let cut = if items.len() as u64 > limit {
            items.truncate(to_usize(limit));
            Cut::at_least(row, of, limit, limit.saturating_add(1))
        } else {
            None
        };
        Self::with(items, cut)
    }

    /// A page: `items` are `offset ..` of `total`. The cut says where the page starts and has `Next::Page`. A total that
    /// disagrees with the page (the list changed between the count and the read) gives an unknown total, never a panic.
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
        let cut = Cut::counted_top(row, of, items.len() as u64, seen, key, 0);
        Self::with(items, cut)
    }

    /// The first items (from 0) when the store already knows the `total`.
    #[must_use]
    pub fn counted(items: Vec<T>, total: u64, row: &CutRow<First>, of: FieldPath) -> Self {
        let cut = Cut::counted_first(row, of, items.len() as u64, total, 0);
        Self::with(items, cut)
    }

    /// The last items, out of `total`.
    #[must_use]
    pub fn last(items: Vec<T>, total: u64, row: &CutRow<Last>, of: FieldPath) -> Self {
        let cut = Cut::counted_last(row, of, items.len() as u64, total);
        Self::with(items, cut)
    }

    /// A sample, out of `total`.
    #[must_use]
    pub fn sampled(items: Vec<T>, total: u64, row: &CutRow<Sample>, of: FieldPath) -> Self {
        let cut = Cut::counted_sample(row, of, items.len() as u64, total);
        Self::with(items, cut)
    }
}

// ---- TypeScript ------------------------------------------------------------------------------------------------

/// `Bounded<T>` and `Cut` cross the wire as `BoundedWire<T>` and `CutWire`; their declarations are those, so a struct
/// that holds them can derive `TS`.
#[cfg(feature = "ts")]
mod ts_impls {
    use super::{Bounded, BoundedWire, Cut, CutWire};
    use ts_rs::{TS, TypeVisitor};

    macro_rules! delegate {
        () => {
            fn ident() -> String {
                <<Self as Wired>::Wire as TS>::ident()
            }
            fn name() -> String {
                <<Self as Wired>::Wire as TS>::name()
            }
            fn inline() -> String {
                <<Self as Wired>::Wire as TS>::inline()
            }
            fn inline_flattened() -> String {
                <<Self as Wired>::Wire as TS>::inline_flattened()
            }
            fn decl() -> String {
                <<Self as Wired>::Wire as TS>::decl()
            }
            fn decl_concrete() -> String {
                <<Self as Wired>::Wire as TS>::decl_concrete()
            }
            fn visit_dependencies(v: &mut impl TypeVisitor)
            where
                Self: 'static,
            {
                <<Self as Wired>::Wire as TS>::visit_dependencies(v)
            }
            fn visit_generics(v: &mut impl TypeVisitor)
            where
                Self: 'static,
            {
                <<Self as Wired>::Wire as TS>::visit_generics(v)
            }
        };
    }

    trait Wired {
        type Wire: TS + 'static;
    }
    impl Wired for Cut {
        type Wire = CutWire;
    }
    impl<T: TS + 'static> Wired for Bounded<T> {
        type Wire = BoundedWire<T>;
    }

    impl TS for Cut {
        type WithoutGenerics = Self;
        type OptionInnerType = Self;
        delegate!();
    }

    impl<T: TS + 'static> TS for Bounded<T> {
        type WithoutGenerics = Bounded<ts_rs::Dummy>;
        type OptionInnerType = Self;
        delegate!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::{LimitRow, Remedy};
    use crate::{Quantity, RowValue, Unit};

    static ROW: LimitRow = LimitRow {
        id: LimitId::new("unit.rows"),
        unit: Unit::Count,
        default: Quantity::Count(5),
        min: Quantity::Count(1),
        max: Quantity::Count(10),
        value: RowValue::Hard,
        kind: RowKind::Cut,
        selections: &[SelectionKind::First],
        remedy: Remedy::None,
    };

    fn cut(of: &str) -> Cut {
        Cut::at_least(
            &CutRow::<First>::of(&ROW),
            FieldPath::parse(of).unwrap(),
            5,
            6,
        )
        .unwrap()
    }

    /// A well-formed aggregate always has an exact count (the wire refuses any other); if one ever did not, `.and` counts
    /// again from the cuts it has instead of trusting it.
    #[test]
    fn an_aggregate_without_an_exact_count_is_not_trusted_by_and() {
        let mut aggregate = cut("");
        aggregate.limit = BOUNDED_CUTS.id;
        aggregate.total = Total::Unknown;
        let mut cuts: Vec<Cut> = (0..31).map(|i| cut(&format!("l{i}"))).collect();
        cuts.push(aggregate);
        let b = Bounded { result: (), cuts }.and(Some(cut("zz")));
        assert_eq!(b.cuts().len(), 32);
        assert!(b.cuts().iter().all(|c| !c.is_aggregate()));
    }
}
