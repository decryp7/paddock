use paddock_slots::{
    Announcement, FsLocks, HoldEnd, OwnerInfo, ReleaseLocks, SlotError, SlotId, SlotLocks,
    acquire_first, acquire_retry, fresh_token, hold, release, slot_states, wait_free,
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
    let _gate = crate::share_gate();
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
    let _gate = crate::share_gate();
    let dir = tempfile::tempdir().unwrap();
    let locks = FsLocks::new(dir.path());
    std::fs::create_dir_all(dir.path().join("slots")).unwrap();
    assert!(locks.read_owner(id(1)).is_none());
    let info = OwnerInfo {
        owner: PathBuf::from("job folder/x"),
        pid: 42,
        token: "tok".into(),
    };
    locks.write_owner(id(1), &info).unwrap();
    assert_eq!(locks.read_owner(id(1)), Some(info));
    std::fs::write(dir.path().join("slots/s2.owner"), "pid=x\nowner=y\n").unwrap();
    assert!(locks.read_owner(id(2)).is_none());
    locks.clear_owner(id(1));
    assert!(locks.read_owner(id(1)).is_none());
    assert!(!locks.release_requested(id(1), "tok"));
    locks.request_release(id(1), "tok").unwrap();
    assert!(locks.release_requested(id(1), "tok"));
    assert!(
        !locks.release_requested(id(1), "other"),
        "only the holder's token counts"
    );
    locks.withdraw_release(id(1), "other");
    assert!(
        locks.release_requested(id(1), "tok"),
        "a request is withdrawn only by its writer"
    );
    locks.withdraw_release(id(1), "tok");
    assert!(!locks.release_requested(id(1), "tok"));
    locks.request_release(id(1), "tok").unwrap();
    locks.clear_release(id(1));
    assert!(!locks.release_requested(id(1), "tok"));
}

#[test]
fn an_unusable_root_is_a_typed_error() {
    let _gate = crate::share_gate();
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
                    pid: 1,
                    token: String::new()
                }
            )
            .is_err()
    );
    assert!(locks.request_release(id(1), "t").is_err());
    assert!(std::error::Error::source(&SlotError::Invalid("x".into())).is_none());
    assert!(
        SlotError::ReleaseTimeout("s1".into())
            .to_string()
            .contains("s1")
    );
}

#[test]
fn hold_announces_and_ends_when_the_owner_folder_goes() {
    let _gate = crate::share_gate();
    let dir = tempfile::tempdir().unwrap();
    let owner = dir.path().join("job");
    std::fs::create_dir(&owner).unwrap();
    let locks = FsLocks::new(dir.path().join("ci"));
    let holder = {
        let (locks, owner) = (locks.clone(), owner.clone());
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let mut writer = ChannelWriter(tx);
        let t = std::thread::spawn(move || hold(&locks, 1, &owner, POLL, None, &mut writer));
        (t, rx)
    };
    // Wait on the announcement, not on time.
    let said = holder.1.recv_timeout(LONG).unwrap();
    let Ok(Announcement::Slot(first, token)) =
        Announcement::parse(&String::from_utf8(said).unwrap())
    else {
        panic!("the holder announces its slot and token");
    };
    assert_eq!(first, id(1));
    assert_eq!(locks.read_owner(id(1)).map(|o| o.token), Some(token));
    assert!(locks.read_owner(id(1)).is_some_and(|o| o.owner == owner));
    assert!(acquire_first(&locks, 1).unwrap().is_none());
    // A second holder with every slot busy reports `none`.
    let mut out = Vec::new();
    assert!(
        hold(&locks, 1, &owner, POLL, None, &mut out)
            .unwrap()
            .is_none()
    );
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
    let _gate = crate::share_gate();
    let dir = tempfile::tempdir().unwrap();
    let owner = dir.path().join("job");
    std::fs::create_dir(&owner).unwrap();
    let locks = FsLocks::new(dir.path().join("ci"));
    let (tx, rx) = std::sync::mpsc::channel();
    let t = {
        let (locks, owner) = (locks.clone(), owner.clone());
        std::thread::spawn(move || hold(&locks, 1, &owner, POLL, None, &mut ChannelWriter(tx)))
    };
    rx.recv_timeout(LONG).unwrap();
    let token = locks.read_owner(id(1)).unwrap().token;
    release(&locks, id(1), &token, LONG, POLL).unwrap();
    assert_eq!(t.join().unwrap().unwrap(), Some((id(1), HoldEnd::Released)));
    assert!(owner.exists(), "the job's folder is not touched");
    // Releasing a free slot is fine.
    release(&locks, id(1), &token, LONG, POLL).unwrap();
}

