//! The crash model of service repair (ADR 0056 section 7): the real [`Manager`] over a simulated OS, explored **to a fixed
//! point** and not to a depth.
//!
//! A state is `(simulated OS, the record, the app's own settings)`. From every reachable state the explorer takes every
//! event of the alphabet (entry points, OS and user changes, faults of single OS calls) and, for an entry point, every
//! interruption of it: a crash before or after each OS effect, a cancel, a record that stops being writable. A crash is an
//! edge like any other, so a crash during recovery is a crash from the state a crash left. The search ends when no new
//! state appears, so it covers every depth.
//!
//! The key of a state abstracts what cannot change a decision: `attempts` saturate at the cap, `resume_pending` is a flag,
//! the clock is gone (the OS keeps "seconds until the job starts", bucketed), versions are four names. The run is
//! breadth-first and ordered, so it is deterministic (there is no randomness and so no seed to print); a violation prints
//! the shortest path, which `MODEL_REPLAY` replays with the OS calls of every step.
//!
//! Violations are either **bugs the model found** (listed in [`KNOWN_FAILING`] with where they point; the test fails when
//! one is not found any more, so a fix removes its line) or new ones (the test fails). No invariant is weakened for a
//! listed bug.

use paddock_service::backend::{
    Backend, Capabilities, DisabledMark, Inspection, Job, LastExit, LoginItemFacts, Mechanism,
    Registration, UnregisterError,
};
use paddock_service::plan::{Hold, MAX_ATTEMPTS, Plan};
use paddock_service::port::{AppServiceStatus, RegisterError};
use paddock_service::record::{Memory, Phase, RecordFile, Recorded, State};
use paddock_service::{Manager, RepairOutcome, ServiceError, Started, Timings};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

const LABEL: &str = "net.model.agent";

/// Fewer states than this means the exploration was cut short or the alphabet shrank: the test fails.
const STATES_FLOOR: usize = 100_000;
/// Fewer edges than this, the same.
const EDGES_FLOOR: u64 = 3_000_000;
/// Safety net: the space is finite, so this is never reached unless a bucket is missing from the key.
const STATES_CEILING: usize = 400_000;

// ---------------------------------------------------------------------------------------------------------------------
// The alphabet

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Ver {
    /// Older than this app (a downgrade, or the other copy).
    V0,
    /// This app's version at the start.
    V1,
    /// The version an update brings.
    V2,
    /// A newer copy's.
    V3,
}

impl Ver {
    fn s(self) -> &'static str {
        match self {
            Ver::V0 => "1.0.0",
            Ver::V1 => "1.1.0",
            Ver::V2 => "1.2.0",
            Ver::V3 => "1.3.0",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Caps {
    Mac,
    Linux,
    NoAuto,
}

fn caps_of(c: Caps) -> Capabilities {
    match c {
        Caps::Mac => Capabilities {
            automatic: true,
            pinned_identity: true,
            login_items: true,
        },
        Caps::Linux => Capabilities {
            automatic: true,
            pinned_identity: false,
            login_items: false,
        },
        Caps::NoAuto => Capabilities {
            automatic: false,
            pinned_identity: false,
            login_items: false,
        },
    }
}

/// How the OS reacts to a registration.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Resp {
    /// The job comes up 2 s after it is registered.
    Fast,
    /// 50 s (inside the 120 s wait).
    Slow,
    /// 300 s (after the wait, still healthy).
    Late,
    /// Never.
    Never,
    /// Refused whatever is registered.
    RefusedAlways,
}

impl Resp {
    fn delay(self) -> Option<u32> {
        match self {
            Resp::Fast => Some(2),
            Resp::Slow => Some(50),
            Resp::Late => Some(300),
            Resp::Never | Resp::RefusedAlways => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Reg {
    Absent,
    Registered,
    SwitchedOff,
    NeedsApproval,
    Unavailable,
}

/// The simulated OS, the record on disk and the app's own settings: one state of the graph.
#[derive(Clone, Debug)]
struct World {
    caps: Caps,
    resp: Resp,
    reg: Reg,
    /// The version whose copy registered it.
    owner: Ver,
    /// The registration points at another copy of the app.
    other_copy: bool,
    program_gone: bool,
    job: Job,
    /// The job is the old build's: refused until it is evicted.
    stale: bool,
    /// Seconds until a registered job comes up.
    start_in: Option<u32>,
    /// Seconds until an evicted job is dropped by the OS (a removal that times out).
    linger: u32,
    mark_unreadable: bool,
    /// This app's version.
    cur: Ver,
    wanted: Option<bool>,
    first_run_done: bool,
    location_ok: bool,
    /// The record cannot be written (a newer format, a read-only folder).
    ro: bool,
    /// The record file is there but cannot be used (damaged, unreadable): the app adopts a service that answers.
    rec_damaged: bool,
    /// One of the rare events happened on this path (see [`is_rare`]).
    rare_used: bool,
    rec: Recorded,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ev {
    // Entry points of the app (the Manager is called).
    Start,
    FirstRun,
    Install,
    Repair,
    Uninstall,
    Resume,
    Update,
    Downgrade,
    // The OS and the user (nothing of the app runs).
    Reboot,
    Wait,
    ToggleLoginItem,
    ToggleMark,
    JobCrash,
    JobUnknown,
    JobRefuse,
    StopRequested,
    SettingsOn,
    SettingsOff,
    SettingsDamaged,
    DamageRecord,
    NewerRecord,
    CopyNewer,
    CopyOlder,
    ProgramGone,
    Relocate,
}

const ALL_EVS: [Ev; 25] = [
    Ev::Start,
    Ev::FirstRun,
    Ev::Install,
    Ev::Repair,
    Ev::Uninstall,
    Ev::Resume,
    Ev::Update,
    Ev::Downgrade,
    Ev::Reboot,
    Ev::Wait,
    Ev::ToggleLoginItem,
    Ev::ToggleMark,
    Ev::JobCrash,
    Ev::JobUnknown,
    Ev::JobRefuse,
    Ev::StopRequested,
    Ev::SettingsOn,
    Ev::SettingsOff,
    Ev::SettingsDamaged,
    Ev::DamageRecord,
    Ev::NewerRecord,
    Ev::CopyNewer,
    Ev::CopyOlder,
    Ev::ProgramGone,
    Ev::Relocate,
];

/// Whether the long run is asked for (`MODEL_DEEP=1`): every event may repeat and faulted replaces are crashed too. The
/// default run keeps the whole alphabet but lets a path take at most one rare event, so it finishes in seconds; both end at a
/// fixed point and print their size.
fn deep() -> bool {
    static DEEP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DEEP.get_or_init(|| std::env::var("MODEL_DEEP").is_ok())
}

/// Events that matter in combination with the core alphabet but not with each other: a path takes at most one of them, so
/// each is tried from every core state without multiplying the state space by every combination.
fn is_rare(e: Ev) -> bool {
    !deep()
        && matches!(
            e,
            Ev::ToggleMark
                | Ev::JobUnknown
                | Ev::JobCrash
                | Ev::StopRequested
                | Ev::SettingsDamaged
                | Ev::DamageRecord
                | Ev::NewerRecord
                | Ev::CopyNewer
                | Ev::CopyOlder
                | Ev::ProgramGone
                | Ev::Relocate
                | Ev::Downgrade
        )
}

fn is_entry(e: Ev) -> bool {
    matches!(
        e,
        Ev::Start
            | Ev::FirstRun
            | Ev::Install
            | Ev::Repair
            | Ev::Uninstall
            | Ev::Resume
            | Ev::Update
            | Ev::Downgrade
    )
}

/// The app acts without the user asking.
fn unattended(e: Ev) -> bool {
    matches!(
        e,
        Ev::Start | Ev::FirstRun | Ev::Resume | Ev::Update | Ev::Downgrade
    )
}

/// A fault of the OS calls of one entry point (all of its calls of that kind).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fault {
    None,
    RegNeedsApproval,
    RegUnavailable,
    RegNotAsWritten,
    RegOther,
    RegNeedsElevation,
    RegDeclined,
    UnregFails,
    /// The old job outlives the wait for its removal.
    RemovalTimeout,
    /// A register or unregister call takes 130 s.
    SlowOs,
    /// Another copy holds the repair lock.
    LockHeld,
}

const FAULTS: [Fault; 10] = [
    Fault::RegNeedsApproval,
    Fault::RegUnavailable,
    Fault::RegNotAsWritten,
    Fault::RegOther,
    Fault::RegNeedsElevation,
    Fault::RegDeclined,
    Fault::UnregFails,
    Fault::RemovalTimeout,
    Fault::SlowOs,
    Fault::LockHeld,
];

fn is_reg_fault(f: Fault) -> bool {
    matches!(
        f,
        Fault::RegNeedsApproval
            | Fault::RegUnavailable
            | Fault::RegNotAsWritten
            | Fault::RegOther
            | Fault::RegNeedsElevation
            | Fault::RegDeclined
    )
}

/// An interruption of one entry point at OS call number `k` (1-based; 0 is before the first).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Intr {
    None,
    CrashBefore(u32),
    CrashAfter(u32),
    /// The cancel (Quit) is set when call `k` is reached.
    Cancel(u32),
    /// From call `k` on the record cannot be written.
    WritesFail(u32),
}

struct Crash;

fn silence_crashes() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if info.payload().downcast_ref::<Crash>().is_none() {
                previous(info);
            }
        }));
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// The simulated OS as a Backend

