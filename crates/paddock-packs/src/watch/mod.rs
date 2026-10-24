//! Watching the packs folder, on `paddock-watch` (plan X23): the [`ChangeSource`] port and its fake and real adapters are
//! that crate's; this module only connects a [`Reloader`] to them.

use crate::{PackError, PackKind, Reloader};
use std::path::Path;
use std::sync::Arc;

#[cfg(feature = "watch")]
pub use paddock_watch::NotifySource;
pub use paddock_watch::{ChangeSource, FakeSource, WatchHandle};

/// Watches `dir` (created if missing) on a background thread: changes go to the reloader's debouncer and, once the folder
/// has been quiet, the store reloads and swaps in the new set. The first load is the caller's (`reload_now`) so the app has
/// packs before the first event. A failure to start the watch is returned; the app keeps its packs and may report it.
pub fn spawn<K: PackKind>(
    reloader: Arc<Reloader<K>>,
    source: &dyn ChangeSource,
    dir: &Path,
) -> Result<WatchHandle, PackError> {
    let store = reloader.store().clone();
    paddock_watch::spawn(
        reloader.debouncer().clone(),
        source,
        dir,
        false,
        move || {
            store.reload();
        },
    )
    .map_err(|e| PackError::Io(e.to_string()))
}
