//! The one policy: whether a message becomes an OS toast. In order: the user's level, whether the user already sees it,
//! quiet hours, then the rate limits. A message that does not pass is still in the notification centre; only the toast
//! is held back. Pure, so the whole table is tested (`notify::os::tests`).

use super::limiter::Limited;
use super::settings::{OsLevel, OsSettings};
use super::time::ClockTime;
use crate::notify::centre::NoticeSeverity;

/// Why a message did not become a toast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Skip {
    /// The user turned OS notifications off.
    Off,
    /// The level is "alerts" and this is not one.
    NotAnAlert,
    /// A window the message belongs to is in front: the user sees it there.
    Seen,
    /// Inside quiet hours.
    QuietHours,
    /// The same cause notified a moment ago.
    Cooldown,
    /// Too many toasts in the window.
    Burst,
}

impl From<Limited> for Skip {
    fn from(l: Limited) -> Skip {
        match l {
            Limited::Cooldown => Skip::Cooldown,
            Limited::Burst => Skip::Burst,
        }
    }
}

/// Whether a severity counts as an alert (something went wrong or needs the user).
pub fn is_alert(severity: NoticeSeverity) -> bool {
    matches!(
        severity,
        NoticeSeverity::Warning | NoticeSeverity::Error | NoticeSeverity::Critical
    )
}

/// The static part of the decision (everything but the rate limits, which need memory): `Ok` when the message may
/// notify. `local` is the user's local time of day.
pub fn screen(
    settings: &OsSettings,
    severity: NoticeSeverity,
    seen: bool,
    local: ClockTime,
) -> Result<(), Skip> {
    match settings.level {
        OsLevel::Off => return Err(Skip::Off),
        OsLevel::Alerts if !is_alert(severity) => return Err(Skip::NotAnAlert),
        OsLevel::Alerts | OsLevel::All => {}
    }
    if seen {
        return Err(Skip::Seen);
    }
    let critical = severity == NoticeSeverity::Critical && settings.critical_in_quiet_hours;
    if !critical && settings.quiet_hours.is_some_and(|q| q.contains(local)) {
        return Err(Skip::QuietHours);
    }
    Ok(())
}