#[derive(Clone, Copy)]
struct Op {
    k: u32,
    label: &'static str,
}

fn mutating(label: &str) -> bool {
    matches!(label, "register" | "unregister" | "evict")
}

struct Sim {
    w: World,
    fault: Fault,
    intr: Intr,
    tick: u32,
    ops: Vec<Op>,
    trace: Vec<String>,
    cancel: Arc<AtomicBool>,
    mem: Arc<Memory>,
    registers: u32,
    prompts: u32,
    reg_while_off: u32,
}

fn lk(s: &Mutex<Sim>) -> std::sync::MutexGuard<'_, Sim> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

/// Time passes in the OS: a registered job comes up, an evicted one is dropped.
fn advance(w: &mut World, secs: u32) {
    if w.linger > 0 {
        if secs >= w.linger {
            // The OS drops the old job; what is registered now starts as it would after a fresh registration.
            w.linger = 0;
            w.stale = false;
            w.start_in = None;
            if w.reg == Reg::Registered {
                if w.resp == Resp::RefusedAlways {
                    w.job = Job::Refused;
                    w.stale = true;
                } else {
                    w.job = Job::NotRunning;
                    w.start_in = w.resp.delay();
                }
            } else {
                w.job = Job::Absent;
            }
        } else {
            w.linger -= secs;
        }
    }
    if let Some(s) = w.start_in
        && w.job == Job::NotRunning
        && w.reg == Reg::Registered
        && !w.stale
    {
        if secs >= s {
            w.job = Job::Running;
            w.start_in = None;
        } else {
            w.start_in = Some(s - secs);
        }
    }
}

struct SimBackend {
    s: Arc<Mutex<Sim>>,
}

impl SimBackend {
    /// Counts the call, applies an interruption set for it and returns its number.
    fn enter(&self, label: &'static str) -> u32 {
        let mut s = lk(&self.s);
        s.tick += 1;
        let k = s.tick;
        s.ops.push(Op { k, label });
        let line = format!(
            "  #{k} {label}: reg {:?} owner {:?} job {:?} stale {}",
            s.w.reg, s.w.owner, s.w.job, s.w.stale
        );
        s.trace.push(line);
        match s.intr {
            Intr::Cancel(n) if n == k => s.cancel.store(true, Ordering::SeqCst),
            Intr::WritesFail(n) if n == k => s.mem.fail_writes(true),
            Intr::CrashBefore(n) if n == k => {
                s.trace.push("  -- crash before the effect".into());
                drop(s);
                panic_any(Crash);
            }
            _ => {}
        }
        k
    }

    fn leave(&self, k: u32) {
        let mut s = lk(&self.s);
        if s.intr == Intr::CrashAfter(k) {
            s.trace.push("  -- crash after the effect".into());
            drop(s);
            panic_any(Crash);
        }
    }

    fn look(w: &World) -> Inspection {
        let mut i = Inspection::absent();
        i.registration = match w.reg {
            Reg::Absent => Registration::Absent,
            Reg::Registered => Registration::Registered,
            Reg::SwitchedOff => Registration::SwitchedOff,
            Reg::NeedsApproval => Registration::NeedsApproval,
            Reg::Unavailable => Registration::Unavailable("the app runs from a disk image".into()),
        };
        i.job = w.job;
        i.program_exists = Some(!w.program_gone);
        i.points_here = Some(!w.other_copy);
        // LaunchdBundled (SMAppService) cannot compare a definition; systemd and the SCM can.
        i.definition_current = (w.caps != Caps::Mac)
            .then(|| w.reg == Reg::Registered && w.owner == w.cur && !w.other_copy);
        if caps_of(w.caps).login_items {
            let (status, mark) = match w.reg {
                Reg::Registered => (AppServiceStatus::Enabled, DisabledMark::NotListed),
                Reg::SwitchedOff => (AppServiceStatus::RequiresApproval, DisabledMark::Disabled),
                Reg::NeedsApproval => (AppServiceStatus::RequiresApproval, DisabledMark::NotListed),
                Reg::Absent => (AppServiceStatus::NotRegistered, DisabledMark::NotListed),
                Reg::Unavailable => (AppServiceStatus::NotFound, DisabledMark::NotListed),
            };
            let mark = if w.mark_unreadable {
                DisabledMark::Unreadable
            } else {
                mark
            };
            i.login_item = Some(LoginItemFacts { status, mark });
        }
        i
    }
}

