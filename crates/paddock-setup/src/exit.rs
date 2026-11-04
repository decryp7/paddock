//! The process exit codes, one table for every command, and the sentence the app shows for an `apply-update` code.

use crate::apply::Refusal;

/// How a command ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// Done; for `same-folder`: the folders are the same.
    Ok,
    /// `same-folder`: not the same (an answer, not a failure).
    NotSame,
    /// `guard`: a per-user copy blocks the install (the installer's "error level 2").
    Blocked,
    /// `check-dir`: the install folder is refused.
    BadDir,
    /// The command line is wrong.
    Usage,
    /// An unexpected I/O or system failure (the log says which).
    Failed,
    /// `stop-all`: a listed program still runs (ADR section 3, exit 11).
    StillRunning,
    /// `apply-update` refused.
    Refused(Refusal),
}

impl Exit {
    /// The process exit code.
    pub fn code(self) -> i32 {
        match self {
            Exit::Ok => 0,
            Exit::NotSame => 1,
            Exit::Blocked => 2,
            Exit::BadDir => 3,
            Exit::Usage => 4,
            Exit::Failed => 5,
            Exit::StillRunning => 11,
            Exit::Refused(r) => r.code(),
        }
    }

    /// The exit for a code, `None` for a code this program never returns.
    pub fn from_code(code: i32) -> Option<Exit> {
        [
            Exit::Ok,
            Exit::NotSame,
            Exit::Blocked,
            Exit::BadDir,
            Exit::Usage,
            Exit::Failed,
            Exit::StillRunning,
        ]
        .into_iter()
        .find(|e| e.code() == code)
        .or_else(|| Refusal::from_code(code).map(Exit::Refused))
    }

    /// A short, user-readable meaning for the installer's details and the app's Failed state.
    pub fn sentence(self) -> &'static str {
        match self {
            Exit::Ok => "done",
            Exit::NotSame => "not the same folder",
            Exit::Blocked => "an older copy installed for one user must be uninstalled first",
            Exit::BadDir => "the install folder is not allowed",
            Exit::Usage => "the setup program was started with wrong arguments",
            Exit::Failed => "the setup program failed; the reason is in setup.log",
            Exit::StillRunning => "a program of the app is still running for another user",
            Exit::Refused(r) => r.sentence(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_and_round_trip() {
        let mut seen = std::collections::BTreeSet::new();
        for e in all() {
            assert!(seen.insert(e.code()), "duplicate code {e:?}");
            assert_eq!(Exit::from_code(e.code()), Some(e));
            assert!(!e.sentence().is_empty());
        }
        assert_eq!(Exit::from_code(99), None);
    }

    #[test]
    fn documented_codes() {
        assert_eq!(Exit::Ok.code(), 0);
        assert_eq!(Exit::Blocked.code(), 2);
        assert_eq!(Exit::StillRunning.code(), 11);
    }

    fn all() -> Vec<Exit> {
        let mut v = vec![
            Exit::Ok,
            Exit::NotSame,
            Exit::Blocked,
            Exit::BadDir,
            Exit::Usage,
            Exit::Failed,
            Exit::StillRunning,
        ];
        v.extend(Refusal::ALL.into_iter().map(Exit::Refused));
        v
    }
}
