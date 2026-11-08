//! Wake-on-LAN adapters behind `dyno_core::wol`'s ports (ADR 0032 section 3): the UDP sender, the card list, the neighbour tables,
//! the echo and this PC's wake settings. Programs run through the one [`crate::runner::ToolRunner`] with a timeout; their text is
//! parsed by `dyno_core::wol`'s parsers, which never panic. A test never reaches these (ER-14): the fakes in `dyno_core::wol::fake`
//! stand in, and the tool-based adapters here are tested with a scripted runner.

mod neighbours;
mod ping;
mod readiness;
mod secrets;
mod udp;

pub use neighbours::OsNeighbours;
pub use ping::OsProber;
pub use readiness::OsReadiness;
pub use secrets::os_store;
pub use udp::{OsInterfaces, OsUdp};

use crate::runner::{SystemRunner, ToolRunner};
use dyno_core::wol::wake::{NeighbourSource, Prober, UdpBroadcast};
use dyno_core::wol::{Interfaces, ReadinessSource};
use std::sync::Arc;

/// The Wake-on-LAN ports of a platform.
pub struct WolPorts {
    /// Sends the datagrams.
    pub udp: Arc<dyn UdpBroadcast>,
    /// The card list.
    pub interfaces: Arc<dyn Interfaces>,
    /// The neighbour tables.
    pub neighbours: Arc<dyn NeighbourSource>,
    /// The echo.
    pub prober: Arc<dyn Prober>,
    /// This PC's wake settings.
    pub readiness: Arc<dyn ReadinessSource>,
    /// Where a device's SecureOn password is kept.
    pub secrets: Arc<dyn paddock_secrets::Secrets>,
    /// The clock a wake waits on.
    pub clock: Arc<dyn dyno_core::wol::WolClock>,
    /// Sets the OS wake for a scheduled wake from inside the agent (Windows: a waitable timer that wakes the computer); `None` where
    /// the administrator helper does it (macOS, Linux) (ADR 0032 section 5).
    pub wake_timer: Option<Arc<dyn dyno_core::wake::WakeScheduler>>,
}

/// The agent's own wake timer where the OS has one the agent may set (Windows); the helper does it elsewhere.
#[cfg(windows)]
fn real_wake_timer() -> Option<Arc<dyn dyno_core::wake::WakeScheduler>> {
    Some(Arc::new(crate::windows::wake_timer::WakeTimers::default()))
}

/// The agent's own wake timer where the OS has one the agent may set (Windows); the helper does it elsewhere.
#[cfg(not(windows))]
fn real_wake_timer() -> Option<Arc<dyn dyno_core::wake::WakeScheduler>> {
    None
}

/// A wake timer that only records (ER-14: a test never sets a real wake). Pending wakes are kept, soonest first.
#[derive(Default)]
#[cfg(any(test, feature = "test-support"))]
pub struct FakeWakeTimer {
    pending: std::sync::Mutex<Vec<dyno_core::wake::WakeEntry>>,
    /// When set, every schedule is refused with this message (the OS said no).
    pub refuse: std::sync::Mutex<Option<String>>,
}

#[cfg(any(test, feature = "test-support"))]
impl dyno_core::wake::WakeScheduler for FakeWakeTimer {
    fn schedule(&self, tag: &dyno_core::wake::WakeTag, at_utc: i64) -> Result<(), dyno_core::ports::PlatformError> {
        if let Some(m) = self.refuse.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone() {
            return Err(dyno_core::ports::PlatformError::Failed(m));
        }
        let mut p = self.pending.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        p.retain(|w| &w.tag != tag);
        p.push(dyno_core::wake::WakeEntry { tag: tag.clone(), at_utc });
        p.sort_by_key(|w| w.at_utc);
        Ok(())
    }

    fn cancel(&self, tag: &dyno_core::wake::WakeTag) -> Result<bool, dyno_core::ports::PlatformError> {
        let mut p = self.pending.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let before = p.len();
        p.retain(|w| &w.tag != tag);
        Ok(p.len() != before)
    }

