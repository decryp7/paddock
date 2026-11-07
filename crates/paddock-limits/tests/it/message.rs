use crate::*;
use paddock_limits::{LimitExceeded, Registry, Scope};

static STOP_ROWS: [LimitRow; 4] = [
    LimitRow {
        id: LimitId::new("plugin.memory"),
        unit: Unit::Bytes,
        default: Quantity::Bytes(1024 << 20),
        min: Quantity::Bytes(64 << 20),
        max: Quantity::Bytes(4096 << 20),
        value: RowValue::Setting {
            path: SettingPath::new("performance.pluginMemoryMb"),
        },
        kind: RowKind::Stop,
        selections: &[],
        remedy: Remedy::RaiseSetting {
            path: SettingPath::new("performance.pluginMemoryMb"),
        },
    },
    LimitRow {
        id: LimitId::new("ipc.frame"),
        unit: Unit::Bytes,
        default: Quantity::Bytes(16 << 20),
        min: Quantity::Bytes(16 << 20),
        max: Quantity::Bytes(16 << 20),
        value: RowValue::Hard,
        kind: RowKind::Stop,
        selections: &[],
        remedy: Remedy::Narrow,
    },
    LimitRow {
        id: LimitId::new("net.retry"),
        unit: Unit::Count,
        default: Quantity::Count(3),
        min: Quantity::Count(3),
        max: Quantity::Count(3),
        value: RowValue::Hard,
        kind: RowKind::Stop,
        selections: &[],
        remedy: Remedy::Retry,
    },
    LimitRow {
        id: LimitId::new("query.hits"),
        unit: Unit::Count,
        default: Quantity::Count(10_000),
        min: Quantity::Count(10_000),
        max: Quantity::Count(10_000),
        value: RowValue::ByRequest,
        kind: RowKind::Both,
        selections: &[SelectionKind::First],
        remedy: Remedy::ShowMore,
    },
];

fn reg() -> Registry {
    Registry::new(&STOP_ROWS)
}

#[test]
fn memory_kill_names_the_value_the_limit_and_the_setting() {
    let e = LimitExceeded::new(
        LimitId::new("plugin.memory"),
        Quantity::Bytes(1024 << 20),
        Scope::Plugin,
    )
    .reaching(Quantity::Bytes(1210 << 20));
    let text = e.message(&reg());
    assert_eq!(
        text,
        "The plugin stopped: it reached 1,210 MiB; the limit (plugin.memory) is 1 GiB. Raise it in Settings \u{2192} performance.pluginMemoryMb."
    );
}

#[test]
fn each_remedy_has_its_sentence() {
    let at = |id: &'static str| {
        LimitExceeded::new(LimitId::new(id), Quantity::Count(1), Scope::Query).message(&reg())
    };
    assert!(at("ipc.frame").ends_with("Narrow the query or the input."));
    assert!(at("net.retry").ends_with("Try again."));
    assert!(at("query.hits").ends_with("Show more."));
}

#[test]
fn no_remedy_without_a_row_and_no_reached_without_a_value() {
    let e = LimitExceeded::new(
        LimitId::new("missing.row"),
        Quantity::Millis(108_000),
        Scope::Import,
    );
    assert_eq!(
        e.message(&reg()),
        "The import stopped: the limit (missing.row) is 108 s."
    );
    assert_eq!(e.to_string(), e.message(&Registry::new(&[])));
    let none = LimitExceeded::new(BOUNDED_CUTS.id, Quantity::Count(31), Scope::Query);
    assert_eq!(
        none.message(&Registry::new(std::slice::from_ref(&BOUNDED_CUTS))),
        "The query stopped: the limit (bounded.cuts) is 31."
    );
    let err: &dyn std::error::Error = &e;
    assert!(err.to_string().contains("missing.row"));
}
