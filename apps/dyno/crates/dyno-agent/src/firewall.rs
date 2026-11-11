//! The firewall view (ADR 0031 section 4, read-only): the agent's own readers (Windows Defender Firewall through COM, the macOS
//! Application Firewall through `socketfilterfw`) and, for the sources only the helper can read (`pf`, `nft`), the helper's
//! answer through the seam ([`crate::helper_first`]; WP-FW-a), else a source that says why and what to do. Every source says who
//! read it (`via`), and the **agent** sets that, never the reader: a reply cannot claim to have been read by the helper. Rule
//! addresses are shown, never logged. A read is kept for a few seconds so a page refresh and the first open do not read twice.
#![cfg_attr(
    not(test),
    deny(clippy::let_underscore_must_use, clippy::let_underscore_future, clippy::let_underscore_untyped)
)]

use crate::helper_first::{HelperCalls, HelperFirst, HelperPort, HelperReading, OffPort, ReadClass};
use crate::helper_link::CallError;
use dyno_core::firewall::{FirewallReader, FirewallSource, FirewallView, RuleSource};
use dyno_core::helper::HelperHost;
use dyno_core::helper_policy::RequestKind;
use dyno_core::metrics::Reading;
use dyno_core::protocol::{AgentError, ErrorCode};
use dyno_core::read_via::ReadVia;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How long a read is reused.
const REUSE: Duration = Duration::from_secs(5);
/// The longest a request waits for a read; a slower read goes on in the background and the next request gets its result.
const WAIT: Duration = Duration::from_secs(2);

/// A rule source only the helper can read (`pf` needs root, `nft` needs `CAP_NET_ADMIN`): the seam reading.
pub struct HelperFirewall {
    source: RuleSource,
}

impl HelperFirewall {
    /// The reading of `source`.
    pub fn new(source: RuleSource) -> HelperFirewall {
        HelperFirewall { source }
    }
}

impl HelperReading for HelperFirewall {
    type Value = FirewallSource;
    const KINDS: &'static [RequestKind] = &[RequestKind::Firewall];

    fn name(&self) -> &'static str {
        "firewall rules"
    }

    /// Reading a firewall is a slow kind with its own time limit, read on the firewall worker, not per tick.
    fn class(&self) -> ReadClass {
        ReadClass::Bulk
    }

    fn via_helper(&self, calls: &dyn HelperCalls) -> Result<FirewallSource, CallError> {
        calls.firewall()
    }

    fn local(&mut self) -> Reading<FirewallSource> {
        let which = match self.source {
            RuleSource::Pf => "the pf firewall",
            RuleSource::Nftables => "the nftables firewall",
            _ => "this firewall",
        };
        Reading::Unavailable(format!("the rules of {which} can be read only by an administrator"))
    }
}

/// A port that several sources share.
struct SharedHelper(Arc<dyn HelperPort>);

impl HelperPort for SharedHelper {
    fn availability(&self) -> crate::helper_first::Availability {
        self.0.availability()
    }
    fn epoch(&self) -> u64 {
        self.0.epoch()
    }
    fn now(&self) -> Duration {
        self.0.now()
    }
    fn calls(&self, class: ReadClass) -> Option<Arc<dyn HelperCalls>> {
        self.0.calls(class)
    }
}

/// The sources only the helper reads, each over the seam.
type HelperSources = Vec<(RuleSource, HelperFirst<HelperFirewall>)>;

fn helper_sources_over(sources: &[RuleSource], port: &Arc<dyn HelperPort>) -> HelperSources {
    sources
        .iter()
        .map(|s| (*s, HelperFirst::new(HelperFirewall::new(*s), Box::new(SharedHelper(port.clone())))))
        .collect()
}

/// A source as the agent presents it: **who read it is set here**, whatever the reader put in the field.
fn presented(mut source: FirewallSource, via: ReadVia) -> FirewallSource {
    source.via = via;
    source
}

type Readers = Vec<(RuleSource, Box<dyn FirewallReader>)>;

