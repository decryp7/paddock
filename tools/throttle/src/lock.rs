//! The slots, on kernel file locks, and the queue around them.
//!
//! ```text
//! <root>/throttle-slotN.lock               slot N (1..): held by an open file with an exclusive lock; the kernel frees it on exit or crash
//! <root>/throttle-slotN.info               who holds it (the owner record), written by the holder after it took the lock, removed before release
//! <root>/throttle-cheapN.lock / .info      the quick lane: rows expected under `CHEAP_SECS`, beside the full slots (at most `CHEAP_LANE`)
//! <root>/pitcrew-heavy.ticket.<us>-<pid>    FIFO queue tickets
//! <root>/pitcrew-heavy.high.<pid>-<name>    --high markers
//! <root>/pitcrew-lock-breaks.log        removals of stale tickets and markers
//! <root>/pitcrew-lock-activity.log      waits, takes, dones
//! ```
//!
//! That a slot is held is the kernel's word ([`crate::slotfile`]); the info record is only for the wait line, `status` and the admission
//! rule (a container run, the order of holders). A record of a slot that is free is ignored. Nothing decides whether a holder is
//! alive, and nothing breaks a lock. The pure rules are in [`crate::core`].
//!
//! Every operation names an exact path; nothing is globbed into a delete. Every read tolerates the file vanishing.

use crate::clock::Clock;
use crate::config::Config;
use crate::core::{self, Holder, Inputs};
use crate::fsinfo::{self, FsKind};
use crate::gate::{CHEAP_LANE, LoadSource, Pressure, is_container_command, load_text};
use crate::log::{Level, Logger};
use crate::procs::{Presence, Procs, evidence, pid_alive};
use crate::record::{self, Owner, OwnerRead};
use crate::slotfile::{self, Held};
use crate::timefmt::{clock_text, day_text, mins, oneline, utc_text};
use crate::waitfor::{self, Break, Role, Rules, Waiter};
use paddock_proc::queue_stall::{Holder as QueueHolder, QueueView, within_cap};
use serde_json::{Value, json};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

/// The file name prefix of a full slot: `<prefix><N>.lock` (the lock) and `<prefix><N>.info` (the owner record).
pub const SLOT_PREFIX: &str = "throttle-slot";
/// The file name prefix of a quick-lane place.
pub const CHEAP_PREFIX: &str = "throttle-cheap";
/// Slot indexes from here are the quick lane's: `CHEAP_BASE + j` is the place `throttle-cheap<j+1>`. Above the most slots a setting allows (64).
pub const CHEAP_BASE: u32 = 64;
/// The breaks log's name.
pub const BREAKS_LOG: &str = "pitcrew-lock-breaks.log";
/// The activity log's name.
pub const ACTIVITY_LOG: &str = "pitcrew-lock-activity.log";
/// Ticket file name prefix.
pub const TICKET_PREFIX: &str = "pitcrew-heavy.ticket.";
/// The wait line for a high marker `<pid>-<name>`: who the waiter is behind, in words a person can act on ("waiting behind train work:
/// merge-check (pid 4121)"; a CI job's marker says CI job).
fn marker_wait_text(suffix: &str) -> String {
    let (pid, name) = suffix.split_once('-').unwrap_or(("?", suffix));
    let what = if record::is_ci_marker(suffix) { "CI job" } else { "train work" };
    format!("waiting behind {what}: {name} (pid {pid})")
}

/// High marker file name prefix.
pub const HIGH_PREFIX: &str = "pitcrew-heavy.high.";
/// Drain marker file name prefix: a container run that has waited long for the machine to be free of other steps.
pub const DRAIN_PREFIX: &str = "pitcrew-heavy.drain.";

/// Failures to inject (tests only; there is no environment switch in the program).
#[derive(Default)]
pub struct Faults {
    /// The next N info writes fail (leaving an empty file).
    pub owner_fail: AtomicU32,
    /// The next N info writes "succeed" but leave an empty file.
    pub owner_empty: AtomicU32,
    /// Ticket writes fail.
    pub ticket_fail: AtomicBool,
    /// Runs once right after a slot was taken, before the count is checked again (race tests: another taker arrives in between).
    pub after_take: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// The wait-for graph is read with the rule from before 7a4aafbee (a drain owner waits for the head of the queue): the model of
    /// the deadlock of 2026-11-18, so a test can see the cycle reported and broken without the deadlock itself being possible.
    pub old_drain_rule: AtomicBool,
}

fn take_one(c: &AtomicU32) -> bool {
    c.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok()
}

/// One participant: its settings, ports, identity and log.
pub struct Throttle {
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
    /// Which file system a folder is on (the OS in the program; a test puts a network one here).
    pub fs_kind: Arc<dyn Fn(&Path) -> FsKind + Send + Sync>,
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
    /// How long this row usually runs, in seconds, from the history ([`crate::history`]); `None` when it has none. Under
    /// `CHEAP_SECS` the row is quick and may run beside the full slots without waiting behind long steps.
    pub expected_secs: Option<f64>,
    /// The row's key in the history (`THROTTLE_ROW`: the caller's name for the row plus its workspace); `None`: no history is read or kept.
    pub row: Option<String>,
}

