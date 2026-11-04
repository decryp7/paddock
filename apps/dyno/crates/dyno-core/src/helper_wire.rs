//! What the agent and the privileged helper say to each other (ADR 0016), and what the app is told about the helper.
//! Pure types and decisions: the helper crate and the agent do the I/O.
//!
//! The wire body is [`HelperMessage`] (postcard inside `paddock-ipc` frames). The helper accepts only
//! [`HelperRequest`]s, from any local account, each restricted by who asks (`helper_policy`, ADR 0027 section 2). None of
//! them changes a setting of the machine. The request arguments and their caps are in `helper_requests`.

use crate::btm::LoginItemsReply;
use crate::firewall::FirewallSource;
use crate::helper::HelperHost;
use crate::helper_policy::{RequestKind, Visibility};
use crate::helper_requests::{
    validate_tool_bytes, CounterQuery, ExtrasKind, ProcessQuery, RequestError, SmartQuery, ToolKind, MAX_LOG_REPLY,
};
use crate::network_traffic::TrafficAnswer;
use crate::smart::SmartReply;
use crate::wake::{IfName, WakeEntry, WakeTag, WolReply};
use serde::{Deserialize, Serialize};

/// The version of the helper's request set. A different value is refused by the agent with "update the helper".
///
/// Wire history (the helper wire is postcard, so a changed type is a changed wire):
/// - 4: `SmartDisk` gained `wear_id`, `bytes_written` and `rated_bytes` (the helper's SMART reply carries the full wear reading).
/// - 5: `FirewallSource` gained `via` (helper-wiring plan, WP-0). The helper writes `Local`; the agent always overwrites it with
///   `Helper` on a helper reply. 1.1.0 is unreleased, so every dev machine with an older helper must Repair.
/// - 6: `HelperReply::Counters(Vec<TrafficCounter>)` became `Traffic(TrafficAnswer)` (helper-wiring plan, WP-TRF): the helper
///   scopes the counters to the asking account and adds the other accounts' programs as one total. A helper of API 5 would send a
///   reply the agent cannot read, so every dev machine with an older helper must Repair.
pub const HELPER_API: u32 = 6;

/// The oldest helper build this Dyno accepts as `Ready`. Raised only when a helper fix must reach users (ADR 0027 section 4).
pub const MIN_HELPER_VERSION: &str = "1.1.0";

/// The major version the IPC handshake carries between the agent and the helper (the minor is 0).
pub const WIRE_MAJOR: u16 = 1;

/// The largest frame between the agent and the helper, in bytes.
pub const MAX_FRAME: usize = 4 * 1024 * 1024;

/// The most audit entries one reply carries.
pub const MAX_AUDIT_REPLY: u32 = 100;

/// The longest account id the install command accepts.
const MAX_SID: usize = 184;

/// What the agent may ask (`HELPER_API` 2, ADR 0027 section 5). A closed set: there is no "run this", and no variant carries
/// a path, a descriptor or a process id chosen by the caller.
///
/// The enum is append-only within an API version: postcard numbers the variants in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HelperRequest {
    /// Who are you? (version, API, host, this caller's scope, tool states).
    Info,
    /// One page of the process list, scoped to the caller.
    Processes(ProcessQuery),
    /// Per-program traffic counters (Windows), scoped to the caller.
    Traffic,
    /// Socket inode to owning program (Linux), scoped to the caller.
    SocketOwners,
    /// The programs holding the file with this device and inode (Linux). The helper never opens the file.
    FileHolders {
        /// Device number.
        dev: u64,
        /// Inode number.
        ino: u64,
    },
    /// Login items of the caller's account plus system-wide ones (macOS).
    LoginItems,
    /// Thermal, energy and fan readings.
    Counters(CounterQuery),
    /// Memory modules from the DMI table (Linux).
    MemoryModules,
    /// SMART health of the disks the helper's own scan finds.
    Smart(SmartQuery),
    /// Puts a downloaded tool package, streamed as bytes, into the helper's tools folder. Owner and administrators only.
    InstallTool {
        /// Which tool; the helper checks the bytes against the SHA-256 and size it has pinned for it.
        tool: ToolKind,
        /// The package bytes (at most `MAX_TOOL_BYTES`).
        bytes: Vec<u8>,
    },
    /// The firewall's state, and its rules for the owner and administrators.
    Firewall,
    /// Wake requests or backup versions (Windows).
    Extras(ExtrasKind),
    /// The last audit entries (at most [`MAX_AUDIT_REPLY`]), newest first. Owner and administrators only.
    Audit(u32),
    /// The last lines of the helper's log (at most `MAX_LOG_REPLY`). Owner and administrators only.
    LogTail(u32),
    /// Removes the helper from this computer. Owner and administrators only.
    Uninstall,
    /// Sets a wake of the computer at a time (Dyno ADR 0032): a wake Dyno tagged, replacing one with the same tag. Owner and
    /// administrators only; macOS and the Linux `.deb`.
    ScheduleWake {
        /// The wake's tag (`[a-z0-9-]{1,32}`).
        tag: WakeTag,
        /// When, seconds since 1970 (UTC): in the future and at most 31 days ahead.
        at_utc: i64,
    },
    /// Cancels the wake with this tag (only Dyno's own). Owner and administrators only.
    CancelWake {
        /// The wake's tag.
        tag: WakeTag,
    },
    /// Dyno's pending wakes. Owner and administrators only.
    ListWakes,
    /// The wake-on-LAN modes of an interface (Linux, read only, modes only). Any caller.
    WolState {
        /// The interface.
        ifname: IfName,
    },
}

