//! The end line of a connection (ADR 0072 §6, WP-7; ER-27), the one implementation for the family's endpoints (rules 42, 55).
//! Every connection end, refused at the handshake or closed while open, logs one `connection ended ...` line of `key=value`
//! pairs through [`log_end`], and every end counts towards the summary of ends by code, logged every [`SUMMARY_EVERY`] ends
//! (the counters are per process). No content: a code, a phase, counts and the cause's own text.
//!
//! The line says only what this side did. `reason` is `sent` when the reason (a refusal frame or line) was written without
//! error and `unsent` when nothing was written or the write failed; `flush` is `none` and `drain` is `none` where the endpoint
//! does no such step (the ordered shutdown with a drain is the deferred WP-2 of the ADR).

use crate::error::IpcError;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// What an end line starts with (after the logger's prefix); the ending harness looks for it.
pub const MARKER: &str = "connection ended";

/// A summary line is logged after this many ends.
pub const SUMMARY_EVERY: u64 = 64;

/// Where in the connection's life it ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Before the connection was open: the handshake (hello, refusal, the limit) or the start of its threads.
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

/// Why a connection ended: the closed vocabulary of ADR 0072 §1 as far as the endpoints map it today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndCode {
    /// The protocol versions do not fit.
    Version,
    /// Wrong token, process or account (MCP: a wrong key).
    Auth,
    /// A limit of time ran out (the handshake, a silent client).
    Timeout,
    /// The other side sent something that is not the protocol (MCP: a lost key line).
    Protocol,
    /// The other side closed, or the stream broke.
    Closed,
    /// No place left, or a resource ran out before the connection could be served.
    Busy,
    /// The other side stopped reading and the queue ran out.
    Stalled,
    /// This side stopped (the owner closed the connection, the listener stopped).
    Stopping,
    /// A failure of this side (a thread that did not start, an unexpected error).
    Internal,
}

impl EndCode {
    /// Every code, in the order the summary prints them.
    pub const ALL: [EndCode; 9] = [
        EndCode::Version,
        EndCode::Auth,
        EndCode::Timeout,
        EndCode::Protocol,
        EndCode::Closed,
        EndCode::Busy,
        EndCode::Stalled,
        EndCode::Stopping,
        EndCode::Internal,
    ];

    /// The word in the log.
    pub fn word(self) -> &'static str {
        match self {
            EndCode::Version => "version",
            EndCode::Auth => "auth",
            EndCode::Timeout => "timeout",
            EndCode::Protocol => "protocol",
            EndCode::Closed => "closed",
            EndCode::Busy => "busy",
            EndCode::Stalled => "stalled",
            EndCode::Stopping => "stopping",
            EndCode::Internal => "internal",
        }
    }

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|c| *c == self)
            .unwrap_or(Self::ALL.len() - 1)
    }

    /// The code of a failure of the handshake: `Io` and `Spawn` there mean this side could not take the connection now.
    pub fn of_handshake(e: &IpcError) -> EndCode {
        match e {
            IpcError::Version(_) => EndCode::Version,
            IpcError::Auth(_) | IpcError::Denied(_) => EndCode::Auth,
            IpcError::Timeout => EndCode::Timeout,
            IpcError::Protocol(_) | IpcError::TooLarge { .. } | IpcError::ReplyTooLarge { .. } => {
                EndCode::Protocol
            }
            IpcError::Closed(_) => EndCode::Closed,
            IpcError::Spawn(_) | IpcError::Io { .. } => EndCode::Busy,
            IpcError::Remote(_) => EndCode::Internal,
        }
    }
}

/// Whether the reason for the end reached the other side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasonOutcome {
    /// A refusal or closing frame or line was written without error.
    Sent,
    /// Nothing was written, or the write failed.
    Unsent,
}

impl ReasonOutcome {
    /// `Sent` when `written` (the write succeeded).
    pub fn of_write(written: bool) -> ReasonOutcome {
        if written {
            ReasonOutcome::Sent
        } else {
            ReasonOutcome::Unsent
        }
    }

    fn word(self) -> &'static str {
        match self {
            ReasonOutcome::Sent => "sent",
            ReasonOutcome::Unsent => "unsent",
        }
    }
}

