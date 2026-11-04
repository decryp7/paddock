//! The seam every agent-side reading the helper can give goes through (Dyno helper-wiring plan section 3, rule 55: one
//! implementation). The policy is the user's of 2026-11-04 (ADR 0027 amended, decision D1): **helper first, then this computer,
//! else an honest needs-admin reason with the next step**.
//!
//! A reading is a [`HelperReading`]: how to ask the helper ([`HelperReading::via_helper`], a typed [`HelperCalls`] method) and what
//! the agent can read itself ([`HelperReading::local`]). [`HelperFirst`] runs it: it asks the [`HelperPort`] whether the helper is
//! ready (a cached answer, [`ReadyGate`]: readiness costs two IPC calls and a reader that runs every tick must not pay them every
//! time), calls the helper when it is, falls back when it is not or when it errors, and says which path answered ([`Sourced`]).
//!
//! Adding a reading: implement [`HelperReading`] (about 20 lines), build `HelperFirst::new(reading, port)` where the agent is
//! composed (`agent.rs`), and put `Sourced::via` on the reply type's `via` field. Nothing else in this file changes.

use dyno_core::btm::LoginItemsReply;
use dyno_core::helper_counters::CounterAnswer;
use dyno_core::helper_extras::ExtrasAnswer;
use dyno_core::helper_holders::{FileHolders, SocketOwners};
use dyno_core::helper_processes::ProcessPage;
use dyno_core::helper_requests::{CounterQuery, ExtrasKind, ProcessQuery, SmartQuery};
use dyno_core::inventory::MemoryLayout;
use dyno_core::metrics::Reading;
use dyno_core::network_traffic::TrafficCounter;
use dyno_core::ports::PlatformError;
use dyno_core::read_via::ReadVia;
use dyno_core::smart::SmartReply;
use paddock_jobs::time::Clock;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// How long a readiness answer is kept (plan section 3.1).
pub const READY_TTL: Duration = Duration::from_secs(5);

/// The object-safe slice of the helper link the readings use (interface segregation; the real one is `HelperLink`, tests use it
/// over a fake transport). One typed method per request kind the agent reads with.
pub trait HelperCalls: Send + Sync {
    /// One page of the process list, scoped to the caller by the helper.
    fn processes(&self, query: ProcessQuery) -> Result<ProcessPage, PlatformError>;
    /// Per-program traffic counters (Windows), scoped to the caller.
    fn traffic(&self) -> Result<Vec<TrafficCounter>, PlatformError>;
    /// Socket inode to owning program (Linux).
    fn socket_owners(&self) -> Result<SocketOwners, PlatformError>;
    /// The programs holding the file with this device and inode (Linux).
    fn file_holders(&self, dev: u64, ino: u64) -> Result<FileHolders, PlatformError>;
    /// Thermal, energy and fan readings.
    fn counters(&self, query: CounterQuery) -> Result<Vec<CounterAnswer>, PlatformError>;
    /// Memory modules from the DMI table (Linux).
    fn memory(&self) -> Result<MemoryLayout, PlatformError>;
    /// Wake requests or Windows Backup versions.
    fn extras(&self, kind: ExtrasKind) -> Result<ExtrasAnswer, PlatformError>;
    /// Login items (macOS).
    fn login_items(&self) -> Result<LoginItemsReply, PlatformError>;
    /// SMART health of the disks the helper's own scan finds.
    fn smart(&self, query: SmartQuery) -> Result<SmartReply, PlatformError>;
}

