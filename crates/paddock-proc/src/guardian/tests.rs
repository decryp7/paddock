//! The orphan guard against a real owner process that is killed with SIGKILL (the CI slot whose test process was killed and
//! left a headless browser running for 2.5 h). The owner is this test binary run again on [`guard_owner_process`]; its child
//! is a shell with a grandchild in its group, as a browser has helpers.

use crate::{ProcessId, RunState, Supervised, run_state};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const OWNER_ENV: &str = "PADDOCK_GUARD_TEST_OWNER_FOLDER";
const OWNER_TEST: &str = "guardian::tests::guard_owner_process";

/// Not a test: the owner process. Starts the guarded child, names its pids and blocks until it is killed (or its standard
/// input ends, when the test itself has gone).
#[test]
fn guard_owner_process() {
    let Ok(folder) = std::env::var(OWNER_ENV) else {
        return;
    };
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", "sleep 60 & echo \"grandchild $!\"; wait"])
        .stdout(Stdio::piped());
    let mut s = Supervised::spawn_owning_folder(cmd, None, Path::new(&folder)).unwrap();
    let mut out = BufReader::new(s.take_stdout().unwrap());
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    println!("child {} {}", s.id().unwrap(), line.trim());
    let mut rest = String::new();
    // Blocks until the test kills this process.
    let _ = std::io::stdin().read_line(&mut rest);
    std::process::exit(0);
}

/// Starts the owner; returns it and the identities of its child and grandchild.
fn owner(folder: &Path) -> (std::process::Child, ProcessId, ProcessId) {
    let mut owner = Command::new(std::env::current_exe().unwrap())
        .args([OWNER_TEST, "--exact", "--nocapture"])
        .env(OWNER_ENV, folder)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(owner.stdout.take().unwrap());
    let mut line = String::new();
    let (child, grand) = loop {
        line.clear();
        assert!(
            out.read_line(&mut line).unwrap() > 0,
            "the owner ended before naming its child"
        );
        // child <pid> grandchild <pid>
        let w: Vec<&str> = line.split_whitespace().collect();
        if let ["child", c, "grandchild", g] = w[..] {
            break (c.parse::<u32>().unwrap(), g.parse::<u32>().unwrap());
        }
    };
    let id = |p| ProcessId::of(p).expect("the process runs, so its start time can be read");
    (owner, id(child), id(grand))
}

fn profile() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("chromium-profile");
    std::fs::create_dir_all(folder.join("Default")).unwrap();
    std::fs::write(folder.join("Default").join("Preferences"), "{}").unwrap();
    (dir, folder)
}

#[test]
fn a_child_whose_owner_is_killed_is_ended_with_its_group_and_its_folder_removed() {
    let (_dir, folder) = profile();
    let (mut owner, child, grand) = owner(&folder);
    assert_eq!(run_state(child), RunState::Running);
    assert_eq!(run_state(grand), RunState::Running);
    // SIGKILL through the handle of this process's own child: nothing of the owner's runs afterwards.
    owner.kill().unwrap();
    owner.wait().unwrap();
    paddock_testkit::wait::wait_until(
        "the orphaned child's group to end and its folder to go",
        || {
            run_state(child) == RunState::Exited
                && run_state(grand) == RunState::Exited
                && !folder.exists()
        },
    );
}

#[test]
fn an_owner_that_ends_its_child_itself_leaves_the_folder_to_its_owner() {
    let (_dir, folder) = profile();
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", "sleep 60"]);
    let mut s = Supervised::spawn_owning_folder(cmd, None, &folder).unwrap();
    let child = s.process().unwrap();
    let guardian = format!("paddock-guardian-{}", child.pid());
    assert!(
        !crate::processes_mentioning(&guardian).is_empty(),
        "the guardian runs"
    );
    s.kill();
    assert_eq!(run_state(child), RunState::Exited);
    // Disarmed: the guardian ends without acting, so the folder stays for its owner (a kept profile is not removed).
    paddock_testkit::wait::wait_until("the disarmed guardian to end", || {
        crate::processes_mentioning(&guardian).is_empty()
    });
    assert!(folder.exists());
}
