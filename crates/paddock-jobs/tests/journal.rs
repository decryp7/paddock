//! The activity journal (plan X44): recording, bounds, undo states, persistence and its failure modes, all on a `FakeClock`
//! and temporary folders.

use paddock_env::Problem;
use paddock_jobs::FakeClock;
use paddock_jobs::journal::{
    DEFAULT_CAPACITY, Entry, Journal, JournalError, MAX_CAPACITY, MAX_DETAIL_CHARS, MAX_KIND_CHARS,
    MAX_TITLE_CHARS, NewEntry, UndoState,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const T0: i64 = 1_792_656_000_000; // 2026-10-22T08:00:00Z

fn clock() -> Arc<FakeClock> {
    Arc::new(FakeClock::at_wall_ms(T0))
}

fn mem(capacity: usize) -> (Journal, Arc<FakeClock>) {
    let c = clock();
    (Journal::in_memory(capacity, c.clone()), c)
}

fn rule(title: &str) -> NewEntry {
    NewEntry::new("rule", title)
}

#[test]
fn an_entry_gets_an_id_the_clock_time_and_its_undo_state() {
    let (j, clock) = mem(10);
    let a = j.record(
        rule("Lowered Chrome's priority")
            .detail("Rule: background apps")
            .undoable()
            .bytes(42),
    );
    assert_eq!(
        a,
        Entry {
            id: 1,
            at_ms: T0,
            kind: "rule".into(),
            title: "Lowered Chrome's priority".into(),
            detail: "Rule: background apps".into(),
            undo: UndoState::Available,
            bytes: Some(42),
        }
    );
    clock.advance(Duration::from_secs(60));
    let b = j.record(NewEntry::new("system", "Started"));
    assert_eq!(
        (b.id, b.at_ms, b.undo, b.bytes),
        (2, T0 + 60_000, UndoState::NotUndoable, None)
    );
    assert_eq!(j.get(1), Some(a.clone()));
    assert_eq!(j.get(9), None);
    assert_eq!(
        j.entries(10).iter().map(|e| e.id).collect::<Vec<_>>(),
        [2, 1],
        "newest first"
    );
    assert_eq!(j.entries(1).len(), 1);
    assert!(j.entries(0).is_empty());
}

#[test]
fn the_oldest_entries_are_dropped_beyond_the_capacity_and_ids_are_never_reused() {
    let (j, _) = mem(3);
    for n in 1..=5 {
        j.record(rule(&format!("action {n}")));
    }
    assert_eq!(
        j.entries(10).iter().map(|e| e.id).collect::<Vec<_>>(),
        [5, 4, 3]
    );
    assert_eq!(
        j.mark(1, UndoState::Undone),
        Err(JournalError::NotFound(1)),
        "a dropped entry is gone"
    );
    j.clear();
    assert!(j.entries(10).is_empty());
    assert_eq!(
        j.record(rule("after clear")).id,
        6,
        "ids keep counting after a clear"
    );
    // The capacity is clamped to something sensible.
    assert_eq!(Journal::in_memory(0, clock()).record(rule("a")).id, 1);
    let many = Journal::in_memory(usize::MAX, clock());
    assert!(DEFAULT_CAPACITY <= MAX_CAPACITY);
    assert_eq!(many.entries(1).len(), 0);
}

#[test]
fn long_text_is_cut_at_a_character_boundary() {
    let (j, _) = mem(10);
    let e = j.record(
        NewEntry::new("k".repeat(100), "é".repeat(500)).detail("日".repeat(MAX_DETAIL_CHARS + 50)),
    );
    assert_eq!(e.kind.chars().count(), MAX_KIND_CHARS);
    assert_eq!(e.title.chars().count(), MAX_TITLE_CHARS);
    assert!(e.title.ends_with('…') && e.detail.ends_with('…'));
    assert_eq!(e.detail.chars().count(), MAX_DETAIL_CHARS);
    // Text at the limit is kept whole.
    let exact = j.record(NewEntry::new("k", "x".repeat(MAX_TITLE_CHARS)));
    assert_eq!(exact.title.chars().count(), MAX_TITLE_CHARS);
    assert!(!exact.title.ends_with('…'));
}

#[test]
fn only_an_available_entry_can_be_undone_or_expired_and_only_once() {
    let (j, _) = mem(10);
    let id = j.record(rule("a").undoable()).id;
    let plain = j.record(rule("b")).id;
    let done = j.mark(id, UndoState::Undone).unwrap();
    assert_eq!(done.undo, UndoState::Undone);
    assert_eq!(j.get(id).unwrap().undo, UndoState::Undone);
    assert_eq!(
        j.mark(id, UndoState::Undone),
        Err(JournalError::NotAvailable {
            id,
            state: UndoState::Undone
        })
    );
    assert_eq!(
        j.mark(plain, UndoState::Expired),
        Err(JournalError::NotAvailable {
            id: plain,
            state: UndoState::NotUndoable
        })
    );
    assert_eq!(
        j.mark(77, UndoState::Undone),
        Err(JournalError::NotFound(77))
    );
    for bad in [UndoState::Available, UndoState::NotUndoable] {
        assert_eq!(j.mark(id, bad), Err(JournalError::BadTarget(bad)));
    }
    let messages: Vec<String> = [
        JournalError::NotFound(3),
        JournalError::NotAvailable {
            id: 4,
            state: UndoState::Expired,
        },
        JournalError::BadTarget(UndoState::Available),
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    assert_eq!(
        messages,
        [
            "activity entry 3 is no longer in the log",
            "activity entry 4 cannot be changed (Expired)",
            "an entry cannot be moved to Available"
        ]
    );
}

#[test]
fn a_quarantine_that_runs_out_expires_its_entries_by_the_clock() {
    let (j, clock) = mem(10);
    let old = j
        .record(NewEntry::new("cleanup", "Moved 3 files").undoable())
        .id;
    let plain = j.record(NewEntry::new("cleanup", "Looked")).id;
    clock.advance(Duration::from_secs(7 * 86_400));
    let fresh = j
        .record(NewEntry::new("cleanup", "Moved 1 file").undoable())
        .id;
    let cutoff = UnixNow::of(&clock) - 3 * 86_400_000;
    let expired = j.expire_before(cutoff);
    assert_eq!(expired.iter().map(|e| e.id).collect::<Vec<_>>(), [old]);
    assert_eq!(j.get(old).unwrap().undo, UndoState::Expired);
    assert_eq!(j.get(plain).unwrap().undo, UndoState::NotUndoable);
    assert_eq!(j.get(fresh).unwrap().undo, UndoState::Available);
    assert!(j.expire_before(cutoff).is_empty(), "nothing left to expire");
}

struct UnixNow;
impl UnixNow {
    fn of(clock: &FakeClock) -> i64 {
        use paddock_jobs::Clock;
        i64::try_from(
            clock
                .wall()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap()
    }
}

#[test]
fn listeners_hear_every_change_and_a_panicking_one_breaks_nothing() {
    let (j, _) = mem(10);
    let heard = Arc::new(Mutex::new(Vec::<(u64, UndoState)>::new()));
    let h = heard.clone();
    j.subscribe(Arc::new(|_| panic!("a bad listener")));
    j.subscribe(Arc::new(move |e| h.lock().unwrap().push((e.id, e.undo))));
    let id = j.record(rule("a").undoable()).id;
    j.mark(id, UndoState::Undone).unwrap();
    let id2 = j.record(NewEntry::new("cleanup", "b").undoable()).id;
    j.expire_before(i64::MAX);
    assert_eq!(
        *heard.lock().unwrap(),
        [
            (id, UndoState::Available),
            (id, UndoState::Undone),
            (id2, UndoState::Available),
            (id2, UndoState::Expired)
        ]
    );
}

#[test]
fn concurrent_recording_gives_unique_increasing_ids() {
    let (j, _) = mem(1000);
    let j = Arc::new(j);
    let threads: Vec<_> = (0..8)
        .map(|t| {
            let j = j.clone();
            std::thread::spawn(move || {
                (0..50)
                    .map(|n| j.record(rule(&format!("t{t} n{n}"))).id)
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let mut ids: Vec<u64> = threads
        .into_iter()
        .flat_map(|t| t.join().unwrap())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, (1..=400).collect::<Vec<_>>());
    assert_eq!(j.entries(1000).len(), 400);
}

#[test]
fn entries_persist_across_a_restart_and_the_file_is_trimmed_to_the_capacity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("activity.json");
    let c = clock();
    let (j, problem) = Journal::open(&path, 100, c.clone());
    assert!(problem.is_none(), "a missing file is not a problem");
    let a = j.record(rule("first").undoable()).id;
    j.record(rule("second"));
    j.mark(a, UndoState::Undone).unwrap();
    assert_eq!(j.save_error(), None);
    drop(j);

    let (again, problem) = Journal::open(&path, 100, c.clone());
    assert!(problem.is_none());
    assert_eq!(
        again
            .entries(10)
            .iter()
            .map(|e| (e.id, e.title.as_str(), e.undo))
            .collect::<Vec<_>>(),
        [
            (2, "second", UndoState::NotUndoable),
            (1, "first", UndoState::Undone)
        ]
    );
    assert_eq!(again.record(rule("third")).id, 3, "ids continue");
    drop(again);

    let (small, _) = Journal::open(&path, 2, c);
    assert_eq!(
        small.entries(10).iter().map(|e| e.id).collect::<Vec<_>>(),
        [3, 2],
        "the oldest is dropped on open"
    );
    // The file is readable by the user only.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o077,
            0
        );
    }
}

#[test]
fn ids_are_not_reused_even_when_the_counter_in_the_file_is_wrong() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("activity.json");
    std::fs::write(
        &path,
        r#"{"version":1,"nextId":2,"entries":[{"id":9,"atMs":5,"kind":"rule","title":"x","detail":"","undo":"available","bytes":null}]}"#,
    )
    .unwrap();
    let (j, problem) = Journal::open(&path, 10, clock());
    assert!(problem.is_none());
    assert_eq!(j.record(rule("y")).id, 10);
}

#[test]
fn a_damaged_file_starts_empty_keeps_the_damaged_copy_and_the_log_keeps_working() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("activity.json");
    std::fs::write(&path, b"{ not json").unwrap();
    let (j, problem) = Journal::open(&path, 10, clock());
    assert!(
        matches!(problem, Some(Problem::Damaged { .. })),
        "{problem:?}"
    );
    assert!(j.entries(10).is_empty());
    j.record(rule("works"));
    assert_eq!(j.save_error(), None);
    let kept = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .any(|f| f.file_name().to_string_lossy().contains("corrupt"));
    assert!(kept, "the damaged file is kept for inspection");
}

#[test]
fn a_file_from_a_newer_version_is_never_overwritten_and_the_save_error_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("activity.json");
    let newer = r#"{"version":99,"nextId":1,"entries":[]}"#;
    std::fs::write(&path, newer).unwrap();
    let (j, problem) = Journal::open(&path, 10, clock());
    assert!(matches!(problem, Some(Problem::Newer { found: 99, .. })));
    let e = j.record(rule("still recorded in memory"));
    assert_eq!(j.get(e.id), Some(e));
    assert!(
        j.save_error().unwrap().contains("newer"),
        "{:?}",
        j.save_error()
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), newer);
}