impl Backend for SimBackend {
    fn mechanism(&self) -> Mechanism {
        match lk(&self.s).w.caps {
            Caps::Mac => Mechanism::LaunchdBundled,
            Caps::Linux => Mechanism::SystemdUser,
            Caps::NoAuto => Mechanism::WindowsService,
        }
    }
    fn capabilities(&self) -> Capabilities {
        caps_of(lk(&self.s).w.caps)
    }
    fn inspect(&mut self) -> Inspection {
        let k = self.enter("inspect");
        let i = Self::look(&lk(&self.s).w);
        self.leave(k);
        i
    }
    fn register(&mut self) -> Result<Inspection, RegisterError> {
        let k = self.enter("register");
        let result = {
            let mut s = lk(&self.s);
            if s.fault == Fault::SlowOs {
                advance(&mut s.w, 130);
            }
            s.registers += 1;
            if !caps_of(s.w.caps).automatic {
                s.prompts += 1;
            }
            match (s.w.reg, s.fault) {
                (Reg::SwitchedOff, _) => {
                    s.reg_while_off += 1;
                    Err(RegisterError::NeedsApproval)
                }
                (Reg::NeedsApproval, _) | (_, Fault::RegNeedsApproval) => {
                    if s.w.caps == Caps::Mac {
                        // The item is registered by this copy and waits for the user's approval.
                        s.w.reg = Reg::NeedsApproval;
                        s.w.owner = s.w.cur;
                        s.w.other_copy = false;
                        s.w.program_gone = false;
                    }
                    Err(RegisterError::NeedsApproval)
                }
                (Reg::Unavailable, _) | (_, Fault::RegUnavailable) => {
                    Err(RegisterError::Unavailable("unavailable here".into()))
                }
                (_, Fault::RegOther) => Err(RegisterError::Other("the tool failed".into())),
                (_, Fault::RegNeedsElevation) => Err(RegisterError::NeedsElevation),
                (_, Fault::RegDeclined) => Err(RegisterError::Declined),
                (_, fault) => {
                    let w = &mut s.w;
                    w.reg = Reg::Registered;
                    w.owner = w.cur;
                    w.other_copy = false;
                    w.program_gone = false;
                    let lingering_old = w.job != Job::Absent && w.stale;
                    if w.resp == Resp::RefusedAlways || lingering_old {
                        w.job = Job::Refused;
                        w.stale = true;
                        w.start_in = None;
                    } else if w.job != Job::Running {
                        w.job = Job::NotRunning;
                        w.start_in = w.resp.delay();
                    }
                    let look = Self::look(&s.w);
                    if fault == Fault::RegNotAsWritten {
                        Err(RegisterError::NotAsWritten(look))
                    } else {
                        Ok(look)
                    }
                }
            }
        };
        self.leave(k);
        result
    }
    fn unregister(&mut self) -> Result<Inspection, UnregisterError> {
        let k = self.enter("unregister");
        let result = {
            let mut s = lk(&self.s);
            if s.fault == Fault::SlowOs {
                advance(&mut s.w, 130);
            }
            if s.fault == Fault::UnregFails {
                Err(UnregisterError::Failed("the tool failed".into()))
            } else {
                if matches!(s.w.reg, Reg::Registered | Reg::NeedsApproval) {
                    s.w.reg = Reg::Absent;
                }
                Ok(Self::look(&s.w))
            }
        };
        self.leave(k);
        result
    }
    fn evict(&mut self) -> Result<(), String> {
        let k = self.enter("evict");
        {
            let mut s = lk(&self.s);
            if s.fault == Fault::RemovalTimeout {
                if s.w.job != Job::Absent {
                    s.w.linger = 25;
                }
            } else if s.w.resp != Resp::RefusedAlways {
                s.w.linger = 0;
                s.w.job = Job::Absent;
                s.w.stale = false;
                s.w.start_in = None;
            }
        }
        self.leave(k);
        Ok(())
    }
    fn job_gone(&mut self) -> bool {
        lk(&self.s).w.job == Job::Absent
    }
    fn pause(&mut self, how_long: Duration) {
        advance(
            &mut lk(&self.s).w,
            u32::try_from(how_long.as_secs()).unwrap_or(u32::MAX).max(1),
        );
    }
    fn open_login_items(&mut self) -> bool {
        lk(&self.s).prompts += 1;
        true
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// One edge: an event applied to a world

/// A violated invariant: its kind (the name in [`KNOWN_FAILING`]) and the details of the run that showed it.
type Viol = (&'static str, String);

struct Edge {
    after: World,
    crashed: bool,
    ops: Vec<Op>,
    trace: Vec<String>,
    viols: Vec<Viol>,
}

fn normalize(w: &mut World) {
    if let State::Attempting { attempts, .. } = &mut w.rec.state {
        *attempts = (*attempts).min(MAX_ATTEMPTS);
    }
    if w.rec.resume_pending.is_some() {
        w.rec.resume_pending = Some(1);
    }
}

fn build(sim: &Arc<Mutex<Sim>>, rec: RecordFile, timings: Option<Timings>) -> Manager {
    let version = lk(sim).w.cur.s();
    let (a, b, c, d) = (sim.clone(), sim.clone(), sim.clone(), sim.clone());
    let backend = SimBackend { s: sim.clone() };
    let mut m = Manager::new(
        "Model",
        Box::new(backend),
        std::path::Path::new("/nonexistent/model"),
        LABEL,
        version,
        move || lk(&a).w.job == Job::Running,
    )
    .with_record(rec)
    .with_app_state(
        move || lk(&b).w.wanted,
        move || lk(&c).w.first_run_done,
        move || lk(&d).w.location_ok,
    );
    if let Some(t) = timings {
        m = m.with_timings(t);
    }
    m
}

fn new_sim(w: &World, fault: Fault, intr: Intr, rec: &RecordFile) -> Arc<Mutex<Sim>> {
    let mem = rec
        .memory_handle()
        .expect("the model uses the memory store");
    let cancel = Arc::new(AtomicBool::new(false));
    if intr == Intr::Cancel(0) {
        cancel.store(true, Ordering::SeqCst);
    }
    mem.fail_writes(w.ro || intr == Intr::WritesFail(0));
    mem.set_damaged(w.rec_damaged);
    Arc::new(Mutex::new(Sim {
        w: w.clone(),
        fault,
        intr,
        tick: 0,
        ops: Vec::new(),
        trace: Vec::new(),
        cancel,
        mem,
        registers: 0,
        prompts: 0,
        reg_while_off: 0,
    }))
}

/// What the entry point returned, for the invariants.
#[derive(Debug)]
#[allow(dead_code)] // the payloads are read in failure output
enum Out {
    Started(Started),
    Install(Result<RepairOutcome, ServiceError>),
    Uninstall(Result<(), ServiceError>),
    Resume(Result<bool, ServiceError>),
}

fn job_after_enable(w: &mut World) {
    w.job = if w.resp == Resp::RefusedAlways {
        Job::Refused
    } else {
        Job::NotRunning
    };
    w.stale = w.resp == Resp::RefusedAlways;
    w.start_in = w.resp.delay();
}

/// An event of the OS or the user: nothing of the app runs.
fn environment(w: &mut World, ev: Ev) {
    match ev {
        Ev::Reboot => {
            w.linger = 0;
            if w.reg == Reg::Registered {
                if w.stale || w.resp == Resp::RefusedAlways {
                    w.job = Job::Refused;
                    w.stale = true;
                    w.start_in = None;
                } else {
                    w.job = Job::NotRunning;
                    w.start_in = w.resp.delay();
                }
            } else {
                w.job = Job::Absent;
                w.stale = false;
                w.start_in = None;
            }
        }
        Ev::Wait => {
            advance(w, 1000);
            if matches!(w.job, Job::Failed(_) | Job::Unknown) {
                if w.reg == Reg::Registered && !w.stale {
                    w.job = Job::Running;
                } else {
                    w.job = Job::NotRunning;
                }
            }
        }
        Ev::ToggleLoginItem => match w.reg {
            Reg::Registered => {
                w.reg = Reg::SwitchedOff;
                w.job = Job::Absent;
                w.stale = false;
                w.start_in = None;
            }
            Reg::SwitchedOff | Reg::NeedsApproval => {
                w.reg = Reg::Registered;
                job_after_enable(w);
            }
            Reg::Absent | Reg::Unavailable => {}
        },
        Ev::ToggleMark => {
            if w.caps == Caps::Mac {
                w.mark_unreadable = !w.mark_unreadable;
            }
        }
        Ev::JobCrash => {
            if w.job == Job::Running {
                w.job = Job::Failed(LastExit::Code(1));
            }
        }
        Ev::JobUnknown => {
            if matches!(w.job, Job::Running | Job::NotRunning) {
                w.job = Job::Unknown;
            }
        }
        Ev::JobRefuse => {
            if w.reg == Reg::Registered
                && matches!(w.job, Job::Running | Job::NotRunning | Job::Failed(_))
            {
                w.job = Job::Refused;
                w.stale = true;
                w.start_in = None;
            }
        }
        Ev::StopRequested => {
            if w.job != Job::Absent {
                w.job = Job::Absent;
                w.stale = false;
                w.start_in = None;
                w.linger = 0;
                w.rec.resume_pending = Some(1);
            }
        }
        Ev::SettingsOn => w.wanted = Some(true),
        Ev::SettingsOff => w.wanted = Some(false),
        Ev::SettingsDamaged => w.wanted = None,
        Ev::DamageRecord => {
            w.rec = Recorded::default();
            w.rec_damaged = true;
        }
        Ev::NewerRecord => {
            w.rec = Recorded::default();
            w.rec_damaged = true;
            w.ro = true;
        }
        Ev::CopyNewer | Ev::CopyOlder => {
            let v = if ev == Ev::CopyNewer {
                Ver::V3
            } else {
                Ver::V0
            };
            w.reg = Reg::Registered;
            w.owner = v;
            w.other_copy = true;
            w.program_gone = false;
            w.job = Job::Running;
            w.stale = false;
            w.start_in = None;
            w.rec.registered_version = Some(v.s().to_string());
            w.rec.state = State::Idle;
        }
        Ev::ProgramGone => {
            if w.reg == Reg::Registered {
                w.program_gone = true;
            }
        }
        Ev::Relocate => w.location_ok = !w.location_ok,
        _ => {}
    }
    normalize(w);
}

fn describe_panic(p: &(dyn std::any::Any + Send)) -> Option<String> {
    if p.downcast_ref::<Crash>().is_some() {
        return None;
    }
    Some(
        p.downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| p.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a panic".into()),
    )
}

/// Applies one event (with a fault and an interruption for an entry point) to `w0` and checks the edge invariants.
fn apply(w0: &World, ev: Ev, fault: Fault, intr: Intr) -> Edge {
    let mut w = w0.clone();
    if is_rare(ev) {
        w.rare_used = true;
    }
    if !is_entry(ev) {
        environment(&mut w, ev);
        return Edge {
            after: w,
            crashed: false,
            ops: Vec::new(),
            trace: Vec::new(),
            viols: Vec::new(),
        };
    }
    match ev {
        Ev::Update => w.cur = Ver::V2,
        Ev::Downgrade => w.cur = Ver::V1,
        Ev::FirstRun | Ev::Install => w.wanted = Some(true),
        Ev::Uninstall => w.wanted = Some(false),
        _ => {}
    }
    let entry = w.clone();
    let rec = RecordFile::memory();
    rec.store(&w.rec)
        .expect("the memory store takes the first write");
    let sim = new_sim(&w, fault, intr, &rec);
    let other_holder = if fault == Fault::LockHeld {
        rec.lock()
    } else {
        None
    };
    let cancel = lk(&sim).cancel.clone();
    let no_wait = Timings::default().without_start_wait();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut m = build(
            &sim,
            rec.reopen(),
            if matches!(ev, Ev::FirstRun | Ev::Install) {
                Some(no_wait)
            } else {
                None
            },
        );
        match ev {
            Ev::Start | Ev::Update | Ev::Downgrade => Out::Started(m.on_start(&cancel)),
            Ev::FirstRun => Out::Install(m.install_unattended(&cancel)),
            Ev::Install | Ev::Repair => Out::Install(m.install(&cancel)),
            Ev::Uninstall => Out::Uninstall(m.uninstall()),
            Ev::Resume => Out::Resume(m.resume(&cancel, &mut |_| {})),
            _ => unreachable!("not an entry point"),
        }
    }));
    drop(other_holder);
    let mut viols: Vec<Viol> = Vec::new();
    let (crashed, out) = match result {
        Ok(o) => (false, Some(o)),
        Err(p) => {
            if let Some(msg) = describe_panic(p.as_ref()) {
                viols.push(("the code under test panicked", msg));
            }
            (true, None)
        }
    };
    let s = lk(&sim);
    let mut after = s.w.clone();
    after.rec = rec.load();
    after.rec_damaged = rec.peek().1;
    if matches!(ev, Ev::FirstRun | Ev::Install) && matches!(out, Some(Out::Install(Ok(_)))) {
        // Dyno writes its first-run marker after a registration that did not fail.
        after.first_run_done = true;
    }
    normalize(&mut after);
    let ops = s.ops.clone();
    let trace = s.trace.clone();
    edge_checks(
        &entry,
        &after,
        ev,
        fault,
        intr,
        &s,
        out.as_ref(),
        crashed,
        &mut viols,
    );
    Edge {
        after,
        crashed,
        ops,
        trace,
        viols,
    }
}

