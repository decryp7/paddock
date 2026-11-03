use crate::{Hold, HoldInfo, HoldRequest, PowerError, PowerOs};
use std::sync::{Arc, Mutex, MutexGuard};

/// The log target of keep-awake decisions.
pub const LOG_TARGET: &str = "paddock_power";

struct Active {
    info: HoldInfo,
    _hold: Box<dyn Hold>,
}

/// At most one hold at a time, taken through a [`PowerOs`]. A new hold replaces the old one without a gap (the new one is
/// taken before the old one is released). Dropping the guard releases the hold.
pub struct KeepAwake {
    os: Arc<dyn PowerOs>,
    active: Mutex<Option<Active>>,
}

impl KeepAwake {
    /// A guard over `os`, holding nothing yet.
    pub fn new(os: Arc<dyn PowerOs>) -> Self {
        Self {
            os,
            active: Mutex::new(None),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Option<Active>> {
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Holds the system awake as `request` says, replacing a hold already taken. On error the old hold (if any) stays.
    pub fn hold(&self, request: &HoldRequest) -> Result<HoldInfo, PowerError> {
        if request.who.is_empty() || request.why.is_empty() {
            return Err(PowerError::Invalid(
                "a hold needs a name and a reason".into(),
            ));
        }
        let hold = match self.os.acquire(request) {
            Ok(h) => h,
            Err(e) => {
                log::warn!(target: LOG_TARGET, "keep-awake acquire failed scope={:?}: {e}", request.scope);
                return Err(e);
            }
        };
        let info = HoldInfo {
            scope: request.scope,
            why: request.why.clone(),
            os_ref: hold.os_ref(),
        };
        log::info!(target: LOG_TARGET, "keep-awake acquired scope={:?} os_ref={}", info.scope, info.os_ref);
        let old = self.lock().replace(Active {
            info: info.clone(),
            _hold: hold,
        });
        if let Some(old) = old {
            log::info!(target: LOG_TARGET, "keep-awake released (replaced) os_ref={}", old.info.os_ref);
        }
        Ok(info)
    }

    /// Releases the hold; false when there was none.
    pub fn release(&self) -> bool {
        let old = self.lock().take();
        match old {
            Some(old) => {
                log::info!(target: LOG_TARGET, "keep-awake released os_ref={}", old.info.os_ref);
                true
            }
            None => false,
        }
    }

    /// The live hold, if any.
    pub fn current(&self) -> Option<HoldInfo> {
        self.lock().as_ref().map(|a| a.info.clone())
    }
}
