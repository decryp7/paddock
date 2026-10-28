//! The agent's side of the privileged helper (WP-01, ADR 0016): the link to the daemon, and the wrapper that
//! route what only the helper can do (Windows traffic) through it.
//!
//! Nothing here installs anything. The link only talks to a helper that is already there; installing is
//! [`crate::helper_centre`], started by a click. A missing or silent helper is an ordinary error with the next step.

use dyno_core::helper_wire::{platform_error, AuditEntry, HelperInfo, HelperMessage, HelperReply, HelperRequest, Probe, HELPER_MISSING};
use dyno_core::network_traffic::TrafficCounter;
use dyno_core::ports::{PlatformError, ProcessTraffic};
use dyno_ipc::{Client, IpcError, Peer, PeerOptions};
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// How long one call to the helper may take (a Defender change over many folders is the slowest).
pub const CALL_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a "who are you" probe may take (the Settings page waits for it).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// The most audit entries the app shows.
const RECENT: u32 = 10;

/// Why a call to the helper did not get an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// No helper is installed (no token file or no endpoint).
    Missing,
    /// It is installed but did not answer, with why.
    Silent(String),
    /// It answered with a refusal or a failure.
    Remote(PlatformError),
}

impl From<LinkError> for PlatformError {
    fn from(e: LinkError) -> PlatformError {
        match e {
            LinkError::Missing => PlatformError::AccessDenied(HELPER_MISSING.into()),
            LinkError::Silent(why) => PlatformError::Failed(format!(
                "The helper did not answer ({why}). Install it again under Settings > Helper to repair it."
            )),
            LinkError::Remote(e) => e,
        }
    }
}

/// What the agent asks of the helper.
pub trait HelperLink: Send + Sync {
    /// Whether the helper answers, who it is and what it did lately.
    fn probe(&self) -> Probe;
    /// Per-program traffic counters.
    fn traffic(&self) -> Result<Vec<TrafficCounter>, PlatformError>;
    /// Removes the helper.
    fn uninstall(&self) -> Result<(), PlatformError>;
}

/// The real link over `dyno-ipc`: one short connection per call.
pub struct IpcHelperLink {
    endpoint: String,
    token_file: PathBuf,
}

impl IpcHelperLink {
    /// A link to the helper at `endpoint`, authenticated with the token in `token_file`.
    pub fn new(endpoint: impl Into<String>, token_file: impl Into<PathBuf>) -> IpcHelperLink {
        IpcHelperLink {
            endpoint: endpoint.into(),
            token_file: token_file.into(),
        }
    }

    /// The link to the helper of this computer.
    pub fn system() -> IpcHelperLink {
        let paths = dyno_platform::elevate::helper_paths();
        IpcHelperLink::new(paths.endpoint, paths.token)
    }

    fn exchange(&self, request: HelperRequest) -> Result<HelperReply, LinkError> {
        self.exchange_within(request, CALL_TIMEOUT)
    }

    fn exchange_within(&self, request: HelperRequest, timeout: Duration) -> Result<HelperReply, LinkError> {
        let text = std::fs::read_to_string(&self.token_file).map_err(|e| match e.kind() {
            ErrorKind::NotFound => LinkError::Missing,
            _ => LinkError::Silent(format!("the token file cannot be read: {e}")),
        })?;
        let token = dyno_ipc::Token::from_hex(text.trim()).ok_or_else(|| LinkError::Silent("the token file is damaged".into()))?;
        let conn = Client::connect(&self.endpoint, &token).map_err(|e| match e {
            IpcError::Io {
                kind: ErrorKind::NotFound, ..
            } => LinkError::Missing,
            IpcError::Auth(m) => LinkError::Silent(format!("it refused this Dyno: {m}")),
            other => LinkError::Silent(other.to_string()),
        })?;
        let peer = Peer::<HelperMessage>::start(conn, PeerOptions::new()).map_err(|e| LinkError::Silent(e.to_string()))?;
        let answer = peer.call(HelperMessage::Request(request), timeout);
        peer.close();
        match answer {
            Ok(HelperMessage::Reply(r)) => Ok(r),
            Ok(HelperMessage::Request(_)) => Err(LinkError::Silent("it answered with a request".into())),
            Err(IpcError::Remote(e)) => Err(LinkError::Remote(platform_error(&e.code, e.message))),
            Err(IpcError::Closed) => Err(LinkError::Silent(
                "it closed the connection; it may serve another user or another Dyno".into(),
            )),
            Err(e) => Err(LinkError::Silent(e.to_string())),
        }
    }

