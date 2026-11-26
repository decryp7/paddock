//! Every process a test started ends with the test, and nothing else does (ADR 0103, ER-14, rule 53). [`Reaper`] acts only on
//! processes it can prove the test created (the rule is `paddock_proc::own::members` over a `Unit`, ADR 0112 WP-2):
//!
//! - the children it started ([`Reaper::spawn`]): each carries the reaper's token, `PADDOCK_REAPER_ID=<random>`, in its
//!   environment and joins a kill-on-close job (Windows). A test helper never makes a new group or session while the test leads
//!   one (ADR 0103, after Bazel's "Tests may create subprocesses, but not process groups or sessions"): under nextest, which
//!   starts each test in a group of its own and ends that group on a timeout, a child stays in the test's group, so a timeout
//!   that never lets the reaper run still ends it. Only when the test does not lead a group (a plain `cargo test`) does a child
//!   lead a group of its own;
//! - every descendant a sampler saw below one of them while they ran (from [`SAMPLE_FIRST`] backing off to [`SAMPLE_EVERY`]);
//!   a descendant that leads a group of its own (a detached child) adds that group to the reaper's groups;
//! - a process of this user, started after the reaper was made, that is in one of the reaper's groups or carries its token
//!   (environments are inherited, so a detached grandchild that left the tree still carries it);
//! - a process of this user, started after the reaper was made, whose command line names a folder the reaper made
//!   ([`Reaper::scratch_in`]: its name holds the token) and that is an orphan (its parent is init) or a child of this test
//!   process (started by the test itself, or an orphan re-parented to it as Linux's child subreaper).
//!
//! A group counts only while its leader is the process the reaper saw lead it, or no process holds the leader's pid (a group
//! keeps its id while it has members). Anything else is left alone, whatever its command line says. A child is ended with its
//! tree (`kill_tree`: its group only when it leads one); every other
//! process is ended alone, by identity (`paddock_proc::end`), never by a group or a pattern. On Linux the test process becomes a
//! child subreaper, so orphans stay in its tree. [`Reaper::reap`] (or drop, also when the test panics partway) ends them, waits
//! until they are gone and returns what it had to end; a test that must leave nothing running asserts that the list is empty.

use paddock_proc::own::{self, Owned, Unit, Waited, Why, members};
use paddock_proc::{ProcessId, StartTime};
use paddock_testkit::wait::{FIRST, LAST};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// The environment variable every child of a reaper carries, with the reaper's own random value.
pub const TOKEN_VAR: &str = "PADDOCK_REAPER_ID";
/// The first and the longest pause of the sampler between two looks at the process table. One look reads the whole table: 18 ms
/// with 890 processes at load 60 (2026-11-14, docs/benchmarks.md), so the sampler starts at 10 ms (a grandchild is usually started
/// first) and backs off to 0.5 s, about 4 % of a core per live reaper; what it misses after that the token and the groups find.
pub const SAMPLE_FIRST: Duration = Duration::from_millis(10);
pub const SAMPLE_EVERY: Duration = Duration::from_millis(500);
/// How long [`Reaper::reap`] waits for what it ended to be gone: a bound that only stops a process the OS does not end.
pub const GONE_WITHIN: Duration = Duration::from_secs(15);

/// The guard for the processes of one test (see the module). Not `Clone`: one per test, dropped at its end.
pub struct Reaper {
    what: String,
    token: String,
    folders: Vec<PathBuf>,
    /// What the reaper holds, by identity: the children, the descendants and groups the sampler saw, the token and the folders
    /// (the pure rule is `paddock_proc::own::members`, ADR 0112).
    unit: Arc<Mutex<Unit>>,
    stop: Arc<AtomicBool>,
    sampler: Option<std::thread::JoinHandle<()>>,
    #[cfg(windows)]
    jobs: Mutex<Vec<own::KillOnClose>>,
    /// What ending reported; `None` until the reaper has ended its processes.
    ended: Option<Vec<Ended>>,
}

/// One process the reaper had to end: it was still running when the test ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ended {
    /// Its identity.
    pub id: ProcessId,
    /// Why it was the test's: `child`, `below a child`, `in a group the reaper made`, `carries the reaper's token` or
    /// `names a folder the reaper made`.
    pub how: &'static str,
}

