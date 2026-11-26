//! The ending harness and the endings of `paddock-ipc`'s own endpoints (ADR 0072 WP-0).
//!
//! - The harness proves it can see each fault: the reference endpoint passes, and each deliberately broken one fails in the
//!   scenario that can see its fault, with that scenario named (ER-33).
//! - `ipc_host_ends_every_connection_right_or_is_in_the_ratchet` drives the crate's host, peer and listener through every
//!   scenario: green with `docs/ipc-endpoint-ratchet.toml`, red without it.

use paddock_ipc::testing::endings::applicability::Applicability;
use paddock_ipc::testing::endings::mini::{self, MiniFault};
use paddock_ipc::testing::endings::ratchet::Ratchet;
use paddock_ipc::testing::endings::{
    self, Fault, Outcome, Scenario, failure_text, ipc, judge, run_all,
};

fn kinds(faults: &[Fault]) -> Vec<&'static str> {
    faults.iter().map(Fault::kind).collect()
}

#[test]
fn the_reference_endpoint_passes_every_scenario_it_supports() {
    let not_applicable: Vec<(Scenario, &str)> = Scenario::ALL
        .iter()
        .filter(|s| !mini::applies(**s))
        .map(|s| (*s, "the mini protocol has no counterpart for this scenario"))
        .collect();
    let reports = run_all(
        "mini-reference",
        &Applicability::of("mini-reference", &not_applicable),
        |s| mini::run(None, s),
    );
    let ran = reports
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Ran(_)))
        .count();
    assert!(
        ran >= 6,
        "the reference runs the six scenarios of the mini protocol, ran {ran}"
    );
    for r in &reports {
        assert!(
            r.faults().is_empty(),
            "{}\n{}",
            r.summary(),
            failure_text(&reports)
        );
    }
}

/// Run 8174: the harness let the peer read again only after it had advanced the reason deadline, so an endpoint that reacted
/// to the deadline first saw a peer that still did not read, gave up on the reason and ended with `drain=none`. The peer
/// must read again before the deadline passes, whatever the order the two threads run in.
#[test]
fn the_reference_ends_an_unread_close_right_when_the_endpoint_reacts_before_the_harness_goes_on() {
    let (observed, expect) = mini::run_with(
        None,
        Scenario::OwnerCloseWhilePeerNotReading,
        &|log, before| {
            assert!(
                log.wait_for_len(before + 1, std::time::Duration::from_secs(30)),
                "the endpoint never decided about the reason"
            );
        },
    )
    .expect("applies");
    let faults = judge(&observed, &expect);
    assert!(faults.is_empty(), "{faults:?}\n{:?}", observed.log);
}

#[test]
fn each_broken_endpoint_fails_in_the_scenario_that_sees_its_fault() {
    for fault in MiniFault::ALL {
        let scenario = fault.seen_in();
        let (observed, expect) =
            mini::run(Some(fault), scenario).expect("the scenario applies to the mini protocol");
        let faults = judge(&observed, &expect);
        let wanted: &[&str] = match fault {
            MiniFault::DropWithoutReason => &["no-reason"],
            MiniFault::CancelBeforeSeal => &["cancelled-after-owner-close"],
            MiniFault::FlushWithoutCap => &["hang"],
            MiniFault::ReleaseWithoutDrain => &["drain-not-eof"],
            MiniFault::DropBacklog => &["no-reason", "reset"],
        };
        assert!(
            kinds(&faults).iter().any(|k| wanted.contains(k)),
            "{fault:?} must fail in {scenario} with one of {wanted:?}, found {:?}",
            kinds(&faults)
        );
    }
}

#[test]
fn a_broken_endpoint_that_a_scenario_cannot_see_is_not_blamed_there() {
    // The control is specific: the reference's scenarios stay green while DropBacklog is only seen by the backlog scenario.
    let (observed, expect) =
        mini::run(Some(MiniFault::DropBacklog), Scenario::WrongToken).expect("applies");
    assert!(
        judge(&observed, &expect).is_empty(),
        "DropBacklog does not break a refusal"
    );
}

