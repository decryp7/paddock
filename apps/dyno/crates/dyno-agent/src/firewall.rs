//! The firewall view (ADR 0031 section 4, read-only): the agent's own readers (Windows Defender Firewall through COM, the macOS
//! Application Firewall through `socketfilterfw`) and, for the sources only the helper can read (`pf`, `nft`), a source that says
//! so. Rule addresses are shown, never logged. A read is kept for a few seconds so a page refresh and the first open do not read
//! twice.

use dyno_core::firewall::{FirewallReader, FirewallSource, FirewallView, RuleSource};
use dyno_core::helper::HelperHost;
use dyno_core::protocol::{AgentError, ErrorCode};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How long a read is reused.
const REUSE: Duration = Duration::from_secs(5);

/// The words shown for a source the helper reads.
pub const NEEDS_HELPER: &str = "These rules need the helper. Turn it on in Settings to see them.";

/// The agent's readers and the last view.
pub struct FirewallCentre {
    os: HelperHost,
    readers: Vec<(RuleSource, Box<dyn FirewallReader>)>,
    helper_sources: Vec<RuleSource>,
    last: Mutex<Option<(Instant, FirewallView)>>,
}

impl FirewallCentre {
    /// The readers of this OS.
    pub fn for_this_os(clock: std::sync::Arc<dyn paddock_jobs::time::Clock>) -> FirewallCentre {
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
    pub fn new(os: HelperHost, readers: Vec<(RuleSource, Box<dyn FirewallReader>)>, helper_sources: Vec<RuleSource>) -> FirewallCentre {
        FirewallCentre {
            os,
            readers,
            helper_sources,
            last: Mutex::new(None),
        }
    }

    /// The view now (or the one read a moment ago). A reader that fails becomes a source with a note saying why, so one broken
    /// reader never hides the others.
    pub fn view(&self) -> Result<FirewallView, AgentError> {
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((at, v)) = last.as_ref() {
            if at.elapsed() < REUSE {
                return Ok(v.clone());
            }
        }
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
        let v = FirewallView { os: self.os, sources };
        *last = Some((Instant::now(), v.clone()));
        Ok(v)
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
        use std::sync::Arc;
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
}