impl Reaper {
    /// A reaper for the processes of `what` (a name for its messages), with a fresh token.
    pub fn new(what: &str) -> Reaper {
        static SUBREAPER: std::sync::Once = std::sync::Once::new();
        SUBREAPER.call_once(|| {
            let _ = own::become_child_subreaper();
        });
        // Linux counts a start time from a boot time in whole seconds, so a process started just after this can read up to about a
        // second earlier: one second of slack. The token and the folders are random and made after this, so the slack can only admit
        // a process of this user in one of the reaper's groups.
        let created = StartTime::from_micros(own::now().as_micros().saturating_sub(1_000_000));
        let token = token();
        let unit = {
            let mut unit = Unit::new(created, std::process::id());
            unit.set_marker(TOKEN_VAR, &token);
            Arc::new(Mutex::new(unit))
        };
        let stop = Arc::new(AtomicBool::new(false));
        let sampler = {
            let (unit, stop) = (unit.clone(), stop.clone());
            std::thread::Builder::new()
                .name("reaper-sampler".into())
                .spawn(move || {
                    let mut pause = SAMPLE_FIRST;
                    while !stop.load(Ordering::Relaxed) {
                        sample(&unit);
                        std::thread::park_timeout(pause);
                        pause = (pause * 2).min(SAMPLE_EVERY);
                    }
                })
                .ok()
        };
        Reaper {
            what: what.into(),
            token,
            folders: Vec::new(),
            unit,
            stop,
            sampler,
            #[cfg(windows)]
            jobs: Mutex::new(Vec::new()),
            ended: None,
        }
    }

    /// The reaper's token, the value of [`TOKEN_VAR`] in its children's environments.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Starts `cmd` with the reaper's token in its environment and in a kill-on-close job (Windows), and keeps its identity. It
    /// stays in the test's process group when the test leads one (nextest), and leads a group of its own otherwise. The child is
    /// the caller's to wait on; the reaper ends it only if it still runs.
    pub fn spawn(&self, cmd: &mut Command) -> std::io::Result<Child> {
        cmd.env(TOKEN_VAR, &self.token);
        if !own::leads_own_group() {
            own::own_group(cmd);
        }
        let child = cmd.spawn()?;
        #[cfg(windows)]
        if let Ok(job) = own::KillOnClose::assign(&child) {
            self.jobs
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(job);
        }
        if let Some(id) = ProcessId::of(child.id()) {
            let mut unit = self.unit.lock().unwrap_or_else(PoisonError::into_inner);
            unit.add_root(id);
            if own::group_of(id.pid()) == Some(id.pid()) {
                unit.add_group(id);
            }
        }
        sample(&self.unit);
        Ok(child)
    }

    /// Starts a [`paddock_testkit::TestCommand`] as [`Reaper::spawn`] starts a plain command, and hands back its
    /// [`paddock_testkit::TestChild`]: the child keeps its listed environment and private folders, ends its own child through its
    /// handle when it drops, and everything that child started below it ends with this reaper (ADR 0100 D8). The testkit cannot do
    /// this itself: it has no `paddock_proc` (that crate's tests use the testkit).
    pub fn spawn_test(
        &self,
        cmd: paddock_testkit::TestCommand,
    ) -> std::io::Result<paddock_testkit::TestChild> {
        let (mut cmd, guard) = cmd.into_parts();
        let child = self.spawn(&mut cmd)?;
        Ok(paddock_testkit::TestChild::from_parts(child, guard))
    }

    /// Makes a folder for the test under `parent`, named with the token, and returns it. An orphan whose command line names it
    /// is ended too; a folder made after the reaper cannot be named by a process that started before it.
    pub fn scratch_in(&mut self, parent: &Path) -> std::io::Result<PathBuf> {
        let dir = parent.join(format!("reaper-{}-{}", self.token, self.folders.len()));
        std::fs::create_dir(&dir)?;
        self.unit
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .add_folder(&dir.to_string_lossy());
        self.folders.push(dir.clone());
        Ok(dir)
    }

    /// Looks at the process table now (the sampler does so on its own; a test calls this right before a step that ends a parent,
    /// so a grandchild it started is known).
    pub fn observe(&self) {
        sample(&self.unit);
    }

