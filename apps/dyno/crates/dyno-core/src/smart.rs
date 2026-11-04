//! What the helper's `Smart` request answers (ADR 0027 section 5): per-disk health, and the traits the helper's handler plugs real
//! readers into. There is no field for a WWN. The serial number travels only inside `wear_id` (the stable id the agent's history is
//! keyed by, "model serial"); amended 2026-11-04 so the helper's reading is as complete as the agent's own and the history
//! continues across both paths.

use crate::helper_requests::{SmartQuery, ToolKind};
use crate::helper_wire::ToolStatus;
use crate::ports::PlatformError;
use serde::{Deserialize, Serialize};

/// The longest device name or reason text kept in a reply.
pub const MAX_SMART_TEXT: usize = 128;

/// One disk's health.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartDisk {
    /// The device as the helper's own scan named it (for example `/dev/sda`).
    pub device: String,
    /// Model name.
    pub model: String,
    /// Capacity in bytes.
    pub capacity: Option<u64>,
    /// Current temperature, degrees Celsius.
    pub temperature_c: Option<i32>,
    /// "Percentage used" of the rated endurance.
    pub percent_used: Option<u16>,
    /// Media and data integrity errors (NVMe).
    pub media_errors: Option<u64>,
    /// Hours powered on.
    pub power_on_hours: Option<u64>,
    /// Unsafe shutdowns (NVMe).
    pub unsafe_shutdowns: Option<u64>,
    /// The drive's own overall verdict, when it gives one.
    pub passed: Option<bool>,
    /// The id the wear history of this disk is kept under (`DiskWear::id`: the model and the serial, else the model); `None`
    /// for a disk with no SSD wear reading (a hard disk). Produced by the same parser as the agent's own reader.
    pub wear_id: Option<String>,
    /// Total bytes written over the disk's life, when reported.
    pub bytes_written: Option<u64>,
    /// The manufacturer's rated endurance in bytes written, when known.
    pub rated_bytes: Option<u64>,
}

impl SmartDisk {
    /// The wear reading of this disk, the same value the agent's own reader makes from the same smartctl output.
    pub fn to_wear(&self) -> Option<crate::hardware::DiskWear> {
        Some(crate::hardware::DiskWear {
            id: self.wear_id.clone()?,
            name: self.model.clone(),
            capacity: self.capacity,
            percent_used: self.percent_used.map(f32::from),
            bytes_written: self.bytes_written,
            rated_bytes: self.rated_bytes,
            power_on_hours: self.power_on_hours,
        })
    }
}

/// A disk the helper found but could not read, and why (never user content).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartSkip {
    /// The device.
    pub device: String,
    /// Why, with the next step.
    pub reason: String,
}

/// What `Smart` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartReply {
    /// The disks that were read.
    pub disks: Vec<SmartDisk>,
    /// The disks that were found but not read.
    pub skipped: Vec<SmartSkip>,
    /// Disks the scan found beyond the query's cap (not visited).
    pub not_visited: u32,
}

/// The helper's `Smart` handler plugs a reader in here.
pub trait SmartReader: Send + Sync {
    /// Scans, then reads each disk of the scan (never a device the caller named).
    fn read(&self, query: SmartQuery) -> Result<SmartReply, PlatformError>;
}

/// The helper's `InstallTool` handler plugs the installer in here. The caller's bytes are checked against the helper's own
/// pinned size and SHA-256.
pub trait ToolInstaller: Send + Sync {
    /// Installs `tool` from `bytes` and says what is now installed.
    fn install(&self, tool: ToolKind, bytes: &[u8]) -> Result<ToolStatus, PlatformError>;
    /// The tools that are installed now, found by looking at the tools folder (empty when none or on a system without them).
    fn installed(&self) -> Vec<ToolStatus>;
}
