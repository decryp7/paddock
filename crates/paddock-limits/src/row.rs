//! The registry: one row per limit.

use crate::id::{LimitId, Quantity, SettingPath, Unit};
use serde::Serialize;
use std::fmt;

/// What a person can do about a limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum Remedy {
    /// Raise the Setting.
    RaiseSetting {
        /// The Setting that holds the limit.
        path: SettingPath,
    },
    /// Show more of a cut reply.
    ShowMore,
    /// Narrow the query or the input.
    Narrow,
    /// Run it again.
    Retry,
    /// Nothing: the limit protects the process.
    None,
}

/// Where a limit's value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum RowValue {
    /// A Setting (rule 21); the row's `min` and `max` are its bounds.
    Setting {
        /// Its path.
        path: SettingPath,
    },
    /// A constant that protects the process.
    Hard,
    /// Set by the request (a page size the person never typed).
    ByRequest,
}

/// What breaking the limit does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum RowKind {
    /// The work stops with a `LimitExceeded`.
    Stop,
    /// The reply is cut and carries a `Cut`.
    Cut,
    /// Either, depending on the caller.
    Both,
}

/// How a cut row may choose the part it shows (ADR 0065 section 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub enum SelectionKind {
    /// The first items in the source's order, or a page.
    First,
    /// The last items.
    Last,
    /// Ranked by a key.
    TopBy,
    /// A sample.
    Sample,
    /// Something may have been left out, unknown how much.
    Maybe,
}

/// One limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LimitRow {
    /// Its key.
    pub id: LimitId,
    /// The unit of `default`, `min` and `max`.
    pub unit: Unit,
    /// The value when nothing sets it.
    pub default: Quantity,
    /// The lowest value a Setting may take.
    pub min: Quantity,
    /// The highest value a Setting may take.
    pub max: Quantity,
    /// Where the value comes from.
    pub value: RowValue,
    /// Stop, cut or both.
    pub kind: RowKind,
    /// For cut rows, the selections a reply may report.
    pub selections: &'static [SelectionKind],
    /// What a person can do about it.
    pub remedy: Remedy,
}

impl LimitRow {
    /// A row of a limit that stops work (`kind` stop, no selections), in the unit of `default`: the shape almost every
    /// limit has. `min` and `max` are the bounds of its Setting, or of what a caller may ask for.
    #[must_use]
    pub const fn stop(
        id: &'static str,
        default: Quantity,
        min: Quantity,
        max: Quantity,
        value: RowValue,
        remedy: Remedy,
    ) -> Self {
        Self {
            id: LimitId::new(id),
            unit: default.unit(),
            default,
            min,
            max,
            value,
            kind: RowKind::Stop,
            selections: &[],
            remedy,
        }
    }
}

/// A registry that breaks its own rules, found by [`Registry::check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryError {
    /// The row.
    pub id: LimitId,
    /// What is wrong.
    pub problem: &'static str,
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "limit row {}: {}", self.id, self.problem)
    }
}

impl std::error::Error for RegistryError {}

/// The rows of one crate or app. Each app has its own registry beside the shared crates' rows.
#[derive(Debug, Clone, Copy)]
pub struct Registry(&'static [LimitRow]);

impl Registry {
    /// A registry over static rows.
    #[must_use]
    pub const fn new(rows: &'static [LimitRow]) -> Self {
        Self(rows)
    }

    /// The rows.
    #[must_use]
    pub const fn rows(&self) -> &'static [LimitRow] {
        self.0
    }

    /// The row with this id.
    #[must_use]
    pub fn get(&self, id: LimitId) -> Option<&'static LimitRow> {
        self.0.iter().find(|r| r.id == id)
    }

    /// Checks the rules every registry follows: unique ids; `default`, `min` and `max` in the row's unit with
    /// `min <= default <= max`; a Setting row has `RaiseSetting` as its remedy with the same path; a stop row declares no
    /// selections and a cut or both row declares at least one and counts items.
    ///
    /// # Errors
    /// The first row that breaks a rule.
    pub fn check(&self) -> Result<(), RegistryError> {
        for (i, row) in self.0.iter().enumerate() {
            let bad = |problem| {
                Err(RegistryError {
                    id: row.id,
                    problem,
                })
            };
            if self.0[..i].iter().any(|r| r.id == row.id) {
                return bad("the id appears twice");
            }
            let q = [row.default, row.min, row.max];
            if q.iter().any(|v| v.unit() != row.unit) {
                return bad("default, min and max must be in the row's unit");
            }
            if !(row.min.at_most(row.default) && row.default.at_most(row.max)) {
                return bad("min <= default <= max does not hold");
            }
            match (row.value, row.remedy) {
                (RowValue::Setting { path }, Remedy::RaiseSetting { path: remedy })
                    if path == remedy => {}
                (RowValue::Setting { .. }, _) => {
                    return bad("a Setting row's remedy is RaiseSetting with the same path");
                }
                (_, Remedy::RaiseSetting { .. }) => {
                    return bad("only a Setting row may have RaiseSetting as its remedy");
                }
                _ => {}
            }
            match row.kind {
                RowKind::Stop if !row.selections.is_empty() => {
                    return bad("a stop row declares no selections");
                }
                RowKind::Cut | RowKind::Both if row.selections.is_empty() => {
                    return bad("a cut row declares its selections");
                }
                RowKind::Cut | RowKind::Both if row.unit != Unit::Count => {
                    return bad("a cut row counts items");
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// The aggregate row of a value with more than 32 cuts (ADR 0065 section 1). Every app's registry includes it (use
/// [`BUILTIN`]).
const BOUNDED_CUTS_ROW: LimitRow = LimitRow {
    id: LimitId::new("bounded.cuts"),
    unit: Unit::Count,
    default: Quantity::Count(31),
    min: Quantity::Count(31),
    max: Quantity::Count(31),
    value: RowValue::Hard,
    kind: RowKind::Cut,
    selections: &[SelectionKind::First],
    remedy: Remedy::None,
};

/// See [`BOUNDED_CUTS_ROW`].
pub static BOUNDED_CUTS: LimitRow = BOUNDED_CUTS_ROW;

static BUILTIN_ROWS: [LimitRow; 1] = [BOUNDED_CUTS_ROW];

/// The rows this crate owns.
pub const BUILTIN: Registry = Registry::new(&BUILTIN_ROWS);
