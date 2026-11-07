use paddock_limits::{
    Bounded, CutRow, FieldPath, First, LimitExceeded, LimitId, LimitRow, Quantity, Registry,
    Remedy, RowKind, RowValue, Scope, Selection, SelectionKind, SettingPath, Total, Unit,
};
use paddock_testkit::limit_case;
use paddock_testkit::limits::{assert_all_exercised, exercised};

static ROWS: [LimitRow; 3] = [
    LimitRow {
        id: LimitId::new("kit.memory"),
        unit: Unit::Bytes,
        default: Quantity::Bytes(64 << 20),
        min: Quantity::Bytes(1 << 20),
        max: Quantity::Bytes(1 << 30),
        value: RowValue::Setting {
            path: SettingPath::new("performance.kitMemory"),
        },
        kind: RowKind::Stop,
        selections: &[],
        remedy: Remedy::RaiseSetting {
            path: SettingPath::new("performance.kitMemory"),
        },
    },
    LimitRow {
        id: LimitId::new("kit.frame"),
        unit: Unit::Bytes,
        default: Quantity::Bytes(16 << 20),
        min: Quantity::Bytes(16 << 20),
        max: Quantity::Bytes(16 << 20),
        value: RowValue::Hard,
        kind: RowKind::Stop,
        selections: &[],
        remedy: Remedy::None,
    },
    LimitRow {
        id: LimitId::new("kit.rows"),
        unit: Unit::Count,
        default: Quantity::Count(3),
        min: Quantity::Count(1),
        max: Quantity::Count(10),
        value: RowValue::Hard,
        kind: RowKind::Cut,
        selections: &[SelectionKind::First],
        remedy: Remedy::ShowMore,
    },
];
const REGISTRY: Registry = Registry::new(&ROWS);
const ROW: CutRow<First> = CutRow::of(&ROWS[2]);

fn rows(n: usize) -> Bounded<Vec<usize>> {
    Bounded::first(0..n, &ROW, FieldPath::lit("rows"))
}

#[test]
fn cases_in_both_forms_pass_and_are_recorded() {
    limit_case!(
        stop,
        REGISTRY,
        "kit.memory",
        got = LimitExceeded::new(
            LimitId::new("kit.memory"),
            Quantity::Bytes(64 << 20),
            Scope::Plugin
        )
        .reaching(Quantity::Bytes(70 << 20)),
        limit = Quantity::Bytes(64 << 20),
        setting = Some("performance.kitMemory")
    );
    limit_case!(
        stop,
        REGISTRY,
        "kit.frame",
        got = LimitExceeded::new(
            LimitId::new("kit.frame"),
            Quantity::Bytes(16 << 20),
            Scope::Process
        ),
        limit = Quantity::Bytes(16 << 20),
        setting = None
    );
    limit_case!(
        cut,
        REGISTRY,
        "kit.rows",
        at = rows(3),
        over = rows(4),
        of = "rows",
        shown = 3,
        total = Total::AtLeast { n: 4 },
        selection = Selection::First { from: 0 }
    );
    assert!(exercised().contains(&"kit.rows"));
    assert_all_exercised(&REGISTRY);
}

#[test]
#[should_panic(expected = "without a limit_case!")]
fn a_row_without_a_case_fails_the_completeness_check() {
    static EXTRA: [LimitRow; 1] = [LimitRow {
        id: LimitId::new("kit.never_run"),
        ..ROWS[1]
    }];
    assert_all_exercised(&Registry::new(&EXTRA));
}

#[test]
#[should_panic(expected = "another value")]
fn a_wrong_value_fails() {
    limit_case!(
        stop,
        REGISTRY,
        "kit.frame",
        got = LimitExceeded::new(
            LimitId::new("kit.frame"),
            Quantity::Bytes(2),
            Scope::Process
        ),
        limit = Quantity::Bytes(1),
        setting = None
    );
}

#[test]
#[should_panic(expected = "Setting path differs")]
fn a_setting_the_row_does_not_have_fails() {
    limit_case!(
        stop,
        REGISTRY,
        "kit.frame",
        got = LimitExceeded::new(
            LimitId::new("kit.frame"),
            Quantity::Bytes(16 << 20),
            Scope::Process
        ),
        limit = Quantity::Bytes(16 << 20),
        setting = Some("a.b")
    );
}

#[test]
#[should_panic(expected = "must not be cut")]
fn a_cut_at_the_limit_fails() {
    limit_case!(
        cut,
        REGISTRY,
        "kit.rows",
        at = rows(4),
        over = rows(5),
        of = "rows",
        shown = 3,
        total = Total::AtLeast { n: 4 },
        selection = Selection::First { from: 0 }
    );
}

#[test]
#[should_panic(expected = "exactly one cut")]
fn data_over_the_limit_without_a_cut_fails() {
    limit_case!(
        cut,
        REGISTRY,
        "kit.rows",
        at = rows(3),
        over = rows(2),
        of = "rows",
        shown = 3,
        total = Total::AtLeast { n: 4 },
        selection = Selection::First { from: 0 }
    );
}

#[test]
#[should_panic(expected = "shown")]
fn wrong_numbers_fail() {
    limit_case!(
        cut,
        REGISTRY,
        "kit.rows",
        at = rows(3),
        over = rows(4),
        of = "rows",
        shown = 2,
        total = Total::AtLeast { n: 4 },
        selection = Selection::First { from: 0 }
    );
}

#[test]
#[should_panic(expected = "stop row")]
fn the_wrong_form_for_a_row_fails() {
    limit_case!(
        cut,
        REGISTRY,
        "kit.frame",
        at = rows(3),
        over = rows(4),
        of = "rows",
        shown = 3,
        total = Total::AtLeast { n: 4 },
        selection = Selection::First { from: 0 }
    );
}

#[test]
#[should_panic(expected = "cut row")]
fn the_wrong_form_for_a_cut_row_fails() {
    limit_case!(
        stop,
        REGISTRY,
        "kit.rows",
        got = LimitExceeded::new(LimitId::new("kit.rows"), Quantity::Count(3), Scope::Query),
        limit = Quantity::Count(3),
        setting = None
    );
}

#[test]
#[should_panic(expected = "not in the registry")]
fn an_unknown_id_fails() {
    limit_case!(
        stop,
        REGISTRY,
        "kit.unknown",
        got = LimitExceeded::new(
            LimitId::new("kit.unknown"),
            Quantity::Count(3),
            Scope::Query
        ),
        limit = Quantity::Count(3),
        setting = None
    );
}
