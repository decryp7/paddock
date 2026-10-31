//! The widget state envelope every surface renders the same way.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::PlacementId;

/// An action a state offers ("Pin counters"); the id is the app's, the label is shown on the button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WidgetAction {
    /// The app-defined action id.
    pub id: String,
    /// The button text: verb plus object.
    pub label: String,
}

/// One widget's value as the engine reports it. `V` is the app's value type. Loading is not on the wire: it is the UI's
/// state before the first value arrives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "state"
)]
pub enum WidgetState<V> {
    /// A value, with when it was read and whether it is out of date.
    Ready {
        /// The app's value.
        value: V,
        /// When the value was read, in milliseconds since the Unix epoch.
        #[ts(type = "number | null")]
        as_of: Option<i64>,
        /// The value is older than its refresh allows.
        stale: bool,
    },
    /// Nothing to show: what, and a next step.
    Empty {
        /// What there is none of.
        title: String,
        /// One line of detail.
        detail: String,
        /// The next step.
        action: Option<WidgetAction>,
    },
    /// The value could not be read.
    Error {
        /// What failed, in plain words.
        title: String,
        /// Why, when known.
        cause: Option<String>,
        /// Whether Retry is offered.
        retry: bool,
        /// Another next step.
        action: Option<WidgetAction>,
    },
}

/// The state of one placement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlacementState<V> {
    /// The placement.
    pub id: PlacementId,
    /// Its state.
    pub state: WidgetState<V>,
}
