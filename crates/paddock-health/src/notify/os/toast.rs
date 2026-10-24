//! What the OS shows for a message: its title and body, bounded, and up to three buttons, one per action the message
//! offers. Spotter's `toast.rs` (buttons per OS), Pitcrew's and Dyno's plain toasts and Debrief's text rule meet here:
//! the toast never carries more than the message does, and the OS hands back only the message's id and which button, so
//! what a click does is read from the centre's message again, never from the OS's payload.

use crate::notify::centre::{MAX_KEY, Notice, NoticeAction, NoticeId};
use std::collections::HashMap;

/// The longest toast title, in characters (the OSes cut long ones; this keeps them readable).
pub const MAX_TOAST_TITLE: usize = 80;
/// The longest toast body, in characters.
pub const MAX_TOAST_BODY: usize = 240;
/// The key of the message id in the toast's payload.
pub const PAYLOAD_KEY: &str = "paddockNotice";

/// Which of the message's three actions a button runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    /// `Notice::primary`.
    Primary,
    /// `Notice::secondary`.
    Secondary,
    /// `Notice::tertiary`.
    Tertiary,
}

impl Slot {
    /// The button id the OS reports back.
    pub fn id(self) -> &'static str {
        match self {
            Slot::Primary => "primary",
            Slot::Secondary => "secondary",
            Slot::Tertiary => "tertiary",
        }
    }

    /// The slot of a button id the OS reported.
    pub fn from_id(id: &str) -> Option<Slot> {
        [Slot::Primary, Slot::Secondary, Slot::Tertiary]
            .into_iter()
            .find(|s| s.id() == id)
    }

    /// The action of `notice` in this slot.
    pub fn action(self, notice: &Notice) -> Option<&NoticeAction> {
        match self {
            Slot::Primary => notice.primary.as_ref(),
            Slot::Secondary => notice.secondary.as_ref(),
            Slot::Tertiary => notice.tertiary.as_ref(),
        }
    }
}

/// One button of a toast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastButton {
    /// Which action it runs.
    pub slot: Slot,
    /// Its label.
    pub label: String,
}

/// A toast, ready for the OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsToast {
    /// The message it is about (what a click reports back).
    pub notice: NoticeId,
    /// The message's cause key.
    pub cause: String,
    /// The title (with a count when the message happened more than once).
    pub title: String,
    /// The body.
    pub body: String,
    /// Buttons, in the message's order.
    pub buttons: Vec<ToastButton>,
}

impl OsToast {
    /// The id of the button layout: toasts with the same labels share one registered action type.
    pub fn action_type(&self) -> Option<String> {
        (!self.buttons.is_empty()).then(|| {
            let mut id = String::from("pd");
            for b in &self.buttons {
                id.push('|');
                id.push_str(b.slot.id());
                id.push(':');
                id.push_str(&b.label);
            }
            id
        })
    }
}

/// Labels for the action kinds a toast may offer as buttons. The shell's own are built in; an app adds its own
/// (`viewJob` is "View job"). An action without a label is not offered as a button: the toast itself still opens the message.
#[derive(Debug, Clone)]
pub struct ActionLabels(HashMap<String, String>);

impl Default for ActionLabels {
    fn default() -> ActionLabels {
        ActionLabels::empty()
            .label(NoticeAction::REPAIR_SERVICE, "Repair")
            .label(NoticeAction::OPEN_SETTINGS, "Open settings")
    }
}

impl ActionLabels {
    /// No labels at all.
    pub fn empty() -> ActionLabels {
        ActionLabels(HashMap::new())
    }

    /// Labels `kind` (an action's `type`).
    pub fn label(mut self, kind: impl Into<String>, label: impl Into<String>) -> ActionLabels {
        let label: String = label.into().chars().take(40).collect();
        let kind: String = kind.into().chars().take(MAX_KEY).collect();
        if !label.trim().is_empty() {
            self.0.insert(kind, label);
        }
        self
    }

    fn get(&self, kind: &str) -> Option<&str> {
        self.0.get(kind).map(String::as_str)
    }
}

/// `s` as one line of at most `max` characters, with an ellipsis when cut.
fn bounded(s: &str, max: usize) -> String {
    let one: String = s
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !c.is_control())
        .collect();
    let one = one.trim();
    if one.chars().count() <= max {
        return one.to_string();
    }
    let mut cut: String = one.chars().take(max.saturating_sub(1)).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('\u{2026}');
    cut
}

/// The toast for a message.
pub fn toast_for(notice: &Notice, labels: &ActionLabels) -> OsToast {
    let title = if notice.count > 1 {
        format!(
            "{} ({})",
            bounded(&notice.title, MAX_TOAST_TITLE.saturating_sub(8)),
            notice.count
        )
    } else {
        bounded(&notice.title, MAX_TOAST_TITLE)
    };
    let buttons = [Slot::Primary, Slot::Secondary, Slot::Tertiary]
        .into_iter()
        .filter_map(|slot| {
            let action = slot.action(notice)?;
            Some(ToastButton {
                slot,
                label: labels.get(&action.kind)?.to_string(),
            })
        })
        .collect();
    OsToast {
        notice: notice.id,
        cause: notice.cause.clone(),
        title,
        body: bounded(&notice.body, MAX_TOAST_BODY),
        buttons,
    }
}