#[test]
fn a_save_that_fails_is_reported_and_clears_when_a_later_save_works() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("state");
    std::fs::create_dir(&folder).unwrap();
    let (j, problem) = Journal::open(folder.join("activity.json"), 10, clock());
    assert!(problem.is_none());
    // The folder is replaced by a regular file, so no save can create the journal file in it.
    std::fs::remove_dir(&folder).unwrap();
    std::fs::write(&folder, b"x").unwrap();
    let e = j.record(rule("not saved"));
    assert!(j.save_error().is_some());
    assert_eq!(j.get(e.id), Some(e), "the action is still logged in memory");
    // Fixing the cause lets the next save through, and the error goes away.
    std::fs::remove_file(&folder).unwrap();
    std::fs::create_dir(&folder).unwrap();
    j.record(rule("saved"));
    assert_eq!(j.save_error(), None);
    assert!(folder.join("activity.json").exists());
}

#[test]
fn entries_use_a_camel_case_wire_format() {
    let (j, _) = mem(10);
    let e = j.record(NewEntry::new("cleanup", "Moved").undoable().bytes(7));
    let json = serde_json::to_value(&e).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"id":1,"atMs":T0,"kind":"cleanup","title":"Moved","detail":"","undo":"available","bytes":7})
    );
    assert_eq!(serde_json::from_value::<Entry>(json).unwrap(), e);
    assert_eq!(
        serde_json::to_string(&UndoState::NotUndoable).unwrap(),
        "\"notUndoable\""
    );
}