impl HelperRequest {
    /// The kind of this request, for the restriction table and the concurrency caps.
    pub fn kind(&self) -> RequestKind {
        match self {
            HelperRequest::Info => RequestKind::Info,
            HelperRequest::Processes(_) => RequestKind::Processes,
            HelperRequest::Traffic => RequestKind::Traffic,
            HelperRequest::SocketOwners => RequestKind::SocketOwners,
            HelperRequest::FileHolders { .. } => RequestKind::FileHolders,
            HelperRequest::LoginItems => RequestKind::LoginItems,
            HelperRequest::Counters(_) => RequestKind::Counters,
            HelperRequest::MemoryModules => RequestKind::MemoryModules,
            HelperRequest::Smart(_) => RequestKind::Smart,
            HelperRequest::InstallTool { .. } => RequestKind::InstallTool,
            HelperRequest::Firewall => RequestKind::Firewall,
            HelperRequest::Extras(_) => RequestKind::Extras,
            HelperRequest::Audit(_) => RequestKind::Audit,
            HelperRequest::LogTail(_) => RequestKind::LogTail,
            HelperRequest::Uninstall => RequestKind::Uninstall,
            HelperRequest::ScheduleWake { .. } => RequestKind::ScheduleWake,
            HelperRequest::CancelWake { .. } => RequestKind::CancelWake,
            HelperRequest::ListWakes => RequestKind::ListWakes,
            HelperRequest::WolState { .. } => RequestKind::WolState,
        }
    }

    /// The helper's own check of a decoded request (the agent checks too, but the helper never relies on it).
    /// `Audit` and `LogTail` are clamped to their caps by [`HelperRequest::audit_len`] and [`HelperRequest::log_len`].
    pub fn validate(&self) -> Result<(), RequestError> {
        match self {
            HelperRequest::Processes(q) => q.validate(),
            HelperRequest::Counters(q) => q.validate(),
            HelperRequest::Smart(q) => q.validate(),
            HelperRequest::InstallTool { bytes, .. } => validate_tool_bytes(bytes.len()),
            _ => Ok(()),
        }
    }

    /// How many audit entries an `Audit(n)` request gets.
    pub fn audit_len(n: u32) -> usize {
        n.min(MAX_AUDIT_REPLY) as usize
    }

    /// How many log lines a `LogTail(n)` request gets.
    pub fn log_len(n: u32) -> usize {
        n.min(MAX_LOG_REPLY) as usize
    }
}

