//! The OS's word that the mounts changed: a volume came, a volume went. The adapters live in `dyno_platform::mount_events`; this
//! is the port the agent's thread runs and tests fake. Waking from sleep comes through the power centre's wake event instead.

use super::centre::DiskCentre;
use std::sync::Arc;

/// A source of mount-change notifications.
pub trait MountEvents: Send + Sync {
    /// Blocks, calling `on_event` with the kind of change ("mount") whenever the OS says the mounts changed, until `stopped`
    /// is true. A source the OS does not offer returns at once (the ticker's fifth-reading relist covers it).
    fn watch(&self, stopped: &(dyn Fn() -> bool + Sync), on_event: &(dyn Fn(&'static str) + Sync));
}

/// The machine's own source.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsMountEvents;

impl MountEvents for OsMountEvents {
    fn watch(&self, stopped: &(dyn Fn() -> bool + Sync), on_event: &(dyn Fn(&'static str) + Sync)) {
        dyno_platform::mount_events::watch(stopped, on_event);
    }
}

/// Runs `source` on the calling thread: every event lists the centre's mounts again.
pub fn run(centre: &Arc<DiskCentre>, source: &dyn MountEvents, stopped: &(dyn Fn() -> bool + Sync)) {
    source.watch(stopped, &|trigger| centre.on_system_event(trigger));
    log::info!("disk volumes: the mount event source ended");
}
