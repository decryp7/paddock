//! Every process a test started ends with the test, and nothing else does (ADR 0103, ER-14, rule 53). [`Reaper`] acts only on
//! processes it can prove the test created:
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

use paddock_proc::own::{self, Seen as Row};
use paddock_proc::{ProcessId, StartTime};
use paddock_testkit::wait::{FIRST, LAST};
use std::collections::BTreeSet;
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

#[derive(Default)]
struct Seen {
    roots: Vec<ProcessId>,
    below: BTreeSet<ProcessId>,
    /// The leaders of the process groups the reaper made (its children's) or saw a descendant make, by identity.
    groups: BTreeSet<ProcessId>,
}

/// The guard for the processes of one test (see the module). Not `Clone`: one per test, dropped at its end.
pub struct Reaper {
    what: String,
    token: String,
    created: StartTime,
    folders: Vec<PathBuf>,
    seen: Arc<Mutex<Seen>>,
    stop: Arc<AtomicBool>,
    sampler: Option<std::thread::JoinHandle<()>>,
    #[cfg(windows)]
    jobs: Mutex<Vec<own::KillOnClose>>,
    reaped: bool,
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
            token: token(),
            created,
            folders: Vec::new(),
            seen,
            stop,
            sampler,
            #[cfg(windows)]
            jobs: Mutex::new(Vec::new()),
            reaped: false,
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
            let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
            seen.roots.push(id);
            if own::group_of(id.pid()) == Some(id.pid()) {
                seen.groups.insert(id);
            }
        }
        sample(&self.seen);
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
        self.folders.push(dir.clone());
        Ok(dir)
    }

    /// Looks at the process table now (the sampler does so on its own; a test calls this right before a step that ends a parent,
    /// so a grandchild it started is known).
    pub fn observe(&self) {
        sample(&self.seen);
    }

    /// Ends everything still running that the test created, waits until it is gone (at most [`GONE_WITHIN`]) and returns what it
    /// had to end. Panics when a process outlives that wait, unless the thread is already panicking.
    pub fn reap(mut self) -> Vec<Ended> {
        self.reap_now()
    }

    /// The processes [`Reaper::reap`] would end now, and why, without ending them.
    pub fn candidates(&self) -> Vec<Ended> {
        let (roots, below, groups) = {
            let seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
            (seen.roots.clone(), seen.below.clone(), seen.groups.clone())
        };
        let folders: Vec<String> = self
            .folders
            .iter()
            .map(|f| f.to_string_lossy().into_owned())
            .collect();
        let names: Vec<&str> = folders.iter().map(String::as_str).collect();
        let table = own::look(Some((TOKEN_VAR, &self.token)), &names);
        choose(
            &table,
            &Facts {
                roots: &roots,
                below: &below,
                groups: &groups,
                created: self.created,
                me: std::process::id(),
            },
        )
    }

    fn reap_now(&mut self) -> Vec<Ended> {
        self.reaped = true;
        self.stop.store(true, Ordering::Relaxed);
        if let Some(s) = self.sampler.take() {
            s.thread().unpark();
            let _ = s.join();
        }
        sample(&self.seen);
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
            .seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .roots
            .clone();
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

impl Drop for Reaper {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.reap_now();
        }
    }
}

/// What the reaper knows, for [`choose`].
struct Facts<'a> {
    roots: &'a [ProcessId],
    below: &'a BTreeSet<ProcessId>,
    groups: &'a BTreeSet<ProcessId>,
    created: StartTime,
    me: u32,
}

/// The rows of `table` that are the test's, each once, with the first reason that holds (the pure rule of the module).
fn choose(table: &[Row], f: &Facts<'_>) -> Vec<Ended> {
    // A group counts while its leader is still the process seen leading it, or while no process holds the leader's pid (the
    // group keeps its id while it has members, and its id is not handed on before it is empty).
    let groups: BTreeSet<u32> = f
        .groups
        .iter()
        .filter(
            |leader| match table.iter().find(|r| r.id.pid() == leader.pid()) {
                Some(r) => r.id == **leader,
                None => true,
            },
        )
        .map(|leader| leader.pid())
        .collect();
    let mut out: Vec<Ended> = Vec::new();
    for row in table.iter().filter(|r| !r.zombie && r.id.pid() != f.me) {
        let since = row.id.started() >= f.created;
        let ours = row.same_user || cfg!(windows);
        let orphan_or_mine = row.parent.is_some_and(|p| p == 1 || p == f.me);
        let how = if f.roots.contains(&row.id) {
            Some("child")
        } else if f.below.contains(&row.id) {
            Some("below a child")
        } else if !ours || !since {
            None
        } else if row.group.is_some_and(|g| groups.contains(&g)) {
            Some("in a group the reaper made")
        } else if row.carries {
            Some("carries the reaper's token")
        } else if row.names && orphan_or_mine {
            Some("names a folder the reaper made")
        } else {
            None
        };
        if let Some(how) = how {
            out.push(Ended { id: row.id, how });
        }
    }
    out
}

