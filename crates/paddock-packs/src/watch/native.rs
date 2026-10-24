//! The real-folder adapter: one `notify` watcher per folder. Native glue, reviewed by hand and left out of coverage;
//! the logic around it is tested with a fake and one real-folder test.

use super::ChangeSource;
use crate::PackError;
use notify::{RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::Sender;

/// Watches a folder with the OS's file-change notifications (non-recursive).
#[derive(Debug, Default, Clone, Copy)]
pub struct NotifySource;

impl ChangeSource for NotifySource {
    fn watch(&self, dir: &Path, tx: Sender<()>) -> Result<Box<dyn Send>, PackError> {
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if res.is_ok() {
                let _ = tx.send(());
            }
        })
        .map_err(|e| PackError::Io(format!("could not watch the packs folder: {e}")))?;
        watcher
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(|e| PackError::Io(format!("could not watch the packs folder: {e}")))?;
        Ok(Box::new(watcher))
    }
}
