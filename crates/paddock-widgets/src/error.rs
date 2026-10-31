//! The typed errors of an edit and the notes a read leaves behind.

use serde::Serialize;
use ts_rs::TS;

use crate::id::MAX_ID;

/// Why an edit or a layout is not valid. The message says what happened and what to do (design guide §10).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum WidgetError {
    /// A placement id is empty, too long or has characters other than `a-z`, `0-9` and `-`.
    #[error("Use 1 to {MAX_ID} characters of a-z, 0-9 and hyphens for an id.")]
    BadId,
    /// Two placements or presets share an id.
    #[error("Two items share the id \"{0}\"; ids must be unique.")]
    DuplicateId(String),
    /// No placement has this id.
    #[error("There is no widget \"{0}\" in this layout; reload the layout and try again.")]
    UnknownPlacement(String),
    /// A count limit of the surface is reached.
    #[error("At most {max} {what} fit; remove one first.")]
    TooMany {
        /// What there are too many of ("widgets", "saved layouts", "widgets in a saved layout").
        what: String,
        /// The limit.
        max: u16,
    },
    /// A kind is placed as often as its catalogue entry allows.
    #[error("At most {max} of \"{kind}\" fit; remove one first.")]
    KindLimit {
        /// The kind's title.
        kind: String,
        /// Its `max_instances`.
        max: u8,
    },
    /// The span is not one the kind allows.
    #[error("\"{kind}\" can be {} column(s) wide; choose one of those.", join_spans(.spans))]
    SpanNotAllowed {
        /// The kind's title.
        kind: String,
        /// The allowed spans.
        spans: Vec<u8>,
    },
    /// The widget's own options (or the edit) are not valid; the text says what to change.
    #[error("{0}")]
    Invalid(String),
    /// No saved or built-in layout has this id.
    #[error("There is no layout \"{0}\"; choose another.")]
    NotAPreset(String),
    /// A built-in layout cannot be deleted.
    #[error("Built-in layouts can't be deleted; save your own layout instead.")]
    BuiltinPreset,
    /// There is no edit to undo.
    #[error("There is nothing to undo.")]
    NothingToUndo,
}

fn join_spans(spans: &[u8]) -> String {
    spans
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(" or ")
}

/// What was wrong when a layout was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "code"
)]
pub enum NoteCode {
    /// The stored layout is damaged; the defaults are used.
    Damaged,
    /// The layout was saved by a newer version; the defaults are used.
    Newer,
    /// The stored layout is larger than the surface allows; the defaults are used.
    TooLarge,
    /// Invalid, unknown or surplus entries were dropped, each on its own.
    Dropped {
        /// How many entries were dropped.
        count: u32,
    },
}

/// One thing to tell the person once after reading a layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct Note {
    /// Machine-readable reason.
    #[serde(flatten)]
    #[ts(flatten)]
    pub code: NoteCode,
    /// The sentence to show.
    pub message: String,
}
