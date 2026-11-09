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
const OWNER_FOLDERS_ENV: &str = "PADDOCK_GUARD_TEST_OWNER_FOLDERS";
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
    fn remove(&mut self, folder: &Path) -> Result<(), String> {
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
    let keeper = ProcessId::new(group + 1, crate::StartTime::from_micros(77));
    format!("{}\n", add_line(group, keeper, folder.map(Path::new)))
}

#[test]
fn lines_round_trip_and_odd_ones_are_refused() {
    assert_eq!(
        parse(add(10, Some("/tmp/a b\n%")).trim_end_matches('\n')),
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
        "add x 2 3 0 -",
        "add 1 2 3 1 -",
        "add 1 2 3 3 abc",
        "add 1 2 3 2 zz",
        "add 1 2 3 0 2f",
        "del",
        "del -1",
        "done",
    ] {
        assert_eq!(parse(odd), None, "{odd:?}");
    }
}

/// Review P1-2: a line the owner did not finish (it died while writing it) has no newline. A prefix of a folder's hex
/// could name an ancestor folder (`/home/u` of `/home/u/profile`): such a line is dropped, as is one whose hex is shorter
/// than it declares.
#[test]
fn a_line_without_its_end_or_with_its_folder_cut_short_is_dropped() {
    let whole = add(10, Some("/p/a/profile"));
    let cut = &whole[..whole.len() - 1 - 8];
    let mut s = Script::default();
    guard(cut.as_bytes(), &mut s, 5);
    assert!(s.ended.is_empty() && s.removed.is_empty(), "{:?}", s.lines);
    assert!(
        s.lines
            .iter()
            .any(|l| l.starts_with("dropped a line without its end")),
        "{:?}",
        s.lines
    );
    let short = whole.replacen(&hex(b"/p/a/profile"), &hex(b"/p/a"), 1);
    let mut s = Script::default();
    guard(short.as_bytes(), &mut s, 5);
    assert!(s.ended.is_empty() && s.removed.is_empty(), "{:?}", s.lines);
}

/// The owner sends each line as one buffer with a single write.
#[test]
fn the_owner_sends_a_whole_line_in_one_write() {
    let (mut r, mut w) = std::io::pipe().unwrap();
    let long = "/x".repeat(2000);
    let line = add_line(
        7,
        ProcessId::new(8, crate::StartTime::from_micros(9)),
        Some(Path::new(&long)),
    );
    send(&mut w, &line).unwrap();
    drop(w);
    let mut got = String::new();
    std::io::Read::read_to_string(&mut r, &mut got).unwrap();
    assert_eq!(got, format!("{line}\n"));
    assert!(
        matches!(parse(got.trim_end_matches('\n')), Some(Line::Add(g)) if g.folder.as_deref() == Some(Path::new(&long)))
    );
}

// ---- who may be served, what may be removed, where it logs (review P1-1) ----

fn me() -> ProcessId {
    ProcessId::of(std::process::id()).unwrap()
}

#[test]
fn only_its_parent_running_the_same_executable_is_served() {
    let owner = me();
    let exe = Path::new("/opt/app/bin/app");
    assert!(check_owner(owner, owner.pid(), true, Some(exe), Some(exe)).is_ok());
    // A forged environment: the owner it names is not its parent.
    assert!(check_owner(owner, owner.pid() + 1, true, Some(exe), Some(exe)).is_err());
    // The parent runs another program (a shell that started this binary with a forged environment and input).
    assert!(
        check_owner(
            owner,
            owner.pid(),
            true,
            Some(Path::new("/bin/sh")),
            Some(exe)
        )
        .is_err()
    );
    assert!(check_owner(owner, owner.pid(), true, None, Some(exe)).is_err());
    // The pid names another process now (rule 53).
    assert!(check_owner(owner, owner.pid(), false, Some(exe), Some(exe)).is_err());
}

fn marked(dir: &Path, name: &str, owner: ProcessId) -> PathBuf {
    let f = std::fs::canonicalize(dir).unwrap().join(name);
    std::fs::create_dir_all(&f).unwrap();
    std::fs::write(f.join(MARKER), identity_text(owner)).unwrap();
    f
}

#[test]
fn only_a_canonical_folder_with_the_owners_marker_is_removed() {
    let d = tempfile::tempdir().unwrap();
    let owner = me();
    let good = marked(d.path(), "good", owner);
    assert_eq!(removable(&good, owner), Ok(()));
    // A folder of the user's without the owner's marker: a forged line cannot name it.
    let plain = std::fs::canonicalize(d.path()).unwrap().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    assert!(removable(&plain, owner).is_err());
    // The marker of another owner.
    let other = marked(
        d.path(),
        "other",
        ProcessId::new(owner.pid(), crate::StartTime::from_micros(1)),
    );
    assert!(removable(&other, owner).is_err());
    // Through a link: the link itself, and a path that goes through one.
    let link = d.path().join("link");
    std::os::unix::fs::symlink(&good, &link).unwrap();
    assert!(removable(&link, owner).is_err());
    let canonical_link = std::fs::canonicalize(d.path()).unwrap().join("link");
    assert!(removable(&canonical_link, owner).is_err());
    assert!(
        removable(&canonical_link.join("..").join("good"), owner).is_err(),
        "not canonical"
    );
    // A marker that is a link to a file naming the owner.
    let linked = std::fs::canonicalize(d.path())
        .unwrap()
        .join("linked-marker");
    std::fs::create_dir_all(&linked).unwrap();
    std::os::unix::fs::symlink(good.join(MARKER), linked.join(MARKER)).unwrap();
    assert!(removable(&linked, owner).is_err());
    // A path outside any marked folder (the user's home) and a relative one.
    assert!(removable(Path::new("/"), owner).is_err());
    assert!(removable(Path::new("good"), owner).is_err());
}

