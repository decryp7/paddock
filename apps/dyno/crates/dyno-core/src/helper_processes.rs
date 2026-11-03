//! The helper's `Processes` answer (ADR 0027 section 5, ADR 0022 section 5): a page of the whole machine's process list, scoped to
//! the caller and redacted. The fields are a closed allow-list: pid, start time, parent, name, program path, owner kind and account
//! name, run state, threads, cumulative CPU time, memory, cumulative disk bytes. **Never** a command line, environment, working
//! directory, open file, module, window title, memory content or network endpoint: there is no field for them.
//!
//! The scoping, redaction and paging are pure ([`build_page`]); the OS reads one [`RawProcess`] per process.

use crate::helper::HelperHost;

use crate::helper_policy::{OthersPolicy, Visibility};
use crate::helper_requests::ProcessQuery;
use crate::helper_wire::HelperUser;
use crate::ports::PlatformError;
use serde::{Deserialize, Serialize};

/// The most processes one sweep considers.
pub const MAX_SWEEP: usize = 16_384;
/// The longest process name.
pub const MAX_NAME: usize = 256;
/// The longest program path.
pub const MAX_PATH: usize = 1_024;

/// Who owns a process, as the caller sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OwnerKind {
    /// The asking account.
    Caller,
    /// The system account (root, SYSTEM).
    System,
    /// A service account, by name (`LOCAL SERVICE`, `_mdnsresponder`, `systemd-resolve`).
    Service(String),
    /// Another person's account (named only to an administrator, or where the policy says detail).
    OtherUser,
}

/// A run state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProcState {
    /// Running.
    Running,
    /// Waiting.
    Sleeping,
    /// Stopped or suspended.
    Stopped,
    /// Anything else.
    Other,
}

/// One process in an answer: only allow-listed fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessEntry {
    /// Process id.
    pub pid: u32,
    /// Start time, seconds since 1970 (with the pid it identifies the process across time).
    pub started: Option<i64>,
    /// Parent process id.
    pub parent: Option<u32>,
    /// Name, at most [`MAX_NAME`] characters.
    pub name: String,
    /// Program path, at most [`MAX_PATH`] characters, redacted when it lies in another account's folder.
    pub path: Option<String>,
    /// Whose it is.
    pub owner: OwnerKind,
    /// The account's name for the caller, the system and service accounts, and for other users only where they are shown.
    pub account: Option<String>,
    /// Run state.
    pub state: ProcState,
    /// Threads.
    pub threads: Option<u32>,
    /// Cumulative CPU time, milliseconds.
    pub cpu_time_ms: Option<u64>,
    /// Memory in the family's real-memory measure where the helper can read it (Linux PSS, macOS footprint, Windows private working
    /// set), else resident memory.
    pub memory_bytes: Option<u64>,
    /// Cumulative bytes read from disk.
    pub disk_read_bytes: Option<u64>,
    /// Cumulative bytes written to disk.
    pub disk_written_bytes: Option<u64>,
}

/// What other accounts' processes add up to, for a caller who may not see them one by one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OthersTotal {
    /// How many processes belong to other accounts.
    pub processes: u32,
    /// Their memory together.
    pub memory_bytes: u64,
    /// Their CPU time together, milliseconds.
    pub cpu_time_ms: u64,
}

/// One page of the process list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessPage {
    /// When the sweep was read, seconds since 1970.
    pub taken_at: i64,
    /// The processes of this page, in pid order.
    pub items: Vec<ProcessEntry>,
    /// The last pid of this page when more follow (pass it as `after`), else none.
    pub next: Option<u32>,
    /// Other accounts' total, on the first page only, when the caller sees them as a total.
    pub others: Option<OthersTotal>,
    /// The sweep held more than [`MAX_SWEEP`] processes and was cut.
    pub truncated: bool,
}

/// Whose a raw process is, as the OS says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawOwner {
    /// A person's or service's account, by id (uid or SID) and name.
    Account {
        /// uid or SID.
        id: HelperUser,
        /// The account's name, when known.
        name: Option<String>,
    },
    /// The system account.
    System,
    /// A service account that is not a person's (by name, and its id so the account that is asking still sees its own).
    Service(String, Option<HelperUser>),
}