/// How a refused connection's drain ended (ADR 0072 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainOutcome {
    /// The endpoint did not drain.
    None,
    /// The client ended its side: everything it sent was read.
    Eof,
    /// The byte cap was read and the client was still writing.
    ByteCap,
    /// The drain's time ran out with the client still connected.
    TimeCap,
    /// The listener stopped.
    Stopped,
    /// A read failed.
    Failed,
    /// Every place of the drain pool was taken.
    PoolFull,
}

impl DrainOutcome {
    fn word(self) -> &'static str {
        match self {
            DrainOutcome::None => "none",
            DrainOutcome::Eof => "eof",
            DrainOutcome::ByteCap => "byteCap",
            DrainOutcome::TimeCap => "timeCap",
            DrainOutcome::Stopped => "stopped",
            DrainOutcome::Failed => "failed",
            DrainOutcome::PoolFull => "poolFull",
        }
    }
}

/// Which connection an end line is about: the peer's number once a peer exists, else the accept sequence number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnId {
    /// [`crate::peer::Peer::id`].
    Peer(u64),
    /// The endpoint's own count of accepted connections (see [`next_accept_id`]).
    Accept(u64),
}

impl std::fmt::Display for ConnId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnId::Peer(n) => write!(f, "p{n}"),
            ConnId::Accept(n) => write!(f, "a{n}"),
        }
    }
}

static NEXT_ACCEPT: AtomicU64 = AtomicU64::new(1);

/// A fresh accept sequence number, for an end before any peer exists (unique in the process, shared by every endpoint, so
/// two endpoints never print the same `a<n>`).
pub fn next_accept_number() -> u64 {
    NEXT_ACCEPT.fetch_add(1, Ordering::Relaxed)
}

/// [`next_accept_number`] as a [`ConnId`].
pub fn next_accept_id() -> ConnId {
    ConnId::Accept(next_accept_number())
}

static COUNTS: [AtomicU64; EndCode::ALL.len()] = [const { AtomicU64::new(0) }; EndCode::ALL.len()];
static TOTAL: AtomicU64 = AtomicU64::new(0);
/// One summary at a time: a line from two threads would interleave their counts.
static SUMMARY: Mutex<()> = Mutex::new(());

