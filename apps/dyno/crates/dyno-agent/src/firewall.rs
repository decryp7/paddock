//! The firewall view (ADR 0031 section 4, read-only): the agent's own readers (Windows Defender Firewall through COM, the macOS
//! Application Firewall through `socketfilterfw`) and, for the sources only the helper can read (`pf`, `nft`), a source that says
//! so. Rule addresses are shown, never logged. A read is kept for a few seconds so a page refresh and the first open do not read
//! twice.

use dyno_core::firewall::{FirewallReader, FirewallSource, FirewallView, RuleSource};
use dyno_core::helper::HelperHost;
use dyno_core::protocol::{AgentError, ErrorCode};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How long a read is reused.
const REUSE: Duration = Duration::from_secs(5);
/// The longest a request waits for a read; a slower read goes on in the background and the next request gets its result.
const WAIT: Duration = Duration::from_secs(2);

/// The words shown for a source the helper reads.
pub const NEEDS_HELPER: &str = "These rules need the helper. Turn it on in Settings to see them.";

type Readers = Vec<(RuleSource, Box<dyn FirewallReader>)>;

#[derive(Default)]
struct State {
    last: Option<(Instant, Result<FirewallView, AgentError>)>,
    reading: bool,
    generation: u64,
}

struct Inner {
    os: HelperHost,
    readers: Readers,
    helper_sources: Vec<RuleSource>,
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
    /// Reads every reader. A reader that fails becomes a source with a note saying why, so one broken reader never hides the others.
    fn read_all(&self) -> Result<FirewallView, AgentError> {
        let mut sources = Vec::new();
        for (source, reader) in &self.readers {
            let started = Instant::now();
            match reader.read() {
                Ok(s) => sources.push(s),
                Err(e) => {
                    log::warn!(target: "dyno::firewall", "reader failed: ms={}", started.elapsed().as_millis());
                    let mut s = FirewallSource::new(*source);
                    s.rules_hidden = true;
                    s.note = Some(e.to_string());
                    sources.push(s);
                }
            }
        }
        for source in &self.helper_sources {
            let mut s = FirewallSource::new(*source);
            s.rules_hidden = true;
            s.note = Some(NEEDS_HELPER.to_string());
            sources.push(s);
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
    /// The readers of this OS.
    pub fn for_this_os(clock: Arc<dyn paddock_jobs::time::Clock>) -> FirewallCentre {
        #[cfg(windows)]
        {
            let _ = clock;
            FirewallCentre::new(
                HelperHost::Windows,
                vec![(
                    RuleSource::WindowsFirewall,
                    Box::new(dyno_platform::windows_firewall::WindowsFirewallReader(
                        dyno_platform::windows::firewall_com::ComFirewall,
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
            let _ = clock;
            FirewallCentre::new(HelperHost::Linux, vec![], vec![RuleSource::Nftables])
        }
    }

    /// Readers given (tests).
    pub fn new(os: HelperHost, readers: Readers, helper_sources: Vec<RuleSource>) -> FirewallCentre {
        FirewallCentre::with_wait(os, readers, helper_sources, WAIT)
    }

    /// As [`new`](Self::new) with a given longest wait (tests).
    pub fn with_wait(os: HelperHost, readers: Readers, helper_sources: Vec<RuleSource>, wait: Duration) -> FirewallCentre {
        FirewallCentre {
            inner: Arc::new(Inner {
                os,
                readers,
                helper_sources,
                state: Mutex::new(State::default()),
                done: Condvar::new(),
            }),
            wait,
        }
    }

    /// The view now, or the one read a moment ago. The read runs on its own thread; when it takes longer than the wait, the
    /// request answers `Busy` ("still reading") and the result is there for the next one.
    pub fn view(&self) -> Result<FirewallView, AgentError> {
        let mut st = self.inner.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((at, r)) = st.last.as_ref() {
            if at.elapsed() < REUSE {
                return r.clone();
            }
        }
        if !st.reading {
            st.reading = true;
            let inner = self.inner.clone();
            let spawned = std::thread::Builder::new().name("dyno-firewall".into()).spawn(move || {
                let started = Instant::now();
                let r = inner.read_all();
                log::info!(target: "dyno::firewall", "read done: ms={} ok={}", started.elapsed().as_millis(), r.is_ok());
                let mut st = inner.state.lock().unwrap_or_else(PoisonError::into_inner);
                st.last = Some((Instant::now(), r));
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
            |(_, r)| r.clone(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn helper_sources_say_they_need_the_helper_and_a_failing_reader_does_not_hide_the_rest() {
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
        assert_eq!(v.sources[0].note, None);
        assert!(v.sources[1].rules_hidden && v.sources[1].note.as_deref().unwrap().contains("failed"));
        assert_eq!(v.sources[2].source, RuleSource::Pf);
        assert_eq!(v.sources[2].note.as_deref(), Some(NEEDS_HELPER));
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
        let mut got = None;
        for _ in 0..200 {
            if let Ok(v) = c.view() {
                got = Some(v);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            got.expect("the finished read is served").sources[0].source,
            RuleSource::WindowsFirewall
        );
    }
}
