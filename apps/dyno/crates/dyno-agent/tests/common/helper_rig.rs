//! The real-engine rig for the helper (helper-wiring plan section 4.2): the agent's side of the helper wire over a REAL
//! `dyno_helper::dispatch::Dispatcher` built from fake readers. Each request is postcard-encoded, handed to the dispatcher as the
//! caller `Caller` describes, and the reply is encoded and decoded again, so the wire encoding, the restriction table, the
//! scoping and the agent's merge are all the real ones. It does not cover the OS readers (the VM rows do). No production reader
//! is built here (ER-14; `no_real_machine.rs` checks it).

#![allow(dead_code)]

use dyno_agent::agent::HelperWiring;
use dyno_agent::helper_centre::{HelperCentre, HelperInstaller};
use dyno_agent::helper_link::{HelperLink, HelperTransport, TransportError};
use dyno_core::btm::{LoginItemsReader, LoginItemsReply};
use dyno_core::helper::HelperHost;
use dyno_core::helper_memory::MemoryReader;
use dyno_core::helper_policy::{access, visibility, OthersPolicy, Standing};
use dyno_core::helper_processes::{ProcessSweeper, RawProcess, Sweep};
use dyno_core::helper_requests::{SmartQuery, ToolKind};
use dyno_core::helper_wire::{HelperAction, HelperInfo, HelperReply, HelperRequest, HelperUser, ToolStatus, HELPER_API};
use dyno_core::inventory::MemoryLayout;
use dyno_core::network_traffic::TrafficCounter;
use dyno_core::ports::{PlatformError, ProcessTraffic};
use dyno_core::smart::{SmartReader, SmartReply, ToolInstaller};
use dyno_helper::audit::AuditLog;
use dyno_helper::dispatch::{Ctx, Dispatcher, Parts};
use paddock_jobs::time::SystemClock;
use std::sync::{Arc, Mutex};

/// Who is asking, as the connection would have established it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Caller {
    /// The account that turned the helper on.
    Owner,
    /// An administrator who is not the owner.
    Admin,
    /// Anyone else.
    Other,
}

impl Caller {
    fn standing(self) -> Standing {
        match self {
            Caller::Owner => Standing::Owner,
            Caller::Admin => Standing::Administrator,
            Caller::Other => Standing::Other,
        }
    }

    /// The account the connection would have established (a Unix uid): the owner is uid 501, the others differ, so scoping by
    /// account has someone to compare with.
    pub fn uid(self) -> u32 {
        match self {
            Caller::Owner => 501,
            Caller::Admin => 502,
            Caller::Other => 503,
        }
    }

    fn ctx(self, request: &HelperRequest) -> Ctx {
        let standing = self.standing();
        let mut ctx = Ctx::anonymous(Some(4242));
        ctx.uid = Some(self.uid());
        ctx.account = Some(HelperUser::Uid(self.uid()));
        ctx.access = access(request.kind(), standing);
        ctx.view.scope = visibility(standing, false, OthersPolicy::Total, false);
        ctx
    }
}

/// The fake readers a rig's dispatcher is built from. Everything not set answers like a system without that reading.
pub struct Readers {
    /// The memory layout the helper reads.
    pub memory: MemoryLayout,
    /// The per-program traffic counters.
    pub traffic: Vec<TrafficCounter>,
    /// The SMART reply.
    pub smart: SmartReply,
    /// The process list the helper's scoping reads whose a program is from (empty: nobody's program is known).
    pub processes: Vec<RawProcess>,
}

impl Default for Readers {
    fn default() -> Readers {
        Readers {
            memory: MemoryLayout::TotalOnly {
                total: 0,
                reason: dyno_core::inventory::Unavailable::failed("fake"),
            },
            traffic: Vec::new(),
            smart: SmartReply {
                disks: vec![],
                skipped: vec![],
                not_visited: 0,
            },
            processes: Vec::new(),
        }
    }
}

struct Memory(MemoryLayout);
impl MemoryReader for Memory {
    fn read(&self) -> MemoryLayout {
        self.0.clone()
    }
}
struct Traffic(Vec<TrafficCounter>);
impl ProcessTraffic for Traffic {
    fn counters(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
        Ok(self.0.clone())
    }
}
struct Sweeps(Vec<RawProcess>);
impl ProcessSweeper for Sweeps {
    fn sweep(&self) -> Result<Sweep, PlatformError> {
        Ok(Sweep {
            processes: self.0.clone(),
            homes: Vec::new(),
            taken_at: 1,
        })
    }
}
struct NoFirewall;
impl dyno_core::firewall::FirewallReader for NoFirewall {
    fn read(&self) -> Result<dyno_core::firewall::FirewallSource, PlatformError> {
        Err(PlatformError::Unsupported("The rig has no firewall.".into()))
    }
}
struct Items;
impl LoginItemsReader for Items {
    fn read(&self, _: u32) -> Result<LoginItemsReply, PlatformError> {
        Ok(LoginItemsReply {
            items: vec![],
            truncated: 0,
            other_accounts: 0,
        })
    }
}
struct Smart(SmartReply);
impl SmartReader for Smart {
    fn read(&self, _: SmartQuery) -> Result<SmartReply, PlatformError> {
        Ok(self.0.clone())
    }
}
struct Tools;
impl ToolInstaller for Tools {
    fn install(&self, tool: ToolKind, _: &[u8]) -> Result<ToolStatus, PlatformError> {
        Ok(ToolStatus {
            tool,
            installed: Some("7.5".into()),
        })
    }
    fn installed(&self) -> Vec<ToolStatus> {
        Vec::new()
    }
}

