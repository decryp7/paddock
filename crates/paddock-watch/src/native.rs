//! The real-folder adapter: one `notify` watcher per folder. Native glue, reviewed by hand and left out of coverage;
//! the logic around it is tested with a fake and one real-folder test.

use crate::{ChangeSource, WatchError};
use notify::{RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::Sender;

/// Watches a folder with the OS's file-change notifications.
#[derive(Debug, Default, Clone, Copy)]
pub struct NotifySource;

impl ChangeSource for NotifySource {
    fn watch(
        &self,
        dir: &Path,
        recursive: bool,
        tx: Sender<()>,
    ) -> Result<Box<dyn Send>, WatchError> {
        let fail = |e: notify::Error| WatchError(format!("could not watch the folder: {e}"));
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if res.is_ok() {
                let _ = tx.send(());
            }
        })
        .map_err(fail)?;
        let mode = if recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        watcher.watch(dir, mode).map_err(fail)?;
        Ok(Box::new(watcher))
    }
}
