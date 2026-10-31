//! The typed errors of an edit and the notes a read leaves behind.

use serde::{Deserialize, Serialize};

ts! {
/// What a limit counts (typed, so the UI and the messages never parse text).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Counted {
    /// Placements on the surface.
    Placements,
    /// The person's saved presets.
    UserPresets,
    /// Placements in one saved preset.
    PresetPlacements,
}

/// What was wrong with a stored layout (shown once).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum NoteCode {
    /// Not a layout at all (not an object, wrong shape): the defaults are in use.
    Damaged,
    /// Larger than the surface's `max_bytes`: the defaults are in use.
    TooLarge,
    /// Saved by a newer version: shown as the defaults, kept unchanged and not saved over (edits are refused).
    Newer {
        /// The stored version.
        version: u32,
    },
    /// Entries that were invalid, unknown or over a limit, each dropped on its own.
    Dropped {
        /// How many.
        count: u32,
    },
}

/// A note with its message for the person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// Machine-readable reason.
    pub code: NoteCode,
    /// The sentence to show.
    pub message: String,
}

/// Why an edit was refused (the message says what to do).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum WidgetError {
    /// No placement has this id.
    #[error("No widget has the id \"{0}\".")]
    UnknownPlacement(String),
    /// No saved or built-in preset has this id.
    #[error("No preset has the id \"{0}\".")]
    UnknownPreset(String),
    /// Two items share an id.
    #[error("Two items share the id \"{0}\"; ids must be unique.")]
    DuplicateId(String),
    /// A count limit of the surface is reached.
    #[error("At most {max} {} fit; remove one first.", counted.noun())]
    TooMany {
        /// What there are too many of.
        counted: Counted,
        /// The limit.
        max: u16,
    },
    /// A kind is placed as often as its catalogue entry allows.
    #[error("At most {max} of this widget fit; remove one first.")]
    KindLimit {
        /// Its `max_instances`.
        max: u8,
    },
    /// The span is not one the kind allows.
    #[error("This widget can be {} columns wide.", join_spans(.allowed))]
    SpanNotAllowed {
        /// The allowed spans.
        allowed: Vec<u8>,
    },
    /// Fewer placements than the surface needs.
    #[error("At least {min} widgets are needed here.")]
    TooFew {
        /// The surface's `min_placements`.
        min: u16,
    },
    /// The widget's own options (or the edit) are not valid; the text says what to change.
    #[error("{0}")]
    Invalid(String),
    /// A built-in preset cannot be deleted.
    #[error("Built-in presets cannot be changed or deleted.")]
    BuiltinPreset,
    /// There is no edit to undo.
    #[error("There is nothing to undo.")]
    NothingToUndo,
    /// The stored layout is a newer version's and is kept unchanged.
    #[error("This layout was saved by a newer version of the app and is kept unchanged; update the app to edit it.")]
    NewerLayout,
}
}

impl Counted {
    /// The words for the count in a message.
    pub fn noun(self) -> &'static str {
        match self {
            Counted::Placements => "widgets",
            Counted::UserPresets => "saved presets",
            Counted::PresetPlacements => "widgets in a saved preset",
        }
    }
}

fn join_spans(spans: &[u8]) -> String {
    spans
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(" or ")
}