    /// Ends everything still running that the test created, waits until it is gone (at most [`GONE_WITHIN`]) and returns what it
    /// had to end. Panics when a process outlives that wait, unless the thread is already panicking.
    pub fn reap(mut self) -> Vec<Ended> {
        self.end().clone()
    }

    /// The processes [`Reaper::reap`] would end now, and why, without ending them.
    pub fn candidates(&self) -> Vec<Ended> {
        let unit = self
            .unit
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        members(&unit.look(), &unit)
            .into_iter()
            .map(|m| Ended {
                id: m.id,
                how: how(m.why),
            })
            .collect()
    }

    fn reap_now(&mut self) -> Vec<Ended> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(s) = self.sampler.take() {
            s.thread().unpark();
            let _ = s.join();
        }
        sample(&self.unit);
        let todo = self.candidates();
        for e in &todo {
            if e.how == "child" {
                // A child leads a group the reaper made: its tree ends with it.
                let _ = paddock_proc::kill_tree(e.id);
            } else {
                let _ = paddock_proc::end(e.id);
            }
            eprintln!(
                "reaper ({}): ended pid {} ({}), still running at the end of the test",
                self.what,
                e.id.pid(),
                e.how
            );
        }
        #[cfg(windows)]
        for job in self
            .jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
        {
            job.terminate();
        }
        let roots = self
            .unit
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .roots()
            .to_vec();
        let began = Instant::now();
        let mut pause = FIRST;
        let left = loop {
            // Only a process seen below a child can have been re-parented to this process: never one the test started itself.
            for e in todo
                .iter()
                .filter(|e| e.how == "below a child" && !roots.contains(&e.id))
            {
                own::reap_zombie(e.id);
            }
            let left: Vec<u32> = todo
                .iter()
                .filter(|e| !own::has_ended(e.id))
                .map(|e| e.id.pid())
                .collect();
            if left.is_empty() || began.elapsed() >= GONE_WITHIN {
                break left;
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

/// The reaper as an owner (ADR 0112): `wait` ends when nothing of the test is running, `end` is [`Reaper::reap`] by reference.
impl Owned for Reaper {
    type Ended = Vec<Ended>;

    fn wait(&mut self, bound: Duration) -> Waited {
        let began = Instant::now();
        let mut pause = FIRST;
        loop {
            if self.candidates().iter().all(|e| own::has_ended(e.id)) {
                return Waited::Gone;
            }
            if began.elapsed() >= bound {
                return Waited::Bound;
            }
            std::thread::sleep(pause.min(bound.saturating_sub(began.elapsed())));
            pause = (pause * 2).min(LAST);
        }
    }

    fn result(&self) -> Option<&Vec<Ended>> {
        self.ended.as_ref()
    }

    fn end(&mut self) -> &Vec<Ended> {
        if self.ended.is_none() {
            let ended = self.reap_now();
            self.ended = Some(ended);
        }
        self.ended.get_or_insert_with(Vec::new)
    }
}

/// The reaper's words for [`Why`] (the strings its callers and tests read).
fn how(why: Why) -> &'static str {
    match why {
        Why::Root => "child",
        Why::Below => "below a child",
        Why::Group => "in a group the reaper made",
        Why::Marker => "carries the reaper's token",
        Why::Folder => "names a folder the reaper made",
    }
}

impl Drop for Reaper {
    fn drop(&mut self) {
        if self.ended.is_none() {
            let _ = self.end();
        }
    }
}

/// Adds every running descendant of every running child to `seen.below`, and the group of each one that leads a group of its own.
fn sample(unit: &Mutex<Unit>) {
    let roots = unit
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .roots()
        .to_vec();
    let mut found = Vec::new();
    for root in roots {
        if root.confirmed_running() {
            found.extend(paddock_proc::descendants_of(root));
        }
    }
    let leaders: Vec<ProcessId> = found
        .iter()
        .filter(|d| own::group_of(d.pid()) == Some(d.pid()))
        .copied()
        .collect();
    let mut unit = unit.lock().unwrap_or_else(PoisonError::into_inner);
    unit.add_below(found);
    for leader in leaders {
        unit.add_group(leader);
    }
}

/// A token no other reaper on the machine has: this process's pid, the time and a counter, as hex.
fn token() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(
        "{:x}{:x}{:x}",
        std::process::id(),
        nanos,
        N.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
mod rule_tests {
    //! The membership rule is `paddock_proc::own::members` and is tested there (`tests/it/owner.rs`, ADR 0112 WP-1); these
    //! tests hold the reaper's own words for it and its ending of a result.
    use super::*;

    #[test]
    fn every_reason_has_its_own_words_and_the_words_callers_read() {
        let all = [
            (Why::Root, "child"),
            (Why::Below, "below a child"),
            (Why::Group, "in a group the reaper made"),
            (Why::Marker, "carries the reaper's token"),
            (Why::Folder, "names a folder the reaper made"),
        ];
        for (why, words) in all {
            assert_eq!(how(why), words);
        }
        let distinct: std::collections::BTreeSet<_> = all.iter().map(|(w, _)| how(*w)).collect();
        assert_eq!(distinct.len(), all.len());
    }

    #[test]
    fn a_reaper_has_no_result_until_it_ends_and_ending_twice_reports_the_first_ending_again() {
        let mut r = Reaper::new("owner result");
        assert!(r.result().is_none());
        assert_eq!(
            r.wait(Duration::from_secs(5)),
            Waited::Gone,
            "nothing was started"
        );
        let first = r.end().clone();
        assert!(first.is_empty());
        assert_eq!(r.result(), Some(&first));
        assert_eq!(r.end(), &first);
    }
}

#[cfg(all(test, unix))]
mod tests {
    //! These tests signal nothing themselves except children they spawned (through their stdin or `Child`). The grandchildren they
    //! make sleep 20 s and end by themselves, so a red run (a reaper that ends nothing) leaks a sleep for at most 20 s and ends no
    //! other process.
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;

    #[test]
    fn wait_ends_at_its_bound_while_a_child_runs_and_reports_gone_after_the_end() {
        let mut r = Reaper::new("owned wait");
        let mut cmd = paddock_proc::command("sleep");
        cmd.arg("20");
        let mut child = r.spawn(&mut cmd).expect("sleep starts");
        let began = Instant::now();
        assert_eq!(r.wait(Duration::from_millis(100)), Waited::Bound);
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "ends at the bound"
        );
        let ended = r.end().clone();
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].how, "child");
        assert_eq!(r.wait(Duration::from_secs(5)), Waited::Gone);
        let _ = child.wait();
    }

