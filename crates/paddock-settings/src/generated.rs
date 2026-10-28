//! The generated TypeScript of this crate (checked on every test run: a stale file fails, `PADDOCK_WRITE_TS=1` rewrites
//! it), in the `shell/settings` entry point of `@decryp7/paddock-ui`:
//!
//! - `settings.ts`: the wire types (ts-rs), the event name and the limits.
//! - `settings-fixtures.ts`: an overview with every field kind, a refused change, a change event and a start-up problem,
//!   produced by this crate's own code (Rust round trips), for the UI's specs and the shell demo's in-memory backend.
//! - `settings-commands.ts` (with the `tauri` feature): the adapter's command names.

use crate::schema::{ByteKind, EnumOption, FieldKind, SectionOwner, SettingField, SettingsSection};
use crate::{Settings, file};
use paddock_commands::testing::{WriteMode, sync_generated};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use ts_rs::TS;

const REGENERATE: &str =
    "PADDOCK_WRITE_TS=1 cargo test -p paddock-settings --all-features --lib generated";

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/shell/settings/generated")
}

fn sync(name: &str, text: &str) {
    sync_generated(&dir().join(name), text, WriteMode::from_env())
        .unwrap_or_else(|e| panic!("{e}"));
}

fn types() -> String {
    let decls = [
        crate::SectionOwner::decl(),
        crate::ByteKind::decl(),
        crate::EnumOption::decl(),
        crate::FieldKind::decl(),
        crate::SettingField::decl(),
        crate::SettingsSection::decl(),
        crate::FieldError::decl(),
        crate::LoadProblemKind::decl(),
        crate::LoadProblem::decl(),
        crate::SectionView::decl(),
        crate::SettingsOverview::decl(),
        crate::SetResult::decl(),
        crate::SettingsChanged::decl(),
        crate::SettingsErrorCode::decl(),
        crate::SettingsCommandError::decl(),
    ];
    let body: String = decls
        .into_iter()
        .map(|d| format!("export {d}\n\n"))
        .collect();
    format!(
        "// Generated from the Rust wire types of paddock-settings (ts-rs). Do not edit: {REGENERATE}.\n\n\
/** A stored value: durations are milliseconds, sizes are bytes. */\n\
export type SettingValue = boolean | number | string | string[];\n\n\
{body}\
/** Every window hears it when a change took effect; the payload is a `SettingsChanged`. */\n\
export const SETTINGS_EVENT = '{}';\n\n\
/** The id of the shell's Appearance section and page. */\n\
export const APPEARANCE_SECTION = '{}';\n\n\
/** The limits the schema check enforces. */\n\
export const SETTINGS_LIMITS = {{ textLen: {}, listItems: {}, itemLen: {}, pathLen: {}, fields: {}, idLen: {} }} as const;\n",
        crate::EVENT_NAME,
        crate::shell::APPEARANCE,
        crate::MAX_TEXT_LEN,
        crate::MAX_LIST_ITEMS,
        crate::MAX_ITEM_LEN,
        crate::MAX_PATH_LEN,
        crate::MAX_FIELDS,
        crate::MAX_ID_LEN,
    )
}

/// The shell demo's sections: one field of every kind between them.
fn demo_sections() -> Vec<SettingsSection> {
    let general = SettingsSection::new("sd.general", "General", SectionOwner::App)
        .describe("How the demo greets you.")
        .field(
            SettingField::new(
                "greeting",
                "Greeting",
                FieldKind::Text {
                    max_len: 40,
                    pattern: Some("[A-Za-z ,]+".into()),
                },
                "Hello",
            )
            .describe("Letters, spaces and commas."),
        )
        .field(SettingField::new(
            "style",
            "Style",
            FieldKind::Enum {
                options: vec![
                    EnumOption::new("casual", "Casual"),
                    EnumOption::new("formal", "Formal"),
                ],
            },
            "casual",
        ))
        .field(
            SettingField::new("notify", "Show a notification", FieldKind::Bool, true)
                .describe("Announce each greeting."),
        )
        .field(
            SettingField::new(
                "repeat",
                "Repeat",
                FieldKind::Int {
                    min: 1,
                    max: 10,
                    step: 1,
                    unit: Some("times".into()),
                },
                1,
            )
            .describe("How often one command greets."),
        )
        .field(
            SettingField::new(
                "volume",
                "Volume",
                FieldKind::Float {
                    min: 0.0,
                    max: 1.0,
                    step: 0.1,
                    unit: None,
                },
                0.5,
            )
            .needs_restart(),
        );
    let storage = SettingsSection::new("sd.storage", "Storage", SectionOwner::App)
        .describe("Where the demo keeps its data.")
        .field(
            SettingField::new("folder", "Data folder", FieldKind::Path { dir: true }, "")
                .describe("Empty: the app's folder."),
        )
        .field(SettingField::new(
            "keep",
            "Keep logs for",
            FieldKind::Duration {
                min_ms: 60_000,
                max_ms: 86_400_000,
            },
            3_600_000,
        ))
        .field(SettingField::new(
            "cache",
            "Cache size",
            FieldKind::Size {
                kind: ByteKind::Storage,
                min_bytes: 1_000_000,
                max_bytes: 1_000_000_000,
            },
            64_000_000,
        ))
        .field(
            SettingField::new(
                "ignore",
                "Ignored names",
                FieldKind::StringList { max_items: 20 },
                json!(["*.tmp"]),
            )
            .describe("One per line.")
            .as_advanced(),
        );
    vec![crate::shell::appearance(), general, storage]
}

