//! Processes as the platform reports them, and the actions a user (or a rule) can take on one.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

/// A process id.
pub type Pid = u32;

/// Whose process it is, relative to the account Dyno runs as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Owner {
    /// The account Dyno runs as.
    Own,
    /// The system or a service account (SYSTEM, LOCAL SERVICE, NETWORK SERVICE; low user ids on Unix).
    System,
    /// Another person's account.
    Other,
    /// The OS did not say; listed on its own.
    Unknown,
}

/// Why a per-process value was not read. Typed, so the UI words it and the tests match on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnavailableReason {
    /// Reading it takes administrator rights Dyno does not have.
    NeedsAdmin,
    /// The OS refused this user the process (another user's process, or a protected one).
    NotPermitted,
    /// This platform or this OS version cannot give the value at all.
    NotSupported,
    /// Linux gives Dyno only the resident memory of other users' and system processes, which Dyno does not show as their memory.
    ResidentMemoryOnly,
}

/// A per-process value that may not have been read. A value that was not read is never reported as `0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Measured<T> {
    /// The value, as read.
    Value(T),
    /// Not read, and why.
    Unavailable {
        /// Why it was not read.
        reason: UnavailableReason,
    },
}

impl<T> Measured<T> {
    /// An unavailable value with this reason.
    pub fn unavailable(reason: UnavailableReason) -> Self {
        Measured::Unavailable { reason }
    }

    /// The value, when it was read.
    pub fn value(&self) -> Option<&T> {
        match self {
            Measured::Value(v) => Some(v),
            Measured::Unavailable { .. } => None,
        }
    }

    /// The reason, when it was not read.
    pub fn reason(&self) -> Option<UnavailableReason> {
        match self {
            Measured::Value(_) => None,
            Measured::Unavailable { reason } => Some(*reason),
        }
    }

    /// Whether the value was not read.
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Measured::Unavailable { .. })
    }

    /// Maps the value, keeping the reason.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Measured<U> {
        match self {
            Measured::Value(v) => Measured::Value(f(v)),
            Measured::Unavailable { reason } => Measured::Unavailable { reason },
        }
    }
}

impl<T: Copy> Measured<T> {
    /// The value by copy, when it was read.
    pub fn copied(&self) -> Option<T> {
        self.value().copied()
    }
}

/// A total over per-process values: only the readable ones are summed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Total<T> {
    /// The sum of the readable values; unavailable when none of them was readable.
    pub sum: Measured<T>,
    /// Whether some values were not readable, so the sum is lower than the real total.
    pub partial: bool,
}

impl<T: Copy + Default + std::ops::Add<Output = T>> Total<T> {
    /// Sums `items`. An empty list sums to a readable zero (nothing is missing); a list where none was readable is unavailable
    /// with the first reason; otherwise the sum is of the readable ones and `partial` says when any were not.
    pub fn of<'a>(items: impl IntoIterator<Item = &'a Measured<T>>) -> Total<T>
    where
        T: 'a,
    {
        let mut sum: Option<T> = None;
        let mut first_reason: Option<UnavailableReason> = None;
        for m in items {
            match m {
                Measured::Value(v) => sum = Some(sum.map_or(*v, |s| s + *v)),
                Measured::Unavailable { reason } => {
                    first_reason.get_or_insert(*reason);
                }
            }
        }
        match (sum, first_reason) {
            (Some(s), r) => Total {
                sum: Measured::Value(s),
                partial: r.is_some(),
            },
            (None, Some(reason)) => Total {
                sum: Measured::Unavailable { reason },
                partial: true,
            },
            (None, None) => Total {
                sum: Measured::Value(T::default()),
                partial: false,
            },
        }
    }
}

/// One process at one instant. Cheap fields only: details ([`ProcessDetails`]) are fetched on demand.
///
/// The OS may refuse some values for a process (another user's, a protected one): those are [`Measured::Unavailable`]
/// with a reason, never `0` or an empty string.
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
    /// The user it runs as.
    pub user: Measured<String>,
    /// CPU in percent of the whole machine (0–100, not per core).
    pub cpu: Measured<f32>,
    /// Resident memory (working set) in bytes.
    pub memory: Measured<u64>,
    /// Bytes read from disk per second.
    pub disk_read_bps: Measured<f64>,
    /// Bytes written to disk per second.
    pub disk_write_bps: Measured<f64>,
    /// When it started (with the pid, this identifies the process: pids are reused).
    pub started: Measured<Timestamp>,
    /// Whose process it is.
    pub owner: Owner,
    /// Run state.
    pub state: RunState,
}

impl ProcessSample {
    /// The identity a later action must match ([`ProcessRef`]).
    pub fn reference(&self) -> ProcessRef {
        ProcessRef {
            pid: self.pid,
            started: self.started.copied(),
        }
    }

