//! What the agent and the privileged helper say to each other (ADR 0016), and what the app is told about the helper.
//! Pure types and decisions: the helper crate and the agent do the I/O.
//!
//! The wire body is [`HelperMessage`] (postcard inside `dyno-ipc` frames). The helper accepts only
//! [`HelperRequest`]s, only from its authorised user. None of them changes a setting of the machine.

use crate::helper::HelperHost;
use crate::network_traffic::TrafficCounter;
use serde::{Deserialize, Serialize};

/// The version of the helper's request set. A different value is refused by the agent with "update the helper".
pub const HELPER_API: u32 = 1;

/// The most audit entries one reply carries.
pub const MAX_AUDIT_REPLY: u32 = 100;

/// The longest account id the install command accepts.
const MAX_SID: usize = 184;

/// What the agent may ask. A closed set: there is no "run this".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HelperRequest {
    /// Who are you? (version, API, host, the agent it serves).
    Info,
    /// Per-program traffic counters (Windows: needs the TCP statistics switched on, which needs administrator rights).
    Traffic,
    /// The last audit entries (at most [`MAX_AUDIT_REPLY`]), newest first.
    Audit(u32),
    /// Removes the helper from this computer.
    Uninstall,
}

/// What the helper answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HelperReply {
    /// Answer to `Info`.
    About(HelperInfo),
    /// Answer to `Traffic`.
    Counters(Vec<TrafficCounter>),
    /// Answer to `Audit`.
    Entries(Vec<AuditEntry>),
    /// Answer to `Uninstall`: the helper is going away.
    Bye,
}

/// The IPC body: one type for both directions (`dyno-ipc` frames carry a single body type).
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

/// Whether the helper is installed and usable, as the app shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HelperState {
    /// This system cannot install it (no polkit, no helper file next to the agent).
    Unsupported,
    /// Not installed; Dyno works in observe mode and installs it only when asked.
    NotInstalled,
    /// Installed and answering.
    Ready,
    /// Installed but built by another version of Dyno.
    Outdated,
    /// Installed but not answering.
    Unreachable,
    /// The system's permission prompt is open or the install is running (poll `HelperStatus` until it changes).
    Installing,
}

/// The helper for the Settings page and the "install the helper" dialog.
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
    /// Why the last install, update or removal did not work (with what to do), until the next attempt.
    pub error: Option<String>,
}

/// What the agent found out about the helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// Nothing is installed (no endpoint or no token).
    Absent,
    /// It answered `Info`.
    Answered(HelperInfo, Vec<AuditEntry>),
    /// The endpoint exists but nobody answered, with the reason.
    Silent(String),
}

/// What the OS asks for on `host`, in words.
pub fn prompt_text(host: HelperHost) -> &'static str {
    match host {
        HelperHost::Windows => "Windows asks you to allow Dyno to make changes to your device (User Account Control).",
        HelperHost::Macos => "macOS asks for your account password to let Dyno install its helper.",
        HelperHost::Linux => "Your desktop asks you to authenticate (polkit) so Dyno can install its helper.",
    }
}

/// The text shown when an action needs a helper that is not there.
pub const HELPER_MISSING: &str =
    "This reading needs Dyno's helper, which is not installed. Install it under Settings > Helper (the system asks for your \
     permission once); nothing else in Dyno needs it.";

/// Decides what to tell the user. `supported` is `Err(reason)` when this system cannot install the helper at all.
pub fn assess(host: HelperHost, supported: Result<(), String>, probe: Probe, our_version: &str) -> HelperStatus {
    let prompt = prompt_text(host).to_string();
    let status = |state, message: String, version: Option<String>, recent| HelperStatus {
        state,
        message,
        version,
        prompt: prompt.clone(),
        recent,
        error: None,
    };
    match probe {
        Probe::Absent => match supported {
            Err(reason) => status(HelperState::Unsupported, reason, None, vec![]),
            Ok(()) => status(
                HelperState::NotInstalled,
                "The helper is not installed. Dyno works without it; install it to read the few things that need administrator rights (for example programs' network traffic on Windows)."
                    .into(),
                None,
                vec![],
            ),
        },
        Probe::Silent(why) => status(
            HelperState::Unreachable,
            format!("The helper is installed but did not answer ({why}). Install it again from Settings to repair it."),
            None,
            vec![],
        ),
        Probe::Answered(info, recent) if info.api != HELPER_API || info.version != our_version => status(
            HelperState::Outdated,
            format!(
                "The installed helper is version {} and this Dyno is {our_version}. Update the helper (the system asks for permission once).",
                info.version
            ),
            Some(info.version),
            recent,
        ),
        Probe::Answered(info, recent) => status(
            HelperState::Ready,
            "The helper is installed and ready. It only reads and changes nothing on this computer.".into(),
            Some(info.version),
            recent,
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
        _ => P::Failed(message),
    }
}

/// The account allowed to use the helper, as passed to `dyno-helper install --user`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
            HelperMessage::Reply(HelperReply::Counters(vec![TrafficCounter {
                pid: 4,
                name: "x".into(),
                bytes_in: 1,
                bytes_out: 2,
            }])),
            HelperMessage::Reply(HelperReply::About(info("1", 1))),
            HelperMessage::Reply(HelperReply::Bye),
        ];
        for m in all {
            let bytes = postcard::to_stdvec(&m).unwrap();
            assert_eq!(postcard::from_bytes::<HelperMessage>(&bytes).unwrap(), m);
        }
    }

    #[test]
    fn garbage_does_not_decode_into_a_request() {
        assert!(postcard::from_bytes::<HelperMessage>(&[0xff, 0xff, 0xff, 0xff]).is_err());
        assert!(postcard::from_bytes::<HelperMessage>(&[]).is_err());
    }

    #[test]
    fn status_says_what_to_do_next() {
        let w = HelperHost::Windows;
        let s = assess(w, Ok(()), Probe::Absent, "1.0.0");
        assert_eq!(s.state, HelperState::NotInstalled);
        assert!(s.message.contains("install"));
        assert!(s.prompt.contains("User Account Control"));
        let s = assess(
            HelperHost::Linux,
            Err("Install polkit (pkexec) first.".into()),
            Probe::Absent,
            "1.0.0",
        );
        assert_eq!(
            (s.state, s.message.as_str()),
            (HelperState::Unsupported, "Install polkit (pkexec) first.")
        );
        let s = assess(w, Ok(()), Probe::Silent("timeout".into()), "1.0.0");
        assert_eq!(s.state, HelperState::Unreachable);
        assert!(s.message.contains("again"));
        let s = assess(w, Ok(()), Probe::Answered(info("0.9.0", 1), vec![]), "1.0.0");
        assert_eq!((s.state, s.version.as_deref()), (HelperState::Outdated, Some("0.9.0")));
        let s = assess(w, Ok(()), Probe::Answered(info("1.0.0", 2), vec![]), "1.0.0");
        assert_eq!(s.state, HelperState::Outdated);
        let s = assess(w, Ok(()), Probe::Answered(info("1.0.0", HELPER_API), vec![]), "1.0.0");
        assert_eq!(s.state, HelperState::Ready);
        assert!(prompt_text(HelperHost::Macos).contains("password"));
        assert!(prompt_text(HelperHost::Linux).contains("polkit"));
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
