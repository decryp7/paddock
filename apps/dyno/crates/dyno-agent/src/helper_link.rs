//! The agent's side of the privileged helper (WP-01, ADR 0016): the link to the daemon, and the three wrappers that
//! route what only the helper can do (administrator tuning, all-users startup entries, Windows traffic) through it.
//!
//! Nothing here installs anything. The link only talks to a helper that is already there; installing is
//! [`crate::helper_centre`], started by a click. A missing or silent helper is an ordinary error with the next step.

use dyno_core::helper::{HelperAction, MachineStartup, TuningHost};
use dyno_core::helper_wire::{platform_error, AuditEntry, HelperInfo, HelperMessage, HelperReply, HelperRequest, Probe, HELPER_MISSING};
use dyno_core::network_traffic::TrafficCounter;
use dyno_core::ports::{PlatformError, ProcessTraffic, StartupItems, TuningSystem};
use dyno_core::startup::{StartupEntry, StartupState};
use dyno_ipc::{Client, IpcError, Peer, PeerOptions};
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// How long one call to the helper may take (a Defender change over many folders is the slowest).
pub const CALL_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a "who are you" probe may take (the Settings page and the tuning list wait for it).
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
    /// Makes an administrator change.
    fn apply(&self, action: &HelperAction) -> Result<(), PlatformError>;
    /// Reads a setting only an administrator can read.
    fn read(&self, action: &HelperAction) -> Result<String, PlatformError>;
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

    fn apply(&self, action: &HelperAction) -> Result<(), PlatformError> {
        self.expect(HelperRequest::Apply(action.clone()), |r| {
            matches!(r, HelperReply::Outcome(_)).then_some(())
        })
    }

    fn read(&self, action: &HelperAction) -> Result<String, PlatformError> {
        self.expect(HelperRequest::Read(action.clone()), |r| match r {
            HelperReply::Value(v) => Some(v),
            _ => None,
        })
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

/// Tuning with the helper behind the changes that need administrator rights. Everything else is the inner system.
pub struct ElevatedTuning<T: TuningSystem> {
    inner: T,
    link: Arc<dyn HelperLink>,
}

impl<T: TuningSystem> ElevatedTuning<T> {
    /// `inner` for what needs no rights, `link` for what does.
    pub fn new(inner: T, link: Arc<dyn HelperLink>) -> Self {
        ElevatedTuning { inner, link }
    }
}

impl<T: TuningSystem> TuningSystem for ElevatedTuning<T> {
    fn host(&self) -> TuningHost {
        self.inner.host()
    }

    fn read(&mut self, action: &HelperAction) -> Result<String, PlatformError> {
        match self.inner.read(action) {
            Err(e) if action.needs_admin_on(self.inner.host()) => {
                // Some settings (a Defender exclusion) only an administrator can read. A missing helper keeps the
                // inner answer: the page then shows "cannot be read here" instead of a prompt to install.
                action.validate().map_err(PlatformError::Failed)?;
                self.link.read(action).map_err(|h| {
                    if h == PlatformError::AccessDenied(HELPER_MISSING.into()) {
                        e
                    } else {
                        h
                    }
                })
            }
            other => other,
        }
    }

    fn apply(&mut self, action: &HelperAction) -> Result<(), PlatformError> {
        if action.needs_admin_on(self.inner.host()) {
            action.validate().map_err(PlatformError::Failed)?;
            self.link.apply(action)
        } else {
            self.inner.apply(action)
        }
    }
}

/// Startup entries with the helper behind the all-users ones.
pub struct ElevatedStartup {
    inner: Box<dyn StartupItems>,
    link: Arc<dyn HelperLink>,
}

impl ElevatedStartup {
    /// `inner` for the user's own entries, `link` for the all-users ones.
    pub fn new(inner: Box<dyn StartupItems>, link: Arc<dyn HelperLink>) -> Self {
        ElevatedStartup { inner, link }
    }
}

impl StartupItems for ElevatedStartup {
    fn list(&mut self) -> Result<Vec<StartupEntry>, PlatformError> {
        self.inner.list()
    }

    fn set_state(&mut self, entry: &StartupEntry, state: StartupState) -> Result<(), PlatformError> {
        if !entry.location.needs_admin() {
            return self.inner.set_state(entry, state);
        }
        let location = MachineStartup::of(&entry.location)
            .ok_or_else(|| PlatformError::Unsupported("Dyno cannot change that kind of startup item.".into()))?;
        let enabled = match state {
            StartupState::Enabled => true,
            StartupState::Disabled => false,
            StartupState::Delayed(_) => {
                return Err(PlatformError::Unsupported(
                    "An item that starts for every user can be turned on or off, not delayed.".into(),
                ))
            }
        };
        let action = HelperAction::SetMachineStartupEnabled {
            location,
            name: entry.name.clone(),
            enabled,
        };
        action.validate().map_err(PlatformError::Failed)?;
        self.link.apply(&action)
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
    use dyno_core::helper::{SleepSource, BALANCED_SCHEME};
    use dyno_core::startup::StartupLocation;
    use dyno_platform::tuning::FakeTuning;
    use std::sync::Mutex;

    /// Records what reached the helper and answers from a script.
    #[derive(Default)]
    struct FakeLink {
        applied: Mutex<Vec<HelperAction>>,
        reads: Mutex<Vec<HelperAction>>,
        missing: bool,
        value: Option<String>,
        counters: Vec<TrafficCounter>,
    }

    impl HelperLink for FakeLink {
        fn probe(&self) -> Probe {
            Probe::Absent
        }
        fn apply(&self, a: &HelperAction) -> Result<(), PlatformError> {
            if self.missing {
                return Err(LinkError::Missing.into());
            }
            self.applied.lock().unwrap().push(a.clone());
            Ok(())
        }
        fn read(&self, a: &HelperAction) -> Result<String, PlatformError> {
            self.reads.lock().unwrap().push(a.clone());
            if self.missing {
                return Err(LinkError::Missing.into());
            }
            self.value.clone().ok_or_else(|| PlatformError::Failed("no".into()))
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

    fn defender(excluded: bool) -> HelperAction {
        HelperAction::SetDefenderExcluded {
            paths: vec!["C:\\Users\\me\\src".into()],
            excluded,
        }
    }

    #[test]
    fn administrator_changes_go_to_the_helper_and_the_rest_stays_local() {
        let link = Arc::new(FakeLink::default());
        let fake = FakeTuning::new(TuningHost::Windows);
        let mut t = ElevatedTuning::new(fake.clone(), link.clone());
        t.apply(&HelperAction::SetHibernate(false)).unwrap();
        t.apply(&defender(true)).unwrap();
        t.apply(&HelperAction::SetPowerScheme(BALANCED_SCHEME.into())).unwrap();
        assert_eq!(link.applied.lock().unwrap().len(), 2);
        assert_eq!(fake.applied(), [HelperAction::SetPowerScheme(BALANCED_SCHEME.into())]);
        // macOS sleep is an administrator change there and not on Windows.
        let sleep = HelperAction::SetSleepMinutes {
            source: SleepSource::Ac,
            minutes: 5,
        };
        let mac_link = Arc::new(FakeLink::default());
        let mut mac = ElevatedTuning::new(FakeTuning::new(TuningHost::Macos), mac_link.clone());
        mac.apply(&sleep).unwrap();
        assert_eq!(mac_link.applied.lock().unwrap().len(), 1);
        assert_eq!(t.host(), TuningHost::Windows);
    }

    #[test]
    fn the_agent_validates_before_asking_and_a_missing_helper_says_how_to_get_it() {
        let link = Arc::new(FakeLink {
            missing: true,
            ..FakeLink::default()
        });
        let mut t = ElevatedTuning::new(FakeTuning::new(TuningHost::Windows), link.clone());
        let bad = HelperAction::SetDefenderExcluded {
            paths: vec!["C:\\Windows\\System32".into()],
            excluded: true,
        };
        assert!(matches!(t.apply(&bad), Err(PlatformError::Failed(_))));
        let err = t.apply(&HelperAction::SetHibernate(true)).unwrap_err();
        assert!(
            matches!(&err, PlatformError::AccessDenied(m) if m.contains("Settings > Helper")),
            "{err:?}"
        );
    }

    #[test]
    fn a_setting_the_system_can_read_is_not_asked_of_the_helper_but_one_it_cannot_is() {
        let link = Arc::new(FakeLink {
            value: Some("excluded".into()),
            ..FakeLink::default()
        });
        let fake = FakeTuning::new(TuningHost::Windows);
        fake.set_value(&HelperAction::SetHibernate(true), "on");
        let mut t = ElevatedTuning::new(fake, link.clone());
        assert_eq!(t.read(&HelperAction::SetHibernate(true)).unwrap(), "on");
        assert!(link.reads.lock().unwrap().is_empty());
        assert_eq!(t.read(&defender(true)).unwrap(), "excluded");
        assert_eq!(link.reads.lock().unwrap().len(), 1);
        // Without a helper the inner answer is kept (no "install it" noise for a read).
        let gone = Arc::new(FakeLink {
            missing: true,
            ..FakeLink::default()
        });
        let mut t = ElevatedTuning::new(FakeTuning::new(TuningHost::Windows), gone);
        assert!(matches!(t.read(&defender(true)), Err(PlatformError::Unsupported(_))));
    }

    fn entry(location: StartupLocation, name: &str) -> StartupEntry {
        StartupEntry {
            id: format!("x:{name}"),
            name: name.into(),
            command: "c".into(),
            exe: None,
            location,
            state: StartupState::Enabled,
            publisher: None,
            impact: dyno_core::startup::StartupImpact {
                cpu_secs: 0.0,
                disk_bytes: 0,
                rating: dyno_core::startup::ImpactRating::NotMeasured,
            },
        }
    }

    struct Inner(Arc<Mutex<Vec<String>>>);
    impl StartupItems for Inner {
        fn list(&mut self) -> Result<Vec<StartupEntry>, PlatformError> {
            Ok(vec![])
        }
        fn set_state(&mut self, e: &StartupEntry, _: StartupState) -> Result<(), PlatformError> {
            self.0.lock().unwrap().push(e.name.clone());
            Ok(())
        }
    }

    #[test]
    fn all_users_startup_entries_are_changed_by_the_helper_and_the_users_own_locally() {
        let link = Arc::new(FakeLink::default());
        let local = Arc::new(Mutex::new(Vec::new()));
        let mut s = ElevatedStartup::new(Box::new(Inner(local.clone())), link.clone());
        s.set_state(&entry(StartupLocation::UserRunKey, "Mine"), StartupState::Disabled)
            .unwrap();
        assert_eq!(*local.lock().unwrap(), ["Mine"]);
        s.set_state(&entry(StartupLocation::MachineRunKey, "Updater"), StartupState::Disabled)
            .unwrap();
        s.set_state(&entry(StartupLocation::SystemLaunchAgent, "com.example.up"), StartupState::Enabled)
            .unwrap();
        assert_eq!(
            *link.applied.lock().unwrap(),
            [
                HelperAction::SetMachineStartupEnabled {
                    location: MachineStartup::RunKey,
                    name: "Updater".into(),
                    enabled: false
                },
                HelperAction::SetMachineStartupEnabled {
                    location: MachineStartup::LaunchAgent,
                    name: "com.example.up".into(),
                    enabled: true
                }
            ]
        );
        let delayed = s.set_state(&entry(StartupLocation::MachineRunKey, "U"), StartupState::Delayed(30));
        assert!(matches!(delayed, Err(PlatformError::Unsupported(_))));
        let bad = s.set_state(&entry(StartupLocation::SystemLaunchAgent, "a b"), StartupState::Disabled);
        assert!(matches!(bad, Err(PlatformError::Failed(_))));
        assert_eq!(s.list().unwrap().len(), 0);
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
