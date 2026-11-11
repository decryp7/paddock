//! The seam every agent-side reading the helper can give goes through (Dyno helper-wiring plan section 3, rule 55: one
//! implementation). The policy is the user's of 2026-11-04 (ADR 0027 amended, decision D1): **helper first, then this computer,
//! else an honest reason with the next step**.
//!
//! A reading is a [`HelperReading`]: how to ask the helper ([`HelperReading::via_helper`], a typed [`HelperCalls`] method), what
//! the agent can read itself ([`HelperReading::local`]) and whether it is [`ReadClass::Live`] or [`ReadClass::Bulk`].
//! [`HelperFirst`] runs it: it asks the [`HelperPort`] how the helper stands ([`Availability`], a cached answer: readiness costs
//! two IPC calls and a reader that runs every tick must not pay them every time, and a helper that hangs must not make every
//! reading wait), calls the helper when it is ready, falls back when it is not or when it errors, and says which path answered
//! ([`Sourced`]).
//!
//! Adding a reading: implement [`HelperReading`] (about 20 lines), build `HelperFirst::new(reading, port)` where the agent is
//! composed (`agent.rs`), and put `Sourced::via` on the reply type's `via` field. Nothing else in this file changes. Say what the
//! reading asks in [`HelperReading::KINDS`]: the seam registers it, and `tests/helper_wiring.rs` compares it with `readings()`.

use dyno_core::btm::LoginItemsReply;
use dyno_core::firewall::FirewallSource;
use dyno_core::helper_counters::CounterAnswer;
use dyno_core::helper_extras::ExtrasAnswer;
use dyno_core::helper_holders::{FileHolders, SocketOwners};
use dyno_core::helper_policy::RequestKind;
use dyno_core::helper_processes::ProcessPage;
use dyno_core::helper_requests::{CounterQuery, ExtrasKind, ProcessQuery, SmartQuery};
use dyno_core::helper_wire::{HelperState, HelperStatus};
use dyno_core::inventory::MemoryLayout;
use dyno_core::metrics::Reading;
use dyno_core::network_traffic::TrafficAnswer;
use dyno_core::read_via::{sentence, NextStep, ReadVia, NEEDS_ADMIN};
use dyno_core::smart::SmartReply;
use dyno_core::wake::{IfName, WolReply};
use paddock_jobs::time::Clock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

pub use crate::helper_link::LIVE_WAIT;
use crate::helper_link::{CallError, HelperLink, HelperTransport, Wait};

/// How long an answer about the helper is kept (plan section 3.1).
pub const READY_TTL: Duration = Duration::from_secs(5);
/// The longest an unreachable helper is left alone: after a hang or a foreign program on the endpoint the kept answer grows
/// 5, 10, 20, 40 s and stops at this, so a hung helper costs one short wait a minute, not one in every read.
pub const BACKOFF_CAP: Duration = Duration::from_secs(60);

/// How a reading is used, which decides how long it may wait for the helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadClass {
    /// A person looks at it and it refreshes every few seconds: the helper gets [`LIVE_WAIT`].
    Live,
    /// A job or a slow kind with its own progress and Cancel: the request kind's own wait.
    Bulk,
}

impl ReadClass {
    fn wait(self) -> Wait {
        match self {
            ReadClass::Live => Wait::Live,
            ReadClass::Bulk => Wait::Default,
        }
    }
}

/// The object-safe slice of the helper link the readings use (interface segregation). One typed method per request kind the
/// agent reads with; [`Bounded`] is the real one.
pub trait HelperCalls: Send + Sync {
    /// One page of the process list, scoped to the caller by the helper.
    fn processes(&self, query: ProcessQuery) -> Result<ProcessPage, CallError>;
    /// Per-program traffic counters (Windows), scoped to the caller.
    fn traffic(&self) -> Result<TrafficAnswer, CallError>;
    /// Socket inode to owning program (Linux).
    fn socket_owners(&self) -> Result<SocketOwners, CallError>;
    /// The programs holding the file with this device and inode (Linux).
    fn file_holders(&self, dev: u64, ino: u64) -> Result<FileHolders, CallError>;
    /// Thermal, energy and fan readings.
    fn counters(&self, query: CounterQuery) -> Result<Vec<CounterAnswer>, CallError>;
    /// Memory modules from the DMI table (Linux).
    fn memory(&self) -> Result<MemoryLayout, CallError>;
    /// Wake requests (Windows).
    fn extras(&self, kind: ExtrasKind) -> Result<ExtrasAnswer, CallError>;
    /// Login items (macOS).
    fn login_items(&self) -> Result<LoginItemsReply, CallError>;
    /// SMART health of the disks the helper's own scan finds.
    fn smart(&self, query: SmartQuery) -> Result<SmartReply, CallError>;
    /// The firewall as the helper reads it (`pf` on macOS, `nft` on Linux), reduced to what the caller may have.
    fn firewall(&self) -> Result<FirewallSource, CallError>;
    /// The last lines of the helper's own log (owner and administrators only), oldest first.
    fn log_tail(&self, lines: u32) -> Result<Vec<String>, CallError>;
    /// The Wake-on-LAN modes of one interface (Linux), modes only.
    fn wol_state(&self, ifname: IfName) -> Result<WolReply, CallError>;
}

/// The helper link with the wait of a [`ReadClass`].
pub struct Bounded<T: HelperTransport> {
    link: Arc<HelperLink<T>>,
    wait: Wait,
}

impl<T: HelperTransport> Bounded<T> {
    /// `link` as a reading of `class` uses it.
    pub fn new(link: Arc<HelperLink<T>>, class: ReadClass) -> Self {
        Bounded { link, wait: class.wait() }
    }
}

impl<T: HelperTransport> HelperCalls for Bounded<T> {
    fn processes(&self, query: ProcessQuery) -> Result<ProcessPage, CallError> {
        self.link.processes_in(query, self.wait)
    }
    fn traffic(&self) -> Result<TrafficAnswer, CallError> {
        self.link.traffic_in(self.wait)
    }
    fn socket_owners(&self) -> Result<SocketOwners, CallError> {
        self.link.socket_owners_in(self.wait)
    }
    fn file_holders(&self, dev: u64, ino: u64) -> Result<FileHolders, CallError> {
        self.link.file_holders_in(dev, ino, self.wait)
    }
    fn counters(&self, query: CounterQuery) -> Result<Vec<CounterAnswer>, CallError> {
        self.link.counters_in(query, self.wait)
    }
    fn memory(&self) -> Result<MemoryLayout, CallError> {
        self.link.memory_in(self.wait)
    }
    fn extras(&self, kind: ExtrasKind) -> Result<ExtrasAnswer, CallError> {
        self.link.extras_in(kind, self.wait)
    }
    fn login_items(&self) -> Result<LoginItemsReply, CallError> {
        self.link.login_items_now_in(self.wait)
    }
    fn smart(&self, query: SmartQuery) -> Result<SmartReply, CallError> {
        self.link.smart_in(query, self.wait)
    }
    fn firewall(&self) -> Result<FirewallSource, CallError> {
        self.link.firewall_in(self.wait)
    }
    fn log_tail(&self, lines: u32) -> Result<Vec<String>, CallError> {
        self.link.log_tail_in(lines, self.wait)
    }
    fn wol_state(&self, ifname: IfName) -> Result<WolReply, CallError> {
        self.link.wol_state_in(ifname, self.wait)
    }
}

