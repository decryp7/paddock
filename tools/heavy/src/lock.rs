//! The lock itself, on the shell plock's on-disk layout (so shell and Rust interoperate):
//!
//! ```text
//! <root>/pitcrew-heavy-v2.lock/owner        the lock: a directory that appears atomically WITH its owner record
//! <root>/pitcrew-heavy-v2.lock.slotN        slots 2.. (only when HEAVY_SLOTS > 1; shell plock ignores them)
//! <root>/pitcrew-heavy.lock                 the old-style lock (honoured while it exists)
//! <root>/pitcrew-heavy.ticket.<us>-<pid>    FIFO queue tickets
//! <root>/pitcrew-heavy.high.<pid>-<name>    --high markers
//! <root>/pitcrew-lock-breaks.log            breaks and removals of stale things
//! <root>/pitcrew-lock-activity.log          waits, takes, dones
//! ```
//!
//! Every operation names an exact path; nothing is globbed into a delete. Every read tolerates the file vanishing.

use crate::clock::Clock;
use crate::config::Config;
use crate::gate::{LoadSource, load_text};
use crate::log::{Level, Logger};
use crate::procs::{Presence, Procs, evidence, pid_alive};
use crate::record::{self, Owner, OwnerRead};
use crate::timefmt::{clock_text, day_text, mins, oneline, utc_text};
use serde_json::{Value, json};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

/// The lock directory's name.
pub const LOCK_NAME: &str = "pitcrew-heavy-v2.lock";
/// The old-style lock directory's name.
pub const OLD_NAME: &str = "pitcrew-heavy.lock";
/// The breaks log's name.
pub const BREAKS_LOG: &str = "pitcrew-lock-breaks.log";
/// The activity log's name.
pub const ACTIVITY_LOG: &str = "pitcrew-lock-activity.log";
/// Ticket file name prefix.
pub const TICKET_PREFIX: &str = "pitcrew-heavy.ticket.";
/// High marker file name prefix.
pub const HIGH_PREFIX: &str = "pitcrew-heavy.high.";

