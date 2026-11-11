//! Where a reading came from (Dyno helper-wiring plan section 3.3, ADR 0027 amended 2026-11-04: helper first, then this
//! computer). Every reply type that carries a reading the administrator helper can give carries a [`ReadVia`], so the page can
//! say who read it and, when nobody could, why and what to do next. App-agent protocol 4 is unreleased, so the fields are
//! appended without a version bump. The wire is postcard: a `via` is always written, never skipped.
//!
//! Reasons are sentence-case full sentences ([`sentence`]). A reason never repeats the action: the page offers it as a button
//! ([`NextStep`]); [`NEEDS_ADMIN`] is for text shown without one.

use serde::{Deserialize, Serialize};

/// The one sentence that tells a person how to get a reading only the helper can give, for text shown where there is no button
/// (a log line, a plain `Reading::Unavailable`). Where the page shows the button "Turn on administrator readings", the reason
/// does not carry it as well.
pub const NEEDS_ADMIN: &str = "Turn on administrator readings in Settings so the helper can read this.";

/// `text` as a sentence: trimmed, the first letter upper case (not in a name such as macOS), a full stop at the end unless it ends a sentence already. Empty
/// stays empty.
pub fn sentence(text: &str) -> String {
    let t = text.trim();
    let mut chars = t.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    // A word that already has a capital in it (macOS, iPhone) is a name, not the start of a sentence in lower case.
    let word_has_capital = t.split_whitespace().next().is_some_and(|w| w.chars().any(char::is_uppercase));
    let mut out: String = if word_has_capital {
        t.to_string()
    } else {
        first.to_uppercase().chain(chars).collect()
    };
    if !out.ends_with(['.', '!', '?']) {
        out.push('.');
    }
    out
}

/// What to do about a reading that could not be given. A closed set; family ADR 0052 defines the shared type in `paddock-jobs`
/// (WP-B1) and this one is replaced by it then (same variants, minus the ones with no meaning for a reading).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NextStep {
    /// Try again.
    Retry,
    /// Open this Settings section (`"helper"` for Settings > Dyno > Administrator readings).
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
    /// Not readable without the helper (or only partly), or the helper needs a step: the reason and that step.
    NeedsHelper {
        /// Why, as a sentence.
        reason: String,
        /// What to do: open Settings > Dyno > Administrator readings, or update the helper.
        next: NextStep,
    },
    /// Neither path can read it on this computer (a virtual machine without sensors, a missing tool, no helper on this system).
    Unavailable {
        /// Why, as a sentence.
        reason: String,
        /// What may help, when something does (Retry, Repair in Settings); none where nothing can.
        next: Option<NextStep>,
    },
}

impl ReadVia {
    /// `NeedsHelper` for `why`: turn the helper on in Settings.
    pub fn needs_admin(why: &str) -> ReadVia {
        ReadVia::NeedsHelper {
            reason: sentence(why),
            next: NextStep::OpenSettings("helper".into()),
        }
    }

    /// `NeedsHelper` for `why`: the helper is older than this Dyno accepts; update it.
    pub fn needs_update(why: &str) -> ReadVia {
        ReadVia::NeedsHelper {
            reason: sentence(why),
            next: NextStep::UpdateHelper,
        }
    }

    /// `Unavailable` for `why`, with nothing to do about it.
    pub fn unavailable(why: &str) -> ReadVia {
        ReadVia::Unavailable {
            reason: sentence(why),
            next: None,
        }
    }

    /// `Unavailable` for `why`, where trying again may work.
    pub fn retry(why: &str) -> ReadVia {
        ReadVia::Unavailable {
            reason: sentence(why),
            next: Some(NextStep::Retry),
        }
    }

    /// The reason, for the two kinds that carry one.
    pub fn reason(&self) -> Option<&str> {
        match self {
            ReadVia::NeedsHelper { reason, .. } | ReadVia::Unavailable { reason, .. } => Some(reason),
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
    /// A note beside the process list while part of it is still to come (not a failure; the page shows it as a hint). Always sent (the agent's wire is postcard, which has no skipped fields).
    pub processes_hint: Option<String>,
}
