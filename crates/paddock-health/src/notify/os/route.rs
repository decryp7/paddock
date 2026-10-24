//! What a click on a toast does. The OS reports the message id and which button; the message is read from the centre
//! again (it may be gone, or changed), and its action in that slot decides: the toast itself opens the message, a repair
//! runs in the shell (rule 19: never through the service that is down), anything else goes to the window or to the app's
//! own handler for that kind of action.

use super::toast::Slot;
use crate::model::is_service_id;
use crate::notify::centre::{Notice, NoticeAction, NoticeId};

/// What the user clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The toast itself.
    Toast,
    /// One of its buttons.
    Button(Slot),
}

impl Target {
    /// From the id the OS reports: `tap` is the toast itself; anything unknown is nothing (the OS payload is not trusted).
    pub fn from_id(id: &str) -> Option<Target> {
        if id == "tap" || id == "default" {
            return Some(Target::Toast);
        }
        Slot::from_id(id).map(Target::Button)
    }
}

/// A click reported by the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Click {
    /// The message the toast was about.
    pub notice: NoticeId,
    /// What was clicked.
    pub target: Target,
}

/// What to do about a click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Bring the window forward and open the message.
    Open(NoticeId),
    /// Repair a service from the shell.
    Repair(String),
    /// Run an action of the app or the window (`openSettings`, `viewJob`...).
    Forward(NoticeAction),
    /// Nothing (a button the message no longer has, a bad service id).
    Nothing,
}

/// The route of a click on a toast of `notice` (as it is in the centre now).
pub fn route(notice: &Notice, target: Target) -> Route {
    let Target::Button(slot) = target else {
        return Route::Open(notice.id);
    };
    let Some(action) = slot.action(notice) else {
        return Route::Nothing;
    };
    if action.kind == NoticeAction::REPAIR_SERVICE {
        return match action.args.get("service").and_then(|v| v.as_str()) {
            Some(service) if is_service_id(service) => Route::Repair(service.to_string()),
            _ => Route::Nothing,
        };
    }
    Route::Forward(action.clone())
}
