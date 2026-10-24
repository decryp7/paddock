//! A child process for paddock-ipc's own tests; never shipped by an app. The first argument picks its behaviour:
//!
//! - `echo`: connects and answers every call with its body (`fail` → a typed error, `panic` → a panic, `big <n>` → an
//!   answer of n bytes, `pid` → its pid)
//! - `protocol <major.minor>`: `echo`, speaking that protocol
//! - `exit`: exits at once with code 3, without connecting
//! - `silent`: never connects
//! - `wrong-token`: connects with a token of its own; exits 0 when refused
//! - `stubborn`: `echo` without a lifeline, and keeps running after the connection closes
//! - `companion <discovery file> <build>`: a link server for that build until a client calls `stop`; prints `ready` once
//!   its discovery file is written
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_ipc::link::{LinkServer, ServerConfig};
use paddock_ipc::{
    CallCtx, Client, FrameLimit, Handler, ProtocolVersion, RemoteError, Token, child,
};
use std::sync::{Arc, Condvar, Mutex};

fn echo(_: &CallCtx, body: &[u8]) -> Result<Vec<u8>, RemoteError> {
    match body {
        b"fail" => Err(RemoteError::new("bad", "typed failure")),
        b"panic" => panic!("handler panicked"),
        b"pid" => Ok(std::process::id().to_string().into_bytes()),
        other => match std::str::from_utf8(other)
            .ok()
            .and_then(|s| s.strip_prefix("big "))
        {
            Some(n) => Ok(vec![
                7;
                n.parse().map_err(|_| RemoteError::new(
                    "bad",
                    "not a number"
                ))?
            ]),
            None => Ok(other.to_vec()),
        },
    }
}

fn park_forever() -> ! {
    loop {
        std::thread::park();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
    match arg(1).as_str() {
        "exit" => std::process::exit(3),
        "silent" => park_forever(),
        "wrong-token" => {
            let b = child::read_bootstrap(std::io::stdin().lock()).unwrap();
            let r = Client::connect(
                &b.endpoint,
                &Token::from_bytes([9; 32]),
                FrameLimit::default(),
                ProtocolVersion::UNVERSIONED,
            );
            std::process::exit(if r.is_err() { 0 } else { 1 });
        }
        "stubborn" => {
            let b = child::read_bootstrap(std::io::stdin().lock()).unwrap();
            let conn = child::connect(&b, ProtocolVersion::UNVERSIONED).unwrap();
            let peer = paddock_ipc::Peer::start(
                conn,
                Arc::new(echo),
                paddock_ipc::PeerOptions::new("host"),
            )
            .unwrap();
            peer.wait_closed(None);
            park_forever();
        }
        "protocol" => {
            let version: ProtocolVersion = arg(2).parse().unwrap();
            let r = child::serve(
                "host",
                version,
                |_| Arc::new(echo) as Arc<dyn Handler>,
                |_, _| {},
            );
            std::process::exit(if r.is_ok() { 0 } else { 2 });
        }
        "companion" => {
            let stopped = Arc::new((Mutex::new(false), Condvar::new()));
            let s = stopped.clone();
            let handler = move |ctx: &CallCtx, body: &[u8]| -> Result<Vec<u8>, RemoteError> {
                if body == b"stop" {
                    *s.0.lock().unwrap() = true;
                    s.1.notify_all();
                    return Ok(Vec::new());
                }
                echo(ctx, body)
            };
            let server =
                LinkServer::start(ServerConfig::new(arg(2), arg(3)), Arc::new(handler)).unwrap();
            // The test reads this line as its "ready" event instead of polling.
            println!("ready");
            let guard = stopped.0.lock().unwrap();
            drop(stopped.1.wait_while(guard, |s| !*s).unwrap());
            drop(server);
        }
        _ => {
            let r = child::serve(
                "host",
                ProtocolVersion::UNVERSIONED,
                |_| Arc::new(echo) as Arc<dyn Handler>,
                |_, _| {},
            );
            std::process::exit(if r.is_ok() { 0 } else { 2 });
        }
    }
}
