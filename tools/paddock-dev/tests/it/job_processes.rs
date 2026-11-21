//! A host job's processes end with the job (ER-14: no process left behind; the 2026-11-20 `vm-lock.sh` that outlived its job by four
//! days). `ci_isolation::processes` finds a job's processes by what they carry (`PADDOCK_JOB_TEMP` in the environment, the job's folder as
//! the current folder, the job's folder in the command line: macOS shows no environment of a program Apple ships, so the three are
//! read together) and ends them through `paddock_proc` by identity (pid plus start time, rule 53). The tests start only stand-in
//! processes of their own (each under a `paddock_testkit_start::Reaper`, which ends whatever a failing test leaves) with a job folder of
//! their own, and end only those.
// windows-twin: none: another process's environment and folder are read on Unix only (paddock_proc::own::survey); Windows host jobs report "not measured"
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![allow(
    clippy::zombie_processes,
    reason = "every stand-in is owned by a Reaper, which ends it and waits for it when the test ends"
)]

use paddock_dev::ci_isolation::processes::{Job, TOKEN, leftovers, members_of, self_and_ancestors, stop};
use paddock_proc::ProcessId;
use paddock_proc::own::survey;
use paddock_testkit_start::Reaper;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const SHORT: Duration = Duration::from_millis(400);

/// A job as the runner makes it: `<work>/<16 hex>/tmp` is its temp folder; marks are where the stand-ins say they are up.
struct Folder {
    _keep: tempfile::TempDir,
    /// `<work>/<16 hex>`: what the runner removes after the job.
    root: PathBuf,
    /// `<root>/tmp`: the token.
    temp: PathBuf,
    marks: PathBuf,
    elsewhere: PathBuf,
}

fn folder() -> Folder {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("work/0123456789abcdef");
    let temp = root.join("tmp");
    let marks = d.path().join("marks");
    let elsewhere = d.path().join("elsewhere");
    for p in [&temp, &marks, &elsewhere] {
        std::fs::create_dir_all(p).unwrap();
    }
    Folder {
        _keep: d,
        root,
        temp,
        marks,
        elsewhere,
    }
}

impl Folder {
    fn job(&self) -> Job {
        Job::of(self.temp.to_str().unwrap())
    }
    /// The script of a stand-in that writes `<marks>/<name>` once it is up, then runs `body`.
    fn up(&self, name: &str, body: &str) -> String {
        format!("echo $$ > '{}'; {body}", paddock_testkit::path::text(self.marks.join(name)))
    }
    fn wait_for(&self, names: &[&str]) {
        for n in names {
            until(n, || self.marks.join(n).exists());
        }
    }
    fn pid(&self, name: &str) -> ProcessId {
        let text = std::fs::read_to_string(self.marks.join(name)).unwrap();
        ProcessId::of(text.trim().parse().unwrap()).unwrap()
    }
}

/// A shell stand-in under `reaper` that carries nothing of its job unless the caller gives it something: `cwd` and `token` are the marks.
#[allow(clippy::disallowed_methods, reason = "a test starts its own stand-in processes under a Reaper")]
fn sh(reaper: &Reaper, script: &str, cwd: &Path, token: Option<&Path>) -> Child {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", script])
        .current_dir(cwd)
        .env_remove(TOKEN)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(t) = token {
        cmd.env(TOKEN, t);
    }
    reaper.spawn(&mut cmd).unwrap()
}

/// Waits until `f` holds (a stand-in is up when its marker file exists), through the testkit's backoff wait.
fn until(what: &str, f: impl FnMut() -> bool) {
    paddock_testkit::wait::wait_until_within(what, Duration::from_secs(20), f);
}

fn members(job: &Job) -> Vec<paddock_dev::ci_isolation::processes::Member> {
    let table = survey(TOKEN);
    let protect = self_and_ancestors(&table, std::process::id());
    members_of(&table, job, &protect)
}

fn ended(id: ProcessId) -> bool {
    paddock_proc::own::has_ended(id)
}

