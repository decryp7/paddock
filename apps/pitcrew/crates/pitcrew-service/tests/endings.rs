#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

//! How the service's endpoint ends a connection, driven through the scenarios of the ending harness (ADR 0072 WP-0, I24 to
//! I26): the raw frames and the exact end of the stream a client sees. What fails today is recorded per instance in
//! `docs/ipc-endpoint-ratchet.toml`; the test is red without it.

use paddock_ipc::endpoint::Stream;
use paddock_ipc::testing::endings::applicability::Applicability;
use paddock_ipc::testing::endings::ratchet::Ratchet;
use paddock_ipc::testing::endings::raw::{read_to_end, write_junk};
use paddock_ipc::testing::endings::{failure_text, run_all, Decoded, Expect, Observed, Scenario, KEEP_WRITING_BYTES};
use pitcrew_core::protocol::{decode_body, encode_frame, frame_len, read_frame, write_frame, Request, Response, PROTOCOL_VERSION};
use pitcrew_service::host::Host;
use std::path::Path;
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
    let data = std::env::temp_dir().join(format!(
        "pe-{}-{}",
        std::process::id(),
        Scenario::ALL.iter().position(|x| *x == s).unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("pitcrew.toml"), "schema_version = 1\n").unwrap();
    let mut host = Some(Host::start(data.clone(), |e| panic!("the IPC server failed: {e}")).unwrap());
    let result = match s {
        Scenario::WrongVersion | Scenario::ClientKeepsWriting => {
            let mut c = connect(&data);
            c.write_all(&hello(PROTOCOL_VERSION + 1)).unwrap();
            if s == Scenario::ClientKeepsWriting {
                write_junk(&mut c, KEEP_WRITING_BYTES);
            }
            let expect = Expect::reason(&["protocolMismatch"]);
            observe(
                c,
                GUARD,
                if s == Scenario::ClientKeepsWriting {
                    expect.with_unread_input()
                } else {
                    expect
                },
            )
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
    let _ = std::fs::remove_dir_all(&data);
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