#[test]
fn release_of_a_holder_that_ignores_the_request_times_out() {
    let _gate = crate::share_gate();
    let dir = tempfile::tempdir().unwrap();
    let locks = FsLocks::new(dir.path());
    let (_, _guard) = acquire_first(&locks, 1).unwrap().unwrap();
    locks
        .write_owner(
            id(1),
            &OwnerInfo {
                owner: PathBuf::from("job"),
                pid: 1,
                token: "mine".into(),
            },
        )
        .unwrap();
    assert!(!wait_free(&locks, id(1), Duration::ZERO, POLL).unwrap());
    // Waiting for a holder that never lets go gives up at the deadline.
    assert!(!wait_free(&locks, id(1), Duration::from_millis(15), POLL).unwrap());
    let err = release(&locks, id(1), "mine", Duration::from_millis(20), POLL).unwrap_err();
    assert!(matches!(err, SlotError::ReleaseTimeout(_)));
}

type HolderThread = std::thread::JoinHandle<Result<Option<(SlotId, HoldEnd)>, SlotError>>;

/// Starts a holder on a thread and returns its handle, its slot and its token once it has announced itself.
fn start_holder(locks: &FsLocks, owner: &std::path::Path) -> (HolderThread, SlotId, String) {
    let (tx, rx) = std::sync::mpsc::channel();
    let (locks, owner) = (locks.clone(), owner.to_path_buf());
    let t = std::thread::spawn(move || hold(&locks, 1, &owner, POLL, None, &mut ChannelWriter(tx)));
    let said = String::from_utf8(rx.recv_timeout(LONG).unwrap()).unwrap();
    let Ok(Announcement::Slot(slot, token)) = Announcement::parse(&said) else {
        panic!("{said:?}")
    };
    (t, slot, token)
}

#[test]
fn a_late_release_never_frees_the_next_jobs_slot() {
    let _gate = crate::share_gate();
    let dir = tempfile::tempdir().unwrap();
    let locks = FsLocks::new(dir.path().join("ci"));
    let (job_a, job_b) = (dir.path().join("a"), dir.path().join("b"));
    std::fs::create_dir(&job_a).unwrap();
    std::fs::create_dir(&job_b).unwrap();
    // A holds s1 and its job ends; B takes the slot; then A's step calls release with A's token.
    let (a, slot, token_a) = start_holder(&locks, &job_a);
    std::fs::remove_dir(&job_a).unwrap();
    assert_eq!(a.join().unwrap().unwrap(), Some((slot, HoldEnd::OwnerGone)));
    let (b, _, token_b) = start_holder(&locks, &job_b);
    assert_ne!(token_a, token_b);
    release(&locks, slot, &token_a, Duration::from_millis(50), POLL).unwrap();
    assert!(
        acquire_first(&locks, 1).unwrap().is_none(),
        "B still holds the slot"
    );
    assert!(
        !locks.release_requested(slot, &token_b),
        "and nothing was asked of B"
    );
    // B's own token does release it.
    release(&locks, slot, &token_b, LONG, POLL).unwrap();
    assert_eq!(b.join().unwrap().unwrap(), Some((slot, HoldEnd::Released)));
}

#[test]
fn a_holder_ignores_a_release_request_with_another_token() {
    let _gate = crate::share_gate();
    let dir = tempfile::tempdir().unwrap();
    let locks = FsLocks::new(dir.path().join("ci"));
    let job = dir.path().join("job");
    std::fs::create_dir(&job).unwrap();
    let (t, slot, token) = start_holder(&locks, &job);
    locks.request_release(slot, "someone-else").unwrap();
    // Not honoured: removing the job folder is what ends it, not the foreign request.
    std::fs::remove_dir(&job).unwrap();
    assert_eq!(t.join().unwrap().unwrap(), Some((slot, HoldEnd::OwnerGone)));
    assert!(wait_free(&locks, slot, LONG, POLL).unwrap());
    assert_ne!(token, "someone-else");
}