/// What the helper answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HelperReply {
    /// Answer to `Info`: who the helper is, and what this caller sees of it.
    About(HelperInfo, CallerView),
    /// Answer to `Traffic`, scoped to the caller: its own programs and the system's, and other accounts' as the policy says.
    Traffic(TrafficAnswer),
    /// Answer to `Audit`.
    Entries(Vec<AuditEntry>),
    /// Answer to `Firewall`, already reduced to what the caller may have.
    Firewall(FirewallSource),
    /// Answer to `ScheduleWake` and `ListWakes`: Dyno's pending wakes, soonest first.
    Wakes(Vec<WakeEntry>),
    /// Answer to `CancelWake`: whether there was one to cancel.
    WakeCancelled(bool),
    /// Answer to `WolState`.
    Wol(WolReply),
    /// Answer to `Counters`: one outcome per requested id.
    CounterValues(Vec<crate::helper_counters::CounterAnswer>),
    /// Answer to `MemoryModules`: the slots and what sits in them (no serial numbers).
    Memory(crate::inventory::MemoryLayout),
    /// Answer to `SocketOwners`: the sockets of programs the caller may see.
    SocketOwners(crate::helper_holders::SocketOwners),
    /// Answer to `FileHolders`: the programs holding the file that the caller may see.
    FileHolders(crate::helper_holders::FileHolders),
    /// Answer to `Extras` (Windows): detail for the owner and administrators, counts for others.
    Extras(crate::helper_extras::ExtrasAnswer),
    /// Answer to `Processes`: one page, scoped to the caller.
    Processes(crate::helper_processes::ProcessPage),
    /// Answer to `Smart`.
    Smart(SmartReply),
    /// Answer to `InstallTool`: the tool is now installed.
    Installed(ToolStatus),
    /// Answer to `LoginItems`.
    LoginItems(LoginItemsReply),
    /// Answer to `LogTail`: the last lines, oldest first.
    Log(Vec<String>),
    /// Answer to `Uninstall`: the helper is going away.
    Bye,
}

/// The IPC body: one type for both directions (`paddock_ipc::BodyPeer` carries a single body type).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HelperMessage {
    /// A request.
    Request(HelperRequest),
    /// A reply.
    Reply(HelperReply),
}

/// What the helper says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperInfo {
    /// Its build version.
    pub version: String,
    /// Its [`HELPER_API`].
    pub api: u32,
    /// The system it runs on.
    pub host: HelperHost,
    /// The agent program it serves (full path).
    pub agent_exe: String,
}

/// What the helper tells one caller about its own standing (attached to the `Info` answer by the handler).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallerView {
    /// What the asking account sees of other accounts' processes and traffic.
    pub scope: Visibility,
    /// The tools in the helper's tools folder.
    pub tools: Vec<ToolStatus>,
}

/// One tool the helper can hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolStatus {
    /// Which tool.
    pub tool: ToolKind,
    /// The installed version, or `None` when it is not installed.
    pub installed: Option<String>,
}

/// How an audited call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuditVerdict {
    /// Done: the request was answered, or the helper removed itself as asked.
    Done,
    /// Not allowed (wrong caller or program).
    Refused,
    /// Allowed but it did not work (for example the log could not be written).
    Failed,
}

/// One line of the helper's audit log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    /// Seconds since 1970.
    pub at: i64,
    /// The caller's process id, when known.
    pub peer: Option<u32>,
    /// What was asked, in words (the action's preview). Never file contents.
    pub what: String,
    /// How it ended.
    pub verdict: AuditVerdict,
    /// Why, for refusals and failures.
    pub detail: String,
}

/// Parses one audit line; damaged lines are `None` (the log is read leniently).
pub fn parse_audit_line(line: &str) -> Option<AuditEntry> {
    serde_json::from_str(line.trim()).ok()
}

/// Whether the helper is turned on and usable, as the app shows it (ADR 0027 section 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HelperState {
    /// This build or system has no helper (an AppImage, no polkit): the readings say so, with no button.
    NotIncluded,
    /// Never turned on: "Turn on administrator readings", with the one prompt.
    Off,
    /// Installed but not answering (stopped, or switched off in Login Items on macOS).
    NotRunning,
    /// Something is wrong with the installation (damaged, a file missing or not protected): Repair.
    Missing,
    /// Another program answers where the helper should be: Dyno does not trust it and sends it nothing. Repair takes the place back.
    Foreign,
    /// Another API, or older than [`MIN_HELPER_VERSION`]: "Update the helper".
    Outdated,
    /// Installed and answering.
    Ready,
}