/// What this process has counted so far: `closed=3 auth=1` (codes never seen are left out).
pub(crate) fn summary_counts() -> String {
    EndCode::ALL
        .iter()
        .zip(COUNTS.iter())
        .filter_map(|(c, n)| {
            let n = n.load(Ordering::Relaxed);
            (n > 0).then(|| format!("{}={n}", c.word()))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Logs this process's summary of ends by code now (an endpoint that stops calls it too).
pub fn log_summary() {
    let _one = SUMMARY.lock().unwrap_or_else(|p| p.into_inner());
    log::info!(
        "connection end summary total={} {}",
        TOTAL.load(Ordering::Relaxed),
        summary_counts()
    );
}

/// What one end line says.
pub struct End<'a> {
    /// Which connection.
    pub id: ConnId,
    /// The connection's label (the peer's, or the endpoint's name).
    pub conn: &'a str,
    /// Where it ended.
    pub phase: Phase,
    /// Why.
    pub code: EndCode,
    /// Whether the reason reached the other side.
    pub reason: ReasonOutcome,
    /// How its drain ended.
    pub drain: DrainOutcome,
    /// Bytes the drain discarded.
    pub bytes: usize,
    /// How long the connection lived, in ms.
    pub open_ms: u128,
    /// Calls this side had out that failed with the end.
    pub pending: usize,
    /// Calls this side served that were cancelled by it.
    pub served: usize,
    /// The cause's text (no user content).
    pub detail: &'a str,
}

impl<'a> End<'a> {
    /// An end with nothing sent, no drain and no calls: set what the endpoint did with struct update syntax.
    pub fn new(id: ConnId, conn: &'a str, phase: Phase, code: EndCode) -> End<'a> {
        End {
            id,
            conn,
            phase,
            code,
            reason: ReasonOutcome::Unsent,
            drain: DrainOutcome::None,
            bytes: 0,
            open_ms: 0,
            pending: 0,
            served: 0,
            detail: "",
        }
    }
}

/// Single-line text: control characters and quotes folded, cut at 120 characters.
fn one_line(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() || c == '"' { ' ' } else { c })
        .take(120)
        .collect()
}

#[cfg(test)]
thread_local! {
    /// The end lines this thread logged (a unit test reads them; the log facade is process-wide).
    pub(crate) static TEST_LINES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Logs the end line and counts the end. Every endpoint's every connection end goes through here.
pub fn log_end(e: &End<'_>) {
    COUNTS[e.code.index()].fetch_add(1, Ordering::Relaxed);
    let line = format!(
        "{MARKER} id={} conn=\"{}\" phase={} code={} flush=none reason={} drain={} bytes={} open_ms={} pending={} served={} detail=\"{}\"",
        e.id,
        one_line(e.conn),
        e.phase.word(),
        e.code.word(),
        e.reason.word(),
        e.drain.word(),
        e.bytes,
        e.open_ms,
        e.pending,
        e.served,
        one_line(e.detail)
    );
    #[cfg(test)]
    TEST_LINES.with(|l| l.borrow_mut().push(line.clone()));
    log::info!("{line}");
    let n = TOTAL.fetch_add(1, Ordering::Relaxed) + 1;
    if n.is_multiple_of(SUMMARY_EVERY) {
        log_summary();
    }
}

/// The end of a connection refused or lost in the handshake.
pub(crate) fn log_handshake_end(
    conn: &str,
    started: std::time::Instant,
    e: &IpcError,
    reason: ReasonOutcome,
) {
    log_end(&End {
        reason,
        open_ms: started.elapsed().as_millis(),
        detail: &e.to_string(),
        ..End::new(
            next_accept_id(),
            conn,
            Phase::Hello,
            EndCode::of_handshake(e),
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn last() -> String {
        TEST_LINES.with(|l| l.borrow().last().cloned().unwrap_or_default())
    }

    #[test]
    fn the_marker_is_the_one_the_harness_looks_for() {
        #[cfg(feature = "testing")]
        assert_eq!(MARKER, crate::testing::endings::log::END_MARKER);
    }

    #[test]
    fn a_line_names_every_field_and_counts_under_its_code() {
        let before = COUNTS[EndCode::Stalled.index()].load(Ordering::Relaxed);
        log_end(&End {
            reason: ReasonOutcome::Sent,
            drain: DrainOutcome::TimeCap,
            bytes: 7,
            detail: "a \"quoted\"\nline",
            ..End::new(ConnId::Peer(3), "a caller", Phase::Open, EndCode::Stalled)
        });
        let line = last();
        for want in [
            "connection ended id=p3 ",
            "phase=open",
            "code=stalled",
            "reason=sent",
            "drain=timeCap",
            "bytes=7",
        ] {
            assert!(line.contains(want), "{want} in {line}");
        }
        assert!(
            !line.contains('\n') && line.matches('"').count() == 4,
            "{line}"
        );
        assert!(COUNTS[EndCode::Stalled.index()].load(Ordering::Relaxed) > before);
        assert!(summary_counts().contains("stalled="));
    }

    #[test]
    fn every_ended_connection_counts_towards_the_summary() {
        let before = TOTAL.load(Ordering::Relaxed);
        for _ in 0..3 {
            log_end(&End::new(
                next_accept_id(),
                "x",
                Phase::Hello,
                EndCode::Busy,
            ));
        }
        assert!(TOTAL.load(Ordering::Relaxed) >= before + 3);
    }

    #[test]
    fn the_handshake_maps_io_to_busy_and_a_refusal_that_failed_to_write_is_unsent() {
        assert_eq!(
            EndCode::of_handshake(&IpcError::Io {
                kind: std::io::ErrorKind::Other,
                message: "x".into()
            }),
            EndCode::Busy
        );
        assert_eq!(ReasonOutcome::of_write(false), ReasonOutcome::Unsent);
        assert_eq!(ReasonOutcome::of_write(true), ReasonOutcome::Sent);
    }

    #[test]
    fn one_line_has_no_newline_or_quote_and_is_cut() {
        let s = one_line(&format!("a\nb\"c{}", "x".repeat(300)));
        assert!(!s.contains('\n') && !s.contains('"'));
        assert!(s.chars().count() <= 120);
    }
}