/// What the OS read for one process: the allow-listed fields and the owner, nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawProcess {
    /// Process id.
    pub pid: u32,
    /// Start time, seconds since 1970.
    pub started: Option<i64>,
    /// Parent.
    pub parent: Option<u32>,
    /// Name.
    pub name: String,
    /// Program path.
    pub path: Option<String>,
    /// Owner.
    pub owner: RawOwner,
    /// State.
    pub state: ProcState,
    /// Threads.
    pub threads: Option<u32>,
    /// CPU time, ms.
    pub cpu_time_ms: Option<u64>,
    /// Memory.
    pub memory_bytes: Option<u64>,
    /// Disk read.
    pub disk_read_bytes: Option<u64>,
    /// Disk written.
    pub disk_written_bytes: Option<u64>,
}

/// One reading of the machine: its processes and every account's profile folder (for redaction).
#[derive(Debug, Clone, Default)]
pub struct Sweep {
    /// The processes.
    pub processes: Vec<RawProcess>,
    /// Each account's home or profile folder, from the OS's own account list (never a guessed pattern).
    pub homes: Vec<(HelperUser, String)>,
    /// When it was read, seconds since 1970.
    pub taken_at: i64,
}

/// The helper's reader of the machine's processes.
pub trait ProcessSweeper: Send + Sync {
    /// Reads every process once.
    fn sweep(&self) -> Result<Sweep, PlatformError>;
}

/// The first person's uid when the machine does not say otherwise.
pub fn default_first_person(host: HelperHost) -> u32 {
    if host == HelperHost::Macos {
        500
    } else {
        1_000
    }
}

/// What a system without a process reader says (an unsupported build).
pub struct NoProcesses;

impl ProcessSweeper for NoProcesses {
    fn sweep(&self) -> Result<Sweep, PlatformError> {
        Err(PlatformError::Unsupported(
            "This system cannot list processes through the helper.".into(),
        ))
    }
}

/// How an account counts: the system account, a service account (named) or a person's account. Decided from the id and the name
/// alone, so it is the same for every caller: Unix uid 0 is the system; a uid below `first_person` (the machine's own first
/// person id: 500 on macOS, `UID_MIN` of `/etc/login.defs` on Linux, default 1000; see [`default_first_person`]) is a service; Windows `S-1-5-18` is the system, other well-known `S-1-5-` ids with fewer than four sub-authorities (and
/// `S-1-5-80-` services, `S-1-5-90-`, `S-1-5-96-` system components) are services; `S-1-5-21-` is a person.
pub fn classify_owner(id: &HelperUser, name: Option<&str>, first_person: u32) -> RawOwner {
    let service = |fallback: &str| {
        RawOwner::Service(
            name.filter(|n| !n.is_empty()).unwrap_or(fallback).chars().take(64).collect(),
            Some(id.clone()),
        )
    };
    match id {
        HelperUser::Uid(0) => RawOwner::System,
        HelperUser::Uid(uid) => {
            if *uid < first_person || *uid == 65_534 {
                service("system service")
            } else {
                RawOwner::Account {
                    id: id.clone(),
                    name: name.map(str::to_string),
                }
            }
        }
        HelperUser::Sid(sid) => match sid.as_str() {
            "S-1-5-18" => RawOwner::System,
            "S-1-5-19" => RawOwner::Service("LOCAL SERVICE".into(), Some(id.clone())),
            "S-1-5-20" => RawOwner::Service("NETWORK SERVICE".into(), Some(id.clone())),
            s if s.starts_with("S-1-5-80-") || s.starts_with("S-1-5-90-") || s.starts_with("S-1-5-96-") => service("system service"),
            _ => RawOwner::Account {
                id: id.clone(),
                name: name.map(str::to_string),
            },
        },
    }
}

fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

fn lower(s: &str) -> String {
    s.to_lowercase().replace('\\', "/")
}

/// `path` inside `home`, compared case-insensitively with either slash (Windows paths are case-insensitive; macOS volumes usually).
fn within(path: &str, home: &str) -> bool {
    let (p, h) = (lower(path), lower(home));
    let h = h.trim_end_matches('/');
    !h.is_empty() && (p == h || p.starts_with(&format!("{h}/")))
}

/// A path in another account's folder shows only its file name.
fn redact(path: &str, caller: &HelperUser, homes: &[(HelperUser, String)]) -> String {
    let other = homes.iter().any(|(who, home)| who != caller && within(path, home));
    let own = homes.iter().any(|(who, home)| who == caller && within(path, home));
    if other && !own {
        let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
        format!("{file} (in another account's folder)")
    } else {
        path.to_string()
    }
}

/// How one raw process looks to `caller`: its entry, or how it counts in the others' total (or not at all).
enum Seen {
    Entry(Box<ProcessEntry>),
    Total,
    Hidden,
}

