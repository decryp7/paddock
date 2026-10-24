//! Boot errors (ADR 0058): an uncaught error, an unhandled rejection or a failed bootstrap in a page before it reported
//! ready goes to the app's log as one bounded line, so a window that never starts leaves its cause in the log. The UI
//! sends the error's message and first stack frame only, never page text or file contents; this side bounds it again.

use serde::Serialize;

/// What failed in the page.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BootErrorKind {
    /// An uncaught error (`window.onerror`).
    Error,
    /// An unhandled promise rejection.
    Rejection,
    /// The bootstrap itself failed (the app never started).
    Bootstrap,
    /// Anything else the page sent (the kind is not trusted).
    Other,
}

impl BootErrorKind {
    /// The kind for the wire value; an unknown value is [`BootErrorKind::Other`].
    pub fn parse(kind: &str) -> BootErrorKind {
        match kind {
            "error" => BootErrorKind::Error,
            "rejection" => BootErrorKind::Rejection,
            "bootstrap" => BootErrorKind::Bootstrap,
            _ => BootErrorKind::Other,
        }
    }

    /// The kind's name in the log.
    pub const fn as_str(self) -> &'static str {
        match self {
            BootErrorKind::Error => "error",
            BootErrorKind::Rejection => "rejection",
            BootErrorKind::Bootstrap => "bootstrap",
            BootErrorKind::Other => "other",
        }
    }
}

/// One line for the log: control characters become spaces, at most `max_len` characters (then an ellipsis).
pub fn clean_error(text: &str, max_len: usize) -> String {
    let mut out: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max_len)
        .collect();
    if text.chars().nth(max_len).is_some() {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_errors_are_one_bounded_line() {
        assert_eq!(clean_error("a\nb\tc\u{1b}", 500), "a b c ");
        let long = "x".repeat(510);
        let cut = clean_error(&long, 500);
        assert_eq!(cut.chars().count(), 501);
        assert!(cut.ends_with('…'));
        assert_eq!(
            clean_error(&"é".repeat(500), 500).chars().count(),
            500,
            "exactly the limit is not cut"
        );
    }

    #[test]
    fn the_kind_is_parsed_not_trusted() {
        for kind in ["error", "rejection", "bootstrap"] {
            assert_eq!(BootErrorKind::parse(kind).as_str(), kind);
        }
        assert_eq!(BootErrorKind::parse("<script>"), BootErrorKind::Other);
        assert_eq!(BootErrorKind::Other.as_str(), "other");
    }
}
