//! A fake world: a clock that only moves when told to, a process table made by hand, a load made by hand; the files are real
//! (in a temp folder), so the on-disk layout is what is tested.
#![allow(dead_code)]

use heavy::clock::{Clock, FakeClock};
use heavy::config::Config;
use heavy::gate::FakeLoad;
use heavy::lock::{Acquirer, Heavy, Poll, Request};
use heavy::log::Logger;
use heavy::procs::FakeProcs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct Rig {
    pub dir: tempfile::TempDir,
    pub clock: Arc<FakeClock>,
    pub procs: Arc<FakeProcs>,
    pub load: Arc<FakeLoad>,
}

pub struct Agent {
    pub h: Heavy,
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
        let (mut c, _) = Config::from_env(&|_| None, false);
        c.root = self.root().to_path_buf();
        c.poll = Duration::from_millis(200);
        c.wait_log = Duration::from_secs(1);
        c.unreadable_limit = 3;
        c
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
        let h = Heavy::new(
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

    pub fn lock_dir(&self) -> PathBuf {
        self.root().join("pitcrew-heavy-v2.lock")
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

    /// Writes a lock dir with an owner file.
    pub fn plant_lock(&self, owner: &str) {
        std::fs::create_dir(self.lock_dir()).expect("mkdir");
        std::fs::write(self.lock_dir().join("owner"), owner).expect("owner");
    }

    pub fn owner_line(&self, n: usize) -> String {
        std::fs::read_to_string(self.lock_dir().join("owner"))
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
    }
}

pub fn req_high(name: &str) -> Request {
    Request { high: true, ..req(name) }
}

/// Polls until held or `max` polls (bounded), sleeping the poll interval between. Returns the last poll.
pub fn acquire(a: &mut Acquirer<'_>, h: &Heavy, max: u32) -> Poll {
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
