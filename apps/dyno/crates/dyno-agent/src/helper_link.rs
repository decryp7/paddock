//! The agent's side of the privileged helper (ADR 0027 WP-11): every request the agent makes of the helper goes through a
//! [`HelperTransport`] (the real one is `paddock-ipc`'s system connection, whose open flags and owner check are WP-1a's), and every
//! answer becomes the agent's own typed value. Absent, silent and foreign helpers are states, never panics, and a missing helper
//! makes a reading `Unavailable` with the reason. Logs carry the request kind, duration and outcome, never content.

use dyno_core::btm::{LoginItemsLink, LoginItemsReply};
use dyno_core::firewall::{FirewallReader, FirewallSource};
use dyno_core::helper::HelperHost;
use dyno_core::helper_counters::CounterAnswer;
use dyno_core::helper_extras::ExtrasAnswer;
use dyno_core::helper_holders::{FileHolders, SocketOwners};
use dyno_core::helper_processes::ProcessPage;
use dyno_core::helper_requests::{CounterQuery, ExtrasKind, ProcessQuery, SmartQuery, ToolKind};
use dyno_core::helper_wire::{
    assess, platform_error, HelperReply, HelperRequest, HelperState, HelperStatus, Probe, ToolStatus, HELPER_MISSING,
};
use dyno_core::inventory::MemoryLayout;
use dyno_core::network_traffic::TrafficCounter;
use dyno_core::ports::PlatformError;
use dyno_core::smart::SmartReply;
use paddock_jobs::time::Clock;
use std::sync::Arc;

/// How many audit entries the status asks for.
const STATUS_AUDIT: u32 = 20;

/// Why a call to the helper did not get an answer from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// Nothing listens on the helper's endpoint.
    Absent,
    /// The endpoint exists but nothing answered (the reason).
    Silent(String),
    /// Something that is not Dyno's helper owns the endpoint (the owner's account id): never trusted.
    Foreign(String),
    /// The helper answered with an error.
    Remote {
        /// The wire code (`dyno_core::helper_wire::codes`).
        code: String,
        /// Its sentence.
        message: String,
    },
}

/// One call to the helper and its answer. Implemented over `paddock-ipc`'s system connection, and by fakes in tests.
pub trait HelperTransport: Send + Sync {
    /// Sends `request` and waits for the answer.
    fn call(&self, request: HelperRequest) -> Result<HelperReply, TransportError>;
}

/// A boxed transport is a transport: the agent's `Shared.helper` is over `Box<dyn HelperTransport>`, so real-engine tests can
/// serve a real helper dispatcher (`tests/common/helper_rig.rs`) where the real agent has the system connection.
impl HelperTransport for Box<dyn HelperTransport> {
    fn call(&self, request: HelperRequest) -> Result<HelperReply, TransportError> {
        (**self).call(request)
    }
}

/// The helper as the agent uses it.
pub struct HelperLink<T: HelperTransport> {
    transport: T,
    clock: Arc<dyn Clock>,
    /// How many calls found the helper unreachable (absent, silent or foreign). The readiness cache drops its answer when
    /// this moves ([`crate::helper_first::ReadyGate`]).
    faults: std::sync::atomic::AtomicU64,
    /// The state last logged at info, so a poll that finds the same state logs at debug (one line per change, not per poll).
    last_state: std::sync::Mutex<Option<HelperState>>,
}

fn to_platform(e: TransportError) -> PlatformError {
    match e {
        TransportError::Absent => PlatformError::AccessDenied(HELPER_MISSING.into()),
        TransportError::Silent(why) => PlatformError::Failed(format!("The helper did not answer ({why}). Start it from Settings.")),
        TransportError::Foreign(_) => PlatformError::AccessDenied(
            "Something other than Dyno's helper answered on its endpoint, so Dyno did not trust it. Repair the helper from Settings."
                .into(),
        ),
        TransportError::Remote { code, message } => platform_error(&code, message),
    }
}

fn unexpected(what: &str) -> PlatformError {
    PlatformError::Failed(format!(
        "The helper answered {what} with the wrong kind of reply. Update the helper from Settings."
    ))
}