#[derive(Default)]
struct State {
    last: Option<(Instant, u64, Result<FirewallView, AgentError>)>,
    reading: bool,
    generation: u64,
}

struct Inner {
    os: HelperHost,
    readers: Readers,
    helper_sources: Mutex<HelperSources>,
    /// The helper the sources over the seam ask; [`FirewallCentre::bind_helper`] replaces the "no helper" one it starts with.
    helper_port: Mutex<Arc<dyn HelperPort>>,
    state: Mutex<State>,
    done: Condvar,
}

/// The agent's readers and the last view. The readers run through the one tool runner (a timeout and bounded output each) on a
/// worker thread, so a slow one (hundreds of Windows rules) never holds up a request for more than [`WAIT`].
pub struct FirewallCentre {
    inner: Arc<Inner>,
    wait: Duration,
}

impl Inner {
    /// The helper's epoch (it moves when the helper is turned on or off, repaired or updated, or readiness flips).
    fn epoch(&self) -> u64 {
        if self.helper_sources.lock().unwrap_or_else(PoisonError::into_inner).is_empty() {
            return 0;
        }
        let port = self.helper_port.lock().unwrap_or_else(PoisonError::into_inner).clone();
        // The reading refreshes the port's epoch as a side effect; what it says is not needed here, only that it ran.
        let availability = port.availability();
        log::trace!(availability:? = availability; "firewall: availability read for the epoch");
        port.epoch()
    }

    /// Reads every reader. A reader that fails becomes a source with a note saying why, so one broken reader never hides the others.
    fn read_all(&self) -> Result<FirewallView, AgentError> {
        let mut sources = Vec::new();
        for (source, reader) in &self.readers {
            let started = Instant::now();
            match reader.read() {
                Ok(s) => sources.push(presented(s, ReadVia::Local)),
                Err(e) => {
                    log::warn!(target: "dyno::firewall", "reader failed: ms={}", started.elapsed().as_millis());
                    let mut s = FirewallSource::new(*source);
                    s.rules_hidden = true;
                    s.note = Some(e.to_string());
                    sources.push(presented(s, ReadVia::unavailable(&e.to_string())));
                }
            }
        }
        for (source, first) in self.helper_sources.lock().unwrap_or_else(PoisonError::into_inner).iter_mut() {
            let got = first.read();
            match got.value {
                Reading::Available(s) => sources.push(presented(s, ReadVia::Helper)),
                Reading::Unavailable(why) => {
                    let mut s = FirewallSource::new(*source);
                    s.rules_hidden = true;
                    s.note = Some(why);
                    sources.push(presented(s, got.via));
                }
            }
        }
        if sources.is_empty() {
            return Err(AgentError::new(
                ErrorCode::Unsupported,
                "There is no firewall to read on this system.",
            ));
        }
        Ok(FirewallView { os: self.os, sources })
    }
}

impl FirewallCentre {
    /// The readers of this OS. The sources only the helper reads say it is off until [`bind_helper`](Self::bind_helper) gives
    /// them the helper (the platform is built before the agent's `Shared` exists).
    pub fn for_this_os(clock: Arc<dyn paddock_jobs::time::Clock>) -> FirewallCentre {
        #[cfg(windows)]
        {
            FirewallCentre::new(
                HelperHost::Windows,
                vec![(
                    RuleSource::WindowsFirewall,
                    Box::new(dyno_platform::windows_firewall::WindowsFirewallReader::new(
                        dyno_platform::windows::firewall_com::ComFirewall,
                        Box::new(dyno_platform::windows::indirect::ShellIndirect),
                        clock,
                    )),
                )],
                vec![],
            )
        }
        #[cfg(target_os = "macos")]
        {
            FirewallCentre::new(
                HelperHost::Macos,
                vec![(
                    RuleSource::ApplicationFirewall,
                    Box::new(dyno_platform::firewall_readers::AlfReader::new(
                        Box::new(dyno_platform::runner::SystemRunner),
                        clock,
                    )),
                )],
                vec![RuleSource::Pf],
            )
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _: std::sync::Arc<dyn paddock_jobs::time::Clock> = clock;
            FirewallCentre::new(HelperHost::Linux, vec![], vec![RuleSource::Nftables])
        }
    }

