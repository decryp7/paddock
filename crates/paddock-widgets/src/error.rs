//! The typed errors of an edit and the notes a read leaves behind.

use serde::{Deserialize, Serialize};

use crate::grid::{Block, Dir};
use crate::place::StepsInfo;

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
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
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
    #[error("{}", too_many(*.counted, *.max, .noun))]
    TooMany {
        /// What there are too many of.
        counted: Counted,
        /// The limit.
        max: u16,
        /// The surface's noun ("popup").
        noun: String,
    },
    /// A kind is placed as often as its catalogue entry allows.
    #[error("At most {max} of this widget {} in the {noun}; remove one first.", fits(u16::from(*.max)))]
    KindLimit {
        /// Its `max_instances`.
        max: u8,
        /// The surface's noun.
        noun: String,
    },
    /// The span is not one the kind allows.
    #[error("This widget can be {} columns wide.", join_spans(.allowed))]
    SpanNotAllowed {
        /// The allowed spans.
        allowed: Vec<u8>,
    },
    /// The size is not one the kind allows.
    #[error("This widget can be {} columns wide and {} rows tall.", steps_text(.widths), steps_text(.heights))]
    SizeNotAllowed {
        /// Allowed widths.
        widths: StepsInfo,
        /// Allowed heights.
        heights: StepsInfo,
    },
    /// An arrangement gesture that changes nothing, and why.
    #[error("{}", block_text(.0))]
    Blocked(Block),
    /// The edit was computed on an older revision of the layout (another window edited meanwhile): reload the view.
    #[error("The layout changed in another window; it was reloaded.")]
    Stale,
    /// There is no undone edit to redo.
    #[error("There is nothing to redo.")]
    NothingToRedo,
    /// Fewer placements than the surface needs.
    #[error("At least {min} {} needed in the {noun}.", if *.min == 1 { "widget is" } else { "widgets are" })]
    TooFew {
        /// The surface's `min_placements`.
        min: u16,
        /// The surface's noun.
        noun: String,
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
    /// The words for the count in a message, singular for 1.
    pub fn noun(self, count: u16) -> &'static str {
        match (self, count == 1) {
            (Counted::Placements, true) => "widget",
            (Counted::Placements, false) => "widgets",
            (Counted::UserPresets, true) => "saved preset",
            (Counted::UserPresets, false) => "saved presets",
            (Counted::PresetPlacements, true) => "widget in a saved preset",
            (Counted::PresetPlacements, false) => "widgets in a saved preset",
        }
    }
}

fn fits(count: u16) -> &'static str {
    if count == 1 { "fits" } else { "fit" }
}

fn too_many(counted: Counted, max: u16, noun: &str) -> String {
    format!(
        "At most {max} {} {} in the {noun}; remove one first.",
        counted.noun(max),
        fits(max)
    )
}

fn join_spans(spans: &[u8]) -> String {
    spans
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(" or ")
}

fn steps_text(s: &StepsInfo) -> String {
    match s {
        StepsInfo::Range { min, max } if min == max => min.to_string(),
        StepsInfo::Range { min, max } => format!("{min} to {max}"),
        StepsInfo::Only { values } => join_spans(values),
    }
}

fn block_text(b: &Block) -> String {
    match b {
        Block::Edge(dir) => format!(
            "Already at the {} edge.",
            match dir {
                Dir::Left => "left",
                Dir::Right => "right",
                Dir::Up => "top",
                Dir::Down => "bottom",
            }
        ),
        Block::Largest => "Can't get any larger.".into(),
        Block::Smallest => "Can't get any smaller.".into(),
        Block::Unchanged => "Nothing to change.".into(),
    }
}
