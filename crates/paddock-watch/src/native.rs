//! The real-folder adapter: one `notify` watcher per folder. Native glue, reviewed by hand and left out of coverage;
//! the logic around it is tested with a fake and one real-folder test, and [`NotifySource::change_from`] is tested
//! directly.

use crate::{Change, ChangeSource, WatchError};
use notify::{RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::Sender;

/// Watches a folder with the OS's file-change notifications.
#[derive(Debug, Default, Clone, Copy)]
pub struct NotifySource;

impl NotifySource {
    /// What one `notify` result means for the watch. An event flagged `Rescan` (notify 9 sets it for a Linux inotify
    /// `IN_Q_OVERFLOW`, an FSEvents `MustScanSubDirs` notice and a Windows `ReadDirectoryChangesW` overflow or
    /// `ERROR_NOTIFY_ENUM_DIR`) asks for a [`Change::Rescan`]; an error (the watch may have stopped, or ended) is a
    /// [`Change::Failed`], after which the watch is started again; every other event is a [`Change::Changed`].
    pub fn change_from(res: &notify::Result<notify::Event>) -> Change {
        match res {
            Ok(event) if event.need_rescan() => Change::Rescan(format!(
                "the OS dropped change events: {}",
                event.info().unwrap_or("its event queue overflowed")
            )),
            Ok(_) => Change::Changed,
            Err(e) => Change::Failed(format!("the watch reported an error: {e}")),
        }
    }
}

impl ChangeSource for NotifySource {
    fn watch(
        &self,
        dir: &Path,
        recursive: bool,
        tx: Sender<Change>,
    ) -> Result<Box<dyn Send>, WatchError> {
        let fail = |e: notify::Error| WatchError(format!("could not watch the folder: {e}"));
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let _ = tx.send(Self::change_from(&res));
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

    fn boxed(&self) -> Box<dyn ChangeSource> {
        Box::new(*self)
    }
}
