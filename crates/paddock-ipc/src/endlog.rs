//! The end line of a connection (ADR 0072 §6, WP-7; ER-27): every connection end, refused at the handshake or closed while
//! open, logs one `connection ended ...` line of `key=value` pairs, and the process counts ends by code and logs a summary
//! every [`SUMMARY_EVERY`] ends. No content: a code, a phase, counts and the cause chain's first message.
//!
//! The line says only what this side knows. `flush`, `reason` and `drain` are `none` where the endpoint does no such step
//! today (the ordered shutdown with a drain is the deferred WP-2 of the ADR, so `drain=none` is the truth, not `eof`).

use crate::error::IpcError;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// What an end line starts with (after the logger's prefix); the ending harness looks for it.
pub const MARKER: &str = "connection ended";

/// A summary line is logged after this many ends.
pub const SUMMARY_EVERY: u64 = 64;

/// Where in the connection's life it ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Before the connection was open: the handshake (hello, refusal, the limit).
    Hello,
    /// After it was open.
    Open,
}

impl Phase {
    fn word(self) -> &'static str {
        match self {
            Phase::Hello => "hello",
            Phase::Open => "open",
        }
    }
}

/// The code of the reason for an end, in the closed vocabulary of the ADR as far as this side maps it today.
pub(crate) fn code_of(e: &IpcError) -> &'static str {
    match e {
        IpcError::Version(_) => "version",
        IpcError::Auth(_) | IpcError::Denied(_) => "auth",
        IpcError::Timeout => "timeout",
        IpcError::Protocol(_) | IpcError::TooLarge { .. } | IpcError::ReplyTooLarge { .. } => {
            "protocol"
        }
        IpcError::Closed(_) => "closed",
        IpcError::Spawn(_) | IpcError::Io { .. } => "busy",
        IpcError::Remote(_) => "internal",
    }
}

static CODES: [&str; 7] = [
    "version", "auth", "timeout", "protocol", "closed", "busy", "internal",
];
static COUNTS: [AtomicU64; 7] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];
static TOTAL: AtomicU64 = AtomicU64::new(0);
/// One summary at a time: a line from two threads would interleave their counts.
static SUMMARY: Mutex<()> = Mutex::new(());

/// What a process has counted so far: `closed=3 auth=1` (codes never seen are left out).
pub fn summary_counts() -> String {
    CODES
        .iter()
        .zip(COUNTS.iter())
        .filter_map(|(c, n)| {
            let n = n.load(Ordering::Relaxed);
            (n > 0).then(|| format!("{c}={n}"))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Logs the summary of ends by code now (also called by an endpoint that stops).
pub fn log_summary() {
    let _one = SUMMARY.lock().unwrap_or_else(|p| p.into_inner());
    log::info!(
        "connection end summary total={} {}",
        TOTAL.load(Ordering::Relaxed),
        summary_counts()
    );
}

/// What one end line says.
pub(crate) struct End<'a> {
    /// The connection's label (the peer's, or the endpoint's name before a peer exists).
    pub conn: &'a str,
    pub phase: Phase,
    pub code: &'static str,
    /// The cause's message (no user content: the protocol's own text).
    pub detail: &'a str,
    /// How long the connection lived, in ms.
    pub open_ms: u128,
    pub pending: usize,
    pub served: usize,
}

/// Single-line text: control characters and spaces folded, cut at 120 characters.
fn one_line(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(120)
        .collect::<String>()
        .replace('"', "'")
}

/// Logs the end line and counts the end.
pub(crate) fn log_end(e: &End<'_>) {
    if let Some(i) = CODES.iter().position(|c| *c == e.code) {
        COUNTS[i].fetch_add(1, Ordering::Relaxed);
    }
    log::info!(
        "{MARKER} conn=\"{}\" phase={} code={} flush=none reason=none drain=none bytes=0 open_ms={} pending={} served={} detail=\"{}\"",
        one_line(e.conn),
        e.phase.word(),
        e.code,
        e.open_ms,
        e.pending,
        e.served,
        one_line(e.detail)
    );
    let n = TOTAL.fetch_add(1, Ordering::Relaxed) + 1;
    if n.is_multiple_of(SUMMARY_EVERY) {
        log_summary();
    }
}

/// Logs the end of a connection refused or lost in the handshake.
pub(crate) fn log_handshake_end(conn: &str, started: std::time::Instant, e: &IpcError) {
    log_end(&End {
        conn,
        phase: Phase::Hello,
        code: code_of(e),
        detail: &e.to_string(),
        open_ms: started.elapsed().as_millis(),
        pending: 0,
        served: 0,
    });
}

/// The end line of a connection an endpoint turned away before any peer existed (its own accept loop: no handshake, a caller
/// that could not be identified, no place left). `code` is one of the vocabulary: `busy`, `auth`, `protocol`, `timeout`,
/// `internal`; `drain` is `none` (this endpoint does not drain).
pub fn log_refused(conn: &str, code: &'static str, detail: &str) {
    log_end(&End {
        conn,
        phase: Phase::Hello,
        code: if CODES.contains(&code) {
            code
        } else {
            "internal"
        },
        detail,
        open_ms: 0,
        pending: 0,
        served: 0,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marker_is_the_one_the_harness_looks_for() {
        #[cfg(feature = "testing")]
        assert_eq!(MARKER, crate::testing::endings::log::END_MARKER);
    }

    #[test]
    fn a_refused_connection_counts_under_its_code_and_an_unknown_code_is_internal() {
        let before = summary_counts();
        log_refused("a caller", "busy", "too many connections");
        log_refused("a caller", "nonsense", "x");
        let after = summary_counts();
        assert_ne!(before, after);
        assert!(after.contains("busy="), "{after}");
        assert!(after.contains("internal="), "{after}");
    }

    #[test]
    fn codes_map_into_the_vocabulary() {
        assert_eq!(code_of(&IpcError::Timeout), "timeout");
        assert_eq!(code_of(&IpcError::Auth("x".into())), "auth");
        assert!(CODES.contains(&code_of(&IpcError::Closed("x".into()))));
    }

    #[test]
    fn one_line_has_no_newline_or_quote_and_is_cut() {
        let s = one_line(&format!("a\nb\"c{}", "x".repeat(300)));
        assert!(!s.contains('\n') && !s.contains('"'));
        assert!(s.chars().count() <= 120);
    }
}
