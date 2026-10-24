//! OS notifications (plan X8): one policy for every app, over the notification centre.
//!
//! Compared and merged from the four adapters: Debrief's `notify.rs` (a level setting, "only when the user cannot see
//! it", a rate limiter), Spotter's `toast.rs` (buttons and click-through, a validated payload) and `QuietHours`, Pitcrew's
//! and Dyno's plain toasts (nothing to take except what they lack: a policy, limits and buttons; Dyno's sustain and
//! cool-down before an alarm exists stay in `dyno-core`, because they decide whether there *is* an alarm).
//!
//! - [`OsSettings`] / [`SharedOsSettings`]: the user's level, quiet hours and limits.
//! - [`screen`]: the pure policy (level, seen, quiet hours); [`RateLimiter`]: one toast per cause per cool-down and a
//!   burst limit, keyed by the centre's cause key; both on the injected [`Clock`] and [`LocalTime`].
//! - [`OsNotifier`]: policy, limits and the [`Toaster`] port in one `offer`. A skipped message is still in the centre.
//! - [`toast_for`]: the toast of a message, with a button per labelled action; [`route`]: what a click does, read from
//!   the centre's message and never from the OS's payload.
//! - [`FakeToaster`]: records toasts and plays clicks, so no test posts a real notification.
//!
//! The Tauri side is `paddock_health::tauri::Setup::os_notifier`; the plugin adapter is `paddock_health::tauri::native`
//! (feature `os-notify`).

mod limiter;
mod policy;
mod presence;
mod route;
mod settings;
mod testing;
mod time;
mod toast;

pub use limiter::{Limited, MAX_CAUSES, RateLimiter, Reservation};
pub use policy::{Skip, is_alert, screen};
pub use presence::{WindowPresence, user_sees};
pub use route::{Click, Route, Target, route};
pub use settings::{
    BURST, BURST_WINDOW_SECS, COOLDOWN_SECS, OsLevel, OsSettings, SharedOsSettings,
};
pub use testing::FakeToaster;
pub use time::{ClockTime, FixedOffset, LocalTime, QuietHours, SystemLocalTime};
pub use toast::{
    ActionLabels, MAX_TOAST_BODY, MAX_TOAST_TITLE, OsToast, PAYLOAD_KEY, Slot, ToastButton,
    toast_for,
};

use crate::clock::Clock;
use crate::notify::centre::Notice;
use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, Mutex};

/// The cause key of the quiet-hours digest message.
pub const DIGEST_CAUSE: &str = "os:quiet-digest";
/// The most distinct causes counted as held during quiet hours.
pub const MAX_HELD: usize = 512;

/// The OS refused or failed to show a toast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastError(pub String);

impl fmt::Display for ToastError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ToastError {}

/// Whether the OS lets the app notify.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// Allowed.
    Granted,
    /// The user said no (Repair: open the system's notification settings).
    Denied,
    /// Not known (desktop systems that do not ask).
    Unknown,
}

/// Where the OS reports clicks to.
pub type ClickSink = Arc<dyn Fn(Click) + Send + Sync>;

/// The port to the OS (the real one is `tauri::native::PluginToaster`; tests use [`FakeToaster`]).
pub trait Toaster: Send + Sync {
    /// Shows the toast. Must not block on the user.
    fn show(&self, toast: &OsToast) -> Result<(), ToastError>;

    /// Starts reporting clicks on this app's toasts to `sink` (once, at start-up, before the first toast). The default
    /// reports nothing: the click-through then falls back to "the app got the focus soon after a toast".
    fn subscribe(&self, _sink: ClickSink) {}

    /// Whether the app may notify.
    fn permission(&self) -> Permission {
        Permission::Unknown
    }
}

/// What offering a message did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The toast was shown.
    Shown(OsToast),
    /// Held back by the policy; the message is in the centre.
    Skipped(Skip),
    /// The OS refused (logged; the limits are not charged).
    Failed(ToastError),
}

impl Outcome {
    /// Whether a toast was shown.
    pub fn shown(&self) -> bool {
        matches!(self, Outcome::Shown(_))
    }
}

/// The policy, the limits and the OS port together.
pub struct OsNotifier {
    settings: SharedOsSettings,
    limiter: Mutex<RateLimiter>,
    clock: Arc<dyn Clock>,
    local: Arc<dyn LocalTime>,
    toaster: Arc<dyn Toaster>,
    labels: ActionLabels,
    /// Causes whose toast quiet hours held back, for the digest.
    held: Mutex<HashSet<String>>,
}

