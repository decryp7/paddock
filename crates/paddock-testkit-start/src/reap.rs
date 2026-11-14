//! Every process a test started ends with the test (ADR 0103, ER-14): [`Reaper`] keeps the identity of each child it started
//! and of each process it has seen below one, and on [`Reaper::reap`] or drop (a passing test, a failed assertion, a panic
//! partway) ends them all through `paddock_proc`, by identity (rule 53), then waits until they are gone.
//!
//! A child's tree is followed while the reaper lives: a sampler looks at the process table with a backoff (from [`SAMPLE_FIRST`]
//! up to [`SAMPLE_EVERY`]) and remembers every descendant, so a detached grandchild (its own process group, its parent already gone,
//! which `kill_tree` of the child no longer reaches) is still ended. A reaper made with [`Reaper::marked`] also ends every
//! process whose command line names the marker (the test's scratch folder): a process that left its tree before the sampler
//! saw it (a `slot hold` whose holder script exited at once, run 8099) is still found.
//!
//! What it had to end is printed on the test's stderr (ER-27). [`Reaper::reap`] returns it, so a test that must leave nothing
//! running by itself asserts that the list is empty.

use paddock_proc::ProcessId;
use paddock_testkit::wait::{FIRST, LAST};
use std::collections::BTreeSet;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// The first and the longest pause of the sampler between two looks at the process table. One look reads the whole table: 18 ms
/// with 890 processes at load 60 (2026-11-14, docs/benchmarks.md), so the sampler starts at 10 ms (a grandchild is usually started
/// first) and backs off to 0.5 s, about 4 % of a core per live reaper; what it misses after that the marker finds.
pub const SAMPLE_FIRST: Duration = Duration::from_millis(10);
pub const SAMPLE_EVERY: Duration = Duration::from_millis(500);
/// The shortest marker [`Reaper::marked`] takes: a test's scratch folder is far longer, a shared prefix (`/tmp`) is shorter.
pub const MARKER_MIN: usize = 12;
/// How long [`Reaper::reap`] waits for what it ended to be gone: a bound that only stops a process the OS does not end.
pub const GONE_WITHIN: Duration = Duration::from_secs(15);

#[derive(Default)]
struct Seen {
    roots: Vec<ProcessId>,
    below: BTreeSet<ProcessId>,
}

/// The guard for the processes of one test (see the module). Not `Clone`: one per test, dropped at its end.
pub struct Reaper {
    what: String,
    marker: Option<String>,
    seen: Arc<Mutex<Seen>>,
    stop: Arc<AtomicBool>,
    sampler: Option<std::thread::JoinHandle<()>>,
    reaped: bool,
}

/// One process the reaper had to end: it was still running when the test ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ended {
    /// Its identity.
    pub id: ProcessId,
    /// How it was found: `child`, `below a child` or `names the marker`.
    pub how: &'static str,
}

impl Reaper {
    /// A reaper for the children of `what` (a name for its messages).
    pub fn new(what: &str) -> Reaper {
        Reaper::make(what, None)
    }

    /// A reaper that also ends every process whose command line names `marker` (use the test's own scratch folder: unique to
    /// the test, and named by everything the test starts there).
    ///
    /// Panics on a marker shorter than [`MARKER_MIN`] characters: a short text (`/tmp`) is in the command lines of unrelated
    /// processes, and the reap would end them.
    pub fn marked(what: &str, marker: &std::path::Path) -> Reaper {
        let marker = marker.to_string_lossy().into_owned();
        assert!(
            marker.chars().count() >= MARKER_MIN,
            "reaper ({what}): the marker {marker:?} must be at least {MARKER_MIN} characters and unique to the test (its scratch folder)"
        );
        Reaper::make(what, Some(marker))
    }

