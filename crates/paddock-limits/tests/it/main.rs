//! Integration tests of `paddock-limits`: one binary, one module per topic.

mod constructors;
mod ids;
mod message;
mod registry;
mod wire;

use paddock_limits::{
    BOUNDED_CUTS, LimitId, LimitRow, Quantity, Remedy, RowKind, RowValue, SelectionKind,
    SettingPath, Unit,
};

/// A cut row for tests: items, hard, with the given selections.
pub const fn cut_row(
    id: &'static str,
    limit: u64,
    selections: &'static [SelectionKind],
    remedy: Remedy,
) -> LimitRow {
    LimitRow {
        id: LimitId::new(id),
        unit: Unit::Count,
        default: Quantity::Count(limit),
        min: Quantity::Count(1),
        max: Quantity::Count(1_000_000),
        value: RowValue::Hard,
        kind: RowKind::Cut,
        selections,
        remedy,
    }
}

pub static FIRST_5: LimitRow = cut_row("test.first", 5, &[SelectionKind::First], Remedy::ShowMore);
pub static FIRST_NARROW: LimitRow =
    cut_row("test.narrow", 5, &[SelectionKind::First], Remedy::Narrow);
pub static TOP_3: LimitRow = cut_row("test.top", 3, &[SelectionKind::TopBy], Remedy::None);
pub static LAST_2: LimitRow = cut_row("test.last", 2, &[SelectionKind::Last], Remedy::None);
pub static SAMPLE_2: LimitRow = cut_row("test.sample", 2, &[SelectionKind::Sample], Remedy::None);
pub static MAYBE: LimitRow = cut_row("test.maybe", 512, &[SelectionKind::Maybe], Remedy::None);
pub static SETTING_ROW: LimitRow = LimitRow {
    id: LimitId::new("test.setting"),
    unit: Unit::Count,
    default: Quantity::Count(4),
    min: Quantity::Count(1),
    max: Quantity::Count(100),
    value: RowValue::Setting {
        path: SettingPath::new("performance.testCap"),
    },
    kind: RowKind::Cut,
    selections: &[SelectionKind::First],
    remedy: Remedy::RaiseSetting {
        path: SettingPath::new("performance.testCap"),
    },
};

#[test]
fn builtin_row_is_listed() {
    assert_eq!(BOUNDED_CUTS.id.as_str(), "bounded.cuts");
}