impl<T: crate::helper_link::HelperTransport> HelperCalls for crate::helper_link::HelperLink<T> {
    fn processes(&self, query: ProcessQuery) -> Result<ProcessPage, PlatformError> {
        crate::helper_link::HelperLink::processes(self, query)
    }
    fn traffic(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
        crate::helper_link::HelperLink::traffic(self)
    }
    fn socket_owners(&self) -> Result<SocketOwners, PlatformError> {
        crate::helper_link::HelperLink::socket_owners(self)
    }
    fn file_holders(&self, dev: u64, ino: u64) -> Result<FileHolders, PlatformError> {
        crate::helper_link::HelperLink::file_holders(self, dev, ino)
    }
    fn counters(&self, query: CounterQuery) -> Result<Vec<CounterAnswer>, PlatformError> {
        crate::helper_link::HelperLink::counters(self, query)
    }
    fn memory(&self) -> Result<MemoryLayout, PlatformError> {
        crate::helper_link::HelperLink::memory(self)
    }
    fn extras(&self, kind: ExtrasKind) -> Result<ExtrasAnswer, PlatformError> {
        crate::helper_link::HelperLink::extras(self, kind)
    }
    fn login_items(&self) -> Result<LoginItemsReply, PlatformError> {
        self.login_items_now()
    }
    fn smart(&self, query: SmartQuery) -> Result<SmartReply, PlatformError> {
        crate::helper_link::HelperLink::smart(self, query)
    }
}

/// Where a reading finds the helper: whether it is ready (cached) and the calls to make. Implemented over the agent's
/// `Shared.helper` by [`SharedPort`], over a centre by tests, and by [`OffPort`] where there is no helper.
pub trait HelperPort: Send + Sync {
    /// Whether the helper is ready. Cached for [`READY_TTL`]; cheap to call on every tick.
    fn ready(&self) -> bool;
    /// The calls, or `None` when the agent is gone (shutting down).
    fn calls(&self) -> Option<Arc<dyn HelperCalls>>;
}

/// A port with no helper: never ready. Readings composed in a test agent that must not touch the machine use it (ER-14).
pub struct OffPort;

impl HelperPort for OffPort {
    fn ready(&self) -> bool {
        false
    }
    fn calls(&self) -> Option<Arc<dyn HelperCalls>> {
        None
    }
}

/// The helper of a running agent.
pub struct SharedPort(pub std::sync::Weak<crate::agent::Shared>);

impl HelperPort for SharedPort {
    fn ready(&self) -> bool {
        self.0.upgrade().is_some_and(|s| s.helper.ready())
    }
    fn calls(&self) -> Option<Arc<dyn HelperCalls>> {
        Some(self.0.upgrade()?.helper.link_arc())
    }
}

/// A centre as a port (tests, and any code that holds the centre rather than the agent).
pub struct CentrePort<T: crate::helper_link::HelperTransport + 'static>(pub Arc<crate::helper_centre::HelperCentre<T>>);

impl<T: crate::helper_link::HelperTransport + 'static> HelperPort for CentrePort<T> {
    fn ready(&self) -> bool {
        self.0.ready()
    }
    fn calls(&self) -> Option<Arc<dyn HelperCalls>> {
        Some(self.0.link_arc())
    }
}

/// A readiness answer kept for a while. It is dropped at once when a call finds the helper unreachable (the link counts those
/// as faults) or when [`ReadyGate::forget`] says a job changed the helper.
pub struct ReadyGate {
    clock: Arc<dyn Clock>,
    ttl: Duration,
    cached: Mutex<Option<Cached>>,
}

#[derive(Clone, Copy)]
struct Cached {
    at: Duration,
    faults: u64,
    ready: bool,
}

impl ReadyGate {
    /// A gate that keeps an answer for `ttl` on `clock`.
    pub fn new(clock: Arc<dyn Clock>, ttl: Duration) -> ReadyGate {
        ReadyGate {
            clock,
            ttl,
            cached: Mutex::new(None),
        }
    }

    /// The cached answer when it is fresh and no call found the helper unreachable since (`faults` is the link's counter), else
    /// `probe()`'s, which is then kept.
    pub fn is_ready(&self, faults: impl Fn() -> u64, probe: impl FnOnce() -> bool) -> bool {
        let now = self.clock.monotonic();
        if let Some(c) = *self.cached.lock().unwrap_or_else(PoisonError::into_inner) {
            if now.saturating_sub(c.at) < self.ttl && c.faults == faults() {
                return c.ready;
            }
        }
        let ready = probe();
        *self.cached.lock().unwrap_or_else(PoisonError::into_inner) = Some(Cached {
            at: self.clock.monotonic(),
            // After the probe: the probe's own failures are what it just reported.
            faults: faults(),
            ready,
        });
        ready
    }

