//! Starting a plugin and talking to it, behind two small traits so the supervision logic can be tested with fakes.

use crate::config::HostConfig;
use crate::discovery::Discovered;
use dyno_ipc::child::launch;
use dyno_ipc::{ChildProcess, ChildSpec, IpcError, Peer, PeerOptions};
use dyno_plugin_api::{PluginMessage, PluginReply, PluginRequest};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A running plugin.
pub trait PluginConnection: Send + Sync {
    /// Sends `req` and waits up to `timeout` for the reply.
    fn call(&self, req: PluginRequest, timeout: Duration) -> Result<PluginReply, IpcError>;

    /// Closes the connection, waits up to `grace` for the plugin to exit, then kills it.
    fn stop(&self, grace: Duration);

    /// Kills it now.
    fn kill(&self);

    /// How it ended, once it has (for error messages), waiting briefly for the OS to report it.
    fn exit_status(&self) -> Option<String>;
}

/// Starts plugins.
pub trait Launcher: Send + Sync {
    /// Starts `plugin` and connects to it (the host then calls `Describe`).
    fn launch(&self, plugin: &Discovered, config: &HostConfig) -> Result<Arc<dyn PluginConnection>, IpcError>;
}

/// The real launcher: a child process over [`dyno_ipc`], in a job object with the memory cap on Windows. The plugin
/// gets `DYNO_PLUGIN_ID` and `DYNO_PLUGIN_DIR` in its environment and its folder as working directory.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessLauncher;

impl Launcher for ProcessLauncher {
    fn launch(&self, plugin: &Discovered, config: &HostConfig) -> Result<Arc<dyn PluginConnection>, IpcError> {
        let mut spec = ChildSpec::new(&plugin.executable);
        spec.envs.push(("DYNO_PLUGIN_ID".into(), plugin.id.clone().into()));
        spec.envs.push(("DYNO_PLUGIN_DIR".into(), plugin.dir.clone().into_os_string()));
        spec.current_dir = Some(plugin.dir.clone());
        spec.memory_limit = config.memory_limit;
        spec.inherit_stderr = config.inherit_stderr;
        let (child, peer) = launch::<PluginMessage>(&spec, PeerOptions::new(), config.timeouts.start)?;
        Ok(Arc::new(ProcessConnection {
            peer,
            child: Mutex::new(child),
        }))
    }
}

struct ProcessConnection {
    peer: Peer<PluginMessage>,
    child: Mutex<ChildProcess>,
}

impl ProcessConnection {
    fn child(&self) -> std::sync::MutexGuard<'_, ChildProcess> {
        self.child.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl PluginConnection for ProcessConnection {
    fn call(&self, req: PluginRequest, timeout: Duration) -> Result<PluginReply, IpcError> {
        match self.peer.call(PluginMessage::Request(req), timeout)? {
            PluginMessage::Reply(reply) => Ok(reply),
            PluginMessage::Request(_) => Err(IpcError::Protocol("the plugin answered with a request".into())),
        }
    }

    fn stop(&self, grace: Duration) {
        self.peer.close();
        let _ = self.child().stop(grace);
    }

    fn kill(&self) {
        self.peer.close();
        let _ = self.child().kill();
    }

    fn exit_status(&self) -> Option<String> {
        let status = self.child().wait_timeout(Duration::from_millis(500)).ok().flatten()?;
        Some(match status.code() {
            // Windows NTSTATUS codes (0xC0000409: aborted, e.g. out of memory under the cap) read better in hex.
            Some(code) if !(0..=0xffff).contains(&code) => format!("exit code {:#010x}", code as u32),
            _ => status.to_string(),
        })
    }
}
