#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

//! How the service's endpoint ends a connection, driven through the scenarios of the ending harness (ADR 0072 WP-0, I24 to
//! I26): the raw frames and the exact end of the stream a client sees. What fails today is recorded per instance in
//! `docs/ipc-endpoint-ratchet.toml`; the test is red without it.

use paddock_ipc::endpoint::Stream;
use paddock_ipc::testing::endings::applicability::Applicability;
use paddock_ipc::testing::endings::ratchet::Ratchet;
use paddock_ipc::testing::endings::raw::{read_to_end, send_then_keep_writing};
use paddock_ipc::testing::endings::{
    failure_text, run_all, Decoded, Expect, Observed, Precondition, QueueGate, QueueHandle, Scenario, KEEP_WRITING_BYTES,
};
use pitcrew_core::protocol::{decode_body, encode_frame, frame_len, read_frame, write_frame, Request, Response, PROTOCOL_VERSION};
use pitcrew_service::host::Host;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const GUARD: Duration = Duration::from_secs(30);
/// The wait for an answer that a right service gives at once; it decides only whether the service answered at all.
const ANSWER_GUARD: Duration = Duration::from_secs(10);

fn connect(data: &Path) -> Stream {
    #[cfg(windows)]
    let name = pitcrew_service::ipc::pipe_name(data);
    #[cfg(unix)]
    let name = data.join(pitcrew_service::ipc::SOCKET_FILE).display().to_string();
    // The service binds on its own thread and announces nothing: poll the connect (bounded) until it listens.
    let deadline = std::time::Instant::now() + GUARD;
    loop {
        match Stream::connect(&name, Duration::from_secs(1)) {
            Ok(s) => return s,
            Err(e) => {
                assert!(std::time::Instant::now() < deadline, "the service did not listen: {e}");
                std::thread::yield_now();
            }
        }
    }
}

/// The reasons in the raw bytes: every `Response::Error` carries its code.
fn decode(bytes: &[u8]) -> Decoded {
    let mut d = Decoded::default();
    let mut at = 0;
    while at + 4 <= bytes.len() {
        let Ok(len) = frame_len([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) else {
            break;
        };
        if at + 4 + len > bytes.len() {
            break;
        }
        match decode_body::<Response>(&bytes[at + 4..at + 4 + len]) {
            Ok(Response::Error(e)) => {
                d.reasons.push(serde_json::to_value(e.code).unwrap().as_str().unwrap().to_string());
                d.after_last_reason = 0;
            }
            Ok(_) if !d.reasons.is_empty() => d.after_last_reason += 1,
            _ => {}
        }
        at += 4 + len;
    }
    d.partial_tail = bytes.len() - at;
    d
}

fn hello(protocol: u32) -> Vec<u8> {
    encode_frame(&Request::Hello { protocol }).unwrap()
}

fn say_hello(s: &mut Stream) {
    write_frame(
        s,
        &Request::Hello {
            protocol: PROTOCOL_VERSION,
        },
    )
    .unwrap();
    s.set_read_timeout(Some(GUARD)).unwrap();
    assert!(matches!(read_frame::<Response>(s).unwrap(), Response::Hello { .. }));
}

/// The service's act gate holding it at `gate` (ADR 0122 §2): every accepted connection passes it; an armed one is held until
/// its bytes are queued.
fn act_gate(gate: &Arc<QueueGate>) -> pitcrew_service::ipc::ActGate {
    let gate = gate.clone();
    Arc::new(move |raw| {
        #[cfg(unix)]
        let handle = QueueHandle::Fd(raw);
        #[cfg(windows)]
        let handle = QueueHandle::Pipe(raw);
        gate.hold(handle);
    })
}

fn observe(mut c: Stream, guard: Duration, expect: Expect) -> Result<(Observed, Expect), &'static str> {
    let raw = read_to_end(&mut c, guard);
    let decoded = decode(&raw.bytes);
    Ok((
        Observed {
            raw: Some(raw),
            decoded,
            ..Observed::default()
        },
        expect,
    ))
}

