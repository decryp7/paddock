//! What the helper does with a request (ADR 0016 section 4): it answers read-only questions (who it is, its audit
//! log, per-program traffic) and removes itself on request. It makes no change to the machine's settings. Pure over
//! its ports: tests give it a fake [`ProcessTraffic`] and a temporary audit log.

use crate::audit::AuditLog;
use dyno_core::btm::LoginItemsReader;
use dyno_core::firewall::FirewallReader;
use dyno_core::helper_counters::{answer, CounterReader, CounterValue};
use dyno_core::helper_extras::{backup_answer, wake_answer, ExtrasReader};
use dyno_core::helper_holders::{file_answer, socket_answer, HolderSource};
use dyno_core::helper_memory::MemoryReader;
use dyno_core::helper_policy::{Access, OthersPolicy, Visibility};
use dyno_core::helper_processes::{build_page, ProcessSweeper};
use dyno_core::helper_requests::ExtrasKind;
use dyno_core::helper_wire::{codes, error_code, AuditEntry, AuditVerdict, CallerView, HelperInfo, HelperReply, HelperRequest, HelperUser};
use dyno_core::network_traffic::{traffic_answer, MAX_COUNTERS};
use dyno_core::ports::{PlatformError, ProcessTraffic};
use dyno_core::smart::{SmartReader, ToolInstaller};
use dyno_core::wake::{plan_schedule, WakeScheduler, WolReader};
use paddock_ipc::RemoteError;

/// Everything a [`Dispatcher`] needs from outside.
pub struct Parts {
    /// What the helper tells about itself.
    pub info: HelperInfo,
    /// Per-program traffic counters.
    pub traffic: Box<dyn ProcessTraffic>,
    /// The audit log.
    pub audit: AuditLog,
    /// Seconds since 1970.
    pub clock: Box<dyn Fn() -> i64 + Send + Sync>,
    /// The macOS login-items reader (`sfltool dumpbtm` as the helper's account, parsed here).
    pub login_items: Box<dyn LoginItemsReader>,
    /// Who holds which socket or file (Linux).
    pub holders: Box<dyn HolderSource>,
    /// The Windows extras reader.
    pub extras: Box<dyn ExtrasReader>,
    /// The sensor reader of this system.
    pub counters: Box<dyn CounterReader>,
    /// The memory slot reader of this system.
    pub memory: Box<dyn MemoryReader>,
    /// The process sweep of this system.
    pub processes: Box<dyn ProcessSweeper>,
    /// The SMART reader of this system.
    pub smart: Box<dyn SmartReader>,
    /// The pinned tool installer of this system.
    pub tools: Box<dyn ToolInstaller>,
    /// Dyno's scheduled wakes of this system (macOS, Linux; elsewhere the agent schedules).
    pub wakes: Box<dyn WakeScheduler>,
    /// The wake-on-LAN reader of this system (Linux).
    pub wol: Box<dyn WolReader>,
    /// The firewall reader of this system (`pf` on macOS, nftables on Linux; Windows is read in the agent).
    pub firewall: Box<dyn FirewallReader>,
    /// The last `n` lines of the helper's log, oldest first.
    pub log_tail: Box<dyn Fn(usize) -> Vec<String> + Send + Sync>,
    /// Called after an `Uninstall` request was audited and answered (it removes the helper in the background).
    pub on_uninstall: Box<dyn Fn() + Send + Sync>,
}

/// The request handler.
pub struct Dispatcher {
    info: HelperInfo,
    traffic: Box<dyn ProcessTraffic>,
    audit: AuditLog,
    clock: Box<dyn Fn() -> i64 + Send + Sync>,
    on_uninstall: Box<dyn Fn() + Send + Sync>,
    login_items: Box<dyn LoginItemsReader>,
    wakes: Box<dyn WakeScheduler>,
    wol: Box<dyn WolReader>,
    holders: Box<dyn HolderSource>,
    extras: Box<dyn ExtrasReader>,
    counters: Box<dyn CounterReader>,
    memory: Box<dyn MemoryReader>,
    processes: Box<dyn ProcessSweeper>,
    smart: Box<dyn SmartReader>,
    tools: Box<dyn ToolInstaller>,
    firewall: Box<dyn FirewallReader>,
    log_tail: Box<dyn Fn(usize) -> Vec<String> + Send + Sync>,
}

/// Who is asking, as the connection established it (never from a request, never from a process id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ctx {
    /// The caller's process id, for the audit line only.
    pub pid: Option<u32>,
    /// The caller's Unix user id (`None` for a Windows account).
    pub uid: Option<u32>,
    /// The caller's account as the connection established it (uid or SID); `None` only for requests made inside the helper.
    pub account: Option<HelperUser>,
    /// What the restriction table gives this caller for the request.
    pub access: Access,
    /// What this caller is told about its own standing.
    pub view: CallerView,
}

impl Ctx {
    /// A caller with full access and the default view (tests, and requests made inside the helper).
    pub fn anonymous(pid: Option<u32>) -> Ctx {
        Ctx {
            pid,
            uid: None,
            account: None,
            access: Access::Full,
            view: CallerView {
                scope: Visibility::Own(OthersPolicy::Total),
                tools: vec![],
            },
        }
    }
}

fn remote(e: &PlatformError) -> RemoteError {
    RemoteError::new(error_code(e), e.to_string())
}

impl Dispatcher {
    /// A dispatcher over `parts`.
    pub fn new(parts: Parts) -> Dispatcher {
        Dispatcher {
            info: parts.info,
            traffic: parts.traffic,
            audit: parts.audit,
            clock: parts.clock,
            on_uninstall: parts.on_uninstall,
            login_items: parts.login_items,
            wakes: parts.wakes,
            wol: parts.wol,
            holders: parts.holders,
            extras: parts.extras,
            counters: parts.counters,
            memory: parts.memory,
            processes: parts.processes,
            smart: parts.smart,
            tools: parts.tools,
            firewall: parts.firewall,
            log_tail: parts.log_tail,
        }
    }

    /// The caller's account for a scoped answer; a request made inside the helper has none.
    fn account_of(&self, ctx: &Ctx) -> Result<HelperUser, RemoteError> {
        ctx.account
            .clone()
            .ok_or_else(|| RemoteError::new(codes::UNSUPPORTED, "This answer is scoped to an account; this caller has none."))
    }