fn see(p: &RawProcess, caller: &HelperUser, scope: Visibility, homes: &[(HelperUser, String)]) -> Seen {
    let (owner, account, other_account) = match &p.owner {
        RawOwner::System => (OwnerKind::System, Some("SYSTEM".to_string()), false),
        RawOwner::Service(name, Some(id)) if id == caller => (OwnerKind::Caller, Some(name.clone()), false),
        RawOwner::Service(name, _) => (OwnerKind::Service(name.clone()), Some(name.clone()), false),
        RawOwner::Account { id, name } if id == caller => (OwnerKind::Caller, name.clone(), false),
        RawOwner::Account { name, .. } => (OwnerKind::OtherUser, name.clone(), true),
    };
    let account = if other_account {
        match scope {
            Visibility::Full | Visibility::Own(OthersPolicy::Detail) => account,
            Visibility::Own(OthersPolicy::Total) => return Seen::Total,
            Visibility::Own(OthersPolicy::Hidden) => return Seen::Hidden,
        }
    } else {
        account
    };
    Seen::Entry(Box::new(ProcessEntry {
        pid: p.pid,
        started: p.started,
        parent: p.parent,
        name: clip(&p.name, MAX_NAME),
        path: p.path.as_deref().map(|path| clip(&redact(path, caller, homes), MAX_PATH)),
        owner,
        account,
        state: p.state,
        threads: p.threads,
        cpu_time_ms: p.cpu_time_ms,
        memory_bytes: p.memory_bytes,
        disk_read_bytes: p.disk_read_bytes,
        disk_written_bytes: p.disk_written_bytes,
    }))
}

