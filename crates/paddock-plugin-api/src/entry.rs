//! The process entry of a plugin: [`serve`] is the whole `main`. It only runs in a process the app started (it reads its
//! bootstrap from standard input), so tests reach everything behind it through the fake host instead; this file is the
//! one thing left out of the coverage gate, like `paddock-ipc`'s test child.

use crate::plugin::Plugin;
use crate::protocol::PLATFORM_PROTOCOL;
use crate::serve::{ELEVATED_ROLE, PluginHandler};
use paddock_ipc::IpcError;
use std::sync::Arc;

/// A plugin process' `main`: `serve(Arc::new(MyPlugin))`.
pub fn serve(plugin: Arc<dyn Plugin>) -> Result<(), IpcError> {
    paddock_ipc::child::serve(
        "the app",
        PLATFORM_PROTOCOL,
        move |boot| Arc::new(PluginHandler::new(plugin, boot.role == ELEVATED_ROLE)),
        |_, _| {},
    )
}
