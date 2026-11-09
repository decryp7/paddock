//! The ending harness (ADR 0072 §5, WP-0): drives an endpoint through every way a connection ends and judges what the peer
//! saw. It asserts on the **raw stream** (below `paddock-ipc`'s folding of resets into `Closed`), on the typed API and on
//! the endpoint's **own end line**, and it proves it can see each fault: [`mini`] has a correct reference endpoint and one
//! deliberately broken endpoint per fault, and a test asserts that the harness fails on each, naming the scenario.
//!
//! What fails today is recorded per instance in the ratchet ([`ratchet`]), which only shrinks. Before M1 the end line does
//! not exist, so the scenarios that need it are ratchet entries; the raw-stream assertions run from the start.

pub mod applicability;
pub mod ipc;
pub mod log;
pub mod mini;
pub mod ratchet;
pub mod raw;

use raw::{RawEnd, RawEndKind};
use std::fmt;

/// How much a refused client keeps writing in [`Scenario::ClientKeepsWriting`]: less than any drain's byte cap, so a right
/// endpoint reads it all and the drain ends at the client's end (`eof`).
pub const KEEP_WRITING_BYTES: usize = 48 << 10;

/// The ways a connection ends that every endpoint is driven through. An endpoint says which of them apply to it
/// ([`Outcome::NotApplicable`], with the reason).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scenario {
    /// A client presents a wrong token.
    WrongToken,
    /// A client speaks another frame or protocol version.
    WrongVersion,
    /// A client runs as another account (Unix); refused before anything is read.
    WrongAccount,
    /// The endpoint has no place left for the client.
    Busy,
    /// A client connects and says nothing until the handshake runs out (on a fake clock).
    SilentClient,
    /// The first frame is not a hello.
    MalformedFirstFrame,
    /// A client keeps writing after it was refused.
    ClientKeepsWriting,
    /// The owner closes with calls in flight whose handler answers its cancel at once.
    OwnerCloseWithCallsInFlight,
    /// The owner closes while the peer is not reading (the flush cap and the reason deadline).
    OwnerCloseWhilePeerNotReading,
    /// A send stalls because the peer stopped reading (the write-stall limit, on a fake clock).
    Stall,
    /// The listener stops with a client connected but not yet accepted.
    ListenerStopWithClientInBacklog,
    /// The endpoint stops with an open connection.
    StopWithOpenConnection,
    /// A peer of today's code, which ignores a `Fail { id: 0 }` after the handshake, sees an orderly end all the same.
    MixedVersionPeerIgnoresFail0,
}

impl Scenario {
    /// Every scenario, in the order of ADR 0072 §5.
    pub const ALL: [Scenario; 13] = [
        Scenario::WrongToken,
        Scenario::WrongVersion,
        Scenario::WrongAccount,
        Scenario::Busy,
        Scenario::SilentClient,
        Scenario::MalformedFirstFrame,
        Scenario::ClientKeepsWriting,
        Scenario::OwnerCloseWithCallsInFlight,
        Scenario::OwnerCloseWhilePeerNotReading,
        Scenario::Stall,
        Scenario::ListenerStopWithClientInBacklog,
        Scenario::StopWithOpenConnection,
        Scenario::MixedVersionPeerIgnoresFail0,
    ];

    /// The name the ratchet and the reports use.
    pub fn name(self) -> &'static str {
        match self {
            Scenario::WrongToken => "wrong-token",
            Scenario::WrongVersion => "wrong-version",
            Scenario::WrongAccount => "wrong-account",
            Scenario::Busy => "busy",
            Scenario::SilentClient => "silent-client",
            Scenario::MalformedFirstFrame => "malformed-first-frame",
            Scenario::ClientKeepsWriting => "client-keeps-writing",
            Scenario::OwnerCloseWithCallsInFlight => "owner-close-with-calls-in-flight",
            Scenario::OwnerCloseWhilePeerNotReading => "owner-close-while-peer-not-reading",
            Scenario::Stall => "stall",
            Scenario::ListenerStopWithClientInBacklog => "listener-stop-with-client-in-backlog",
            Scenario::StopWithOpenConnection => "stop-with-open-connection",
            Scenario::MixedVersionPeerIgnoresFail0 => "mixed-version-peer-ignores-fail0",
        }
    }
}

impl fmt::Display for Scenario {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What can be wrong with an ending. The kind names ([`Fault::kind`]) are the ratchet's vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// The stream ended without the reason the scenario expects.
    NoReason,
    /// A reason arrived, but not the expected code.
    WrongReason {
        /// The codes expected.
        expected: Vec<String>,
        /// The last code received.
        got: String,
    },
    /// Something followed the reason on the stream: it must be the last thing written.
    ReasonNotLast,
    /// The raw read failed with a reset instead of ending in order.
    Reset(std::io::ErrorKind),
    /// The typed API reported a bare transport end where the scenario expects a said reason.
    TypedTransport,
    /// The typed API reported `Remote(cancelled)` after an owner close.
    CancelledAfterOwnerClose,
    /// The raw read waited its guard and nothing came: no answer and no end.
    Unanswered,
    /// The scenario did not finish (a flush or a release that never ends).
    Hang,
    /// The endpoint wrote no end line.
    NoEndLine,
    /// The end line says a stream was dropped without a verdict.
    DroppedWithoutVerdict,
    /// The end line's drain outcome is not `eof`.
    DrainNotEof(String),
}