#[allow(
    clippy::disallowed_methods,
    reason = "a test builds its scratch path from the OS temp folder and removes it"
)]
fn scenario(s: Scenario) -> Result<(Observed, Expect), &'static str> {
    use std::io::Write;
    if !matches!(
        s,
        Scenario::WrongVersion
            | Scenario::MalformedFirstFrame
            | Scenario::ClientKeepsWriting
            | Scenario::Busy
            | Scenario::StopWithOpenConnection
    ) {
        return Err("this scenario is not driven by the service rig; if it does not apply, declare it in [[not_applicable]]");
    }
    // A short path: the service's Unix socket lives in this folder (the length limit of a socket path).
    let scratch = paddock_testkit::canonical_tempdir();
    let data = scratch.join("pe");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("pitcrew.toml"), "schema_version = 1\n").unwrap();
    let gate = Arc::new(QueueGate::new(paddock_ipc::testing::endings::raw::READ_GUARD));
    let mut host = Some(Host::start_with_act_gate(data.clone(), act_gate(&gate), |e| panic!("the IPC server failed: {e}")).unwrap());
    let result = match s {
        Scenario::WrongVersion | Scenario::ClientKeepsWriting => {
            let first = hello(PROTOCOL_VERSION + 1);
            let keeps_writing = s == Scenario::ClientKeepsWriting;
            if keeps_writing {
                // The service is held at its act gate until more than the hello is queued (ADR 0122).
                gate.arm(first.len());
            }
            let mut c = connect(&data);
            if keeps_writing {
                send_then_keep_writing(&mut c, &first, KEEP_WRITING_BYTES).unwrap();
            } else {
                c.write_all(&first).unwrap();
            }
            let expect = Expect::reason(&["protocolMismatch"]);
            observe(c, GUARD, expect).map(|(mut o, e)| {
                if keeps_writing {
                    o.evidence.push((Precondition::InputQueuedWhenRefused, gate.evidence(GUARD)));
                }
                (o, e)
            })
        }
        Scenario::MalformedFirstFrame => {
            let mut c = connect(&data);
            let mut garbage = 8u32.to_le_bytes().to_vec();
            garbage.extend_from_slice(&[0xff; 8]);
            c.write_all(&garbage).unwrap();
            observe(c, GUARD, Expect::reason(&["badRequest"]))
        }
        Scenario::Busy => {
            // 32 clients hold the 32 places; the 33rd must be told `busy`, not left waiting.
            let mut held: Vec<Stream> = (0..pitcrew_service::ipc::MAX_CONNECTIONS).map(|_| connect(&data)).collect();
            for c in &mut held {
                say_hello(c);
            }
            let mut c = connect(&data);
            c.write_all(&hello(PROTOCOL_VERSION)).unwrap();
            let r = observe(c, ANSWER_GUARD, Expect::reason(&["busy"]));
            drop(held);
            r
        }
        Scenario::StopWithOpenConnection => {
            let mut c = connect(&data);
            say_hello(&mut c);
            // The service stops with a client connected (the supervisor ends, the IPC runtime shuts down).
            host.take().unwrap().stop();
            observe(c, GUARD, Expect::reason(&["internal"]))
        }
        _ => unreachable!("filtered above"),
    };
    if let Some(host) = host {
        host.stop();
    }
    result
}

/// The ending test of the register's `pitcrew-service` entry.
#[test]
fn pitcrew_service_ends_every_connection_right_or_is_in_the_ratchet() {
    let applicability = Applicability::load_from(env!("CARGO_MANIFEST_DIR")).expect("the register file");
    let reports = run_all("pitcrew-service", &applicability, scenario);
    let ratchet = Ratchet::load_from(env!("CARGO_MANIFEST_DIR")).expect("the ratchet file");
    if let Err(e) = ratchet.check("pitcrew-service", &reports) {
        panic!("{e}\n\n{}", failure_text(&reports));
    }
}

/// The positive control of the service's act gate (ADR 0122 §3): a client whose hello is written only after the gate gave up is
/// a missed precondition, the rig's failure once the verdict blocks; one whose hello is written first is held.
#[allow(
    clippy::disallowed_methods,
    reason = "a test builds its scratch path from the OS temp folder and removes it"
)]
#[test]
fn the_act_gate_sees_a_client_that_wrote_nothing_and_one_that_wrote_its_hello() {
    use paddock_ipc::testing::endings::applicability::Applicability;
    use paddock_ipc::testing::endings::{conclude, Evidence, PreconditionVerdict};
    use std::io::Write;
    for writes_first in [false, true] {
        let scratch = paddock_testkit::canonical_tempdir();
        let data = scratch.join("pe");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("pitcrew.toml"), "schema_version = 1\n").unwrap();
        let gate = Arc::new(QueueGate::new(Duration::from_millis(200)));
        let host = Host::start_with_act_gate(data.clone(), act_gate(&gate), |e| panic!("the IPC server failed: {e}")).unwrap();
        gate.arm(0);
        let mut c = connect(&data);
        if writes_first {
            c.write_all(&hello(PROTOCOL_VERSION)).unwrap();
        }
        let evidence = gate.evidence(GUARD);
        drop(c);
        host.stop();
        if writes_first {
            assert!(evidence.is_held(), "{evidence}");
            continue;
        }
        assert!(matches!(evidence, Evidence::Missed(_)), "{evidence}");
        let report = conclude(
            "pitcrew-service",
            Scenario::ClientKeepsWriting,
            &Applicability::none(),
            PreconditionVerdict::Blocking,
            Ok::<_, String>((
                Observed {
                    evidence: vec![(Precondition::InputQueuedWhenRefused, evidence)],
                    ..Observed::default()
                },
                Expect::reason(&["protocolMismatch"]).without_end_line(),
            )),
        );
        assert!(
            report
                .summary()
                .starts_with("pitcrew-service client-keeps-writing: RIG FAILURE (precondition input-queued-when-refused not met: "),
            "{}",
            report.summary()
        );
    }
}
