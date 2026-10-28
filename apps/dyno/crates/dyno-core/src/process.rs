//! Processes as the platform reports them, and the actions a user (or a rule) can take on one.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

/// A process id.
pub type Pid = u32;

/// One process at one instant. Cheap fields only: details ([`ProcessDetails`]) are fetched on demand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessSample {
    /// Process id.
    pub pid: Pid,
    /// Parent process id, when known.
    pub parent: Option<Pid>,
    /// Executable name, e.g. "chrome.exe".
    pub name: String,
    /// Full path of the executable, when readable.
    pub exe: Option<String>,
    /// The user it runs as, when readable.
    pub user: Option<String>,
    /// CPU in percent of the whole machine (0–100, not per core).
    pub cpu: f32,
    /// Resident memory (working set) in bytes.
    pub memory: u64,
    /// Bytes read from disk per second.
    pub disk_read_bps: f64,
    /// Bytes written to disk per second.
    pub disk_write_bps: f64,
    /// When it started (with the pid, this identifies the process: pids are reused).
    pub started: Timestamp,
    /// Run state.
    pub state: RunState,
}

impl ProcessSample {
    /// The identity a later action must match ([`ProcessRef`]).
    pub fn reference(&self) -> ProcessRef {
        ProcessRef {
            pid: self.pid,
            started: self.started,
        }
    }
}

/// A process's run state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunState {
    /// Running or runnable.
    Running,
    /// Sleeping / waiting.
    Sleeping,
    /// Suspended (by the user, Dyno or the OS).
    Suspended,
    /// Anything else (zombie, unknown).
    Other,
}

/// Identifies one process across time: the pid plus its start time, so an action never hits a reused pid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRef {
    /// Process id.
    pub pid: Pid,
    /// Start time.
    pub started: Timestamp,
}

/// Scheduling priority, in the platform-neutral classes both OSes can express.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Priority {
    /// Windows IDLE_PRIORITY_CLASS / nice 19.
    Idle,
    /// BELOW_NORMAL / nice 10.
    BelowNormal,
    /// NORMAL / nice 0.
    Normal,
    /// ABOVE_NORMAL / nice -5 (macOS needs root for negative nice: refused there).
    AboveNormal,
    /// HIGH / nice -10 (same).
    High,
}

impl Priority {
    /// The Unix nice value for this class.
    pub fn nice(self) -> i32 {
        match self {
            Priority::Idle => 19,
            Priority::BelowNormal => 10,
            Priority::Normal => 0,
            Priority::AboveNormal => -5,
            Priority::High => -10,
        }
    }

    /// The class for a Unix nice value.
    pub fn from_nice(nice: i32) -> Priority {
        match nice {
            n if n >= 15 => Priority::Idle,
            n if n >= 5 => Priority::BelowNormal,
            n if n > -5 => Priority::Normal,
            n if n > -10 => Priority::AboveNormal,
            _ => Priority::High,
        }
    }
}

/// The user-facing name of a priority class.
pub fn priority_name(p: Priority) -> &'static str {
    match p {
        Priority::Idle => "Low",
        Priority::BelowNormal => "Below normal",
        Priority::Normal => "Normal",
        Priority::AboveNormal => "Above normal",
        Priority::High => "High",
    }
}

/// Everything known about one process, fetched when the user opens it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessDetails {
    /// The process.
    pub process: ProcessSample,
    /// Command line, split into arguments.
    pub command_line: Vec<String>,
    /// Working directory, when readable.
    pub cwd: Option<String>,
    /// Current priority class.
    pub priority: Option<Priority>,
    /// Current affinity mask (Windows), when readable.
    pub affinity: Option<u64>,
    /// Whether efficiency mode is on, when readable.
    pub efficiency_mode: Option<bool>,
    /// Thread count, when readable.
    pub threads: Option<u32>,
    /// Handle (Windows) or file-descriptor count, when readable.
    pub handles: Option<u32>,
    /// Parent chain from the direct parent up to the root: name and pid.
    pub parents: Vec<ParentLink>,
    /// Who signed the executable.
    pub signature: SignatureInfo,
    /// Open files (bounded list), or why they can't be listed.
    pub open_files: ListOrReason<String>,
    /// Network connections (bounded list), or why they can't be listed.
    pub connections: ListOrReason<Connection>,
    /// Loaded modules / libraries (bounded list), or why they can't be listed.
    pub modules: ListOrReason<String>,
    /// How it was started at login, when it matches a startup entry.
    pub startup_source: Option<String>,
}