    /// Writes the audit line of a wake change before it happens: a log that cannot be written means nothing was changed.
    fn audit_wake(&self, peer: Option<u32>, what: String) -> Result<(), RemoteError> {
        self.audit
            .record(&AuditEntry {
                at: (self.clock)(),
                peer,
                what,
                verdict: AuditVerdict::Done,
                detail: "Asked by the owner or an administrator.".into(),
            })
            .map_err(|m| RemoteError::new(codes::FAILED, format!("{m} Nothing was changed.")))
    }

    /// Records a connection that was turned away (wrong user or program) without answering it.
    pub fn record_refused_caller(&self, peer: Option<u32>, why: &str) {
        let _ = self.audit.record(&AuditEntry {
            at: (self.clock)(),
            peer,
            what: "A connection was refused".into(),
            verdict: AuditVerdict::Refused,
            detail: why.chars().take(300).collect(),
        });
    }

    /// Every tool install is audited, success or not (it changes the machine).
    fn audit_tool(
        &self,
        peer: Option<u32>,
        tool: dyno_core::helper_requests::ToolKind,
        outcome: &Result<dyno_core::helper_wire::ToolStatus, PlatformError>,
    ) {
        let _ = self.audit.record(&AuditEntry {
            at: (self.clock)(),
            peer,
            what: format!("Install the tool {tool:?}"),
            verdict: if outcome.is_ok() {
                AuditVerdict::Done
            } else {
                AuditVerdict::Failed
            },
            detail: outcome
                .as_ref()
                .err()
                .map(|e| e.to_string().chars().take(300).collect())
                .unwrap_or_default(),
        });
    }

    /// Answers one request from the authenticated peer `peer` (its process id).
    pub fn handle(&self, peer: Option<u32>, request: HelperRequest) -> Result<HelperReply, RemoteError> {
        self.handle_for(&Ctx::anonymous(peer), request)
    }

