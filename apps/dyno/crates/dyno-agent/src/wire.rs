//! How Dyno's processes speak to the agent over `paddock-ipc`: the one protocol version, the frame limit and the way to open
//! a connection, so the agent, the app and the tests cannot disagree. The command line (which must not link the agent) uses
//! the same constants from `dyno_core::protocol`.

use dyno_core::protocol::{MAX_FRAME, WIRE_MAJOR};
use paddock_ipc::{Client, Connection, FrameLimit, Host, IpcError, ProtocolVersion, Token};

/// The protocol the handshake carries: [`WIRE_MAJOR`], minor 0.
pub fn protocol() -> ProtocolVersion {
    ProtocolVersion::new(WIRE_MAJOR, 0)
}

/// The frame limit of every connection to the agent.
pub fn limit() -> FrameLimit {
    FrameLimit::new(MAX_FRAME)
}

/// A fresh private endpoint and token for the agent.
pub fn listen() -> std::io::Result<Host> {
    Ok(Host::listen(limit(), protocol())?.named("Dyno"))
}

/// Connects to the agent at `endpoint` with its `token`.
pub fn connect(endpoint: &str, token: &Token) -> Result<Connection, IpcError> {
    Client::connect(endpoint, token, limit(), protocol())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_client_of_the_same_protocol_connects_and_another_major_is_refused_with_what_to_update() {
        let mut host = listen().unwrap();
        let (endpoint, token) = (host.endpoint().to_string(), host.token().clone());
        let e = endpoint.clone();
        let t = token.clone();
        let ok = std::thread::spawn(move || connect(&e, &t));
        assert!(host.accept(Duration::from_secs(10)).is_ok());
        assert!(ok.join().unwrap().is_ok());
        let newer = ProtocolVersion::new(WIRE_MAJOR + 1, 0);
        let bad = std::thread::spawn(move || Client::connect(&endpoint, &token, limit(), newer));
        assert!(matches!(host.accept(Duration::from_secs(10)), Err(IpcError::Version(m)) if m.contains("update")));
        assert!(matches!(bad.join().unwrap(), Err(IpcError::Version(_))));
        assert_eq!(limit().get(), MAX_FRAME);
    }
}