#[allow(clippy::too_many_arguments)]
fn edge_checks(
    entry: &World,
    after: &World,
    ev: Ev,
    fault: Fault,
    intr: Intr,
    s: &Sim,
    out: Option<&Out>,
    crashed: bool,
    v: &mut Vec<Viol>,
) {
    let count = |l: &str| s.ops.iter().filter(|o| o.label == l).count() as u32;
    let (registers, unregisters, evicts) = (count("register"), count("unregister"), count("evict"));
    let unat = unattended(ev);
    let cur = entry.cur.s();
    let mut bad = |kind: &'static str, why: String| {
        v.push((
            kind,
            format!("{why} (event {ev:?}, fault {fault:?}, interruption {intr:?})"),
        ))
    };

    if s.reg_while_off > 0 {
        bad(
            "registered while the OS says the user switched it off",
            String::new(),
        );
    }
    if unat && entry.wanted == Some(false) && registers > 0 {
        bad(
            "registered unattended although the user's switch is off",
            String::new(),
        );
    }
    if unat && entry.wanted.is_none() && registers > 0 {
        bad(
            "registered unattended although the user's switch could not be read",
            String::new(),
        );
    }
    if matches!(ev, Ev::Start | Ev::Update | Ev::Downgrade | Ev::Resume)
        && entry.wanted == Some(true)
        && entry.rec.state == State::Removing
        && (unregisters + evicts) > 0
        // A finished removal forgets the record (what uninstall does last); a replace that stops half-way leaves an attempt.
        && after.rec.state == State::Idle
        && after.rec.registered_version.is_none()
    {
        bad("finished a removal the user took back", String::new());
    }
    if unat && s.prompts > 0 {
        bad("an unasked prompt", format!("{} prompts", s.prompts));
    }
    if unat && entry.caps == Caps::NoAuto && registers + unregisters + evicts > 0 {
        bad(
            "changed the OS unattended where every change needs a prompt",
            String::new(),
        );
    }
    // A service that answers is not removed or registered again, unless the version, the copy or the program changed.
    let answering = entry.reg == Reg::Registered
        && entry.job == Job::Running
        && !entry.stale
        && entry.owner == entry.cur
        && !entry.other_copy
        && !entry.program_gone
        && entry.rec.state == State::Idle
        // A record that does not know the version replaces once by design (ADR 0056 section 1); an unusable one adopts.
        && (entry.rec.registered_version.as_deref() == Some(cur) || entry.rec_damaged);
    if matches!(ev, Ev::Start | Ev::Resume | Ev::FirstRun)
        && answering
        && entry.wanted != Some(false)
        && (unregisters + evicts) > 0
    {
        bad("removed a service that answers", String::new());
    }
    if matches!(ev, Ev::Start | Ev::FirstRun) && answering && registers > 0 {
        bad("registered again a service that answers", String::new());
    }
    // The unattended register budget: what is left of this version's attempts, one free finish, one light resume.
    if unat {
        let finishing = u32::from(entry.rec.phase_of(cur) == Some(Phase::Registering));
        let budget = MAX_ATTEMPTS.saturating_sub(entry.rec.attempts_of(cur))
            + finishing
            + u32::from(ev == Ev::Resume);
        if registers > budget {
            bad(
                "more unattended registers of one version than its attempts allow",
                format!("{registers} registers, budget {budget}"),
            );
        }
    }
    // Never install unasked.
    if matches!(ev, Ev::Start | Ev::Update | Ev::Downgrade | Ev::Resume)
        && registers > 0
        && entry.reg == Reg::Absent
        && !matches!(
            entry.job,
            Job::Running | Job::NotRunning | Job::Refused | Job::Failed(_)
        )
        && !(entry.wanted == Some(true) && entry.first_run_done)
        && !matches!(
            entry.rec.state,
            State::Attempting {
                phase: Phase::RemovingOld { .. } | Phase::Registering | Phase::RegisterFailed(_),
                ..
            }
        )
    {
        bad("installed unasked", String::new());
    }
    // No OS call without a durable write: after the record stopped being writable no register or unregister (they open a
    // step that is written first); the user's own Repair or Install proceeds, loudly.
    if unat {
        let from = if entry.ro {
            Some(0)
        } else if let Intr::WritesFail(k) = intr {
            Some(k)
        } else {
            None
        };
        if let Some(k) = from
            && s.ops
                .iter()
                .any(|o| o.k > k && matches!(o.label, "register" | "unregister"))
        {
            bad(
                "an OS call without a durable record of it",
                format!("writes failed from call {k}"),
            );
        }
    }
    // A cancel before the first call leaves everything as it was.
    if intr == Intr::Cancel(0)
        && matches!(ev, Ev::Install | Ev::Repair | Ev::Start)
        && entry.wanted != Some(false)
        && entry.rec.state != State::Removing
        && registers + unregisters + evicts > 0
        && !crashed
    {
        bad(
            "cancelled before the first call and the OS was still changed",
            String::new(),
        );
    }
    // A state of the user or the OS is not an attempt (N1).
    if !crashed
        && matches!(
            fault,
            Fault::RegNeedsApproval
                | Fault::RegUnavailable
                | Fault::RegNeedsElevation
                | Fault::RegDeclined
        )
        && matches!(
            ev,
            Ev::Start | Ev::Update | Ev::Downgrade | Ev::Install | Ev::Repair
        )
        && registers > 0
        && !matches!(entry.rec.state, State::Removing)
        && after.rec.attempts_of(cur) > entry.rec.attempts_of(cur)
    {
        bad(
            "a refusal of the user or the OS counted as an attempt",
            format!(
                "attempts {} to {}",
                entry.rec.attempts_of(cur),
                after.rec.attempts_of(cur)
            ),
        );
    }
    // The record only says a version is registered when the OS holds that version's registration.
    if entry.rec.registered_version.as_deref() != Some(cur)
        && after.rec.registered_version.as_deref() == Some(cur)
        && !(after.reg == Reg::Registered && after.owner == after.cur)
        // An unusable record on a Mac: the version cannot be told, so the app adopts a service that answers (ADR 0056).
        && !(entry.rec_damaged && entry.caps == Caps::Mac)
    {
        bad(
            "the record says this version is registered but the OS holds another registration",
            format!("reg {:?} owner {:?}", after.reg, after.owner),
        );
    }
    // The marker of a stop on request is not lost.
    if entry.rec.resume_pending.is_some()
        && after.rec.resume_pending.is_none()
        && !matches!(ev, Ev::Resume | Ev::Uninstall)
    {
        bad("the marker of a stop on request was lost", String::new());
    }
    if ev == Ev::Resume
        && entry.rec.resume_pending.is_some()
        && matches!(out, Some(Out::Resume(Ok(true))))
        && after.rec.resume_pending.is_some()
    {
        bad(
            "a resume completed and its marker is still pending",
            String::new(),
        );
    }
    // An uninstall that returns Ok leaves nothing registered; one that fails puts the record back.
    match out {
        Some(Out::Uninstall(Ok(())))
            if !matches!(after.reg, Reg::Absent | Reg::SwitchedOff | Reg::Unavailable) =>
        {
            bad(
                "uninstall returned Ok and the OS still holds the registration",
                format!("{:?}", after.reg),
            );
        }
        Some(Out::Uninstall(Ok(())))
            if after.rec.registered_version.is_some() || after.rec.state != State::Idle =>
        {
            bad(
                "uninstall returned Ok and the record still remembers it",
                format!("{:?}", after.rec),
            );
        }
        Some(Out::Uninstall(Err(ServiceError::Busy))) | None => {}
        Some(Out::Uninstall(Err(_)))
            if after.reg == Reg::Registered && after.rec.state != entry.rec.state =>
        {
            bad(
                "a failed uninstall did not put the record back",
                format!("{:?} to {:?}", entry.rec.state, after.rec.state),
            );
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Probes and state invariants

/// The same world with an OS that registers at once.
fn cooperative(w: &World) -> World {
    let mut c = w.clone();
    c.resp = Resp::Fast;
    c
}

fn settled(w: &World) -> bool {
    w.reg == Reg::Registered && w.job == Job::Running
}

fn run_chain(w: &World, evs: &[Ev]) -> World {
    let mut c = w.clone();
    for e in evs {
        c = apply(&c, *e, Fault::None, Intr::None).after;
    }
    c
}

/// What the app would see of a state: the plan of a start, `pending()` and `supervises()`.
fn observe(w: &World) -> (Plan, bool, bool) {
    let rec = RecordFile::memory();
    rec.store(&w.rec)
        .expect("the memory store takes the first write");
    let sim = new_sim(w, Fault::None, Intr::None, &rec);
    let mut m = build(&sim, rec, None);
    let plan = m.check();
    (plan, m.pending(), m.supervises())
}

fn state_checks(w: &World) -> Vec<Viol> {
    let mut v: Vec<Viol> = Vec::new();
    let (plan, pending, supervises) = observe(w);
    let hold = matches!(
        plan,
        Plan::Leave(Hold::Unanswered(_) | Hold::Refused(_) | Hold::RegisterFailed(_))
    );
    let answers = w.job == Job::Running;
    if hold {
        let c = cooperative(w);
        let repair = settled(&run_chain(&c, &[Ev::Repair]));
        let update = w.cur != Ver::V2
            && settled(&run_chain(
                &c,
                &[Ev::Update, Ev::Wait, Ev::Start, Ev::Wait, Ev::Start],
            ));
        if !(answers || repair || update) {
            v.push((
                "a hold with no way out",
                format!("plan {plan:?}; Repair {repair}, update {update}"),
            ));
        }
        if supervises {
            v.push(("a held job is supervised", format!("plan {plan:?}")));
        }
    }
    // The service is not lost: two starts of a cooperative OS leave it registered when the user wants it.
    if w.wanted == Some(true)
        && w.first_run_done
        && w.caps != Caps::NoAuto
        && w.location_ok
        && !w.ro
        && w.rec.state != State::Removing
        && matches!(w.reg, Reg::Absent | Reg::Registered)
        && !w.program_gone
        && !hold
    {
        let after = run_chain(&cooperative(w), &[Ev::Start, Ev::Wait, Ev::Start, Ev::Wait]);
        if after.reg == Reg::Absent {
            v.push((
                "the user's service was lost",
                format!("nothing is registered after two starts; plan {plan:?}"),
            ));
        }
    }
    // `pending()` is false in a state with no way forward: where a start and a long wait change nothing.
    if pending {
        let round = run_chain(w, &[Ev::Start, Ev::Wait]);
        if key(&round) == key(w) {
            v.push((
                "pending() is true in a state with no way forward",
                format!("plan {plan:?}"),
            ));
        }
    }
    v
}

// ---------------------------------------------------------------------------------------------------------------------
// The graph

fn bucket(s: Option<u32>) -> &'static str {
    match s {
        None => "-",
        Some(0..=120) => "soon",
        Some(_) => "late",
    }
}

fn key(w: &World) -> String {
    format!(
        "{:?}|{:?}|{}|{}|{:?}|{}|{}|{}|{:?}|{:?}|{}|{}|{}|{}|{}|{}|{:?}",
        w.reg,
        w.owner,
        w.other_copy,
        w.program_gone,
        w.job,
        w.stale,
        bucket(w.start_in),
        w.linger > 0,
        w.cur,
        w.wanted,
        w.first_run_done,
        w.location_ok,
        w.ro,
        w.rec_damaged,
        w.mark_unreadable,
        w.rare_used,
        w.rec
    )
}

fn seeds(caps: Caps, resp: Resp) -> Vec<(&'static str, World)> {
    let base = World {
        caps,
        resp,
        reg: Reg::Absent,
        owner: Ver::V1,
        other_copy: false,
        program_gone: false,
        job: Job::Absent,
        stale: false,
        start_in: None,
        linger: 0,
        mark_unreadable: false,
        cur: Ver::V1,
        wanted: Some(true),
        first_run_done: false,
        location_ok: true,
        ro: false,
        rec_damaged: false,
        rare_used: false,
        rec: Recorded::default(),
    };
    let mut healthy = base.clone();
    healthy.reg = Reg::Registered;
    healthy.job = Job::Running;
    healthy.first_run_done = true;
    healthy.rec.registered_version = Some(Ver::V1.s().into());
    let mut old = healthy.clone();
    old.owner = Ver::V0;
    old.job = Job::Refused;
    old.stale = true;
    old.rec.registered_version = Some(Ver::V0.s().into());
    let mut unavailable = base.clone();
    unavailable.reg = Reg::Unavailable;
    vec![
        ("Fresh", base),
        ("Healthy", healthy),
        ("OldRefused", old),
        ("Unavailable", unavailable),
    ]
}

fn token(ev: Ev, fault: Fault, intr: Intr) -> String {
    let mut t = format!("{ev:?}");
    if fault != Fault::None {
        t += &format!("+{fault:?}");
    }
    if intr != Intr::None {
        t += &format!("@{intr:?}");
    }
    t
}

fn parse_token(t: &str) -> (Ev, Fault, Intr) {
    let (head, intr) = t.split_once('@').unwrap_or((t, ""));
    let (ev, fault) = head.split_once('+').unwrap_or((head, ""));
    let ev = *ALL_EVS
        .iter()
        .find(|e| format!("{e:?}") == ev)
        .unwrap_or_else(|| panic!("no event {ev}"));
    let fault = if fault.is_empty() {
        Fault::None
    } else {
        *FAULTS
            .iter()
            .find(|f| format!("{f:?}") == fault)
            .unwrap_or_else(|| panic!("no fault {fault}"))
    };
    let num = |p: &str| {
        intr[p.len()..]
            .trim_matches(|c| c == '(' || c == ')')
            .parse::<u32>()
            .unwrap()
    };
    let intr = if intr.is_empty() {
        Intr::None
    } else if intr.starts_with("CrashBefore") {
        Intr::CrashBefore(num("CrashBefore"))
    } else if intr.starts_with("CrashAfter") {
        Intr::CrashAfter(num("CrashAfter"))
    } else if intr.starts_with("Cancel") {
        Intr::Cancel(num("Cancel"))
    } else if intr.starts_with("WritesFail") {
        Intr::WritesFail(num("WritesFail"))
    } else {
        panic!("no interruption {intr}")
    };
    (ev, fault, intr)
}

/// The interruptions worth trying for an entry point whose fault-free run made these calls.
fn interruptions(ops: &[Op]) -> Vec<Intr> {
    let mut v = vec![Intr::Cancel(0), Intr::WritesFail(0)];
    let muts: Vec<&Op> = ops.iter().filter(|o| mutating(o.label)).collect();
    for o in &muts {
        v.extend([Intr::CrashBefore(o.k), Intr::CrashAfter(o.k)]);
    }
    // A cancel or a failing write between steps: after the first and after the last mutating call.
    for o in muts.first().into_iter().chain(muts.last()) {
        v.extend([Intr::Cancel(o.k), Intr::WritesFail(o.k)]);
    }
    v.dedup();
    v
}

#[derive(Default)]
struct Found {
    count: u64,
    /// The replay of the shortest path that showed it first, and the details.
    first: Option<(String, String)>,
}

#[derive(Default)]
struct Explored {
    states: usize,
    /// States before any rare event: the closure of the core alphabet.
    core: usize,
    edges: u64,
    crash_edges: u64,
    by_event: BTreeMap<String, u64>,
    by_fault: BTreeMap<String, u64>,
    by_intr: BTreeMap<&'static str, u64>,
    kinds: BTreeMap<&'static str, Found>,
    finished: bool,
}

struct Node {
    w: World,
    parent: Option<(usize, String)>,
    /// Steps from the seed along the shortest path.
    depth: u32,
    /// Steps since the path took its one rare event (0 before it).
    since_rare: u32,
}

/// A rare event is taken from core states up to this depth, and a path goes on this many steps after it (default run).
const RARE_FROM_DEPTH: u32 = 4;
const RARE_STEPS: u32 = 3;

fn path_of(nodes: &[Node], mut i: usize) -> Vec<String> {
    let mut v = Vec::new();
    while let Some((p, t)) = &nodes[i].parent {
        v.push(t.clone());
        i = *p;
    }
    v.reverse();
    v
}

fn explore_config(caps: Caps, resp: Resp) -> Explored {
    let mut ex = Explored::default();
    let mut nodes: Vec<Node> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut queue: VecDeque<usize> = VecDeque::new();
    let mut roots: Vec<&'static str> = Vec::new();
    for (name, mut w) in seeds(caps, resp) {
        normalize(&mut w);
        if seen.insert(key(&w), nodes.len()).is_none() {
            queue.push_back(nodes.len());
            nodes.push(Node {
                w,
                parent: None,
                depth: 0,
                since_rare: 0,
            });
            roots.push(name);
        }
    }
    let n_roots = roots.len();
    // The seed a node descends from is the first of its path; a replay starts from it by name.
    let root_of = |nodes: &[Node], mut i: usize| {
        while let Some((p, _)) = &nodes[i].parent {
            i = *p;
        }
        roots[i.min(n_roots - 1)]
    };
    let record =
        |ex: &mut Explored, nodes: &[Node], at: usize, tail: Option<&str>, viols: Vec<Viol>| {
            for (kind, why) in viols {
                let f = ex.kinds.entry(kind).or_default();
                f.count += 1;
                if f.first.is_none() {
                    let mut p = path_of(nodes, at);
                    if let Some(t) = tail {
                        p.push(t.to_string());
                    }
                    f.first = Some((
                        format!("{caps:?}/{resp:?}/{}|{}", root_of(nodes, at), p.join(";")),
                        why,
                    ));
                }
            }
        };
    ex.finished = true;
    let mut popped = 0u64;
    while let Some(i) = queue.pop_front() {
        popped += 1;
        if popped.is_multiple_of(500) && std::env::var("MODEL_PROGRESS").is_ok() {
            eprintln!(
                "{caps:?}/{resp:?}: {popped} done, {} queued, {} states, {} edges",
                queue.len(),
                nodes.len(),
                ex.edges
            );
        }
        let w = nodes[i].w.clone();
        let sv = state_checks(&w);
        record(&mut ex, &nodes, i, None, sv);
        if !deep() && nodes[i].since_rare >= RARE_STEPS {
            continue;
        }
        for &ev in &ALL_EVS {
            if (is_rare(ev) && (w.rare_used || nodes[i].depth > RARE_FROM_DEPTH))
                || (ev == Ev::FirstRun && w.first_run_done)
            {
                continue;
            }
            let mut tries: Vec<(Fault, Intr)> = vec![(Fault::None, Intr::None)];
            // After a rare event only the plain edges are taken (and the interruptions of a start): the faults and the
            // interruptions of every entry point are covered from the core states.
            let light = w.rare_used && !deep();
            let mut idx = 0;
            while idx < tries.len() {
                let (fault, intr) = tries[idx];
                let first = idx == 0;
                idx += 1;
                let e = apply(&w, ev, fault, intr);
                ex.edges += 1;
                if e.crashed {
                    ex.crash_edges += 1;
                }
                *ex.by_event.entry(format!("{ev:?}")).or_default() += 1;
                if fault != Fault::None {
                    *ex.by_fault.entry(format!("{fault:?}")).or_default() += 1;
                }
                let kind = match intr {
                    Intr::None => None,
                    Intr::CrashBefore(_) => Some("CrashBefore"),
                    Intr::CrashAfter(_) => Some("CrashAfter"),
                    Intr::Cancel(_) => Some("Cancel"),
                    Intr::WritesFail(_) => Some("WritesFail"),
                };
                if let Some(k) = kind {
                    *ex.by_intr.entry(k).or_default() += 1;
                }
                let t = token(ev, fault, intr);
                if !e.viols.is_empty() {
                    record(&mut ex, &nodes, i, Some(&t), e.viols);
                }
                if first && is_entry(ev) {
                    let has = |l: &str| e.ops.iter().any(|o| o.label == l);
                    for f in FAULTS.iter().copied().filter(|_| !light) {
                        let applies = match f {
                            f if is_reg_fault(f) => has("register") && ev != Ev::Uninstall,
                            Fault::UnregFails => has("unregister"),
                            Fault::RemovalTimeout => has("evict"),
                            Fault::SlowOs => has("register") || has("unregister"),
                            Fault::LockHeld => e.ops.iter().any(|o| mutating(o.label)),
                            _ => false,
                        };
                        let caps_ok = match f {
                            Fault::RegNeedsElevation | Fault::RegDeclined => caps == Caps::NoAuto,
                            Fault::RegNeedsApproval => caps != Caps::NoAuto,
                            _ => true,
                        };
                        if applies && caps_ok {
                            tries.push((f, Intr::None));
                        }
                    }
                    if (!light || ev == Ev::Start) && !(ev == Ev::Repair && !deep()) {
                        for it in interruptions(&e.ops) {
                            tries.push((Fault::None, it));
                        }
                    }
                } else if deep()
                    && intr == Intr::None
                    && matches!(
                        fault,
                        Fault::RegOther | Fault::RemovalTimeout | Fault::RegNotAsWritten
                    )
                {
                    // A crash during a replace that already met a fault.
                    for o in e.ops.iter().filter(|o| mutating(o.label)) {
                        tries.push((fault, Intr::CrashBefore(o.k)));
                        tries.push((fault, Intr::CrashAfter(o.k)));
                    }
                }
                let mut after = e.after;
                normalize(&mut after);
                let k = key(&after);
                if let std::collections::hash_map::Entry::Vacant(slot) = seen.entry(k) {
                    if nodes.len() >= STATES_CEILING {
                        ex.finished = false;
                        continue;
                    }
                    slot.insert(nodes.len());
                    queue.push_back(nodes.len());
                    let since_rare = if !after.rare_used {
                        0
                    } else {
                        nodes[i].since_rare + 1
                    };
                    let depth = nodes[i].depth + 1;
                    nodes.push(Node {
                        w: after,
                        parent: Some((i, t)),
                        depth,
                        since_rare,
                    });
                }
            }
        }
    }
    ex.states = nodes.len();
    ex.core = nodes.iter().filter(|n| !n.w.rare_used).count();
    ex
}

const CONFIGS: [(Caps, Resp); 8] = [
    (Caps::Mac, Resp::Fast),
    (Caps::Mac, Resp::Never),
    (Caps::Mac, Resp::RefusedAlways),
    (Caps::Linux, Resp::Never),
    (Caps::NoAuto, Resp::Fast),
    // The long run adds the slow starts and the other mechanism with a fast OS.
    (Caps::Mac, Resp::Slow),
    (Caps::Mac, Resp::Late),
    (Caps::Linux, Resp::Fast),
];

/// The configurations of the default run (the first five) or the long one (all).
fn configs() -> &'static [(Caps, Resp)] {
    if deep() { &CONFIGS } else { &CONFIGS[..5] }
}

