//! The plugin process' side: [`serve`] runs the one child loop of `pitwall-ipc` and answers the host's
//! [`Request`]s with the plugin's implementations ([`Plugin::answer`]).

use crate::plugin::{ItemSink, Plugin, PluginError};
use crate::protocol::{GenerateItem, Request, CONTRACT_VERSION};
use pitwall_ipc::peer::{decode_request, encode_reply, CallCtx, Handler};
use pitwall_ipc::RemoteError;
use std::process::ExitCode;
use std::sync::Arc;

/// Serves `plugin` in this process: a plugin's whole `main` is `pitwall_plugin_api::serve(my_plugin())`. Connects
/// to Pitwall (the endpoint and token arrive on stdin), answers until Pitwall closes the connection, and exits
/// when Pitwall goes away. Returns a failure exit code, with the reason on standard error, when the manifest does not
/// match the plugin or Pitwall cannot be reached.
pub fn serve(plugin: Plugin) -> ExitCode {
    if let Err(e) = plugin.check() {
        eprintln!("pitwall plugin: {e}");
        return ExitCode::from(2);
    }
    let handler: Arc<dyn Handler> = Arc::new(PluginHandler(plugin));
    match pitwall_ipc::child::child_main("Pitwall", CONTRACT_VERSION, move |_| handler) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pitwall plugin: {e}");
            ExitCode::from(1)
        }
    }
}

/// Answers the host's calls for one plugin.
pub struct PluginHandler(pub Plugin);

impl Handler for PluginHandler {
    fn call(&self, ctx: &CallCtx, body: &[u8]) -> Result<Vec<u8>, RemoteError> {
        let request: Request = decode_request(body)?;
        let mut sink = Stream(ctx);
        let reply = self.0.answer(request, &mut sink);
        if ctx.is_cancelled() {
            return Err(CallCtx::cancelled_error());
        }
        encode_reply(&reply.map_err(RemoteError::from)?)
    }
}

/// The items of a streamed call, sent as the host grants credit.
struct Stream<'a>(&'a CallCtx);

impl ItemSink for Stream<'_> {
    fn item(&mut self, item: GenerateItem) -> Result<(), PluginError> {
        self.0.send(&item).map_err(|e| PluginError::new(e.code, e.message))
    }

    fn cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}
