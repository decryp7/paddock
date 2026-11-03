//! Starting a plugin and talking to it, behind two small traits so the supervision logic can be tested with fakes.

use crate::config::HostConfig;
use crate::discovery::Discovered;
use dyno_plugin_api::protocol::{wire, MAX_FRAME};
use dyno_plugin_api::{PluginMessage, PluginReply, PluginRequest};
use paddock_ipc::{launch, BodyPeer, Child, ChildSpec, FrameLimit, IpcError, NoCalls};
use std::sync::Arc;
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

/// The real launcher: a child process over `paddock-ipc`, in a job object on Windows and with the memory watch of `paddock-proc`. The plugin
/// gets `DYNO_PLUGIN_ID` and `DYNO_PLUGIN_DIR` in its environment and its folder as working directory.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessLauncher;

impl Launcher for ProcessLauncher {
    fn launch(&self, plugin: &Discovered, config: &HostConfig) -> Result<Arc<dyn PluginConnection>, IpcError> {
        let mut spec = ChildSpec::new(&plugin.executable, format!("plugin \"{}\"", plugin.id));
        spec.envs.push(("DYNO_PLUGIN_ID".into(), plugin.id.clone().into()));
        spec.envs.push(("DYNO_PLUGIN_DIR".into(), plugin.dir.clone().into_os_string()));
        spec.clean_env = false;
        spec.current_dir = Some(plugin.dir.clone());
        spec.memory_limit = config.memory_limit;
        // The Windows job object's hard commit limit, as in Dyno 1.0: an allocation past it fails in the plugin at once.
        spec.memory_hard = true;
        spec.inherit_stderr = config.inherit_stderr;
        spec.limit = FrameLimit::new(MAX_FRAME);
        spec.protocol = wire();
        spec.host_name = "Dyno".into();
        spec.start_timeout = config.timeouts.start;
        let began = std::time::Instant::now();
        let child = match launch(&spec, Arc::new(NoCalls)) {
            Ok(child) => child,
            Err(e) => {
                log::warn!(Plugin = plugin.id.as_str(), StartMs = elapsed_ms(began), Error = e.to_string().as_str(); "Plugin could not be started");
                return Err(e);
            }
        };
        log::info!(Plugin = plugin.id.as_str(), Pid = child.pid(), StartMs = elapsed_ms(began); "Plugin started and connected");
        Ok(Arc::new(ProcessConnection {
            id: plugin.id.clone(),
            peer: BodyPeer::wrap(child.peer().clone()),
            child,
        }))
    }
}

fn elapsed_ms(since: std::time::Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

struct ProcessConnection {
    id: String,
    peer: BodyPeer<PluginMessage>,
    child: Child,
}

impl PluginConnection for ProcessConnection {
    fn call(&self, req: PluginRequest, timeout: Duration) -> Result<PluginReply, IpcError> {
        match self.peer.call(PluginMessage::Request(req), timeout)? {
            PluginMessage::Reply(reply) => Ok(reply),
            PluginMessage::Request(_) => Err(IpcError::Protocol("the plugin answered with a request".into())),
        }
    }

    fn stop(&self, grace: Duration) {
        self.child.stop(grace);
    }

    fn kill(&self) {
        self.child.kill();
    }

    fn exit_status(&self) -> Option<String> {
        let status = self.child.exit_status(Duration::from_millis(500))?;
        log::warn!(Plugin = self.id.as_str(), Pid = self.child.pid(), Status = status.to_string().as_str(); "Plugin exited");
        Some(match status.code() {
            // Windows NTSTATUS codes (0xC0000409: aborted, e.g. out of memory under the cap) read better in hex.
            Some(code) if !(0..=0xffff).contains(&code) => format!("exit code {:#010x}", code as u32),
            _ => status.to_string(),
        })
    }
}