/// How the helper stands, as far as a reading is concerned (built from the helper's [`HelperStatus`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    /// Installed, answering and a build this Dyno accepts.
    Ready,
    /// Never turned on: the reading asks for it.
    Off,
    /// Answers, but is older than this Dyno accepts (the sentence says so): update it.
    Outdated(String),
    /// This build or system cannot have a helper (an AppImage, no polkit): nothing to turn on.
    Unsupported(String),
    /// Installed but damaged: Repair in Settings.
    Broken(String),
    /// Another program answers where the helper should be; nothing is sent to it.
    Foreign(String),
    /// Installed but not answering (or too slowly).
    Silent(String),
}

impl Availability {
    /// The standing `status` describes.
    pub fn from_status(status: &HelperStatus) -> Availability {
        let m = || status.message.clone();
        match status.state {
            HelperState::Ready => Availability::Ready,
            HelperState::Off => Availability::Off,
            HelperState::Outdated => Availability::Outdated(m()),
            HelperState::NotIncluded => Availability::Unsupported(m()),
            HelperState::Missing => Availability::Broken(m()),
            HelperState::Foreign => Availability::Foreign(m()),
            HelperState::NotRunning | HelperState::Starting => Availability::Silent(m()),
        }
    }

    /// Whether the helper can be asked.
    pub fn is_ready(&self) -> bool {
        matches!(self, Availability::Ready)
    }

    /// Whether the helper hung or is not what it should be: it is left alone for a growing time.
    fn is_unreachable(&self) -> bool {
        matches!(self, Availability::Silent(_) | Availability::Foreign(_))
    }

    /// Why a reading is not available while the helper is not ready, with `why` (its own reason, may be empty) first.
    fn via(&self, why: &str) -> ReadVia {
        let join = |extra: &str| {
            [sentence(why), sentence(extra)]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        };
        let settings = || Some(NextStep::OpenSettings("helper".into()));
        match self {
            // Ready is not a reason; a caller that gets here has its own `why`.
            Availability::Ready => ReadVia::unavailable(why),
            Availability::Off => ReadVia::needs_admin(why),
            Availability::Outdated(m) => ReadVia::needs_update(&join(m)),
            // No helper can exist here: never "turn it on".
            Availability::Unsupported(m) => ReadVia::unavailable(&join(m)),
            Availability::Broken(m) => ReadVia::Unavailable {
                reason: join(m),
                next: settings(),
            },
            Availability::Foreign(m) => ReadVia::Unavailable {
                reason: join(m),
                next: settings(),
            },
            Availability::Silent(m) => ReadVia::Unavailable {
                reason: join(m),
                next: Some(NextStep::Retry),
            },
        }
    }

    /// `via` with the value-less text a reader without a button shows: the reason, and for `Off` the sentence that says what to
    /// turn on.
    fn without_value<V>(&self, why: &str) -> Sourced<V> {
        let via = self.via(why);
        let mut text = via.reason().unwrap_or_default().to_string();
        if matches!(
            via,
            ReadVia::NeedsHelper {
                next: NextStep::OpenSettings(_),
                ..
            }
        ) {
            text = format!("{text} {NEEDS_ADMIN}");
        }
        Sourced {
            value: Reading::Unavailable(text),
            via,
        }
    }
}

/// Where a reading finds the helper: how it stands (cached), when that last changed, and the calls to make. Implemented over the
/// agent's `Shared.helper` by [`SharedPort`], over a centre by tests, and by [`OffPort`] where there is no helper.
pub trait HelperPort: Send + Sync {
    /// How the helper stands. Cached (5 s; longer while it is unreachable); cheap to call on every tick.
    fn availability(&self) -> Availability;
    /// Moves whenever readiness flips or a job changes the helper: a consumer that keeps a reading re-reads when it changes.
    fn epoch(&self) -> u64;
    /// The calls for a reading of `class`, or `None` when the agent is gone (shutting down).
    fn calls(&self, class: ReadClass) -> Option<Arc<dyn HelperCalls>>;
    /// The clock the strikes are counted on.
    fn now(&self) -> Duration;
}

/// How long something unreachable is left alone after its `strikes`th failure in a row: 5, 10, 20, 40 s, then [`BACKOFF_CAP`].
fn backoff(strikes: u32) -> Duration {
    READY_TTL.saturating_mul(1u32 << strikes.saturating_sub(1).min(10)).min(BACKOFF_CAP)
}

/// A port with no helper: never ready. Readings composed in a test agent that must not touch the machine use it (ER-14).
pub struct OffPort;

impl HelperPort for OffPort {
    fn availability(&self) -> Availability {
        Availability::Off
    }
    fn epoch(&self) -> u64 {
        0
    }
    fn calls(&self, _: ReadClass) -> Option<Arc<dyn HelperCalls>> {
        None
    }
    fn now(&self) -> Duration {
        Duration::ZERO
    }
}

/// A centre as a port.
pub struct CentrePort<T: HelperTransport + 'static>(pub Arc<crate::helper_centre::HelperCentre<T>>);

impl<T: HelperTransport + 'static> HelperPort for CentrePort<T> {
    fn availability(&self) -> Availability {
        self.0.availability()
    }
    fn epoch(&self) -> u64 {
        self.0.epoch()
    }
    fn calls(&self, class: ReadClass) -> Option<Arc<dyn HelperCalls>> {
        Some(Arc::new(Bounded::new(self.0.link_arc(), class)))
    }
    fn now(&self) -> Duration {
        self.0.link().clock().monotonic()
    }
}

/// The helper of a running agent.
pub struct SharedPort(pub std::sync::Weak<crate::agent::Shared>);

impl SharedPort {
    /// The centre, with the agent let go of at once: a probe or a call can take seconds and must not keep the whole agent alive.
    fn centre(&self) -> Option<Arc<crate::helper_centre::HelperCentre<Box<dyn HelperTransport>>>> {
        let shared = self.0.upgrade()?;
        let centre = shared.helper.clone();
        drop(shared);
        Some(centre)
    }
}

impl HelperPort for SharedPort {
    fn availability(&self) -> Availability {
        self.centre().map_or(Availability::Off, |c| c.availability())
    }
    fn epoch(&self) -> u64 {
        self.centre().map_or(0, |c| c.epoch())
    }
    fn calls(&self, class: ReadClass) -> Option<Arc<dyn HelperCalls>> {
        Some(Arc::new(Bounded::new(self.centre()?.link_arc(), class)))
    }
    fn now(&self) -> Duration {
        self.centre().map_or(Duration::ZERO, |c| c.link().clock().monotonic())
    }
}

/// An answer about the helper kept for a while. It is dropped at once when a call finds the helper unreachable (the link counts
/// those as faults) or when [`ReadyGate::forget`] says a job changed the helper, and a probe that was in flight during a
/// `forget` does not store its stale answer (the generation).
pub struct ReadyGate {
    clock: Arc<dyn Clock>,
    epoch: AtomicU64,
    state: Mutex<GateState>,
}