/// Bugs the model found in the code under test. Each entry fails the test when it is **not** found any more (it was fixed:
/// delete the line), and anything not listed fails the test when found. The text says where it points.
const KNOWN_FAILING: &[(&str, &str)] = &[];

#[test]
fn the_crash_model_reaches_a_fixed_point_and_the_invariants_hold_in_every_state() {
    silence_crashes();
    let started = Instant::now();
    let only = std::env::var("MODEL_CONFIG").ok();
    let configs: Vec<(Caps, Resp)> = configs()
        .iter()
        .copied()
        .filter(|(c, r)| only.as_deref().is_none_or(|o| o == format!("{c:?}/{r:?}")))
        .collect();
    let results: Vec<(Caps, Resp, Explored)> = std::thread::scope(|sc| {
        let handles: Vec<_> = configs
            .iter()
            .map(|&(c, r)| sc.spawn(move || (c, r, explore_config(c, r))))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("an exploration thread panicked"))
            .collect()
    });
    let mut states = 0;
    let mut edges = 0;
    let mut crash_edges = 0;
    let mut by_event: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_fault: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_intr: BTreeMap<&str, u64> = BTreeMap::new();
    let mut kinds: BTreeMap<&str, (u64, String, String)> = BTreeMap::new();
    let mut out = String::new();
    for (c, r, ex) in &results {
        out += &format!(
            "{c:?}/{r:?}: {} states ({} core), {} edges ({} crashes){}\n",
            ex.states,
            ex.core,
            ex.edges,
            ex.crash_edges,
            if ex.finished { "" } else { "  NOT FINISHED" }
        );
        assert!(
            ex.finished,
            "{c:?}/{r:?}: the state ceiling was reached, so the key misses a bucket\n{out}"
        );
        states += ex.states;
        edges += ex.edges;
        crash_edges += ex.crash_edges;
        for (k, n) in &ex.by_event {
            *by_event.entry(k.clone()).or_default() += n;
        }
        for (k, n) in &ex.by_fault {
            *by_fault.entry(k.clone()).or_default() += n;
        }
        for (k, n) in &ex.by_intr {
            *by_intr.entry(k).or_default() += n;
        }
        for (k, f) in &ex.kinds {
            let e = kinds
                .entry(k)
                .or_insert_with(|| (0, String::new(), String::new()));
            e.0 += f.count;
            if e.1.is_empty()
                && let Some((replay, why)) = &f.first
            {
                e.1 = replay.clone();
                e.2 = why.clone();
            }
        }
    }
    out += &format!(
        "total: {states} states, {edges} edges ({crash_edges} crash edges) in {:?}\nedges by event {by_event:?}\nedges by fault {by_fault:?}\nedges by interruption {by_intr:?}\n",
        started.elapsed()
    );
    for (k, (n, replay, why)) in &kinds {
        out += &format!("- {k}: {n} edges or states\n    MODEL_REPLAY='{replay}'\n    {why}\n");
    }
    println!("{out}");
    // The alphabet is not silently trimmed: every event, fault and interruption was taken, and the floors hold.
    for e in ALL_EVS {
        assert!(
            by_event.get(&format!("{e:?}")).copied().unwrap_or(0) > 0,
            "event {e:?} was never taken\n{out}"
        );
    }
    for f in FAULTS {
        assert!(
            by_fault.get(&format!("{f:?}")).copied().unwrap_or(0) > 0,
            "fault {f:?} was never injected\n{out}"
        );
    }
    for i in ["CrashBefore", "CrashAfter", "Cancel", "WritesFail"] {
        assert!(
            by_intr.get(i).copied().unwrap_or(0) > 0,
            "interruption {i} was never taken\n{out}"
        );
    }
    assert!(
        states >= STATES_FLOOR,
        "only {states} states (floor {STATES_FLOOR})\n{out}"
    );
    assert!(
        edges >= EDGES_FLOOR,
        "only {edges} edges (floor {EDGES_FLOOR})\n{out}"
    );
    let known: Vec<&str> = KNOWN_FAILING.iter().map(|(k, _)| *k).collect();
    let new: Vec<&&str> = kinds.keys().filter(|k| !known.contains(k)).collect();
    let fixed: Vec<&&str> = known.iter().filter(|k| !kinds.contains_key(*k)).collect();
    assert!(new.is_empty(), "new violations {new:?}\n{out}");
    assert!(
        fixed.is_empty(),
        "known-failing entries that no longer fail (delete them): {fixed:?}\n{out}"
    );
}

