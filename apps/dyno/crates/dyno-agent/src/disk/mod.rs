//! The disk map (read-only): volumes, scans as jobs and one-level queries over a size index. Nothing here changes a file.

pub mod centre;
pub mod engine;
pub mod fs;
pub mod folder;
#[cfg(test)]
pub(crate) mod testing;
pub mod volumes;
pub mod walk;

use dyno_core::protocol::{AgentError, ErrorCode, Message, Reply, Request};

/// Answers the disk map's requests (volumes, scans as jobs, one level at a time).
pub fn handle(shared: &crate::agent::Shared, request: Request) -> Result<Reply, AgentError> {
    let d = &shared.disk;
    match request {
        Request::DiskVolumes => Ok(Reply::DiskVolumes(d.volumes())),
        Request::StartDiskScan { target } => d.start(target).map(|s| Reply::DiskScan(Box::new(s))),
        Request::CancelDiskScan { scan } => d.cancel(scan).map(|()| Reply::Ok),
        Request::DiskScans => Ok(Reply::DiskScans(d.scans())),
        Request::DiskChildren {
            scan,
            node,
            offset,
            limit,
            min_fraction,
        } => d
            .children(scan, node, offset, limit, min_fraction)
            .map(|c| Reply::DiskChildren(Box::new(c))),
        Request::DiskPath { scan, node } => d.path(scan, node).map(Reply::DiskPath),
        _ => Err(AgentError::new(ErrorCode::Internal, "Not a disk map request.")),
    }
}

/// Sends scan statuses to every connected app.
pub struct BroadcastStatus(pub std::sync::Weak<crate::agent::Shared>);

impl centre::StatusSink for BroadcastStatus {
    fn status(&self, status: dyno_core::disk::scan::DiskScanStatus) {
        if let Some(shared) = self.0.upgrade() {
            shared.broadcast(&Message::DiskScan(Box::new(status)));
        }
    }
}