/// One page for `caller` out of `sweep`: scoped by `scope` (an administrator and the owner see every account in detail; others
/// see their own and the system's, and the rest as the policy says), redacted, in pid order, `query.limit` entries after
/// `query.after`. The caller's own processes are never redacted from the caller.
pub fn build_page(sweep: &Sweep, caller: &HelperUser, scope: Visibility, query: &ProcessQuery) -> ProcessPage {
    let truncated = sweep.processes.len() > MAX_SWEEP;
    let mut all: Vec<&RawProcess> = sweep.processes.iter().take(MAX_SWEEP).collect();
    all.sort_by_key(|p| p.pid);
    let mut total = OthersTotal::default();
    let mut seen_total = false;
    let mut entries = Vec::new();
    for p in all {
        match see(p, caller, scope, &sweep.homes) {
            Seen::Entry(e) => entries.push(*e),
            Seen::Total => {
                seen_total = true;
                total.processes += 1;
                total.memory_bytes = total.memory_bytes.saturating_add(p.memory_bytes.unwrap_or(0));
                total.cpu_time_ms = total.cpu_time_ms.saturating_add(p.cpu_time_ms.unwrap_or(0));
            }
            Seen::Hidden => {}
        }
    }
    let limit = usize::from(query.limit.clamp(1, crate::helper_requests::MAX_PROCESS_PAGE));
    let start = query.after.map_or(0, |after| entries.partition_point(|e| e.pid <= after));
    let rest = &entries[start.min(entries.len())..];
    let items: Vec<ProcessEntry> = rest.iter().take(limit).cloned().collect();
    let next = (rest.len() > limit).then(|| items.last().map(|e| e.pid)).flatten();
    ProcessPage {
        taken_at: sweep.taken_at,
        items,
        next,
        others: (query.after.is_none() && seen_total).then_some(total),
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_requests::RequestClass;

    fn raw(pid: u32, owner: RawOwner, path: Option<&str>) -> RawProcess {
        RawProcess {
            pid,
            started: Some(1_700_000_000 + i64::from(pid)),
            parent: Some(1),
            name: format!("proc{pid}"),
            path: path.map(str::to_string),
            owner,
            state: ProcState::Running,
            threads: Some(2),
            cpu_time_ms: Some(10),
            memory_bytes: Some(1_000),
            disk_read_bytes: Some(1),
            disk_written_bytes: Some(2),
        }
    }

    fn user(uid: u32, name: &str) -> RawOwner {
        RawOwner::Account {
            id: HelperUser::Uid(uid),
            name: Some(name.into()),
        }
    }

    fn sweep() -> Sweep {
        Sweep {
            taken_at: 1_800_000_000,
            homes: vec![
                (HelperUser::Uid(501), "/Users/ana".into()),
                (HelperUser::Uid(502), "/Users/ben".into()),
            ],
            processes: vec![
                raw(1, RawOwner::System, Some("/sbin/launchd")),
                raw(20, user(501, "ana"), Some("/Users/ana/bin/tool")),
                raw(30, user(502, "ben"), Some("/Users/ben/Secret Project/app")),
                raw(
                    40,
                    RawOwner::Service("_mdnsresponder".into(), None),
                    Some("/usr/sbin/mDNSResponder"),
                ),
                raw(50, user(502, "ben"), Some("/Applications/Shared.app/x")),
            ],
        }
    }

    fn query(after: Option<u32>, limit: u16) -> ProcessQuery {
        ProcessQuery {
            after,
            limit,
            class: RequestClass::User,
        }
    }

    #[test]
    fn an_administrator_sees_every_account_in_detail_but_other_peoples_folders_are_redacted() {
        let page = build_page(&sweep(), &HelperUser::Uid(0), Visibility::Full, &query(None, 100));
        assert_eq!(page.items.iter().map(|e| e.pid).collect::<Vec<_>>(), [1, 20, 30, 40, 50]);
        assert!(page.others.is_none());
        let ben = page.items.iter().find(|e| e.pid == 30).unwrap();
        assert_eq!((&ben.owner, ben.account.as_deref()), (&OwnerKind::OtherUser, Some("ben")));
        assert_eq!(ben.path.as_deref(), Some("app (in another account's folder)"));
        assert_eq!(
            page.items.iter().find(|e| e.pid == 50).unwrap().path.as_deref(),
            Some("/Applications/Shared.app/x")
        );
    }

    #[test]
    fn another_account_gets_its_own_and_the_systems_in_detail_and_the_rest_as_one_total() {
        let page = build_page(
            &sweep(),
            &HelperUser::Uid(501),
            Visibility::Own(OthersPolicy::Total),
            &query(None, 100),
        );
        assert_eq!(
            page.items.iter().map(|e| (e.pid, e.owner.clone())).collect::<Vec<_>>(),
            [
                (1, OwnerKind::System),
                (20, OwnerKind::Caller),
                (40, OwnerKind::Service("_mdnsresponder".into())),
            ]
        );
        assert_eq!(
            page.others,
            Some(OthersTotal {
                processes: 2,
                memory_bytes: 2_000,
                cpu_time_ms: 20
            })
        );
        assert_eq!(
            page.items[1].path.as_deref(),
            Some("/Users/ana/bin/tool"),
            "its own folder is not redacted from itself"
        );
        let detail = build_page(
            &sweep(),
            &HelperUser::Uid(501),
            Visibility::Own(OthersPolicy::Detail),
            &query(None, 100),
        );
        assert_eq!(detail.items.len(), 5);
        assert!(detail.others.is_none());
        let hidden = build_page(
            &sweep(),
            &HelperUser::Uid(501),
            Visibility::Own(OthersPolicy::Hidden),
            &query(None, 100),
        );
        assert_eq!((hidden.items.len(), hidden.others), (3, None));
    }

    #[test]
    fn pages_follow_the_cursor_cover_the_list_once_and_carry_the_total_on_the_first_page_only() {
        let s = sweep();
        let who = HelperUser::Uid(501);
        let scope = Visibility::Own(OthersPolicy::Total);
        let first = build_page(&s, &who, scope, &query(None, 2));
        assert_eq!(
            (first.items.iter().map(|e| e.pid).collect::<Vec<_>>(), first.next),
            (vec![1, 20], Some(20))
        );
        assert!(first.others.is_some());
        let second = build_page(&s, &who, scope, &query(first.next, 2));
        assert_eq!(
            (second.items.iter().map(|e| e.pid).collect::<Vec<_>>(), second.next, second.others),
            (vec![40], None, None)
        );
        let past = build_page(&s, &who, scope, &query(Some(9_999), 2));
        assert!(past.items.is_empty() && past.next.is_none());
    }

    #[test]
    fn windows_paths_redact_case_insensitively_and_a_folder_prefix_is_not_a_match() {
        let homes = vec![
            (HelperUser::Sid("S-1-5-21-1".into()), r"C:\Users\Ana".into()),
            (HelperUser::Sid("S-1-5-21-2".into()), r"C:\Users\Ben".into()),
        ];
        let me = HelperUser::Sid("S-1-5-21-1".into());
        assert_eq!(
            redact(r"c:\users\ben\Downloads\x.exe", &me, &homes),
            "x.exe (in another account's folder)"
        );
        assert_eq!(
            redact(r"C:\Users\Benjamin\x.exe", &me, &homes),
            r"C:\Users\Benjamin\x.exe",
            "Ben's folder is not Benjamin's"
        );
        assert_eq!(redact(r"C:\Users\Ana\x.exe", &me, &homes), r"C:\Users\Ana\x.exe");
        assert_eq!(redact(r"C:\Program Files\x.exe", &me, &homes), r"C:\Program Files\x.exe");
    }

    #[test]
    fn a_caller_below_the_first_person_id_still_sees_its_own_processes_as_its_own() {
        let owner = classify_owner(&HelperUser::Uid(501), Some("ana"), 1_000);
        let sweep = Sweep {
            processes: vec![raw(7, owner, None)],
            homes: vec![],
            taken_at: 1,
        };
        let page = build_page(
            &sweep,
            &HelperUser::Uid(501),
            Visibility::Own(OthersPolicy::Total),
            &query(None, 10),
        );
        assert_eq!(page.items[0].owner, OwnerKind::Caller);
        let other = build_page(
            &sweep,
            &HelperUser::Uid(502),
            Visibility::Own(OthersPolicy::Total),
            &query(None, 10),
        );
        assert_eq!(other.items[0].owner, OwnerKind::Service("ana".into()));
    }

    #[test]
    fn owners_are_told_apart_by_id_and_first_person() {
        use HelperHost::{Linux, Macos};
        let first = default_first_person;
        let uid = HelperUser::Uid;
        assert_eq!(classify_owner(&uid(0), Some("root"), first(Linux)), RawOwner::System);
        assert_eq!(
            classify_owner(&uid(241), Some("_mdnsresponder"), first(Macos)),
            RawOwner::Service("_mdnsresponder".into(), Some(uid(241)))
        );
        assert!(matches!(
            classify_owner(&uid(501), Some("ana"), first(Macos)),
            RawOwner::Account { .. }
        ));
        assert!(
            matches!(classify_owner(&uid(501), Some("ana"), first(Linux)), RawOwner::Service(..)),
            "a Linux person starts at 1000 by default"
        );
        assert!(
            matches!(classify_owner(&uid(501), Some("ana"), 500), RawOwner::Account { .. }),
            "UID_MIN 500 makes 501 a person"
        );
        assert_eq!(
            classify_owner(&uid(65_534), None, 500),
            RawOwner::Service("system service".into(), Some(uid(65_534)))
        );
        let sid = |s: &str| HelperUser::Sid(s.into());
        assert_eq!(classify_owner(&sid("S-1-5-18"), None, 0), RawOwner::System);
        assert_eq!(
            classify_owner(&sid("S-1-5-19"), None, 0),
            RawOwner::Service("LOCAL SERVICE".into(), Some(sid("S-1-5-19")))
        );
        assert!(matches!(classify_owner(&sid("S-1-5-80-1-2-3"), Some("Spooler"), 0), RawOwner::Service(n, _) if n == "Spooler"));
        assert!(matches!(
            classify_owner(&sid("S-1-5-21-1-2-3-1001"), Some("Ana"), 0),
            RawOwner::Account { .. }
        ));
    }

    #[test]
    fn names_and_paths_are_cut_and_a_huge_sweep_says_so() {
        let mut s = sweep();
        s.processes[0].name = "n".repeat(1_000);
        s.processes[0].path = Some(format!("/{}", "p".repeat(5_000)));
        let page = build_page(&s, &HelperUser::Uid(0), Visibility::Full, &query(None, 1));
        assert_eq!(
            (page.items[0].name.len(), page.items[0].path.as_ref().unwrap().len()),
            (MAX_NAME, MAX_PATH)
        );
        let many = Sweep {
            processes: (0..MAX_SWEEP as u32 + 5).map(|p| raw(p, RawOwner::System, None)).collect(),
            ..Sweep::default()
        };
        assert!(build_page(&many, &HelperUser::Uid(0), Visibility::Full, &query(None, 1)).truncated);
    }

    #[test]
    fn the_wire_roundtrips_and_has_no_field_for_a_command_line() {
        let page = build_page(&sweep(), &HelperUser::Uid(0), Visibility::Full, &query(None, 100));
        let back: ProcessPage = postcard::from_bytes(&postcard::to_stdvec(&page).unwrap()).unwrap();
        assert_eq!(back, page);
        let json = serde_json::to_string(&page).unwrap().to_lowercase();
        for never in ["commandline", "cmdline", "environ", "cwd", "openfile", "window", "argv"] {
            assert!(!json.contains(never), "{never}");
        }
    }
}