impl Request {
    /// Whether this row is expected to be quick: it has a history and the history says under `CHEAP_SECS`. A container run never is.
    pub fn is_quick(&self) -> bool {
        self.expected_secs.is_some_and(|s| s < crate::gate::CHEAP_SECS) && !is_container_command(&self.cmdline)
    }
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
    Taken(Held),
    Busy,
    WriteFailed,
    LockFailed(String),
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

/// The pure view of probed holders ([`core::Holder`]): their records' epoch and pid, and whether each runs a container command.
pub fn holders_of(held: &[(u32, OwnerRead)]) -> Vec<Holder> {
    held.iter()
        .map(|(_, o)| match o {
            OwnerRead::Ok(o) => Holder {
                epoch: o.epoch,
                pid: o.pid,
                container: is_container_command(&o.cmd),
            },
            _ => Holder {
                epoch: None,
                pid: 0,
                container: false,
            },
        })
        .collect()
}

impl Throttle {
    /// A participant. `say` receives the human-readable lines.
    pub fn new(
        cfg: Config,
        clock: Arc<dyn Clock>,
        procs: Arc<dyn Procs>,
        load: Arc<dyn LoadSource>,
        log: Arc<Logger>,
        pid: u32,
        say: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> Throttle {
        Throttle {
            cfg,
            clock,
            procs,
            load,
            log,
            pid,
            say,
            faults: Faults::default(),
            fs_kind: Arc::new(fsinfo::of),
            seq: AtomicU64::new(0),
        }
    }

    /// Now, in epoch seconds.
    pub fn now(&self) -> u64 {
        self.clock.now_secs()
    }

    /// The lock file of slot `i` (0-based; from [`CHEAP_BASE`] the quick lane's places).
    pub fn slot_lock(&self, i: u32) -> PathBuf {
        self.slot_file(i, "lock")
    }

    /// The info file of slot `i`: the owner record of its holder.
    pub fn slot_info(&self, i: u32) -> PathBuf {
        self.slot_file(i, "info")
    }

    fn slot_file(&self, i: u32, ext: &str) -> PathBuf {
        if i >= CHEAP_BASE {
            self.cfg.root.join(format!("{CHEAP_PREFIX}{}.{ext}", i - CHEAP_BASE + 1))
        } else {
            self.cfg.root.join(format!("{SLOT_PREFIX}{}.{ext}", i + 1))
        }
    }

    /// Checks the lock folder before anything is taken in it: it exists and is not on a network file system (kernel file locks there are
    /// not reliable). `Err` is the message to print. A file system the OS did not name is logged and let through.
    pub fn check_root(&self) -> Result<(), String> {
        let _ = fs::create_dir_all(&self.cfg.root);
        let kind = (self.fs_kind)(&self.cfg.root);
        if let FsKind::Unknown(why) = &kind {
            self.log.warn(
                "throttle.fs",
                "the file system of the lock folder is not known; locks are used as they are",
                &[("Why", json!(why)), ("Root", json!(self.cfg.root.to_string_lossy()))],
            );
        }
        fsinfo::check_root(&kind, &self.cfg.root)
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

    /// The owner record of slot `i`'s holder. Missing, unreadable or zero-byte is `Empty` (a taker that has the lock and has not yet
    /// written its record).
    pub fn read_owner(&self, i: u32) -> OwnerRead {
        let f = self.slot_info(i);
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

    /// Tries to take slot `i`: the kernel lock first, then the holder's record (an old record is removed, then the new one is written
    /// aside and renamed into place, so a reader sees no record or a whole one). A record that cannot be written gives the slot back.
    fn try_take(&self, i: u32, req: &Request, epoch_out: &mut u64) -> Take {
        let held = match slotfile::take(&self.slot_lock(i)) {
            Ok(Some(h)) => h,
            Ok(None) => return Take::Busy,
            Err(e) => return Take::LockFailed(e.to_string()),
        };
        let info = self.slot_info(i);
        let _ = fs::remove_file(&info);
        let tmp = self.tmp_name("");
        let epoch = self.now();
        let written = self.write_owner(&tmp, req, epoch) && Self::non_empty(&tmp)
            // conformance: write-atomic — the rename claims a name in the lock protocol (it fails when the name exists); the marker is a few bytes written under this holder's own name
            && fs::rename(&tmp, &info).is_ok()
            && Self::non_empty(&info);
        if !written {
            let _ = fs::remove_file(&tmp);
            let _ = fs::remove_file(&info);
            drop(held);
            return Take::WriteFailed;
        }
        *epoch_out = epoch;
        Take::Taken(held)
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

    /// The live high markers as (name suffix, age in seconds); markers of dead processes are removed.
    pub fn live_high_markers(&self, who: &str) -> Vec<(String, u64)> {
        let mut out = Vec::new();
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
                out.push((suffix.to_string(), self.now().saturating_sub(ep.unwrap_or(0))));
                continue;
            }
            let evid = pid.map_or("pid not a number".to_string(), |p| evidence(&*self.procs, p, ep));
            self.blog(&format!("{who} removed the stale high-priority marker {b}"));
            self.log.warn(
                "throttle.stale",
                &format!("removed the stale high-priority marker {b}"),
                &[("Evidence", json!(evid))],
            );
            let _ = fs::remove_file(&f);
        }
        out
    }

    /// The suffix of the first live high marker that still counts: not older than `priority_max`, and not seen by a waiter that
    /// has itself waited `priority_max` (`waited` seconds). The bound is what keeps priority from starving anyone for good.
    pub fn counting_high_marker(&self, who: &str, waited: u64) -> (Option<String>, Vec<String>) {
        let (mut counts, mut ignored) = (None, Vec::new());
        for (m, age) in self.live_high_markers(who) {
            let max = self.marker_max(&m).as_secs();
            if age >= max || waited >= max {
                ignored.push(m);
            } else if counts.is_none() {
                counts = Some(m);
            }
        }
        (counts, ignored)
    }

    /// The bound of a marker: a CI job's has its own (`ci_priority_max`), every other priority run's is `priority_max`.
    pub fn marker_max(&self, suffix: &str) -> Duration {
        if record::is_ci_marker(suffix) {
            self.cfg.ci_priority_max
        } else {
            self.cfg.priority_max
        }
    }

    /// Announces a CI job (`name` starts with `ci-`) on this host: a high marker held by `pid`, the job's long-lived process.
    /// It goes with that process (a dead holder's marker is removed by the next run that sees it) and is ignored after
    /// `ci_priority_max`, so a crashed or hung job cannot hold every agent for good. Err: the marker could not be written.
    pub fn mark(&self, name: &str, pid: u32) -> Result<PathBuf, String> {
        let _ = fs::create_dir_all(&self.cfg.root);
        let m = self.cfg.root.join(format!("{HIGH_PREFIX}{pid}-{}", record::sanitize_name(name)));
        let tmp = self.tmp_name("high.");
        // The holder started before this call: its record epoch is now.
        let body = record::format_marker(pid, "ci", self.now());
        // conformance: write-atomic — the rename claims a name in the lock protocol (it fails when the name exists); the marker is a few bytes written under this holder's own name
        if fs::write(&tmp, body).is_err() || fs::rename(&tmp, &m).is_err() {
            let _ = fs::remove_file(&tmp);
            return Err(format!(
                "cannot write the marker {} (is {} writable and not full?)",
                m.display(),
                self.cfg.root.display()
            ));
        }
        self.log
            .info("throttle.mark", "announced a CI job", &[("Name", json!(name)), ("Pid", json!(pid))]);
        Ok(m)
    }

    /// Removes the marker `mark` wrote; true when there was one.
    pub fn unmark(&self, name: &str, pid: u32) -> bool {
        let m = self.cfg.root.join(format!("{HIGH_PREFIX}{pid}-{}", record::sanitize_name(name)));
        let gone = fs::remove_file(&m).is_ok();
        self.log.info(
            "throttle.mark",
            "CI job marker removed",
            &[("Name", json!(name)), ("Pid", json!(pid)), ("Was", json!(gone))],
        );
        gone
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
                        "throttle.stale",
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
                        "throttle.stale",
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
    fn holder_note(&self, slot: u32) -> (String, Vec<(&'static str, Value)>) {
        match self.read_owner(slot) {
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
            OwnerRead::Empty | OwnerRead::Unparsable => (
                "the lock is held (its holder has not written who it is yet)".into(),
                vec![("Reason", json!("held-unknown"))],
            ),
        }
    }

    /// The first live drain marker of another run that still counts (younger than `priority_max`), by its name suffix; the markers of
    /// dead processes are removed. A container run that waited `drain_after` has one: while it lives no other step starts, so the
    /// machine drains and the container run goes first (the bound keeps a stuck one from holding everybody for good).
    pub fn live_drain_marker(&self, who: &str) -> Option<String> {
        let mut found = None;
        for f in self.dir_entries(DRAIN_PREFIX) {
            let b = file_name(&f);
            let suffix = &b[DRAIN_PREFIX.len()..];
            let Ok(bytes) = read_bounded(&f) else { continue };
            let ep = record::parse_marker_epoch(&bytes).or_else(|| mtime(&f));
            let pid = record::marker_pid(suffix);
            if pid.is_some_and(|p| pid_alive(&*self.procs, p, ep)) {
                if pid != Some(self.pid) && self.now().saturating_sub(ep.unwrap_or(0)) < self.cfg.priority_max.as_secs() && found.is_none()
                {
                    found = Some(suffix.to_string());
                }
                continue;
            }
            self.blog(&format!("{who} removed the stale drain marker {b}"));
            let _ = fs::remove_file(&f);
        }
        found
    }

    /// Whether a taker that holds a slot with the record epoch `my_epoch` now sees more holders than the rule allows (the load rose, or a
    /// container run took a slot, between its look and its take) and is one of the younger ones ([`core::keeps_slot`]).
    fn over_allowed(&self, req: &Request, my_epoch: u64) -> bool {
        let held = self.held_holders();
        let (allowed, _) = self.allowed(req, &held);
        !core::keeps_slot(&held, (my_epoch, self.pid), allowed)
    }

    /// The slots among `indexes` that are held now, by the kernel's word, with their records (an unreadable record is `Empty`). A slot
    /// that cannot be probed counts as held: an answer that is not known is never read as free (ER-33).
    fn probe(&self, indexes: impl Iterator<Item = u32>) -> Vec<(u32, OwnerRead)> {
        indexes
            .filter(|&i| match slotfile::is_held(&self.slot_lock(i)) {
                Ok(held) => held,
                Err(e) => {
                    self.log.warn(
                        "throttle.take",
                        "a slot could not be probed; counted as held",
                        &[("Slot", json!(i + 1)), ("Error", json!(e.to_string()))],
                    );
                    true
                }
            })
            .map(|i| (i, self.read_owner(i)))
            .collect()
    }

    /// The holders of the full slots.
    pub fn held_slots(&self) -> Vec<(u32, OwnerRead)> {
        self.probe(0..self.cfg.slots)
    }

    /// The holders of the quick lane.
    pub fn held_cheap(&self) -> Vec<(u32, OwnerRead)> {
        self.probe(CHEAP_BASE..CHEAP_BASE + CHEAP_LANE)
    }

    /// The pure view of the holders of the full slots.
    fn held_holders(&self) -> Vec<Holder> {
        holders_of(&self.held_slots())
    }

    /// How many steps may run together now for `req`, and what the rule saw (for the wait line and the log). A step that is a
    /// container run counts as holding the machine alone; so does any live container holder.
    pub fn allowed(&self, req: &Request, held: &[Holder]) -> (u32, String) {
        let p = self.pressure();
        let a = core::admission(&Inputs {
            max_slots: self.cfg.slots,
            cpus: self.cfg.cpus,
            pressure: &p,
            req_container: is_container_command(&req.cmdline),
            req_quick: false,
            at_head: true,
            held,
            cheap_held: 0,
        });
        let n = a.allowed;
        let seen = format!(
            "{n} of at most {} at once ({} cores, 5-minute load {}, CPU idle {}, memory available {}{})",
            self.cfg.slots,
            self.cfg.cpus,
            p.load5.map_or("unknown".into(), |x| format!("{x:.1}")),
            p.idle.map_or("unknown".into(), |x| format!("{x:.0}%")),
            p.memory_mb.map_or("unknown".into(), |m| format!("{m} MiB")),
            if a.container { ", a container run alone" } else { "" }
        );
        (n, seen)
    }

    /// What the machine looks like now: the load averages, the CPU idle share and the memory available.
    pub fn pressure(&self) -> Pressure {
        let l = self.load.load();
        Pressure {
            load1: l.map(|l| l[0]),
            load5: l.map(|l| l[1]),
            load15: l.map(|l| l[2]),
            idle: self.load.idle_percent(),
            memory_mb: self.load.available_memory_mb(),
        }
    }

    /// The text of the admission decision, for the log and the wait line: what the rule saw and why a step was let in (or not).
    fn admission_props(&self, req: &Request, lane: &str, running: usize, allowed: u32) -> Vec<(&'static str, Value)> {
        let p = self.pressure();
        vec![
            ("Lane", json!(lane)),
            ("Running", json!(running)),
            ("Allowed", json!(allowed)),
            ("MaxSlots", json!(self.cfg.slots)),
            ("Cpus", json!(self.cfg.cpus)),
            ("Load1", json!(p.load1)),
            ("Load5", json!(p.load5)),
            ("Load15", json!(p.load15)),
            ("IdlePercent", json!(p.idle)),
            ("MemoryAvailableMiB", json!(p.memory_mb)),
            ("ExpectedSeconds", json!(req.expected_secs)),
        ]
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

    /// Everyone who waits, as the folder shows it, without changing anything: the live tickets in queue order, the live high-priority
    /// markers and the live drain markers (a dead holder's file is left for the next run that removes stale things).
    pub fn waiters(&self) -> Vec<Waiter> {
        let live = |pid: Option<u32>, ep: Option<u64>| pid.is_some_and(|p| pid_alive(&*self.procs, p, ep));
        let marker_pids = |prefix: &str| -> Vec<(u32, String, String)> {
            self.dir_entries(prefix)
                .iter()
                .filter_map(|f| {
                    let b = file_name(f);
                    let suffix = &b[prefix.len()..];
                    let bytes = read_bounded(f).ok()?;
                    let ep = record::parse_marker_epoch(&bytes).or_else(|| mtime(f));
                    let pid = record::marker_pid(suffix);
                    live(pid, ep).then(|| {
                        (
                            pid.unwrap_or(0),
                            suffix.split_once('-').map_or(suffix, |(_, n)| n).to_string(),
                            b.clone(),
                        )
                    })
                })
                .collect()
        };
        let drain = marker_pids(DRAIN_PREFIX);
        let high = marker_pids(HIGH_PREFIX);
        let mut out: Vec<Waiter> = Vec::new();
        for f in self.dir_entries(TICKET_PREFIX) {
            if file_name(&f).ends_with(".tmp") {
                continue;
            }
            let Ok(bytes) = read_bounded(&f) else { continue };
            let t = record::parse_ticket(&bytes);
            if !live(t.pid, t.epoch.or_else(|| mtime(&f))) {
                continue;
            }
            let pid = t.pid.unwrap_or(0);
            let drain_owner = drain.iter().any(|(p, _, _)| *p == pid);
            out.push(Waiter {
                id: file_name(&f),
                name: t.name,
                pid,
                position: Some(out.len() + 1),
                high: false,
                drain_owner,
                container: t.container || drain_owner,
            });
        }
        for (pid, name, id) in high {
            if !out.iter().any(|w| w.pid == pid) {
                out.push(Waiter {
                    id,
                    name,
                    pid,
                    position: None,
                    high: true,
                    drain_owner: false,
                    container: false,
                });
            }
        }
        out
    }

    /// The cycle in the wait-for graph of the folder now, if there is one: the members' names in order.
    pub fn wait_cycle(&self) -> Option<Vec<String>> {
        let rules = Rules {
            drain_owner_follows_queue: self.faults.old_drain_rule.load(Ordering::SeqCst),
        };
        let w = self.waiters();
        waitfor::find_cycle(&w, rules).map(|c| c.iter().map(|&i| w[i].name.clone()).collect())
    }

    /// The text of `status`.
    pub fn status(&self) -> String {
        let mut out = String::new();
        let multi = self.cfg.slots > 1;
        if let Err(e) = self.check_root() {
            out += &format!("REFUSED: {e}\n");
            return out;
        }
        let held = self.held_slots();
        for i in 0..self.cfg.slots {
            let pre = if multi { format!("slot {}: ", i + 1) } else { String::new() };
            match held.iter().find(|(s, _)| *s == i) {
                Some((_, OwnerRead::Ok(o))) => {
                    out += &format!(
                        "{pre}held by {} pid {} since '{}' for {}\n",
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
                Some(_) => out += &format!("{pre}held (its holder has not written who it is yet)\n"),
                None => out += &format!("{pre}free\n"),
            }
        }
        for (i, o) in self.held_cheap() {
            if let OwnerRead::Ok(o) = o {
                out += &format!(
                    "quick lane {}: held by {} pid {} for {}\n",
                    i - CHEAP_BASE + 1,
                    o.name,
                    o.pid,
                    mins(self.now(), o.epoch)
                );
            }
        }
        for legacy in ["pitcrew-heavy-v2.lock", "pitcrew-heavy.lock"] {
            let p = self.cfg.root.join(legacy);
            if p.is_dir() {
                out += &format!(
                    "a lock folder of an older throttle exists: {} (not honoured any more; an agent still on that binary does not wait for this one)\n",
                    p.display()
                );
            }
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
        if let Some(c) = self.wait_cycle() {
            let mut s = c.join(" -> ");
            s.push_str(" -> ");
            s.push_str(c.first().map_or("", String::as_str));
            out += &format!("WAIT-FOR CYCLE (nobody can go first): {s}\n");
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
    h: &'a Throttle,
    /// The request.
    pub req: Request,
    ticket: PathBuf,
    marker: Option<PathBuf>,
    held: Option<(u32, Held)>,
    /// How many steps the rule allowed at once when this run took its slot: its share of the cores is `1/share`
    /// (`gate::step_threads`). 1 until it holds a slot.
    share: u32,
    epoch: u64,
    fails: u32,
    t0: u64,
    t0_micros: u64,
    last_note: u64,
    last_reason: String,
    waited_micros: u64,
    bound_said: bool,
    /// The drain marker of this run, once it asked for the machine to drain.
    drain: Option<PathBuf>,
    /// When a wait-for cycle was last reported by this run (it is said once a minute at most).
    cycle_said: Option<u64>,
}

impl<'a> Acquirer<'a> {
    /// How many steps the rule allowed at once when this run took its slot (1 until it holds one): the command's share of the cores.
    pub fn share(&self) -> u32 {
        self.share
    }

    /// Queues (a ticket) or announces itself (a high marker). Err: nothing could be written; no lock was taken.
    pub fn new(h: &'a Throttle, req: Request) -> Result<Acquirer<'a>, String> {
        h.check_root()?;
        let t0 = h.now();
        let mut a = Acquirer {
            h,
            req,
            ticket: PathBuf::from("/nonexistent.ticket"),
            marker: None,
            held: None,
            share: 1,
            epoch: 0,
            fails: 0,
            t0,
            t0_micros: h.clock.now_micros(),
            last_note: t0,
            last_reason: String::new(),
            waited_micros: 0,
            bound_said: false,
            drain: None,
            cycle_said: None,
        };
        if a.req.high {
            let m = h
                .cfg
                .root
                .join(format!("{HIGH_PREFIX}{}-{}", h.pid, record::sanitize_name(&a.req.name)));
            let tmp = h.tmp_name("high.");
            // conformance: write-atomic — the rename claims a name in the lock protocol (it fails when the name exists); the marker is a few bytes written under this holder's own name
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
            "throttle.queue",
            if a.req.high { "announced a high-priority run" } else { "queued" },
            &[("Agent", json!(a.req.name)), ("High", json!(a.req.high))],
        );
        Ok(a)
    }

    fn make_ticket(&mut self) -> Result<(), String> {
        let n = self.h.clock.now_micros();
        let ticket = self.h.cfg.root.join(format!("{TICKET_PREFIX}{n:016}-{}", self.h.pid));
        self.write_ticket(ticket)
    }

    /// Writes this run's ticket at `ticket` (its name carries the queue age) and remembers it.
    fn write_ticket(&mut self, ticket: PathBuf) -> Result<(), String> {
        let h = self.h;
        let tmp = PathBuf::from(format!("{}.tmp", ticket.display()));
        let body = record::format_ticket(
            h.pid,
            &h.start_text(),
            &self.req.name,
            h.now(),
            is_container_command(&self.req.cmdline),
        );
        let ok = !h.faults.ticket_fail.load(Ordering::SeqCst)
            && fs::write(&tmp, body).is_ok()
            && Throttle::non_empty(&tmp)
            // conformance: write-atomic — the rename claims a name in the lock protocol (it fails when the name exists); the marker is a few bytes written under this holder's own name
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
            h.log.info("throttle.wait", &format!("waiting: {msg}"), &p);
        }
        if self.due() {
            let t = h.now();
            (h.say)(&format!("{} waiting {waited}s: {msg}", self.req.name));
            h.alog(&format!("{} waiting {waited}s: {msg}", self.req.name));
            let mut p: Vec<(&str, Value)> = vec![("WaitedSeconds", json!(waited))];
            p.extend(props.iter().map(|(k, v)| (*k, v.clone())));
            h.log.info("throttle.wait", &format!("still waiting: {msg}"), &p);
            self.last_note = t;
        }
    }

    /// Looks for a cycle in who waits for whom and, when this run is part of one, says so at once, naming the members. A drain owner
    /// among them withdraws its own marker (the drain is only a fairness aid: withdrawing it frees the others and loses nothing); any
    /// other member only reports. A run outside the cycle says nothing.
    fn check_cycle(&mut self) {
        let h = self.h;
        let now = h.now();
        if self.cycle_said.is_some_and(|t| now.saturating_sub(t) < 60) {
            return;
        }
        let waiters = h.waiters();
        let rules = Rules {
            drain_owner_follows_queue: h.faults.old_drain_rule.load(Ordering::SeqCst),
        };
        let Some(cycle) = waitfor::find_cycle(&waiters, rules) else {
            return;
        };
        // Who this run is: its ticket, or its high-priority marker (never its name, which two runs can share).
        let mine = self.ticket.file_name().or_else(|| self.marker.as_deref().and_then(Path::file_name));
        let Some(me) = waiters.iter().position(|w| mine.is_some_and(|m| m.to_string_lossy() == w.id)) else {
            return;
        };
        let action = waitfor::break_action(&waiters, &cycle, me);
        if action == Break::Outside {
            return;
        }
        self.cycle_said = Some(now);
        let msg = format!(
            "{} is in a wait-for cycle: {}; {}",
            self.req.name,
            waitfor::describe(&waiters, &cycle),
            if action == Break::WithdrawDrain {
                "it withdraws its drain marker so the others can go on"
            } else {
                "it waits (the drain owner of the cycle withdraws its marker)"
            }
        );
        (h.say)(&msg);
        h.alog(&msg);
        h.log.warn(
            "throttle.cycle",
            &msg,
            &[("Members", json!(cycle.iter().map(|&i| waiters[i].name.clone()).collect::<Vec<_>>()))],
        );
        if action == Break::WithdrawDrain
            && let Some(d) = self.drain.take()
        {
            let _ = fs::remove_file(d);
        }
    }

    /// One attempt. Call again after a poll interval while it says `Waiting`.
    pub fn poll(&mut self) -> Poll {
        let h = self.h;
        let name = self.req.name.clone();
        {
            if self.held.is_some() {
                return Poll::Held;
            }
            // A container run that has waited long for the machine to be free of other steps asks for it to drain; every other
            // step then waits (a stream of high-priority runs cannot keep the container run out for good).
            let container_req = is_container_command(&self.req.cmdline);
            let waited_now = h.now().saturating_sub(self.t0);
            if container_req && self.drain.is_none() && waited_now >= h.cfg.drain_after.as_secs() && !h.held_slots().is_empty() {
                let m = h.cfg.root.join(format!("{DRAIN_PREFIX}{}-{}", h.pid, record::sanitize_name(&name)));
                let tmp = h.tmp_name("drain.");
                // conformance: write-atomic — the rename claims a name in the lock protocol (it fails when the name exists); the marker is a few bytes written under this holder's own name
                if fs::write(&tmp, record::format_marker(h.pid, &h.start_text(), h.now())).is_ok() && fs::rename(&tmp, &m).is_ok() {
                    h.log.info(
                        "throttle.wait",
                        "a container run asks for the machine to drain",
                        &[("WaitedSeconds", json!(waited_now))],
                    );
                    self.drain = Some(m);
                } else {
                    let _ = fs::remove_file(&tmp);
                }
            }
            // A container run that asked for the machine to drain goes before the high-priority runs it made wait.
            let role = Role::of(self.req.high, self.drain.is_some());
            let rules = Rules {
                drain_owner_follows_queue: false,
            };
            if waitfor::yields_to_high(role) {
                let waited = h.now().saturating_sub(self.t0);
                let (counts, ignored) = h.counting_high_marker(&name, waited);
                if !ignored.is_empty() && !self.bound_said {
                    self.bound_said = true;
                    let msg = format!(
                        "{name} no longer yields to the priority run(s) {} (priority bound {} s reached, waited {waited}s)",
                        ignored.join(", "),
                        ignored.iter().map(|m| h.marker_max(m).as_secs()).max().unwrap_or(0)
                    );
                    (h.say)(&msg);
                    h.alog(&msg);
                    h.log.warn(
                        "throttle.priority",
                        &msg,
                        &[
                            ("Markers", json!(ignored)),
                            ("PriorityMaxSeconds", json!(h.cfg.priority_max.as_secs())),
                            ("WaitedSeconds", json!(waited)),
                        ],
                    );
                }
                if let Some(hm) = counts {
                    self.wait_for(
                        "high",
                        &marker_wait_text(&hm),
                        &[("Reason", json!("high-priority")), ("Marker", json!(hm))],
                    );
                    return Poll::Waiting;
                }
            }
            // A container run that asked for the drain goes by its own marker, not by its place in the queue: every other step waits for
            // the drain (above), so a head that waits for it would otherwise wait for a drain owner queued behind it, which waits for
            // the head (the deadlock of 2026-11-18). Its ticket stays until it holds a slot.
            // A quick row (expected under `CHEAP_SECS` by the history) goes first when the machine has the idle CPU and the memory for a
            // step more: it takes a place in the quick lane beside the full slots and does not queue behind long steps. It keeps its
            // ticket meanwhile (a waiter holds a ticket, never a slot, ER-43) and falls back to the queue when the lane is not open.
            let held = h.held_slots();
            let cheap = h.held_cheap();
            let holders = holders_of(&held);
            let pressure = h.pressure();
            let mut inputs = Inputs {
                max_slots: h.cfg.slots,
                cpus: h.cfg.cpus,
                pressure: &pressure,
                req_container: container_req,
                req_quick: self.req.is_quick(),
                at_head: true,
                held: &holders,
                cheap_held: cheap.len(),
            };
            let quick_ok = core::quick_ok(&inputs);
            if waitfor::follows_queue(role, rules) {
                if !self.ticket.is_file()
                    && let Err(e) = self.make_ticket()
                {
                    return Poll::GaveUp(e);
                }
                let (pos, len, head) = h.queue(&self.ticket, &name);
                inputs.at_head = pos == 1;
                if pos != 1 && !quick_ok {
                    self.check_cycle();
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
            if waitfor::waits_for_drain(container_req)
                && let Some(m) = h.live_drain_marker(&name)
            {
                self.check_cycle();
                self.wait_for(
                    "drain",
                    &format!("a container run waits for the machine to drain ({m}); nothing starts before it"),
                    &[("Reason", json!("drain")), ("Marker", json!(m))],
                );
                return Poll::Waiting;
            }
            let mut busy: Option<u32> = None;
            let adm = core::admission(&inputs);
            let (allowed, limited) = (adm.allowed, adm.limited);
            let seen = if h.cfg.slots > 1 {
                h.allowed(&self.req, &holders).1
            } else {
                String::new()
            };
            // A waiter holds a ticket, never a slot (ER-43): the slot of a step that waits for the load stays free for whoever
            // the rule lets in. A container run also waits for the quick lane to empty: it runs alone.
            let quick_lane = if quick_ok { CHEAP_BASE..CHEAP_BASE + CHEAP_LANE } else { 0..0 };
            for i in (0..h.cfg.slots).chain(quick_lane) {
                let in_quick_lane = i >= CHEAP_BASE;
                let is_held = if in_quick_lane {
                    cheap.iter().any(|(s, _)| *s == i)
                } else {
                    held.iter().any(|(s, _)| *s == i)
                };
                if is_held {
                    if !in_quick_lane {
                        busy.get_or_insert(i);
                    }
                    continue;
                }
                if limited && !in_quick_lane {
                    continue;
                }
                let t = h.clock.now_micros();
                match h.try_take(i, &self.req, &mut self.epoch) {
                    Take::Taken(lock) => {
                        self.held = Some((i, lock));
                        if let Some(hook) = h.faults.after_take.lock().ok().and_then(|mut x| x.take()) {
                            hook();
                        }
                        // Two takers can both have seen room (the load rose, a container run arrived, a high run came in beside the
                        // head): look again now that the slot is ours, and step back if the younger of too many. The ticket stays.
                        if h.cfg.slots > 1 && !in_quick_lane && h.over_allowed(&self.req, self.epoch) {
                            self.release_slot();
                            h.log.info(
                                "throttle.wait",
                                "took a slot and stepped back: more steps run than the rule allows now",
                                &[("Slot", json!(i + 1))],
                            );
                            self.wait_for(
                                "stepped-back",
                                "stepped back from a slot: more steps run than the load (or a container run) allows now",
                                &[("Reason", json!("stepped-back"))],
                            );
                            return Poll::Waiting;
                        }
                        let running = held.len() + cheap.len() + 1;
                        self.share = core::share(allowed, running);
                        self.waited_micros = h.clock.now_micros().saturating_sub(self.t0_micros);
                        // The admission decision, with what the rule saw: a stall explains itself from this line (ER-27).
                        let lane = if in_quick_lane { "quick" } else { "slot" };
                        let mut props = h.admission_props(&self.req, lane, running, allowed);
                        props.push(("WaitedSeconds", json!(self.waited_micros / 1_000_000)));
                        h.log
                            .info("throttle.admit", &format!("admitted to the {lane} lane: {seen}"), &props);
                        h.log.info(
                            "throttle.phase",
                            "took the lock",
                            &[
                                ("Phase", json!("take")),
                                ("DurationMs", json!(h.clock.now_micros().saturating_sub(t) / 1000)),
                                ("Slot", json!(i + 1)),
                                ("RecordEpoch", json!(self.epoch)),
                            ],
                        );
                        h.log.info(
                            "throttle.phase",
                            "queue wait over",
                            &[("Phase", json!("queue_wait")), ("DurationMs", json!(self.waited_micros / 1000))],
                        );
                        // One slot: the lock itself keeps the others out, so the marker goes now (the shell plock does the same).
                        // Several slots: it stays until release, or a free slot would let a normal run in beside the priority run.
                        if h.cfg.slots == 1
                            && let Some(m) = self.marker.take()
                        {
                            let _ = fs::remove_file(m);
                        }
                        let _ = fs::remove_file(&self.ticket);
                        if let Some(d) = self.drain.take() {
                            let _ = fs::remove_file(d);
                        }
                        return Poll::Held;
                    }
                    Take::Busy => {
                        busy.get_or_insert(i);
                    }
                    Take::LockFailed(e) => {
                        h.log.event(
                            Level::Error,
                            "throttle.take",
                            "the lock file could not be locked",
                            &[("Error", json!(e)), ("Root", json!(h.cfg.root.to_string_lossy()))],
                        );
                        return Poll::GaveUp(format!(
                            "giving up: no lock taken, the lock file in {} could not be locked ({e}); use a folder on a local disk (THROTTLE_DIR)",
                            h.cfg.root.display()
                        ));
                    }
                    Take::WriteFailed => {
                        self.fails += 1;
                        (h.say)(&format!(
                            "{name} could not write the lock owner record (attempt {} of 3)",
                            self.fails
                        ));
                        h.log.event(
                            Level::Error,
                            "throttle.take",
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
            if self.drain.is_some() {
                // A drain owner waits here for the running steps to end; it is the member of a cycle that can withdraw its marker.
                self.check_cycle();
            }
            if let Some(slot) = busy {
                let (mut msg, props) = h.holder_note(slot);
                let mut reason = format!("{}:slot{slot}", msg.split(" for ").next().unwrap_or(""));
                if h.cfg.slots > 1 && held.len() > 1 || (limited && h.cfg.slots > 1) {
                    msg = format!("{msg}; {} running, {seen}", held.len());
                    reason = format!("{reason}:limit{allowed}");
                }
                self.wait_for(&reason, &msg, &props);
            }
            Poll::Waiting
        }
    }

    /// The queue as this run sees it now (its place and who holds the slots): what [`paddock_proc::queue_stall::StallClock`] judges
    /// progress by.
    ///
    /// A holder whose record is younger than `holder_cap` is `live`: running, and inside the bound this waiter would give its own
    /// command, so the wait behind it is progress, not a stall. `None`: no holder is trusted for any time.
    pub fn queue_view(&self, holder_cap: Option<Duration>) -> QueueView {
        let position = if self.ticket.is_file() {
            self.h.queue(&self.ticket, &self.req.name).0
        } else {
            0
        };
        let now = self.h.now();
        let owners: Vec<Owner> = self
            .h
            .held_slots()
            .into_iter()
            .chain(self.h.held_cheap())
            .filter_map(|(_, o)| match o {
                OwnerRead::Ok(o) => Some(o),
                _ => None,
            })
            .collect();
        let live = holder_cap.is_some_and(|c| owners.iter().any(|o| within_cap(o.epoch, now, c)));
        let holders = owners
            .into_iter()
            .map(|o| QueueHolder {
                name: o.name,
                pid: o.pid,
                epoch: o.epoch,
            })
            .collect();
        QueueView { position, holders, live }
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

    /// The slot this run holds (1-based; the quick lane's places are named by their lane index too), for logs.
    pub fn slot(&self) -> Option<u32> {
        self.held
            .as_ref()
            .map(|(i, _)| if *i >= CHEAP_BASE { *i - CHEAP_BASE + 1 } else { *i + 1 })
    }

    /// Gives everything back: the marker, the ticket, and the slot.
    pub fn release(&mut self) {
        if let Some(m) = self.marker.take() {
            let _ = fs::remove_file(m);
        }
        let _ = fs::remove_file(&self.ticket);
        let _ = fs::remove_file(PathBuf::from(format!("{}.tmp", self.ticket.display())));
        if let Some(d) = self.drain.take() {
            let _ = fs::remove_file(d);
        }
        self.release_slot();
    }

    /// Gives the slot back and nothing else: the ticket and the marker stay, for a taker that steps back and waits again.
    /// Gives the slot back and keeps the claim, with its age: the ticket (removed when the slot was taken) is written again under its
    /// own name, so the next [`Acquirer::poll`] finds this run where it stood and a younger waiter does not pass it. For a slot taken
    /// just as the load gate closed. A high-priority run (no ticket) just waits again.
    pub fn step_back(&mut self) {
        self.release_slot();
        if self.marker.is_none() && !self.ticket.is_file() {
            let _ = self.write_ticket(self.ticket.clone());
        }
    }

    /// Gives the slot back: the record goes first (so the next taker's record is never the one removed), then the kernel lock.
    fn release_slot(&mut self) {
        let h = self.h;
        if let Some((i, lock)) = self.held.take() {
            let _ = fs::remove_file(h.slot_info(i));
            match lock.release() {
                Ok(()) => h.log.info("throttle.release", "released the lock", &[("Slot", json!(i + 1))]),
                Err(e) => h.log.warn(
                    "throttle.release",
                    "the unlock failed; the slot is freed when this handle closes",
                    &[("Slot", json!(i + 1)), ("Error", json!(e.to_string()))],
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