impl Fault {
    /// The short name of the kind, as the ratchet writes it.
    pub fn kind(&self) -> &'static str {
        match self {
            Fault::NoReason => "no-reason",
            Fault::WrongReason { .. } => "wrong-reason",
            Fault::ReasonNotLast => "reason-not-last",
            Fault::Reset(_) => "reset",
            Fault::TypedTransport => "typed-transport",
            Fault::CancelledAfterOwnerClose => "cancelled-after-owner-close",
            Fault::Unanswered => "unanswered",
            Fault::Hang => "hang",
            Fault::NoEndLine => "no-end-line",
            Fault::DroppedWithoutVerdict => "dropped-without-verdict",
            Fault::DrainNotEof(_) => "drain-not-eof",
        }
    }
}

/// The reason frames or lines found in a raw byte stream, decoded by the endpoint's own protocol.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Decoded {
    /// The codes of the reasons in order of arrival.
    pub reasons: Vec<String>,
    /// How many complete frames or lines followed the last reason.
    pub after_last_reason: usize,
    /// Bytes at the end that were not a complete frame or line.
    pub partial_tail: usize,
}

/// What the typed API of the side under test reported (when the scenario has a typed client).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Typed {
    /// The end carries no said reason (`Closure::Transport` once M1 lands; today every `IpcError::Closed`).
    pub transport_only: bool,
    /// The call ended as `Remote(cancelled)`.
    pub cancelled: bool,
}

/// What the harness saw in one scenario.
#[derive(Debug, Clone, Default)]
pub struct Observed {
    /// What the raw client read and how it ended.
    pub raw: Option<RawEnd>,
    /// The raw bytes decoded by the endpoint's protocol.
    pub decoded: Decoded,
    /// The typed client's result.
    pub typed: Option<Typed>,
    /// The lines the endpoint logged during the scenario (the product's log, printed when a rig fails).
    pub log: Vec<String>,
    /// The scenario did not finish within its guard.
    pub hung: bool,
}

/// How a reason is expected.
#[derive(Debug, Clone)]
pub enum ReasonRule {
    /// One of these codes must arrive, last.
    OneOf(Vec<String>),
    /// A reason may arrive (the peer may not be reading), but if the stream carries one it must be last.
    Optional,
}

/// What a scenario expects to have happened.
#[derive(Debug, Clone)]
pub struct Expect {
    /// The reason rule.
    pub reason: ReasonRule,
    /// The endpoint is expected to log an end line with drain outcome `eof` (ADR 0072 §6).
    pub end_line: bool,
    /// The client sent bytes the endpoint never reads before it closes (a refusal before the hello is read, a client that
    /// keeps writing). Closing with unread input makes the OS send a reset, which may or may not reach the client before the
    /// reason does (a race: macOS delivers either), so a missing reason is then attributed to the reset and reported as
    /// [`Fault::Reset`], the one outcome that does not depend on the race.
    pub unread_input: bool,
}

impl Expect {
    /// A reason with one of `codes` is expected, and the end line.
    pub fn reason(codes: &[&str]) -> Expect {
        Expect {
            reason: ReasonRule::OneOf(codes.iter().map(|c| (*c).to_string()).collect()),
            end_line: true,
            unread_input: false,
        }
    }

    /// An optional reason, and the end line.
    pub fn optional_reason() -> Expect {
        Expect {
            reason: ReasonRule::Optional,
            end_line: true,
            unread_input: false,
        }
    }

    /// The same expectation for a client whose input the endpoint leaves unread (see [`Expect::unread_input`]).
    pub fn with_unread_input(mut self) -> Expect {
        self.unread_input = true;
        self
    }
}