/// A stream that records each write call and reads nothing.
#[derive(Default)]
struct Recorder {
    writes: Vec<Vec<u8>>,
}

impl std::io::Read for Recorder {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Ok(0)
    }
}

impl std::io::Write for Recorder {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.writes.push(buf.to_vec());
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl endings::raw::RawStream for Recorder {
    fn set_read_timeout(&mut self, _: Option<std::time::Duration>) -> std::io::Result<()> {
        Ok(())
    }
    fn shutdown_write(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Regression (CI run 9001, Linux: "stale ratchet entry I10: ipc-host / client-keeps-writing: reset no longer happens"): the
/// scenario's premise is that the endpoint closes with the client's later bytes unread. When the hello and the junk went out
/// in separate writes, the host could read the hello, refuse and close before the first junk write arrived, so nothing was
/// unread and Linux ended the stream in order instead of with a reset. The junk must travel in the same write as the hello,
/// so it is in the endpoint's receive queue before the endpoint can read the hello and act on it.
#[test]
fn a_client_that_keeps_writing_sends_junk_in_the_same_write_as_its_hello() {
    let hello = b"HELLO bad\n";
    let mut rec = Recorder::default();
    let junk = endings::raw::send_then_keep_writing(&mut rec, hello, endings::KEEP_WRITING_BYTES)
        .expect("a recorder takes every write");
    let first = rec.writes.first().expect("something was written");
    assert!(
        first.starts_with(hello) && first.len() > hello.len(),
        "the first write carried {} bytes, the hello alone is {}: junk written separately can arrive after the endpoint \
         read the hello and closed, and then nothing is unread at the close (CI run 9001)",
        first.len(),
        hello.len()
    );
    assert_eq!(
        junk,
        endings::KEEP_WRITING_BYTES,
        "every junk byte is written"
    );
    let total: usize = rec.writes.iter().map(Vec::len).sum();
    assert_eq!(total, hello.len() + endings::KEEP_WRITING_BYTES);
}

#[test]
fn the_ratchet_reads_its_file_and_only_shrinks() {
    let text = r#"
# a comment
[[ratchet]]
instance = "I11"
endpoint = "ipc-host"
scenario = "listener-stop-with-client-in-backlog"
fault = "no-reason"
blocks_tag = "dyno-v*"
note = "why"
"#;
    let ratchet = Ratchet::parse(text).expect("parses");
    assert_eq!(ratchet.entries.len(), 1);
    assert_eq!(ratchet.entries[0].blocks_tag, "dyno-v*");
    let (o, e) = mini::run(
        Some(MiniFault::DropBacklog),
        Scenario::ListenerStopWithClientInBacklog,
    )
    .expect("applies");
    let report = endings::Report {
        endpoint: "ipc-host".into(),
        scenario: Scenario::ListenerStopWithClientInBacklog,
        outcome: Outcome::Ran(judge(&o, &e)),
        log: o.log,
        warnings: Vec::new(),
    };
    // A fault the ratchet lists is green only if it still happens in the way listed; the reset variant is a new failure.
    let result = ratchet.check("ipc-host", std::slice::from_ref(&report));
    if report.faults().iter().all(|f| {
        f.kind() == "no-reason" || f.kind() == "no-end-line" || f.kind() == "drain-not-eof"
    }) {
        assert!(
            result.is_err(),
            "the end-line faults are not listed, so they are new failures: {result:?}"
        );
    }
    // A listed fault that does not happen is stale.
    let clean = endings::Report {
        endpoint: "ipc-host".into(),
        scenario: Scenario::WrongToken,
        outcome: Outcome::Ran(vec![]),
        log: vec![],
        warnings: Vec::new(),
    };
    let stale = ratchet
        .check("ipc-host", std::slice::from_ref(&clean))
        .expect_err("stale entry");
    assert!(stale.contains("stale ratchet entry I11"), "{stale}");
    assert!(
        Ratchet::parse("[[ratchet]]\ninstance = \"I1\"\n").is_err(),
        "an entry without all keys is refused"
    );
    assert!(
        Ratchet::parse("[[ratchet]]\nbogus = \"x\"\n").is_err(),
        "an unknown key is refused"
    );
    // A report-only row names its app (`./dev check ipc-endpoints`, C102, reads it): the ratchet reader accepts the key.
    let app_row = "[[ratchet]]\ninstance = \"I4\"\nendpoint = \"ipc-host\"\nscenario = \"silent-client\"\nfault = \"no-reason\"\nblocks_tag = \"report-only\"\napp = \"dyno\"\n";
    assert!(
        Ratchet::parse(app_row).is_ok(),
        "{:?}",
        Ratchet::parse(app_row).err()
    );
}

/// The ending test of the register's `ipc-host` entry (and of the listener primitives it stands on).
#[test]
fn ipc_host_ends_every_connection_right_or_is_in_the_ratchet() {
    let applicability =
        Applicability::load_from(env!("CARGO_MANIFEST_DIR")).expect("the register file");
    let reports = run_all("ipc-host", &applicability, ipc::in_process);
    let ratchet = Ratchet::load_from(env!("CARGO_MANIFEST_DIR")).expect("the ratchet file");
    if let Err(e) = ratchet.check("ipc-host", &reports) {
        panic!("{e}\n\n{}", failure_text(&reports));
    }
    // The end line tells the truth about the reason (WP-7 review): a refusal that was written says `sent`; a silent client
    // is timed out with nothing written (`unsent`).
    assert_reason(&reports, Scenario::WrongToken, "sent");
    assert_reason(&reports, Scenario::WrongVersion, "sent");
    assert_reason(&reports, Scenario::Busy, "sent");
    assert_reason(&reports, Scenario::SilentClient, "unsent");
}

/// The `reason=` of the one end line of `scenario`.
fn assert_reason(reports: &[endings::Report], scenario: Scenario, want: &str) {
    let report = reports
        .iter()
        .find(|r| r.scenario == scenario)
        .expect("a report");
    let lines = endings::log::end_lines(&report.log);
    assert_eq!(lines.len(), 1, "{}", failure_text(reports));
    assert_eq!(
        endings::log::field(lines[0], "reason"),
        Some(want),
        "{scenario}: {}",
        lines[0]
    );
}

pub mod link {
    use super::*;
    use paddock_ipc::link::{LinkServer, ServerConfig, discovery};
    use paddock_ipc::testing::endings::ipc::IpcTarget;
    use paddock_ipc::testing::endings::raw::{READ_GUARD, read_to_end};
    use paddock_ipc::testing::endings::{Precondition, QueueGate};
    use paddock_ipc::{
        CallCtx, Client, FrameLimit, Handler, NoCalls, ProtocolVersion, RemoteError,
    };
    use paddock_jobs::time::FakeClock;
    use std::sync::Arc;
    use std::time::Duration;

    const GUARD: Duration = Duration::from_secs(30);

    fn echo() -> Arc<dyn Handler> {
        Arc::new(|_: &CallCtx, body: &[u8]| -> Result<Vec<u8>, RemoteError> { Ok(body.to_vec()) })
    }

    struct Rig {
        server: Option<LinkServer>,
        target: IpcTarget,
        clock: FakeClock,
        /// The server's act gate (ADR 0122 §2): every accepted client passes it; an armed one is held until its bytes are
        /// queued.
        gate: Arc<QueueGate>,
        _dir: tempfile::TempDir,
    }

    fn rig(max_connections: usize) -> Rig {
        rig_with_gate(max_connections, READ_GUARD)
    }

    /// A rig whose act gate waits at most `bound` for the client's bytes.
    fn rig_with_gate(max_connections: usize, bound: Duration) -> Rig {
        let gate = Arc::new(QueueGate::new(bound));
        let dir = tempfile::tempdir().expect("a temporary folder");
        let path = dir.path().join("agent.json");
        let clock = FakeClock::new();
        let mut c = ServerConfig::new(&path, "ending-test");
        c.socket_dir = Some(dir.path().to_path_buf());
        c.clock = Arc::new(clock.clone());
        c.handshake_timeout = Duration::from_secs(10);
        c.max_connections = max_connections;
        let c = c.with_act_gate(ipc::act_gate(&gate));
        let server = LinkServer::start(c, echo()).expect("the link server starts");
        let found = discovery::read(&path, |_| true);
        let (discovery::Found::Live(d) | discovery::Found::Stale(d)) = found else {
            panic!("no discovery file")
        };
        let target = IpcTarget {
            endpoint: d.endpoint.clone(),
            token: d.token().expect("a token"),
            limit: FrameLimit::default(),
            protocol: ProtocolVersion::UNVERSIONED,
        };
        Rig {
            server: Some(server),
            target,
            clock,
            gate,
            _dir: dir,
        }
    }

    /// How the busy client of [`busy`] writes its hello.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum BusyClient {
        /// Right after it connected: the server's act gate sees the hello queued before the server refuses (CI run 8425 was
        /// this client without the gate: the server refused first and the write failed).
        WritesItsHello,
        /// Only after the server's act gate gave up waiting (the positive control of test 2): the precondition is missed.
        WritesAfterTheGate,
    }

    /// The busy scenario (one place, held by a first client): the refused client's hello must be queued before the server
    /// refuses it, which the act gate establishes and measures (ADR 0122 test 2). A write that fails after a missed
    /// precondition is part of what the rig saw, never a `RIG FAILURE (write)`.
    pub fn busy(
        client: BusyClient,
        bound: Duration,
    ) -> Result<(endings::Observed, endings::Expect), &'static str> {
        let rig = rig_with_gate(1, bound);
        // One connection takes the only place; its handshake is over (and its pass through the gate) when `connect` returns.
        let held = Client::connect(
            &rig.target.endpoint,
            &rig.target.token,
            rig.target.limit,
            rig.target.protocol,
        )
        .map_err(|_| "the first client could not connect")?;
        let first = paddock_ipc::frame::encode(
            &paddock_ipc::Frame::Hello {
                version: paddock_ipc::frame::VERSION,
                protocol: rig.target.protocol,
                token: *rig.target.token.as_bytes(),
                pid: std::process::id(),
            },
            rig.target.limit,
        )
        .map_err(|_| "encode")?;
        rig.gate.arm(0);
        let mut c = paddock_ipc::endpoint::Stream::connect(&rig.target.endpoint, GUARD)
            .map_err(|_| "connect")?;
        let evidence = match client {
            BusyClient::WritesItsHello => {
                std::io::Write::write_all(&mut c, &first).map_err(|_| "write")?;
                rig.gate.evidence(GUARD)
            }
            BusyClient::WritesAfterTheGate => {
                let evidence = rig.gate.evidence(GUARD);
                // The server may have refused and closed by now: a failed write is what this client saw, not the rig's.
                drop(std::io::Write::write_all(&mut c, &first));
                evidence
            }
        };
        let raw = read_to_end(&mut c, READ_GUARD);
        drop(held);
        let decoded = ipc::decode_frames(&raw.bytes, rig.target.limit);
        Ok((
            endings::Observed {
                raw: Some(raw),
                decoded,
                evidence: vec![(Precondition::InputQueuedWhenRefused, evidence)],
                ..Default::default()
            },
            // The refused client's end, and the end of the held client's connection (dropped before the read).
            endings::Expect::reason(&["busy"]).with_ends(2),
        ))
    }

    pub fn scenario(s: Scenario) -> Result<(endings::Observed, endings::Expect), &'static str> {
        match s {
            Scenario::WrongToken
            | Scenario::WrongVersion
            | Scenario::MalformedFirstFrame
            | Scenario::ClientKeepsWriting => {
                let rig = rig(64);
                ipc::black_box_gated(&rig.target, s, Some(&rig.gate)).expect("covered")
            }
            Scenario::Busy => busy(BusyClient::WritesItsHello, READ_GUARD),
            Scenario::SilentClient => {
                let rig = rig(64);
                let mut c = paddock_ipc::endpoint::Stream::connect(&rig.target.endpoint, GUARD)
                    .map_err(|_| "connect")?;
                if !rig.clock.wait_for_pending(1, GUARD) {
                    return Err("the handshake never started its limit");
                }
                rig.clock.advance(Duration::from_secs(10));
                let raw = read_to_end(&mut c, READ_GUARD);
                let decoded = ipc::decode_frames(&raw.bytes, rig.target.limit);
                Ok((
                    endings::Observed {
                        raw: Some(raw),
                        decoded,
                        ..Default::default()
                    },
                    endings::Expect::reason(&["timeout"]),
                ))
            }
            Scenario::StopWithOpenConnection => {
                let mut rig = rig(64);
                let mut server = rig.server.take().expect("a server");
                let target = rig.target.clone();
                ipc::stop_with_open_connection(&target, move || server.stop())
            }
            _ => Err(
                "this scenario is not driven by the link-server rig; if it does not apply, declare it in [[not_applicable]]",
            ),
        }
    }