/// What the user asked of the helper in Settings; each runs as a job with phases (ADR 0027 section 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HelperAction {
    /// Turn on administrator readings (the install through the OS's one prompt).
    TurnOn,
    /// Install again over a helper that is missing something (one prompt).
    Repair,
    /// Install the bundled newer helper (one prompt).
    Update,
    /// Turn the helper off (it removes itself; the prompt only when it cannot answer).
    TurnOff,
}

impl HelperAction {
    /// The sentence for the job card.
    pub fn title(self) -> &'static str {
        match self {
            HelperAction::TurnOn => "Turning on administrator readings",
            HelperAction::Repair => "Repairing the helper",
            HelperAction::Update => "Updating the helper",
            HelperAction::TurnOff => "Turning off administrator readings",
        }
    }
}

/// The helper for the Settings page and the "turn on" dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperStatus {
    /// The state.
    pub state: HelperState,
    /// One sentence saying what it means and what to do next.
    pub message: String,
    /// The installed helper's version, when it answered.
    pub version: Option<String>,
    /// What the OS will ask for when installing, in words (shown before the prompt).
    pub prompt: String,
    /// The last audit entries, newest first.
    pub recent: Vec<AuditEntry>,
    /// What the helper shows, for the card's list (the same list whether it is off or on); empty where the helper cannot exist.
    pub readings: Vec<String>,
    /// Technical details for a "Details" disclosure (versions, an owner's account id, the OS's words); never the headline.
    pub detail: Option<String>,
    /// Why the last turn on, update or removal did not work (with what to do), until the next attempt.
    pub error: Option<String>,
}

/// What the agent found out about the helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// Nothing is installed (no endpoint).
    Absent,
    /// It answered `Info`.
    Answered(HelperInfo, Vec<AuditEntry>),
    /// The endpoint exists but nobody answered, with the reason.
    Silent(String),
    /// Something that is not Dyno's helper owns the endpoint (pipe squatting): the owner's account id (or uid).
    Foreign(String),
}

/// Whether `have` ("1.2.3", a pre-release suffix is ignored) is at least `min`. An unreadable version is not.
pub fn version_at_least(have: &str, min: &str) -> bool {
    fn parts(v: &str) -> Option<[u64; 3]> {
        let core = v.split(['-', '+']).next()?;
        let mut it = core.split('.');
        let mut out = [0u64; 3];
        for slot in &mut out {
            *slot = it.next()?.parse().ok()?;
        }
        it.next().is_none().then_some(out)
    }
    match (parts(have), parts(min)) {
        (Some(h), Some(m)) => h >= m,
        _ => false,
    }
}

/// What the OS asks for on `host`, in words.
pub fn prompt_text(host: HelperHost, verb: &str) -> String {
    match host {
        HelperHost::Windows => {
            "Windows will ask whether to let an app from an unknown publisher (dyno-helper.exe) make changes to your device. That is expected: Dyno's helper is not signed. Choose Yes."
                .to_string()
        }
        HelperHost::Macos => format!("macOS asks for your account password, in a window titled by osascript, to let Dyno {verb} its helper."),
        HelperHost::Linux => format!("Your desktop asks you to authenticate (polkit) so Dyno can {verb} its helper."),
    }
}

/// What the helper shows once it is on, per system: the one list the card uses both for "turning it on adds" and "Dyno now also shows".
pub fn readings(host: HelperHost) -> &'static [&'static str] {
    match host {
        HelperHost::Windows => &[
            "Every process of every account",
            "Per-program network traffic",
            "Disk health",
            "What is keeping the computer awake",
            "Windows Backup versions (the backups of Backup and Restore)",
        ],
        HelperHost::Macos => &[
            "Every process of every account",
            "Login items and background tasks",
            "Firewall (pf) rules",
            "Temperature and energy readings",
        ],
        HelperHost::Linux => &[
            "Every process of every account",
            "Which program owns each socket and file",
            "Firewall (nft) rules",
            "Memory modules",
        ],
    }
}

/// The text shown when an action needs a helper that is not there.
pub const HELPER_MISSING: &str =
    "This reading needs Dyno's helper, which is not installed. Install it under Settings > Helper (the system asks for your \
     permission once); nothing else in Dyno needs it.";

