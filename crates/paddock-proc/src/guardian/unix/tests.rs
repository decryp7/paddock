//! The guardian's logic against a scripted [`Ends`], and the whole guard against a real owner process that is killed with
//! SIGKILL (the CI slot whose test process was killed and left a headless browser running for 2.5 h). The owner is this
//! test binary run again on [`guard_owner_process`]; each of its children is a shell with a grandchild in its group, as a
//! browser has helpers.

use super::*;
use crate::{RunState, Supervised, kill_tree, run_state};
use std::io::BufReader;

/// Not a test: the guardian of the owner processes below (this binary run with [`GUARDIAN_ENV`]).
#[test]
fn guardian_entry() {
    run_if_requested();
}

const ENTRY: &str = "guardian::unix::tests::guardian_entry";
const OWNER_ENV: &str = "PADDOCK_GUARD_TEST_OWNER_FOLDERS";
const OWNER_TEST: &str = "guardian::unix::tests::guard_owner_process";

// ---- the logic ----

#[derive(Default)]
struct Script {
    /// Groups whose keeper no longer holds them.
    unheld: Vec<u32>,
    /// How many looks each ended group takes to empty.
    empties_after: u32,
    looks: u32,
    ended: Vec<u32>,
    removed: Vec<PathBuf>,
    lines: Vec<String>,
}

impl Ends for Script {
    fn end_group(&mut self, g: &Guarded) -> bool {
        if self.unheld.contains(&g.group) {
            return false;
        }
        self.ended.push(g.group);
        self.looks = 0;
        true
    }
    fn group_exists(&mut self, _: u32) -> bool {
        self.looks < self.empties_after
    }
    fn remove(&mut self, folder: &Path) -> std::io::Result<()> {
        self.removed.push(folder.to_path_buf());
        Ok(())
    }
    fn pause(&mut self) {
        self.looks += 1;
    }
    fn log(&mut self, line: &str) {
        self.lines.push(line.to_string());
    }
}

fn add(group: u32, folder: Option<&str>) -> String {
    let folder = folder.map_or_else(|| "-".to_string(), |f| hex(f.as_bytes()));
    format!("add {group} {} 77 {folder}\n", group + 1)
}

#[test]
fn lines_round_trip_and_odd_ones_are_refused() {
    assert_eq!(
        parse(add(10, Some("/tmp/a b\n%")).trim()),
        Some(Line::Add(Guarded {
            group: 10,
            keeper: ProcessId::new(11, crate::StartTime::from_micros(77)),
            folder: Some(PathBuf::from("/tmp/a b\n%")),
        }))
    );
    assert_eq!(parse("del 10"), Some(Line::Del(10)));
    for odd in [
        "",
        "add 1 2 3",
        "add x 2 3 -",
        "add 1 2 3 abc",
        "add 1 2 3 zz",
        "del",
        "del -1",
        "done",
    ] {
        assert_eq!(parse(odd), None, "{odd:?}");
    }
}

#[test]
fn at_end_of_file_every_group_still_registered_is_ended_and_its_folder_removed() {
    let input = [
        add(10, Some("/p/a")),
        add(20, None),
        add(30, Some("/p/c")),
        "del 20\n".into(),
        "junk\n".into(),
    ]
    .concat();
    let mut s = Script {
        empties_after: 2,
        ..Script::default()
    };
    guard(input.as_bytes(), &mut s, 5);
    assert_eq!(s.ended, [10, 30]);
    assert_eq!(s.removed, [PathBuf::from("/p/a"), PathBuf::from("/p/c")]);
    assert!(
        s.lines.iter().any(|l| l.contains("ignored")),
        "{:?}",
        s.lines
    );
    assert!(
        s.lines.iter().any(|l| l.contains("2 child group(s)")),
        "{:?}",
        s.lines
    );
    assert!(
        s.lines
            .iter()
            .any(|l| l == "ended group 10 (emptied) and removed /p/a"),
        "{:?}",
        s.lines
    );
}

#[test]
fn a_group_its_keeper_no_longer_holds_is_left_alone_and_logged() {
    let input = add(10, Some("/p/a"));
    let mut s = Script {
        unheld: vec![10],
        ..Script::default()
    };
    guard(input.as_bytes(), &mut s, 5);
    assert!(s.ended.is_empty() && s.removed.is_empty());
    assert!(
        s.lines.iter().any(|l| l.starts_with("left group 10")),
        "{:?}",
        s.lines
    );
}

#[test]
fn a_group_that_does_not_empty_is_waited_for_a_bounded_number_of_looks() {
    let mut s = Script {
        empties_after: u32::MAX,
        ..Script::default()
    };
    guard(add(10, None).as_bytes(), &mut s, 7);
    assert_eq!(s.looks, 7);
    assert!(
        s.lines
            .iter()
            .any(|l| l == "ended group 10 (still had members)"),
        "{:?}",
        s.lines
    );
}