/// Replays one path printed by a failure: `MODEL_REPLAY='Mac/Fast/Healthy|Start;Install+RegOther@CrashAfter(2)'` prints the
/// OS calls of every step, the record and the state invariants of the end.
#[test]
fn replay_one_path_from_the_environment() {
    let Ok(spec) = std::env::var("MODEL_REPLAY") else {
        return;
    };
    silence_crashes();
    let (cfg, path) = spec
        .split_once('|')
        .expect("MODEL_REPLAY is config|step;step");
    let parts: Vec<&str> = cfg.split('/').collect();
    let (caps, resp) = CONFIGS
        .iter()
        .copied()
        .find(|(c, r)| format!("{c:?}") == parts[0] && format!("{r:?}") == parts[1])
        .expect("unknown config");
    let mut w = seeds(caps, resp)
        .into_iter()
        .find(|(n, _)| *n == parts[2])
        .expect("unknown seed")
        .1;
    normalize(&mut w);
    println!("start {w:#?}");
    for t in path.split(';').filter(|t| !t.is_empty()) {
        let (ev, fault, intr) = parse_token(t);
        let e = apply(&w, ev, fault, intr);
        println!("{t} -> crashed {}", e.crashed);
        for l in &e.trace {
            println!("{l}");
        }
        for (k, why) in &e.viols {
            println!("  VIOLATION {k}: {why}");
        }
        w = e.after;
        normalize(&mut w);
        println!(
            "  now reg {:?} owner {:?} job {:?} start_in {:?}\n  record {:?}",
            w.reg, w.owner, w.job, w.start_in, w.rec
        );
    }
    let (plan, pending, supervises) = observe(&w);
    println!("plan {plan:?} pending {pending} supervises {supervises}");
    for (k, why) in state_checks(&w) {
        println!("  STATE VIOLATION {k}: {why}");
    }
    let e = apply(&cooperative(&w), Ev::Repair, Fault::None, Intr::None);
    println!("probe: Repair on a cooperative OS");
    for l in &e.trace {
        println!("{l}");
    }
    println!(
        "  now reg {:?} job {:?} record {:?}",
        e.after.reg, e.after.job, e.after.rec
    );
}