    /// Answers one request from the caller `ctx` describes.
    pub fn handle_for(&self, ctx: &Ctx, request: HelperRequest) -> Result<HelperReply, RemoteError> {
        let peer = ctx.pid;
        request.validate().map_err(|e| RemoteError::new(codes::REFUSED, e.to_string()))?;
        match request {
            HelperRequest::Info => {
                // The tools are what the helper's own folder holds, not what the caller's view says (ER-27: Settings must
                // be able to tell "installed" from "not installed" after an install).
                let view = CallerView {
                    tools: self.tools.installed(),
                    ..ctx.view.clone()
                };
                Ok(HelperReply::About(self.info.clone(), view))
            }
            HelperRequest::LoginItems => {
                let uid = ctx.uid.ok_or_else(|| {
                    RemoteError::new(codes::UNSUPPORTED, "Login items are read for a Unix account; this caller has none.")
                })?;
                self.login_items.read(uid).map(HelperReply::LoginItems).map_err(|e| remote(&e))
            }
            HelperRequest::Processes(q) => {
                let who = ctx.account.clone().ok_or_else(|| {
                    RemoteError::new(
                        codes::UNSUPPORTED,
                        "The process list is scoped to an account; this caller has none.",
                    )
                })?;
                let sweep = self.processes.sweep().map_err(|e| remote(&e))?;
                let page = build_page(&sweep, &who, ctx.view.scope, &q);
                log::info!(items = page.items.len(), more = page.next.is_some(); "Helper: processes answered");
                Ok(HelperReply::Processes(page))
            }
            HelperRequest::SocketOwners => {
                let who = self.account_of(ctx)?;
                let scan = self.holders.sockets().map_err(|e| remote(&e))?;
                let a = socket_answer(&scan, &who, ctx.view.scope);
                log::info!(sockets = a.owners.len(), others = a.others, truncated = a.truncated; "Helper: socket owners answered");
                Ok(HelperReply::SocketOwners(a))
            }
            HelperRequest::FileHolders { dev, ino } => {
                let who = self.account_of(ctx)?;
                let scan = self.holders.file(dev, ino).map_err(|e| remote(&e))?;
                let a = file_answer(&scan, &who, ctx.view.scope);
                log::info!(holders = a.holders.len(), others = a.others, truncated = a.truncated; "Helper: file holders answered");
                Ok(HelperReply::FileHolders(a))
            }
            HelperRequest::Extras(kind) => {
                let detail = ctx.access == Access::Full;
                let answer = match kind {
                    ExtrasKind::WakeRequests => self.extras.wake_requests().map(|i| wake_answer(i, detail)),
                    ExtrasKind::BackupVersions => self.extras.backup_versions().map(|i| backup_answer(i, detail)),
                };
                log::info!(detail = detail, ok = answer.is_ok(); "Helper: extras answered");
                answer.map(HelperReply::Extras).map_err(|e| remote(&e))
            }
            HelperRequest::Counters(q) => {
                let answers = answer(self.counters.as_ref(), &q);
                log::info!(
                    ids = answers.len(),
                    unavailable = answers.iter().filter(|a| matches!(a.value, CounterValue::Unavailable(_))).count();
                    "Helper: counters answered"
                );
                Ok(HelperReply::CounterValues(answers))
            }
            HelperRequest::MemoryModules => Ok(HelperReply::Memory(self.memory.read())),
            HelperRequest::Smart(q) => self.smart.read(q).map(HelperReply::Smart).map_err(|e| remote(&e)),
            HelperRequest::InstallTool { tool, bytes } => {
                let outcome = self.tools.install(tool, &bytes);
                self.audit_tool(peer, tool, &outcome);
                outcome.map(HelperReply::Installed).map_err(|e| remote(&e))
            }
            HelperRequest::ListWakes => self.wakes.list().map(HelperReply::Wakes).map_err(|e| remote(&e)),
            HelperRequest::WolState { ifname } => self.wol.read(&ifname).map(HelperReply::Wol).map_err(|e| remote(&e)),
            HelperRequest::ScheduleWake { tag, at_utc } => {
                let pending = self.wakes.list().map_err(|e| remote(&e))?;
                plan_schedule(&pending, &tag, at_utc, (self.clock)()).map_err(|r| RemoteError::new(codes::REFUSED, r.message()))?;
                self.audit_wake(peer, format!("Set the wake {} for {at_utc}", tag.as_str()))?;
                self.wakes.schedule(&tag, at_utc).map_err(|e| remote(&e))?;
                self.wakes.list().map(HelperReply::Wakes).map_err(|e| remote(&e))
            }
            HelperRequest::CancelWake { tag } => {
                self.audit_wake(peer, format!("Cancel the wake {}", tag.as_str()))?;
                self.wakes.cancel(&tag).map(HelperReply::WakeCancelled).map_err(|e| remote(&e))
            }
            HelperRequest::LogTail(n) => Ok(HelperReply::Log((self.log_tail)(HelperRequest::log_len(n)))),
            HelperRequest::Audit(n) => Ok(HelperReply::Entries(self.audit.tail(HelperRequest::audit_len(n)))),
            HelperRequest::Traffic => {
                // Scoped to the caller before anything leaves (plan decision D6, security): a standard account sees its own
                // programs and the system's, never another account's program names or process ids. Without an account to scope
                // to, or without the sweep that says whose a process is, nothing is answered.
                let who = self.account_of(ctx)?;
                let mut counters = self.traffic.counters().map_err(|e| remote(&e))?;
                counters.truncate(MAX_COUNTERS);
                let sweep = self.processes.sweep().map_err(|e| remote(&e))?;
                let all = counters.len();
                let a = traffic_answer(counters, &sweep, &who, ctx.view.scope);
                log::info!(
                    read = all, shown = a.counters.len(), others = a.others.map_or(0, |o| o.programs);
                    "Helper: traffic answered"
                );
                Ok(HelperReply::Traffic(a))
            }
            HelperRequest::Firewall => {
                let started = std::time::Instant::now();
                let view = self.firewall.read();
                let ms = started.elapsed().as_millis() as u64;
                match &view {
                    Ok(v) => log::info!(rules = v.rule_count, access:? = ctx.access, ms = ms; "Helper: firewall read"),
                    Err(e) => log::warn!(error = e.to_string().as_str(), ms = ms; "Helper: firewall read failed"),
                }
                view.map_err(|e| remote(&e))?
                    .for_access(ctx.access)
                    .map(HelperReply::Firewall)
                    .ok_or_else(|| RemoteError::new(codes::REFUSED, "This account may not read the firewall."))
            }
            HelperRequest::Uninstall => {
                self.audit
                    .record(&AuditEntry {
                        at: (self.clock)(),
                        peer,
                        what: "Remove the Dyno helper from this computer".into(),
                        verdict: AuditVerdict::Done,
                        detail: "Asked by the authorised user.".into(),
                    })
                    .map_err(|m| RemoteError::new(codes::FAILED, format!("{m} Nothing was removed.")))?;
                (self.on_uninstall)();
                Ok(HelperReply::Bye)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::btm::LoginItemsReply;
    use dyno_core::helper::HelperHost;
    use dyno_core::helper_extras::ExtrasAnswer;
    use dyno_core::helper_requests::{ProcessQuery, SmartQuery, ToolKind};
    use dyno_core::helper_wire::MAX_AUDIT_REPLY;
    use dyno_core::network_traffic::TrafficCounter;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    struct Traffic(Result<Vec<TrafficCounter>, PlatformError>);
    impl ProcessTraffic for Traffic {
        fn counters(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
            self.0.clone()
        }
    }

    struct NoSmart;
    impl SmartReader for NoSmart {
        fn read(&self, _: SmartQuery) -> Result<dyno_core::smart::SmartReply, PlatformError> {
            Ok(dyno_core::smart::SmartReply {
                disks: vec![],
                skipped: vec![],
                not_visited: 3,
            })
        }
    }
    impl ToolInstaller for NoSmart {
        fn install(&self, tool: ToolKind, _: &[u8]) -> Result<dyno_core::helper_wire::ToolStatus, PlatformError> {
            Ok(dyno_core::helper_wire::ToolStatus {
                tool,
                installed: Some("7.5".into()),
            })
        }
        fn installed(&self) -> Vec<dyno_core::helper_wire::ToolStatus> {
            vec![dyno_core::helper_wire::ToolStatus {
                tool: ToolKind::Smartmontools,
                installed: Some("7.5".into()),
            }]
        }
    }

    struct Items(Result<LoginItemsReply, PlatformError>);
    impl LoginItemsReader for Items {
        fn read(&self, uid: u32) -> Result<LoginItemsReply, PlatformError> {
            self.0.clone().map(|mut r| {
                r.other_accounts = uid;
                r
            })
        }
    }

    fn items_reply() -> LoginItemsReply {
        LoginItemsReply {
            items: vec![],
            truncated: 0,
            other_accounts: 0,
        }
    }

    struct Rig {
        d: Dispatcher,
        gone: Arc<AtomicBool>,
        _dir: tempfile::TempDir,
    }

    fn rig(host: HelperHost, traffic: Result<Vec<TrafficCounter>, PlatformError>) -> Rig {
        rig_with(
            host,
            traffic,
            Box::new(dyno_core::wake::NoWakes),
            Box::new(dyno_core::wake::NoWakes),
        )
    }

    fn rig_with(
        host: HelperHost,
        traffic: Result<Vec<TrafficCounter>, PlatformError>,
        wakes: Box<dyn dyno_core::wake::WakeScheduler>,
        wol: Box<dyn dyno_core::wake::WolReader>,
    ) -> Rig {
        rig_parts(host, traffic, wakes, wol, Box::new(dyno_core::helper_processes::NoProcesses))
    }

    fn rig_parts(
        host: HelperHost,
        traffic: Result<Vec<TrafficCounter>, PlatformError>,
        wakes: Box<dyn dyno_core::wake::WakeScheduler>,
        wol: Box<dyn dyno_core::wake::WolReader>,
        processes: Box<dyn ProcessSweeper>,
    ) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let gone = Arc::new(AtomicBool::new(false));
        let flag = gone.clone();
        let d = Dispatcher::new(Parts {
            info: HelperInfo {
                version: "1.0.0".into(),
                api: 1,
                host,
                agent_exe: "/x/dyno-agent".into(),
            },
            traffic: Box::new(Traffic(traffic)),
            audit: AuditLog::new(dir.path().join("audit.jsonl")),
            clock: Box::new(|| 1_000),
            on_uninstall: Box::new(move || flag.store(true, Ordering::SeqCst)),
            login_items: Box::new(Items(Ok(items_reply()))),
            holders: Box::new(dyno_core::helper_holders::NoHolders),
            extras: Box::new(dyno_core::helper_extras::NoExtras),
            counters: Box::new(dyno_core::helper_counters::NoCounters),
            memory: Box::new(dyno_core::helper_memory::NoMemory),
            processes,
            smart: Box::new(NoSmart),
            tools: Box::new(NoSmart),
            firewall: Box::new(crate::tools::NotHere("none")),
            wakes,
            wol,
            log_tail: Box::new(|n| (0..n.min(3)).map(|i| format!("line {i}")).collect()),
        });
        Rig { d, gone, _dir: dir }
    }

    fn audit(r: &Rig) -> Vec<AuditEntry> {
        match r.d.handle(None, HelperRequest::Audit(50)).unwrap() {
            HelperReply::Entries(e) => e,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn it_says_who_it_is() {
        let r = rig(HelperHost::Linux, Ok(vec![]));
        match r.d.handle(Some(4), HelperRequest::Info).unwrap() {
            HelperReply::About(i, _) => assert_eq!((i.version.as_str(), i.host), ("1.0.0", HelperHost::Linux)),
            other => panic!("{other:?}"),
        }
    }

    use dyno_core::helper_policy::{OthersPolicy as Others, Visibility as Vis};
    use dyno_core::helper_processes::{ProcState, RawOwner, RawProcess, Sweep};

    struct Sweeps(Result<Sweep, PlatformError>);
    impl ProcessSweeper for Sweeps {
        fn sweep(&self) -> Result<Sweep, PlatformError> {
            self.0.clone()
        }
    }

    fn process(pid: u32, owner: RawOwner) -> RawProcess {
        RawProcess {
            pid,
            started: Some(1),
            parent: Some(1),
            name: format!("p{pid}"),
            path: None,
            owner,
            state: ProcState::Running,
            threads: Some(1),
            cpu_time_ms: Some(1),
            memory_bytes: Some(1),
            disk_read_bytes: Some(0),
            disk_written_bytes: Some(0),
        }
    }

    fn account(uid: u32) -> RawOwner {
        RawOwner::Account {
            id: HelperUser::Uid(uid),
            name: Some(format!("user{uid}")),
        }
    }

    fn counter(pid: u32, name: &str) -> TrafficCounter {
        TrafficCounter {
            pid,
            name: name.into(),
            bytes_in: 10,
            bytes_out: 20,
        }
    }

    /// Pid 10 is the caller's (uid 1000), 20 and 21 are another account's (uid 2000), 30 is the system's; 40 is in no sweep.
    fn scoped_rig() -> Rig {
        let sweep = Sweep {
            processes: vec![
                process(10, account(1_000)),
                process(20, account(2_000)),
                process(21, account(2_000)),
                process(30, RawOwner::System),
            ],
            homes: vec![],
            taken_at: 1,
        };
        let counters = vec![
            counter(10, "mine"),
            counter(20, "theirs"),
            counter(21, "theirs-too"),
            counter(30, "system"),
            counter(40, "gone"),
        ];
        rig_parts(
            HelperHost::Windows,
            Ok(counters),
            Box::new(dyno_core::wake::NoWakes),
            Box::new(dyno_core::wake::NoWakes),
            Box::new(Sweeps(Ok(sweep))),
        )
    }

    fn as_account(uid: u32, scope: Vis) -> Ctx {
        let mut ctx = Ctx::anonymous(Some(7));
        ctx.uid = Some(uid);
        ctx.account = Some(HelperUser::Uid(uid));
        ctx.view.scope = scope;
        ctx
    }

    fn traffic_for(r: &Rig, ctx: &Ctx) -> dyno_core::network_traffic::TrafficAnswer {
        match r.d.handle_for(ctx, HelperRequest::Traffic).unwrap() {
            HelperReply::Traffic(a) => a,
            other => panic!("{other:?}"),
        }
    }

    fn pids(a: &dyno_core::network_traffic::TrafficAnswer) -> Vec<u32> {
        a.counters.iter().map(|c| c.pid).collect()
    }

    #[test]
    fn traffic_is_scoped_to_the_caller() {
        let r = scoped_rig();
        // A standard account, default policy: its own and the system's by name; the other account's programs (and the one no
        // sweep knows) as one total with no name or process id in it.
        let a = traffic_for(&r, &as_account(1_000, Vis::Own(Others::Total)));
        assert_eq!(pids(&a), vec![10, 30]);
        let o = a.others.expect("the others' total");
        assert_eq!((o.programs, o.bytes_in, o.bytes_out), (3, 30, 60));
        let wire = format!("{a:?}");
        assert!(!wire.contains("theirs") && !wire.contains("gone"), "{wire}");
    }

    #[test]
    fn traffic_follows_the_policy_for_other_accounts() {
        let r = scoped_rig();
        let hidden = traffic_for(&r, &as_account(1_000, Vis::Own(Others::Hidden)));
        assert_eq!((pids(&hidden), hidden.others), (vec![10, 30], None));
        let detail = traffic_for(&r, &as_account(1_000, Vis::Own(Others::Detail)));
        assert_eq!((pids(&detail), detail.others), (vec![10, 20, 21, 30, 40], None));
        let full = traffic_for(&r, &as_account(1_000, Vis::Full));
        assert_eq!(pids(&full), vec![10, 20, 21, 30, 40]);
        // The other account sees its own programs, not the first account's.
        let theirs = traffic_for(&r, &as_account(2_000, Vis::Own(Others::Total)));
        assert_eq!(pids(&theirs), vec![20, 21, 30]);
    }

    #[test]
    fn traffic_is_not_answered_when_it_cannot_be_scoped() {
        let r = scoped_rig();
        // No account: nothing to scope to.
        assert_eq!(r.d.handle(None, HelperRequest::Traffic).unwrap_err().code, codes::UNSUPPORTED);
        // No sweep: whose a program is cannot be told, so nothing is sent.
        let blind = rig_parts(
            HelperHost::Windows,
            Ok(vec![counter(10, "mine")]),
            Box::new(dyno_core::wake::NoWakes),
            Box::new(dyno_core::wake::NoWakes),
            Box::new(Sweeps(Err(PlatformError::Failed("the process list could not be read".into())))),
        );
        assert!(blind.d.handle_for(&as_account(1_000, Vis::Full), HelperRequest::Traffic).is_err());
    }

    #[test]
    fn traffic_is_bounded_and_a_system_without_it_says_so() {
        let many: Vec<TrafficCounter> = (0..MAX_COUNTERS as u32 + 10).map(|pid| counter(pid, "p")).collect();
        let sweep = Sweep {
            processes: (0..MAX_COUNTERS as u32 + 10).map(|pid| process(pid, account(1_000))).collect(),
            homes: vec![],
            taken_at: 1,
        };
        let r = rig_parts(
            HelperHost::Windows,
            Ok(many),
            Box::new(dyno_core::wake::NoWakes),
            Box::new(dyno_core::wake::NoWakes),
            Box::new(Sweeps(Ok(sweep))),
        );
        assert_eq!(
            traffic_for(&r, &as_account(1_000, Vis::Own(Others::Total))).counters.len(),
            MAX_COUNTERS
        );
        let unsupported = rig(HelperHost::Linux, Err(PlatformError::Unsupported("no".into())));
        assert_eq!(
            unsupported
                .d
                .handle_for(&as_account(1_000, Vis::Full), HelperRequest::Traffic)
                .unwrap_err()
                .code,
            codes::UNSUPPORTED
        );
    }

    #[test]
    fn the_audit_reply_is_bounded_and_a_turned_away_caller_is_recorded() {
        let r = rig(HelperHost::Linux, Ok(vec![]));
        for _ in 0..MAX_AUDIT_REPLY + 20 {
            r.d.record_refused_caller(Some(3), &"x".repeat(1000));
        }
        match r.d.handle(None, HelperRequest::Audit(u32::MAX)).unwrap() {
            HelperReply::Entries(e) => {
                assert_eq!(e.len(), MAX_AUDIT_REPLY as usize);
                assert_eq!(e[0].detail.len(), 300);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn invalid_requests_are_refused_and_later_readers_say_unsupported() {
        let r = rig(HelperHost::Linux, Ok(vec![]));
        let bad = HelperRequest::Processes(ProcessQuery {
            limit: 0,
            ..Default::default()
        });
        assert_eq!(r.d.handle(None, bad).unwrap_err().code, codes::REFUSED);
        let bad = HelperRequest::Smart(SmartQuery { max_disks: 99 });
        assert_eq!(r.d.handle(None, bad).unwrap_err().code, codes::REFUSED);
    }

    struct FakeFirewall(Result<dyno_core::firewall::FirewallSource, PlatformError>);
    impl dyno_core::firewall::FirewallReader for FakeFirewall {
        fn read(&self) -> Result<dyno_core::firewall::FirewallSource, PlatformError> {
            self.0.clone()
        }
    }

    fn firewall_view() -> dyno_core::firewall::FirewallSource {
        use dyno_core::firewall::{FirewallProfile, FirewallRule, RuleAction, RuleDirection, RuleSource};
        let rule = FirewallRule {
            name: "pf rule 1".into(),
            direction: RuleDirection::In,
            action: RuleAction::Block,
            enabled: true,
            program: None,
            ports: Some("= 22".into()),
            addresses: Some("from 192.0.2.0/24 to any".into()),
            profile: FirewallProfile::All,
            source: RuleSource::Pf,
            group: None,
            service: None,
            protocol: Some("tcp".into()),
        };
        dyno_core::firewall::FirewallSource::new(RuleSource::Pf).with_rules(vec![rule])
    }

    fn rig_firewall(reader: FakeFirewall) -> Rig {
        let mut r = rig(HelperHost::Macos, Ok(vec![]));
        r.d.firewall = Box::new(reader);
        r
    }

    #[test]
    fn firewall_answers_the_readers_view_and_counts_only_for_other_callers() {
        let r = rig_firewall(FakeFirewall(Ok(firewall_view())));
        let full = r.d.handle(None, HelperRequest::Firewall).unwrap();
        match full {
            HelperReply::Firewall(v) => assert_eq!((v.rule_count, v.rules.len(), v.rules_hidden), (1, 1, false)),
            other => panic!("{other:?}"),
        }
        let mut other = Ctx::anonymous(Some(9));
        other.access = Access::CountsOnly;
        match r.d.handle_for(&other, HelperRequest::Firewall).unwrap() {
            HelperReply::Firewall(v) => assert_eq!((v.rule_count, v.rules.len(), v.rules_hidden), (1, 0, true)),
            other => panic!("{other:?}"),
        }
        other.access = Access::Denied;
        assert_eq!(r.d.handle_for(&other, HelperRequest::Firewall).unwrap_err().code, codes::REFUSED);
    }

    #[test]
    fn firewall_reader_failures_keep_their_code_and_a_system_without_one_says_so() {
        let r = rig_firewall(FakeFirewall(Err(PlatformError::Unsupported(
            "nft is not available on this computer.".into(),
        ))));
        assert_eq!(r.d.handle(None, HelperRequest::Firewall).unwrap_err().code, codes::UNSUPPORTED);
        let r = rig_firewall(FakeFirewall(Err(PlatformError::Failed("pfctl failed (x).".into()))));
        assert_eq!(r.d.handle(None, HelperRequest::Firewall).unwrap_err().code, codes::FAILED);
        let r = rig(HelperHost::Windows, Ok(vec![]));
        assert_eq!(r.d.handle(None, HelperRequest::Firewall).unwrap_err().code, codes::UNSUPPORTED);
    }

    #[test]
    fn uninstalling_is_audited_then_handed_to_the_remover() {
        let r = rig(HelperHost::Linux, Ok(vec![]));
        assert_eq!(r.d.handle(Some(5), HelperRequest::Uninstall).unwrap(), HelperReply::Bye);
        assert!(r.gone.load(Ordering::SeqCst));
        assert!(audit(&r)[0].what.contains("Remove the Dyno helper"));
    }

    #[test]
    fn a_log_that_cannot_be_written_means_nothing_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let d = Dispatcher::new(Parts {
            info: HelperInfo {
                version: "1".into(),
                api: 1,
                host: HelperHost::Windows,
                agent_exe: "/x".into(),
            },
            traffic: Box::new(Traffic(Ok(vec![]))),
            audit: AuditLog::new(dir.path().join("missing").join("a.jsonl")),
            clock: Box::new(|| 5),
            on_uninstall: Box::new(|| {}),
            login_items: Box::new(Items(Ok(items_reply()))),
            holders: Box::new(dyno_core::helper_holders::NoHolders),
            extras: Box::new(dyno_core::helper_extras::NoExtras),
            counters: Box::new(dyno_core::helper_counters::NoCounters),
            memory: Box::new(dyno_core::helper_memory::NoMemory),
            processes: Box::new(dyno_core::helper_processes::NoProcesses),
            smart: Box::new(NoSmart),
            tools: Box::new(NoSmart),
            firewall: Box::new(crate::tools::NotHere("none")),
            wakes: Box::new(dyno_core::wake::NoWakes),
            wol: Box::new(dyno_core::wake::NoWakes),
            log_tail: Box::new(|_| vec![]),
        });
        assert!(d
            .handle(None, HelperRequest::Uninstall)
            .unwrap_err()
            .message
            .contains("Nothing was removed"));
    }

    #[test]
    fn info_carries_the_callers_view_and_login_items_are_read_for_the_callers_uid() {
        let r = rig(HelperHost::Macos, Ok(vec![]));
        let view = CallerView {
            scope: Visibility::Full,
            tools: vec![],
        };
        let ctx = Ctx {
            pid: Some(9),
            uid: Some(501),
            account: None,
            access: Access::Full,
            view: view.clone(),
        };
        match r.d.handle_for(&ctx, HelperRequest::Info).unwrap() {
            HelperReply::About(_, v) => {
                assert_eq!(v.scope, view.scope);
                // The tools are the helper's own, never the caller's say-so.
                assert_eq!(
                    v.tools,
                    vec![dyno_core::helper_wire::ToolStatus {
                        tool: ToolKind::Smartmontools,
                        installed: Some("7.5".into())
                    }]
                );
            }
            other => panic!("{other:?}"),
        }
        match r.d.handle_for(&ctx, HelperRequest::LoginItems).unwrap() {
            HelperReply::LoginItems(l) => assert_eq!(l.other_accounts, 501, "the reader got the connection's uid"),
            other => panic!("{other:?}"),
        }
        // A caller without a Unix account (Windows) gets "unsupported", never somebody else's items.
        let e = r.d.handle_for(&Ctx::anonymous(None), HelperRequest::LoginItems).unwrap_err();
        assert_eq!(e.code, codes::UNSUPPORTED);
        let failing = Dispatcher::new(Parts {
            info: HelperInfo {
                version: "1".into(),
                api: 2,
                host: HelperHost::Macos,
                agent_exe: String::new(),
            },
            traffic: Box::new(Traffic(Ok(vec![]))),
            audit: AuditLog::new(r._dir.path().join("a2.jsonl")),
            clock: Box::new(|| 1),
            on_uninstall: Box::new(|| {}),
            login_items: Box::new(Items(Err(PlatformError::AccessDenied("no".into())))),
            holders: Box::new(dyno_core::helper_holders::NoHolders),
            extras: Box::new(dyno_core::helper_extras::NoExtras),
            counters: Box::new(dyno_core::helper_counters::NoCounters),
            memory: Box::new(dyno_core::helper_memory::NoMemory),
            processes: Box::new(dyno_core::helper_processes::NoProcesses),
            smart: Box::new(NoSmart),
            tools: Box::new(NoSmart),
            firewall: Box::new(crate::tools::NotHere("none")),
            wakes: Box::new(dyno_core::wake::NoWakes),
            wol: Box::new(dyno_core::wake::NoWakes),
            log_tail: Box::new(|_| vec![]),
        });
        assert!(failing.handle_for(&ctx, HelperRequest::LoginItems).is_err());
    }

    #[test]
    fn the_log_tail_is_clamped_to_its_cap() {
        let r = rig(HelperHost::Linux, Ok(vec![]));
        match r.d.handle(None, HelperRequest::LogTail(u32::MAX)).unwrap() {
            HelperReply::Log(l) => assert_eq!(l, ["line 0", "line 1", "line 2"]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn smart_is_read_and_a_tool_install_is_handed_over_and_audited() {
        let r = rig(HelperHost::Windows, Ok(vec![]));
        match r.d.handle(None, HelperRequest::Smart(SmartQuery::default())).unwrap() {
            HelperReply::Smart(s) => assert_eq!(s.not_visited, 3),
            other => panic!("{other:?}"),
        }
        let install = HelperRequest::InstallTool {
            tool: ToolKind::Smartmontools,
            bytes: vec![1, 2, 3],
        };
        match r.d.handle(Some(8), install).unwrap() {
            HelperReply::Installed(t) => assert_eq!(t.installed.as_deref(), Some("7.5")),
            other => panic!("{other:?}"),
        }
        let entry = &audit(&r)[0];
        assert!(entry.what.contains("Smartmontools") && entry.verdict == AuditVerdict::Done);
    }

    use dyno_core::wake::{IfName, WakeEntry, WakeScheduler, WakeTag, WolMode, WolReader, WolReply, MAX_WAKES, MAX_WAKE_AHEAD_SECS};
    use std::sync::Mutex;

    /// An in-memory set of Dyno's wakes.
    #[derive(Clone, Default)]
    struct Wakes(Arc<Mutex<Vec<WakeEntry>>>);
    impl WakeScheduler for Wakes {
        fn schedule(&self, tag: &WakeTag, at_utc: i64) -> Result<(), PlatformError> {
            let mut w = self.0.lock().unwrap();
            w.retain(|e| &e.tag != tag);
            w.push(WakeEntry { tag: tag.clone(), at_utc });
            w.sort_by_key(|e| e.at_utc);
            Ok(())
        }
        fn cancel(&self, tag: &WakeTag) -> Result<bool, PlatformError> {
            let mut w = self.0.lock().unwrap();
            let before = w.len();
            w.retain(|e| &e.tag != tag);
            Ok(w.len() != before)
        }
        fn list(&self) -> Result<Vec<WakeEntry>, PlatformError> {
            Ok(self.0.lock().unwrap().clone())
        }
    }
    struct Wol;
    impl WolReader for Wol {
        fn read(&self, ifname: &IfName) -> Result<WolReply, PlatformError> {
            assert_eq!(ifname.as_str(), "eth0");
            Ok(WolReply {
                enabled: vec![WolMode::Magic],
                supported: vec![WolMode::Magic, WolMode::Unicast],
            })
        }
    }
    fn tag(s: &str) -> WakeTag {
        WakeTag::parse(s).unwrap()
    }

    #[test]
    fn wakes_are_checked_against_the_caps_audited_before_they_happen_and_listed() {
        let wakes = Wakes::default();
        let r = rig_with(HelperHost::Linux, Ok(vec![]), Box::new(wakes.clone()), Box::new(Wol));
        let schedule = |t: &str, at: i64| r.d.handle(Some(9), HelperRequest::ScheduleWake { tag: tag(t), at_utc: at });
        // The clock of the rig is 1_000 s.
        for (at, why) in [(1_000, "past"), (999, "past"), (1_000 + MAX_WAKE_AHEAD_SECS + 1, "ahead")] {
            let e = schedule("a", at).unwrap_err();
            assert_eq!(e.code, codes::REFUSED, "{why}");
        }
        assert!(wakes.0.lock().unwrap().is_empty(), "nothing was set");
        match schedule("a", 5_000).unwrap() {
            HelperReply::Wakes(w) => assert_eq!(
                w,
                [WakeEntry {
                    tag: tag("a"),
                    at_utc: 5_000
                }]
            ),
            other => panic!("{other:?}"),
        }
        assert!(audit(&r)[0].what.contains("Set the wake a"), "{:?}", audit(&r));
        for i in 0..MAX_WAKES - 1 {
            schedule(&format!("w{i}"), 6_000 + i as i64).unwrap();
        }
        assert_eq!(schedule("one-too-many", 9_000).unwrap_err().code, codes::REFUSED);
        assert!(schedule("a", 7_000).is_ok(), "a wake with an existing tag is replaced, not added");
        match r.d.handle(None, HelperRequest::ListWakes).unwrap() {
            HelperReply::Wakes(w) => assert_eq!(w.len(), MAX_WAKES),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            r.d.handle(None, HelperRequest::CancelWake { tag: tag("a") }).unwrap(),
            HelperReply::WakeCancelled(true)
        );
        assert_eq!(
            r.d.handle(None, HelperRequest::CancelWake { tag: tag("a") }).unwrap(),
            HelperReply::WakeCancelled(false)
        );
        assert!(audit(&r).iter().any(|e| e.what.contains("Cancel the wake a")));
    }

    #[test]
    fn a_wake_that_cannot_be_audited_is_not_set_and_wol_modes_are_read_only() {
        let wakes = Wakes::default();
        let r = rig_with(HelperHost::Linux, Ok(vec![]), Box::new(wakes.clone()), Box::new(Wol));
        // The audit log's folder goes away: the write fails, and so does the request, before anything is set.
        std::fs::remove_dir_all(r._dir.path()).unwrap();
        let e =
            r.d.handle(
                None,
                HelperRequest::ScheduleWake {
                    tag: tag("a"),
                    at_utc: 5_000,
                },
            )
            .unwrap_err();
        assert_eq!(e.code, codes::FAILED);
        assert!(wakes.0.lock().unwrap().is_empty());
        match r
            .d
            .handle(
                None,
                HelperRequest::WolState {
                    ifname: IfName::parse("eth0").unwrap(),
                },
            )
            .unwrap()
        {
            HelperReply::Wol(w) => assert_eq!(w.enabled, [WolMode::Magic]),
            other => panic!("{other:?}"),
        }
        // A system without wakes says so.
        let none = rig(HelperHost::Windows, Ok(vec![]));
        assert_eq!(none.d.handle(None, HelperRequest::ListWakes).unwrap_err().code, codes::UNSUPPORTED);
    }

    struct FakeSweep(dyno_core::helper_processes::Sweep);
    impl ProcessSweeper for FakeSweep {
        fn sweep(&self) -> Result<dyno_core::helper_processes::Sweep, PlatformError> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn processes_are_scoped_to_the_connections_account_and_never_to_a_request_field() {
        use dyno_core::helper_processes::{OwnerKind, ProcState, RawOwner, RawProcess, Sweep};
        let raw = |pid: u32, uid: u32| RawProcess {
            pid,
            started: Some(1),
            parent: None,
            name: format!("p{pid}"),
            path: None,
            owner: RawOwner::Account {
                id: HelperUser::Uid(uid),
                name: Some(format!("u{uid}")),
            },
            state: ProcState::Running,
            threads: None,
            cpu_time_ms: Some(5),
            memory_bytes: Some(7),
            disk_read_bytes: None,
            disk_written_bytes: None,
        };
        let sweep = Sweep {
            processes: vec![raw(10, 501), raw(11, 502)],
            homes: vec![],
            taken_at: 9,
        };
        let mut r = rig(HelperHost::Macos, Ok(vec![]));
        r.d.processes = Box::new(FakeSweep(sweep));
        let ask = |scope: Visibility, account: Option<HelperUser>| {
            let ctx = Ctx {
                pid: Some(1),
                uid: Some(501),
                account,
                access: Access::Full,
                view: CallerView { scope, tools: vec![] },
            };
            r.d.handle_for(&ctx, HelperRequest::Processes(Default::default()))
        };
        let me = Some(HelperUser::Uid(501));
        match ask(Visibility::Own(OthersPolicy::Total), me.clone()).unwrap() {
            HelperReply::Processes(page) => {
                assert_eq!(page.items.len(), 1);
                assert_eq!(page.items[0].owner, OwnerKind::Caller);
                assert_eq!(page.others.map(|o| (o.processes, o.memory_bytes)), Some((1, 7)));
            }
            other => panic!("{other:?}"),
        }
        match ask(Visibility::Full, me).unwrap() {
            HelperReply::Processes(page) => assert_eq!(page.items.len(), 2),
            other => panic!("{other:?}"),
        }
        assert_eq!(ask(Visibility::Full, None).unwrap_err().code, codes::UNSUPPORTED);
        // A system without a reader says so, in words.
        let none = rig(HelperHost::Windows, Ok(vec![]));
        let e = none
            .d
            .handle_for(
                &Ctx {
                    account: Some(HelperUser::Uid(1)),
                    ..Ctx::anonymous(None)
                },
                HelperRequest::Processes(Default::default()),
            )
            .unwrap_err();
        assert_eq!(e.code, codes::UNSUPPORTED);
    }

    struct Sensors;
    impl CounterReader for Sensors {
        fn read(&self, id: dyno_core::helper_requests::CounterId) -> Result<CounterValue, PlatformError> {
            match id {
                dyno_core::helper_requests::CounterId::Fans => Err(PlatformError::Unsupported("No fans.".into())),
                _ => Ok(CounterValue::Thermal(vec![])),
            }
        }
    }

    struct OneStick;
    impl MemoryReader for OneStick {
        fn read(&self) -> dyno_core::inventory::MemoryLayout {
            dyno_core::inventory::MemoryLayout::TotalOnly {
                total: 8,
                reason: dyno_core::inventory::Unavailable::failed("test"),
            }
        }
    }

    #[test]
    fn counters_answer_per_id_and_memory_modules_come_from_the_reader() {
        use dyno_core::helper_requests::{CounterId, CounterQuery, RequestClass};
        let mut r = rig(HelperHost::Linux, Ok(vec![]));
        r.d.counters = Box::new(Sensors);
        r.d.memory = Box::new(OneStick);
        let q = CounterQuery {
            ids: vec![CounterId::Fans, CounterId::Thermal],
            class: RequestClass::User,
        };
        match r.d.handle(None, HelperRequest::Counters(q)).unwrap() {
            HelperReply::CounterValues(a) => {
                assert!(matches!(&a[0].value, CounterValue::Unavailable(m) if m == "No fans."));
                assert!(matches!(a[1].value, CounterValue::Thermal(_)));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            r.d.handle(None, HelperRequest::MemoryModules).unwrap(),
            HelperReply::Memory(_)
        ));
        // Without readers the helper says each id is unavailable, and the memory reader says why.
        let none = rig(HelperHost::Windows, Ok(vec![]));
        let q = CounterQuery {
            ids: vec![CounterId::Thermal],
            class: RequestClass::User,
        };
        assert!(
            matches!(none.d.handle(None, HelperRequest::Counters(q)).unwrap(), HelperReply::CounterValues(a) if matches!(a[0].value, CounterValue::Unavailable(_)))
        );
    }

    struct FakeHolders;
    impl HolderSource for FakeHolders {
        fn sockets(&self) -> Result<dyno_core::helper_holders::HolderScan, PlatformError> {
            Ok(scan_of(&[100]))
        }
        fn file(&self, dev: u64, ino: u64) -> Result<dyno_core::helper_holders::HolderScan, PlatformError> {
            assert_eq!((dev, ino), (7, 9), "the request's own device and inode reach the reader");
            Ok(scan_of(&[]))
        }
    }

    fn scan_of(sockets: &[u64]) -> dyno_core::helper_holders::HolderScan {
        use dyno_core::helper_holders::{HolderScan, RawHolder};
        use dyno_core::helper_processes::RawOwner;
        let acct = |uid| RawOwner::Account {
            id: HelperUser::Uid(uid),
            name: None,
        };
        let h = |pid, owner| RawHolder {
            pid,
            started: Some(1),
            name: "x".into(),
            owner,
            sockets: sockets.to_vec(),
        };
        HolderScan {
            holders: vec![h(1, acct(501)), h(2, acct(502))],
            truncated: false,
        }
    }

    struct FakeExtras;
    impl ExtrasReader for FakeExtras {
        fn wake_requests(&self) -> Result<Vec<dyno_core::helper_extras::WakeRequest>, PlatformError> {
            Ok(dyno_core::helper_extras::parse_requests("SYSTEM:\n[PROCESS] a.exe\n"))
        }
        fn backup_versions(&self) -> Result<Vec<dyno_core::helper_extras::BackupVersion>, PlatformError> {
            Err(PlatformError::Failed("wbadmin failed: no message".into()))
        }
    }

    #[test]
    fn holders_are_scoped_and_extras_give_detail_only_to_full_access() {
        let mut r = rig(HelperHost::Linux, Ok(vec![]));
        r.d.holders = Box::new(FakeHolders);
        r.d.extras = Box::new(FakeExtras);
        let ctx = |access, account| Ctx {
            pid: Some(1),
            uid: Some(501),
            account,
            access,
            view: CallerView {
                scope: Visibility::Own(OthersPolicy::Total),
                tools: vec![],
            },
        };
        let me = Some(HelperUser::Uid(501));
        match r.d.handle_for(&ctx(Access::Full, me.clone()), HelperRequest::SocketOwners).unwrap() {
            HelperReply::SocketOwners(a) => assert_eq!((a.owners.len(), a.others), (1, 1)),
            other => panic!("{other:?}"),
        }
        match r
            .d
            .handle_for(&ctx(Access::Full, me.clone()), HelperRequest::FileHolders { dev: 7, ino: 9 })
            .unwrap()
        {
            HelperReply::FileHolders(a) => assert_eq!((a.holders.len(), a.others), (1, 1)),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            r.d.handle_for(&ctx(Access::Full, None), HelperRequest::SocketOwners)
                .unwrap_err()
                .code,
            codes::UNSUPPORTED
        );
        let wake = |access| {
            r.d.handle_for(&ctx(access, me.clone()), HelperRequest::Extras(ExtrasKind::WakeRequests))
                .unwrap()
        };
        assert!(matches!(
            wake(Access::Full),
            HelperReply::Extras(ExtrasAnswer::WakeRequests { count: 1, items: Some(_) })
        ));
        assert!(matches!(
            wake(Access::CountsOnly),
            HelperReply::Extras(ExtrasAnswer::WakeRequests { count: 1, items: None })
        ));
        let e =
            r.d.handle_for(&ctx(Access::Full, me), HelperRequest::Extras(ExtrasKind::BackupVersions))
                .unwrap_err();
        assert!(e.message.contains("wbadmin failed"));
    }
}