/// Judges `observed` against `expect`: every fault found, in a fixed order.
pub fn judge(observed: &Observed, expect: &Expect) -> Vec<Fault> {
    let mut faults = Vec::new();
    if observed.hung {
        faults.push(Fault::Hang);
    }
    if let Some(raw) = &observed.raw {
        match raw.kind {
            kind if kind.is_reset() => {
                if let RawEndKind::Error(k) = kind {
                    faults.push(Fault::Reset(k));
                }
            }
            RawEndKind::TimedOut => faults.push(Fault::Unanswered),
            _ => {}
        }
        if let ReasonRule::OneOf(codes) = &expect.reason {
            match observed.decoded.reasons.last() {
                None if raw.kind != RawEndKind::TimedOut => faults.push(Fault::NoReason),
                None => {}
                Some(got) if !codes.iter().any(|c| c == got) => {
                    faults.push(Fault::WrongReason {
                        expected: codes.clone(),
                        got: got.clone(),
                    });
                }
                Some(_) => {}
            }
        }
        if !observed.decoded.reasons.is_empty() && observed.decoded.after_last_reason > 0 {
            faults.push(Fault::ReasonNotLast);
        }
    }
    if let Some(typed) = observed.typed {
        if typed.cancelled {
            faults.push(Fault::CancelledAfterOwnerClose);
        }
        if typed.transport_only {
            faults.push(Fault::TypedTransport);
        }
    }
    if expect.unread_input && faults.contains(&Fault::NoReason) {
        // The reset explains the missing reason (it may have discarded it), so only the reset is reported.
        faults.retain(|f| *f != Fault::NoReason);
        if !faults.iter().any(|f| matches!(f, Fault::Reset(_))) {
            faults.push(Fault::Reset(std::io::ErrorKind::ConnectionReset));
        }
    }
    if expect.end_line {
        match log::find_end_line(&observed.log) {
            None => faults.push(Fault::NoEndLine),
            Some(line) => {
                if line.contains("DroppedWithoutVerdict") {
                    faults.push(Fault::DroppedWithoutVerdict);
                }
                match log::field(&line, "drain") {
                    Some("eof") => {}
                    Some(other) => faults.push(Fault::DrainNotEof(other.to_string())),
                    None => faults.push(Fault::DrainNotEof(String::new())),
                }
            }
        }
    }
    faults
}

/// The result of one scenario against one endpoint.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// The scenario does not apply to this endpoint; the register (`[[not_applicable]]`) says why. It was not run.
    NotApplicable(String),
    /// The rig itself failed (an endpoint that did not start, a client that could not connect): the test is red, never a
    /// quiet "not applicable".
    RigFailure(String),
    /// It ran; these are the faults found (none: the ending is right).
    Ran(Vec<Fault>),
}

/// One scenario's report.
#[derive(Debug, Clone)]
pub struct Report {
    /// The endpoint's id in `docs/ipc-endpoints.toml`.
    pub endpoint: String,
    /// The scenario.
    pub scenario: Scenario,
    /// What happened.
    pub outcome: Outcome,
    /// The endpoint's log of the scenario, printed when the rig fails.
    pub log: Vec<String>,
}

impl Report {
    /// The faults of a scenario that ran (empty when it did not run).
    pub fn faults(&self) -> &[Fault] {
        match &self.outcome {
            Outcome::Ran(f) => f,
            Outcome::NotApplicable(_) | Outcome::RigFailure(_) => &[],
        }
    }

    /// A one-line summary: `endpoint scenario: ok | faults | n/a (why)`.
    pub fn summary(&self) -> String {
        match &self.outcome {
            Outcome::NotApplicable(why) => format!(
                "{} {}: not applicable ({why})",
                self.endpoint, self.scenario
            ),
            Outcome::RigFailure(why) => {
                format!("{} {}: RIG FAILURE ({why})", self.endpoint, self.scenario)
            }
            Outcome::Ran(f) if f.is_empty() => format!("{} {}: ok", self.endpoint, self.scenario),
            Outcome::Ran(f) => {
                format!(
                    "{} {}: {}",
                    self.endpoint,
                    self.scenario,
                    f.iter().map(Fault::kind).collect::<Vec<_>>().join(", ")
                )
            }
        }
    }
}

/// A failure of the rig, not a finding about the endpoint.
pub type RigError = String;

/// Runs `run` for every scenario of [`Scenario::ALL`] that the register does not declare not applicable to `endpoint`
/// (`applicability`), and collects the reports. A scenario the register declares not applicable is not run; one that `run`
/// cannot drive returns a [`RigError`], which becomes [`Outcome::RigFailure`] and fails [`ratchet::Ratchet::check`].
pub fn run_all<E: std::fmt::Display>(
    endpoint: &str,
    applicability: &applicability::Applicability,
    mut run: impl FnMut(Scenario) -> Result<(Observed, Expect), E>,
) -> Vec<Report> {
    Scenario::ALL
        .iter()
        .map(|&scenario| {
            let report = |outcome, log| Report {
                endpoint: endpoint.to_string(),
                scenario,
                outcome,
                log,
            };
            if let Some(why) = applicability.reason(endpoint, scenario) {
                return report(Outcome::NotApplicable(why), Vec::new());
            }
            match run(scenario) {
                Err(why) => report(Outcome::RigFailure(why.to_string()), Vec::new()),
                Ok((observed, expect)) => {
                    let faults = judge(&observed, &expect);
                    report(Outcome::Ran(faults), observed.log)
                }
            }
        })
        .collect()
}

/// The text a rig prints when a check fails: the summaries and, for every scenario with faults, the endpoint's log.
pub fn failure_text(reports: &[Report]) -> String {
    let mut out = String::new();
    for r in reports {
        out.push_str(&r.summary());
        out.push('\n');
        if !r.faults().is_empty() {
            for l in &r.log {
                out.push_str("    | ");
                out.push_str(l);
                out.push('\n');
            }
        }
    }
    out
}
