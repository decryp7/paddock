//! How the loopback listener ends a connection, driven through the scenarios of the ending harness (ADR 0072 WP-0): the
//! raw bytes and the exact end of the stream a client sees, below any library. What fails today is recorded per instance
//! in `docs/ipc-endpoint-ratchet.toml`; the test is red without it.

use crate::common::rig;
use paddock_ipc::testing::endings::applicability::Applicability;
use paddock_ipc::testing::endings::log::LogCapture;
use paddock_ipc::testing::endings::ratchet::Ratchet;
use paddock_ipc::testing::endings::raw::{READ_GUARD, read_to_end, write_junk};
use paddock_ipc::testing::endings::{
    Decoded, Expect, KEEP_WRITING_BYTES, Observed, Scenario, failure_text, run_all,
};
use paddock_mcp::{KeyStore, serve_loopback};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, OnceLock};

/// The product's log (the `log` facade), routed into a capture so the harness can find the endpoint's end line and a rig
/// can print it when it fails.
fn captured() -> &'static LogCapture {
    static CAPTURE: OnceLock<LogCapture> = OnceLock::new();
    struct Sink(LogCapture);
    impl log::Log for Sink {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            self.0
                .push(format!("{}: {}", record.level(), record.args()));
        }
        fn flush(&self) {}
    }
    CAPTURE.get_or_init(|| {
        let capture = LogCapture::new();
        // One logger per test binary; another test may have installed its own, which only costs this test its log lines.
        if log::set_boxed_logger(Box::new(Sink(capture.clone()))).is_ok() {
            log::set_max_level(log::LevelFilter::Info);
        }
        capture
    })
}

/// Decodes the listener's JSON lines: a refusal (`{"ok":false,"code":...}`) and the closing notification (D6) are reasons.
fn decode_lines(bytes: &[u8]) -> Decoded {
    let text = String::from_utf8_lossy(bytes);
    let mut d = Decoded::default();
    let complete = text.rfind('\n').map_or(0, |i| i + 1);
    d.partial_tail = text.len() - complete;
    for line in text[..complete].lines() {
        let v: Value = serde_json::from_str(line).unwrap_or(Value::Null);
        let refusal = (v["ok"] == Value::Bool(false))
            .then(|| v["code"].as_str().map(str::to_string))
            .flatten();
        let closing = (v["method"] == "notifications/paddock/closing")
            .then(|| v["params"]["code"].as_str().map(str::to_string))
            .flatten();
        if let Some(code) = refusal.or(closing) {
            d.reasons.push(code);
            d.after_last_reason = 0;
        } else if !d.reasons.is_empty() {
            d.after_last_reason += 1;
        }
    }
    d
}

struct Rig {
    serving: Option<paddock_mcp::Serving>,
    addr: SocketAddr,
    key: String,
    log: &'static LogCapture,
    from: usize,
}

fn start(max_connections: usize) -> Rig {
    let log = captured();
    let keys = Arc::new(KeyStore::new());
    let (_, key) = keys.issue("ending test", 1).expect("a key");
    let serving = serve_loopback(
        rig().server,
        keys.clone(),
        TcpListener::bind("127.0.0.1:0").expect("a loopback port"),
        max_connections,
    )
    .expect("serving");
    let addr = serving.addr();
    Rig {
        serving: Some(serving),
        addr,
        key,
        log,
        from: log.len(),
    }
}

fn observe(
    rig: &Rig,
    mut client: TcpStream,
    expect: Expect,
) -> Result<(Observed, Expect), &'static str> {
    let raw = read_to_end(&mut client, READ_GUARD);
    let decoded = decode_lines(&raw.bytes);
    Ok((
        Observed {
            raw: Some(raw),
            decoded,
            log: rig.log.since(rig.from),
            ..Observed::default()
        },
        expect,
    ))
}

fn pair(rig: &Rig) -> Result<TcpStream, &'static str> {
    let mut c = TcpStream::connect(rig.addr).map_err(|_| "connect")?;
    c.write_all(format!("{{\"key\":\"{}\"}}\n", rig.key).as_bytes())
        .map_err(|_| "write")?;
    let mut line = String::new();
    BufReader::new(c.try_clone().map_err(|_| "clone")?)
        .read_line(&mut line)
        .map_err(|_| "no answer")?;
    if !line.contains("\"ok\":true") {
        return Err("the client was not paired");
    }
    Ok(c)
}

fn scenario(s: Scenario) -> Result<(Observed, Expect), &'static str> {
    match s {
        Scenario::WrongToken | Scenario::MalformedFirstFrame | Scenario::ClientKeepsWriting => {
            let rig = start(4);
            let mut c = TcpStream::connect(rig.addr).map_err(|_| "connect")?;
            let (first, code): (&[u8], &str) = if s == Scenario::MalformedFirstFrame {
                (b"this is not json\n", "key")
            } else {
                (b"{\"key\":\"pmk_wrong\"}\n", "key")
            };
            c.write_all(first).map_err(|_| "write")?;
            if s == Scenario::ClientKeepsWriting {
                write_junk(&mut c, KEEP_WRITING_BYTES);
            }
            let expect = Expect::reason(&[code, "handshake"]);
            observe(
                &rig,
                c,
                if s == Scenario::ClientKeepsWriting {
                    expect.with_unread_input()
                } else {
                    expect
                },
            )
        }
        Scenario::Busy => {
            let rig = start(1);
            let held = pair(&rig)?;
            let mut c = TcpStream::connect(rig.addr).map_err(|_| "connect")?;
            c.write_all(format!("{{\"key\":\"{}\"}}\n", rig.key).as_bytes())
                .map_err(|_| "write")?;
            let result = observe(&rig, c, Expect::reason(&["busy"]));
            drop(held);
            result
        }
        Scenario::StopWithOpenConnection => {
            let mut rig = start(4);
            let c = pair(&rig)?;
            rig.serving.take().ok_or("no serving")?.stop();
            observe(&rig, c, Expect::reason(&["stopping"]))
        }
        _ => Err(
            "this scenario is not driven by the MCP rig; if it does not apply, declare it in [[not_applicable]]",
        ),
    }
}

/// The ending test of the register's `mcp-loopback` entry.
#[test]
fn mcp_loopback_ends_every_connection_right_or_is_in_the_ratchet() {
    let applicability =
        Applicability::load_from(env!("CARGO_MANIFEST_DIR")).expect("the register file");
    let reports = run_all("mcp-loopback", &applicability, scenario);
    let ratchet = Ratchet::load_from(env!("CARGO_MANIFEST_DIR")).expect("the ratchet file");
    if let Err(e) = ratchet.check("mcp-loopback", &reports) {
        panic!("{e}\n\n{}", failure_text(&reports));
    }
}
