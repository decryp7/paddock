//! The family's apps and what each one accepts: the hand-off contract of `docs/ecosystem.md`, written once.

use crate::error::SiblingError;

/// What an app accepts from another app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accepts {
    /// Files to import. `extensions` (lowercase, no dot) limits them; empty accepts any file.
    Import {
        /// The accepted extensions.
        extensions: &'static [&'static str],
    },
    /// An item to show, of these kinds.
    Show {
        /// The accepted kinds (lowercase words and dashes).
        kinds: &'static [&'static str],
    },
}

/// One app of the family: how it is found and what it accepts. Every app can be opened ([`crate::Handoff::Open`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppSpec {
    /// The id other apps use (`debrief`).
    pub id: &'static str,
    /// The name the user knows (`Debrief`), also the macOS bundle and Windows folder name.
    pub name: &'static str,
    /// The command-line program of the app (also the main executable inside the macOS bundle).
    pub cli: &'static str,
    /// What the program was called before a rename; found without [`AppSpec::cli`], it means the app is too old.
    pub legacy_cli: Option<&'static str>,
    /// The app's own URL scheme, which its command line accepts as a deep link (`debrief://show/job/42`).
    pub scheme: &'static str,
    /// What it accepts besides being opened.
    pub accepts: &'static [Accepts],
}

/// The apps of the family that take part in hand-offs, from `docs/ecosystem.md`:
/// job logs and issues open in Debrief (a file), Dyno's installed software becomes Spotter's watch list (a file) and
/// Dyno's machine health shows next to the runners in Pitcrew (an item).
pub const FAMILY: &[AppSpec] = &[
    AppSpec {
        id: "debrief",
        name: "Debrief",
        cli: "debrief",
        legacy_cli: Some("imt"),
        scheme: "debrief",
        accepts: &[Accepts::Import { extensions: &[] }],
    },
    AppSpec {
        id: "pitcrew",
        name: "Pitcrew",
        cli: "pitcrew",
        legacy_cli: None,
        scheme: "pitcrew",
        accepts: &[Accepts::Show {
            kinds: &["machine"],
        }],
    },
    AppSpec {
        id: "pitwall",
        name: "Pitwall",
        cli: "pitwall",
        legacy_cli: None,
        scheme: "pitwall",
        accepts: &[],
    },
    AppSpec {
        id: "dyno",
        name: "Dyno",
        cli: "dyno",
        legacy_cli: None,
        scheme: "dyno",
        accepts: &[],
    },
    AppSpec {
        id: "spotter",
        name: "Spotter",
        cli: "spotter",
        legacy_cli: None,
        scheme: "spotter",
        accepts: &[Accepts::Import {
            extensions: &["json"],
        }],
    },
];

impl AppSpec {
    /// The app called `id` (case-insensitive), or [`SiblingError::UnknownApp`]: an app that is not in the family is never
    /// launched.
    pub fn lookup(id: &str) -> Result<&'static AppSpec, SiblingError> {
        FAMILY
            .iter()
            .find(|a| a.id.eq_ignore_ascii_case(id))
            .ok_or_else(|| SiblingError::UnknownApp(id.chars().take(40).collect()))
    }

    /// The accepted extensions for imports, `None` when the app takes no imports, `Some(&[])` for any file.
    pub fn import_extensions(&self) -> Option<&'static [&'static str]> {
        self.accepts.iter().find_map(|a| match a {
            Accepts::Import { extensions } => Some(*extensions),
            Accepts::Show { .. } => None,
        })
    }

    /// The kinds of item it can show, empty when it shows none.
    pub fn show_kinds(&self) -> &'static [&'static str] {
        self.accepts
            .iter()
            .find_map(|a| match a {
                Accepts::Show { kinds } => Some(*kinds),
                Accepts::Import { .. } => None,
            })
            .unwrap_or(&[])
    }
}
