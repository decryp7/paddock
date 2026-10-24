//! The shell's own sections, registered by the app next to its own (`settings.register(shell::appearance())`).

use crate::schema::{EnumOption, FieldKind, SectionOwner, SettingField, SettingsSection};

/// The id of the shell's Appearance section (and of its settings page in the UI).
pub const APPEARANCE: &str = "pd.appearance";

/// Appearance: theme (`system`, `light`, `dark`) and density (`comfortable`, `standard`, `compact`) of every window.
/// The UI applies them in each window when they change (`@decryp7/paddock-ui/shell/settings`).
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
}

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
        // `SU_THEMES` and `SU_DENSITIES` of @decryp7/paddock-ui.
        assert_eq!(values("theme"), ["system", "light", "dark"]);
        assert_eq!(values("density"), ["comfortable", "standard", "compact"]);
    }
}
