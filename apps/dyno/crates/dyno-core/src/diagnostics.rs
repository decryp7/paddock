//! What the agent hands the app for a problem report: its own data files as plain text (inventory, alarms, sockets, activity,
//! alerts, the recorder, the helper's log). The app's Report a problem dialog (`paddock-diag`) redacts, previews and zips them with
//! the rest of the report; this crate no longer redacts or zips anything (there is one redactor and one bundle).

use serde::{Deserialize, Serialize};

/// Most files the agent hands over.
pub const MAX_FILES: usize = 32;
/// Longest path inside the report, characters (`paddock_diag::valid_entry_name` allows 100).
pub const MAX_PATH_CHARS: usize = 100;

/// One data file for the report. `text` is not redacted yet: the report's redactor does that.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsFile {
    /// Path inside the report: ASCII letters, digits and `/ . - _`.
    pub name: String,
    /// One line for the preview, in words the user understands.
    pub summary: String,
    /// The content.
    pub text: String,
}

/// The agent's answer to `DiagnosticsFiles`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsFiles {
    /// The files, in report order.
    pub files: Vec<DiagnosticsFile>,
    /// What could not be collected, in words for the user.
    pub notes: Vec<String>,
}

impl DiagnosticsFile {
    /// A file of the agent's data.
    pub fn new(name: &str, summary: &str, text: String) -> Self {
        DiagnosticsFile {
            name: name.into(),
            summary: summary.into(),
            text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_round_trips_as_json() {
        let a = DiagnosticsFiles {
            files: vec![DiagnosticsFile::new("alarms/open.json", "Alarms", "[]".into())],
            notes: vec!["the recorder is empty".into()],
        };
        let back: DiagnosticsFiles = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(a, back);
    }
}
