//! Where a reading came from (Dyno helper-wiring plan section 3.3, ADR 0027 amended 2026-11-04: helper first, then this
//! computer). Every reply type that carries a reading the administrator helper can give carries a [`ReadVia`], so the page can
//! say who read it and, when nobody could, why and what to do next. App-agent protocol 4 is unreleased, so the fields are
//! appended without a version bump. The wire is postcard: a `via` is always written, never skipped.

use serde::{Deserialize, Serialize};

/// The one sentence that tells a person how to get a reading only the helper can give. Shared by every reading (rule 55).
pub const NEEDS_ADMIN: &str = "Turn on Administrator readings in Settings so the helper can read this.";

/// What to do about a reading that could not be given. A closed set; family ADR 0052 defines the shared type in `paddock-jobs`
/// (WP-B1) and this one is replaced by it then (same variants, minus the ones with no meaning for a reading).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NextStep {
    /// Try again.
    Retry,
    /// Open this Settings section (`"helper"` for Settings > Helper).
    OpenSettings(String),
    /// Update the helper.
    UpdateHelper,
    /// Update Dyno.
    UpdateApp,
    /// Open the log.
    ShowLog,
    /// Nothing can be done, and why.
    None {
        /// The reason, in words.
        why: String,
    },
}

/// Who read a reading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ReadVia {
    /// The administrator helper read it.
    Helper,
    /// Dyno read it with the person's own rights (also the answer for readings that never needed the helper).
    #[default]
    Local,
    /// Not readable without the helper (or only partly): the reason and the next step.
    NeedsHelper {
        /// Why, in words, ending with the shared sentence.
        reason: String,
        /// What to do: open Settings > Helper.
        next: NextStep,
    },
    /// Neither path can read it on this computer (a virtual machine without sensors, a missing tool).
    Unavailable {
        /// Why, in words.
        reason: String,
    },
}

impl ReadVia {
    /// `NeedsHelper` for `why`, with the shared sentence and Settings > Helper as the next step.
    pub fn needs_admin(why: &str) -> ReadVia {
        ReadVia::NeedsHelper {
            reason: format!("{why} {NEEDS_ADMIN}"),
            next: NextStep::OpenSettings("helper".into()),
        }
    }

    /// The reason, for the two kinds that carry one.
    pub fn reason(&self) -> Option<&str> {
        match self {
            ReadVia::NeedsHelper { reason, .. } | ReadVia::Unavailable { reason } => Some(reason),
            ReadVia::Helper | ReadVia::Local => None,
        }
    }

    /// Whether this is the default, `Local`.
    pub fn is_local(&self) -> bool {
        matches!(self, ReadVia::Local)
    }
}

/// Where the parts of one live push came from: the processes, the "Other users" total and the temperatures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LiveVia {
    /// The process list.
    pub processes: ReadVia,
    /// The "Other users" total.
    pub others: ReadVia,
    /// The temperatures.
    pub temperatures: ReadVia,
}