    /// A shell that starts a grandchild (`sleep 20` under a second shell whose `$0` is `arg`) in its own process group, detached
    /// from the shell's output, prints its pid, then waits for a line on stdin before it exits.
    fn parent_of_a_detached_grandchild(arg: &str) -> Command {
        let mut c = paddock_proc::command("sh");
        c.args([
            "-c",
            "set -m; sh -c 'sleep 20; exit 0' \"$0\" </dev/null >/dev/null 2>&1 & echo $!; read _",
            arg,
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
        // conformance: recorded-pid — the parent shell is blocked on `read` or `exec sleep` and holds the grandchild when the line is read
        ProcessId::of(line.trim().parse().unwrap()).expect("the grandchild runs")
    }

    fn let_parent_end(child: &mut Child) {
        child.stdin.take().unwrap().write_all(b"\n").unwrap();
        assert!(child.wait().unwrap().success());
    }

    /// A `TestChild` ends the child it holds when it drops (through its own handle), and its private folder goes after (review of
    /// a8fca0ebd, P2 1: a child that waited on its stdin ended by itself when the stdin closed, so the kill was never seen).
    #[test]
    fn a_dropped_test_child_is_killed_by_its_handle_and_its_private_folder_goes() {
        let child = paddock_testkit::TestCommand::new("sh")
            .args(["-c", "exec sleep 30"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let id = ProcessId::of(child.id()).expect("the child runs");
        let private = child.private_dir().to_path_buf();
        assert!(id.confirmed_running(), "the child runs until the drop");
        drop(child);
        assert!(
            own::has_ended(id),
            "the child {} outlived its TestChild",
            id.pid()
        );
        assert!(!private.exists(), "the private folder outlived the child");
    }

    /// What a `TestCommand` child started below it ends with the test when the child was started through the reaper
    /// (`spawn_test`, ADR 0100 D8 and ADR 0103): the `TestChild` ends its own child, the reaper the rest.
    #[test]
    fn a_test_commands_grandchild_ends_with_the_reaper_it_was_started_through() {
        let reaper = Reaper::new("the TestCommand grandchild test");
        let mut cmd = paddock_testkit::TestCommand::new("sh");
        cmd.args([
            "-c",
            "set -m; sh -c 'sleep 20; exit 0' </dev/null >/dev/null 2>&1 & echo $!; exec sleep 20",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
        let mut child = reaper.spawn_test(cmd).unwrap();
        let g = grandchild(&mut child);
        let c = ProcessId::of(child.id()).expect("the child runs");
        reaper.observe();
        drop(child);
        assert!(
            own::has_ended(c),
            "the child {} outlived its TestChild",
            c.pid()
        );
        assert!(
            g.confirmed_running(),
            "the grandchild outlives the child (the leak this guards)"
        );
        let ended = reaper.reap();
        assert!(
            own::has_ended(g),
            "the grandchild {} outlived the reaper",
            g.pid()
        );
        assert!(ended.iter().any(|e| e.id == g), "{ended:?}");
    }

    #[test]
    fn a_detached_grandchild_whose_parent_ended_is_ended_with_the_test() {
        let reaper = Reaper::new("the grandchild test");
        let mut child = reaper
            .spawn(&mut parent_of_a_detached_grandchild("x"))
            .unwrap();
        let g = grandchild(&mut child);
        reaper.observe();
        let_parent_end(&mut child);
        assert!(
            g.confirmed_running(),
            "the grandchild outlives its parent (the leak this guards)"
        );
        let ended = reaper.reap();
        assert!(
            own::has_ended(g),
            "the detached grandchild {} outlived the reaper",
            g.pid()
        );
        assert!(ended.iter().any(|e| e.id == g), "{ended:?}");
    }

    // Linux only: macOS 27 shows no process's environment, not even this user's (sysinfo's environ() and `ps -E` both come back
    // without it, measured 2026-11-15), so there the groups, the sampler and the folder find a detached grandchild instead.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_grandchild_the_sampler_never_saw_is_found_by_the_token_it_inherited() {
        let reaper = Reaper::new("the token test");
        let mut child = reaper
            .spawn(&mut parent_of_a_detached_grandchild("x"))
            .unwrap();
        let g = grandchild(&mut child);
        let_parent_end(&mut child);
        let chosen = reaper.candidates();
        assert!(chosen.iter().any(|e| e.id == g), "{chosen:?}");
        let _ = reaper.reap();
        assert!(
            own::has_ended(g),
            "the grandchild {} outlived the reaper",
            g.pid()
        );
    }

    #[test]
    fn an_orphan_naming_the_reapers_folder_is_ended_and_a_process_that_only_names_it_is_not() {
        let parent = tempfile::tempdir().unwrap();
        let mut reaper = Reaper::new("the folder test");
        let folder = reaper
            .scratch_in(parent.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        // Started outside the reaper (no token): an orphan naming the folder once its parent ends.
        let mut outside = parent_of_a_detached_grandchild(&folder).spawn().unwrap();
        let orphan = grandchild(&mut outside);
        let_parent_end(&mut outside);
        // Also outside the reaper: a shell whose child names the folder and has a live parent (the shell). Naming the folder
        // alone proves nothing. The shell's own command line does not name it (`$F` comes from the environment).
        let mut holder = paddock_proc::command("sh")
            .args([
                "-c",
                "exec 3<&0; sh -c 'read _ <&3; exit 0' \"$F\" & echo $!; wait",
            ])
            .env("F", &folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let held = grandchild(&mut holder);
        let chosen = reaper.candidates();
        assert!(
            chosen
                .iter()
                .any(|e| e.id == orphan && e.how == "names a folder the reaper made"),
            "{chosen:?}"
        );
        assert!(
            !chosen.iter().any(|e| e.id == held),
            "a process with a live parent and no token: {chosen:?}"
        );
        let _ = reaper.reap();
        assert!(
            own::has_ended(orphan),
            "the orphan {} outlived the reaper",
            orphan.pid()
        );
        assert!(
            held.confirmed_running(),
            "the reaper ended a process it could not prove was the test's"
        );
        drop(holder.stdin.take());
        assert!(holder.wait().unwrap().success());
    }

    #[test]
    fn another_reapers_children_are_left_alone() {
        let a = Reaper::new("reaper a");
        let b = Reaper::new("reaper b");
        let mut of_b = b
            .spawn(
                paddock_proc::command("sh")
                    .args(["-c", "read _; exit 0"])
                    .stdin(Stdio::piped()),
            )
            .unwrap();
        let id = ProcessId::of(of_b.id()).unwrap();
        assert!(!a.candidates().iter().any(|e| e.id == id));
        let _ = a.reap();
        assert!(id.confirmed_running(), "reaper a ended reaper b's child");
        drop(of_b.stdin.take());
        assert!(of_b.wait().unwrap().success());
        assert!(b.reap().is_empty());
    }

    #[test]
    fn a_test_that_panics_partway_still_ends_its_children() {
        let (tx, rx) = std::sync::mpsc::channel();
        let r = std::panic::catch_unwind(move || {
            let reaper = Reaper::new("the panicking test");
            let mut child = reaper
                .spawn(&mut parent_of_a_detached_grandchild("x"))
                .unwrap();
            tx.send((ProcessId::of(child.id()).unwrap(), grandchild(&mut child)))
                .unwrap();
            reaper.observe();
            panic!("an assertion failed partway");
        });
        assert!(r.is_err());
        let (parent, g) = rx.recv().unwrap();
        assert!(
            own::has_ended(parent) && own::has_ended(g),
            "the parent or the grandchild outlived a test that panicked"
        );
    }

    /// Set when this test binary runs as "the test" under [`a_runner_that_ends_the_tests_group_also_ends_the_reapers_children`].
    const UNDER_RUNNER: &str = "PADDOCK_REAPER_TEST_UNDER_RUNNER";

    /// The test a fake runner starts: a reaper starts `sleep 60`, the test prints its pid and blocks until the runner kills it, so
    /// the reaper never runs. Run by itself (without the variable), it does nothing.
    #[test]
    fn under_a_fake_runner() {
        if std::env::var_os(UNDER_RUNNER).is_none() {
            return;
        }
        let reaper = Reaper::new("under a fake runner");
        let mut child = reaper
            .spawn(
                paddock_proc::command("sleep")
                    .arg("60")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null()),
            )
            .unwrap();
        println!("pid {}", child.id());
        std::io::stdout().flush().unwrap();
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        drop(reaper);
        let _ = child.wait();
    }

    /// nextest starts each test in a process group of its own and, on a timeout, kills that group, so no drop runs. A reaper's
    /// child must be in that group to end with it (round 2 of the ADR 0103 review). The fake runner here is this test: it starts
    /// this binary as the test above, leading a group of its own as nextest does, kills that group (its own child's, nothing
    /// else) and checks that the reaper's `sleep` ended with it. Safe while red: a child that escaped is a `sleep 60` that ends by
    /// itself.
    #[test]
    fn a_runner_that_ends_the_tests_group_also_ends_the_reapers_children() {
        use std::os::unix::process::CommandExt;
        let exe = std::env::current_exe().unwrap();
        let mut test = paddock_proc::command(&exe)
            .args([
                "--exact",
                "reap::tests::under_a_fake_runner",
                "--nocapture",
                "--test-threads",
                "1",
            ])
            .env(UNDER_RUNNER, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap();
        let mut sleep = None;
        for line in BufReader::new(test.stdout.take().unwrap()).lines() {
            let line = line.unwrap();
            // libtest prints the test's name and "... " on the same line before the test's own output.
            if let Some((_, pid)) = line.rsplit_once("pid ") {
                // conformance: recorded-pid — the writer blocks on stdin, which this test holds
                sleep = ProcessId::of(pid.trim().parse().unwrap());
                break;
            }
        }
        let sleep = sleep.expect("the test under the fake runner printed its child's pid");
        // The runner's timeout: SIGKILL to the test's group, which this test made for its own child.
        paddock_proc::end_unreaped_group(test.id());
        let _ = test.wait();
        // conformance: hang-guard — 30 s only stops a sleep that escaped the group (it ends by itself after 60 s)
        let gone =
            paddock_testkit::poll_until_within(Duration::from_secs(30), || own::has_ended(sleep));
        assert!(
            gone,
            "the reaper's child {} outlived a runner that killed the test's group",
            sleep.pid()
        );
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