/// A parent in the chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParentLink {
    /// Process id.
    pub pid: Pid,
    /// Name.
    pub name: String,
}

/// A list the platform may refuse (access denied, unsupported), capped at [`MAX_DETAIL_ITEMS`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ListOrReason<T> {
    /// The items; `truncated` when there were more than the cap.
    Items {
        /// The items.
        items: Vec<T>,
        /// More existed than were returned.
        truncated: bool,
    },
    /// Why not, e.g. "Access denied: the process runs as another user".
    Unavailable(String),
}

/// The most items a detail list carries.
pub const MAX_DETAIL_ITEMS: usize = 500;

impl<T> ListOrReason<T> {
    /// Caps `items` at [`MAX_DETAIL_ITEMS`].
    pub fn capped(mut items: Vec<T>) -> Self {
        let truncated = items.len() > MAX_DETAIL_ITEMS;
        items.truncate(MAX_DETAIL_ITEMS);
        ListOrReason::Items { items, truncated }
    }
}

/// A network connection owned by a process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    /// "TCP" or "UDP" (with "6" for IPv6).
    pub protocol: String,
    /// Local address:port.
    pub local: String,
    /// Remote address:port (empty for listening / UDP).
    pub remote: String,
    /// TCP state, e.g. "ESTABLISHED", "LISTEN".
    pub state: String,
}

/// The executable's code signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SignatureInfo {
    /// Signed and the signature verifies; the signer's name.
    Signed(String),
    /// Not signed.
    Unsigned,
    /// Signed but the signature does not verify.
    Invalid(String),
    /// Could not be checked, and why.
    Unknown(String),
}

/// A process that holds a file open (the answer to "what is locking this file").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Locker {
    /// The process.
    pub process: ProcessRef,
    /// Its name (application name when the OS gives one).
    pub name: String,
    /// What kind of program it is (e.g. "Service", "Application", "Explorer"), when known.
    pub kind: Option<String>,
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    /// A process with the given pid, parent, name, CPU and memory (MiB), started at `pid` seconds.
    pub fn proc(pid: Pid, parent: Option<Pid>, name: &str, cpu: f32, mem_mib: u64) -> ProcessSample {
        ProcessSample {
            pid,
            parent,
            name: name.into(),
            exe: Some(format!("C:\\Apps\\{name}")),
            user: Some("me".into()),
            cpu,
            memory: mem_mib << 20,
            disk_read_bps: 0.0,
            disk_write_bps: 0.0,
            started: Timestamp(i64::from(pid) * 1_000),
            state: RunState::Running,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::proc;
    use super::*;

    #[test]
    fn priorities_map_to_nice_and_back() {
        for p in [
            Priority::Idle,
            Priority::BelowNormal,
            Priority::Normal,
            Priority::AboveNormal,
            Priority::High,
        ] {
            assert_eq!(Priority::from_nice(p.nice()), p);
        }
        assert_eq!(Priority::from_nice(-20), Priority::High);
    }

    #[test]
    fn references_and_capped_lists() {
        let p = proc(7, None, "a.exe", 1.0, 1);
        assert_eq!(
            p.reference(),
            ProcessRef {
                pid: 7,
                started: Timestamp(7_000)
            }
        );
        match ListOrReason::capped((0..MAX_DETAIL_ITEMS + 3).collect::<Vec<_>>()) {
            ListOrReason::Items { items, truncated } => {
                assert_eq!(items.len(), MAX_DETAIL_ITEMS);
                assert!(truncated);
            }
            ListOrReason::Unavailable(_) => unreachable!(),
        }
        assert_eq!(
            ListOrReason::capped(vec![1]),
            ListOrReason::Items {
                items: vec![1],
                truncated: false
            }
        );
    }
}
