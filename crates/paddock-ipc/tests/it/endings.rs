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
}

mod link {
    use super::*;
    use paddock_ipc::link::{LinkServer, ServerConfig, discovery};
    use paddock_ipc::testing::endings::ipc::IpcTarget;
    use paddock_ipc::testing::endings::raw::{READ_GUARD, read_to_end};
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
        _dir: tempfile::TempDir,
    }

    fn rig(max_connections: usize) -> Rig {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let path = dir.path().join("agent.json");
        let clock = FakeClock::new();
        let mut c = ServerConfig::new(&path, "ending-test");
        c.socket_dir = Some(dir.path().to_path_buf());
        c.clock = Arc::new(clock.clone());
        c.handshake_timeout = Duration::from_secs(10);
        c.max_connections = max_connections;
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
            _dir: dir,
        }
    }

    pub fn scenario(s: Scenario) -> Result<(endings::Observed, endings::Expect), &'static str> {
        match s {
            Scenario::WrongToken
            | Scenario::WrongVersion
            | Scenario::MalformedFirstFrame
            | Scenario::ClientKeepsWriting => {
                let rig = rig(64);
                ipc::black_box(&rig.target, s).expect("covered")
            }
            Scenario::Busy => {
                let rig = rig(1);
                // One connection takes the only place.
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
                let mut c = paddock_ipc::endpoint::Stream::connect(&rig.target.endpoint, GUARD)
                    .map_err(|_| "connect")?;
                std::io::Write::write_all(&mut c, &first).map_err(|_| "write")?;
                let raw = read_to_end(&mut c, READ_GUARD);
                drop(held);
                let decoded = ipc::decode_frames(&raw.bytes, rig.target.limit);
                Ok((
                    endings::Observed {
                        raw: Some(raw),
                        decoded,
                        ..Default::default()
                    },
                    endings::Expect::reason(&["busy"]).with_unread_input(),
                ))
            }
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
    let reports = run_all("ipc-link-server", &applicability, link::scenario);
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
    let reports = run_all("ipc-launcher", &applicability, launcher::scenario);
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
                    unread_input: false,
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
                unread_input: false,
            },
        ))
    });
    Ratchet::default().check("rigged", &fine).expect("green");
}
