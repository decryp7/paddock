//! The demo's settings sections: one field of every kind between them, the same schema the browser build plays from
//! `paddock-settings`' fixtures. The shell's generic form shows each (their pages are `DEMO_SETTINGS_PAGES` in the UI).

use paddock_settings::{
    Configurable, EnumOption, FieldKind, SectionOwner, SettingField, SettingsSection, ValidValues,
};
use serde_json::json;
use std::sync::Arc;

/// How the demo greets you.
pub fn general() -> Arc<dyn Configurable> {
    let section = SettingsSection::new("sd.general", "General", SectionOwner::App)
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
    paddock_settings::section(section, applied("sd.general"))
}

/// Where the demo keeps its data.
pub fn storage() -> Arc<dyn Configurable> {
    let section = SettingsSection::new("sd.storage", "Storage", SectionOwner::App)
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
                min_bytes: 1 << 20,
                max_bytes: 1 << 30,
            },
            64 << 20,
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
    paddock_settings::section(section, applied("sd.storage"))
}

/// The demo has nothing to reconfigure: a change is logged (keys only, never values) and takes effect.
fn applied(
    id: &'static str,
) -> impl Fn(&ValidValues) -> Result<(), paddock_settings::ApplyError> + Send + Sync + 'static {
    move |_values| {
        log::info!(target: "shell_demo", Section = id; "Settings applied");
        Ok(())
    }
}