#[test]
fn an_owner_that_ended_everything_leaves_the_guardian_nothing_to_do() {
    let mut s = Script::default();
    guard(
        [add(10, Some("/p/a")), "del 10\n".into()]
            .concat()
            .as_bytes(),
        &mut s,
        5,
    );
    assert!(s.ended.is_empty() && s.removed.is_empty() && s.lines.is_empty());
}

// ---- the whole guard ----

/// Not a test: the owner process. Starts one guarded child per folder in [`OWNER_ENV`] (`:`-separated), names their
/// pids and blocks until it is killed (or its standard input ends, when the test itself has gone).
#[test]
fn guard_owner_process() {
    let Ok(folders) = std::env::var(OWNER_ENV) else {
        return;
    };
    enable_in_test_binary(ENTRY);
    let mut held = Vec::new();
    for folder in folders.split(':') {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "sleep 60 & echo \"$!\"; wait"])
            .stdout(Stdio::piped());
        let mut s = Supervised::spawn_owning_folder(cmd, None, Path::new(folder)).unwrap();
        let mut line = String::new();
        BufReader::new(s.take_stdout().unwrap())
            .read_line(&mut line)
            .unwrap();
        println!("child {} grandchild {}", s.id().unwrap(), line.trim());
        held.push(s);
    }
    println!("ready");
    let _ = std::io::stdin().read_line(&mut String::new());
    std::process::exit(0);
}

/// Ends what a failed test would leave behind (ER-14), by identity, then fails with `why`.
fn fail(ids: &[ProcessId], why: &str) -> ! {
    for id in ids {
        kill_tree(*id);
    }
    panic!("{why}");
}

#[test]
fn an_owner_killed_with_three_guarded_children_leaves_none_and_no_folder() {
    let dir = tempfile::tempdir().unwrap();
    let folders: Vec<PathBuf> = (0..3)
        .map(|i| dir.path().join(format!("profile-{i}")))
        .collect();
    for f in &folders {
        std::fs::create_dir_all(f.join("Default")).unwrap();
    }
    let log = dir.path().join("guardian.log");
    let joined = folders
        .iter()
        .map(|f| f.display().to_string())
        .collect::<Vec<_>>()
        .join(":");
    let mut owner = Command::new(std::env::current_exe().unwrap())
        .args([OWNER_TEST, "--exact", "--nocapture"])
        .env(OWNER_ENV, &joined)
        .env(LOG_ENV, &log)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let owner_id = ProcessId::of(owner.id()).unwrap();
    let mut out = BufReader::new(owner.stdout.take().unwrap());
    let mut ids = Vec::new();
    let mut line = String::new();
    while line.trim() != "ready" {
        line.clear();
        if out.read_line(&mut line).unwrap() == 0 {
            fail(
                &[owner_id],
                "the owner ended before its children were ready",
            );
        }
        let w: Vec<&str> = line.split_whitespace().collect();
        if let ["child", c, "grandchild", g] = w[..] {
            for p in [c, g] {
                ids.push(ProcessId::of(p.parse().unwrap()).unwrap());
            }
        }
    }
    // N children cost N + 1 more processes: a keeper each and one guardian. Below the owner: 3 children, 3 grandchildren,
    // 3 keepers and the guardian.
    let helpers = crate::descendants_of(owner_id)
        .len()
        .saturating_sub(ids.len());
    if helpers != 4 {
        fail(
            &[owner_id],
            &format!("3 guarded children need 4 helper processes, not {helpers}"),
        );
    }
    // SIGKILL through the handle of this process's own child: none of the owner's own clean-up runs.
    owner.kill().unwrap();
    owner.wait().unwrap();
    let gone = paddock_testkit::wait::poll_until_within(paddock_testkit::wait::HANG_GUARD, || {
        ids.iter().all(|id| run_state(*id) == RunState::Exited)
            && folders.iter().all(|f| !f.exists())
    });
    if !gone {
        fail(
            &ids,
            "the orphaned children's groups did not end, or their folders stayed",
        );
    }
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(text.matches(") and removed ").count(), 3, "{text}");
}

#[test]
fn an_owner_that_ends_its_child_itself_keeps_the_folder_and_ends_the_keeper() {
    enable_in_test_binary(ENTRY);
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("kept");
    std::fs::create_dir_all(&folder).unwrap();
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", "sleep 60"]);
    let mut s = Supervised::spawn_owning_folder(cmd, None, &folder).unwrap();
    let child = s.process().unwrap();
    let keeper = s.guard.as_ref().and_then(|r| ProcessId::of(r.keeper.id()));
    let Some(keeper) = keeper else {
        fail(&[child], "the child was not registered");
    };
    assert_eq!(
        crate::sys::group_of(keeper.pid()),
        Some(child.pid()),
        "the keeper holds the child's group"
    );
    s.kill();
    assert_eq!(run_state(child), RunState::Exited);
    assert_eq!(
        run_state(keeper),
        RunState::Exited,
        "the keeper is ended and reaped with the registration"
    );
    assert!(
        folder.exists(),
        "a child its owner ended keeps its folder for the owner"
    );
}