    fn make(what: &str, marker: Option<String>) -> Reaper {
        let seen = Arc::new(Mutex::new(Seen::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let sampler = {
            let (seen, stop) = (seen.clone(), stop.clone());
            std::thread::Builder::new()
                .name("reaper-sampler".into())
                .spawn(move || {
                    let mut pause = SAMPLE_FIRST;
                    while !stop.load(Ordering::Relaxed) {
                        sample(&seen);
                        std::thread::park_timeout(pause);
                        pause = (pause * 2).min(SAMPLE_EVERY);
                    }
                })
                .ok()
        };
        Reaper {
            what: what.into(),
            marker,
            seen,
            stop,
            sampler,
            reaped: false,
        }
    }

    /// Starts `cmd` and keeps its identity. The child is the caller's to wait on; the reaper ends it only if it is still
    /// running at the end.
    pub fn spawn(&self, cmd: &mut Command) -> std::io::Result<Child> {
        let child = cmd.spawn()?;
        self.adopt(&child);
        Ok(child)
    }

    /// Keeps the identity of a child the test started itself.
    pub fn adopt(&self, child: &Child) {
        self.adopt_pid(child.id());
    }

    /// Keeps the identity of a process the test learned the pid of (a pid file, a line the child printed).
    pub fn adopt_pid(&self, pid: u32) {
        if let Some(id) = ProcessId::of(pid) {
            self.seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .roots
                .push(id);
            sample(&self.seen);
        }
    }

    /// Looks at the process table now (the sampler does so on its own; a test calls this right before a step that ends a
    /// parent, so a grandchild it started is known).
    pub fn observe(&self) {
        sample(&self.seen);
    }

    /// Ends everything still running that the test started, waits until it is gone (at most [`GONE_WITHIN`]) and returns what
    /// it had to end. Panics when a process outlives that wait (a leak the OS would not end), unless the thread is already
    /// panicking.
    pub fn reap(mut self) -> Vec<Ended> {
        self.reap_now()
    }

    fn reap_now(&mut self) -> Vec<Ended> {
        self.reaped = true;
        self.stop.store(true, Ordering::Relaxed);
        if let Some(s) = self.sampler.take() {
            s.thread().unpark();
            let _ = s.join();
        }
        sample(&self.seen);
        let (roots, below) = {
            let seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
            (seen.roots.clone(), seen.below.clone())
        };
        let mut todo: Vec<Ended> = Vec::new();
        let mut push = |id: ProcessId, how: &'static str| {
            if !todo.iter().any(|e| e.id == id) && id.confirmed_running() {
                todo.push(Ended { id, how });
            }
        };
        for id in &roots {
            push(*id, "child");
        }
        for id in &below {
            push(*id, "below a child");
        }
        if let Some(marker) = &self.marker {
            let me = std::process::id();
            for id in paddock_proc::processes_mentioning(marker) {
                if id.pid() != me {
                    push(id, "names the marker");
                }
            }
        }
        for e in &todo {
            // A tree first (its group and everything below it), then the process itself by identity.
            let _ = paddock_proc::kill_tree(e.id);
            let _ = paddock_proc::end(e.id);
            eprintln!(
                "reaper ({}): ended pid {} ({}), still running at the end of the test",
                self.what,
                e.id.pid(),
                e.how
            );
        }
        let began = Instant::now();
        let mut pause = FIRST;
        let left = loop {
            let left: Vec<&Ended> = todo.iter().filter(|e| !e.id.has_ended()).collect();
            if left.is_empty() || began.elapsed() >= GONE_WITHIN {
                break left.iter().map(|e| e.id.pid()).collect::<Vec<_>>();
            }
            std::thread::sleep(pause);
            pause = (pause * 2).min(LAST);
        };
        if !left.is_empty() && !std::thread::panicking() {
            panic!(
                "reaper ({}): pids {left:?} were still running {GONE_WITHIN:?} after they were ended",
                self.what
            );
        }
        todo
    }
}

impl Drop for Reaper {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.reap_now();
        }
    }
}