fn fixtures() -> String {
    let settings = Settings::in_memory();
    for s in demo_sections() {
        settings.register(Arc::new(s)).unwrap();
    }
    let overview = settings.overview();
    let refused = settings
        .set(
            "sd.general",
            &json!({ "greeting": "Hi there!", "repeat": 0 }),
        )
        .unwrap();
    assert!(!refused.applied);
    struct Last(Mutex<Option<crate::SettingsChanged>>);
    impl crate::ChangeSink for Last {
        fn changed(&self, c: &crate::SettingsChanged) {
            *self.0.lock().unwrap() = Some(c.clone());
        }
    }
    let sink = Arc::new(Last(Mutex::new(None)));
    settings.set_sink(sink.clone());
    settings
        .set("sd.general", &json!({ "volume": 0.8 }))
        .unwrap();
    let changed = sink.0.lock().unwrap().clone().unwrap();
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join(crate::FILE_NAME);
    std::fs::write(&path, "{ \"version\": 1, \"sections\": ").unwrap();
    let problem = file::load(&path).problem.unwrap();
    let pretty = |v: serde_json::Value| serde_json::to_string_pretty(&v).unwrap();
    format!(
        "// Generated from paddock-settings' own code by its tests (Rust round trips). Do not edit: {REGENERATE}.\n\n\
import type {{ LoadProblem, SetResult, SettingsChanged, SettingsOverview }} from './settings';\n\n\
/** The shell demo's sections (Appearance, General, Storage: every field kind) at their defaults, as `settings_overview`\n * answers. */\n\
export const SETTINGS_OVERVIEW_FIXTURE: SettingsOverview = {};\n\n\
/** `settings_set` refusing a greeting that does not match its pattern and a repeat below its minimum. */\n\
export const SETTINGS_REFUSED_FIXTURE: SetResult = {};\n\n\
/** The event after the volume (a restart-only field) changed. */\n\
export const SETTINGS_CHANGED_FIXTURE: SettingsChanged = {};\n\n\
/** A damaged settings file at start-up. */\n\
export const SETTINGS_PROBLEM_FIXTURE: LoadProblem = {};\n",
        pretty(serde_json::to_value(&overview).unwrap()),
        pretty(serde_json::to_value(&refused).unwrap()),
        pretty(serde_json::to_value(&changed).unwrap()),
        pretty(serde_json::to_value(&problem).unwrap()),
    )
}

#[test]
fn generated_typescript_is_current() {
    sync("settings.ts", &types());
    sync("settings-fixtures.ts", &fixtures());
}

#[cfg(feature = "tauri")]
#[test]
fn generated_command_names_are_current() {
    let text = paddock_commands::ts::names_module(
        &crate::tauri::COMMANDS,
        "SETTINGS_COMMANDS",
        "SettingsCommand",
        REGENERATE,
    )
    .unwrap();
    sync("settings-commands.ts", &text);
}

#[test]
fn the_demo_sections_cover_every_field_kind() {
    let kinds: std::collections::BTreeSet<String> = demo_sections()
        .iter()
        .flat_map(|s| s.fields.iter())
        .map(|f| {
            serde_json::to_value(&f.kind).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(kinds.len(), 9, "{kinds:?}");
}
