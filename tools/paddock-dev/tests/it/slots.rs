//! `dev ci slot` end to end with the real binary: acquire, status, release, a race of many processes, a killed holder and an
//! owner folder that disappears.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
use paddock_slots::{FsLocks, SlotId, SlotLocks};
use serde_json::Value;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const POLL: &str = "20";

fn dev(args: &[&str]) -> (i32, Value, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_paddock-dev"))
        .args(["--json"])
        .args(args)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    (
        out.status.code().unwrap(),
        serde_json::from_str(text.trim()).unwrap_or(Value::Null),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn acquire(root: &Path, owner: &Path, max: &str) -> (i32, Value, String) {
    dev(&[
        "ci",
        "slot",
        "acquire",
        "--max",
        max,
        "--owner",
        owner.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--poll-ms",
        POLL,
        // Whatever a failing test leaves behind ends by itself (tests leave no process on the machine).
        "--max-hold-secs",
        "120",
        // A full house is refused after 50 ms, not the production 5 s.
        "--acquire-grace-ms",
        "50",
    ])
}

/// Takes a slot with the real binary and returns (slot, token).
fn take(root: &Path, owner: &Path, max: &str) -> (String, String) {
    let (code, v, err) = acquire(root, owner, max);
    assert_eq!(code, 0, "{v} {err}");
    (v["slot"].as_str().unwrap().to_string(), v["token"].as_str().unwrap().to_string())
}

fn release(root: &Path, slot: &str, token: &str) -> (i32, Value, String) {
    dev(&[
        "ci",
        "slot",
        "release",
        "--slot",
        slot,
        "--token",
        token,
        "--root",
        root.to_str().unwrap(),
    ])
}

fn is_free(root: &Path, n: u32) -> bool {
    FsLocks::new(root).try_lock(SlotId::new(n).unwrap()).unwrap().is_some()
}

fn wait_until_free(root: &Path, n: u32) {
    let locks = FsLocks::new(root);
    let id = SlotId::new(n).unwrap();
    assert!(
        paddock_slots::wait_free(&locks, id, Duration::from_secs(60), Duration::from_millis(20)).unwrap(),
        "slot s{n} never became free"
    );
}

#[test]
fn acquire_status_and_release() {
    let d = tempfile::tempdir().unwrap();
    let (root, job) = (d.path().join("ci"), d.path().join("job"));
    std::fs::create_dir(&job).unwrap();
    let (code, v, _) = acquire(&root, &job, "2");
    assert_eq!((code, v["slot"].as_str()), (0, Some("s1")), "{v}");
    assert!(v["holder_pid"].as_u64().is_some());
    let token = v["token"].as_str().unwrap().to_string();
    assert!(!token.is_empty());
    let (_, v, _) = dev(&["ci", "slot", "status", "--max", "2", "--root", root.to_str().unwrap()]);
    assert_eq!(v["slots"][0]["busy"], true);
    assert_eq!(v["slots"][1]["busy"], false);
    assert_eq!(v["slots"][0]["owner"].as_str().map(|o| o.ends_with("job")), Some(true));
    let (code, v, _) = release(&root, "s1", &token);
    assert_eq!((code, v["released"].clone()), (0, true.into()));
    assert!(is_free(&root, 1));
    assert!(job.exists(), "release does not touch the job folder");
    let (code, v, _) = release(&root, "bogus", "t");
    assert_eq!((code, v["kind"].clone()), (2, "usage".into()));
}

#[test]
fn a_full_house_is_refused_and_bad_arguments_are_usage() {
    let d = tempfile::tempdir().unwrap();
    let (root, job) = (d.path().join("ci"), d.path().join("job"));
    std::fs::create_dir(&job).unwrap();
    let (_, token) = take(&root, &job, "1");
    let (code, v, err) = acquire(&root, &job, "1");
    assert_eq!((code, v["kind"].clone()), (4, "refused".into()));
    assert!(err.contains("busy"));
    assert_eq!(acquire(&root, &job, "0").0, 2);
    assert_eq!(acquire(&root, &d.path().join("missing"), "1").0, 2);
    assert_eq!(dev(&["ci", "slot", "status", "--max", "0", "--root", root.to_str().unwrap()]).0, 2);
    release(&root, "s1", &token);
}

#[test]
fn the_slot_frees_when_the_job_folder_is_removed() {
    let d = tempfile::tempdir().unwrap();
    let (root, job) = (d.path().join("ci"), d.path().join("job"));
    std::fs::create_dir(&job).unwrap();
    assert_eq!(acquire(&root, &job, "1").0, 0);
    assert!(!is_free(&root, 1));
    std::fs::remove_dir(&job).unwrap();
    wait_until_free(&root, 1);
}

#[test]
fn a_killed_holder_frees_its_slot_at_once() {
    let d = tempfile::tempdir().unwrap();
    let (root, job) = (d.path().join("ci"), d.path().join("job"));
    std::fs::create_dir(&job).unwrap();
    let (_, v, _) = acquire(&root, &job, "1");
    let pid = v["holder_pid"].as_u64().unwrap().to_string();
    assert!(!is_free(&root, 1));
    let killed = if cfg!(windows) {
        Command::new("taskkill").args(["/F", "/PID", &pid]).status()
    } else {
        Command::new("kill").args(["-9", &pid]).status()
    };
    assert!(killed.unwrap().success());
    wait_until_free(&root, 1);
    assert!(job.exists());
}

#[test]
fn racing_acquirers_never_share_a_slot() {
    const SLOTS: usize = 3;
    const RACERS: usize = 8;
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("ci");
    let jobs: Vec<_> = (0..RACERS).map(|i| d.path().join(format!("job{i}"))).collect();
    jobs.iter().for_each(|j| std::fs::create_dir(j).unwrap());
    let start = std::sync::Barrier::new(RACERS);
    let results: Vec<(i32, Value, String)> = std::thread::scope(|s| {
        let handles: Vec<_> = jobs
            .iter()
            .map(|job| {
                let (root, start) = (&root, &start);
                s.spawn(move || {
                    start.wait();
                    acquire(root, job, &SLOTS.to_string())
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut got: Vec<String> = results
        .iter()
        .filter(|r| r.0 == 0)
        .map(|r| r.1["slot"].as_str().unwrap().to_string())
        .collect();
    got.sort();
    assert_eq!(got, ["s1", "s2", "s3"], "exactly the three slots, each once: {results:?}");
    assert!(
        results.iter().filter(|r| r.0 != 0).all(|r| r.0 == 4),
        "the others are refused, not failed"
    );
    for r in results.iter().filter(|r| r.0 == 0) {
        let (slot, token) = (r.1["slot"].as_str().unwrap(), r.1["token"].as_str().unwrap());
        assert_eq!(release(&root, slot, token).0, 0);
    }
}

/// The CI kit's acquire (job-isolation.sh) with as many jobs as slots, all at once: every job gets a slot of its own.
#[test]
fn eight_parallel_acquires_get_eight_distinct_slots() {
    const N: usize = 8;
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("ci");
    let jobs: Vec<_> = (0..N).map(|i| d.path().join(format!("job{i}"))).collect();
    jobs.iter().for_each(|j| std::fs::create_dir(j).unwrap());
    let start = std::sync::Barrier::new(N);
    let results: Vec<(i32, Value, String)> = std::thread::scope(|s| {
        let handles: Vec<_> = jobs
            .iter()
            .map(|job| {
                let (root, start) = (&root, &start);
                s.spawn(move || {
                    start.wait();
                    acquire(root, job, &N.to_string())
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert!(results.iter().all(|r| r.0 == 0), "every job gets a slot: {results:?}");
    let mut got: Vec<String> = results.iter().map(|r| r.1["slot"].as_str().unwrap().to_string()).collect();
    got.sort();
    got.dedup();
    assert_eq!(got.len(), N, "eight distinct slots: {results:?}");
    for n in 1..=N as u32 {
        assert!(!is_free(&root, n), "s{n} is held by its job's holder");
    }
    for r in &results {
        assert_eq!(release(&root, r.1["slot"].as_str().unwrap(), r.1["token"].as_str().unwrap()).0, 0);
    }
    for n in 1..=N as u32 {
        assert!(is_free(&root, n));
    }
}

fn idle_run(root: &Path, slot: &str, command: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_paddock-dev"))
        .args(["ci", "slot", "idle-run", "--slot", slot, "--root", root.to_str().unwrap(), "--"])
        .args(command)
        .output()
        .unwrap()
}

/// The disk sweep's guarantee: the command runs only on an idle slot and the slot stays locked until it ends, so no job can
/// take the slot while its folders are deleted; a held slot is skipped and its folders are left alone.
#[test]
fn idle_run_runs_only_on_an_idle_slot_and_holds_it_until_the_command_ends() {
    let d = tempfile::tempdir().unwrap();
    let (root, job) = (d.path().join("ci"), d.path().join("job"));
    std::fs::create_dir(&job).unwrap();
    let bin = env!("CARGO_BIN_EXE_paddock-dev");
    let r = root.to_str().unwrap();
    // Free: the command runs, and while it runs the slot is busy (it looks at the slot itself).
    let out = idle_run(&root, "s1", &[bin, "ci", "slot", "status", "--max", "1", "--root", r]);
    let said = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{said}");
    assert!(
        said.contains("s1 busy\n") && said.contains("s1 idle: ran"),
        "the command saw its slot locked: {said}"
    );
    assert!(is_free(&root, 1), "the lock goes with the command");
    // Held by a job: skipped, the command never runs.
    let (slot, token) = take(&root, &job, "1");
    let out = idle_run(&root, "s1", &[bin, "ci", "slot", "status", "--max", "1", "--root", r]);
    let said = String::from_utf8_lossy(&out.stdout);
    assert_eq!((out.status.code(), said.trim()), (Some(0), "s1 busy: skipped"));
    assert_eq!(release(&root, &slot, &token).0, 0);
    // A failing command is a failure; a bad slot, no command or a program that cannot start are reported.
    assert_eq!(
        idle_run(&root, "s1", &[bin, "ci", "slot", "status", "--max", "0", "--root", r])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(idle_run(&root, "s0", &[bin]).status.code(), Some(2));
    assert_eq!(idle_run(&root, "s1", &[]).status.code(), Some(2));
    let missing = d.path().join("no-such-program");
    let out = idle_run(&root, "s1", &[missing.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("could not be started"));
}

/// A pipe the job's runner waits on must not be kept open by the holder (a descriptor above 2 inherited by `acquire`).
#[cfg(unix)]
#[test]
fn the_holder_does_not_keep_an_inherited_pipe_open() {
    let d = tempfile::tempdir().unwrap();
    let (root, job) = (d.path().join("ci"), d.path().join("job"));
    std::fs::create_dir(&job).unwrap();
    let script = r#"exec 7>&1 1>/dev/null; exec "$0" ci slot acquire --max 1 --owner "$1" --root "$2" --poll-ms 20 --max-hold-secs 120"#;
    let (tx, rx) = std::sync::mpsc::channel();
    let (bin, job_arg, root_arg) = (
        env!("CARGO_BIN_EXE_paddock-dev"),
        job.display().to_string(),
        root.display().to_string(),
    );
    std::thread::spawn(move || {
        // `output()` returns only when every holder of the pipe's write end has closed it.
        let out = Command::new("sh").args(["-c", script, bin, &job_arg, &root_arg]).output().unwrap();
        let _ = tx.send(out.status.code());
    });
    let code = rx
        .recv_timeout(Duration::from_secs(60))
        .expect("the pipe stayed open: the holder kept an inherited descriptor");
    assert_eq!(code, Some(0));
    assert!(!is_free(&root, 1), "the holder is still running");
    // The step's output went to /dev/null, so the holder's own record gives the token to release with.
    let token = FsLocks::new(&root).read_owner(SlotId::new(1).unwrap()).unwrap().token;
    assert_eq!(release(&root, "s1", &token).0, 0);
    assert!(is_free(&root, 1));
}

/// Windows hands a child every inheritable handle of its parent, and Unix runs the same shape of check through the descriptor
/// test above: a step's output pipe must be closed when `acquire` ends, whatever the holder does.
#[test]
fn the_step_output_pipe_closes_when_acquire_ends_even_though_the_holder_lives_on() {
    let d = tempfile::tempdir().unwrap();
    let (root, job) = (d.path().join("ci"), d.path().join("job"));
    std::fs::create_dir(&job).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let (bin, job_arg, root_arg) = (
        env!("CARGO_BIN_EXE_paddock-dev"),
        job.display().to_string(),
        root.display().to_string(),
    );
    std::thread::spawn(move || {
        // `output()` returns only when every holder of the stdout and stderr pipes has closed it.
        let out = Command::new(bin)
            .args([
                "ci",
                "slot",
                "acquire",
                "--max",
                "1",
                "--owner",
                &job_arg,
                "--root",
                &root_arg,
                "--poll-ms",
                POLL,
                "--max-hold-secs",
                "120",
            ])
            .output()
            .unwrap();
        let _ = tx.send((out.status.code(), String::from_utf8_lossy(&out.stdout).trim().to_string()));
    });
    let (code, said) = rx
        .recv_timeout(Duration::from_secs(60))
        .expect("the step's pipe stayed open: the holder inherited it");
    assert_eq!(code, Some(0));
    let token = said.split_whitespace().nth(1).unwrap().to_string();
    assert!(!is_free(&root, 1), "the holder is still running");
    release(&root, "s1", &token);
}

/// The step's output pipe can reach `acquire` as an inherited handle that is not one of its standard handles: `./dev ci isolation
/// acquire` starts `ci slot acquire` with the default handle inheritance, so `acquire` inherits the step's pipe from it as a plain
/// handle. The Gitea runner waits 10 s for a step's pipe to close after the step ends, then ends every process of the step's job
/// object (gitea/runner internal/pkg/process/treekill.go), which killed a holder that had inherited it (run 8019, jobs 37039 and
/// 37040, where two jobs then shared slot s1). Here the test is `isolation acquire`: an inheritable pipe that `acquire` inherits as
/// a plain handle must be closed once `acquire` has ended, while its holder lives on.
#[cfg(windows)]
#[test]
#[allow(unsafe_code)] // one Win32 call that marks the test's own pipe handle inheritable
fn the_holder_does_not_keep_a_pipe_that_acquire_inherited_as_a_plain_handle() {
    use std::io::Read;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
    let d = tempfile::tempdir().unwrap();
    let (root, job) = (d.path().join("ci"), d.path().join("job"));
    std::fs::create_dir(&job).unwrap();
    let (mut reader, writer) = std::io::pipe().unwrap();
    // SAFETY: the handle is the test's own open pipe; only its inherit flag changes.
    assert_ne!(
        unsafe { SetHandleInformation(writer.as_raw_handle(), HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) },
        0
    );
    let (code, v, err) = acquire(&root, &job, "1");
    assert_eq!(code, 0, "{v} {err}");
    // `acquire` has ended; only the holder could still have the write end.
    drop(writer);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut rest = Vec::new();
        let _ = tx.send(reader.read_to_end(&mut rest).map(|_| ()));
    });
    let closed = rx.recv_timeout(Duration::from_secs(60));
    assert!(!is_free(&root, 1), "the holder is still running");
    // Release before the verdict, so a failure leaves no holder behind.
    let token = v["token"].as_str().unwrap().to_string();
    assert_eq!(release(&root, "s1", &token).0, 0);
    assert!(
        matches!(closed, Ok(Ok(()))),
        "the pipe stayed open after acquire ended: the holder inherited it ({closed:?})"
    );
}

#[test]
fn a_release_with_the_wrong_token_leaves_the_slot_with_its_holder() {
    let d = tempfile::tempdir().unwrap();
    let (root, job) = (d.path().join("ci"), d.path().join("job"));
    std::fs::create_dir(&job).unwrap();
    let (slot, token) = take(&root, &job, "1");
    let (code, _, _) = release(&root, &slot, "not-the-token");
    assert_eq!(
        code, 0,
        "a slot that is not yours is not yours to release, and that is not an error"
    );
    assert!(!is_free(&root, 1), "the holder keeps it");
    assert_eq!(release(&root, &slot, &token).0, 0);
    assert!(is_free(&root, 1));
}

#[test]
fn a_program_inside_the_job_folder_cannot_hold_a_slot() {
    let d = tempfile::tempdir().unwrap();
    let (root, job) = (d.path().join("ci"), d.path().join("job"));
    std::fs::create_dir_all(job.join("bin")).unwrap();
    let copy = job.join("bin").join(if cfg!(windows) { "paddock-dev.exe" } else { "paddock-dev" });
    std::fs::copy(env!("CARGO_BIN_EXE_paddock-dev"), &copy).unwrap();
    let out = Command::new(&copy)
        .args([
            "--json",
            "ci",
            "slot",
            "acquire",
            "--max",
            "1",
            "--owner",
            job.to_str().unwrap(),
            "--root",
            root.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stdout).contains("inside the job folder"));
    assert!(is_free(&root, 1), "no holder was started");
}

/// Sets the flag when it goes out of scope, so a panic in the test body still stops the prober (a flag set only on the success path
/// left the prober, and with it the test run, waiting forever).
struct StopOnDrop<'a>(&'a std::sync::atomic::AtomicBool);

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[test]
fn acquire_is_never_refused_because_a_status_probe_looked_at_the_slot() {
    let d = tempfile::tempdir().unwrap();
    let (root, job) = (d.path().join("ci"), d.path().join("job"));
    std::fs::create_dir(&job).unwrap();
    let stop = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|s| {
        s.spawn(|| {
            let locks = FsLocks::new(&root);
            // Its own deadline as well: the probe ends even if nothing ever sets the flag.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
            while !stop.load(std::sync::atomic::Ordering::Relaxed) && std::time::Instant::now() < deadline {
                let _ = paddock_slots::slot_states(&locks, 1);
            }
        });
        let _stop = StopOnDrop(&stop);
        for _ in 0..8 {
            let (slot, token) = take(&root, &job, "1");
            assert_eq!(release(&root, &slot, &token).0, 0);
        }
    });
}

/// The refusal says how long the holder kept trying (ER-27); a probe in flight is covered in paddock-slots (fs.rs).
#[test]
fn a_refusal_says_how_long_the_holder_kept_trying() {
    let d = tempfile::tempdir().unwrap();
    let (root, job) = (d.path().join("ci"), d.path().join("job"));
    std::fs::create_dir(&job).unwrap();
    let (_, token) = take(&root, &job, "1");
    let (code, v, err) = acquire(&root, &job, "1");
    assert_eq!((code, v["kind"].clone()), (4, "refused".into()));
    assert!(err.contains("kept trying for 0.1 s"), "{err}");
    assert_eq!(release(&root, "s1", &token).0, 0);
}

#[test]
fn failures_of_the_slot_files_are_reported_with_exit_1_by_every_command() {
    let d = tempfile::tempdir().unwrap();
    let (root, job) = (d.path().join("ci"), d.path().join("job"));
    std::fs::create_dir(&job).unwrap();
    let file_root = d.path().join("not-a-folder");
    std::fs::write(&file_root, "x").unwrap();
    let r = file_root.to_str().unwrap();
    assert_eq!(dev(&["ci", "slot", "status", "--max", "1", "--root", r]).0, 1);
    assert_eq!(release(&file_root, "s1", "t").0, 1);
    assert_eq!(
        dev(&["ci", "slot", "hold", "--max", "1", "--owner", job.to_str().unwrap(), "--root", r]).0,
        1
    );
    assert_eq!(
        dev(&["ci", "slot", "hold", "--max", "0", "--owner", job.to_str().unwrap(), "--root", r]).0,
        2
    );
    assert_eq!(acquire(&file_root, &job, "1").0, 1, "a CI home that is a file cannot be created");
    // The home exists but its slots folder is a file: the holder cannot take a slot and says nothing.
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("slots"), "x").unwrap();
    let (code, v, err) = acquire(&root, &job, "1");
    assert_eq!(code, 1, "{v}");
    assert!(err.contains("ended before it took a slot"), "{err}");
}

#[test]
fn a_holder_lets_go_at_its_lifetime_cap_while_the_job_folder_still_exists() {
    let d = tempfile::tempdir().unwrap();
    let (root, job) = (d.path().join("ci"), d.path().join("job"));
    std::fs::create_dir(&job).unwrap();
    let (code, _, err) = dev(&[
        "ci",
        "slot",
        "acquire",
        "--max",
        "1",
        "--owner",
        job.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--poll-ms",
        POLL,
        "--max-hold-secs",
        "1",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(!is_free(&root, 1), "held at first");
    wait_until_free(&root, 1);
    assert!(job.exists(), "the job folder was never removed");
}
