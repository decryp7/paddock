//! When the service restarts itself because its own program changed on disk.
//!
//! launchd and systemd keep the old process running after the app is updated or reinstalled, so the service would run
//! old code until the next log-in. It notices that its executable changed and exits cleanly when no job runs; launchd
//! (`KeepAlive`) or systemd (`Restart=always`) starts the new one, which adopts the runners. On Windows the app or the
//! installer restarts the service, so the watch is off there.

/// A cheap fingerprint of the service's executable file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinaryStamp {
    /// Size in bytes.
    pub len: u64,
    /// Modification time, milliseconds since the Unix epoch.
    pub modified_ms: u64,
}

/// What to do about a changed executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartDecision {
    /// Nothing changed (or it cannot be told): keep running.
    Stay,
    /// It changed but work is running: wait, and look again later.
    WaitForIdle,
    /// It changed and nothing is running: exit so the service manager starts the new program.
    Restart,
}

/// Decides from the stamp taken at start (`started`), the stamp now (`now`; `None` when the file is momentarily
/// missing, as during an update that replaces it) and whether a job or an update is running.
pub fn decide(started: Option<BinaryStamp>, now: Option<BinaryStamp>, busy: bool) -> RestartDecision {
    match (started, now) {
        (Some(a), Some(b)) if a != b => {
            if busy {
                RestartDecision::WaitForIdle
            } else {
                RestartDecision::Restart
            }
        }
        _ => RestartDecision::Stay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(len: u64, modified_ms: u64) -> Option<BinaryStamp> {
        Some(BinaryStamp { len, modified_ms })
    }

    #[test]
    fn an_unchanged_or_unreadable_program_is_left_alone() {
        assert_eq!(decide(s(1, 1), s(1, 1), false), RestartDecision::Stay);
        assert_eq!(decide(s(1, 1), None, false), RestartDecision::Stay);
        assert_eq!(decide(None, s(1, 1), false), RestartDecision::Stay);
        assert_eq!(decide(None, None, true), RestartDecision::Stay);
    }

    #[test]
    fn a_changed_program_restarts_the_service_only_when_idle() {
        assert_eq!(decide(s(1, 1), s(2, 1), false), RestartDecision::Restart);
        assert_eq!(decide(s(1, 1), s(1, 9), false), RestartDecision::Restart);
        assert_eq!(decide(s(1, 1), s(2, 1), true), RestartDecision::WaitForIdle);
    }
}
