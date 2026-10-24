//! The notification to deliver, and its bounds.

use serde::{Deserialize, Serialize};

/// Longest title (e-mail subject, ntfy title), in characters.
pub const MAX_TITLE_CHARS: usize = 200;
/// Longest body sent, in bytes; a longer one is cut on a character boundary.
pub const MAX_BODY_BYTES: usize = 64 << 10;
const MAX_SHORT: usize = 64;

/// How important a message is. Ordered: `Info < Warning < Error`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// Informational.
    #[default]
    Info,
    /// Needs attention soon.
    Warning,
    /// Needs attention now.
    Error,
}

/// A notification to deliver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// Short headline.
    pub title: String,
    /// Details.
    pub body: String,
    /// Importance.
    pub severity: Severity,
    /// Event kind id such as `diskLow`; channels subscribe to kinds.
    pub kind: String,
    /// Who raised it, for example a runner name.
    pub source: String,
}

impl Message {
    /// A message with the given parts, not yet bounded (the dispatcher bounds every message it sends).
    pub fn new(kind: &str, severity: Severity, title: &str, body: &str, source: &str) -> Message {
        Message {
            title: title.to_string(),
            body: body.to_string(),
            severity,
            kind: kind.to_string(),
            source: source.to_string(),
        }
    }

    /// The same message within the limits: the title on one line of at most [`MAX_TITLE_CHARS`] characters, the body at most
    /// [`MAX_BODY_BYTES`] bytes (cut on a character boundary, with a marker), kind and source short and on one line.
    pub fn bounded(&self) -> Message {
        let mut body = self.body.clone();
        if body.len() > MAX_BODY_BYTES {
            const MARK: &str = "\n[cut]";
            let mut cut = MAX_BODY_BYTES - MARK.len();
            while !body.is_char_boundary(cut) {
                cut -= 1;
            }
            body.truncate(cut);
            body.push_str(MARK);
        }
        Message {
            title: one_line(&self.title, MAX_TITLE_CHARS),
            body,
            severity: self.severity,
            kind: one_line(&self.kind, MAX_SHORT),
            source: one_line(&self.source, MAX_SHORT),
        }
    }
}

/// Control characters dropped (so a header or subject stays on one line), cut to `max` characters, trimmed.
pub(crate) fn one_line(text: &str, max: usize) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}
