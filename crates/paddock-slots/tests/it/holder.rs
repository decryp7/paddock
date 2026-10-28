use paddock_slots::{
    FsLocks, HoldEnd, OwnerInfo, SlotError, SlotId, SlotLocks, acquire_first, hold, release,
    slot_states, wait_free,
};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

const POLL: Duration = Duration::from_millis(5);
const LONG: Duration = Duration::from_secs(30);

fn id(n: u32) -> SlotId {
    SlotId::new(n).unwrap()
}

#[test]
fn a_held_slot_is_busy_until_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let locks = FsLocks::new(dir.path());
    let (first, guard) = acquire_first(&locks, 2).unwrap().unwrap();
    assert_eq!(first, id(1));
    assert_eq!(acquire_first(&locks, 2).unwrap().unwrap().0, id(2));
    let states = slot_states(&locks, 2).unwrap();
    assert_eq!(
        states.iter().map(|s| s.busy).collect::<Vec<_>>(),
        [true, false]
    );
    drop(guard);
    assert_eq!(acquire_first(&locks, 2).unwrap().unwrap().0, id(1));
}

#[test]
fn owner_records_round_trip_and_bad_ones_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let locks = FsLocks::new(dir.path());
    std::fs::create_dir_all(dir.path().join("slots")).unwrap();
    assert!(locks.read_owner(id(1)).is_none());
    let info = OwnerInfo {
        owner: PathBuf::from("job folder/x"),
        pid: 42,
    };
    locks.write_owner(id(1), &info).unwrap();
    assert_eq!(locks.read_owner(id(1)), Some(info));
    std::fs::write(dir.path().join("slots/s2.owner"), "pid=x\nowner=y\n").unwrap();
    assert!(locks.read_owner(id(2)).is_none());
    locks.clear_owner(id(1));
    assert!(locks.read_owner(id(1)).is_none());
    assert!(!locks.release_requested(id(1)));
    locks.request_release(id(1)).unwrap();
    assert!(locks.release_requested(id(1)));
    locks.clear_release(id(1));
    assert!(!locks.release_requested(id(1)));
}

#[test]
fn an_unusable_root_is_a_typed_error() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("plain");
    std::fs::write(&file, "x").unwrap();
    let locks = FsLocks::new(&file);
    let err = locks.try_lock(id(1)).unwrap_err();
    assert!(matches!(err, SlotError::Io { .. }));
    assert!(err.to_string().contains("writable"));
    assert!(std::error::Error::source(&err).is_some());
    assert!(
        locks
            .write_owner(
                id(1),
                &OwnerInfo {
                    owner: PathBuf::new(),
                    pid: 1
                }
            )
            .is_err()
    );
    assert!(locks.request_release(id(1)).is_err());
    assert!(std::error::Error::source(&SlotError::Invalid("x".into())).is_none());
    assert!(
        SlotError::ReleaseTimeout("s1".into())
            .to_string()
            .contains("s1")
    );
}

#[test]
fn hold_announces_and_ends_when_the_owner_folder_goes() {
    let dir = tempfile::tempdir().unwrap();
    let owner = dir.path().join("job");
    std::fs::create_dir(&owner).unwrap();
    let locks = FsLocks::new(dir.path().join("ci"));
    let holder = {
        let (locks, owner) = (locks.clone(), owner.clone());
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let writer = ChannelWriter(tx);
        let t = std::thread::spawn(move || hold(&locks, 1, &owner, POLL, writer));
        (t, rx)
    };
    // Wait on the announcement, not on time.
    let said = holder.1.recv_timeout(LONG).unwrap();
    assert_eq!(said, b"s1\n");
    assert!(locks.read_owner(id(1)).is_some_and(|o| o.owner == owner));
    assert!(acquire_first(&locks, 1).unwrap().is_none());
    // A second holder with every slot busy reports `none`.
    let mut out = Vec::new();
    assert!(hold(&locks, 1, &owner, POLL, &mut out).unwrap().is_none());
    assert_eq!(out, b"none\n");
    std::fs::remove_dir(&owner).unwrap();
    let end = holder.0.join().unwrap().unwrap();
    assert_eq!(end, Some((id(1), HoldEnd::OwnerGone)));
    assert!(acquire_first(&locks, 1).unwrap().is_some());
    assert!(locks.read_owner(id(1)).is_none());
}

struct ChannelWriter(std::sync::mpsc::Sender<Vec<u8>>);

impl std::io::Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = self.0.send(buf.to_vec());
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn release_asks_the_holder_and_waits_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let owner = dir.path().join("job");
    std::fs::create_dir(&owner).unwrap();
    let locks = FsLocks::new(dir.path().join("ci"));
    let (tx, rx) = std::sync::mpsc::channel();
    let t = {
        let (locks, owner) = (locks.clone(), owner.clone());
        std::thread::spawn(move || hold(&locks, 1, &owner, POLL, ChannelWriter(tx)))
    };
    rx.recv_timeout(LONG).unwrap();
    release(&locks, id(1), LONG, POLL).unwrap();
    assert_eq!(t.join().unwrap().unwrap(), Some((id(1), HoldEnd::Released)));
    assert!(owner.exists(), "the job's folder is not touched");
    // Releasing a free slot is fine.
    release(&locks, id(1), LONG, POLL).unwrap();
}

#[test]
fn release_of_a_holder_that_ignores_the_request_times_out() {
    let dir = tempfile::tempdir().unwrap();
    let locks = FsLocks::new(dir.path());
    let (_, _guard) = acquire_first(&locks, 1).unwrap().unwrap();
    assert!(!wait_free(&locks, id(1), Duration::ZERO, POLL).unwrap());
    let err = release(&locks, id(1), Duration::from_millis(20), POLL).unwrap_err();
    assert!(matches!(err, SlotError::ReleaseTimeout(_)));
}

/// The child half of `a_killed_holder_frees_its_slot`: when started by that test it holds slot s1 until it is killed.
#[test]
fn killed_holder_child() {
    let (Ok(root), Ok(owner)) = (
        std::env::var("PADDOCK_SLOTS_TEST_ROOT"),
        std::env::var("PADDOCK_SLOTS_TEST_OWNER"),
    ) else {
        return;
    };
    let _ = hold(
        &FsLocks::new(root),
        1,
        &PathBuf::from(owner),
        Duration::from_millis(50),
        std::io::stdout(),
    );
}

/// Ends the child even when an assertion fails, so no process is left behind.
struct KillOnDrop(std::process::Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn a_killed_holder_frees_its_slot() {
    let dir = tempfile::tempdir().unwrap();
    let owner = dir.path().join("job");
    std::fs::create_dir(&owner).unwrap();
    let root = dir.path().join("ci");
    let mut child = KillOnDrop(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "holder::killed_holder_child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("PADDOCK_SLOTS_TEST_ROOT", &root)
            .env("PADDOCK_SLOTS_TEST_OWNER", &owner)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    // The harness prints "test ... " before the child's own output, so the announcement ends a line.
    let mut lines = BufReader::new(child.0.stdout.take().unwrap()).lines();
    assert!(
        lines.any(|l| l.is_ok_and(|l| l.ends_with("s1"))),
        "the child never announced a slot"
    );
    let locks = FsLocks::new(&root);
    assert!(
        acquire_first(&locks, 1).unwrap().is_none(),
        "held while the child lives"
    );
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    assert!(
        wait_free(&locks, id(1), LONG, POLL).unwrap(),
        "the kernel frees a dead holder's lock"
    );
    assert!(owner.exists());
}
