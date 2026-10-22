//! Snapshots of the whole process table and the diff between two ("what changed since this morning?").

use crate::process::{ProcessRef, ProcessSample};
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A stored snapshot's header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInfo {
    /// Id.
    pub id: String,
    /// When it was taken.
    pub at: Timestamp,
    /// The user's label.
    pub label: String,
    /// Processes in it.
    pub process_count: usize,
}

/// A full snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Header.
    pub info: SnapshotInfo,
    /// Memory in use, bytes.
    pub memory_used: u64,
    /// CPU, percent.
    pub cpu: f32,
    /// Every process.
    pub processes: Vec<ProcessSample>,
}

/// A process in a diff.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessSummary {
    /// Pid.
    pub pid: u32,
    /// Name.
    pub name: String,
    /// Executable.
    pub exe: Option<String>,
    /// CPU.
    pub cpu: f32,
    /// Memory.
    pub memory: u64,
}

/// A process in both snapshots whose use changed noticeably.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessChange {
    /// Pid.
    pub pid: u32,
    /// Name.
    pub name: String,
    /// CPU before.
    pub cpu_before: f32,
    /// CPU after.
    pub cpu_after: f32,
    /// Memory before.
    pub memory_before: u64,
    /// Memory after.
    pub memory_after: u64,
}

/// The diff.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotDiff {
    /// The earlier snapshot.
    pub before: SnapshotInfo,
    /// The later one.
    pub after: SnapshotInfo,
    /// Started in between.
    pub added: Vec<ProcessSummary>,
    /// Exited in between.
    pub removed: Vec<ProcessSummary>,
    /// Changed noticeably (memory by ≥ 10 MiB and 10 %, or CPU by ≥ 5 points), biggest memory change first.
    pub changed: Vec<ProcessChange>,
    /// Change of memory in use, bytes.
    pub memory_used_delta: i64,
    /// Change of CPU, points.
    pub cpu_delta: f32,
}

/// The most processes a snapshot keeps and a diff list returns.
pub const MAX_PROCESSES: usize = 5_000;

fn summary(p: &ProcessSample) -> ProcessSummary {
    ProcessSummary {
        pid: p.pid,
        name: p.name.clone(),
        exe: p.exe.clone(),
        cpu: p.cpu,
        memory: p.memory,
    }
}

/// Builds a snapshot (keeping at most [`MAX_PROCESSES`], the biggest by memory).
pub fn take(id: String, at: Timestamp, label: &str, memory_used: u64, cpu: f32, mut processes: Vec<ProcessSample>) -> Snapshot {
    processes.sort_by(|a, b| b.memory.cmp(&a.memory));
    processes.truncate(MAX_PROCESSES);
    let label: String = label.trim().chars().take(100).collect();
    Snapshot {
        info: SnapshotInfo {
            id,
            at,
            label: if label.is_empty() { "Snapshot".into() } else { label },
            process_count: processes.len(),
        },
        memory_used,
        cpu,
        processes,
    }
}

/// What changed from `a` to `b` (processes are matched by pid and start time).
pub fn diff(a: &Snapshot, b: &Snapshot) -> SnapshotDiff {
    let before: HashMap<ProcessRef, &ProcessSample> = a.processes.iter().map(|p| (p.reference(), p)).collect();
    let after: HashMap<ProcessRef, &ProcessSample> = b.processes.iter().map(|p| (p.reference(), p)).collect();
    let mut added: Vec<ProcessSummary> = b.processes.iter().filter(|p| !before.contains_key(&p.reference())).map(summary).collect();
    let mut removed: Vec<ProcessSummary> = a.processes.iter().filter(|p| !after.contains_key(&p.reference())).map(summary).collect();
    let mut changed: Vec<ProcessChange> = b
        .processes
        .iter()
        .filter_map(|p| {
            let old = before.get(&p.reference())?;
            let dm = p.memory.abs_diff(old.memory);
            let mem = dm >= 10 << 20 && dm as f64 >= old.memory as f64 * 0.1;
            let cpu = (p.cpu - old.cpu).abs() >= 5.0;
            (mem || cpu).then(|| ProcessChange {
                pid: p.pid,
                name: p.name.clone(),
                cpu_before: old.cpu,
                cpu_after: p.cpu,
                memory_before: old.memory,
                memory_after: p.memory,
            })
        })
        .collect();
    added.sort_by(|x, y| y.memory.cmp(&x.memory));
    removed.sort_by(|x, y| y.memory.cmp(&x.memory));
    changed.sort_by_key(|c| std::cmp::Reverse(c.memory_after.abs_diff(c.memory_before)));
    SnapshotDiff {
        before: a.info.clone(),
        after: b.info.clone(),
        added,
        removed,
        changed,
        memory_used_delta: b.memory_used as i64 - a.memory_used as i64,
        cpu_delta: b.cpu - a.cpu,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::fixtures::proc;

    #[test]
    fn diffs_show_added_removed_and_changed() {
        let a = take(
            "a".into(),
            Timestamp(0),
            " morning ",
            8 << 30,
            10.0,
            vec![proc(1, None, "keep.exe", 1.0, 100), proc(2, None, "gone.exe", 0.0, 50), proc(3, None, "grow.exe", 1.0, 100), proc(4, None, "busy.exe", 0.0, 10)],
        );
        let b = take(
            "b".into(),
            Timestamp(1),
            "",
            9 << 30,
            25.0,
            vec![proc(1, None, "keep.exe", 1.0, 105), proc(3, None, "grow.exe", 1.0, 400), proc(4, None, "busy.exe", 30.0, 10), proc(5, None, "new.exe", 0.0, 70)],
        );
        assert_eq!(a.info.label, "morning");
        assert_eq!(b.info.label, "Snapshot");
        let d = diff(&a, &b);
        assert_eq!(d.added.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["new.exe"]);
        assert_eq!(d.removed.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["gone.exe"]);
        assert_eq!(d.changed.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["grow.exe", "busy.exe"]);
        assert_eq!(d.memory_used_delta, 1 << 30);
        assert_eq!(d.cpu_delta, 15.0);
    }

    #[test]
    fn a_reused_pid_is_a_new_process_and_snapshots_are_bounded() {
        let a = take("a".into(), Timestamp(0), "x", 0, 0.0, vec![proc(1, None, "a.exe", 0.0, 1)]);
        let mut reused = proc(1, None, "b.exe", 0.0, 1);
        reused.started = Timestamp(999_999);
        let b = take("b".into(), Timestamp(1), "y", 0, 0.0, vec![reused]);
        let d = diff(&a, &b);
        assert_eq!((d.added.len(), d.removed.len()), (1, 1));
        let many: Vec<_> = (0..(MAX_PROCESSES as u32 + 10)).map(|i| proc(i, None, "p", 0.0, u64::from(i))).collect();
        let s = take("c".into(), Timestamp(0), "z", 0, 0.0, many);
        assert_eq!(s.info.process_count, MAX_PROCESSES);
        assert_eq!(s.processes[0].pid, MAX_PROCESSES as u32 + 9, "the biggest are kept");
    }
}