#[test]
fn the_end_of_a_job_stops_a_process_that_ignores_sigterm_a_grandchild_and_a_detached_one() {
    let reaper = Reaper::new("the job group test");
    let f = folder();
    // What a step leaves behind, each marked only by the job's folder as its current folder (the way a step's script has it):
    // 1 ignores SIGTERM; 2 has a grandchild that ignores it too; 3 left its session (a detached process).
    let ignore = f.up("ignores", "trap '' TERM; while :; do sleep 1; done");
    let grand = f.up(
        "grand",
        &format!(
            "(echo $$ > '{}'; trap '' TERM; while :; do sleep 1; done) & wait",
            paddock_testkit::path::text(f.marks.join("grandchild"))
        ),
    );
    let detached = f.up(
        "detached",
        "exec perl -e 'use POSIX; POSIX::setsid(); $SIG{TERM}=\"IGNORE\"; sleep 300'",
    );
    let _a = sh(&reaper, &ignore, &f.root, None);
    let _b = sh(&reaper, &grand, &f.root, None);
    let _c = sh(&reaper, &detached, &f.root, None);
    f.wait_for(&["ignores", "grand", "grandchild", "detached"]);
    // The positive control of the selection: another job's process (its own folder, its own token) must survive the stop.
    let other = folder();
    let _o = sh(
        &reaper,
        &other.up("other", "while :; do sleep 1; done"),
        &other.root,
        Some(&other.temp),
    );
    other.wait_for(&["other"]);

    let found = members(&f.job());
    assert!(found.len() >= 4, "three stand-ins and a grandchild at least, got {found:?}");
    assert!(found.iter().all(|m| m.job == f.temp.to_str().unwrap()));
    let began = Instant::now();
    let results = stop(&found, SHORT);
    assert_eq!(results.len(), found.len());
    assert!(
        began.elapsed() < Duration::from_secs(15),
        "the stop is bounded by the grace, in parallel"
    );
    for m in &found {
        assert!(ended(m.id), "{} is still running: {}", m.id.pid(), m.command);
    }
    // The ones that ignore SIGTERM needed the kill; the stop says so per process.
    assert!(
        results.iter().any(|(_, how)| *how == paddock_proc::Terminated::Killed),
        "{results:?}"
    );
    assert!(!ended(other.pid("other")), "another job's process was ended");
    assert!(!members(&other.job()).is_empty());
}

/// Each mark finds a process on its own (the three are separate, because macOS hides the environment of Apple's programs).
#[test]
fn each_mark_alone_finds_a_process_of_the_job_and_none_finds_one_of_another() {
    let reaper = Reaper::new("the marks test");
    let f = folder();
    // cwd only: no token, a command line that names nothing of the job.
    let _cwd = sh(&reaper, &f.up("cwd", "sleep 300"), &f.root, None);
    // command line only: the script text names the job's temp folder; the current folder is elsewhere.
    let _args = sh(
        &reaper,
        &f.up(
            "args",
            &format!("sleep 300 # {}", paddock_testkit::path::text(f.temp.join("x.sock"))),
        ),
        &f.elsewhere,
        None,
    );
    // environment only: a program whose environment macOS shows (this very tool, waiting), current folder elsewhere.
    #[allow(clippy::disallowed_methods, reason = "a test starts its own stand-in under a Reaper")]
    let mut env_only = crate::dev_bin::command();
    env_only
        .args(["wait", "--file"])
        .arg(f.elsewhere.join("never"))
        .args(["--line", "never", "--max", "120s"])
        .current_dir(&f.elsewhere)
        .env(TOKEN, &f.temp)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let env_child = reaper.spawn_test(env_only).unwrap();
    f.wait_for(&["cwd", "args"]);
    let mut table = Vec::new();
    until("the environment of the stand-in to be readable", || {
        table = survey(TOKEN);
        table.iter().any(|r| r.id.pid() == env_child.id() && r.value.is_some())
    });
    let job = f.job();
    let by_pid = |pid: u32| {
        table
            .iter()
            .find(|r| r.id.pid() == pid)
            .unwrap_or_else(|| panic!("pid {pid} is not in the table"))
    };
    assert!(job.owns(by_pid(f.pid("cwd").pid())), "the current folder finds it");
    assert!(job.owns(by_pid(f.pid("args").pid())), "the command line finds it");
    assert!(
        job.owns(by_pid(env_child.id())),
        "the environment finds it (a program whose environment the OS shows)"
    );
    // None of the three belongs to another job.
    let other = folder();
    let other_job = other.job();
    for pid in [f.pid("cwd").pid(), f.pid("args").pid(), env_child.id()] {
        assert!(!other_job.owns(by_pid(pid)), "pid {pid} was taken for another job's process");
    }
    // A longer name is not the job's folder: `<root>x` is another folder, not `<root>`.
    assert!(!Job::of(&format!("{}x/tmp", f.root.display())).owns(by_pid(f.pid("cwd").pid())));
}

