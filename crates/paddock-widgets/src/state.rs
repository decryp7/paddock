//! The widget state envelope every surface renders the same way.

use serde::{Deserialize, Serialize};

use crate::id::PlacementId;

ts! {
/// A next step the app offers (its own closed action enum `A`, e.g. "open the alarms page", "retry the backup check").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetAction<A> {
    /// The app's action.
    pub action: A,
    /// Verb plus object, sentence case: "Pin counters", "Retry".
    pub label: String,
}

/// How far a job behind a widget has come (ER-18: a growing count when the total is unknown, never a static 0).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Progress {
    /// A growing count of finished items; the total is not known.
    Count {
        /// Items finished so far.
        #[cfg_attr(any(test, feature = "ts"), ts(type = "number"))]
        done: u64,
        /// What is counted: "volumes", "files".
        unit: String,
    },
    /// Finished items out of a known total.
    Fraction {
        /// Items finished so far.
        #[cfg_attr(any(test, feature = "ts"), ts(type = "number"))]
        done: u64,
        /// All items.
        #[cfg_attr(any(test, feature = "ts"), ts(type = "number"))]
        total: u64,
        /// What is counted.
        unit: String,
    },
}

/// One widget's value as the engine reports it. Loading is not on the wire: it is the UI's state before the first value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "state")]
pub enum WidgetState<V, A> {
    /// A value, read at `as_of` (ms since the epoch); `stale_since` is set when the source has not refreshed it in time
    /// (the UI says "Updated 4 min ago" and dims nothing else).
    Ready {
        /// The app's value.
        value: V,
        /// When the value was read, in milliseconds since the Unix epoch.
        #[cfg_attr(any(test, feature = "ts"), ts(type = "number"))]
        as_of: i64,
        /// Since when the value has been out of date, in milliseconds since the Unix epoch.
        #[cfg_attr(any(test, feature = "ts"), ts(type = "number | null"))]
        stale_since: Option<i64>,
    },
    /// Work is running for this widget (a check, a job): phase-labelled progress, never timed out by the UI (ER-19);
    /// `last` is the previous value, still shown under the progress when there is one.
    Working {
        /// What is running.
        title: String,
        /// The current phase.
        phase: String,
        /// How far it has come.
        progress: Progress,
        /// The previous value, still shown.
        last: Option<V>,
        /// How to cancel it, when it can be cancelled.
        cancel: Option<WidgetAction<A>>,
    },
    /// Nothing to show; `title` starts with what the person gains, `next` is the next step.
    Empty {
        /// What there is none of.
        title: String,
        /// One line of detail.
        detail: String,
        /// The next step.
        next: Option<WidgetAction<A>>,
        /// `true`: the empty state means "all good" (no active alarms), drawn as one calm row with a success check.
        /// `false` (the default): "nothing here yet", drawn with the info icon and the next step.
        #[serde(default)]
        calm: bool,
    },
    /// The source failed. A busy source is `Working` or a stale `Ready`, never an error.
    Error {
        /// What failed, in plain words.
        title: String,
        /// Why, when known.
        cause: Option<String>,
        /// The next step, such as Retry.
        next: Option<WidgetAction<A>>,
    },
}

/// A widget's state, by placement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementState<V, A> {
    /// The placement.
    pub id: PlacementId,
    /// Its state.
    pub state: WidgetState<V, A>,
}

/// How serious a surface notice is (always drawn with its icon and word).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tone {
    /// For information.
    Info,
    /// Something needs attention.
    Warning,
    /// Something is broken.
    Danger,
}

/// One message for the whole surface instead of one per widget (e.g. Dyno's "Agent stopped" with Restart agent).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceNotice<A> {
    /// How serious it is.
    pub tone: Tone,
    /// What happened, in plain words.
    pub title: String,
    /// One line of detail.
    pub detail: String,
    /// The next step.
    pub action: Option<WidgetAction<A>>,
}

/// All values of a surface at once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceValues<V, A> {
    /// A message for the whole surface.
    pub notice: Option<SurfaceNotice<A>>,
    /// Each placement's state.
    pub placements: Vec<PlacementState<V, A>>,
}
}