    fn list(&self) -> Result<Vec<dyno_core::wake::WakeEntry>, dyno_core::ports::PlatformError> {
        Ok(self.pending.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone())
    }
}

/// The real clock: monotonic time and a real sleep.
pub struct RealClock(std::time::Instant);

impl RealClock {
    /// A clock whose origin is now.
    pub fn new() -> RealClock {
        RealClock(std::time::Instant::now())
    }
}

impl Default for RealClock {
    fn default() -> Self {
        Self::new()
    }
}

impl dyno_core::wol::WolClock for RealClock {
    fn now(&self) -> std::time::Duration {
        self.0.elapsed()
    }

    fn sleep(&self, d: std::time::Duration) {
        std::thread::sleep(d);
    }
}

impl WolPorts {
    /// This machine's own adapters. Only the real wiring of each OS uses it, never a test (ER-14).
    pub fn os(secrets_dir: &std::path::Path) -> WolPorts {
        let runner = || -> Box<dyn ToolRunner> { Box::new(SystemRunner) };
        WolPorts {
            udp: Arc::new(OsUdp),
            interfaces: Arc::new(OsInterfaces),
            neighbours: Arc::new(OsNeighbours::new(runner())),
            prober: Arc::new(OsProber::new(runner())),
            readiness: Arc::new(OsReadiness::new(runner())),
            secrets: os_store(secrets_dir),
            clock: Arc::new(RealClock::new()),
            wake_timer: real_wake_timer(),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl WolPorts {
    /// Fakes that record and script and touch no socket, table or process (ER-14): one card on a documentation network, no
    /// neighbours, nothing answering ping, nothing readable. The fake platform wires these.
    pub fn fake() -> WolPorts {
        use dyno_core::wol::fake::{FakeInterfaces, FakeNeighbours, FakeProber, FakeReadiness, FakeUdp};
        WolPorts {
            udp: Arc::new(FakeUdp::default()),
            interfaces: Arc::new(FakeInterfaces(vec![dyno_core::wol::NetIf {
                name: "en0".into(),
                addr: std::net::Ipv4Addr::new(192, 0, 2, 17),
                prefix: 24,
            }])),
            neighbours: Arc::new(FakeNeighbours::default()),
            prober: Arc::new(FakeProber::default()),
            readiness: Arc::new(FakeReadiness(dyno_core::wol::ThisPcWol::default())),
            secrets: Arc::new(paddock_secrets::MemorySecrets::new("Dyno")),
            clock: Arc::new(dyno_core::wol::fake::FakeClock::default()),
            wake_timer: Some(Arc::new(FakeWakeTimer::default())),
        }
    }
}

/// A runner that answers from a script, for the adapters' tests: `(program, first arg)` to the output.
#[cfg(test)]
pub(crate) mod script {
    use crate::runner::{Tool, ToolOutput, ToolRunner};
    use dyno_core::ports::PlatformError;

    /// Answers each call from `answers` by program name; records the calls.
    pub struct Script {
        pub answers: Vec<(&'static str, Result<ToolOutput, PlatformError>)>,
        pub calls: std::sync::Arc<std::sync::Mutex<Vec<Tool>>>,
    }

    pub fn out(stdout: &str) -> Result<ToolOutput, PlatformError> {
        Ok(ToolOutput {
            ok: true,
            stdout: stdout.into(),
            error: String::new(),
        })
    }

    pub fn fail(code_error: &str) -> Result<ToolOutput, PlatformError> {
        Ok(ToolOutput {
            ok: false,
            stdout: String::new(),
            error: code_error.into(),
        })
    }

    impl ToolRunner for Script {
        fn run_tool(&self, t: &Tool, _timeout: std::time::Duration) -> Result<ToolOutput, PlatformError> {
            self.calls.lock().unwrap().push(t.clone());
            let name = t.program.rsplit(['/', '\\']).next().unwrap_or("").to_string();
            for (program, answer) in &self.answers {
                if *program == name {
                    return answer.clone();
                }
            }
            Err(PlatformError::NotFound(format!("no such program: {name}")))
        }
    }
}
