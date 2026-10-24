//! The shell's own sections, registered by the app next to its own (`settings.register(shell::appearance())`).

use crate::schema::{EnumOption, FieldKind, SectionOwner, SettingField, SettingsSection};

/// The id of the shell's Appearance section (and of its settings page in the UI).
pub const APPEARANCE: &str = "pd.appearance";

/// Appearance of every window: theme (`system`, `light`, `dark`), contrast (`system`, `standard`, `more`), motion
/// (`system`, `reduce`, `full`), density (`comfortable`, `standard`, `compact`) and the theme pack (an app's
/// `defineTheme` id, empty for none). The engine is the one source of truth (rule 34); each window's appearance store
/// follows its changes (`@decryp7/paddock-ui/shell/settings`, `settingsAppearanceBackend`).
pub fn appearance() -> SettingsSection {
    let options = |list: &[(&str, &str)]| FieldKind::Enum {
        options: list.iter().map(|(v, l)| EnumOption::new(*v, *l)).collect(),
    };
    SettingsSection::new(APPEARANCE, "Appearance", SectionOwner::Shell)
        .describe("How every window of the app looks. Changes apply at once.")
        .field(
            SettingField::new(
                "theme",
                "Theme",
                options(&[
                    ("system", "Same as the system"),
                    ("light", "Light"),
                    ("dark", "Dark"),
                ]),
                "system",
            )
            .describe("Light or dark, or follow the operating system."),
        )
        .field(
            SettingField::new(
                "contrast",
                "Contrast",
                options(&[
                    ("system", "Same as the system"),
                    ("standard", "Standard"),
                    ("more", "High contrast"),
                ]),
                "system",
            )
            .describe("Stronger borders and no translucency, or follow the operating system."),
        )
        .field(
            SettingField::new(
                "motion",
                "Motion",
                options(&[
                    ("system", "Same as the system"),
                    ("reduce", "Reduced"),
                    ("full", "Full"),
                ]),
                "system",
            )
            .describe("Reduced: fades only, no movement. Or follow the operating system."),
        )
        .field(
            SettingField::new(
                "density",
                "Density",
                options(&[
                    ("comfortable", "Comfortable"),
                    ("standard", "Standard"),
                    ("compact", "Compact"),
                ]),
                "standard",
            )
            .describe("How much space controls and rows take."),
        )
        .field(
            SettingField::new("pack", "Theme pack", FieldKind::Text { max_len: 128, pattern: Some(PACK_ID.into()) }, "")
                .describe("An app's colour pack over the theme; empty for none. Never applied in high contrast.")
                .as_advanced(),
        )
}

/// A theme pack id as the UI writes it (`db.theme.console`), or empty for none.
pub const PACK_ID: &str = r"(?:[a-z][a-z0-9]*(?:\.[a-z0-9]+(?:-[a-z0-9]+)*)+)?";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_appearance_section_is_valid_and_matches_the_ui_values() {
        let s = appearance();
        s.check().unwrap();
        assert_eq!(s.id, APPEARANCE);
        let values = |key: &str| match &s.find(key).unwrap().kind {
            FieldKind::Enum { options } => {
                options.iter().map(|o| o.value.clone()).collect::<Vec<_>>()
            }
            other => panic!("{other:?}"),
        };
        // `SU_THEMES`, `SU_CONTRASTS`, `SU_MOTIONS` and `SU_DENSITIES` of @decryp7/paddock-ui.
        assert_eq!(values("theme"), ["system", "light", "dark"]);
        assert_eq!(values("contrast"), ["system", "standard", "more"]);
        assert_eq!(values("motion"), ["system", "reduce", "full"]);
        assert_eq!(values("density"), ["comfortable", "standard", "compact"]);
        assert_eq!(
            s.fields.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(),
            ["theme", "contrast", "motion", "density", "pack"]
        );
    }

    #[test]
    fn the_theme_pack_takes_an_id_or_nothing() {
        let s = appearance();
        let ok = |v: &str| {
            crate::values::validate_field(s.find("pack").unwrap(), &serde_json::json!(v)).is_ok()
        };
        assert!(ok(""));
        assert!(ok("db.theme.console"));
        assert!(ok("sd.theme.warm-paper"));
        assert!(!ok("Console"));
        assert!(!ok("db.theme.console; x"));
        assert!(!ok(&"a.b".repeat(60)));
    }
}