impl OsNotifier {
    /// A notifier over `toaster`, reading the user's `settings` on every message, with the system's clock and zone and
    /// the shell's button labels.
    pub fn new(settings: SharedOsSettings, toaster: Arc<dyn Toaster>) -> OsNotifier {
        OsNotifier {
            settings,
            limiter: Mutex::new(RateLimiter::new()),
            clock: Arc::new(crate::clock::SystemClock),
            local: Arc::new(SystemLocalTime),
            toaster,
            labels: ActionLabels::default(),
            held: Mutex::new(HashSet::new()),
        }
    }

    /// The clock and the local time (tests move both by hand).
    pub fn with_time(mut self, clock: Arc<dyn Clock>, local: Arc<dyn LocalTime>) -> OsNotifier {
        self.clock = clock;
        self.local = local;
        self
    }

    /// The app's button labels.
    pub fn with_labels(mut self, labels: ActionLabels) -> OsNotifier {
        self.labels = labels;
        self
    }

    /// The settings it reads.
    pub fn settings(&self) -> &SharedOsSettings {
        &self.settings
    }

    /// The OS port.
    pub fn toaster(&self) -> &Arc<dyn Toaster> {
        &self.toaster
    }

    /// Offers a message: shows a toast when the policy and the limits allow it. `seen` is whether the user already sees
    /// the message in a window ([`user_sees`]). Never panics and never fails the caller.
    pub fn offer(&self, notice: &Notice, seen: bool) -> Outcome {
        let settings = self.settings.get();
        let now = self.clock.now_ms();
        if let Err(skip) = screen(&settings, notice.severity, seen, self.local.clock_time(now)) {
            if skip == Skip::QuietHours {
                let mut held = self
                    .held
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if held.len() < MAX_HELD {
                    held.insert(notice.cause.clone());
                }
            }
            return Outcome::Skipped(skip);
        }
        let reservation = {
            let mut limiter = self
                .limiter
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match limiter.allow(&settings, &notice.cause, now) {
                Ok(r) => r,
                Err(limited) => return Outcome::Skipped(limited.into()),
            }
        };
        let toast = toast_for(notice, &self.labels);
        match self.toaster.show(&toast) {
            Ok(()) => Outcome::Shown(toast),
            Err(e) => {
                self.limiter
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .undo(reservation);
                log::warn!(target: "paddock_health", "The OS notification could not be shown: {e}");
                Outcome::Failed(e)
            }
        }
    }

    /// How many causes quiet hours held back and are due a digest now: quiet hours are over (or off), notifications are
    /// on, and the limits would allow a toast. `None` while quiet hours last, when nothing was held, or when the limits
    /// hold the digest back (ask again later). Turning notifications off forgets what was held.
    pub fn pending_digest(&self) -> Option<u32> {
        let settings = self.settings.get();
        let now = self.clock.now_ms();
        if settings.level == OsLevel::Off {
            self.held
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear();
            return None;
        }
        if settings
            .quiet_hours
            .is_some_and(|q| q.contains(self.local.clock_time(now)))
        {
            return None;
        }
        let n = self
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len();
        if n == 0 {
            return None;
        }
        let mut limiter = self
            .limiter
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        limiter.check(&settings, DIGEST_CAUSE, now).ok()?;
        u32::try_from(n).ok()
    }

    /// Shows the digest toast for `notice` (the message [`pending_digest`](Self::pending_digest) was posted as) and
    /// forgets what was held. When the user already sees the window the digest is dropped; when the limits or the OS
    /// hold it back, what was held stays for the next try. Rate-limited under [`DIGEST_CAUSE`] like any other toast.
    pub fn send_digest(&self, notice: &Notice, seen: bool) -> Outcome {
        let clear = || {
            self.held
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear()
        };
        if seen {
            clear();
            return Outcome::Skipped(Skip::Seen);
        }
        let settings = self.settings.get();
        let reservation = {
            let mut limiter = self
                .limiter
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match limiter.allow(&settings, DIGEST_CAUSE, self.clock.now_ms()) {
                Ok(r) => r,
                Err(limited) => return Outcome::Skipped(limited.into()),
            }
        };
        let toast = toast_for(notice, &self.labels);
        match self.toaster.show(&toast) {
            Ok(()) => {
                clear();
                Outcome::Shown(toast)
            }
            Err(e) => {
                self.limiter
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .undo(reservation);
                log::warn!(target: "paddock_health", "The quiet-hours digest could not be shown: {e}");
                Outcome::Failed(e)
            }
        }
    }
}

#[cfg(test)]
mod tests;
