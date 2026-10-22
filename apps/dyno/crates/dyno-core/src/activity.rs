//! The activity log: every action Dyno took (by a rule or a click), with what is needed to undo it.

use crate::process::{Priority, ProcessAction, ProcessRef};
use crate::startup::StartupState;
use crate::time::{Timestamp, DAY};
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
    /// A cleanup ran.
    Cleanup,
    /// A tuning change was applied or reverted.
    Tuning,
    /// A startup entry changed.
    Startup,
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
    /// No longer possible (the process exited, the quarantine expired).
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
    /// Put a startup entry back.
    Startup {
        /// Entry id.
        entry_id: String,
        /// Its state before.
        previous: StartupState,
    },
    /// Revert a tuning change.
    Tuning {
        /// Suggestion id.
        id: String,
    },
    /// Restore a quarantined cleanup.
    Quarantine {
        /// Quarantine id.
        id: String,
        /// Until when it can be restored.
        expires: Timestamp,
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

/// The undo state of a record now: process undo needs the process alive; quarantine undo expires.
pub fn undo_state(record: &UndoRecord, undone: bool, now: Timestamp, alive: impl Fn(ProcessRef) -> bool) -> UndoState {
    if undone {
        return UndoState::Undone;
    }
    match record {
        UndoRecord::None => UndoState::NotUndoable,
        UndoRecord::Process { actions, .. } if actions.is_empty() => UndoState::NotUndoable,
        UndoRecord::Process { target, .. } if !alive(*target) => UndoState::Expired,
        UndoRecord::Quarantine { expires, .. } if now >= *expires => UndoState::Expired,
        _ => UndoState::Available,
    }
}

/// How long quarantined files are kept before they are deleted for good.
pub const QUARANTINE_DAYS: i64 = 7;

/// When a quarantine made at `at` expires.
pub fn quarantine_expiry(at: Timestamp) -> Timestamp {
    at.plus(QUARANTINE_DAYS * DAY)
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
        let q = UndoRecord::Quarantine {
            id: "q".into(),
            expires: quarantine_expiry(Timestamp(0)),
        };
        assert_eq!(undo_state(&q, false, now, |_| true), UndoState::Available);
        assert_eq!(undo_state(&q, false, Timestamp(8 * DAY), |_| true), UndoState::Expired);
        let t = UndoRecord::Tuning { id: "x".into() };
        assert_eq!(undo_state(&t, false, now, |_| false), UndoState::Available);
        let s = UndoRecord::Startup {
            entry_id: "e".into(),
            previous: StartupState::Enabled,
        };
        assert_eq!(undo_state(&s, false, now, |_| false), UndoState::Available);
        const { assert!(MAX_ENTRIES > 100) };
    }
}
