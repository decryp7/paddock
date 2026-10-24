//! The generated TypeScript of the service, checked on every test run (a stale file fails CI): the wire types (ts-rs)
//! and the command names, in the `shell/service` entry point of `@decryp7/paddock-ui`, plus the launch-at-login section
//! as `settings_overview` answers it (the shell demo's browser build shows it without a Rust engine).

use crate::backend::Mechanism;
use crate::control::{ServiceCommandError, ServiceErrorCode, ServiceUpdate};
use crate::status::{Condition, ServiceStatus};
use crate::tracker::{RepairNotice, RepairState};
use paddock_commands::testing::{self, WriteMode};
use std::path::{Path, PathBuf};
use ts_rs::TS;

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p paddock-service --lib generated";

fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/shell/service/generated")
}

fn generated_files() -> Vec<(&'static str, String)> {
    let header = format!(
        "// Generated from the Rust wire types of paddock-service (ts-rs). Do not edit: {REGENERATE}.\n\n"
    );
    let decls = [
        Mechanism::decl(),
        Condition::decl(),
        RepairState::decl(),
        RepairNotice::decl(),
        ServiceStatus::decl(),
        ServiceErrorCode::decl(),
        ServiceCommandError::decl(),
        ServiceUpdate::decl(),
    ];
    let types: Vec<String> = decls.into_iter().map(|d| format!("export {d}\n")).collect();
    let consts = format!(
        "\n/** The event every service update is sent on, to every window. */\nexport const SERVICE_EVENT = '{}';\n\n\
/** The id of the launch-at-login settings section (and of its settings page). */\nexport const LOGIN_SECTION = '{}';\n",
        crate::EVENT_NAME,
        crate::LOGIN_SECTION
    );
    let settings = paddock_settings::Settings::in_memory();
    settings
        .register(std::sync::Arc::new(crate::login::launch_at_login_section(
            "Shell Demo",
            true,
        )))
        .unwrap();
    let section = serde_json::to_string_pretty(&settings.overview().sections[0]).unwrap();
    let fixtures = format!(
        "// Generated from paddock-service's own code by its tests (Rust round trips). Do not edit: {REGENERATE}.\n\n\
import type {{ SectionView }} from '@decryp7/paddock-ui/shell/settings';\n\n\
/** The \"Start at login\" settings section at its defaults (app \"Shell Demo\"), as `settings_overview` answers it. */\n\
export const LOGIN_SECTION_FIXTURE: SectionView = {section};\n"
    );
    vec![
        (
            "service.ts",
            format!("{header}{}{consts}", types.join("\n")),
        ),
        ("service-fixtures.ts", fixtures),
        (
            "service-commands.ts",
            paddock_commands::ts::names_module(
                &crate::COMMANDS,
                "SERVICE_COMMANDS",
                "ServiceCommand",
                REGENERATE,
            )
            .unwrap_or_default(),
        ),
    ]
}

#[test]
fn generated_typescript_is_current() {
    let mode = WriteMode::from_env();
    let stale: Vec<String> = generated_files()
        .into_iter()
        .filter_map(|(name, text)| {
            testing::sync_generated(&generated_dir().join(name), &text, mode).err()
        })
        .map(|e| e.to_string())
        .collect();
    assert!(stale.is_empty(), "{}", stale.join("\n"));
}

#[test]
fn the_table_has_the_service_commands() {
    let names: Vec<&str> = crate::COMMANDS.specs().iter().map(|s| s.name()).collect();
    assert_eq!(
        names,
        [
            "service_status",
            "service_repair",
            "service_install",
            "open_login_items"
        ]
    );
    for c in ["service_repair", "service_install", "open_login_items"] {
        assert!(
            crate::COMMANDS.opens_to_dialogs(c),
            "the Settings dialog may {c}"
        );
        assert_eq!(crate::COMMANDS.kind_of(c), paddock_commands::Kind::Mutation);
    }
    assert_eq!(
        crate::COMMANDS.kind_of("service_status"),
        paddock_commands::Kind::Query
    );
}
