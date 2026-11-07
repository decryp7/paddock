use paddock_limits::{
    Bounded, CutRow, FieldPath, First, LimitExceeded, LimitId, LimitRow, Quantity, Registry,
    Remedy, RowKind, RowValue, Scope, Selection, SelectionKind, SettingPath, Total, Unit,
};
use paddock_testkit::limits::assert_cases_match;
use paddock_testkit::{limit_case, limit_cases};

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

fn exceeded(id: &'static str, limit: Quantity, scope: Scope) -> LimitExceeded {
    LimitExceeded::new(LimitId::new(id), limit, scope)
}

// One #[test] per case, plus the test that the cases and the registry's rows are the same set.
limit_cases! { REGISTRY;
    memory_cap: "kit.memory" => stop(
        got = exceeded("kit.memory", Quantity::Bytes(64 << 20), Scope::Plugin).reaching(Quantity::Bytes(70 << 20)).unwrap(),
        limit = Quantity::Bytes(64 << 20),
        setting = Some("performance.kitMemory")
    ),
    frame_size: "kit.frame" => stop(
        got = exceeded("kit.frame", Quantity::Bytes(16 << 20), Scope::Process),
        limit = Quantity::Bytes(16 << 20),
        setting = None
    ),
    row_list: "kit.rows" => cut(
        at = rows(3),
        over = rows(4),
        of = "rows",
        shown = 3,
        total = Total::AtLeast { n: 4 },
        selection = Selection::First { from: 0 }
    ),
}

#[test]
fn the_case_list_is_checked_against_the_rows_both_ways() {
    assert_cases_match(&REGISTRY, LIMIT_CASE_IDS);
    // A row without a case.
    let missing =
        std::panic::catch_unwind(|| assert_cases_match(&REGISTRY, &["kit.memory", "kit.frame"]));
    assert!(panic_text(missing).contains("without a case: [\"kit.rows\"]"));
    // A case without a row.
    let extra = std::panic::catch_unwind(|| {
        assert_cases_match(
            &REGISTRY,
            &["kit.memory", "kit.frame", "kit.rows", "kit.ghost"],
        )
    });
    assert!(panic_text(extra).contains("without a row: [\"kit.ghost\"]"));
}

fn panic_text(r: std::thread::Result<()>) -> String {
    let payload = r.expect_err("it must panic");
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_default()
}

#[test]
#[should_panic(expected = "another value")]
fn a_wrong_value_fails() {
    limit_case!(
        stop,
        REGISTRY,
        "kit.frame",
        got = exceeded("kit.frame", Quantity::Bytes(2), Scope::Process),
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
        got = exceeded("kit.frame", Quantity::Bytes(16 << 20), Scope::Process),
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
        got = exceeded("kit.rows", Quantity::Count(3), Scope::Query),
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
        got = exceeded("kit.unknown", Quantity::Count(3), Scope::Query),
        limit = Quantity::Count(3),
        setting = None
    );
}
