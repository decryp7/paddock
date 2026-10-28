//! The activity log: every action Dyno took (by a rule or a click), with what is needed to undo it.

use crate::process::{Priority, ProcessAction, ProcessRef};
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

/// What kind of activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivityKind {
    /// A rule acted.
    Rule,
    /// A rule's actions were released.
    Release,
    /// The user acted on a process.
    Manual,
    /// An alert fired.
    Alert,
    /// Dyno itself (started, profile switched, rules paused).
    System,
}

/// Whether an entry can be undone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UndoState {
    /// Nothing to undo.
    NotUndoable,
    /// Can be undone.
    Available,
    /// Was undone.
    Undone,
    /// No longer possible (the process exited).
    Expired,
}

/// One entry as the UI shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    /// Id.
    pub id: String,
    /// When.
    pub at: Timestamp,
    /// Kind.
    pub kind: ActivityKind,
    /// One line, e.g. "Lowered Chrome's priority".
    pub title: String,
    /// Why or what exactly.
    pub detail: String,
    /// Undo state.
    pub undo: UndoState,
    /// Bytes freed, for cleanups.
    pub bytes: Option<u64>,
}

/// How to undo an entry (kept by the agent next to it, never sent to the UI).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UndoRecord {
    /// Nothing.
    None,
    /// Carry out these actions on the process (inverse order already applied); `rule` snoozes that rule for the app.
    Process {
        /// The process.
        target: ProcessRef,
        /// The undoing actions.
        actions: Vec<ProcessAction>,
        /// (rule id, app group id) when a rule did it.
        rule: Option<(String, String)>,
    },
}

/// A process's settings before an action, so it can be put back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Previous {
    /// Priority class.
    pub priority: Option<Priority>,
    /// Affinity mask.
    pub affinity: Option<u64>,
    /// Efficiency mode.
    pub efficiency: Option<bool>,
}

/// The action that undoes `action`, given the settings before it; `None` when it can't be undone.
pub fn inverse(action: &ProcessAction, previous: Previous) -> Option<ProcessAction> {
    match action {
        ProcessAction::End | ProcessAction::Resume | ProcessAction::SetCpuCap(None) => None,
        ProcessAction::Suspend => Some(ProcessAction::Resume),
        ProcessAction::SetPriority(_) => Some(ProcessAction::SetPriority(previous.priority.unwrap_or(Priority::Normal))),
        ProcessAction::SetAffinity(_) => previous.affinity.map(ProcessAction::SetAffinity),
        ProcessAction::SetEfficiencyMode(on) => Some(ProcessAction::SetEfficiencyMode(previous.efficiency.unwrap_or(!on))),
        ProcessAction::SetCpuCap(Some(_)) => Some(ProcessAction::SetCpuCap(None)),
    }
}

/// The undo state of a record now: process undo needs the process alive.
pub fn undo_state(record: &UndoRecord, undone: bool, _now: Timestamp, alive: impl Fn(ProcessRef) -> bool) -> UndoState {
    if undone {
        return UndoState::Undone;
    }
    match record {
        UndoRecord::None => UndoState::NotUndoable,
        UndoRecord::Process { actions, .. } if actions.is_empty() => UndoState::NotUndoable,
        UndoRecord::Process { target, .. } if !alive(*target) => UndoState::Expired,
        _ => UndoState::Available,
    }
}

/// The most entries the agent keeps.
pub const MAX_ENTRIES: usize = 5_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverses() {
        let prev = Previous {
            priority: Some(Priority::High),
            affinity: Some(0b11),
            efficiency: Some(false),
        };
        assert_eq!(inverse(&ProcessAction::End, prev), None);
        assert_eq!(inverse(&ProcessAction::Resume, prev), None);
        assert_eq!(inverse(&ProcessAction::SetCpuCap(None), prev), None);
        assert_eq!(inverse(&ProcessAction::Suspend, prev), Some(ProcessAction::Resume));
        assert_eq!(
            inverse(&ProcessAction::SetPriority(Priority::Idle), prev),
            Some(ProcessAction::SetPriority(Priority::High))
        );
        assert_eq!(
            inverse(&ProcessAction::SetPriority(Priority::Idle), Previous::default()),
            Some(ProcessAction::SetPriority(Priority::Normal))
        );
        assert_eq!(
            inverse(&ProcessAction::SetAffinity(1), prev),
            Some(ProcessAction::SetAffinity(0b11))
        );
        assert_eq!(inverse(&ProcessAction::SetAffinity(1), Previous::default()), None);
        assert_eq!(
            inverse(&ProcessAction::SetEfficiencyMode(true), prev),
            Some(ProcessAction::SetEfficiencyMode(false))
        );
        assert_eq!(
            inverse(&ProcessAction::SetEfficiencyMode(true), Previous::default()),
            Some(ProcessAction::SetEfficiencyMode(false))
        );
        assert_eq!(
            inverse(&ProcessAction::SetCpuCap(Some(5)), prev),
            Some(ProcessAction::SetCpuCap(None))
        );
    }

    #[test]
    fn undo_states() {
        let target = ProcessRef {
            pid: 1,
            started: Timestamp(0),
        };
        let p = UndoRecord::Process {
            target,
            actions: vec![ProcessAction::Resume],
            rule: None,
        };
        let now = Timestamp(10);
        assert_eq!(undo_state(&p, true, now, |_| true), UndoState::Undone);
        assert_eq!(undo_state(&p, false, now, |_| true), UndoState::Available);
        assert_eq!(undo_state(&p, false, now, |_| false), UndoState::Expired);
        let empty = UndoRecord::Process {
            target,
            actions: vec![],
            rule: None,
        };
        assert_eq!(undo_state(&empty, false, now, |_| true), UndoState::NotUndoable);
        assert_eq!(undo_state(&UndoRecord::None, false, now, |_| true), UndoState::NotUndoable);
        const { assert!(MAX_ENTRIES > 100) };
    }
}