    #[allow(dead_code)]
    fn unused(_: NoCalls) {}
}

/// The ending test of the register's `ipc-link-server` entry.
#[test]
fn ipc_link_server_ends_every_connection_right_or_is_in_the_ratchet() {
    let applicability =
        Applicability::load_from(env!("CARGO_MANIFEST_DIR")).expect("the register file");
    let reports = run_all("ipc-link-server", &applicability, |s| {
        ipc::logged(|| link::scenario(s))
    });
    let ratchet = Ratchet::load_from(env!("CARGO_MANIFEST_DIR")).expect("the ratchet file");
    if let Err(e) = ratchet.check("ipc-link-server", &reports) {
        panic!("{e}\n\n{}", failure_text(&reports));
    }
}

mod launcher {
    use super::*;
    use crate::common::child_program;
    use paddock_ipc::testing::endings::ipc::decode_frames;
    use paddock_ipc::testing::endings::raw::{RawEnd, RawEndKind};
    use paddock_ipc::{ChildSpec, FrameLimit, NoCalls, launch};
    use std::sync::Arc;
    use std::time::Duration;

    pub fn scenario(s: Scenario) -> Result<(endings::Observed, endings::Expect), String> {
        if s != Scenario::StopWithOpenConnection {
            return Err(format!(
                "{s} is not driven by the launcher rig; if it does not apply, declare it in [[not_applicable]]"
            ));
        }
        let dir = tempfile::tempdir().map_err(|e| format!("no temporary folder: {e}"))?;
        let file = dir.path().join("seen");
        let spec = ChildSpec::new(child_program(), "the observer")
            .arg("observe")
            .arg(&file);
        let child = launch(&spec, Arc::new(NoCalls))
            .map_err(|e| format!("the observer child did not connect: {e}"))?;
        // The owner stops the child: the connection closes first (`Child::stop` closes the peer, then the lifeline).
        child.peer().close();
        let status = child
            .exit_status(Duration::from_secs(60))
            .ok_or("the observer child did not exit within 60 s")?;
        if !status.success() {
            return Err(format!("the observer child failed: {status}"));
        }
        let read = |ext: &str| format!("{}.{ext}", file.display());
        let bytes = std::fs::read(read("bin"))
            .map_err(|e| format!("the observer wrote no bytes file: {e}"))?;
        let end = std::fs::read_to_string(read("end"))
            .map_err(|e| format!("the observer wrote no end file: {e}"))?;
        // Every end the child can report has its own arm; anything else is a rig failure, not a quiet "timed out".
        let kind = match end.as_str() {
            "eof" => RawEndKind::Eof,
            "reset" => RawEndKind::Error(std::io::ErrorKind::ConnectionReset),
            "timeout" => RawEndKind::TimedOut,
            other if other.starts_with("error ") => RawEndKind::Error(std::io::ErrorKind::Other),
            other => {
                return Err(format!(
                    "the observer reported an end this rig does not know: {other:?}"
                ));
            }
        };
        let decoded = decode_frames(&bytes, FrameLimit::default());
        Ok((
            endings::Observed {
                raw: Some(RawEnd { bytes, kind }),
                decoded,
                ..Default::default()
            },
            endings::Expect::reason(&["stopping", "closed"]),
        ))
    }
}

