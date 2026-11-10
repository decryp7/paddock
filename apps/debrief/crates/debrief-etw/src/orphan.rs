//! Which ETW sessions belong to a Debrief that is gone: the decision, apart from the OS, so every platform tests it.

/// What a look at the process that owns a session found (rule 53: "could not look" is not "gone").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// The process runs.
    Running,
    /// The OS said no process has the pid.
    Gone,
    /// The look was refused or failed (an elevated Debrief, or one of another user, refuses `OpenProcess`): nothing is known.
    Unknown,
}

/// `ERROR_INVALID_PARAMETER`: what `OpenProcess` says for a pid nobody holds.
pub const ERROR_INVALID_PARAMETER: u32 = 87;

/// What a failed `OpenProcess` (Win32 error `code`) means: only "invalid parameter" says no such process; access denied
/// and everything else leave the owner unknown.
pub fn owner_after_open_failed(code: u32) -> Owner {
    if code == ERROR_INVALID_PARAMETER {
        Owner::Gone
    } else {
        Owner::Unknown
    }
}

/// Whether a session name with `prefix` belongs to a Debrief that is no longer running. Only an owner seen gone counts: an
/// owner that cannot be looked at keeps its session.
pub fn is_orphan(prefix: &str, name: &str, owner: impl Fn(u32) -> Owner) -> bool {
    let Some(rest) = name.strip_prefix(prefix) else {
        return false;
    };
    match rest.split('-').next().and_then(|p| p.parse::<u32>().ok()) {
        Some(pid) => pid != std::process::id() && owner(pid) == Owner::Gone,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREFIX: &str = "Debrief-dotnet-";

    #[test]
    fn only_a_session_whose_owner_is_seen_gone_is_an_orphan() {
        let name = "Debrief-dotnet-999999-5-1";
        assert!(is_orphan(PREFIX, name, |_| Owner::Gone));
        assert!(!is_orphan(PREFIX, name, |_| Owner::Running));
        assert!(
            !is_orphan(PREFIX, name, |_| Owner::Unknown),
            "an owner that refuses the look (elevated Debrief) keeps its session"
        );
    }

    #[test]
    fn our_own_and_foreign_sessions_are_never_orphans() {
        let me = std::process::id();
        assert!(!is_orphan(PREFIX, &format!("Debrief-dotnet-{me}-5-1"), |_| Owner::Gone));
        assert!(!is_orphan(PREFIX, "Debrief-native-999999-5-1", |_| Owner::Gone));
        assert!(!is_orphan(PREFIX, "Debrief-dotnet-notapid-5-1", |_| Owner::Gone));
    }

    #[test]
    fn access_denied_is_unknown_and_only_invalid_parameter_is_gone() {
        assert_eq!(owner_after_open_failed(ERROR_INVALID_PARAMETER), Owner::Gone);
        assert_eq!(owner_after_open_failed(5), Owner::Unknown, "ERROR_ACCESS_DENIED");
        assert_eq!(owner_after_open_failed(1), Owner::Unknown);
    }
}
