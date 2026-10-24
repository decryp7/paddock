//! The shell demo's Rust side, as an app keeps it (`ui/projects/shell-demo` has no `src-tauri` yet; this test stands in
//! for it): its own command table, the shell plugin with its services and sections, and the checks every app runs over
//! its generated files: the dialog contracts and `paddock-manifest.json` against the plugin and the app's table.
//! It also writes the demo's command names (`src/app/generated/app-commands.ts`), as `paddock-commands` writes an app's.

use paddock_commands::testing::{WriteMode, sync_generated};
use paddock_health::ServiceSpec;
use paddock_settings::{FieldKind, SectionOwner, SettingField, SettingsSection};
use paddock_shell::{AppIdentity, Builder};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Manager;
use tauri::test::MockRuntime;

mod demo {
    /// What the demo's inspector shows.
    #[tauri::command]
    pub async fn demo_status() -> Result<String, String> {
        Ok("running".into())
    }

    /// Forgets the greetings.
    #[tauri::command]
    pub async fn demo_reset() -> Result<(), String> {
        Ok(())
    }
}

paddock_commands::command_table! {
    /// The demo's own commands (the shell's are the plugin's).
    static DEMO_COMMANDS, macro demo_handler;
    Mutation [MAIN] => demo::demo_reset,
    Query [MAIN | DIALOG] => demo::demo_status,
}

fn demo_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../ui/projects/shell-demo")
}

fn read(name: &str) -> String {
    std::fs::read_to_string(demo_dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn section(id: &str, title: &str) -> Arc<SettingsSection> {
    Arc::new(
        SettingsSection::new(id, title, SectionOwner::App).field(SettingField::new(
            "enabled",
            "Enabled",
            FieldKind::Bool,
            true,
        )),
    )
}

/// The demo's shell, as its `main.rs` would build it.
fn demo_shell() -> Builder<MockRuntime> {
    Builder::new(AppIdentity::new("shell-demo", "Paddock demo", "0.0.0"))
        .app_commands(&DEMO_COMMANDS)
        .dialog_contracts(read("dialog-contracts.json"))
        .health(vec![
            ServiceSpec::new("sd.engine", "the demo engine"),
            ServiceSpec::new("sd.index", "the search index"),
        ])
        .section(section("sd.general", "General"))
        .section(section("sd.storage", "Storage"))
        .in_memory()
}

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p paddock-shell --test demo";

#[test]
fn the_demos_command_names_are_generated_from_its_table() {
    let text = paddock_commands::ts::names_module(
        &DEMO_COMMANDS,
        "APP_COMMANDS",
        "AppCommand",
        REGENERATE,
    )
    .unwrap();
    let path = demo_dir().join("src/app/generated/app-commands.ts");
    sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn the_demos_contracts_and_manifest_agree_with_the_plugin_and_its_table() {
    let manifest = read("paddock-manifest.json");
    assert_eq!(demo_shell().problems(Some(&manifest)), Vec::<String>::new());
}

#[test]
fn a_drifted_demo_is_reported() {
    let manifest = read("paddock-manifest.json");
    let missing_service =
        demo_shell().health(vec![ServiceSpec::new("sd.engine", "the demo engine")]);
    let p = missing_service.problems(Some(&manifest));
    assert!(p.iter().any(|x| x.contains("sd.index")), "{p:?}");
    let renamed = manifest.replace("\"about_info\"", "\"about_infos\"");
    let p = demo_shell().problems(Some(&renamed));
    assert!(p.iter().any(|x| x.contains("about_infos")), "{p:?}");
    let bad = demo_shell().dialog_contracts("{").problems(None);
    assert!(bad[0].contains("not valid"), "{bad:?}");
    assert_eq!(demo_shell().service_ids(), ["sd.engine", "sd.index"]);
    assert_eq!(demo_shell().section_ids(), ["sd.general", "sd.storage"]);
}

#[test]
fn the_demo_app_builds_with_the_plugin_next_to_its_own_guarded_handler() {
    let app = tauri::test::mock_builder()
        .plugin(demo_shell().native_menu(false).build())
        .invoke_handler(paddock_commands::tauri::Guard::new(&DEMO_COMMANDS).wrap(demo_handler!()))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    assert_eq!(
        app.handle().state::<AppIdentity>().inner().id,
        "shell-demo",
        "the plugin's set-up ran"
    );
}
