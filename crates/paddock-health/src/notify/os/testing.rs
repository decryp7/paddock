//! A [`Toaster`] that shows nothing: it records the toasts, can be told to fail, and plays clicks back. Public so an
//! app's wiring tests use the same fake (rule 7: tests never post real notifications).

use super::{Click, ClickSink, OsToast, Permission, ToastError, Toaster};
use std::sync::Mutex;

/// Records toasts instead of showing them.
#[derive(Default)]
pub struct FakeToaster {
    shown: Mutex<Vec<OsToast>>,
    sink: Mutex<Option<ClickSink>>,
    failing: Mutex<Option<String>>,
    permission: Mutex<Option<Permission>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl FakeToaster {
    /// The toasts shown so far.
    pub fn shown(&self) -> Vec<OsToast> {
        lock(&self.shown).clone()
    }

    /// Makes the next toasts fail with `reason` (`None`: succeed again).
    pub fn fail_with(&self, reason: Option<&str>) {
        *lock(&self.failing) = reason.map(str::to_string);
    }

    /// What `permission` answers.
    pub fn set_permission(&self, permission: Permission) {
        *lock(&self.permission) = Some(permission);
    }

    /// Plays a click from the OS. False when nobody subscribed.
    pub fn click(&self, click: Click) -> bool {
        let sink = lock(&self.sink).clone();
        sink.map(|s| s(click)).is_some()
    }
}

impl Toaster for FakeToaster {
    fn show(&self, toast: &OsToast) -> Result<(), ToastError> {
        if let Some(reason) = lock(&self.failing).clone() {
            return Err(ToastError(reason));
        }
        lock(&self.shown).push(toast.clone());
        Ok(())
    }

    fn subscribe(&self, sink: ClickSink) {
        *lock(&self.sink) = Some(sink);
    }

    fn permission(&self) -> Permission {
        lock(&self.permission).unwrap_or(Permission::Unknown)
    }
}