// ---------------------------------------------------------------------------------------------------------------------
// The lock, with a real process

const LOCK_CHILD: &str = "MODEL_LOCK_CHILD";

/// Runs in the child: the test binary again, with the lock file in the environment. It takes the lock, says so on stdout and
/// holds it until it is killed (or, as a safety net for a failed run, two minutes).
#[test]
fn lock_holder_child() {
    use paddock_proc::{Claim, InstanceLock};
    use std::io::Write;
    let Ok(path) = std::env::var(LOCK_CHILD) else {
        return;
    };
    let lock = match InstanceLock::claim(std::path::Path::new(&path)) {
        Ok(Claim::Acquired(l)) => l,
        other => panic!("the child could not take the lock: {:?}", other.map(|_| ())),
    };
    println!("LOCK-HELD");
    std::io::stdout().flush().expect("stdout");
    std::thread::sleep(Duration::from_secs(120));
    drop(lock);
}

/// A process that holds the repair lock and is killed frees it: the OS drops the lock with its holder (rule 53), so there is
/// no stale lock and nothing to take over by age. The test waits on the child's own line and on its exit, never on a sleep.
#[test]
fn a_lock_held_by_a_killed_process_is_free_again() {
    use paddock_proc::{Claim, InstanceLock};
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    if std::env::var(LOCK_CHILD).is_ok() {
        return;
    }
    /// Kills and reaps the child whatever the test does, so no process is left behind.
    struct Reap(std::process::Child);
    impl Drop for Reap {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let record = RecordFile::new(dir.path(), LABEL);
    // The lock file of the record, as the record names it.
    let path = dir.path().join(format!("service-{LABEL}.lock"));
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "model::lock_holder_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(LOCK_CHILD, &path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut child = Reap(child);
    let held = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .any(|l| l.contains("LOCK-HELD"));
    assert!(held, "the child ended without taking the lock");
    assert!(
        matches!(InstanceLock::claim(&path), Ok(Claim::Held)),
        "a lock held by a live process must read as held"
    );
    assert!(
        record.lock().is_none(),
        "the repair lock of the record must be busy while the other process lives"
    );
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    match InstanceLock::claim(&path) {
        Ok(Claim::Acquired(_)) => {}
        other => panic!(
            "the lock of a killed process must be free, got {:?}",
            other.map(|_| ())
        ),
    }
    assert!(
        record.lock().is_some(),
        "the repair lock must be free after its holder was killed"
    );
}