/// The ending test of the register's `ipc-launcher` entry.
#[test]
fn ipc_launcher_ends_every_connection_right_or_is_in_the_ratchet() {
    let applicability =
        Applicability::load_from(env!("CARGO_MANIFEST_DIR")).expect("the register file");
    let reports = run_all("ipc-launcher", &applicability, |s| {
        ipc::logged(|| launcher::scenario(s))
    });
    let ratchet = Ratchet::load_from(env!("CARGO_MANIFEST_DIR")).expect("the ratchet file");
    if let Err(e) = ratchet.check("ipc-launcher", &reports) {
        panic!("{e}\n\n{}", failure_text(&reports));
    }
}

#[test]
fn a_rig_that_cannot_drive_a_scenario_turns_the_run_red_and_a_declared_not_applicable_does_not() {
    let declared = Applicability::of("rigged", &[(Scenario::Stall, "declared in the register")]);
    let reports = run_all("rigged", &declared, |s| -> Result<_, String> {
        if s == Scenario::WrongToken {
            Err("the endpoint did not start".into())
        } else {
            Ok((
                endings::Observed::default(),
                endings::Expect {
                    reason: endings::ReasonRule::Optional,
                    end_line: false,
                    ends: 1,
                    preconditions: Vec::new(),
                },
            ))
        }
    });
    let stall = reports
        .iter()
        .find(|r| r.scenario == Scenario::Stall)
        .expect("a report");
    assert!(
        matches!(stall.outcome, Outcome::NotApplicable(_)),
        "a declared scenario is not run: {}",
        stall.summary()
    );
    let broken = reports
        .iter()
        .find(|r| r.scenario == Scenario::WrongToken)
        .expect("a report");
    assert!(
        matches!(broken.outcome, Outcome::RigFailure(_)),
        "{}",
        broken.summary()
    );
    let verdict = Ratchet::default().check("rigged", &reports);
    let message = verdict.expect_err("a rig failure must fail the run, not pass as not applicable");
    assert!(
        message.contains("rig failure: rigged / wrong-token: the endpoint did not start"),
        "{message}"
    );
    // Without the broken rig the same run is green.
    let fine = run_all("rigged", &declared, |_| -> Result<_, String> {
        Ok((
            endings::Observed::default(),
            endings::Expect {
                reason: endings::ReasonRule::Optional,
                end_line: false,
                ends: 1,
                preconditions: Vec::new(),
            },
        ))
    });
    Ratchet::default().check("rigged", &fine).expect("green");
}

