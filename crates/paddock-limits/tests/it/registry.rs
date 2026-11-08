use crate::*;
use paddock_limits::{BUILTIN, Registry};

fn row(f: impl FnOnce(&mut LimitRow)) -> &'static LimitRow {
    let mut r = SETTING_ROW;
    f(&mut r);
    Box::leak(Box::new(r))
}

fn check_one(r: &'static LimitRow) -> Result<(), String> {
    let rows: &'static [LimitRow] = Box::leak(vec![*r].into_boxed_slice());
    Registry::new(rows).check().map_err(|e| e.to_string())
}

#[test]
fn good_registries_pass() {
    BUILTIN.check().unwrap();
    assert!(check_one(&SETTING_ROW).is_ok());
    Registry::new(Box::leak(vec![FIRST_5, TOP_3].into_boxed_slice()))
        .check()
        .unwrap();
}

#[test]
fn lookup_by_id() {
    let rows: &'static [LimitRow] = Box::leak(vec![FIRST_5, TOP_3].into_boxed_slice());
    let reg = Registry::new(rows);
    assert_eq!(
        reg.get(LimitId::new("test.top")).unwrap().default,
        Quantity::Count(3)
    );
    assert!(reg.get(LimitId::new("test.none")).is_none());
    assert_eq!(reg.rows().len(), 2);
}

#[test]
fn duplicate_ids_are_refused() {
    let rows: &'static [LimitRow] = Box::leak(vec![FIRST_5, FIRST_5].into_boxed_slice());
    assert!(
        Registry::new(rows)
            .check()
            .unwrap_err()
            .to_string()
            .contains("twice")
    );
}

#[test]
fn units_and_bounds_are_checked() {
    assert!(
        check_one(row(|r| r.default = Quantity::Bytes(4)))
            .unwrap_err()
            .contains("unit")
    );
    assert!(
        check_one(row(|r| r.default = Quantity::Count(0)))
            .unwrap_err()
            .contains("min <= default")
    );
    assert!(
        check_one(row(|r| r.default = Quantity::Count(101)))
            .unwrap_err()
            .contains("min <= default")
    );
}

#[test]
fn setting_and_remedy_must_agree() {
    assert!(
        check_one(row(|r| r.remedy = Remedy::ShowMore))
            .unwrap_err()
            .contains("RaiseSetting")
    );
    assert!(
        check_one(row(|r| r.remedy = Remedy::RaiseSetting {
            path: SettingPath::new("other.path")
        }))
        .unwrap_err()
        .contains("same path")
    );
    assert!(
        check_one(row(|r| {
            r.value = RowValue::Hard;
        }))
        .unwrap_err()
        .contains("only a Setting row")
    );
}

#[test]
fn kind_and_selections_must_agree() {
    assert!(
        check_one(row(|r| {
            r.kind = RowKind::Stop;
        }))
        .unwrap_err()
        .contains("no selections")
    );
    assert!(
        check_one(row(|r| r.selections = &[]))
            .unwrap_err()
            .contains("declares its selections")
    );
    let bytes = row(|r| {
        r.unit = Unit::Bytes;
        r.default = Quantity::Bytes(4);
        r.min = Quantity::Bytes(1);
        r.max = Quantity::Bytes(100);
    });
    assert!(check_one(bytes).unwrap_err().contains("counts items"));
    // A row that can cut counts items too, whichever way it is used.
    let both = row(|r| {
        r.kind = RowKind::Both;
        r.unit = Unit::Bytes;
        r.default = Quantity::Bytes(4);
        r.min = Quantity::Bytes(1);
        r.max = Quantity::Bytes(100);
    });
    assert!(check_one(both).unwrap_err().contains("counts items"));
    // A stop row in bytes is fine.
    let stop = row(|r| {
        r.kind = RowKind::Stop;
        r.selections = &[];
        r.unit = Unit::Bytes;
        r.default = Quantity::Bytes(4);
        r.min = Quantity::Bytes(1);
        r.max = Quantity::Bytes(100);
    });
    assert!(check_one(stop).is_ok());
}

#[test]
fn the_stop_constructor_gives_a_row_that_passes_the_checks() {
    static ROWS: [LimitRow; 1] = [LimitRow::stop(
        "kit.stop",
        Quantity::Millis(30_000),
        Quantity::Millis(1),
        Quantity::Millis(60_000),
        RowValue::Hard,
        Remedy::None,
    )];
    let reg = Registry::new(&ROWS);
    reg.check().unwrap();
    assert_eq!((ROWS[0].unit, ROWS[0].kind), (Unit::Millis, RowKind::Stop));
    assert!(ROWS[0].selections.is_empty());
}

/// `const` evaluation is not counted by coverage: the same constructor, called at run time.
#[test]
fn the_stop_constructor_also_works_at_run_time() {
    let row = LimitRow::stop(
        "kit.runtime",
        Quantity::Count(5),
        Quantity::Count(1),
        Quantity::Count(9),
        RowValue::ByRequest,
        Remedy::Narrow,
    );
    assert_eq!(
        (row.unit, row.kind, row.remedy),
        (Unit::Count, RowKind::Stop, Remedy::Narrow)
    );
    assert_eq!(row.value, RowValue::ByRequest);
}