/// Adds every running descendant of every running root to `seen.below` (one look at the table per root; a process seen below a
/// root is ended with its own tree at the end, so what it starts later is ended with it).
fn sample(seen: &Mutex<Seen>) {
    let roots = seen
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .roots
        .clone();
    let mut found = Vec::new();
    for root in roots {
        if root.confirmed_running() {
            found.extend(paddock_proc::descendants_of(root));
        }
    }
    seen.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .below
        .extend(found);
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;

    /// A shell that starts a grandchild in its own process group (job control: `set -m`), detached from the shell's output, prints
    /// its pid, then waits for a line on stdin before it exits. The grandchild runs `sleep` under a second shell whose arguments
    /// name `tag`, so its command line carries the tag.
    fn parent_of_a_detached_grandchild(tag: &str) -> Command {
        let mut c = paddock_proc::command("sh");
        c.args([
            "-c",
            "set -m; sh -c 'sleep 300; exit 0' \"$0\" </dev/null >/dev/null 2>&1 & echo $!; read _",
            tag,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
        c
    }

    fn grandchild(child: &mut Child) -> ProcessId {
        let mut line = String::new();
        BufReader::new(child.stdout.as_mut().unwrap())
            .read_line(&mut line)
            .unwrap();
        ProcessId::of(line.trim().parse().unwrap()).expect("the grandchild runs")
    }

    /// A text no other process names: this test process's pid and the time.
    fn tag() -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        format!("paddock-reaper-{}-{nanos}", std::process::id())
    }

    #[test]
    fn a_detached_grandchild_whose_parent_ended_is_ended_with_the_test() {
        let reaper = Reaper::new("the grandchild test");
        let mut child = reaper
            .spawn(&mut parent_of_a_detached_grandchild(&tag()))
            .unwrap();
        let g = grandchild(&mut child);
        reaper.observe();
        // The parent ends: the grandchild leads its own group and has a new parent now, out of the child's tree.
        child.stdin.take().unwrap().write_all(b"\n").unwrap();
        assert!(child.wait().unwrap().success());
        assert!(
            g.confirmed_running(),
            "the grandchild outlives its parent (the leak this guards)"
        );
        let ended = reaper.reap();
        assert!(
            g.has_ended(),
            "the detached grandchild {} outlived the reaper",
            g.pid()
        );
        assert!(
            ended.iter().any(|e| e.id == g && e.how == "below a child"),
            "{ended:?}"
        );
    }

    #[test]
    fn a_process_that_left_before_it_was_seen_is_found_by_the_marker() {
        let marker = tag();
        // Started outside the reaper: nothing ties the grandchild to it but the marker in its command line.
        let mut child = parent_of_a_detached_grandchild(&marker).spawn().unwrap();
        let g = grandchild(&mut child);
        child.stdin.take().unwrap().write_all(b"\n").unwrap();
        assert!(child.wait().unwrap().success());
        let reaper = Reaper::marked("the marker test", std::path::Path::new(&marker));
        drop(reaper);
        assert!(
            g.has_ended(),
            "the grandchild {} that names the marker outlived the reaper",
            g.pid()
        );
    }

    #[test]
    fn a_test_that_panics_partway_still_ends_its_children() {
        let marker = tag();
        let (tx, rx) = std::sync::mpsc::channel();
        let r = std::panic::catch_unwind(move || {
            let reaper = Reaper::new("the panicking test");
            let mut child = reaper
                .spawn(&mut parent_of_a_detached_grandchild(&marker))
                .unwrap();
            tx.send((ProcessId::of(child.id()).unwrap(), grandchild(&mut child)))
                .unwrap();
            reaper.observe();
            panic!("an assertion failed partway");
        });
        assert!(r.is_err());
        let (parent, g) = rx.recv().unwrap();
        assert!(
            parent.has_ended() && g.has_ended(),
            "the parent or the grandchild outlived a test that panicked"
        );
    }

    #[test]
    #[should_panic(expected = "at least 12 characters")]
    fn a_marker_too_short_to_be_unique_to_the_test_is_refused() {
        // A short marker names unrelated processes (`/tmp` is in the command line of half the machine's shells): a reap by it would
        // end processes that are not the test's. The marker here matches nothing, so the test is safe while it is red too.
        let _ = Reaper::marked("a short marker", std::path::Path::new("zq%x"));
    }

    #[test]
    fn a_child_that_ended_by_itself_is_not_reported() {
        let reaper = Reaper::new("a quiet test");
        let mut child = reaper
            .spawn(paddock_proc::command("true").stdout(Stdio::null()))
            .unwrap();
        assert!(child.wait().unwrap().success());
        assert_eq!(reaper.reap(), Vec::new());
    }
}