/// The verb the system's prompt uses for what the card offers in `state`.
fn verb_for(state: HelperState) -> &'static str {
    match state {
        HelperState::Off => "install",
        HelperState::Outdated => "update",
        HelperState::NotRunning | HelperState::Missing | HelperState::Foreign => "repair",
        _ => "change",
    }
}

/// Decides what to tell the user. `supported` is `Err(reason)` when this build or system has no helper at all. The headline
/// (`message`) is plain words with the next step; versions, owners and the OS's own words go in `detail`.
pub fn assess(host: HelperHost, supported: Result<(), String>, probe: Probe, our_version: &str) -> HelperStatus {
    let status = |state, message: String, version: Option<String>, recent, detail: Option<String>| HelperStatus {
        state,
        message,
        version,
        prompt: prompt_text(host, verb_for(state)).to_string(),
        recent,
        readings: if state == HelperState::NotIncluded {
            Vec::new()
        } else {
            readings(host).iter().map(|r| (*r).to_string()).collect()
        },
        detail,
        error: None,
    };
    match probe {
        Probe::Absent => match supported {
            Err(reason) => status(HelperState::NotIncluded, reason, None, vec![], None),
            Ok(()) => status(
                HelperState::Off,
                "Administrator readings are not turned on. Dyno works without them."
                    .into(),
                None,
                vec![],
                None,
            ),
        },
        Probe::Silent(why) => status(
            HelperState::NotRunning,
            "The helper is installed but did not answer. Repair it to start it again.".into(),
            None,
            vec![],
            Some(why),
        ),
        Probe::Foreign(owner) => status(
            HelperState::Foreign,
            "Another program is answering where Dyno's helper should be. Dyno does not trust it and sends it nothing. Repair the helper to take its place back.".into(),
            None,
            vec![],
            Some(owner),
        ),
        Probe::Answered(info, recent) if info.api != HELPER_API || !version_at_least(&info.version, MIN_HELPER_VERSION) => status(
            HelperState::Outdated,
            format!("This Dyno needs helper version {MIN_HELPER_VERSION} or newer, and the installed one is older. Update the helper."),
            Some(info.version.clone()),
            recent,
            Some(format!("Installed: version {} (API {}). Needed: API {HELPER_API}, version {MIN_HELPER_VERSION} or newer.", info.version, info.api)),
        ),
        Probe::Answered(info, recent) => status(
            HelperState::Ready,
            "Administrator readings are on.".to_string(),
            Some(info.version.clone()),
            recent,
            Some(format!("Helper version {}, Dyno {our_version}.", info.version)),
        ),
    }
}

/// The codes a refused or failed helper call carries (`RemoteError::code`).
pub mod codes {
    /// The request is not one the helper will make (not an administrator change, invalid arguments, wrong caller).
    pub const REFUSED: &str = "refused";
    /// The system said no.
    pub const DENIED: &str = "denied";
    /// Not possible on this system.
    pub const UNSUPPORTED: &str = "unsupported";
    /// The thing is not there.
    pub const NOT_FOUND: &str = "notFound";
    /// A tool took too long.
    pub const TIMEOUT: &str = "timeout";
    /// It did not work.
    pub const FAILED: &str = "failed";
    /// The account already has too many requests running; try again.
    pub const BUSY: &str = "busy";
}

/// The wire code of a platform error.
pub fn error_code(e: &crate::ports::PlatformError) -> &'static str {
    use crate::ports::PlatformError as P;
    match e {
        P::NotFound(_) => codes::NOT_FOUND,
        P::AccessDenied(_) => codes::DENIED,
        P::Unsupported(_) => codes::UNSUPPORTED,
        P::Timeout(_) => codes::TIMEOUT,
        P::Failed(_) => codes::FAILED,
    }
}

/// The platform error a wire code and message stand for (unknown codes are failures).
pub fn platform_error(code: &str, message: String) -> crate::ports::PlatformError {
    use crate::ports::PlatformError as P;
    match code {
        codes::NOT_FOUND => P::NotFound(message),
        codes::DENIED | codes::REFUSED => P::AccessDenied(message),
        codes::UNSUPPORTED => P::Unsupported(message),
        codes::TIMEOUT => P::Timeout(message),
        codes::BUSY => P::Failed(message),
        _ => P::Failed(message),
    }
}

