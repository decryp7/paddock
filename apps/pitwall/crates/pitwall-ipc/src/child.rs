//! The plugin process' side (ADR 0008 §2): [`child_main`]
//!
//! 1. reads its [`Bootstrap`] (one bounded JSON line on stdin);
//! 2. connects to the host and authenticates with the token, saying which contract it speaks;
//! 3. answers the host's calls with its handler until the connection closes, then returns (the process exits).
//!
//! It also exits at once when its stdin closes (the lifeline: the host stopped it or died), so nothing is left
//! behind. The sandbox was applied by the launcher before the process started; nothing here can widen it.

use crate::frame::ContractVersion;
use crate::launch::Bootstrap;
use crate::link::Token;
use crate::peer::{Duplex, Handler, Peer, PeerOptions};
use crate::IpcError;
use std::io::{self, BufRead, Read};
use std::sync::Arc;

/// Largest bootstrap line read from stdin.
const MAX_BOOTSTRAP: u64 = 64 * 1024;

/// Reads the bootstrap line from `input` (at most 64 KiB).
pub fn read_bootstrap(input: impl Read) -> io::Result<Bootstrap> {
    let mut line = String::new();
    io::BufReader::new(input.take(MAX_BOOTSTRAP)).read_line(&mut line)?;
    serde_json::from_str(line.trim()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("bad bootstrap: {e}")))
}

/// The lifeline: when the host closes stdin (or dies), exit at once.
pub fn lifeline() {
    let spawned = std::thread::Builder::new().name("pitwall-lifeline".into()).spawn(|| {
        let mut sink = [0u8; 256];
        while matches!(io::stdin().read(&mut sink), Ok(n) if n > 0) {}
        std::process::exit(0);
    });
    drop(spawned);
}

/// Connects to the host named in `bootstrap` and authenticates: the raw stream after the handshake (tests and
/// tools that speak frames themselves; plugins use [`child_main`]).
pub fn connect(bootstrap: &Bootstrap, label: &str, contract: ContractVersion) -> Result<Box<dyn Duplex>, IpcError> {
    let token = Token::from_hex(&bootstrap.token).ok_or_else(|| IpcError::Protocol("bad token in the bootstrap".into()))?;
    let (address, label_owned) = (bootstrap.address.clone(), label.to_string());
    crate::runtime::run(async move { crate::link::handshake(&address, &token, &label_owned, contract).await }).map_err(IpcError::Spawn)?
}

/// A plugin process' `main`; see the module docs. `label` names the host in messages ("Pitwall"); `make` builds
/// the handler once connected.
pub fn child_main(label: &str, contract: ContractVersion, make: impl FnOnce(&Bootstrap) -> Arc<dyn Handler>) -> io::Result<()> {
    let bootstrap = read_bootstrap(io::stdin().lock())?;
    let stream = connect(&bootstrap, label, contract).map_err(|e| io::Error::other(e.to_string()))?;
    let handler = make(&bootstrap);
    let mut opts = PeerOptions::new(label);
    opts.max_frame = bootstrap.max_frame;
    let rt = crate::runtime::shared().map_err(io::Error::other)?;
    let peer = Peer::start(stream, handler, opts, rt.handle());
    lifeline();
    peer.wait_closed(None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bootstrap_is_one_bounded_line() {
        let b = Bootstrap {
            address: "a".into(),
            token: "t".into(),
            max_frame: 1024,
        };
        let line = format!("{}\nrest", serde_json::to_string(&b).unwrap());
        assert_eq!(read_bootstrap(line.as_bytes()).unwrap(), b);
        assert_eq!(read_bootstrap(&b"nope\n"[..]).unwrap_err().kind(), io::ErrorKind::InvalidData);
        let long = format!("{{\"address\":\"{}\"}}\n", "a".repeat(70_000));
        assert!(read_bootstrap(long.as_bytes()).is_err());
        assert!(matches!(connect(&b, "Pitwall", ContractVersion::default()), Err(IpcError::Protocol(m)) if m.contains("token")));
    }
}