impl<T: HelperTransport> HelperLink<T> {
    /// A link over `transport`; `clock` times the calls for the log.
    pub fn new(transport: T, clock: Arc<dyn Clock>) -> Self {
        HelperLink {
            transport,
            clock,
            faults: std::sync::atomic::AtomicU64::new(0),
            last_state: std::sync::Mutex::new(None),
        }
    }

    fn call(&self, kind: &str, request: HelperRequest) -> Result<HelperReply, TransportError> {
        let started = self.clock.monotonic();
        let out = self.transport.call(request);
        let ms = self.clock.monotonic().saturating_sub(started).as_millis() as u64;
        match &out {
            Ok(_) => log::debug!(Request = kind, Ms = ms; "Helper call answered"),
            Err(TransportError::Remote { code, .. }) => log::warn!(Request = kind, Ms = ms, Code = code.as_str(); "Helper call refused"),
            Err(e) => {
                self.faults.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                log::warn!(Request = kind, Ms = ms, State = format!("{e:?}").split(['(', ' ']).next().unwrap_or(""); "Helper not reachable")
            }
        }
        out
    }

    /// How many calls so far found the helper unreachable.
    pub fn faults(&self) -> u64 {
        self.faults.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// The clock that times the calls (the readiness cache uses the same one).
    pub fn clock(&self) -> Arc<dyn Clock> {
        self.clock.clone()
    }

    /// What Settings shows: `Info`, then the audit entries when it answered. `supported` is `Err(reason)` where the helper
    /// cannot exist (an AppImage, no polkit).
    pub fn status(&self, host: HelperHost, supported: Result<(), String>, our_version: &str) -> HelperStatus {
        let probe = match self.call("info", HelperRequest::Info) {
            Ok(HelperReply::About(info, _)) => {
                // The audit log is for the owner and administrators; a refusal just means an empty list.
                let recent = match self.call("audit", HelperRequest::Audit(STATUS_AUDIT)) {
                    Ok(HelperReply::Entries(e)) => e,
                    _ => Vec::new(),
                };
                Probe::Answered(info, recent)
            }
            Ok(_) => Probe::Silent("it answered with the wrong kind of reply".into()),
            Err(TransportError::Absent) => Probe::Absent,
            Err(TransportError::Foreign(owner)) => Probe::Foreign(owner),
            Err(TransportError::Silent(why)) => Probe::Silent(why),
            Err(TransportError::Remote { code, message }) => Probe::Silent(format!("{code}: {message}")),
        };
        let status = assess(host, supported, probe, our_version);
        let mut last = self.last_state.lock().unwrap_or_else(|p| p.into_inner());
        if *last == Some(status.state) {
            log::debug!(State = format!("{:?}", status.state).as_str(); "Helper state unchanged");
        } else {
            log::info!(State = format!("{:?}", status.state).as_str(); "Helper state");
            *last = Some(status.state);
        }
        status
    }

    /// Sets the wake `tag` at `at_utc` (seconds since 1970): the helper's own caps and refusals come back as the error.
    pub fn schedule_wake(&self, tag: &dyno_core::wake::WakeTag, at_utc: i64) -> Result<(), PlatformError> {
        match self
            .call("schedule-wake", HelperRequest::ScheduleWake { tag: tag.clone(), at_utc })
            .map_err(to_platform)?
        {
            HelperReply::Wakes(_) => Ok(()),
            _ => Err(unexpected("ScheduleWake")),
        }
    }

    /// Cancels the wake `tag`; whether there was one.
    pub fn cancel_wake(&self, tag: &dyno_core::wake::WakeTag) -> Result<bool, PlatformError> {
        match self
            .call("cancel-wake", HelperRequest::CancelWake { tag: tag.clone() })
            .map_err(to_platform)?
        {
            HelperReply::WakeCancelled(b) => Ok(b),
            _ => Err(unexpected("CancelWake")),
        }
    }

    /// Dyno's pending wakes, soonest first.
    pub fn list_wakes(&self) -> Result<Vec<dyno_core::wake::WakeEntry>, PlatformError> {
        match self.call("list-wakes", HelperRequest::ListWakes).map_err(to_platform)? {
            HelperReply::Wakes(w) => Ok(w),
            _ => Err(unexpected("ListWakes")),
        }
    }

    /// The tools the helper holds for this account (from its `Info` answer); empty when it does not answer.
    pub fn tools(&self) -> Vec<ToolStatus> {
        match self.call("info", HelperRequest::Info) {
            Ok(HelperReply::About(_, view)) => view.tools,
            _ => Vec::new(),
        }
    }

    /// The firewall as the helper reads it (`pf` or `nft`), reduced to what this account may have.
    pub fn firewall(&self) -> Result<FirewallSource, PlatformError> {
        match self.call("firewall", HelperRequest::Firewall).map_err(to_platform)? {
            HelperReply::Firewall(f) => Ok(f),
            _ => Err(unexpected("Firewall")),
        }
    }

    /// One page of the process list, scoped to the caller by the helper.
    pub fn processes(&self, query: ProcessQuery) -> Result<ProcessPage, PlatformError> {
        match self.call("processes", HelperRequest::Processes(query)).map_err(to_platform)? {
            HelperReply::Processes(p) => Ok(p),
            _ => Err(unexpected("Processes")),
        }
    }

    /// Per-program traffic counters (Windows), scoped to the caller.
    pub fn traffic(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
        match self.call("traffic", HelperRequest::Traffic).map_err(to_platform)? {
            HelperReply::Counters(c) => Ok(c),
            _ => Err(unexpected("Traffic")),
        }
    }

    /// Socket inode to owning program (Linux), scoped to the caller.
    pub fn socket_owners(&self) -> Result<SocketOwners, PlatformError> {
        match self.call("socketOwners", HelperRequest::SocketOwners).map_err(to_platform)? {
            HelperReply::SocketOwners(o) => Ok(o),
            _ => Err(unexpected("SocketOwners")),
        }
    }

    /// The programs holding the file with this device and inode (Linux). The helper never opens the file.
    pub fn file_holders(&self, dev: u64, ino: u64) -> Result<FileHolders, PlatformError> {
        match self
            .call("fileHolders", HelperRequest::FileHolders { dev, ino })
            .map_err(to_platform)?
        {
            HelperReply::FileHolders(h) => Ok(h),
            _ => Err(unexpected("FileHolders")),
        }
    }

    /// Thermal, energy and fan readings.
    pub fn counters(&self, query: CounterQuery) -> Result<Vec<CounterAnswer>, PlatformError> {
        match self.call("counters", HelperRequest::Counters(query)).map_err(to_platform)? {
            HelperReply::CounterValues(v) => Ok(v),
            _ => Err(unexpected("Counters")),
        }
    }

    /// Memory modules from the DMI table (Linux).
    pub fn memory(&self) -> Result<MemoryLayout, PlatformError> {
        match self.call("memory", HelperRequest::MemoryModules).map_err(to_platform)? {
            HelperReply::Memory(m) => Ok(m),
            _ => Err(unexpected("MemoryModules")),
        }
    }

    /// Wake requests or Windows Backup versions.
    pub fn extras(&self, kind: ExtrasKind) -> Result<ExtrasAnswer, PlatformError> {
        match self.call("extras", HelperRequest::Extras(kind)).map_err(to_platform)? {
            HelperReply::Extras(e) => Ok(e),
            _ => Err(unexpected("Extras")),
        }
    }

    /// The login items of the caller's account plus system-wide ones (macOS).
    pub fn login_items_now(&self) -> Result<LoginItemsReply, PlatformError> {
        match self.call("loginItems", HelperRequest::LoginItems).map_err(to_platform)? {
            HelperReply::LoginItems(r) => Ok(r),
            _ => Err(unexpected("LoginItems")),
        }
    }

    /// SMART health of the disks the helper's own scan finds.
    pub fn smart(&self, query: SmartQuery) -> Result<SmartReply, PlatformError> {
        match self.call("smart", HelperRequest::Smart(query)).map_err(to_platform)? {
            HelperReply::Smart(s) => Ok(s),
            _ => Err(unexpected("Smart")),
        }
    }

    /// Streams a downloaded tool package to the helper, which checks it against its own pinned size and SHA-256.
    pub fn install_tool(&self, tool: ToolKind, bytes: Vec<u8>) -> Result<ToolStatus, PlatformError> {
        match self
            .call("installTool", HelperRequest::InstallTool { tool, bytes })
            .map_err(to_platform)?
        {
            HelperReply::Installed(t) => Ok(t),
            _ => Err(unexpected("InstallTool")),
        }
    }

    /// Asks the helper to remove itself. The reply is `Bye`; the state is `Off` afterwards.
    pub fn uninstall(&self) -> Result<(), PlatformError> {
        match self.call("uninstall", HelperRequest::Uninstall).map_err(to_platform)? {
            HelperReply::Bye => Ok(()),
            _ => Err(unexpected("Uninstall")),
        }
    }

    /// The helper's own log lines for the diagnostics bundle.
    pub fn log_tail(&self, lines: u32) -> Result<Vec<String>, PlatformError> {
        match self.call("logTail", HelperRequest::LogTail(lines)).map_err(to_platform)? {
            HelperReply::Log(l) => Ok(l),
            _ => Err(unexpected("LogTail")),
        }
    }

    /// Whether the helper answers right now (cheap: one `Info`).
    pub fn is_ready(&self, host: HelperHost, our_version: &str) -> bool {
        self.status(host, Ok(()), our_version).state == HelperState::Ready
    }
}

impl<T: HelperTransport> LoginItemsLink for HelperLink<T>
where
    T: 'static,
{
    fn login_items(&mut self) -> Result<LoginItemsReply, PlatformError> {
        self.login_items_now()
    }
}

impl<T: HelperTransport> FirewallReader for HelperLink<T> {
    fn read(&self) -> Result<FirewallSource, PlatformError> {
        self.firewall()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::firewall::RuleSource;
    use dyno_core::helper_policy::Visibility;
    use dyno_core::helper_wire::{codes, CallerView, HelperInfo, HELPER_API, MIN_HELPER_VERSION};
    use paddock_jobs::time::FakeClock;
    use std::sync::Mutex;

    /// Answers each request kind from a script and records the order of calls.
    struct Script {
        answers: Mutex<Vec<Result<HelperReply, TransportError>>>,
        seen: Mutex<Vec<HelperRequest>>,
    }
    impl HelperTransport for Script {
        fn call(&self, request: HelperRequest) -> Result<HelperReply, TransportError> {
            self.seen.lock().unwrap().push(request);
            self.answers.lock().unwrap().remove(0)
        }
    }

    fn link(answers: Vec<Result<HelperReply, TransportError>>) -> HelperLink<Script> {
        HelperLink::new(
            Script {
                answers: Mutex::new(answers),
                seen: Mutex::new(Vec::new()),
            },
            Arc::new(FakeClock::new()),
        )
    }

    fn about(version: &str, api: u32) -> HelperReply {
        HelperReply::About(
            HelperInfo {
                version: version.into(),
                api,
                host: HelperHost::Linux,
                agent_exe: "/x".into(),
            },
            CallerView {
                scope: Visibility::Full,
                tools: vec![],
            },
        )
    }

    #[test]
    fn status_covers_every_helper_state() {
        let w = HelperHost::Windows;
        type Answers = Vec<Result<HelperReply, TransportError>>;
        let cases: Vec<(Answers, Result<(), String>, HelperState)> = vec![
            (vec![Err(TransportError::Absent)], Ok(()), HelperState::Off),
            (
                vec![Err(TransportError::Absent)],
                Err("no helper in this build".into()),
                HelperState::NotIncluded,
            ),
            (
                vec![Err(TransportError::Silent("timed out".into()))],
                Ok(()),
                HelperState::NotRunning,
            ),
            (
                vec![Err(TransportError::Foreign("S-1-5-21-1".into()))],
                Ok(()),
                HelperState::Foreign,
            ),
            (
                vec![Ok(about("1.0.0", HELPER_API)), Ok(HelperReply::Entries(vec![]))],
                Ok(()),
                HelperState::Outdated,
            ),
            (
                vec![Ok(about(MIN_HELPER_VERSION, HELPER_API - 1)), Ok(HelperReply::Entries(vec![]))],
                Ok(()),
                HelperState::Outdated,
            ),
            (
                vec![Ok(about(MIN_HELPER_VERSION, HELPER_API)), Ok(HelperReply::Entries(vec![]))],
                Ok(()),
                HelperState::Ready,
            ),
            // A caller who may not read the audit log still sees Ready.
            (
                vec![
                    Ok(about(MIN_HELPER_VERSION, HELPER_API)),
                    Err(TransportError::Remote {
                        code: codes::REFUSED.into(),
                        message: "owner only".into(),
                    }),
                ],
                Ok(()),
                HelperState::Ready,
            ),
            (vec![Ok(HelperReply::Bye)], Ok(()), HelperState::NotRunning),
        ];
        for (answers, supported, want) in cases {
            assert_eq!(link(answers).status(w, supported, "1.1.0").state, want);
        }
    }

    #[test]
    fn readings_without_a_helper_say_so_and_remote_errors_keep_their_meaning() {
        let l = link(vec![
            Err(TransportError::Absent),
            Err(TransportError::Remote {
                code: codes::TIMEOUT.into(),
                message: "slow".into(),
            }),
        ]);
        assert!(matches!(l.firewall(), Err(PlatformError::AccessDenied(m)) if m.contains("helper")));
        assert!(matches!(l.smart(SmartQuery::default()), Err(PlatformError::Timeout(m)) if m == "slow"));
        let l = link(vec![Err(TransportError::Foreign("S-1-5-21-9".into()))]);
        assert!(matches!(l.uninstall(), Err(PlatformError::AccessDenied(m)) if m.contains("did not trust")));
        let l = link(vec![Err(TransportError::Silent("pipe busy".into()))]);
        assert!(matches!(l.log_tail(10), Err(PlatformError::Failed(m)) if m.contains("pipe busy")));
    }

    #[test]
    fn a_wrong_kind_of_reply_is_an_error_not_a_panic() {
        let l = link(vec![Ok(HelperReply::Bye), Ok(HelperReply::Bye), Ok(HelperReply::Bye)]);
        assert!(matches!(l.firewall(), Err(PlatformError::Failed(m)) if m.contains("Update the helper")));
        assert!(l.smart(SmartQuery::default()).is_err());
        assert!(l.install_tool(ToolKind::Smartmontools, vec![1]).is_err());
    }

    #[test]
    fn answers_pass_through_and_requests_carry_exactly_what_was_asked() {
        let src = FirewallSource::new(RuleSource::Pf);
        let l = link(vec![
            Ok(HelperReply::Firewall(src.clone())),
            Ok(HelperReply::Installed(ToolStatus {
                tool: ToolKind::Smartmontools,
                installed: Some("7.5".into()),
            })),
            Ok(HelperReply::Bye),
        ]);
        assert_eq!(l.firewall().unwrap(), src);
        assert_eq!(
            l.install_tool(ToolKind::Smartmontools, vec![9, 9]).unwrap().installed.as_deref(),
            Some("7.5")
        );
        l.uninstall().unwrap();
        let seen = l.transport.seen.lock().unwrap();
        assert_eq!(
            *seen,
            [
                HelperRequest::Firewall,
                HelperRequest::InstallTool {
                    tool: ToolKind::Smartmontools,
                    bytes: vec![9, 9]
                },
                HelperRequest::Uninstall
            ]
        );
    }

    #[test]
    fn login_items_come_through_the_link_trait() {
        let reply = LoginItemsReply {
            items: vec![],
            truncated: 0,
            other_accounts: 4,
        };
        let mut l = link(vec![Ok(HelperReply::LoginItems(reply.clone())), Err(TransportError::Absent)]);
        assert_eq!(l.login_items().unwrap(), reply);
        assert!(matches!(l.login_items(), Err(PlatformError::AccessDenied(_))));
    }
}
