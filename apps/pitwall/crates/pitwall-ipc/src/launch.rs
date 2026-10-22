//! The one launcher (ADR 0008 §2): creates the host's private endpoint, starts the plugin process in its sandbox
//! ([`crate::process`]), tells it the endpoint and the token on stdin (one JSON line), and accepts its authenticated
//! connection. The plugin side of the same contract is [`crate::child`].

use crate::endpoint::PrivateEndpoint;
use crate::link::{Access, Listener};
use crate::peer::{Handler, Peer, PeerOptions};
use crate::process::{ChildProcess, SpawnSpec};
use crate::IpcError;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// What a plugin process reads first: one JSON line on stdin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    /// The host's endpoint.
    pub address: String,
    /// The per-launch token, hex.
    pub token: String,
    /// Largest frame on the connection.
    pub max_frame: usize,
}

/// How to start one plugin process.
#[derive(Debug, Clone)]
pub struct ChildSpec {
    /// The program, its arguments, environment and sandbox.
    pub spawn: SpawnSpec,
    /// How the connection runs (label, frame limit, contract).
    pub peer: PeerOptions,
}

impl ChildSpec {
    /// A spec whose connection is labelled like the process.
    pub fn new(spawn: SpawnSpec) -> ChildSpec {
        let mut peer = PeerOptions::new(spawn.label.clone());
        // The host answers no calls of a plugin: few at once is plenty, and bounds a flood.
        peer.max_concurrent_calls = 4;
        ChildSpec { spawn, peer }
    }
}

/// A started plugin with its authenticated connection. Killed on drop.
pub struct Child {
    process: Arc<ChildProcess>,
    peer: Peer,
    /// Kept so the private directory lives as long as the connection.
    _endpoint: PrivateEndpoint,
}

impl Child {
    /// The connection.
    pub fn peer(&self) -> &Peer {
        &self.peer
    }

    /// The process.
    pub fn process(&self) -> &ChildProcess {
        &self.process
    }

    /// The process id.
    pub fn pid(&self) -> u32 {
        self.process.pid()
    }

    /// How the plugin ended (for messages after a broken call).
    pub fn exit_text(&self) -> String {
        self.process.exit_text()
    }

    /// Whether the process has exited.
    pub fn exited(&self) -> bool {
        self.process.exited()
    }

    /// Closes the connection (a well-behaved plugin exits at once) and kills the process if it did not.
    pub fn stop(&self) {
        self.peer.close();
        self.process.kill();
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Who may open the endpoint: on Windows the plugin's AppContainer too (it runs below low integrity).
fn access(spawn: &SpawnSpec) -> Result<Access, IpcError> {
    #[cfg(windows)]
    {
        let c = crate::contain::Container::open(&spawn.sandbox.name)
            .map_err(|e| IpcError::Spawn(format!("cannot prepare the sandbox of {}: {e}", spawn.label)))?;
        Ok(Access::OwnerInContainer(c.sid_string))
    }
    #[cfg(not(windows))]
    {
        let _ = spawn;
        Ok(Access::Owner)
    }
}

/// Starts the plugin described by `spec` and accepts its connection; `handler` answers the calls the plugin makes
/// back ([`crate::peer::NoCalls`] when there are none). Blocks (run it off async tasks).
pub fn launch(spec: &ChildSpec, handler: Arc<dyn Handler>) -> Result<Child, IpcError> {
    let listener = Listener::create(&spec.spawn.endpoint_prefix, access(&spec.spawn)?)
        .map_err(|e| IpcError::Spawn(format!("cannot prepare a private connection for {}: {e}", spec.spawn.label)))?;
    let bootstrap = Bootstrap {
        address: listener.address().to_string(),
        token: listener.token().to_hex(),
        max_frame: spec.peer.max_frame,
    };
    let process = Arc::new(ChildProcess::spawn(&spec.spawn, listener.address())?);
    if let Err(e) = process.send_line(&bootstrap) {
        process.kill();
        return Err(e);
    }
    let pid = process.pid();
    let exited = {
        let (p, label) = (process.clone(), spec.spawn.label.clone());
        move || p.try_exit().map(|s| format!("{label} exited while starting ({s})"))
    };
    let (window, opts) = (spec.spawn.connect_window, spec.peer.clone());
    let accepted = crate::runtime::run(async move { listener.accept(Some(pid), window, exited, handler, opts).await })
        .map_err(IpcError::Spawn)
        .and_then(|r| r);
    match accepted {
        Ok((peer, endpoint)) => Ok(Child {
            process,
            peer,
            _endpoint: endpoint,
        }),
        Err(e) => {
            process.kill();
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bootstrap_is_one_json_line() {
        let b = Bootstrap {
            address: "a".into(),
            token: "t".into(),
            max_frame: 1024,
        };
        let line = serde_json::to_string(&b).unwrap();
        assert!(!line.contains('\n'));
        assert_eq!(serde_json::from_str::<Bootstrap>(&line).unwrap(), b);
        let spec = ChildSpec::new(SpawnSpec::new("x", "the x", crate::SandboxSpec::locked("n", "/p")));
        assert_eq!(spec.peer.label, "the x");
        assert_eq!(spec.peer.max_concurrent_calls, 4);
    }
}