#[derive(Default)]
struct GateState {
    cached: Option<Cached>,
    /// Consecutive unreachable answers.
    strikes: u32,
    /// Moves on every forget.
    generation: u64,
    /// Whether the last stored answer was Ready.
    was_ready: bool,
}

struct Cached {
    at: Duration,
    faults: u64,
    ttl: Duration,
    answer: Availability,
}

impl ReadyGate {
    /// A gate on `clock`.
    pub fn new(clock: Arc<dyn Clock>) -> ReadyGate {
        ReadyGate {
            clock,
            epoch: AtomicU64::new(0),
            state: Mutex::new(GateState::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, GateState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The kept answer when it is fresh and no call found the helper unreachable since (`faults` is the link\'s counter), else
    /// `probe()`\'s, which is then kept.
    pub fn availability(&self, faults: impl Fn() -> u64, probe: impl FnOnce() -> Availability) -> Availability {
        let generation = {
            let state = self.lock();
            if let Some(c) = &state.cached {
                if self.clock.monotonic().saturating_sub(c.at) < c.ttl && c.faults == faults() {
                    return c.answer.clone();
                }
            }
            state.generation
        };
        let answer = probe();
        self.store(generation, answer.clone(), faults());
        answer
    }

    /// Keeps an answer someone else just learned (Settings polls the status).
    pub fn record(&self, answer: Availability, faults: u64) {
        let generation = self.lock().generation;
        self.store(generation, answer, faults);
    }

    fn store(&self, generation: u64, answer: Availability, faults: u64) {
        let mut state = self.lock();
        if state.generation != generation {
            return;
        }
        state.strikes = if answer.is_unreachable() {
            state.strikes.saturating_add(1)
        } else {
            0
        };
        let ttl = if answer.is_unreachable() {
            backoff(state.strikes)
        } else {
            READY_TTL
        };
        if answer.is_ready() != state.was_ready {
            state.was_ready = answer.is_ready();
            self.epoch.fetch_add(1, Ordering::SeqCst);
        }
        state.cached = Some(Cached {
            at: self.clock.monotonic(),
            faults,
            ttl,
            answer,
        });
    }

    /// Drops the kept answer and moves the epoch (the helper was turned on, off, repaired or updated).
    pub fn forget(&self) {
        let mut state = self.lock();
        state.generation += 1;
        state.cached = None;
        state.strikes = 0;
        self.epoch.fetch_add(1, Ordering::SeqCst);
    }

    /// The epoch: moves when readiness flips or the helper is forgotten.
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }
}

/// One reading the helper can give better than the agent.
pub trait HelperReading: Send {
    /// The domain value: the same type from both paths.
    type Value;
    /// The helper request kinds `via_helper` sends, one per typed call it makes. Required, so a reading that does not say what it
    /// asks cannot be written; [`HelperFirst::new`] registers them where the composition wires the reading, and
    /// [`wired_kinds_while`] hands them to the test that compares them with what the helper advertises.
    const KINDS: &'static [RequestKind];
    /// A short name for the log (no user content).
    fn name(&self) -> &'static str;
    /// How long the helper may take: [`ReadClass::Live`] or [`ReadClass::Bulk`]. Every reading says, so none gets a 2 s deadline by
    /// default (SMART takes as long as smartctl takes).
    fn class(&self) -> ReadClass;
    /// Asks the helper, through a typed [`HelperCalls`] method. An answer that holds nothing useful is an `Err` with the reason,
    /// so the agent's own reader gets its turn (Smart: no disk with wear counters).
    ///
    /// # Errors
    /// What the helper refused or could not read, in words.
    fn via_helper(&self, calls: &dyn HelperCalls) -> Result<Self::Value, CallError>;
    /// What the agent can read itself; `Reading::Unavailable(reason)` when nothing.
    fn local(&mut self) -> Reading<Self::Value>;
    /// Whether the helper reads this on this system at all. When it does not (SMART on macOS: the agent runs its own smartctl as the
    /// person), the helper is never asked and no reason sends a person to turn it on: the reason is the local reader's alone.
    fn helper_applies(&self) -> bool {
        true
    }
    /// For a local value that is only part of what the helper would give (Linux memory: the total without the slots), the
    /// sentence saying what is missing. The value is still shown; the reading's source then says what would add the rest.
    fn partial(&self, _local: &Self::Value) -> Option<String> {
        None
    }
}

/// A reading and where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Sourced<V> {
    /// The value, or why there is none.
    pub value: Reading<V>,
    /// Which path answered.
    pub via: ReadVia,
}

thread_local! {
    /// The kinds of the readings built on this thread while a [`wired_kinds_while`] runs; `None` otherwise (production never records).
    static WIRED: std::cell::RefCell<Option<std::collections::HashSet<RequestKind>>> = const { std::cell::RefCell::new(None) };
}

fn note_wired(kinds: &[RequestKind]) {
    WIRED.with(|w| {
        if let Some(set) = w.borrow_mut().as_mut() {
            set.extend(kinds.iter().copied());
        }
    });
}

/// Runs `build` and returns what it wired: the [`HelperReading::KINDS`] of every reading handed to [`HelperFirst::new`] on this
/// thread meanwhile. The registry is the seam itself, so a reading composed without it cannot be missed by a text scan; the
/// composition test (`tests/helper_wiring.rs`) compares the result with the readings the helper advertises. A call inside
/// `build` keeps its own thread's record apart (a nested call restores the outer one).
pub fn wired_kinds_while<T>(build: impl FnOnce() -> T) -> (T, std::collections::HashSet<RequestKind>) {
    let outer = WIRED.with(|w| w.replace(Some(std::collections::HashSet::new())));
    let built = build();
    let wired = WIRED.with(|w| w.replace(outer)).unwrap_or_default();
    (built, wired)
}

/// Runs a [`HelperReading`]: helper first, then local, else the reason.
pub struct HelperFirst<R: HelperReading> {
    reading: R,
    port: Box<dyn HelperPort>,
    fallback: bool,
    /// What the last read came to, so a repeat is logged at debug and a change at warn (one line per change, not per tick).
    last_outcome: Option<String>,
    /// Calls of this reading that found the helper unreachable in a row, and until when it is left alone: one slow kind must not
    /// cost every read a deadline while the rest of the helper answers.
    strikes: u32,
    quiet_until: Option<Duration>,
}

impl<R: HelperReading> HelperFirst<R> {
    /// The reading over `port`, with the local fallback on (the product choice, plan decision D1).
    pub fn new(reading: R, port: Box<dyn HelperPort>) -> Self {
        note_wired(R::KINDS);
        HelperFirst {
            reading,
            port,
            fallback: true,
            last_outcome: None,
            strikes: 0,
            quiet_until: None,
        }
    }

    /// Chooses whether the local fallback is used (tests, and the one switch for the product choice).
    pub fn with_fallback(mut self, on: bool) -> Self {
        self.fallback = on;
        self
    }

    /// The reading itself (for its own settings).
    pub fn reading(&self) -> &R {
        &self.reading
    }

    /// The reading itself, to give it the inputs of the next read.
    pub fn reading_mut(&mut self) -> &mut R {
        &mut self.reading
    }

    /// Ends the quiet window of this reading (a person asked to try again), so the next read asks the helper at once.
    pub fn retry_now(&mut self) {
        self.strikes = 0;
        self.quiet_until = None;
    }

    /// The helper epoch, brought up to date: a consumer that keeps this reading re-reads when it differs from the one it kept.
    pub fn epoch(&self) -> u64 {
        // Asking is what notices a flip: the answer is cached, so this is cheap.
        let _ = self.port.availability();
        self.port.epoch()
    }

    fn note(&mut self, outcome: &str, bad: bool, started: Instant) {
        let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let name = self.reading.name();
        if self.last_outcome.as_deref() == Some(outcome) {
            log::debug!(target: "dyno::helper", Ms = ms; "{name}: {outcome}");
            return;
        }
        self.last_outcome = Some(outcome.to_string());
        if bad {
            log::warn!(target: "dyno::helper", Ms = ms; "{name}: {outcome}");
        } else {
            log::info!(target: "dyno::helper", Ms = ms; "{name}: {outcome}");
        }
    }

    /// Reads once.
    pub fn read(&mut self) -> Sourced<R::Value> {
        let started = Instant::now();
        if !self.reading.helper_applies() {
            let out = match self.reading.local() {
                Reading::Available(v) => Sourced {
                    value: Reading::Available(v),
                    via: ReadVia::Local,
                },
                Reading::Unavailable(why) => {
                    let via = ReadVia::retry(&why);
                    Sourced {
                        value: Reading::Unavailable(via.reason().unwrap_or_default().to_string()),
                        via,
                    }
                }
            };
            self.note(
                "read on this computer, the helper does not read this on this system",
                false,
                started,
            );
            return out;
        }
        let availability = self.port.availability();
        if !availability.is_ready() {
            let out = self.without_helper(&availability);
            self.note(
                &format!("read on this computer, the helper is {}", state_word(&availability)),
                false,
                started,
            );
            return out;
        }
        let Some(calls) = self.port.calls(self.reading.class()) else {
            let out = self.without_helper(&Availability::Silent("The helper cannot be reached.".into()));
            self.note("read on this computer, the helper cannot be reached", true, started);
            return out;
        };
        if self.quiet_until.is_some_and(|until| self.port.now() < until) {
            let out = self.without_helper(&Availability::Silent("The helper did not answer this reading in time.".into()));
            self.note(
                "read on this computer, the helper is being left alone for this reading",
                true,
                started,
            );
            return out;
        }
        match self.reading.via_helper(&*calls) {
            Ok(v) => {
                self.strikes = 0;
                self.quiet_until = None;
                self.note("the helper answered", false, started);
                Sourced {
                    value: Reading::Available(v),
                    via: ReadVia::Helper,
                }
            }
            Err(e) => {
                if e.unreachable {
                    // This call found the helper unreachable (a refusal would not have): this reading waits before asking again.
                    self.strikes = self.strikes.saturating_add(1);
                    self.quiet_until = Some(self.port.now() + backoff(self.strikes));
                }
                let out = self.after_helper_error(&e.error.to_string());
                self.note(&format!("the helper could not read it: {e}"), true, started);
                out
            }
        }
    }

    /// The helper cannot be asked (`why`): the local reader, or the reason.
    fn without_helper(&mut self, availability: &Availability) -> Sourced<R::Value> {
        if !self.fallback {
            return availability.without_value("This reading comes from the helper only");
        }
        match self.reading.local() {
            Reading::Available(v) => {
                let via = match (availability, self.reading.partial(&v)) {
                    // Where no helper can exist a partial value is what there is.
                    (Availability::Unsupported(_), _) | (_, None) => ReadVia::Local,
                    (a, Some(missing)) => a.via(&missing),
                };
                Sourced {
                    value: Reading::Available(v),
                    via,
                }
            }
            Reading::Unavailable(why) => availability.without_value(&why),
        }
    }

    /// The helper was ready and could not read it.
    fn after_helper_error(&mut self, error: &str) -> Sourced<R::Value> {
        let helper = format!("The helper could not read it: {error}");
        let unavailable = |reason: String| ReadVia::Unavailable {
            reason,
            next: Some(NextStep::Retry),
        };
        if !self.fallback {
            let via = unavailable(sentence(&helper));
            return Sourced {
                value: Reading::Unavailable(via.reason().unwrap_or_default().to_string()),
                via,
            };
        }
        match self.reading.local() {
            Reading::Available(v) => {
                let via = match self.reading.partial(&v) {
                    Some(missing) => unavailable(format!("{} {}", sentence(&missing), sentence(&helper))),
                    None => ReadVia::Local,
                };
                Sourced {
                    value: Reading::Available(v),
                    via,
                }
            }
            Reading::Unavailable(local) => {
                let via = unavailable(format!("{} {}", sentence(&helper), sentence(&local)));
                Sourced {
                    value: Reading::Unavailable(via.reason().unwrap_or_default().to_string()),
                    via,
                }
            }
        }
    }
}

fn state_word(a: &Availability) -> &'static str {
    match a {
        Availability::Ready => "ready",
        Availability::Off => "off",
        Availability::Outdated(_) => "outdated",
        Availability::Unsupported(_) => "not supported here",
        Availability::Broken(_) => "damaged",
        Availability::Foreign(_) => "not trusted",
        Availability::Silent(_) => "not answering",
    }
}

/// Fakes for the tests of readings: a scripted helper wire and a centre over it.
#[cfg(test)]
pub(crate) mod testkit {
    use super::*;
    use crate::helper_centre::{HelperCentre, HelperInstaller};
    use crate::helper_link::{HelperLink, HelperTransport, TransportError};
    use dyno_core::helper::HelperHost;
    use dyno_core::helper_policy::Visibility;
    use dyno_core::helper_wire::{CallerView, HelperAction, HelperInfo, HelperReply, HelperRequest, HELPER_API, MIN_HELPER_VERSION};
    use paddock_jobs::time::FakeClock;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// How the fake helper behaves.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(crate) enum Mode {
        /// Answers.
        Up,
        /// Never installed.
        Absent,
        /// Installed but hung: a call lasts as long as its deadline (30 s when it has none).
        Silent,
        /// Something else owns the endpoint.
        Foreign,
        /// Answers, but is older than this Dyno accepts.
        Outdated,
    }

