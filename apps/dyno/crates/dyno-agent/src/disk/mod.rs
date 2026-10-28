//! The disk map (read-only): volumes, scans as jobs and one-level queries over a size index. Nothing here changes a file.

pub mod centre;
pub mod engine;
pub mod folder;
pub mod fs;
pub mod names;
#[cfg(test)]
pub(crate) mod testing;
pub mod volumes;
pub mod walk;

use dyno_core::protocol::{AgentError, ErrorCode, Reply, Request};

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
        Request::RetryDiskScan { scan } => d.retry(scan).map(|s| Reply::DiskScan(Box::new(s))),
        Request::OpenFullDiskAccessSettings => shared.disk.open_access_settings().map(|()| Reply::Ok),
        // The path is the scan's own, never the caller's: only a node of a kept scan can be shown.
        Request::DiskReveal { scan, node } => {
            let path = d.path_buf(scan, node)?;
            (shared.reveal)(&path)?;
            Ok(Reply::Ok)
        }
        _ => Err(AgentError::new(ErrorCode::Internal, "Not a disk map request.")),
    }
}

/// What the disk map reads from the machine besides folders: the mounts and the access rights. The real ones ask the OS;
/// tests give fixed ones, so no test depends on (or waits for) the machine it runs on.
#[derive(Clone)]
pub struct DiskPorts {
    /// The volume list.
    pub volumes: std::sync::Arc<dyn volumes::VolumeLister>,
    /// Whether Full Disk Access is missing.
    pub access: std::sync::Arc<dyn centre::AccessProbe>,
}

/// No volumes and no access rights to ask about: what an agent over a fake platform (tests, embedding) gets, so it never reads
/// the machine's mount table, runs `diskutil` or probes a share.
struct NoVolumes;

impl volumes::VolumeLister for NoVolumes {
    fn list(&self) -> Vec<volumes::ListedVolume> {
        Vec::new()
    }
}

struct NoAccessRights;

impl centre::AccessProbe for NoAccessRights {
    fn full_disk_access_missing(&self) -> bool {
        false
    }

    fn open_settings(&self) -> Result<(), dyno_core::ports::PlatformError> {
        Err(dyno_core::ports::PlatformError::Unsupported(
            "This agent has no system to ask.".into(),
        ))
    }
}

impl DiskPorts {
    /// Nothing of the machine (the default of [`Running::start`](crate::Running::start)).
    pub fn none() -> DiskPorts {
        DiskPorts {
            volumes: std::sync::Arc::new(NoVolumes),
            access: std::sync::Arc::new(NoAccessRights),
        }
    }

    /// The machine's own: only the real agent binaries use it (`Running::start_os`).
    pub fn os() -> DiskPorts {
        DiskPorts {
            volumes: std::sync::Arc::new(volumes::OsVolumes::default()),
            access: std::sync::Arc::new(centre::OsAccess),
        }
    }
}

/// Sends scan statuses to every connected app.
pub struct BroadcastStatus(pub std::sync::Weak<crate::agent::Shared>);

impl centre::StatusSink for BroadcastStatus {
    fn volumes_changed(&self, volumes: Vec<dyno_core::disk::volume::Volume>) {
        if let Some(shared) = self.0.upgrade() {
            shared.broadcast(&dyno_core::protocol::Message::DiskVolumes(volumes));
        }
    }

    fn status(&self, status: dyno_core::disk::scan::DiskScanStatus) {
        if let Some(shared) = self.0.upgrade() {
            shared.broadcast_scan(&status);
        }
    }
}