/// Adds every running descendant of every running child to `seen.below`, and the group of each one that leads a group of its own.
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
    let leaders: Vec<ProcessId> = found
        .iter()
        .filter(|d| own::group_of(d.pid()) == Some(d.pid()))
        .copied()
        .collect();
    let mut seen = seen.lock().unwrap_or_else(PoisonError::into_inner);
    seen.below.extend(found);
    seen.groups.extend(leaders);
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
    use super::*;

    fn row(pid: u32, started: u64) -> Row {
        Row {
            id: ProcessId::new(pid, StartTime::from_micros(started)),
            parent: Some(500),
            group: Some(pid),
            same_user: true,
            zombie: false,
            carries: false,
            names: false,
        }
    }

    fn facts<'a>(
        roots: &'a [ProcessId],
        below: &'a BTreeSet<ProcessId>,
        groups: &'a BTreeSet<ProcessId>,
    ) -> Facts<'a> {
        Facts {
            roots,
            below,
            groups,
            created: StartTime::from_micros(1_000),
            me: 42,
        }
    }

    #[test]
    fn a_process_that_only_names_the_folder_is_left_alone_unless_it_is_an_orphan() {
        let (b, g) = (BTreeSet::new(), BTreeSet::new());
        let mut named = row(7, 2_000);
        named.names = true;
        assert!(
            choose(std::slice::from_ref(&named), &facts(&[], &b, &g)).is_empty(),
            "its parent is alive and it carries no token"
        );
        named.parent = Some(1);
        assert_eq!(
            choose(&[named], &facts(&[], &b, &g))[0].how,
            "names a folder the reaper made"
        );
    }

    #[test]
    fn nothing_started_before_the_reaper_or_of_another_user_is_chosen_by_token_group_or_folder() {
        let (b, g) = (
            BTreeSet::new(),
            BTreeSet::from([ProcessId::new(9, StartTime::from_micros(999))]),
        );
        let mut old = row(9, 999);
        old.carries = true;
        old.names = true;
        old.parent = Some(1);
        assert!(
            choose(&[old], &facts(&[], &b, &g)).is_empty(),
            "started before the reaper"
        );
        let mut other = row(9, 2_000);
        other.same_user = false;
        other.carries = true;
        if cfg!(unix) {
            assert!(
                choose(&[other], &facts(&[], &b, &g)).is_empty(),
                "another user's process"
            );
        }
    }

    #[test]
    fn a_group_counts_while_its_leader_is_the_process_seen_or_no_process_holds_its_pid() {
        let leader = row(10, 2_000);
        let mut member = row(12, 2_200);
        member.group = Some(10);
        let (b, groups) = (BTreeSet::new(), BTreeSet::from([leader.id]));
        let chosen = |table: &[Row]| {
            choose(table, &facts(&[], &b, &groups))
                .iter()
                .map(|e| e.id.pid())
                .collect::<Vec<_>>()
        };
        // The leader runs: the group is the reaper's (the leader itself is chosen as a member of its own group too).
        assert_eq!(chosen(&[leader.clone(), member.clone()]), [10, 12]);
        // The leader is gone and nobody holds pid 10: the group keeps its id while it has members.
        assert_eq!(chosen(std::slice::from_ref(&member)), [12]);
        // Pid 10 is another process now: the group id may be someone else's, so nothing is chosen by it.
        let mut other = row(10, 9_000);
        other.group = Some(10);
        assert!(chosen(&[other, member]).is_empty());
    }

    #[test]
    fn zombies_and_this_process_are_never_chosen() {
        let (b, g) = (BTreeSet::new(), BTreeSet::new());
        let mut z = row(7, 2_000);
        z.carries = true;
        z.zombie = true;
        let mut me = row(42, 2_000);
        me.carries = true;
        assert!(choose(&[z, me], &facts(&[], &b, &g)).is_empty());
    }

    #[test]
    fn children_their_descendants_groups_and_token_carriers_are_chosen_once_each() {
        let root = row(10, 2_000);
        let mut desc = row(11, 2_100);
        desc.group = Some(10);
        let mut grouped = row(12, 2_200);
        grouped.group = Some(10);
        let mut carrier = row(13, 2_300);
        carrier.carries = true;
        let below = BTreeSet::from([desc.id]);
        let groups = BTreeSet::from([root.id]);
        let roots = [root.id];
        let chosen = choose(
            &[root, desc, grouped, carrier],
            &facts(&roots, &below, &groups),
        );
        let hows: Vec<&str> = chosen.iter().map(|e| e.how).collect();
        assert_eq!(
            hows,
            [
                "child",
                "below a child",
                "in a group the reaper made",
                "carries the reaper's token"
            ]
        );
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
