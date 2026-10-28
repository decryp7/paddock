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
    let (_, v, _) = dev(&["ci", "slot", "status", "--max", "2", "--root", root.to_str().unwrap()]);
    assert_eq!(v["slots"][0]["busy"], true);
    assert_eq!(v["slots"][1]["busy"], false);
    assert_eq!(v["slots"][0]["owner"].as_str().map(|o| o.ends_with("job")), Some(true));
    let (code, v, _) = dev(&["ci", "slot", "release", "--slot", "s1", "--root", root.to_str().unwrap()]);
    assert_eq!((code, v["released"].clone()), (0, true.into()));
    assert!(is_free(&root, 1));
    assert!(job.exists(), "release does not touch the job folder");
    let (code, v, _) = dev(&["ci", "slot", "release", "--slot", "bogus", "--root", root.to_str().unwrap()]);
    assert_eq!((code, v["kind"].clone()), (2, "usage".into()));
}

#[test]
fn a_full_house_is_refused_and_bad_arguments_are_usage() {
    let d = tempfile::tempdir().unwrap();
    let (root, job) = (d.path().join("ci"), d.path().join("job"));
    std::fs::create_dir(&job).unwrap();
    assert_eq!(acquire(&root, &job, "1").0, 0);
    let (code, v, err) = acquire(&root, &job, "1");
    assert_eq!((code, v["kind"].clone()), (4, "refused".into()));
    assert!(err.contains("busy"));
    assert_eq!(acquire(&root, &job, "0").0, 2);
    assert_eq!(acquire(&root, &d.path().join("missing"), "1").0, 2);
    assert_eq!(dev(&["ci", "slot", "status", "--max", "0", "--root", root.to_str().unwrap()]).0, 2);
    dev(&["ci", "slot", "release", "--slot", "s1", "--root", root.to_str().unwrap()]);
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
    for n in 1..=SLOTS as u32 {
        dev(&[
            "ci",
            "slot",
            "release",
            "--slot",
            &format!("s{n}"),
            "--root",
            root.to_str().unwrap(),
        ]);
    }
}

/// A pipe the job's runner waits on must not be kept open by the holder (a descriptor above 2 inherited by `acquire`).
#[cfg(unix)]
#[test]
fn the_holder_does_not_keep_an_inherited_pipe_open() {
    let d = tempfile::tempdir().unwrap();
    let (root, job) = (d.path().join("ci"), d.path().join("job"));
    std::fs::create_dir(&job).unwrap();
    let script = r#"exec 7>&1 1>/dev/null; exec "$0" ci slot acquire --max 1 --owner "$1" --root "$2" --poll-ms 20"#;
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
    dev(&["ci", "slot", "release", "--slot", "s1", "--root", root.to_str().unwrap()]);
}