    /// Readers given (tests); the helper is off until bound.
    pub fn new(os: HelperHost, readers: Readers, helper_sources: Vec<RuleSource>) -> FirewallCentre {
        FirewallCentre::with_wait(os, readers, helper_sources, WAIT)
    }

    /// Gives the sources only the helper reads the helper to ask (composition, once the agent exists). The kept view is dropped.
    pub fn bind_helper(&self, port: Arc<dyn HelperPort>) {
        let sources: Vec<RuleSource> = self
            .inner
            .helper_sources
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(s, _)| *s)
            .collect();
        *self.inner.helper_sources.lock().unwrap_or_else(PoisonError::into_inner) = helper_sources_over(&sources, &port);
        *self.inner.helper_port.lock().unwrap_or_else(PoisonError::into_inner) = port;
        self.inner.state.lock().unwrap_or_else(PoisonError::into_inner).last = None;
    }

    /// As [`new`](Self::new) with a given longest wait (tests).
    pub fn with_wait(os: HelperHost, readers: Readers, helper_sources: Vec<RuleSource>, wait: Duration) -> FirewallCentre {
        let off: Arc<dyn HelperPort> = Arc::new(OffPort);
        FirewallCentre {
            inner: Arc::new(Inner {
                os,
                readers,
                helper_sources: Mutex::new(helper_sources_over(&helper_sources, &off)),
                helper_port: Mutex::new(off),
                state: Mutex::new(State::default()),
                done: Condvar::new(),
            }),
            wait,
        }
    }

    /// The view now, or the one read a moment ago. The read runs on its own thread; when it takes longer than the wait, the
    /// request answers `Busy` ("still reading") and the result is there for the next one.
    pub fn view(&self) -> Result<FirewallView, AgentError> {
        // The helper turned on, off or repaired since the kept view: it is read again (the epoch moves then).
        let epoch = self.inner.epoch();
        let mut st = self.inner.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((at, kept, r)) = st.last.as_ref() {
            if at.elapsed() < REUSE && *kept == epoch {
                return r.clone();
            }
        }
        if !st.reading {
            st.reading = true;
            let inner = self.inner.clone();
            let spawned = std::thread::Builder::new().name("dyno-firewall".into()).spawn(move || {
                let started = Instant::now();
                let epoch = inner.epoch();
                let r = inner.read_all();
                log::info!(target: "dyno::firewall", "read done: ms={} ok={}", started.elapsed().as_millis(), r.is_ok());
                let mut st = inner.state.lock().unwrap_or_else(PoisonError::into_inner);
                st.last = Some((Instant::now(), epoch, r));
                st.reading = false;
                st.generation += 1;
                inner.done.notify_all();
            });
            if spawned.is_err() {
                st.reading = false;
                return Err(AgentError::new(
                    ErrorCode::Internal,
                    "The firewall could not be read: no thread was available.",
                ));
            }
        }
        let started_at = st.generation;
        let (st, timeout) = self
            .inner
            .done
            .wait_timeout_while(st, self.wait, |s| s.generation == started_at)
            .unwrap_or_else(PoisonError::into_inner);
        if timeout.timed_out() {
            return Err(AgentError::new(
                ErrorCode::Busy,
                "Still reading the firewall. Try Refresh in a moment.",
            ));
        }
        st.last.as_ref().map_or_else(
            || Err(AgentError::new(ErrorCode::Internal, "The firewall read left no result.")),
            |(_, _, r)| r.clone(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_first::testkit::{centre, Mode, Net};
    use crate::helper_first::CentrePort;
    use crate::helper_link::TransportError;
    use dyno_core::helper_wire::HelperReply;
    use dyno_core::ports::PlatformError;

    struct Ok1;
    impl FirewallReader for Ok1 {
        fn read(&self) -> Result<FirewallSource, PlatformError> {
            Ok(FirewallSource::new(RuleSource::ApplicationFirewall))
        }
    }
    struct Bad;
    impl FirewallReader for Bad {
        fn read(&self) -> Result<FirewallSource, PlatformError> {
            Err(PlatformError::Failed("socketfilterfw failed (exit 1).".into()))
        }
    }

    #[test]
    fn a_failing_reader_does_not_hide_the_rest_and_a_helper_source_without_the_helper_says_what_to_do() {
        let c = FirewallCentre::new(
            HelperHost::Macos,
            vec![
                (RuleSource::ApplicationFirewall, Box::new(Ok1)),
                (RuleSource::WindowsFirewall, Box::new(Bad)),
            ],
            vec![RuleSource::Pf],
        );
        let v = c.view().unwrap();
        assert_eq!(v.sources.len(), 3);
        assert_eq!((v.sources[0].note.as_deref(), &v.sources[0].via), (None, &ReadVia::Local));
        assert!(v.sources[1].rules_hidden && v.sources[1].note.as_deref().unwrap().contains("failed"));
        assert!(matches!(v.sources[1].via, ReadVia::Unavailable { .. }));
        let pf = &v.sources[2];
        assert_eq!(pf.source, RuleSource::Pf);
        assert!(
            pf.rules_hidden && pf.note.as_deref().unwrap().contains("pf firewall"),
            "{:?}",
            pf.note
        );
        assert!(
            matches!(&pf.via, ReadVia::NeedsHelper { reason, .. } if reason.contains("pf firewall")),
            "{:?}",
            pf.via
        );
    }

    fn bound(up: bool, script: Vec<Result<HelperReply, TransportError>>) -> (FirewallCentre, Arc<Net>, paddock_jobs::time::FakeClock) {
        let (net, clock) = (Net::new(up, script), paddock_jobs::time::FakeClock::new());
        let c = FirewallCentre::new(HelperHost::Linux, vec![], vec![RuleSource::Nftables]);
        c.bind_helper(Arc::new(CentrePort(centre(&net, &clock))));
        (c, net, clock)
    }

    fn nft(rules: usize) -> FirewallSource {
        FirewallSource::new(RuleSource::Nftables).with_rules(
            (0..rules)
                .map(|i| dyno_core::firewall::FirewallRule {
                    name: format!("rule {i}"),
                    direction: dyno_core::firewall::RuleDirection::In,
                    action: dyno_core::firewall::RuleAction::Allow,
                    enabled: true,
                    program: None,
                    ports: Some("22".into()),
                    addresses: None,
                    profile: dyno_core::firewall::FirewallProfile::All,
                    source: RuleSource::Nftables,
                    group: None,
                    service: None,
                    protocol: None,
                })
                .collect(),
        )
    }

    #[test]
    fn a_ready_helper_gives_the_rules_and_the_agent_says_so() {
        let (c, _, _) = bound(true, vec![Ok(HelperReply::Firewall(nft(2)))]);
        let s = &c.view().unwrap().sources[0];
        assert_eq!((s.rules.len(), &s.via, s.rules_hidden), (2, &ReadVia::Helper, false));
    }

    #[test]
    fn the_agent_always_overwrites_via() {
        // The helper's reply claims it was not read by the helper; a local reader claims it was. Neither is believed.
        let lie = |via| HelperReply::Firewall(nft(1).with_via(via));
        let (c, _, _) = bound(true, vec![Ok(lie(ReadVia::Local))]);
        assert_eq!(
            c.view().unwrap().sources[0].via,
            ReadVia::Helper,
            "what the helper answered was read by the helper"
        );
        struct Claims;
        impl FirewallReader for Claims {
            fn read(&self) -> Result<FirewallSource, PlatformError> {
                Ok(FirewallSource::new(RuleSource::ApplicationFirewall).with_via(ReadVia::Helper))
            }
        }
        let c = FirewallCentre::new(HelperHost::Macos, vec![(RuleSource::ApplicationFirewall, Box::new(Claims))], vec![]);
        assert_eq!(
            c.view().unwrap().sources[0].via,
            ReadVia::Local,
            "a reader of this computer was read locally"
        );
    }

    #[test]
    fn a_caller_who_may_only_count_gets_the_counts_and_the_helper_is_still_the_reader() {
        let (c, _, _) = bound(true, vec![Ok(HelperReply::Firewall(nft(12).counts_only()))]);
        let s = &c.view().unwrap().sources[0];
        assert_eq!(
            (s.rule_count, s.rules.len(), s.rules_hidden, &s.via),
            (12, 0, true, &ReadVia::Helper)
        );
    }

    #[test]
    fn a_helper_that_cannot_read_gives_the_reason_not_rules() {
        let refused = Err(TransportError::Remote {
            code: "unsupported".into(),
            message: "nftables is not installed".into(),
        });
        let (c, _, _) = bound(true, vec![refused]);
        let s = &c.view().unwrap().sources[0];
        assert!(s.rules_hidden && s.rules.is_empty());
        assert_ne!(s.via, ReadVia::Helper);
        assert!(s.note.as_deref().unwrap().contains("nftables is not installed"), "{:?}", s.note);
    }

    #[test]
    fn turning_the_helper_on_makes_the_next_view_read_again_inside_the_reuse_window() {
        let (c, net, clock) = bound(false, vec![Ok(HelperReply::Firewall(nft(3)))]);
        assert!(
            matches!(c.view().unwrap().sources[0].via, ReadVia::NeedsHelper { .. }),
            "the helper is not there yet"
        );
        net.set(Mode::Up);
        // The kept view is younger than the reuse time, but the helper's standing changed: Ready now.
        clock.advance(Duration::from_secs(6));
        let v = c.view().unwrap();
        assert_eq!((&v.sources[0].via, v.sources[0].rules.len()), (&ReadVia::Helper, 3));
    }

    #[test]
    fn no_reader_and_no_helper_source_is_an_unsupported_system() {
        let e = FirewallCentre::new(HelperHost::Linux, vec![], vec![]).view().unwrap_err();
        assert_eq!(e.code, ErrorCode::Unsupported);
    }

    #[test]
    fn a_read_is_reused_for_a_moment() {
        use std::sync::atomic::{AtomicU32, Ordering};
        struct Count(Arc<AtomicU32>);
        impl FirewallReader for Count {
            fn read(&self) -> Result<FirewallSource, PlatformError> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(FirewallSource::new(RuleSource::ApplicationFirewall))
            }
        }
        let n = Arc::new(AtomicU32::new(0));
        let c = FirewallCentre::new(
            HelperHost::Macos,
            vec![(RuleSource::ApplicationFirewall, Box::new(Count(n.clone())))],
            vec![],
        );
        c.view().unwrap();
        c.view().unwrap();
        assert_eq!(n.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_slow_read_answers_busy_in_time_and_its_result_is_there_for_the_next_request() {
        struct Gate(std::sync::Mutex<std::sync::mpsc::Receiver<()>>);
        impl FirewallReader for Gate {
            fn read(&self) -> Result<FirewallSource, PlatformError> {
                let _ = self.0.lock().unwrap().recv();
                Ok(FirewallSource::new(RuleSource::WindowsFirewall))
            }
        }
        let (go, wait_for) = std::sync::mpsc::channel();
        let c = FirewallCentre::with_wait(
            HelperHost::Windows,
            vec![(RuleSource::WindowsFirewall, Box::new(Gate(std::sync::Mutex::new(wait_for))))],
            vec![],
            Duration::from_millis(30),
        );
        assert_eq!(c.view().unwrap_err().code, ErrorCode::Busy);
        assert_eq!(
            c.view().unwrap_err().code,
            ErrorCode::Busy,
            "a second request does not start a second read"
        );
        go.send(()).unwrap();
        let got = paddock_testkit::poll_for(Duration::from_secs(20), || c.view().ok());
        assert_eq!(
            got.expect("the finished read is served").sources[0].source,
            RuleSource::WindowsFirewall
        );
    }
}