/// The account allowed to use the helper, as passed to `dyno-helper install --user`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HelperUser {
    /// A Unix user id.
    Uid(u32),
    /// A Windows security identifier (`S-1-5-21-…`).
    Sid(String),
}

impl HelperUser {
    /// Parses `uid:501` or `sid:S-1-5-21-1-2-3-1001`.
    pub fn parse(s: &str) -> Result<HelperUser, String> {
        if let Some(n) = s.strip_prefix("uid:") {
            return n.parse::<u32>().map(HelperUser::Uid).map_err(|_| format!("{s} is not a user id."));
        }
        if let Some(sid) = s.strip_prefix("sid:") {
            let ok = sid.len() <= MAX_SID
                && sid.starts_with("S-1-")
                && sid.len() > 4
                && sid[2..].bytes().all(|b| b.is_ascii_digit() || b == b'-')
                && !sid.ends_with('-');
            return if ok {
                Ok(HelperUser::Sid(sid.to_string()))
            } else {
                Err(format!("{sid} is not a Windows account id."))
            };
        }
        Err(format!("{s} is not a user (expected uid:<n> or sid:<S-1-…>)."))
    }

    /// The text [`HelperUser::parse`] reads back.
    pub fn to_arg(&self) -> String {
        match self {
            HelperUser::Uid(n) => format!("uid:{n}"),
            HelperUser::Sid(s) => format!("sid:{s}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(version: &str, api: u32) -> HelperInfo {
        HelperInfo {
            version: version.into(),
            api,
            host: HelperHost::Linux,
            agent_exe: "/x/dyno-agent".into(),
        }
    }

    #[test]
    fn the_wire_roundtrips_through_postcard() {
        let all = [
            HelperMessage::Request(HelperRequest::Info),
            HelperMessage::Request(HelperRequest::Traffic),
            HelperMessage::Request(HelperRequest::Uninstall),
            HelperMessage::Request(HelperRequest::Audit(5)),
            HelperMessage::Reply(HelperReply::Traffic(TrafficAnswer {
                counters: vec![crate::network_traffic::TrafficCounter {
                    pid: 4,
                    name: "x".into(),
                    bytes_in: 1,
                    bytes_out: 2,
                }],
                others: Some(crate::network_traffic::OthersTraffic {
                    programs: 3,
                    bytes_in: 4,
                    bytes_out: 5,
                }),
            })),
            HelperMessage::Reply(HelperReply::About(
                info("1", 1),
                CallerView {
                    scope: Visibility::Full,
                    tools: vec![],
                },
            )),
            HelperMessage::Reply(HelperReply::Log(vec!["a".into()])),
            HelperMessage::Reply(HelperReply::LoginItems(LoginItemsReply {
                items: vec![],
                truncated: 0,
                other_accounts: 2,
            })),
            HelperMessage::Reply(HelperReply::Bye),
        ];
        for m in all {
            let bytes = postcard::to_stdvec(&m).unwrap();
            assert_eq!(postcard::from_bytes::<HelperMessage>(&bytes).unwrap(), m);
        }
    }

    /// A changed wire must never keep its API number: the history above names the current one, so a change that bumps the constant
    /// without saying why (or says why without bumping) fails here.
    #[test]
    fn the_wire_history_names_the_current_api() {
        let source = include_str!("helper_wire.rs");
        let line = format!("/// - {HELPER_API}: ");
        assert!(
            source.contains(&line),
            "add `{line}<what changed>` to the wire history of HELPER_API"
        );
        let newest = source
            .lines()
            .filter_map(|l| l.strip_prefix("/// - "))
            .filter_map(|l| l.split_once(": "))
            .filter_map(|(n, _)| n.parse::<u32>().ok())
            .max();
        assert_eq!(newest, Some(HELPER_API), "the newest history entry is the current API");
    }

    #[test]
    fn garbage_does_not_decode_into_a_request() {
        assert!(postcard::from_bytes::<HelperMessage>(&[0xff, 0xff, 0xff, 0xff]).is_err());
        assert!(postcard::from_bytes::<HelperMessage>(&[]).is_err());
    }

    #[test]
    fn status_says_what_to_do_next() {
        let w = HelperHost::Windows;
        let s = assess(w, Ok(()), Probe::Absent, "1.1.0");
        assert_eq!(s.state, HelperState::Off);
        assert!(s.message.contains("not turned on") && !s.readings.is_empty());
        assert!(
            s.prompt.contains("unknown publisher") && !s.prompt.contains("password"),
            "Windows asks about the publisher, not a password"
        );
        let s = assess(HelperHost::Linux, Err("This build has no helper.".into()), Probe::Absent, "1.1.0");
        assert_eq!(
            (s.state, s.message.as_str()),
            (HelperState::NotIncluded, "This build has no helper.")
        );
        let s = assess(w, Ok(()), Probe::Silent("timeout".into()), "1.1.0");
        assert_eq!(s.state, HelperState::NotRunning);
        assert!(s.message.contains("Repair it") && !s.message.contains("Settings") && !s.message.contains("timeout"));
        assert_eq!(
            s.detail.as_deref(),
            Some("timeout"),
            "the OS's words are in the details, not the headline"
        );
        let s = assess(w, Ok(()), Probe::Foreign("S-1-5-21-1-2-3-1001".into()), "1.1.0");
        assert_eq!(s.state, HelperState::Foreign);
        assert!(
            s.message.contains("does not trust it") && s.message.contains("Repair") && !s.message.contains("S-1-5"),
            "{}",
            s.message
        );
        assert_eq!(s.detail.as_deref(), Some("S-1-5-21-1-2-3-1001"));
        let s = assess(w, Ok(()), Probe::Answered(info("1.0.9", HELPER_API), vec![]), "1.1.0");
        assert_eq!((s.state, s.version.as_deref()), (HelperState::Outdated, Some("1.0.9")));
        assert!(
            s.message.contains(MIN_HELPER_VERSION) && !s.message.contains("API"),
            "only the needed version in the headline"
        );
        assert!(s.detail.as_deref().is_some_and(|d| d.contains("API")));
        let s = assess(w, Ok(()), Probe::Answered(info("1.1.0", HELPER_API - 1), vec![]), "1.1.0");
        assert_eq!(s.state, HelperState::Outdated);
        let s = assess(w, Ok(()), Probe::Answered(info("1.1.0", HELPER_API), vec![]), "1.1.0");
        assert_eq!(s.state, HelperState::Ready);
        for host in [HelperHost::Windows, HelperHost::Macos, HelperHost::Linux] {
            let ready = assess(host, Ok(()), Probe::Answered(info("1.1.0", HELPER_API), vec![]), "1.1.0");
            assert!(
                ready.readings.iter().map(String::as_str).eq(readings(host).iter().copied()) && !ready.message.contains("1.1.0"),
                "{host:?}: the readings now on, not the version"
            );
            let off = assess(host, Ok(()), Probe::Absent, "1.1.0");
            assert_eq!(off.readings, ready.readings, "Off and On list the same readings");
        }
        // A newer or older-but-supported helper than the app is Ready: the update is offered, never forced.
        let s = assess(w, Ok(()), Probe::Answered(info(MIN_HELPER_VERSION, HELPER_API), vec![]), "1.3.0");
        assert_eq!(s.state, HelperState::Ready);
        assert!(
            prompt_text(HelperHost::Macos, "repair").contains("password")
                && prompt_text(HelperHost::Macos, "repair").contains("repair its helper")
        );
        assert!(prompt_text(HelperHost::Linux, "update").contains("polkit"));
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(version_at_least("1.10.0", "1.9.9"));
        assert!(version_at_least("1.1.0-beta.1", "1.1.0"));
        assert!(!version_at_least("1.0.99", "1.1.0"));
        for bad in ["", "1.1", "1.1.0.1", "a.b.c", "1.1.x"] {
            assert!(!version_at_least(bad, "1.1.0"), "{bad}");
        }
    }

    #[test]
    fn every_request_has_a_kind_and_the_caps_are_checked() {
        use crate::helper_requests::*;
        let reqs = vec![
            HelperRequest::Info,
            HelperRequest::Processes(ProcessQuery::default()),
            HelperRequest::Traffic,
            HelperRequest::SocketOwners,
            HelperRequest::FileHolders { dev: 1, ino: 2 },
            HelperRequest::LoginItems,
            HelperRequest::Counters(CounterQuery {
                ids: vec![CounterId::Thermal],
                class: RequestClass::User,
            }),
            HelperRequest::MemoryModules,
            HelperRequest::Smart(SmartQuery::default()),
            HelperRequest::InstallTool {
                tool: ToolKind::Smartmontools,
                bytes: vec![1],
            },
            HelperRequest::Firewall,
            HelperRequest::Extras(ExtrasKind::WakeRequests),
            HelperRequest::Audit(5),
            HelperRequest::LogTail(5),
            HelperRequest::Uninstall,
            HelperRequest::ScheduleWake {
                tag: WakeTag::parse("nightly").unwrap(),
                at_utc: 1_800_000_000,
            },
            HelperRequest::CancelWake {
                tag: WakeTag::parse("nightly").unwrap(),
            },
            HelperRequest::ListWakes,
            HelperRequest::WolState {
                ifname: IfName::parse("eth0").unwrap(),
            },
        ];
        let kinds: Vec<_> = reqs.iter().map(|r| r.kind()).collect();
        assert_eq!(kinds, RequestKind::ALL, "one kind per request, in the table's order");
        for r in &reqs {
            assert!(r.validate().is_ok(), "{r:?}");
            let bytes = postcard::to_stdvec(&HelperMessage::Request(r.clone())).unwrap();
            assert_eq!(
                postcard::from_bytes::<HelperMessage>(&bytes).unwrap(),
                HelperMessage::Request(r.clone())
            );
        }
        assert!(HelperRequest::Processes(ProcessQuery {
            limit: 0,
            ..Default::default()
        })
        .validate()
        .is_err());
        assert!(HelperRequest::InstallTool {
            tool: ToolKind::Smartmontools,
            bytes: vec![0; MAX_TOOL_BYTES + 1]
        }
        .validate()
        .is_err());
        assert_eq!(HelperRequest::audit_len(u32::MAX), MAX_AUDIT_REPLY as usize);
        assert_eq!(HelperRequest::log_len(u32::MAX), MAX_LOG_REPLY as usize);
    }

    #[test]
    fn users_parse_and_roundtrip() {
        assert_eq!(HelperUser::parse("uid:501"), Ok(HelperUser::Uid(501)));
        let sid = "sid:S-1-5-21-1-2-3-1001";
        let u = HelperUser::parse(sid).unwrap();
        assert_eq!(u.to_arg(), sid);
        assert_eq!(HelperUser::Uid(0).to_arg(), "uid:0");
        for bad in [
            "",
            "uid:",
            "uid:-1",
            "uid:x",
            "root",
            "sid:",
            "sid:S-1-",
            "sid:S-1-5-",
            "sid:S-1-5-a",
            "sid:X-1-5",
            "sid:S-1-5 (A;;GA;;;WD)",
        ] {
            assert!(HelperUser::parse(bad).is_err(), "{bad}");
        }
        let long = format!("sid:S-1-{}", "5-".repeat(100));
        assert!(HelperUser::parse(long.trim_end_matches('-')).is_err());
    }

    #[test]
    fn platform_errors_cross_the_wire_by_code() {
        use crate::ports::PlatformError as P;
        for e in [
            P::NotFound("a".into()),
            P::AccessDenied("b".into()),
            P::Unsupported("c".into()),
            P::Timeout("d".into()),
            P::Failed("e".into()),
        ] {
            let back = platform_error(error_code(&e), e.to_string());
            assert_eq!(std::mem::discriminant(&back), std::mem::discriminant(&e));
        }
        assert!(matches!(platform_error(codes::REFUSED, "x".into()), P::AccessDenied(_)));
        assert!(matches!(platform_error("weird", "x".into()), P::Failed(_)));
    }

    #[test]
    fn audit_lines_parse_leniently() {
        let e = AuditEntry {
            at: 5,
            peer: Some(3),
            what: "x".into(),
            verdict: AuditVerdict::Done,
            detail: String::new(),
        };
        let line = serde_json::to_string(&e).unwrap();
        assert!(
            line.contains("\"verdict\":\"done\""),
            "the wire value says done, not a change: {line}"
        );
        assert_eq!(parse_audit_line(&format!("{line}\n")), Some(e));
        assert_eq!(parse_audit_line("{not json"), None);
        assert_eq!(parse_audit_line(""), None);
    }
}
