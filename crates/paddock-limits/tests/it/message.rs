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
    .reaching(Quantity::Bytes(1210 << 20))
    .unwrap();
    let text = e.message(&reg());
    assert_eq!(
        text,
        "The plugin stopped: it reached 1.2 GiB; the limit (plugin.memory) is 1 GiB. Raise it in Settings \u{2192} performance.pluginMemoryMb."
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

struct Labels;
impl paddock_limits::SettingLabels for Labels {
    fn label(&self, path: SettingPath) -> Option<(&str, &str)> {
        (path.as_str() == "performance.pluginMemoryMb").then_some(("Performance", "Plugin memory"))
    }
}

#[test]
fn the_schema_words_replace_the_dotted_path_when_the_schema_knows_it() {
    let e = LimitExceeded::new(
        LimitId::new("plugin.memory"),
        Quantity::Bytes(1024 << 20),
        Scope::Plugin,
    );
    assert!(
        e.message_with(&reg(), &Labels)
            .ends_with("Raise it in Settings \u{2192} Performance \u{2192} Plugin memory.")
    );
    // A path the schema does not know falls back to the dotted path, which stays the structured log field.
    struct Unknown;
    impl paddock_limits::SettingLabels for Unknown {
        fn label(&self, _: SettingPath) -> Option<(&str, &str)> {
            None
        }
    }
    assert!(
        e.message_with(&reg(), &Unknown)
            .ends_with("performance.pluginMemoryMb.")
    );
    assert_eq!(
        e.setting_path(&reg()),
        Some(SettingPath::new("performance.pluginMemoryMb"))
    );
    assert_eq!(
        LimitExceeded::new(LimitId::new("ipc.frame"), Quantity::Bytes(1), Scope::File)
            .setting_path(&reg()),
        None
    );
    assert_eq!(
        LimitExceeded::new(LimitId::new("missing.id"), Quantity::Bytes(1), Scope::File)
            .setting_path(&reg()),
        None
    );
}

#[test]
fn measured_values_are_rounded_and_limits_stay_exact() {
    assert_eq!(Quantity::Bytes(1_210_123_456).approx(), "1.1 GiB");
    assert_eq!(Quantity::Bytes(3 << 30).approx(), "3 GiB");
    assert_eq!(Quantity::Bytes(70 << 20).approx(), "70 MiB");
    assert_eq!(Quantity::Bytes(1536).approx(), "1.5 KiB");
    assert_eq!(Quantity::Bytes(900).approx(), "900 B");
    assert_eq!(Quantity::Millis(1_540).approx(), "1.5 s");
    assert_eq!(Quantity::Millis(250).approx(), "250 ms");
    assert_eq!(Quantity::Count(1_204).approx(), "1,204");
    assert_eq!(
        Quantity::Bytes(1_210_123_456).to_string(),
        "1,210,123,456 B"
    );
}

#[test]
fn a_message_for_a_person_shows_the_value_not_the_id() {
    let e = LimitExceeded::new(
        LimitId::new("plugin.memory"),
        Quantity::Bytes(1024 << 20),
        Scope::Plugin,
    );
    let for_logs = e.message(&reg());
    let for_people = e.message_with(&reg(), &Labels);
    assert!(for_logs.contains("(plugin.memory)"), "{for_logs}");
    assert!(
        for_people.contains("the limit is 1 GiB.") && !for_people.contains("plugin.memory"),
        "{for_people}"
    );
}
