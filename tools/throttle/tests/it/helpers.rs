//! A fake world: a clock that only moves when told to, a process table made by hand, a load made by hand; the files are real
//! (in a temp folder), so the on-disk layout is what is tested.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use throttle::clock::{Clock, FakeClock};
use throttle::config::Config;
use throttle::gate::FakeLoad;
use throttle::lock::{Acquirer, Poll, Request, Throttle};
use throttle::log::Logger;
use throttle::procs::FakeProcs;

pub struct Rig {
    pub dir: tempfile::TempDir,
    pub clock: Arc<FakeClock>,
    pub procs: Arc<FakeProcs>,
    pub load: Arc<FakeLoad>,
}

/// Plants holders in a lock folder (see [`Rig::plant_slot`]).
#[derive(Clone)]
pub struct Planter {
    root: PathBuf,
}

impl Planter {
    pub fn plant_slot(&self, n: u32, owner: &str) -> throttle::slotfile::Held {
        let held = throttle::slotfile::take(&self.root.join(format!("throttle-slot{n}.lock")))
            .expect("lock file")
            .expect("free");
        std::fs::write(self.root.join(format!("throttle-slot{n}.info")), owner).expect("info");
        held
    }
}

pub struct Agent {
    pub h: Throttle,
    pub said: Arc<Mutex<Vec<String>>>,
}

impl Agent {
    pub fn said(&self) -> String {
        self.said.lock().map(|s| s.join("\n")).unwrap_or_default()
    }
}

impl Rig {
    pub fn new() -> Rig {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(1_800_000_000);
        Rig {
            dir: tempfile::tempdir().expect("tempdir"),
            clock: Arc::new(FakeClock::at_secs(now)),
            procs: Arc::new(FakeProcs::new()),
            load: Arc::new(FakeLoad::new(1.0)),
        }
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    pub fn cfg(&self) -> Config {
        let (mut c, _) = Config::from_env(&|_| None, &throttle::places::Host::none());
        c.root = self.root().to_path_buf();
        c.poll = Duration::from_millis(200);
        c.wait_log = Duration::from_secs(1);
        c.unreadable_limit = 3;
        // One at a time unless a test asks for more (slots) on a machine of 16 cores.
        c.slots = 1;
        c.cpus = 16;
        c
    }

    /// One poll after the fake clock has passed the wait-line rate limit (`wait_log`): a wait line is said at most once per `wait_log`, so a
    /// test that reads `said()` right after the first poll sees nothing. Use this, not a hand-picked `advance`, before asserting a wait line.
    pub fn poll_said(&self, a: &mut Acquirer<'_>) -> Poll {
        let wait_log = self.cfg().wait_log;
        self.clock.advance(wait_log + Duration::from_secs(1));
        a.poll()
    }

    pub fn now(&self) -> u64 {
        self.clock.now_secs()
    }

    /// A participant with a made-up pid that "runs" since 100 s ago.
    pub fn agent(&self, pid: u32) -> Agent {
        self.agent_with(pid, self.cfg())
    }

    pub fn agent_with(&self, pid: u32, cfg: Config) -> Agent {
        self.procs.run(pid, self.now().saturating_sub(100));
        let said = Arc::new(Mutex::new(Vec::new()));
        let s2 = Arc::clone(&said);
        let log = Arc::new(Logger::new(
            format!("run-{pid}"),
            pid,
            self.clock.clone(),
            Some(self.root().join(format!("log-{pid}.clef"))),
            false,
        ));
        let h = Throttle::new(
            cfg,
            self.clock.clone(),
            self.procs.clone(),
            self.load.clone(),
            log,
            pid,
            Arc::new(move |m: &str| {
                if let Ok(mut v) = s2.lock() {
                    v.push(m.to_string());
                }
            }),
        );
        Agent { h, said }
    }

    /// The info record of slot 1: there while a holder holds the slot, gone after a clean release.
    pub fn lock_dir(&self) -> PathBuf {
        self.root().join("throttle-slot1.info")
    }

    /// How many slots with this file name prefix (`throttle-slot`, `throttle-cheap`, `throttle-`) have a holder's record now.
    pub fn held(&self, prefix: &str) -> usize {
        std::fs::read_dir(self.root())
            .map(|r| {
                r.filter_map(Result::ok)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.starts_with(prefix) && n.ends_with(".info"))
                    .count()
            })
            .unwrap_or(0)
    }

    pub fn breaks_log(&self) -> String {
        std::fs::read_to_string(self.root().join("pitcrew-lock-breaks.log")).unwrap_or_default()
    }

    pub fn activity_log(&self) -> String {
        std::fs::read_to_string(self.root().join("pitcrew-lock-activity.log")).unwrap_or_default()
    }

    /// Entries of the root whose name starts with `prefix`.
    pub fn count(&self, prefix: &str) -> usize {
        std::fs::read_dir(self.root())
            .map(|r| {
                r.filter_map(Result::ok)
                    .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
                    .count()
            })
            .unwrap_or(0)
    }

    /// Makes slot 1 held by "somebody else" with this info record: the kernel lock is taken here and kept while the returned handle lives.
    pub fn plant_holder(&self, owner: &str) -> throttle::slotfile::Held {
        self.plant_slot(1, owner)
    }

    /// The same for slot `n` (1-based).
    pub fn plant_slot(&self, n: u32, owner: &str) -> throttle::slotfile::Held {
        self.clone_paths().plant_slot(n, owner)
    }

    /// A handle to plant holders from a closure that must be `'static` (a fault hook).
    pub fn clone_paths(&self) -> Planter {
        Planter {
            root: self.root().to_path_buf(),
        }
    }

    pub fn owner_line(&self, n: usize) -> String {
        std::fs::read_to_string(self.lock_dir())
            .unwrap_or_default()
            .lines()
            .nth(n)
            .unwrap_or("")
            .to_string()
    }
}

pub fn req(name: &str) -> Request {
    Request {
        name: name.into(),
        cwd: "/work".into(),
        cmdline: "cargo test".into(),
        high: false,
        expected_secs: None,
        row: None,
    }
}

pub fn req_high(name: &str) -> Request {
    Request { high: true, ..req(name) }
}

/// Polls until held or `max` polls (bounded), sleeping the poll interval between. Returns the last poll.
pub fn acquire(a: &mut Acquirer<'_>, h: &Throttle, max: u32) -> Poll {
    let mut last = Poll::Waiting;
    for _ in 0..max {
        last = a.poll();
        if last != Poll::Waiting {
            return last;
        }
        h.clock.sleep(h.cfg.poll);
    }
    last
}

/// One poll.
pub fn once(a: &mut Acquirer<'_>) -> Poll {
    a.poll()
}