#[test]
fn the_process_that_runs_the_stop_and_its_parents_are_never_members() {
    let reaper = Reaper::new("the protect test");
    let f = folder();
    let _a = sh(&reaper, &f.up("a", "sleep 300"), &f.root, None);
    f.wait_for(&["a"]);
    let mut table = survey(TOKEN);
    // This test process stands for the release step: its current folder is the job's, as a step's is, and its parent chain is its own.
    let me = std::process::id();
    let job = f.job();
    for r in table.iter_mut().filter(|r| r.id.pid() == me) {
        r.cwd = Some(f.root.to_str().unwrap().to_string());
    }
    assert!(
        table.iter().any(|r| r.id.pid() == me && job.owns(r)),
        "the caller is a member by its folder"
    );
    let protect = self_and_ancestors(&table, me);
    assert!(protect.contains(&me));
    let found = members_of(&table, &job, &protect);
    assert!(
        found.iter().all(|m| m.id.pid() != me && !protect.contains(&m.id.pid())),
        "the caller or its ancestor was listed: {found:?}"
    );
    assert!(!found.is_empty());
}

#[test]
fn leftovers_of_an_earlier_job_are_reported_and_a_running_jobs_are_not() {
    let reaper = Reaper::new("the sweep test");
    let gone = folder();
    let live = folder();
    // The finished job's process names its folder in its command line (a script path), as `bash <job>/tmp/<id>.sh` does.
    let _g = sh(
        &reaper,
        &gone.up(
            "gone",
            &format!("sleep 300 # {}", paddock_testkit::path::text(gone.temp.join("step.sh"))),
        ),
        &gone.elsewhere,
        None,
    );
    let _l = sh(
        &reaper,
        &live.up(
            "live",
            &format!("sleep 300 # {}", paddock_testkit::path::text(live.temp.join("step.sh"))),
        ),
        &live.elsewhere,
        None,
    );
    gone.wait_for(&["gone"]);
    live.wait_for(&["live"]);
    let table = survey(TOKEN);
    let protect = self_and_ancestors(&table, std::process::id());
    // The runner removes a finished job's folder.
    std::fs::remove_dir_all(&gone.root).unwrap();
    let exists = |p: &str| Path::new(p).exists();
    // Both jobs live in `<work>`, each under its own tempdir here: the work folder of the two differs, so each is asked about its own.
    let work = |f: &Folder| f.job().work().unwrap().to_string();
    let left = leftovers(&table, Some(&work(&gone)), &exists, &protect);
    let ours: Vec<_> = left.iter().filter(|m| m.id == gone.pid("gone")).collect();
    assert_eq!(ours.len(), 1, "the finished job's process is a leftover: {left:?}");
    assert_eq!(ours[0].job, gone.root.to_str().unwrap());
    assert!(
        left.iter().all(|m| m.id != live.pid("live")),
        "a running job's process is no leftover"
    );
    let left = leftovers(&table, Some(&work(&live)), &exists, &protect);
    assert!(
        left.iter().all(|m| m.id != live.pid("live")),
        "a running job's process is no leftover"
    );
    // Only the stand-in of this test is ended below; the table may list others' leftovers of the real machine, which are never touched.
    let mine: Vec<_> = leftovers(&table, Some(&work(&gone)), &exists, &protect)
        .into_iter()
        .filter(|m| m.id == gone.pid("gone"))
        .collect();
    stop(&mine, SHORT);
    assert!(mine.iter().all(|m| ended(m.id)));
}

#[test]
fn positive_control_a_process_with_none_of_the_marks_is_no_member() {
    // The control of the selection (ER-33): a stand-in with nothing of the job is no member of any job.
    let reaper = Reaper::new("the control test");
    let f = folder();
    let _c = sh(&reaper, &f.up("plain", "sleep 300"), &f.elsewhere, None);
    f.wait_for(&["plain"]);
    let plain = f.pid("plain");
    // The stand-in is in the table (the survey can see it), and still no mark of the job is on it.
    until("the stand-in to be in the process table", || {
        survey(TOKEN).iter().any(|r| r.id == plain)
    });
    let found = members(&f.job());
    assert!(
        found.iter().all(|m| m.id != plain),
        "a process with none of the marks was taken for a member: {found:?}"
    );
}