    fn expect<T>(&self, request: HelperRequest, pick: impl FnOnce(HelperReply) -> Option<T>) -> Result<T, PlatformError> {
        let reply = self.exchange(request)?;
        pick(reply).ok_or_else(|| PlatformError::Failed("The helper answered with something unexpected.".into()))
    }
}

impl HelperLink for IpcHelperLink {
    fn probe(&self) -> Probe {
        let info: HelperInfo = match self.exchange_within(HelperRequest::Info, PROBE_TIMEOUT) {
            Ok(HelperReply::About(i)) => i,
            Ok(_) => return Probe::Silent("it answered with something unexpected".into()),
            Err(LinkError::Missing) => return Probe::Absent,
            Err(LinkError::Silent(why)) => return Probe::Silent(why),
            Err(LinkError::Remote(e)) => return Probe::Silent(e.to_string()),
        };
        let recent: Vec<AuditEntry> = match self.exchange_within(HelperRequest::Audit(RECENT), PROBE_TIMEOUT) {
            Ok(HelperReply::Entries(e)) => e,
            _ => Vec::new(),
        };
        Probe::Answered(info, recent)
    }

    fn traffic(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
        self.expect(HelperRequest::Traffic, |r| match r {
            HelperReply::Counters(c) => Some(c),
            _ => None,
        })
    }

    fn uninstall(&self) -> Result<(), PlatformError> {
        self.expect(HelperRequest::Uninstall, |r| matches!(r, HelperReply::Bye).then_some(()))
    }
}

/// Traffic counters with the helper behind the "needs administrator rights" answer (Windows).
pub struct ElevatedTraffic<T: ProcessTraffic> {
    inner: T,
    link: Arc<dyn HelperLink>,
}

impl<T: ProcessTraffic> ElevatedTraffic<T> {
    /// `inner` first; the helper only when the inner source says it needs administrator rights.
    pub fn new(inner: T, link: Arc<dyn HelperLink>) -> Self {
        ElevatedTraffic { inner, link }
    }
}

impl<T: ProcessTraffic> ProcessTraffic for ElevatedTraffic<T> {
    fn counters(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
        match self.inner.counters() {
            Err(PlatformError::Unsupported(why)) if why.contains("administrator rights") => match self.link.traffic() {
                Err(PlatformError::AccessDenied(m)) if m == HELPER_MISSING => Err(PlatformError::Unsupported(why)),
                other => other,
            },
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records what reached the helper and answers from a script.
    #[derive(Default)]
    struct FakeLink {
        missing: bool,
        counters: Vec<TrafficCounter>,
    }

    impl HelperLink for FakeLink {
        fn probe(&self) -> Probe {
            Probe::Absent
        }
        fn traffic(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
            if self.missing {
                Err(LinkError::Missing.into())
            } else {
                Ok(self.counters.clone())
            }
        }
        fn uninstall(&self) -> Result<(), PlatformError> {
            Ok(())
        }
    }

    struct Needs(PlatformError);
    impl ProcessTraffic for Needs {
        fn counters(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
            Err(self.0.clone())
        }
    }

    #[test]
    fn traffic_asks_the_helper_only_when_the_system_says_it_needs_administrator_rights() {
        let c = TrafficCounter {
            pid: 4,
            name: "chrome.exe".into(),
            bytes_in: 10,
            bytes_out: 20,
        };
        let needs = PlatformError::Unsupported("Traffic per program is not available without administrator rights on Windows".into());
        let ok = Arc::new(FakeLink {
            counters: vec![c.clone()],
            ..FakeLink::default()
        });
        assert_eq!(ElevatedTraffic::new(Needs(needs.clone()), ok.clone()).counters().unwrap(), [c]);
        // No helper: the original reason stays, so the page can point at Settings.
        let gone = Arc::new(FakeLink {
            missing: true,
            ..FakeLink::default()
        });
        assert_eq!(ElevatedTraffic::new(Needs(needs.clone()), gone).counters(), Err(needs));
        // Other failures are not the helper's business.
        let other = PlatformError::Failed("no sockets".into());
        assert_eq!(ElevatedTraffic::new(Needs(other.clone()), ok).counters(), Err(other));
    }
}
