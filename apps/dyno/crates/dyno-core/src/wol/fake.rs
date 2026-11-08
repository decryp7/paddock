//! Fakes for the Wake-on-LAN ports: they record and script, and touch no socket, table or process (ER-14). Used by this
//! crate's tests, the agent's integration tests and the fake platform; never by real wiring.

use super::neighbours::Neighbour;
use super::plan::NetIf;
use super::readiness::ThisPcWol;
use super::wake::{NeighbourSource, Pacer, Prober, UdpBroadcast, WakePhase};
use super::{Interfaces, ReadinessSource, WolClock};
use crate::ports::PlatformError;
use std::net::Ipv4Addr;
use std::sync::{Mutex, MutexGuard, PoisonError};

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One datagram a [`FakeUdp`] was asked to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentDatagram {
    /// The bound source address.
    pub source: Ipv4Addr,
    /// The destination.
    pub dest: Ipv4Addr,
    /// The port.
    pub port: u16,
    /// The payload size.
    pub len: usize,
    /// The first payload bytes (the sync stream and the address start), for assertions.
    pub payload: Vec<u8>,
}

/// Records datagrams instead of sending them.
#[derive(Default)]
pub struct FakeUdp {
    sent: Mutex<Vec<SentDatagram>>,
    fail_dest: Option<(Ipv4Addr, String)>,
    fail_all: Option<String>,
}

impl FakeUdp {
    /// Fails every send to `dest` with `error`.
    pub fn failing_for(dest: Ipv4Addr, error: &str) -> FakeUdp {
        FakeUdp {
            fail_dest: Some((dest, error.into())),
            ..FakeUdp::default()
        }
    }

    /// Fails every send with `error`.
    pub fn failing_all(error: &str) -> FakeUdp {
        FakeUdp {
            fail_all: Some(error.into()),
            ..FakeUdp::default()
        }
    }

    /// What was sent, in order.
    pub fn sent(&self) -> Vec<SentDatagram> {
        locked(&self.sent).clone()
    }
}

impl UdpBroadcast for FakeUdp {
    fn send(&self, source: Ipv4Addr, dest: Ipv4Addr, port: u16, payload: &[u8]) -> Result<(), String> {
        if let Some(e) = &self.fail_all {
            return Err(e.clone());
        }
        if let Some((d, e)) = &self.fail_dest {
            if *d == dest {
                return Err(e.clone());
            }
        }
        locked(&self.sent).push(SentDatagram {
            source,
            dest,
            port,
            len: payload.len(),
            payload: payload.to_vec(),
        });
        Ok(())
    }
}

/// A neighbour table that answers a script: the n-th read returns the n-th table, the last repeats.
#[derive(Default)]
pub struct FakeNeighbours {
    script: Mutex<Vec<Vec<Neighbour>>>,
    reads: Mutex<usize>,
    fail: bool,
}

impl FakeNeighbours {
    /// A table that never changes.
    pub fn fixed(table: Vec<Neighbour>) -> FakeNeighbours {
        FakeNeighbours::scripted(vec![table])
    }

    /// The n-th read answers the n-th table.
    pub fn scripted(tables: Vec<Vec<Neighbour>>) -> FakeNeighbours {
        FakeNeighbours {
            script: Mutex::new(tables),
            ..FakeNeighbours::default()
        }
    }

    /// A table the OS refuses to give.
    pub fn failing() -> FakeNeighbours {
        FakeNeighbours {
            fail: true,
            ..FakeNeighbours::default()
        }
    }
}

impl NeighbourSource for FakeNeighbours {
    fn neighbours(&self) -> Result<Vec<Neighbour>, PlatformError> {
        if self.fail {
            return Err(PlatformError::Unsupported("the fake table is unreadable".into()));
        }
        let script = locked(&self.script);
        let mut n = locked(&self.reads);
        let table = script.get(*n).or_else(|| script.last()).cloned().unwrap_or_default();
        *n += 1;
        Ok(table)
    }
}

/// An echo that answers a script of yes/no, then no.
#[derive(Default)]
pub struct FakeProber {
    script: Mutex<Vec<bool>>,
    calls: Mutex<usize>,
    error: bool,
}

impl FakeProber {
    /// The n-th ping answers the n-th value; later ones are no.
    pub fn scripted(answers: &[bool]) -> FakeProber {
        FakeProber {
            script: Mutex::new(answers.to_vec()),
            ..FakeProber::default()
        }
    }

    /// A ping the OS cannot do.
    pub fn erroring() -> FakeProber {
        FakeProber {
            error: true,
            ..FakeProber::default()
        }
    }

    /// How many pings were asked for.
    pub fn calls(&self) -> usize {
        *locked(&self.calls)
    }
}

impl Prober for FakeProber {
    fn ping(&self, _ip: &str) -> Result<bool, String> {
        if self.error {
            return Err("ping is not available".into());
        }
        let mut n = locked(&self.calls);
        let answer = locked(&self.script).get(*n).copied().unwrap_or(false);
        *n += 1;
        Ok(answer)
    }
}

/// A clock that only counts the pauses it was asked for, and can be cancelled at the n-th pause.
#[derive(Default)]
pub struct ManualPacer {
    /// Every pause asked for, in seconds.
    pub pauses: Vec<u32>,
    /// The last progress reported.
    pub last_progress: Option<(WakePhase, u32, Option<u32>)>,
    cancel_at: Option<usize>,
    elapsed: u32,
}

impl ManualPacer {
    /// Cancels when the `n`-th (1-based) pause is asked for.
    pub fn cancelling_at_pause(n: usize) -> ManualPacer {
        ManualPacer {
            cancel_at: Some(n),
            ..ManualPacer::default()
        }
    }
}

impl Pacer for ManualPacer {
    fn pause(&mut self, secs: u32) -> bool {
        self.pauses.push(secs);
        if self.cancel_at == Some(self.pauses.len()) {
            return false;
        }
        self.elapsed += secs;
        true
    }

    fn elapsed_secs(&self) -> u32 {
        self.elapsed
    }

    fn progress(&mut self, phase: WakePhase, done: u32, total: Option<u32>) {
        self.last_progress = Some((phase, done, total));
    }
}

/// A fixed list of cards.
pub struct FakeInterfaces(pub Vec<NetIf>);

impl Interfaces for FakeInterfaces {
    fn ipv4(&self) -> Vec<NetIf> {
        self.0.clone()
    }
}

/// A fixed readiness answer.
pub struct FakeReadiness(pub ThisPcWol);

impl ReadinessSource for FakeReadiness {
    fn read(&self) -> ThisPcWol {
        self.0.clone()
    }
}

/// A clock that advances only when slept on: a wake that "waits two minutes" takes no time (ER-12).
#[derive(Default)]
pub struct FakeClock(Mutex<std::time::Duration>);

impl WolClock for FakeClock {
    fn now(&self) -> std::time::Duration {
        *locked(&self.0)
    }

    fn sleep(&self, d: std::time::Duration) {
        *locked(&self.0) += d;
    }
}