#[test]
fn a_probe_holding_a_lock_for_an_instant_does_not_make_acquire_refuse() {
    let _gate = crate::share_gate();
    let dir = tempfile::tempdir().unwrap();
    let locks = FsLocks::new(dir.path());
    // A status probe has slot s1 locked right now...
    let mut probe = Some(locks.try_lock(id(1)).unwrap().unwrap());
    // ...and lets go while the acquirer pauses between its tries.
    let (slot, _guard) = acquire_retry(&locks, 1, 3, || drop(probe.take()))
        .unwrap()
        .unwrap();
    assert_eq!(slot, id(1));
    // A real holder keeps the slot through every try.
    assert!(acquire_retry(&locks, 1, 3, || {}).unwrap().is_none());
}

#[test]
fn announcements_round_trip_and_garbage_is_refused() {
    let a = Announcement::Slot(id(7), fresh_token());
    assert_eq!(Announcement::parse(&a.line()).unwrap(), a);
    assert_eq!(
        Announcement::parse(&Announcement::None.line()).unwrap(),
        Announcement::None
    );
    for bad in ["", "s1", "s1 ", "s1 a b", "x1 tok", "hello world"] {
        assert!(Announcement::parse(bad).is_err(), "{bad:?}");
    }
    assert_ne!(fresh_token(), fresh_token());
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
        // A child left behind by a failed test ends by itself (tests leave no process on the machine).
        Some(Duration::from_secs(60)),
        &mut std::io::stdout(),
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
    let _gate = crate::exclusive_gate();
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
        lines.any(|l| l.is_ok_and(|l| l
            .rsplit("... ")
            .next()
            .is_some_and(|t| matches!(Announcement::parse(t), Ok(Announcement::Slot(..)))))),
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

#[test]
fn a_release_whose_holder_ended_and_whose_slot_was_taken_again_meanwhile_withdraws_its_request() {
    let _gate = crate::share_gate();
    let dir = tempfile::tempdir().unwrap();
    let locks = FsLocks::new(dir.path());
    let (_, _guard) = acquire_first(&locks, 1).unwrap().unwrap();
    let info = |token: &str| OwnerInfo {
        owner: PathBuf::from("job"),
        pid: 1,
        token: token.into(),
    };
    locks.write_owner(id(1), &info("mine")).unwrap();
    let swapped = std::thread::scope(|s| {
        // The holder "ends" and a new one records itself, as soon as the request has been made.
        let swapper = s.spawn(|| {
            while !locks.release_requested(id(1), "mine") {
                std::thread::yield_now();
            }
            locks.write_owner(id(1), &info("next")).unwrap();
        });
        let r = release(&locks, id(1), "mine", LONG, POLL);
        swapper.join().unwrap();
        r
    });
    swapped.unwrap();
    assert!(
        !locks.release_requested(id(1), "mine"),
        "the stale request is withdrawn"
    );
    assert_eq!(
        locks.read_owner(id(1)).map(|o| o.token),
        Some("next".to_string())
    );
}

#[test]
fn a_holder_that_cannot_record_itself_reports_it_and_frees_the_slot() {
    let _gate = crate::share_gate();
    let dir = tempfile::tempdir().unwrap();
    let locks = FsLocks::new(dir.path());
    std::fs::create_dir_all(dir.path().join("slots/s1.owner")).unwrap();
    let mut said = Vec::new();
    let err = hold(&locks, 1, dir.path(), POLL, None, &mut said).unwrap_err();
    assert!(matches!(err, SlotError::Io { .. }));
    assert!(said.is_empty(), "nothing was announced");
    assert!(
        acquire_first(&locks, 1).unwrap().is_some(),
        "the slot is free again"
    );
}

#[test]
fn owner_records_missing_a_field_are_ignored() {
    let _gate = crate::share_gate();
    let dir = tempfile::tempdir().unwrap();
    let locks = FsLocks::new(dir.path());
    std::fs::create_dir_all(dir.path().join("slots")).unwrap();
    for (n, text) in [
        (1, "pid=1\nowner=o\n"),
        (2, "pid=1\ntoken=t\n"),
        (3, "token=t\nowner=o\n"),
    ] {
        std::fs::write(dir.path().join(format!("slots/s{n}.owner")), text).unwrap();
        assert!(locks.read_owner(id(n)).is_none(), "s{n}");
    }
}

#[test]
fn a_lock_file_that_cannot_be_opened_is_a_typed_error() {
    let _gate = crate::share_gate();
    let dir = tempfile::tempdir().unwrap();
    let locks = FsLocks::new(dir.path());
    std::fs::create_dir_all(dir.path().join("slots/s1.lock")).unwrap();
    assert!(matches!(locks.try_lock(id(1)), Err(SlotError::Io { .. })));
    assert!(wait_free(&locks, id(1), Duration::ZERO, POLL).is_err());
    assert!(release(&locks, id(1), "t", Duration::ZERO, POLL).is_err());
    let mut said = Vec::new();
    assert!(hold(&locks, 1, dir.path(), POLL, None, &mut said).is_err());
    assert!(said.is_empty());
}

#[test]
fn a_release_request_that_cannot_be_written_is_a_typed_error() {
    let _gate = crate::share_gate();
    let dir = tempfile::tempdir().unwrap();
    let locks = FsLocks::new(dir.path());
    let (_, _guard) = acquire_first(&locks, 1).unwrap().unwrap();
    let info = OwnerInfo {
        owner: PathBuf::from("job"),
        pid: 1,
        token: "mine".into(),
    };
    locks.write_owner(id(1), &info).unwrap();
    std::fs::create_dir(dir.path().join("slots/s1.release")).unwrap();
    assert!(matches!(
        release(&locks, id(1), "mine", LONG, POLL),
        Err(SlotError::Io { .. })
    ));
}

/// A store whose slot is held until a release is requested, after which looking at the lock fails.
struct FailingAfterRequest {
    requested: std::cell::Cell<bool>,
}

impl SlotLocks for FailingAfterRequest {
    type Guard = ();

    fn try_lock(&self, _id: SlotId) -> Result<Option<()>, SlotError> {
        if self.requested.get() {
            Err(SlotError::Io {
                path: PathBuf::from("s1.lock"),
                source: std::io::Error::other("disk gone"),
            })
        } else {
            Ok(None)
        }
    }
}

impl ReleaseLocks for FailingAfterRequest {
    fn read_owner(&self, _id: SlotId) -> Option<OwnerInfo> {
        Some(OwnerInfo {
            owner: PathBuf::from("job"),
            pid: 1,
            token: "mine".into(),
        })
    }
    fn request_release(&self, _id: SlotId, _token: &str) -> Result<(), SlotError> {
        self.requested.set(true);
        Ok(())
    }
    fn withdraw_release(&self, _id: SlotId, _token: &str) {}
}

#[test]
fn a_lock_that_becomes_unusable_while_waiting_for_a_release_is_a_typed_error() {
    let locks = FailingAfterRequest {
        requested: std::cell::Cell::new(false),
    };
    assert!(matches!(
        release(&locks, id(1), "mine", LONG, POLL),
        Err(SlotError::Io { .. })
    ));
}

#[test]
fn a_holder_with_a_lifetime_cap_lets_go_when_it_is_reached() {
    let _gate = crate::share_gate();
    let dir = tempfile::tempdir().unwrap();
    let owner = dir.path().join("job");
    std::fs::create_dir(&owner).unwrap();
    let locks = FsLocks::new(dir.path().join("ci"));
    let mut said = Vec::new();
    let end = hold(
        &locks,
        1,
        &owner,
        POLL,
        Some(Duration::from_millis(30)),
        &mut said,
    )
    .unwrap();
    assert_eq!(end, Some((id(1), HoldEnd::Expired)));
    assert!(
        acquire_first(&locks, 1).unwrap().is_some(),
        "the slot is free again"
    );
}