fn observed_with_log(lines: &[&str]) -> endings::Observed {
    endings::Observed {
        log: lines.iter().map(|l| (*l).to_string()).collect(),
        ..Default::default()
    }
}

#[test]
fn a_connection_must_end_exactly_once_and_with_a_drain_outcome_of_eof() {
    let one = "INFO connection ended id=p1 phase=open code=closed drain=eof";
    let two = "INFO connection ended id=p2 phase=open code=closed drain=eof";
    let optional = endings::Expect::optional_reason();
    assert!(judge(&observed_with_log(&[one]), &optional).is_empty());
    assert_eq!(
        kinds(&judge(&observed_with_log(&[one, two]), &optional)),
        vec!["end-line-count"],
        "two end lines for one connection"
    );
    assert!(
        judge(
            &observed_with_log(&[one, two]),
            &optional.clone().with_ends(2)
        )
        .is_empty()
    );
    assert_eq!(
        kinds(&judge(
            &observed_with_log(&[one, one]),
            &optional.clone().with_ends(2)
        )),
        vec!["duplicate-end-id"],
        "one connection that ended twice, or two endpoints sharing a counter"
    );
    assert_eq!(
        kinds(&judge(&observed_with_log(&[]), &optional)),
        vec!["no-end-line"]
    );
    let none = "INFO connection ended id=p1 phase=open code=closed drain=none";
    assert_eq!(
        kinds(&judge(&observed_with_log(&[none]), &optional)),
        vec!["drain-not-eof"],
        "an endpoint that does not drain is a ratchet entry (WP-2), not a pass"
    );
    assert_eq!(
        kinds(&judge(
            &observed_with_log(&["INFO connection ended id=p1 code=closed"]),
            &optional
        )),
        vec!["drain-not-eof"]
    );
}