    /// The user name, when it was read.
    pub fn user_name(&self) -> Option<&str> {
        self.user.value().map(String::as_str)
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
    /// Start time, when it was read (without it the pid alone identifies the process).
    pub started: Option<Timestamp>,
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
            user: Measured::Value("me".into()),
            cpu: Measured::Value(cpu),
            memory: Measured::Value(mem_mib << 20),
            disk_read_bps: Measured::Value(0.0),
            disk_write_bps: Measured::Value(0.0),
            started: Measured::Value(Timestamp(i64::from(pid) * 1_000)),
            owner: Owner::Own,
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
                started: Some(Timestamp(7_000))
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

    #[test]
    fn a_value_not_read_is_never_a_zero() {
        let none: Measured<u64> = Measured::unavailable(UnavailableReason::NeedsAdmin);
        assert!(none.is_unavailable());
        assert_eq!(none.value(), None);
        assert_eq!(none.copied(), None);
        assert_eq!(none.reason(), Some(UnavailableReason::NeedsAdmin));
        assert_eq!(none.map(|v| v + 1), Measured::unavailable(UnavailableReason::NeedsAdmin));
        let some = Measured::Value(4u64);
        assert!(!some.is_unavailable());
        assert_eq!(some.reason(), None);
        assert_eq!(some.copied(), Some(4));
        assert_eq!(some.map(|v| v * 2), Measured::Value(8));
        let mut p = proc(1, None, "a.exe", 1.0, 1);
        assert_eq!(p.user_name(), Some("me"));
        p.user = Measured::unavailable(UnavailableReason::NotPermitted);
        assert_eq!(p.user_name(), None);
    }

    #[test]
    fn totals_sum_only_what_was_read_and_say_when_partial() {
        let na = |r| Measured::<u64>::unavailable(r);
        let all = [Measured::Value(2u64), Measured::Value(3)];
        assert_eq!(
            Total::of(&all),
            Total {
                sum: Measured::Value(5),
                partial: false
            }
        );
        let some = [Measured::Value(2u64), na(UnavailableReason::NeedsAdmin), Measured::Value(3)];
        assert_eq!(
            Total::of(&some),
            Total {
                sum: Measured::Value(5),
                partial: true
            }
        );
        let none = [na(UnavailableReason::NotPermitted), na(UnavailableReason::NeedsAdmin)];
        assert_eq!(
            Total::of(&none),
            Total {
                sum: Measured::unavailable(UnavailableReason::NotPermitted),
                partial: true
            },
            "nothing readable is unavailable with the first reason, not a zero"
        );
        let empty: [Measured<u64>; 0] = [];
        assert_eq!(
            Total::of(&empty),
            Total {
                sum: Measured::Value(0),
                partial: false
            }
        );
    }

    /// The wire shape shared with the TypeScript tests: `contract/measured-wire.json` is read by both sides.
    #[test]
    fn the_shared_fixture_round_trips_and_names_every_reason() {
        use crate::others::OtherUsers;
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../../../contract/measured-wire.json")).unwrap();
        let process: ProcessSample = serde_json::from_value(fixture["process"].clone()).unwrap();
        assert_eq!(process.cpu.reason(), Some(UnavailableReason::NeedsAdmin));
        assert_eq!(process.disk_write_bps.reason(), Some(UnavailableReason::NotPermitted));
        assert_eq!((process.memory, process.owner), (Measured::Value(5), Owner::Other));
        assert_eq!(
            serde_json::to_value(&process).unwrap(),
            fixture["process"],
            "Rust writes exactly what the fixture says"
        );
        let other: OtherUsers = serde_json::from_value(fixture["otherUsers"].clone()).unwrap();
        assert_eq!((other.count, other.partial), (3, true));
        assert_eq!(serde_json::to_value(&other).unwrap(), fixture["otherUsers"]);
        let reasons: Vec<UnavailableReason> = serde_json::from_value(fixture["reasons"].clone()).unwrap();
        assert_eq!(
            reasons,
            [
                UnavailableReason::NeedsAdmin,
                UnavailableReason::NotPermitted,
                UnavailableReason::NotSupported,
                UnavailableReason::ResidentMemoryOnly
            ],
            "a new reason must be added to the fixture and to the TypeScript union"
        );
    }

    #[test]
    fn the_wire_shape_of_measured_values_is_stable() {
        use serde_json::json;
        assert_eq!(serde_json::to_value(Measured::Value(3u64)).unwrap(), json!({"value": 3}));
        for (reason, name) in [
            (UnavailableReason::NeedsAdmin, "needsAdmin"),
            (UnavailableReason::NotPermitted, "notPermitted"),
            (UnavailableReason::NotSupported, "notSupported"),
            (UnavailableReason::ResidentMemoryOnly, "residentMemoryOnly"),
        ] {
            let m = Measured::<u64>::unavailable(reason);
            let v = serde_json::to_value(m).unwrap();
            assert_eq!(v, json!({"unavailable": {"reason": name}}));
            assert_eq!(serde_json::from_value::<Measured<u64>>(v).unwrap(), m);
        }
        let mut p = proc(2, Some(1), "b.exe", 1.5, 1);
        p.cpu = Measured::unavailable(UnavailableReason::NeedsAdmin);
        let v = serde_json::to_value(&p).unwrap();
        for key in ["user", "cpu", "memory", "diskReadBps", "diskWriteBps", "started"] {
            let o = v[key].as_object().unwrap_or_else(|| panic!("{key} is a tagged object"));
            assert!(o.contains_key("value") || o.contains_key("unavailable"), "{key}");
        }
        assert_eq!(v["cpu"], json!({"unavailable": {"reason": "needsAdmin"}}));
        assert_eq!(serde_json::from_value::<ProcessSample>(v).unwrap(), p);
    }
}