    /// Drops the kept answer (a job turned the helper on, off, repaired or updated it).
    pub fn forget(&self) {
        *self.cached.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// One reading the helper can give better than the agent.
pub trait HelperReading: Send {
    /// The domain value: the same type from both paths.
    type Value;
    /// A short name for the log (no user content).
    fn name(&self) -> &'static str;
    /// Asks the helper, through a typed [`HelperCalls`] method. An answer that holds nothing useful is an `Err` with the reason,
    /// so the agent's own reader gets its turn (Smart: no disk with wear counters).
    ///
    /// # Errors
    /// What the helper refused or could not read, in words.
    fn via_helper(&self, calls: &dyn HelperCalls) -> Result<Self::Value, PlatformError>;
    /// What the agent can read itself; `Reading::Unavailable(reason)` when nothing.
    fn local(&mut self) -> Reading<Self::Value>;
    /// For a local value that is only part of what the helper would give (Linux memory: the total without the slots), the
    /// sentence saying what is missing. The value is still shown; the reading's source then asks for the helper.
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

/// Runs a [`HelperReading`]: helper first, then local, else the needs-admin reason.
pub struct HelperFirst<R: HelperReading> {
    reading: R,
    port: Box<dyn HelperPort>,
    fallback: bool,
}

impl<R: HelperReading> HelperFirst<R> {
    /// The reading over `port`, with the local fallback on (the product choice, plan decision D1).
    pub fn new(reading: R, port: Box<dyn HelperPort>) -> Self {
        HelperFirst {
            reading,
            port,
            fallback: true,
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

    /// Reads once.
    pub fn read(&mut self) -> Sourced<R::Value> {
        let name = self.reading.name();
        if !self.port.ready() {
            return self.local("the helper is not on", false);
        }
        let Some(calls) = self.port.calls() else {
            return self.local("the helper is not reachable", false);
        };
        match self.reading.via_helper(&*calls) {
            Ok(v) => {
                log::debug!(target: "dyno::helper", "{name}: the helper answered");
                Sourced {
                    value: Reading::Available(v),
                    via: ReadVia::Helper,
                }
            }
            Err(e) => {
                log::warn!(target: "dyno::helper", "{name}: the helper could not read it: {e}");
                self.local(&format!("the helper could not read it: {e}"), true)
            }
        }
    }

    /// The local path. `helper_tried`: the helper was ready and failed, so a missing local value is `Unavailable`, not a request
    /// to turn the helper on.
    fn local(&mut self, why: &str, helper_tried: bool) -> Sourced<R::Value> {
        let name = self.reading.name();
        if !self.fallback {
            let reason = format!("{why}.");
            return Self::needs(reason);
        }
        log::debug!(target: "dyno::helper", "{name}: reading on this computer instead ({why})");
        match self.reading.local() {
            Reading::Available(v) => {
                let via = match (helper_tried, self.reading.partial(&v)) {
                    (false, Some(missing)) => ReadVia::needs_admin(&missing),
                    _ => ReadVia::Local,
                };
                Sourced {
                    value: Reading::Available(v),
                    via,
                }
            }
            Reading::Unavailable(local_why) if helper_tried => {
                let reason = format!("{why}; {local_why}");
                Sourced {
                    value: Reading::Unavailable(reason.clone()),
                    via: ReadVia::Unavailable { reason },
                }
            }
            Reading::Unavailable(local_why) => Self::needs(local_why),
        }
    }

    fn needs(why: String) -> Sourced<R::Value> {
        let via = ReadVia::needs_admin(&why);
        Sourced {
            value: Reading::Unavailable(via.reason().unwrap_or_default().to_string()),
            via,
        }
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

    /// Answers `Info` and `Audit` like a ready helper (or fails like an absent one) and the rest from a script; counts the calls.
    pub(crate) struct Net {
        pub(crate) up: Mutex<bool>,
        script: Mutex<VecDeque<Result<HelperReply, TransportError>>>,
        pub(crate) infos: AtomicU32,
    }
    impl Net {
        pub(crate) fn new(up: bool, script: Vec<Result<HelperReply, TransportError>>) -> Arc<Net> {
            Arc::new(Net {
                up: Mutex::new(up),
                script: Mutex::new(script.into()),
                infos: AtomicU32::new(0),
            })
        }
    }
    pub(crate) struct Wire(Arc<Net>);
    impl HelperTransport for Wire {
        fn call(&self, request: HelperRequest) -> Result<HelperReply, TransportError> {
            let n = &self.0;
            if !*n.up.lock().unwrap() {
                return Err(TransportError::Absent);
            }
            match request {
                HelperRequest::Info => {
                    n.infos.fetch_add(1, Ordering::SeqCst);
                    Ok(HelperReply::About(
                        HelperInfo {
                            version: MIN_HELPER_VERSION.into(),
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

    pub(crate) fn centre(net: &Arc<Net>, clock: &FakeClock) -> Arc<HelperCentre<Box<dyn HelperTransport>>> {
        Arc::new(HelperCentre::new(
            HelperLink::new(Box::new(Wire(net.clone())) as Box<dyn HelperTransport>, Arc::new(clock.clone())),
            HelperHost::Linux,
            Ok(()),
            "1.1.0",
            Box::new(NoInstall),
        ))
    }

    /// The production port over a centre.
    pub(crate) fn port(c: &Arc<HelperCentre<Box<dyn HelperTransport>>>) -> Box<dyn HelperPort> {
        Box::new(CentrePort(c.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::{centre, port, Net};
    use super::*;
    use crate::helper_link::TransportError;
    use dyno_core::helper_wire::HelperReply;
    use dyno_core::inventory::{MemoryLayout, Unavailable};
    use dyno_core::metrics::Reading;
    use dyno_core::ports::PlatformError;
    use dyno_core::read_via::{NextStep, NEEDS_ADMIN};
    use paddock_jobs::time::{Clock, FakeClock};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn readiness_costs_two_calls_once_and_is_cached_for_five_seconds() {
        let (net, clock) = (Net::new(true, vec![]), FakeClock::new());
        let c = centre(&net, &clock);
        assert!(c.ready());
        assert!(c.ready());
        assert_eq!(net.infos.load(Ordering::SeqCst), 1, "the second read used the cached answer");
        clock.advance(Duration::from_secs(4));
        assert!(c.ready());
        assert_eq!(net.infos.load(Ordering::SeqCst), 1, "still inside the 5 s");
        clock.advance(Duration::from_secs(2));
        assert!(c.ready());
        assert_eq!(net.infos.load(Ordering::SeqCst), 2, "asked again after 5 s");
    }

    #[test]
    fn a_transport_error_drops_the_cached_ready_at_once() {
        let (net, clock) = (Net::new(true, vec![]), FakeClock::new());
        let c = centre(&net, &clock);
        assert!(c.ready());
        *net.up.lock().unwrap() = false;
        // Some other call finds the helper gone: the cache must not keep saying Ready.
        assert!(c.link().traffic().is_err());
        assert!(!c.ready(), "the fault dropped the cache; the probe now sees an absent helper");
    }

    #[test]
    fn a_job_that_changes_the_helper_drops_the_cache() {
        let (net, clock) = (Net::new(true, vec![]), FakeClock::new());
        let c = centre(&net, &clock);
        assert!(c.ready());
        c.forget_ready();
        assert!(c.ready());
        assert_eq!(net.infos.load(Ordering::SeqCst), 2);
    }

    /// A reading of a number: the helper's answer is `Smart`-free so the test needs no domain; it uses `MemoryModules`.
    struct Mem {
        local: Reading<MemoryLayout>,
        local_calls: Arc<AtomicU32>,
        partial_when_local: bool,
    }
    fn total_only(total: u64, why: &str) -> MemoryLayout {
        MemoryLayout::TotalOnly {
            total,
            reason: Unavailable::failed(why),
        }
    }
    impl HelperReading for Mem {
        type Value = MemoryLayout;
        fn name(&self) -> &'static str {
            "memory modules"
        }
        fn via_helper(&self, calls: &dyn HelperCalls) -> Result<MemoryLayout, PlatformError> {
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

    #[test]
    fn a_ready_helper_answers_and_the_local_reader_is_not_asked() {
        let layout = total_only(8, "from the helper");
        let (net, clock) = (Net::new(true, vec![Ok(HelperReply::Memory(layout.clone()))]), FakeClock::new());
        let (r, local_calls) = mem(Reading::unavailable("local"), false);
        let mut f = HelperFirst::new(r, port(&centre(&net, &clock)));
        let got = f.read();
        assert_eq!((got.value.value(), got.via), (Some(&layout), ReadVia::Helper));
        assert_eq!(local_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn without_the_helper_the_local_value_is_read_and_says_local() {
        let (net, clock) = (Net::new(false, vec![]), FakeClock::new());
        let (r, _) = mem(Reading::Available(total_only(4, "local")), false);
        let got = HelperFirst::new(r, port(&centre(&net, &clock))).read();
        assert_eq!((got.value.value().map(MemoryLayout::total), got.via), (Some(4), ReadVia::Local));
    }

    #[test]
    fn a_partial_local_value_with_no_helper_keeps_the_value_and_asks_for_the_helper() {
        let (net, clock) = (Net::new(false, vec![]), FakeClock::new());
        let (r, _) = mem(Reading::Available(total_only(4, "local")), true);
        let got = HelperFirst::new(r, port(&centre(&net, &clock))).read();
        assert_eq!(got.value.value().map(MemoryLayout::total), Some(4));
        assert_eq!(got.via, ReadVia::needs_admin("Only the total is known."));
    }

    #[test]
    fn nothing_readable_without_the_helper_is_needs_helper_with_the_shared_sentence_and_the_settings_step() {
        let (net, clock) = (Net::new(false, vec![]), FakeClock::new());
        let (r, _) = mem(Reading::unavailable("needs root"), false);
        let got = HelperFirst::new(r, port(&centre(&net, &clock))).read();
        let ReadVia::NeedsHelper { reason, next } = got.via else {
            panic!("{:?}", got.via)
        };
        assert!(reason.contains("needs root") && reason.contains(NEEDS_ADMIN), "{reason}");
        assert_eq!(next, NextStep::OpenSettings("helper".into()));
        assert!(matches!(got.value, Reading::Unavailable(m) if m.contains(NEEDS_ADMIN)));
    }

    #[test]
    fn a_helper_error_falls_back_to_local_and_when_both_fail_the_reason_names_both() {
        let refused = || {
            Err(TransportError::Remote {
                code: "refused".into(),
                message: "owner only".into(),
            })
        };
        let (net, clock) = (Net::new(true, vec![refused()]), FakeClock::new());
        let (r, local_calls) = mem(Reading::Available(total_only(4, "local")), false);
        let got = HelperFirst::new(r, port(&centre(&net, &clock))).read();
        assert_eq!(got.via, ReadVia::Local);
        assert_eq!(local_calls.load(Ordering::SeqCst), 1);
        let (net, clock) = (Net::new(true, vec![refused()]), FakeClock::new());
        let (r, _) = mem(Reading::unavailable("no sensor"), false);
        let got = HelperFirst::new(r, port(&centre(&net, &clock))).read();
        let ReadVia::Unavailable { reason } = got.via else {
            panic!("{:?}", got.via)
        };
        assert!(reason.contains("owner only") && reason.contains("no sensor"), "{reason}");
    }

    #[test]
    fn the_local_fallback_can_be_switched_off() {
        let (net, clock) = (Net::new(false, vec![]), FakeClock::new());
        let (r, local_calls) = mem(Reading::Available(total_only(4, "local")), false);
        let got = HelperFirst::new(r, port(&centre(&net, &clock))).with_fallback(false).read();
        assert!(matches!(got.via, ReadVia::NeedsHelper { .. }));
        assert!(got.value.value().is_none());
        assert_eq!(local_calls.load(Ordering::SeqCst), 0);
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
    fn a_port_without_a_machine_is_never_ready() {
        let (r, local_calls) = mem(Reading::Available(total_only(4, "local")), false);
        let got = HelperFirst::new(r, Box::new(OffPort)).read();
        assert_eq!(got.via, ReadVia::Local);
        assert_eq!(local_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_clock_the_gate_uses_is_the_links() {
        let (net, clock) = (Net::new(true, vec![]), FakeClock::new());
        let c = centre(&net, &clock);
        assert_eq!(c.link().clock().monotonic(), clock.monotonic());
    }
}
