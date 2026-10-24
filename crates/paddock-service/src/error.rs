//! Typed errors. Every message says what to do next; none contains file contents.

use std::fmt;
use std::path::PathBuf;

/// A [`crate::ServiceSpec`] part that would break a file or a command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecError {
    /// Which part.
    pub field: &'static str,
    /// What is wrong with it.
    pub reason: String,
}

impl SpecError {
    pub(crate) fn new(field: &'static str, reason: impl Into<String>) -> Self {
        Self {
            field,
            reason: reason.into(),
        }
    }
}

impl fmt::Display for SpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the service's {} {}", self.field, self.reason)
    }
}

impl std::error::Error for SpecError {}

/// Why registering, removing or repairing the background process did not work.
#[derive(Debug)]
pub enum ServiceError {
    /// The user's home folder is unknown, so the OS's folder for the definition cannot be found.
    NoHome,
    /// A file could not be written, read or removed.
    Io {
        /// What was being done.
        op: &'static str,
        /// The file.
        path: PathBuf,
        /// The cause.
        source: std::io::Error,
    },
    /// macOS needs the user's approval in System Settings > General > Login Items.
    NeedsApproval,
    /// Registering needs administrator rights (Windows service, launch daemon, systemd system unit).
    NeedsElevation,
    /// The user declined the OS's permission prompt.
    Declined,
    /// It cannot be registered from here; the text says why and what to do.
    Unavailable(String),
    /// The user switched it off (Login Items, Task Manager > Startup, Services, `systemctl disable`); Paddock never
    /// switches it back on by itself.
    SwitchedOff,
    /// Another copy of the app, a newer version, looks after the registration.
    NewerOwner(String),
    /// Another registration or repair is running (in this app or another copy of it).
    Busy,
    /// The OS refused or a step failed; the text says what to do.
    Failed(String),
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoHome => f.write_str("Your home folder cannot be found (HOME is not set), so the background service cannot be registered."),
            Self::Io { op, path, source } => write!(f, "Could not {op} {}: {source}", path.display()),
            Self::NeedsApproval => f.write_str("macOS needs your approval: allow the app in System Settings > General > Login Items."),
            Self::NeedsElevation => f.write_str("This needs administrator rights; confirm Windows' permission prompt, or run the installer again."),
            Self::Declined => f.write_str("The permission prompt was declined, so nothing was changed."),
            Self::Unavailable(why) | Self::Failed(why) => f.write_str(why),
            Self::SwitchedOff => f.write_str(
                "The background service is switched off in the system settings. Switch it on there; the app never does it for you.",
            ),
            Self::NewerOwner(v) => write!(
                f,
                "A newer copy of the app (version {v}) looks after the background service. Open that copy, or remove the older one."
            ),
            Self::Busy => f.write_str("The background service is being registered or repaired already; wait for it to finish."),
        }
    }
}

impl std::error::Error for ServiceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_says_what_to_do() {
        let io = ServiceError::Io {
            op: "write",
            path: PathBuf::from("/x/y.plist"),
            source: std::io::Error::other("disk full"),
        };
        assert!(io.to_string().contains("/x/y.plist") && io.to_string().contains("disk full"));
        assert!(std::error::Error::source(&io).is_some());
        for (e, needle) in [
            (ServiceError::NoHome, "HOME"),
            (ServiceError::NeedsApproval, "Login Items"),
            (ServiceError::NeedsElevation, "administrator"),
            (ServiceError::Declined, "declined"),
            (ServiceError::Unavailable("move it".into()), "move it"),
            (ServiceError::Failed("why".into()), "why"),
            (ServiceError::SwitchedOff, "never does it for you"),
            (ServiceError::NewerOwner("2.0.0".into()), "2.0.0"),
            (ServiceError::Busy, "already"),
        ] {
            assert!(e.to_string().contains(needle), "{e}");
            assert!(std::error::Error::source(&e).is_none());
        }
        let s = SpecError::new("label", "is bad");
        assert_eq!(s.to_string(), "the service's label is bad");
    }
}
