//! The plugin's checks over a real consumer's generated files: the shell demo's dialog contracts and
//! `paddock-manifest.json` against the plugin and a table with the demo's dialog commands. The demo app itself
//! (`apps/shell-demo/src-tauri`, its own workspace) runs the same checks with its real table and writes the UI's command
//! names; this test keeps the plugin's side of them in this workspace's runs and coverage.

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

use paddock_shell::dialog_commands as shell;

paddock_dialogs::dialogs! {
    /// The demo's own dialogs, as `apps/shell-demo/src-tauri/src/dialogs.rs` declares them (the drift check against the
    /// demo's manifest keeps the two equal).
    static DEMO_DIALOGS, base &[shell::startup::window_pong, shell::startup::window_ready];
    "sd.confirm-reset" => Decision,
    "sd.rename" => Form(Modal) { width: Some(Narrow) },
    "sd.about" => Viewer,
    "sd.inspector" => Tool { width: Some(Px(720)), height: Some(480), commands: &[dialog_callable::demo_status] },
    "sd.choose-style" => Decision { required_choice: true },
    "sd.unsaved" => Decision,
    "pd.settings" => Tool {
        width: Some(Px(880)),
        height: Some(620),
        commands: &[
            prefix(shell::settings::ALL, "settings_"),
            prefix(shell::service::ALL, "service_"),
            shell::service::open_login_items,
            prefix(shell::setup::ALL, "setup_"),
            prefix(shell::packs::ALL, "packs_"),
            shell::channels::channels_get,
            shell::channels::channels_save,
            shell::channels::channels_clear_secret,
            shell::channels::channels_test,
            prefix(shell::storage::ALL, "storage_"),
            shell::mcp::mcp_get,
            shell::mcp::mcp_save,
            shell::mcp::mcp_revoke_key,
            shell::mcp::mcp_audit,
            shell::mcp::mcp_audit_clear,
            shell::dialogs::dialog_open,
            shell::startup::app_relaunch,
        ],
    },
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
        .dialogs(&[&DEMO_DIALOGS])
        .health(vec![
            ServiceSpec::new("sd.engine", "the demo engine"),
            ServiceSpec::new("sd.index", "the search index"),
            ServiceSpec::new("sd.agent", "the demo agent"),
        ])
        .os_notifications(paddock_health::notify::os::SharedOsSettings::default())
        .section(section("sd.general", "General"))
        .section(section("sd.storage", "Storage"))
        .in_memory()
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
    let bad = demo_shell().dialogs(&[&DEMO_DIALOGS]).problems(None);
    assert!(bad[0].contains("do not fit together"), "{bad:?}");
    assert_eq!(
        demo_shell().service_ids(),
        ["sd.engine", "sd.index", "sd.agent"]
    );
    assert_eq!(
        demo_shell().section_ids(),
        ["pd.notifications", "sd.general", "sd.storage"]
    );
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