#[test]
fn the_log_is_never_opened_through_a_link_or_in_a_shared_folder() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("logs");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let log = dir.join("guardian.log");
    let f = open_log(&log).expect("a private folder of this user");
    drop(f);
    use std::os::unix::fs::MetadataExt;
    assert_eq!(std::fs::metadata(&log).unwrap().mode() & 0o777, 0o600);
    // A link where the log should be (to a file the attacker wants appended to).
    let target = d.path().join("victim");
    std::fs::write(&target, "").unwrap();
    let linked = dir.join("linked.log");
    std::os::unix::fs::symlink(&target, &linked).unwrap();
    assert!(open_log(&linked).is_none());
    // A folder others can write to (as /tmp).
    let shared = d.path().join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o1777)).unwrap();
    assert!(open_log(&shared.join("guardian.log")).is_none());
}

/// A guardian started by someone who is not its owner (a forged environment naming another process, any input) does
/// nothing and says so in its log.
#[test]
fn a_guardian_with_a_forged_owner_refuses_and_ends_nothing() {
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(
        d.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    // A victim: its own group, a keeper in it, and a folder marked for it.
    let mut victim = Command::new("/bin/sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .unwrap();
    let victim_id = ProcessId::of(victim.id()).unwrap();
    let mut keeper = Command::new(cat().unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .process_group(i32::try_from(victim.id()).unwrap())
        .spawn()
        .unwrap();
    let keeper_id = ProcessId::of(keeper.id()).unwrap();
    let folder = marked(d.path(), "profile", victim_id);
    let log = std::fs::canonicalize(d.path())
        .unwrap()
        .join("guardian.log");
    let mut g = Command::new(std::env::current_exe().unwrap())
        .args([ENTRY, "--exact"])
        .env(GUARDIAN_ENV, "1")
        .env(OWNER_ENV, identity_text(victim_id))
        .env(LOG_ENV, &log)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let line = add_line(victim.id(), keeper_id, Some(&folder));
    let mut input = g.stdin.take().unwrap();
    let _ = send_to(&mut input, &line);
    drop(input);
    let status = g.wait().unwrap();
    let alive = run_state(victim_id) == RunState::Running;
    let kept = folder.exists();
    // Ended through the handles of this test's own children before any assert can fail (ER-14).
    let _ = keeper.kill();
    let _ = keeper.wait();
    let _ = victim.kill();
    let _ = victim.wait();
    assert!(!status.success(), "{status}");
    assert!(alive && kept, "a forged owner made the guardian act");
    if crate::sys::effective_uid() == 0 {
        // Root: the guardian exits before anything, and the log variable is ignored for root.
        assert_eq!(
            status.code(),
            Some(3),
            "a guardian running as root serves nobody"
        );
        return;
    }
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(text.contains("refused to serve"), "{text}");
}

fn send_to(w: &mut impl Write, line: &str) -> std::io::Result<()> {
    w.write_all(format!("{line}\n").as_bytes())
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

/// Not a test: the owner process. Starts one guarded child per folder in [`OWNER_FOLDERS_ENV`] (`:`-separated), names their
/// pids and blocks until it is killed (or its standard input ends, when the test itself has gone).
#[test]
fn guard_owner_process() {
    let Ok(folders) = std::env::var(OWNER_FOLDERS_ENV) else {
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

/// [`fail`] unless `ok`.
fn fail_unless(ok: bool, ids: &[ProcessId], why: &str) {
    if !ok {
        fail(ids, why);
    }
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
        .env(OWNER_FOLDERS_ENV, &joined)
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
    if crate::sys::effective_uid() == 0 {
        // Root (the Linux CI containers): the guardian is off for root binaries (ADR 0076 amendment 2026-11-09), so a
        // killed owner's children stay. Asserted, then ended by identity; the guarded path runs as a user (macOS, and the
        // Linux run as uid 1000).
        if helpers != 0 {
            fail(
                &[owner_id],
                &format!("a root owner started {helpers} helper processes"),
            );
        }
        owner.kill().unwrap();
        owner.wait().unwrap();
        let stayed = ids.iter().all(|id| run_state(*id) == RunState::Running);
        fail_unless(
            stayed,
            &ids,
            "a root owner's children were ended without a guardian",
        );
        for id in &ids {
            kill_tree(*id);
        }
        return;
    }
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
    if crate::sys::effective_uid() == 0 {
        // Root: the guardian is off for root binaries (ADR 0076 amendment 2026-11-09): nothing is registered.
        let registered = s.guard.is_some();
        s.kill();
        assert!(
            !registered,
            "a root process registered a child with a guardian"
        );
        return;
    }
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

/// Tests never write into the user's folders (ER-14): a test binary's guardian has no log unless the test names one.
#[test]
fn a_test_binary_has_no_default_log() {
    enable_in_test_binary(ENTRY);
    if std::env::var_os(LOG_ENV).is_none() {
        assert_eq!(log_path(), None);
    }
}