/// Failures to inject (tests only; there is no environment switch in the program).
#[derive(Default)]
pub struct Faults {
    /// The next N owner writes fail (leaving an empty file).
    pub owner_fail: AtomicU32,
    /// The next N owner writes "succeed" but leave an empty file.
    pub owner_empty: AtomicU32,
    /// Ticket writes fail.
    pub ticket_fail: AtomicBool,
    /// Runs once inside a break, after the decision and before the rename (race tests).
    pub before_aside: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

fn take_one(c: &AtomicU32) -> bool {
    c.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok()
}

/// One participant: its settings, ports, identity and log.
pub struct Heavy {
    /// Settings.
    pub cfg: Config,
    /// Wall clock and sleeping.
    pub clock: Arc<dyn Clock>,
    /// The process table.
    pub procs: Arc<dyn Procs>,
    /// The load average.
    pub load: Arc<dyn LoadSource>,
    /// The troubleshooting log of this run.
    pub log: Arc<Logger>,
    /// This participant's pid (the real one, or a made-up one in tests).
    pub pid: u32,
    /// Where human-readable lines go (stderr in the program).
    pub say: Arc<dyn Fn(&str) + Send + Sync>,
    /// Injected failures.
    pub faults: Faults,
    seq: AtomicU64,
}

/// What a request to run is.
#[derive(Debug, Clone)]
pub struct Request {
    /// The agent name (one line).
    pub name: String,
    /// The working folder (one line).
    pub cwd: String,
    /// The masked command line.
    pub cmdline: String,
    /// Skip the queue and go first.
    pub high: bool,
}

/// The result of one attempt to take the lock.
#[derive(Debug, PartialEq, Eq)]
pub enum Poll {
    /// The lock is held.
    Held,
    /// Not yet; wait one poll interval.
    Waiting,
    /// Cannot proceed at all (message says what to fix).
    GaveUp(String),
}

enum Take {
    Taken,
    Busy,
    WriteFailed,
}

/// Identity of a directory on disk, to notice that it was replaced.
#[cfg(unix)]
fn file_id(p: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(p).ok().map(|m| (m.dev(), m.ino()))
}
#[cfg(not(unix))]
fn file_id(p: &Path) -> Option<(u64, u64)> {
    fs::metadata(p).ok().map(|_| (0, 0))
}

fn mtime(p: &Path) -> Option<u64> {
    fs::metadata(p)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

fn read_bounded(p: &Path) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    fs::File::open(p)?.take(record::MAX_RECORD as u64).read_to_end(&mut buf)?;
    Ok(buf)
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

impl Heavy {
    /// A participant. `say` receives the human-readable lines.
    pub fn new(
        cfg: Config,
        clock: Arc<dyn Clock>,
        procs: Arc<dyn Procs>,
        load: Arc<dyn LoadSource>,
        log: Arc<Logger>,
        pid: u32,
        say: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> Heavy {
        Heavy {
            cfg,
            clock,
            procs,
            load,
            log,
            pid,
            say,
            faults: Faults::default(),
            seq: AtomicU64::new(0),
        }
    }

    /// Now, in epoch seconds.
    pub fn now(&self) -> u64 {
        self.clock.now_secs()
    }

    /// The lock directory of slot `i` (0-based; slot 0 is the plock-compatible lock).
    pub fn slot_dir(&self, i: u32) -> PathBuf {
        if i == 0 {
            self.cfg.root.join(LOCK_NAME)
        } else {
            self.cfg.root.join(format!("{LOCK_NAME}.slot{}", i + 1))
        }
    }

    fn stamp(&self) -> String {
        let n = self.now();
        format!("{} {}", day_text(n), clock_text(n))
    }

    fn append(&self, name: &str, msg: &str) {
        let _ = fs::create_dir_all(&self.cfg.root);
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(self.cfg.root.join(name)) {
            use std::io::Write;
            let _ = writeln!(f, "{} {}", self.stamp(), msg);
        }
    }

    /// A line in the breaks log.
    pub fn blog(&self, msg: &str) {
        self.append(BREAKS_LOG, msg);
    }

    /// A line in the activity log.
    pub fn alog(&self, msg: &str) {
        self.append(ACTIVITY_LOG, msg);
    }

    fn age_of(&self, p: &Path) -> Option<u64> {
        mtime(p).map(|m| self.now().saturating_sub(m))
    }

    fn age_text(&self, p: &Path) -> String {
        self.age_of(p).map_or("unknown".into(), |a| format!("{a}s"))
    }

    /// The owner record in `dir`. Missing, unreadable or zero-byte is `Empty`.
    pub fn read_owner(&self, dir: &Path) -> OwnerRead {
        let f = dir.join("owner");
        match read_bounded(&f) {
            Ok(b) => record::parse_owner(&b, mtime(&f)),
            Err(_) => OwnerRead::Empty,
        }
    }

    fn start_text(&self) -> String {
        match self.procs.probe(self.pid) {
            Presence::Present { start_micros: Some(s) } => utc_text(s / 1_000_000),
            _ => String::new(),
        }
    }

    fn tmp_name(&self, kind: &str) -> PathBuf {
        let n = self.seq.fetch_add(1, Ordering::SeqCst);
        self.cfg
            .root
            .join(format!(".plock.{kind}{}.{}{n}", self.pid, self.clock.now_micros() % 1_000_000))
    }

    fn write_owner(&self, file: &Path, req: &Request, epoch: u64) -> bool {
        if take_one(&self.faults.owner_fail) {
            let _ = fs::write(file, b"");
            return false;
        }
        if take_one(&self.faults.owner_empty) {
            let _ = fs::write(file, b"");
            return true;
        }
        let o = Owner {
            pid: self.pid,
            start_text: self.start_text(),
            name: req.name.clone(),
            epoch: None,
            cwd: req.cwd.clone(),
            cmd: req.cmdline.clone(),
        };
        fs::write(file, record::format_owner(&o, epoch)).is_ok()
    }

    fn non_empty(p: &Path) -> bool {
        fs::metadata(p).is_ok_and(|m| m.len() > 0)
    }

    /// Publishes the lock directory with its owner record already inside (written aside, verified, renamed into place).
    fn try_take(&self, dir: &Path, req: &Request, epoch_out: &mut u64) -> Take {
        let tmp = self.tmp_name("");
        if fs::create_dir(&tmp).is_err() {
            return Take::WriteFailed;
        }
        let epoch = self.now();
        let ot = tmp.join("owner.tmp");
        let o = tmp.join("owner");
        let written = self.write_owner(&ot, req, epoch) && Self::non_empty(&ot) && fs::rename(&ot, &o).is_ok() && Self::non_empty(&o);
        if !written {
            let _ = fs::remove_dir_all(&tmp);
            return Take::WriteFailed;
        }
        // rename(2) of a directory onto an existing NON-EMPTY directory fails; the lock dir is never empty.
        if !dir.exists() && fs::rename(&tmp, dir).is_ok() {
            *epoch_out = epoch;
            return Take::Taken;
        }
        let _ = fs::remove_dir_all(&tmp);
        Take::Busy
    }

    /// Breaks the lock in `dir` only when its owner is dead or recycled (or the record is empty and the lock old). Renames it
    /// aside, re-checks identity and owner, and puts it back if either changed. Returns whether it was broken.
    pub fn break_if_stale(&self, dir: &Path, who: &str) -> bool {
        let Some(id) = file_id(dir) else { return false };
        let first = self.read_owner(dir);
        let (victim, why, evid, kind) = match &first {
            OwnerRead::Ok(o) => {
                if pid_alive(&*self.procs, o.pid, o.epoch) {
                    return false;
                }
                (
                    o.name.clone(),
                    format!("pid {} is gone, or recycled (started after the record was written)", o.pid),
                    evidence(&*self.procs, o.pid, o.epoch),
                    "dead owner",
                )
            }
            OwnerRead::Unparsable => return false,
            OwnerRead::Empty => {
                let Some(age) = self.age_of(dir) else { return false };
                if age < self.cfg.unreadable_limit {
                    return false;
                }
                (
                    "an unknown owner".to_string(),
                    format!(
                        "owner record empty or missing and lock {age}s old (limit {}s)",
                        self.cfg.unreadable_limit
                    ),
                    format!("no owner content; lock dir age {age}s"),
                    "empty owner",
                )
            }
        };
        let aside = self.tmp_name("break.");
        let hook = self.faults.before_aside.lock().ok().and_then(|mut h| h.take());
        if let Some(h) = hook {
            h();
        }
        if fs::rename(dir, &aside).is_err() {
            return false;
        }
        let second = self.read_owner(&aside);
        let same = file_id(&aside) == Some(id)
            && match (&first, &second) {
                (OwnerRead::Ok(a), OwnerRead::Ok(b)) => a.pid == b.pid && a.epoch == b.epoch,
                (OwnerRead::Empty, OwnerRead::Empty) => true,
                _ => false,
            };
        if same {
            self.blog(&format!("{who} broke a stale lock of {victim} ({why})"));
            self.log.warn(
                "heavy.break",
                &format!("{who} broke a stale lock of {victim}"),
                &[
                    ("Kind", json!(kind)),
                    ("Why", json!(why)),
                    ("Evidence", json!(evid)),
                    ("Lock", json!(dir.to_string_lossy())),
                ],
            );
            let _ = fs::remove_dir_all(&aside);
            return true;
        }
        if !dir.exists() && fs::rename(&aside, dir).is_ok() {
            self.log.warn(
                "heavy.break",
                "the lock changed while it was being broken; put back, not broken",
                &[("Evidence", json!(evid)), ("Lock", json!(dir.to_string_lossy()))],
            );
            return false;
        }
        self.blog(&format!(
            "{who} found the lock changed while breaking it and could not put it back; left at {}",
            aside.display()
        ));
        self.log.event(
            Level::Error,
            "heavy.break",
            "could not put a changed lock back",
            &[("LeftAt", json!(aside.to_string_lossy()))],
        );
        false
    }

    fn dir_entries(&self, prefix: &str) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = match fs::read_dir(&self.cfg.root) {
            Ok(rd) => rd
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| file_name(p).starts_with(prefix))
                .collect(),
            Err(_) => Vec::new(),
        };
        v.sort_by_key(|p| file_name(p));
        v
    }

    /// The suffix of the first live high marker's name (markers of dead processes are removed).
    pub fn live_high_marker(&self, who: &str) -> Option<String> {
        for f in self.dir_entries(HIGH_PREFIX) {
            let b = file_name(&f);
            let suffix = &b[HIGH_PREFIX.len()..];
            let bytes = match read_bounded(&f) {
                Ok(b) => b,
                Err(_) => continue, // vanished: nothing to judge
            };
            let ep = record::parse_marker_epoch(&bytes).or_else(|| mtime(&f));
            let pid = record::marker_pid(suffix);
            if pid.is_some_and(|p| pid_alive(&*self.procs, p, ep)) {
                return Some(suffix.to_string());
            }
            let evid = pid.map_or("pid not a number".to_string(), |p| evidence(&*self.procs, p, ep));
            self.blog(&format!("{who} removed the stale high-priority marker {b}"));
            self.log.warn(
                "heavy.stale",
                &format!("removed the stale high-priority marker {b}"),
                &[("Evidence", json!(evid))],
            );
            let _ = fs::remove_file(&f);
        }
        None
    }

    /// Prunes dead tickets; returns (my position 1-based or 0, queue length, name of the head).
    pub fn queue(&self, ticket: &Path, who: &str) -> (usize, usize, String) {
        let (mut pos, mut len, mut head) = (0, 0, String::new());
        for f in self.dir_entries(TICKET_PREFIX) {
            let b = file_name(&f);
            if b.ends_with(".tmp") {
                if self.age_of(&f).is_some_and(|a| a >= self.cfg.unreadable_limit) {
                    let _ = fs::remove_file(&f);
                }
                continue;
            }
            let bytes = match read_bounded(&f) {
                Ok(b) => b,
                Err(_) => continue, // vanished between listing and reading
            };
            let t = record::parse_ticket(&bytes);
            let mut name = t.name.clone();
            if t.pid_text.is_empty() {
                if self.age_of(&f).is_some_and(|a| a >= self.cfg.unreadable_limit) {
                    self.blog(&format!("{who} removed the unreadable ticket {b}"));
                    self.log.warn(
                        "heavy.stale",
                        &format!("removed the unreadable ticket {b}"),
                        &[("Age", json!(self.age_text(&f)))],
                    );
                    let _ = fs::remove_file(&f);
                    continue;
                }
                name = "?".into();
            } else {
                let ep = t.epoch.or_else(|| mtime(&f));
                if !t.pid.is_some_and(|p| pid_alive(&*self.procs, p, ep)) {
                    let evid = t.pid.map_or("pid not a number".to_string(), |p| evidence(&*self.procs, p, ep));
                    self.blog(&format!(
                        "{who} removed the stale ticket {b} of {} (pid {} gone or recycled)",
                        t.name, t.pid_text
                    ));
                    self.log.warn(
                        "heavy.stale",
                        &format!("removed the stale ticket {b} of {}", t.name),
                        &[("Evidence", json!(evid))],
                    );
                    let _ = fs::remove_file(&f);
                    continue;
                }
            }
            len += 1;
            if head.is_empty() {
                head = name;
            }
            if f == ticket {
                pos = len;
            }
        }
        (pos, len, head)
    }

    /// The text for "who holds it" in a wait line, with the log properties.
    fn holder_note(&self, dir: &Path) -> (String, Vec<(&'static str, Value)>) {
        match self.read_owner(dir) {
            OwnerRead::Ok(o) => {
                let cmd = if o.cmd.is_empty() {
                    String::new()
                } else {
                    format!(", running: {}", o.cmd)
                };
                (
                    format!(
                        "the lock is held by {} (pid {}) for {}{cmd}",
                        o.name,
                        o.pid,
                        mins(self.now(), o.epoch)
                    ),
                    vec![
                        ("Reason", json!("held")),
                        ("HolderName", json!(o.name)),
                        ("HolderPid", json!(o.pid)),
                        ("HolderStart", json!(o.start_text)),
                        ("RecordEpoch", json!(o.epoch)),
                        ("Evidence", json!(evidence(&*self.procs, o.pid, o.epoch))),
                    ],
                )
            }
            OwnerRead::Empty => (
                format!("the lock dir has an empty owner record (age {})", self.age_text(dir)),
                vec![("Reason", json!("empty-owner")), ("Age", json!(self.age_text(dir)))],
            ),
            OwnerRead::Unparsable => (
                "the lock dir has an unparsable owner record (not broken automatically)".into(),
                vec![("Reason", json!("unparsable-owner"))],
            ),
        }
    }

    /// The load gate for one check: Ok(None) open, Ok(Some(text)) closed.
    pub fn gate_closed(&self) -> Option<String> {
        let l = self.load.load()?;
        (l[0] >= self.cfg.gate_load).then(|| format!("load gate, 1-minute load {:.2} >= {}", l[0], self.cfg.gate_load))
    }

    /// The load text, or a note that there is none.
    pub fn load_now(&self) -> String {
        self.load.load().map_or("unavailable on this platform".into(), load_text)
    }

    /// Sleeps `d` in short steps so a cancel is noticed; returns false if `stop` became true.
    pub fn nap(&self, d: Duration, stop: &dyn Fn() -> bool) -> bool {
        let mut left = d;
        while !left.is_zero() {
            if stop() {
                return false;
            }
            let step = left.min(Duration::from_millis(100));
            self.clock.sleep(step);
            left -= step;
        }
        !stop()
    }

    /// The text of `status`.
    pub fn status(&self) -> String {
        let mut out = String::new();
        let multi = self.cfg.slots > 1;
        for i in 0..self.cfg.slots {
            let dir = self.slot_dir(i);
            let pre = if multi { format!("slot {}: ", i + 1) } else { String::new() };
            if dir.is_dir() {
                match self.read_owner(&dir) {
                    OwnerRead::Ok(o) => {
                        let alive = if pid_alive(&*self.procs, o.pid, o.epoch) { "yes" } else { "NO" };
                        out += &format!(
                            "{pre}held by {} pid {} since '{}' for {} (alive: {alive})\n",
                            o.name,
                            o.pid,
                            o.start_text,
                            mins(self.now(), o.epoch)
                        );
                        if !o.cmd.is_empty() {
                            out += &format!("  command: {}\n", o.cmd);
                        }
                        if !o.cwd.is_empty() {
                            out += &format!("  cwd: {}\n", o.cwd);
                        }
                    }
                    OwnerRead::Empty => {
                        out += &format!(
                            "{pre}lock dir with an empty owner record (age {}; broken after {}s)\n",
                            self.age_text(&dir),
                            self.cfg.unreadable_limit
                        );
                    }
                    OwnerRead::Unparsable => {
                        out += &format!(
                            "{pre}lock dir with an UNPARSABLE owner record (never broken automatically; inspect {}/owner)\n",
                            dir.display()
                        );
                    }
                }
            } else {
                out += &format!("{pre}free\n");
            }
        }
        let old = self.cfg.root.join(OLD_NAME);
        if old.is_dir() {
            out += &format!("old-style lock {} exists (an agent still on the old script)\n", old.display());
        }
        for f in self.dir_entries(TICKET_PREFIX) {
            if file_name(&f).ends_with(".tmp") {
                continue;
            }
            if let Ok(b) = read_bounded(&f) {
                let t = record::parse_ticket(&b);
                out += &format!("queued: {} pid {}\n", t.name, t.pid_text);
            }
        }
        for f in self.dir_entries(HIGH_PREFIX) {
            out += &format!("high marker: {}\n", f.display());
        }
        for (label, name, n) in [("breaks (last 5):", BREAKS_LOG, 5), ("activity (last 3):", ACTIVITY_LOG, 3)] {
            if let Ok(text) = fs::read_to_string(self.cfg.root.join(name)) {
                let lines: Vec<&str> = text.lines().collect();
                out += &format!("{label}\n");
                for l in &lines[lines.len().saturating_sub(n)..] {
                    out += &format!("{l}\n");
                }
            }
        }
        out
    }
}

/// One run's claim on the lock: its ticket or marker, and the slot it holds.
pub struct Acquirer<'a> {
    h: &'a Heavy,
    /// The request.
    pub req: Request,
    ticket: PathBuf,
    marker: Option<PathBuf>,
    held: Option<u32>,
    epoch: u64,
    fails: u32,
    t0: u64,
    t0_micros: u64,
    last_note: u64,
    last_reason: String,
    waited_micros: u64,
}

impl<'a> Acquirer<'a> {
    /// Queues (a ticket) or announces itself (a high marker). Err: nothing could be written; no lock was taken.
    pub fn new(h: &'a Heavy, req: Request) -> Result<Acquirer<'a>, String> {
        let _ = fs::create_dir_all(&h.cfg.root);
        let t0 = h.now();
        let mut a = Acquirer {
            h,
            req,
            ticket: PathBuf::from("/nonexistent.ticket"),
            marker: None,
            held: None,
            epoch: 0,
            fails: 0,
            t0,
            t0_micros: h.clock.now_micros(),
            last_note: t0,
            last_reason: String::new(),
            waited_micros: 0,
        };
        if a.req.high {
            let m = h
                .cfg
                .root
                .join(format!("{HIGH_PREFIX}{}-{}", h.pid, record::sanitize_name(&a.req.name)));
            let tmp = h.tmp_name("high.");
            let ok = fs::write(&tmp, record::format_marker(h.pid, &h.start_text(), h.now())).is_ok() && fs::rename(&tmp, &m).is_ok();
            if !ok {
                let _ = fs::remove_file(&tmp);
                return Err(format!(
                    "cannot write a high-priority marker in {} (is it writable and not full?); giving up, no lock taken",
                    h.cfg.root.display()
                ));
            }
            a.marker = Some(m);
        } else {
            a.make_ticket()?;
        }
        h.log.info(
            "heavy.queue",
            if a.req.high { "announced a high-priority run" } else { "queued" },
            &[("Agent", json!(a.req.name)), ("High", json!(a.req.high))],
        );
        Ok(a)
    }

    fn make_ticket(&mut self) -> Result<(), String> {
        let h = self.h;
        let n = h.clock.now_micros();
        let ticket = h.cfg.root.join(format!("{TICKET_PREFIX}{n:016}-{}", h.pid));
        let tmp = PathBuf::from(format!("{}.tmp", ticket.display()));
        let body = record::format_ticket(h.pid, &h.start_text(), &self.req.name, h.now());
        let ok = !h.faults.ticket_fail.load(Ordering::SeqCst)
            && fs::write(&tmp, body).is_ok()
            && Heavy::non_empty(&tmp)
            && fs::rename(&tmp, &ticket).is_ok();
        if !ok {
            let _ = fs::remove_file(&tmp);
            return Err(format!(
                "cannot write a queue ticket in {} (is it writable and not full?); giving up, no lock taken",
                h.cfg.root.display()
            ));
        }
        self.ticket = ticket;
        Ok(())
    }

    fn due(&self) -> bool {
        self.h.now().saturating_sub(self.last_note) as f64 >= self.h.cfg.wait_log.as_secs_f64()
    }

    /// A wait reason: logged to the CLEF log when it changes, and said (stderr + activity log) every `wait_log` seconds.
    fn wait_for(&mut self, reason: &str, msg: &str, props: &[(&'static str, Value)]) {
        let h = self.h;
        let waited = h.now().saturating_sub(self.t0);
        if self.last_reason != reason {
            self.last_reason = reason.to_string();
            let mut p: Vec<(&str, Value)> = vec![("WaitedSeconds", json!(waited))];
            p.extend(props.iter().map(|(k, v)| (*k, v.clone())));
            h.log.info("heavy.wait", &format!("waiting: {msg}"), &p);
        }
        if self.due() {
            let t = h.now();
            (h.say)(&format!("{} waiting {waited}s: {msg}", self.req.name));
            h.alog(&format!("{} waiting {waited}s: {msg}", self.req.name));
            let mut p: Vec<(&str, Value)> = vec![("WaitedSeconds", json!(waited))];
            p.extend(props.iter().map(|(k, v)| (*k, v.clone())));
            h.log.info("heavy.wait", &format!("still waiting: {msg}"), &p);
            self.last_note = t;
        }
    }

    /// One attempt. Call again after a poll interval while it says `Waiting`.
    pub fn poll(&mut self) -> Poll {
        let h = self.h;
        let name = self.req.name.clone();
        'again: loop {
            if self.held.is_some() {
                return Poll::Held;
            }
            if !self.req.high
                && let Some(hm) = h.live_high_marker(&name)
            {
                self.wait_for(
                    "high",
                    &format!("a high-priority run goes first ({hm})"),
                    &[("Reason", json!("high-priority")), ("Marker", json!(hm))],
                );
                return Poll::Waiting;
            }
            let old = h.cfg.root.join(OLD_NAME);
            if old.is_dir() {
                self.wait_for(
                    "old",
                    &format!("the old-style lock {} is held (an agent still on the old script)", old.display()),
                    &[("Reason", json!("old-style-lock"))],
                );
                return Poll::Waiting;
            }
            if !self.req.high {
                if !self.ticket.is_file()
                    && let Err(e) = self.make_ticket()
                {
                    return Poll::GaveUp(e);
                }
                let (pos, len, head) = h.queue(&self.ticket, &name);
                if pos != 1 {
                    self.wait_for(
                        "queue",
                        &format!("queued, position {pos} of {len}, behind {head}"),
                        &[
                            ("Reason", json!("queued")),
                            ("Position", json!(pos)),
                            ("QueueLength", json!(len)),
                            ("Head", json!(head)),
                        ],
                    );
                    return Poll::Waiting;
                }
            }
            let mut busy: Option<PathBuf> = None;
            for i in 0..h.cfg.slots {
                let dir = h.slot_dir(i);
                if dir.exists() {
                    if h.break_if_stale(&dir, &name) {
                        continue 'again;
                    }
                    busy.get_or_insert(dir);
                    continue;
                }
                let t = h.clock.now_micros();
                match h.try_take(&dir, &self.req, &mut self.epoch) {
                    Take::Taken => {
                        self.held = Some(i);
                        self.waited_micros = h.clock.now_micros().saturating_sub(self.t0_micros);
                        h.log.info(
                            "heavy.phase",
                            "took the lock",
                            &[
                                ("Phase", json!("take")),
                                ("DurationMs", json!(h.clock.now_micros().saturating_sub(t) / 1000)),
                                ("Slot", json!(i + 1)),
                                ("RecordEpoch", json!(self.epoch)),
                            ],
                        );
                        h.log.info(
                            "heavy.phase",
                            "queue wait over",
                            &[("Phase", json!("queue_wait")), ("DurationMs", json!(self.waited_micros / 1000))],
                        );
                        if let Some(m) = self.marker.take() {
                            let _ = fs::remove_file(m);
                        }
                        let _ = fs::remove_file(&self.ticket);
                        return Poll::Held;
                    }
                    Take::Busy => {
                        busy.get_or_insert(dir);
                    }
                    Take::WriteFailed => {
                        self.fails += 1;
                        (h.say)(&format!(
                            "{name} could not write the lock owner record (attempt {} of 3)",
                            self.fails
                        ));
                        h.log.event(
                            Level::Error,
                            "heavy.take",
                            "could not write the lock owner record",
                            &[("Attempt", json!(self.fails)), ("Root", json!(h.cfg.root.to_string_lossy()))],
                        );
                        if self.fails >= 3 {
                            h.blog(&format!(
                                "{name} gave up: owner record could not be written in {}",
                                h.cfg.root.display()
                            ));
                            return Poll::GaveUp(format!(
                                "giving up: no lock taken (is {} writable and not full?)",
                                h.cfg.root.display()
                            ));
                        }
                        return Poll::Waiting;
                    }
                }
            }
            if let Some(dir) = busy {
                let (msg, props) = h.holder_note(&dir);
                let reason = format!("{}:{}", msg.split(" for ").next().unwrap_or(""), file_name(&dir));
                self.wait_for(&reason, &msg, &props);
            }
            return Poll::Waiting;
        }
    }

    /// Microseconds spent waiting for the lock (valid once held).
    pub fn waited_micros(&self) -> u64 {
        self.waited_micros
    }

    /// Seconds since this claim began.
    pub fn t0(&self) -> u64 {
        self.t0
    }

    /// Whether the lock is held.
    pub fn is_held(&self) -> bool {
        self.held.is_some()
    }

    /// Gives everything back: the marker, the ticket, and the lock if the owner record is still ours.
    pub fn release(&mut self) {
        let h = self.h;
        if let Some(m) = self.marker.take() {
            let _ = fs::remove_file(m);
        }
        let _ = fs::remove_file(&self.ticket);
        let _ = fs::remove_file(PathBuf::from(format!("{}.tmp", self.ticket.display())));
        if let Some(i) = self.held.take() {
            let dir = h.slot_dir(i);
            match h.read_owner(&dir) {
                OwnerRead::Ok(o) if o.pid == h.pid && o.epoch == Some(self.epoch) => {
                    let _ = fs::remove_dir_all(&dir);
                    h.log.info("heavy.release", "released the lock", &[("Slot", json!(i + 1))]);
                }
                other => h.log.warn(
                    "heavy.release",
                    "the lock is no longer ours; left alone",
                    &[("Slot", json!(i + 1)), ("Found", json!(format!("{other:?}")))],
                ),
            }
        }
    }
}

impl Drop for Acquirer<'_> {
    fn drop(&mut self) {
        self.release();
    }
}

/// `oneline` for names handed in from the command line.
pub fn clean_name(s: &str) -> String {
    oneline(s)
}
