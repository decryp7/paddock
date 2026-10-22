//! A child process for dyno-ipc's own tests; never shipped. The first argument picks its behaviour:
//!
//! - `echo`: connects and answers every call with the same string; `alloc <MiB>` allocates and touches that much
//! - `exit`: exits at once with code 3, without connecting
//! - `silent`: never connects
//! - `wrong-token`: connects with a token of its own
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used)]

use dyno_ipc::{ChildLink, Client, Peer, PeerOptions, RemoteError, Token};
use std::time::Duration;

fn echo(body: String) -> Result<String, RemoteError> {
    if let Some(mib) = body.strip_prefix("alloc ") {
        let mib: usize = mib.parse().map_err(|_| RemoteError::new("bad", "not a number"))?;
        let mut v = vec![0u8; mib * 1024 * 1024];
        for i in (0..v.len()).step_by(4096) {
            v[i] = 1;
        }
        return Ok(format!("allocated {}", v.len()));
    }
    Ok(body)
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    match mode.as_str() {
        "exit" => std::process::exit(3),
        "silent" => loop {
            std::thread::sleep(Duration::from_secs(60));
        },
        "wrong-token" => {
            let b = ChildLink::read_bootstrap(std::io::stdin().lock()).unwrap();
            let r = Client::connect(&b.endpoint, &Token::from_bytes([9; 32]));
            std::process::exit(if r.is_err() { 0 } else { 1 });
        }
        _ => {
            let conn = ChildLink::from_stdin().expect("connect");
            let peer = Peer::start(conn, PeerOptions::new().serve(echo)).expect("peer");
            peer.wait_closed(None);
        }
    }
}