/// A real dispatcher over `readers`, with its audit log in `dir`.
pub fn dispatcher(host: HelperHost, readers: Readers, dir: &std::path::Path) -> Dispatcher {
    Dispatcher::new(Parts {
        info: HelperInfo {
            version: dyno_core::helper_wire::MIN_HELPER_VERSION.into(),
            api: HELPER_API,
            host,
            agent_exe: "/x/dyno-agent".into(),
        },
        traffic: Box::new(Traffic(readers.traffic)),
        audit: AuditLog::new(dir.join("audit.jsonl")),
        clock: Box::new(|| 1_000),
        login_items: Box::new(Items),
        holders: Box::new(dyno_core::helper_holders::NoHolders),
        extras: Box::new(dyno_core::helper_extras::NoExtras),
        counters: Box::new(dyno_core::helper_counters::NoCounters),
        memory: Box::new(Memory(readers.memory)),
        processes: Box::new(Sweeps(readers.processes)),
        smart: Box::new(Smart(readers.smart)),
        tools: Box::new(Tools),
        firewall: Box::new(NoFirewall),
        wakes: Box::new(dyno_core::wake::NoWakes),
        wol: Box::new(dyno_core::wake::NoWakes),
        log_tail: Box::new(|_| Vec::new()),
        on_uninstall: Box::new(|| {}),
    })
}

/// The agent's transport over a dispatcher: postcard both ways, the caller's standing for the restriction table.
pub struct DispatcherTransport {
    dispatcher: Dispatcher,
    caller: Mutex<Caller>,
    /// Whether the helper answers at all (flip it to take the helper away and bring it back).
    pub up: Arc<std::sync::atomic::AtomicBool>,
    /// Every request kind served, in order (the served-kinds line a VM run reads).
    pub served: Arc<Mutex<Vec<String>>>,
}

impl DispatcherTransport {
    /// A transport that is up, asking as `caller`.
    pub fn new(dispatcher: Dispatcher, caller: Caller) -> DispatcherTransport {
        DispatcherTransport {
            dispatcher,
            caller: Mutex::new(caller),
            up: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            served: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl HelperTransport for DispatcherTransport {
    fn call(&self, request: HelperRequest) -> Result<HelperReply, TransportError> {
        if !self.up.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(TransportError::Absent);
        }
        let on_wire = postcard::to_stdvec(&request).expect("a request encodes");
        let request: HelperRequest = postcard::from_bytes(&on_wire).expect("a request decodes");
        let ctx = self.caller.lock().unwrap().ctx(&request);
        self.served.lock().unwrap().push(format!("{:?}", request.kind()));
        match self.dispatcher.handle_for(&ctx, request) {
            Ok(reply) => {
                let bytes = postcard::to_stdvec(&reply).expect("a reply encodes");
                Ok(postcard::from_bytes(&bytes).expect("a reply decodes"))
            }
            Err(e) => Err(TransportError::Remote {
                code: e.code,
                message: e.message,
            }),
        }
    }
}

struct NeverInstall;
impl HelperInstaller for NeverInstall {
    fn install(&mut self, _: HelperAction) -> Result<(), String> {
        Err("the rig installs nothing".into())
    }
    fn uninstall(&mut self) -> Result<(), String> {
        Err("the rig installs nothing".into())
    }
}

/// A centre over `transport` (what `Shared.helper` is in the rig).
pub fn centre_over(transport: DispatcherTransport, host: HelperHost) -> HelperCentre<Box<dyn HelperTransport>> {
    HelperCentre::new(
        HelperLink::new(Box::new(transport) as Box<dyn HelperTransport>, Arc::new(SystemClock)),
        host,
        Ok(()),
        env!("CARGO_PKG_VERSION"),
        Box::new(NeverInstall),
    )
}

/// What `Running::start_with_helper` takes: the centre, and the local memory reader of a computer whose slot table only an
/// administrator reads (Linux without the helper).
pub fn wiring(transport: DispatcherTransport, host: HelperHost) -> HelperWiring {
    HelperWiring {
        centre: centre_over(transport, host),
        local_memory: Box::new(|total| MemoryLayout::TotalOnly {
            total,
            reason: dyno_core::inventory::Unavailable::linux_dmi_root_only(),
        }),
    }
}

/// An owner account for fixtures.
pub fn owner() -> HelperUser {
    HelperUser::parse("uid:501").unwrap()
}
