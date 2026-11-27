//! The "explorer orphan" shape (ADR 0033): on Windows a process keeps the pid of a parent that has exited, and that pid can
//! later be handed to an unrelated new process (here: a browser helper). The orphan then looks like the helper's child and,
//! through it, like part of the browser's tree. It is older than the helper, so it cannot be its child.
//!
//! The tables below are injected (no real process is touched). Each row has a start time: a process counts as a child only
//! if it started no earlier than its parent. These tests failed (`should_panic`) on the walks that followed parent pids
//! without that check, before ADR 0033. Not covered here: the Spotter watchdog's kill (`launch.rs`), which follows.

use paddock_proc::{
    ProcEntry, ProcessId, ProcessSource, StartTime, Table, descendants, table_from,
    tree_memory_from, tree_memory_of, tree_memory_with,
};

const CHROME: u32 = 100;
const HELPER: u32 = 200;
/// Started at logon, long before the helper; its parent (userinit.exe) exited, and the helper later got that pid.
const STRANGER: u32 = 300;
const STRANGER_CHILD: u32 = 301;
const MB: u64 = 1 << 20;

/// When each process started (µs): the stranger and its child long before Chrome and the helper.
fn started(pid: u32) -> StartTime {
    StartTime::from_micros(match pid {
        STRANGER => 50,
        STRANGER_CHILD => 60,
        p => u64::from(p),
    })
}

fn id(pid: u32) -> ProcessId {
    ProcessId::new(pid, started(pid))
}

fn entry(pid: u32, parent: Option<u32>, rss: u64) -> ProcEntry {
    ProcEntry {
        pid,
        parent,
        rss,
        started: Some(started(pid)),
        is_thread: false,
    }
}

/// Chrome -> helper, and the stranger (with a child of its own) naming the helper's pid as its parent.
fn machine() -> Table {
    table_from([
        entry(1, None, MB),
        entry(CHROME, Some(1), 10 * MB),
        entry(HELPER, Some(CHROME), 20 * MB),
        entry(STRANGER, Some(HELPER), 500 * MB),
        entry(STRANGER_CHILD, Some(STRANGER), 500 * MB),
    ])
}

/// Walks like the Windows scan fallback: children come from parent links only.
struct ByParent(Table);

impl ProcessSource for ByParent {
    fn memory(&self, pid: u32) -> Option<u64> {
        self.0.get(&pid).map(|e| e.rss)
    }
    fn children(&self, pid: u32) -> Vec<u32> {
        self.0
            .iter()
            .filter(|(k, row)| **k != pid && row.parent == Some(pid))
            .map(|(k, _)| *k)
            .collect()
    }
    fn started(&self, pid: u32) -> Option<StartTime> {
        self.0.get(&pid).and_then(|e| e.started)
    }
    fn table(&self) -> Table {
        self.0.clone()
    }
}

/// Control (passes): a stranger whose parent pid names nothing in the tree is not Chrome's.
#[test]
fn a_process_with_an_unrelated_parent_is_not_a_descendant() {
    let t = table_from([
        entry(CHROME, Some(1), MB),
        entry(HELPER, Some(CHROME), MB),
        entry(STRANGER, Some(777), MB),
    ]);
    assert_eq!(descendants(&t, id(CHROME)), vec![id(HELPER)]);
}

/// Control (passes): the job-member path counts exactly the listed pids, so a stranger that never joined the job is not
/// counted whatever its parent pid says.
#[test]
fn the_job_member_list_never_counts_a_stranger() {
    let src = ByParent(machine());
    assert_eq!(tree_memory_of(&src, &[CHROME, HELPER]), 30 * MB);
}

/// Table walk (`kill_tree`, `descendants_of`, the `snapshot()` / sysinfo fallbacks all end here).
#[test]
fn descendants_skip_a_stranger_older_than_its_claimed_parent() {
    let mut kids = descendants(&machine(), id(CHROME));
    kids.sort_unstable();
    assert_eq!(
        kids,
        vec![id(HELPER)],
        "orphan {STRANGER} (and its child) counted as Chrome's descendants"
    );
}

/// Memory-cap path over a table (`tree_memory_with`).
#[test]
fn table_memory_does_not_count_a_stranger() {
    let total = tree_memory_with(&machine(), CHROME, |_| None);
    assert_eq!(total, 30 * MB, "orphan memory counted in Chrome's tree");
}

/// Memory-cap path of the per-pid walk (`tree_memory_from`, used by the watcher where the OS has no job and no children
/// call: the Windows scan fallback).
#[test]
fn per_pid_walk_does_not_count_a_stranger() {
    let total = tree_memory_from(&ByParent(machine()), id(CHROME));
    assert_eq!(total, 30 * MB, "orphan memory counted in Chrome's tree");
}

/// The watcher's stop decision: a stranger's memory must never push Chrome over its cap (and so get killed with it).
#[test]
fn a_stranger_cannot_push_the_browser_over_its_cap() {
    let cap = 100 * MB;
    let measured = tree_memory_from(&ByParent(machine()), id(CHROME));
    assert!(
        measured <= cap,
        "orphan memory (1000 MB) pushed a 30 MB browser over a 100 MB cap"
    );
}