    /// Answers `Info` and `Audit` according to its mode and the rest from a script; counts and times the calls.
    pub(crate) struct Net {
        pub(crate) mode: Mutex<Mode>,
        script: Mutex<VecDeque<Result<HelperReply, TransportError>>>,
        pub(crate) infos: AtomicU32,
        /// The deadline each call was given, in order.
        pub(crate) maxes: Mutex<Vec<Option<Duration>>>,
        /// The clock a hung call spends its time on.
        pub(crate) clock: Mutex<Option<FakeClock>>,
        /// When each `Info` was asked, on that clock.
        pub(crate) info_times: Mutex<Vec<Duration>>,
    }
    impl Net {
        pub(crate) fn new(up: bool, script: Vec<Result<HelperReply, TransportError>>) -> Arc<Net> {
            Arc::new(Net {
                mode: Mutex::new(if up { Mode::Up } else { Mode::Absent }),
                script: Mutex::new(script.into()),
                infos: AtomicU32::new(0),
                maxes: Mutex::new(Vec::new()),
                clock: Mutex::new(None),
                info_times: Mutex::new(Vec::new()),
            })
        }
        /// How many scripted answers have not been asked for yet.
        pub(crate) fn scripted_left(&self) -> usize {
            self.script.lock().unwrap().len()
        }
        pub(crate) fn set(&self, mode: Mode) {
            *self.mode.lock().unwrap() = mode;
        }
    }
    pub(crate) struct Wire(Arc<Net>);
    impl HelperTransport for Wire {
        fn call(&self, request: HelperRequest) -> Result<HelperReply, TransportError> {
            self.call_within(request, None)
        }
        fn call_within(&self, request: HelperRequest, max: Option<Duration>) -> Result<HelperReply, TransportError> {
            let n = &self.0;
            n.maxes.lock().unwrap().push(max);
            let clock = n.clock.lock().unwrap().clone();
            if matches!(request, HelperRequest::Info) {
                n.info_times
                    .lock()
                    .unwrap()
                    .push(clock.as_ref().map_or(Duration::ZERO, |c| c.monotonic()));
            }
            let mode = *n.mode.lock().unwrap();
            match mode {
                Mode::Absent => return Err(TransportError::Absent),
                Mode::Foreign => return Err(TransportError::Foreign("S-1-5-21-9".into())),
                Mode::Silent => {
                    if let Some(c) = clock {
                        c.advance(max.unwrap_or(Duration::from_secs(30)));
                    }
                    return Err(TransportError::Silent("no answer".into()));
                }
                Mode::Up | Mode::Outdated => {}
            }
            match request {
                HelperRequest::Info => {
                    n.infos.fetch_add(1, Ordering::SeqCst);
                    Ok(HelperReply::About(
                        HelperInfo {
                            version: if mode == Mode::Outdated { "1.0.0" } else { MIN_HELPER_VERSION }.into(),
                            api: HELPER_API,
                            host: HelperHost::Linux,
                            agent_exe: "/x".into(),
                        },
                        CallerView {
                            scope: Visibility::Full,
                            tools: vec![],
                        },
                    ))
                }
                HelperRequest::Audit(_) => Ok(HelperReply::Entries(vec![])),
                _ => n.script.lock().unwrap().pop_front().expect("a scripted answer"),
            }
        }
    }
    pub(crate) struct NoInstall;
    impl HelperInstaller for NoInstall {
        fn install(&mut self, _: HelperAction) -> Result<(), String> {
            Err("no".into())
        }
        fn uninstall(&mut self) -> Result<(), String> {
            Err("no".into())
        }
    }

    pub(crate) type TestCentre = HelperCentre<Box<dyn HelperTransport>>;

    pub(crate) fn centre_supported(net: &Arc<Net>, clock: &FakeClock, supported: Result<(), String>) -> Arc<TestCentre> {
        *net.clock.lock().unwrap() = Some(clock.clone());
        Arc::new(HelperCentre::new(
            HelperLink::new(Box::new(Wire(net.clone())) as Box<dyn HelperTransport>, Arc::new(clock.clone())),
            HelperHost::Linux,
            supported,
            "1.1.0",
            Box::new(NoInstall),
        ))
    }

    pub(crate) fn centre(net: &Arc<Net>, clock: &FakeClock) -> Arc<TestCentre> {
        centre_supported(net, clock, Ok(()))
    }

    /// The production port over a centre.
    pub(crate) fn port(c: &Arc<TestCentre>) -> Box<dyn HelperPort> {
        Box::new(CentrePort(c.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::{centre, centre_supported, port, Mode, Net};
    use super::*;
    use crate::helper_link::TransportError;
    use dyno_core::helper_wire::HelperReply;
    use dyno_core::inventory::{MemoryLayout, Unavailable};
    use dyno_core::metrics::Reading;
    use dyno_core::read_via::{NextStep, NEEDS_ADMIN};
    use paddock_jobs::time::{Clock, FakeClock};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    const S: fn(u64) -> Duration = Duration::from_secs;

    #[test]
    fn readiness_costs_two_calls_once_and_is_cached_for_five_seconds() {
        let (net, clock) = (Net::new(true, vec![]), FakeClock::new());
        let c = centre(&net, &clock);
        assert!(c.ready());
        assert!(c.ready());
        assert_eq!(net.infos.load(Ordering::SeqCst), 1, "the second read used the cached answer");
        clock.advance(S(4));
        assert!(c.ready());
        assert_eq!(net.infos.load(Ordering::SeqCst), 1, "still inside the 5 s");
        clock.advance(S(2));
        assert!(c.ready());
        assert_eq!(net.infos.load(Ordering::SeqCst), 2, "asked again after 5 s");
    }

    #[test]
    fn a_remote_error_keeps_the_cache_but_silent_and_foreign_drop_it() {
        let remote = || {
            Err(TransportError::Remote {
                code: "refused".into(),
                message: "owner only".into(),
            })
        };
        let (net, clock) = (Net::new(true, vec![remote()]), FakeClock::new());
        let c = centre(&net, &clock);
        assert!(c.ready());
        assert!(c.link().traffic().is_err());
        assert!(c.ready());
        assert_eq!(
            net.infos.load(Ordering::SeqCst),
            1,
            "the helper refused one request; it is still there"
        );
        for mode in [Mode::Silent, Mode::Foreign] {
            let (net, clock) = (Net::new(true, vec![]), FakeClock::new());
            let c = centre(&net, &clock);
            assert!(c.ready());
            net.set(mode);
            assert!(c.link().traffic().is_err());
            assert!(!c.ready(), "{mode:?}: the fault dropped the cache and the probe sees it");
        }
    }

    #[test]
    fn a_job_that_changes_the_helper_drops_the_cache_and_moves_the_epoch() {
        let (net, clock) = (Net::new(true, vec![]), FakeClock::new());
        let c = centre(&net, &clock);
        assert!(c.ready());
        let before = c.epoch();
        c.forget_ready();
        assert_ne!(c.epoch(), before);
        assert!(c.ready());
        assert_eq!(net.infos.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn the_epoch_moves_when_readiness_flips_and_only_then() {
        let (net, clock) = (Net::new(false, vec![]), FakeClock::new());
        let c = centre(&net, &clock);
        assert!(!c.ready());
        let off = c.epoch();
        clock.advance(S(6));
        assert!(!c.ready());
        assert_eq!(c.epoch(), off, "asked again, same answer");
        net.set(Mode::Up);
        clock.advance(S(6));
        assert!(c.ready());
        let on = c.epoch();
        assert_ne!(on, off, "Off to Ready");
        // Settings polling the status feeds the same answer, so a flip seen there is seen by the readers.
        net.set(Mode::Absent);
        assert_eq!(c.status().state, dyno_core::helper_wire::HelperState::Off);
        assert_ne!(c.epoch(), on);
        assert!(!c.ready(), "and the readers do not ask again for it");
    }

    #[test]
    fn a_probe_that_races_a_forget_does_not_store_its_stale_answer() {
        let clock = FakeClock::new();
        let gate = ReadyGate::new(Arc::new(clock.clone()));
        let asked = AtomicU32::new(0);
        let a = gate.availability(
            || 0,
            || {
                asked.fetch_add(1, Ordering::SeqCst);
                gate.forget(); // a job changed the helper while this probe was in flight
                Availability::Ready
            },
        );
        assert!(a.is_ready(), "the caller still gets the answer it asked for");
        gate.availability(
            || 0,
            || {
                asked.fetch_add(1, Ordering::SeqCst);
                Availability::Ready
            },
        );
        assert_eq!(asked.load(Ordering::SeqCst), 2, "the stale answer was not kept");
    }

    #[test]
    fn live_reads_get_a_short_deadline_and_bulk_reads_do_not() {
        for (class, want) in [(ReadClass::Live, Some(LIVE_WAIT)), (ReadClass::Bulk, None)] {
            let (net, clock) = (Net::new(true, vec![Ok(HelperReply::Memory(total_only(8, "h")))]), FakeClock::new());
            let (r, _) = mem(Reading::unavailable("x"), false);
            let mut f = HelperFirst::new(Classed(r, class), port(&centre(&net, &clock)));
            assert_eq!(f.read().via, ReadVia::Helper);
            let maxes = net.maxes.lock().unwrap().clone();
            let memory_call = maxes.last().copied().flatten();
            assert_eq!(memory_call, want, "{class:?}");
            assert!(
                maxes[..maxes.len() - 1].iter().all(|m| *m == Some(LIVE_WAIT)),
                "the readiness probe is always live"
            );
        }
    }

    #[test]
    fn a_hung_helper_costs_one_short_wait_then_backs_off_5_10_20_up_to_60_seconds() {
        let (net, clock) = (Net::new(true, vec![]), FakeClock::new());
        net.set(Mode::Silent);
        let c = centre(&net, &clock);
        let (r, local_calls) = mem(Reading::Available(total_only(4, "local")), false);
        let mut f = HelperFirst::new(r, port(&c));
        let start = clock.monotonic();
        let mut spent = Duration::ZERO;
        for _ in 0..400 {
            let before = clock.monotonic();
            let got = f.read();
            spent += clock.monotonic() - before;
            assert_eq!(
                got.value.value().map(MemoryLayout::total),
                Some(4),
                "the local value is always there"
            );
            clock.advance(S(1));
        }
        let times = net.info_times.lock().unwrap().clone();
        let gaps: Vec<u64> = times.windows(2).map(|w| (w[1] - w[0]).as_secs()).collect();
        // Each probe lasts the 2 s deadline, then the not-ready answer is kept 5, 10, 20, 40, 60, 60 ... seconds.
        assert_eq!(gaps[..4], [7, 12, 22, 42], "{gaps:?}");
        assert!(gaps.iter().all(|g| *g <= 62), "{gaps:?}");
        assert!(gaps[4..].iter().all(|g| *g == 62), "{gaps:?}");
        assert!(
            spent <= LIVE_WAIT * u32::try_from(times.len()).unwrap(),
            "reads only ever waited for the probes: {spent:?}"
        );
        assert!(local_calls.load(Ordering::SeqCst) >= 400);
        assert!(clock.monotonic() - start < S(500));
    }

    #[test]
    fn a_working_helper_ends_the_backoff() {
        let (net, clock) = (Net::new(true, vec![]), FakeClock::new());
        net.set(Mode::Silent);
        let c = centre(&net, &clock);
        assert!(!c.ready());
        net.set(Mode::Up);
        clock.advance(S(6));
        assert!(c.ready());
        net.set(Mode::Silent);
        c.forget_ready();
        assert!(!c.ready());
        clock.advance(S(6));
        assert!(!c.ready());
        assert_eq!(
            net.info_times.lock().unwrap().len(),
            4,
            "the strikes started again after the helper worked (a 5 s answer, not a 10 s one, was kept after the last probe)"
        );
    }

    /// A reading of the memory layout over the helper's `MemoryModules`, which is all the tests need.
    struct Mem {
        local: Reading<MemoryLayout>,
        local_calls: Arc<AtomicU32>,
        partial_when_local: bool,
    }
    struct Classed(Mem, ReadClass);
    impl HelperReading for Classed {
        type Value = MemoryLayout;
        const KINDS: &'static [RequestKind] = &[RequestKind::MemoryModules];
        fn name(&self) -> &'static str {
            self.0.name()
        }
        fn class(&self) -> ReadClass {
            self.1
        }
        fn via_helper(&self, calls: &dyn HelperCalls) -> Result<MemoryLayout, CallError> {
            self.0.via_helper(calls)
        }
        fn local(&mut self) -> Reading<MemoryLayout> {
            self.0.local()
        }
    }
    fn total_only(total: u64, why: &str) -> MemoryLayout {
        MemoryLayout::TotalOnly {
            total,
            reason: Unavailable::failed(why),
        }
    }
    impl HelperReading for Mem {
        type Value = MemoryLayout;
        const KINDS: &'static [RequestKind] = &[RequestKind::MemoryModules];
        fn name(&self) -> &'static str {
            "memory modules"
        }
        fn class(&self) -> ReadClass {
            ReadClass::Live
        }
        fn via_helper(&self, calls: &dyn HelperCalls) -> Result<MemoryLayout, CallError> {
            calls.memory()
        }
        fn local(&mut self) -> Reading<MemoryLayout> {
            self.local_calls.fetch_add(1, Ordering::SeqCst);
            self.local.clone()
        }
        fn partial(&self, _: &MemoryLayout) -> Option<String> {
            self.partial_when_local.then(|| "Only the total is known.".to_string())
        }
    }
    fn mem(local: Reading<MemoryLayout>, partial: bool) -> (Mem, Arc<AtomicU32>) {
        let calls = Arc::new(AtomicU32::new(0));
        (
            Mem {
                local,
                local_calls: calls.clone(),
                partial_when_local: partial,
            },
            calls,
        )
    }
    fn read_with(mode: Mode, supported: Result<(), String>, local: Reading<MemoryLayout>) -> Sourced<MemoryLayout> {
        let (net, clock) = (Net::new(true, vec![]), FakeClock::new());
        net.set(mode);
        let (r, _) = mem(local, false);
        HelperFirst::new(r, port(&centre_supported(&net, &clock, supported))).read()
    }

    #[test]
    fn a_ready_helper_answers_and_the_local_reader_is_not_asked() {
        let layout = total_only(8, "from the helper");
        let (net, clock) = (Net::new(true, vec![Ok(HelperReply::Memory(layout.clone()))]), FakeClock::new());
        let (r, local_calls) = mem(Reading::unavailable("local"), false);
        let got = HelperFirst::new(r, port(&centre(&net, &clock))).read();
        assert_eq!((got.value.value(), got.via), (Some(&layout), ReadVia::Helper));
        assert_eq!(local_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn off_means_turn_it_on() {
        let got = read_with(Mode::Absent, Ok(()), Reading::unavailable("needs root"));
        assert_eq!(got.via, ReadVia::needs_admin("needs root"));
        assert!(matches!(&got.via, ReadVia::NeedsHelper { next, .. } if *next == NextStep::OpenSettings("helper".into())));
        assert!(matches!(got.value, Reading::Unavailable(m) if m.contains("Needs root.") && m.contains(NEEDS_ADMIN)));
    }

    #[test]
    fn outdated_means_update_the_helper() {
        let got = read_with(Mode::Outdated, Ok(()), Reading::unavailable("needs root"));
        let ReadVia::NeedsHelper { reason, next } = got.via else {
            panic!("{:?}", got.via)
        };
        assert_eq!(next, NextStep::UpdateHelper);
        assert!(reason.contains("Needs root.") && reason.contains("older"), "{reason}");
        assert!(matches!(got.value, Reading::Unavailable(m) if !m.contains(NEEDS_ADMIN) && m.contains("older")));
    }

    #[test]
    fn a_system_that_cannot_have_a_helper_never_says_turn_it_on() {
        let got = read_with(
            Mode::Absent,
            Err("This AppImage has no helper; install the .deb.".into()),
            Reading::unavailable("needs root"),
        );
        let ReadVia::Unavailable { reason, next } = got.via else {
            panic!("{:?}", got.via)
        };
        assert!(reason.contains("Needs root.") && reason.contains("AppImage"), "{reason}");
        assert_eq!(next, None);
        assert!(matches!(got.value, Reading::Unavailable(m) if !m.contains(NEEDS_ADMIN) && !m.to_lowercase().contains("turn on")));
    }

    #[test]
    fn a_foreign_program_on_the_endpoint_is_unavailable_with_its_reason() {
        let got = read_with(Mode::Foreign, Ok(()), Reading::unavailable("needs root"));
        let ReadVia::Unavailable { reason, next } = got.via else {
            panic!("{:?}", got.via)
        };
        assert!(reason.contains("does not trust"), "{reason}");
        assert_eq!(next, Some(NextStep::OpenSettings("helper".into())), "Repair is in Settings");
    }

    #[test]
    fn a_silent_helper_is_unavailable_and_retry_is_the_next_step() {
        let got = read_with(Mode::Silent, Ok(()), Reading::unavailable("needs root"));
        assert!(
            matches!(&got.via, ReadVia::Unavailable { next: Some(NextStep::Retry), reason } if reason.contains("did not answer")),
            "{:?}",
            got.via
        );
    }

    #[test]
    fn without_the_helper_the_local_value_is_read_and_says_local() {
        let got = read_with(Mode::Absent, Ok(()), Reading::Available(total_only(4, "local")));
        assert_eq!((got.value.value().map(MemoryLayout::total), got.via), (Some(4), ReadVia::Local));
    }

    #[test]
    fn a_partial_local_value_with_no_helper_keeps_the_value_and_asks_for_the_helper() {
        let (net, clock) = (Net::new(false, vec![]), FakeClock::new());
        let (r, _) = mem(Reading::Available(total_only(4, "local")), true);
        let got = HelperFirst::new(r, port(&centre(&net, &clock))).read();
        assert_eq!(got.value.value().map(MemoryLayout::total), Some(4));
        assert_eq!(got.via, ReadVia::needs_admin("Only the total is known."));
        // Where no helper can exist the partial value is simply what there is.
        let (net, clock) = (Net::new(false, vec![]), FakeClock::new());
        let (r, _) = mem(Reading::Available(total_only(4, "local")), true);
        let got = HelperFirst::new(r, port(&centre_supported(&net, &clock, Err("No helper in an AppImage.".into())))).read();
        assert_eq!(got.via, ReadVia::Local);
    }

    fn refused() -> Result<HelperReply, TransportError> {
        Err(TransportError::Remote {
            code: "refused".into(),
            message: "owner only".into(),
        })
    }

    #[test]
    fn a_helper_error_falls_back_to_local_and_when_both_fail_the_reason_names_both() {
        let (net, clock) = (Net::new(true, vec![refused()]), FakeClock::new());
        let (r, local_calls) = mem(Reading::Available(total_only(4, "local")), false);
        let got = HelperFirst::new(r, port(&centre(&net, &clock))).read();
        assert_eq!(got.via, ReadVia::Local);
        assert_eq!(local_calls.load(Ordering::SeqCst), 1);
        let (net, clock) = (Net::new(true, vec![refused()]), FakeClock::new());
        let (r, _) = mem(Reading::unavailable("no sensor"), false);
        let got = HelperFirst::new(r, port(&centre(&net, &clock))).read();
        let ReadVia::Unavailable { reason, .. } = got.via else {
            panic!("{:?}", got.via)
        };
        assert!(reason.contains("owner only") && reason.contains("No sensor"), "{reason}");
    }

    #[test]
    fn a_partial_local_value_after_a_helper_failure_names_the_helpers_reason() {
        let (net, clock) = (Net::new(true, vec![refused()]), FakeClock::new());
        let (r, _) = mem(Reading::Available(total_only(4, "local")), true);
        let got = HelperFirst::new(r, port(&centre(&net, &clock))).read();
        assert_eq!(
            got.value.value().map(MemoryLayout::total),
            Some(4),
            "the partial value is still shown"
        );
        let ReadVia::Unavailable { reason, .. } = got.via else {
            panic!("{:?}", got.via)
        };
        assert!(
            reason.contains("Only the total is known.") && reason.contains("owner only"),
            "{reason}"
        );
        assert!(!reason.contains(NEEDS_ADMIN));
    }

    #[test]
    fn the_local_fallback_can_be_switched_off_and_a_helper_error_is_then_unavailable_not_turn_it_on() {
        let (net, clock) = (Net::new(false, vec![]), FakeClock::new());
        let (r, local_calls) = mem(Reading::Available(total_only(4, "local")), false);
        let got = HelperFirst::new(r, port(&centre(&net, &clock))).with_fallback(false).read();
        assert!(matches!(got.via, ReadVia::NeedsHelper { .. }));
        assert!(got.value.value().is_none());
        assert_eq!(local_calls.load(Ordering::SeqCst), 0);
        let (net, clock) = (Net::new(true, vec![refused()]), FakeClock::new());
        let (r, _) = mem(Reading::Available(total_only(4, "local")), false);
        let got = HelperFirst::new(r, port(&centre(&net, &clock))).with_fallback(false).read();
        let ReadVia::Unavailable { reason, .. } = got.via else {
            panic!("{:?}", got.via)
        };
        assert!(reason.contains("owner only"), "{reason}");
        assert!(matches!(got.value, Reading::Unavailable(m) if !m.contains(NEEDS_ADMIN)));
    }

    #[test]
    fn many_reads_inside_five_seconds_probe_the_helper_once() {
        let answers: Vec<_> = (0..3).map(|_| Ok(HelperReply::Memory(total_only(8, "h")))).collect();
        let (net, clock) = (Net::new(true, answers), FakeClock::new());
        let (r, _) = mem(Reading::unavailable("x"), false);
        let mut f = HelperFirst::new(r, port(&centre(&net, &clock)));
        for _ in 0..3 {
            assert_eq!(f.read().via, ReadVia::Helper);
        }
        assert_eq!(net.infos.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_port_without_a_machine_is_off_and_the_local_value_is_used() {
        let (r, local_calls) = mem(Reading::Available(total_only(4, "local")), false);
        let got = HelperFirst::new(r, Box::new(OffPort)).read();
        assert_eq!(got.via, ReadVia::Local);
        assert_eq!(local_calls.load(Ordering::SeqCst), 1);
    }

    /// A port that says Ready but has no calls (the agent is shutting down).
    struct GonePort;
    impl HelperPort for GonePort {
        fn availability(&self) -> Availability {
            Availability::Ready
        }
        fn epoch(&self) -> u64 {
            0
        }
        fn now(&self) -> Duration {
            Duration::ZERO
        }
        fn calls(&self, _: ReadClass) -> Option<Arc<dyn HelperCalls>> {
            None
        }
    }

    #[test]
    fn a_ready_port_with_no_calls_falls_back_to_local() {
        let (r, local_calls) = mem(Reading::Available(total_only(4, "local")), false);
        let got = HelperFirst::new(r, Box::new(GonePort)).read();
        assert_eq!(got.via, ReadVia::Local);
        assert_eq!(local_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_gate_expires_by_the_links_clock() {
        let (net, clock) = (Net::new(true, vec![]), FakeClock::new());
        let c = centre(&net, &clock);
        // The link's clock is the one given to the link, not a private copy.
        let before = c.link().clock().monotonic();
        clock.advance(S(3));
        assert_eq!(c.link().clock().monotonic() - before, S(3));
        assert!(c.ready());
        clock.advance(S(5));
        assert!(c.ready());
        assert_eq!(net.infos.load(Ordering::SeqCst), 2, "5 s on that clock expired the answer");
    }

    fn silent() -> Result<HelperReply, TransportError> {
        Err(TransportError::Silent("no answer".into()))
    }
    fn memory_answer() -> Result<HelperReply, TransportError> {
        Ok(HelperReply::Memory(total_only(8, "h")))
    }

    #[test]
    fn a_reading_that_times_out_is_left_alone_for_a_growing_time_while_the_helper_stays_ready() {
        // The probe answers every time; only this reading's call hangs: it earns its own strike (5, 10 s ...), and meanwhile the
        // local value is read without waiting for the helper again.
        let (net, clock) = (Net::new(true, vec![silent(), silent(), memory_answer()]), FakeClock::new());
        let c = centre(&net, &clock);
        let (r, local_calls) = mem(Reading::Available(total_only(4, "local")), false);
        let mut f = HelperFirst::new(r, port(&c));
        assert_eq!(f.read().via, ReadVia::Local, "the call hung: the local value");
        assert_eq!(net.scripted_left(), 2);
        clock.advance(S(1));
        assert_eq!(f.read().via, ReadVia::Local);
        assert_eq!(
            net.scripted_left(),
            2,
            "no call to the helper for this reading during the first 5 s"
        );
        clock.advance(S(5));
        assert_eq!(f.read().via, ReadVia::Local, "asked again after 5 s; it hung again (second strike)");
        assert_eq!(net.scripted_left(), 1);
        clock.advance(S(6));
        assert_eq!(f.read().via, ReadVia::Local, "the second strike keeps it quiet for 10 s, not 5");
        assert_eq!(net.scripted_left(), 1);
        clock.advance(S(5));
        assert_eq!(f.read().via, ReadVia::Helper, "and a working call ends the strikes");
        assert_eq!(local_calls.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn the_seam_records_the_kinds_of_the_readings_built_while_a_scope_runs() {
        let (net, clock) = (Net::new(true, vec![]), FakeClock::new());
        let c = centre(&net, &clock);
        let before = HelperFirst::new(mem(Reading::Available(total_only(4, "local")), false).0, port(&c));
        let (inner, wired) = wired_kinds_while(|| {
            let (r, _) = mem(Reading::Available(total_only(4, "local")), false);
            // A nested scope keeps its own record and gives the outer one back.
            let (_, nested) = wired_kinds_while(|| HelperFirst::new(Noisy(mem(Reading::Available(total_only(4, "x")), false).0), port(&c)));
            assert_eq!(nested.len(), 2);
            HelperFirst::new(r, port(&c))
        });
        assert_eq!(
            wired,
            [RequestKind::MemoryModules].into_iter().collect(),
            "only what was built in the scope, not before it"
        );
        drop((before, inner));
        let (_, none) = wired_kinds_while(|| ());
        assert!(none.is_empty());
    }

    #[test]
    fn a_refusal_is_not_a_strike() {
        let (net, clock) = (Net::new(true, vec![refused(), memory_answer()]), FakeClock::new());
        let c = centre(&net, &clock);
        let (r, _) = mem(Reading::Available(total_only(4, "local")), false);
        let mut f = HelperFirst::new(r, port(&c));
        assert_eq!(f.read().via, ReadVia::Local);
        clock.advance(S(1));
        assert_eq!(
            f.read().via,
            ReadVia::Helper,
            "the helper answered a refusal, so it is asked again at once"
        );
    }

    /// A reading that also asks the helper for something else first, and has that other call fail: the other call's fault is not
    /// this reading's strike.
    struct Noisy(Mem);
    impl HelperReading for Noisy {
        type Value = MemoryLayout;
        const KINDS: &'static [RequestKind] = &[RequestKind::Traffic, RequestKind::MemoryModules];
        fn name(&self) -> &'static str {
            "noisy"
        }
        fn class(&self) -> ReadClass {
            ReadClass::Live
        }
        fn via_helper(&self, calls: &dyn HelperCalls) -> Result<MemoryLayout, crate::helper_link::CallError> {
            let _ = calls.traffic();
            calls.memory()
        }
        fn local(&mut self) -> Reading<MemoryLayout> {
            self.0.local()
        }
    }

    #[test]
    fn another_calls_fault_is_not_this_readings_strike() {
        // The first call (traffic) finds the helper silent; the reading's own call is refused. Only the reading's own typed error
        // counts, and a refusal is not a strike, so the next read asks again.
        let (net, clock) = (
            Net::new(true, vec![silent(), refused(), refused(), memory_answer()]),
            FakeClock::new(),
        );
        let (r, _) = mem(Reading::Available(total_only(4, "local")), false);
        let mut f = HelperFirst::new(Noisy(r), port(&centre(&net, &clock)));
        assert_eq!(f.read().via, ReadVia::Local);
        clock.advance(S(1));
        assert_eq!(net.scripted_left(), 2);
        assert_eq!(f.read().via, ReadVia::Helper, "asked again at once");
    }

    #[test]
    fn retry_now_ends_the_quiet_window_of_a_reading() {
        let (net, clock) = (Net::new(true, vec![silent(), memory_answer()]), FakeClock::new());
        let (r, _) = mem(Reading::Available(total_only(4, "local")), false);
        let mut f = HelperFirst::new(r, port(&centre(&net, &clock)));
        assert_eq!(f.read().via, ReadVia::Local, "the call hung");
        clock.advance(S(1));
        assert_eq!(f.read().via, ReadVia::Local, "left alone");
        assert_eq!(net.scripted_left(), 1);
        f.retry_now();
        assert_eq!(f.read().via, ReadVia::Helper, "the person asked: it is asked at once");
    }
}
