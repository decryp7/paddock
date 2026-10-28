//! Whether the OS has given Dyno the rights to read the whole disk. Only macOS asks for them (Full Disk Access, a
//! privacy permission); everywhere else a refused folder is just a folder the user may not read.

use std::io;

/// A file every Mac has that the privacy layer shields: opening it works only with Full Disk Access.
#[cfg(target_os = "macos")]
const SHIELDED: &str = "/Library/Application Support/com.apple.TCC/TCC.db";

/// Whether an attempt to open the shielded file says the permission is missing: only a refusal does. A file that is
/// not there, or any other error, says nothing, and Dyno then does not claim a cause it cannot see.
pub fn refusal_means_missing(attempt: &io::Result<()>) -> bool {
    matches!(attempt, Err(e) if e.kind() == io::ErrorKind::PermissionDenied)
}

/// True when this is a Mac and Dyno has not been given Full Disk Access (the file is opened, never read).
pub fn full_disk_access_missing() -> bool {
    #[cfg(target_os = "macos")]
    {
        refusal_means_missing(&std::fs::File::open(SHIELDED).map(|_| ()))
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// The pane of System Settings where Full Disk Access is given.
#[cfg(target_os = "macos")]
const PANE: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

/// Opens the system pane where the person gives Dyno Full Disk Access. Only macOS has one: elsewhere the answer is
/// `Unsupported` and says what to do instead.
pub fn open_full_disk_access_settings() -> Result<(), dyno_core::ports::PlatformError> {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = crate::process::command("/usr/bin/open");
        cmd.arg(PANE);
        let out = crate::process::run_with_timeout(cmd, std::time::Duration::from_secs(5))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(dyno_core::ports::PlatformError::Failed(
                "System Settings did not open. Open Privacy & Security > Full Disk Access yourself.".into(),
            ))
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(dyno_core::ports::PlatformError::Unsupported(
            "Full Disk Access is a macOS setting. Here, give your user read access to the folders that were refused.".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_refusal_means_the_permission_is_missing() {
        assert!(refusal_means_missing(&Err(io::Error::from(io::ErrorKind::PermissionDenied))));
        assert!(!refusal_means_missing(&Ok(())));
        assert!(!refusal_means_missing(&Err(io::Error::from(io::ErrorKind::NotFound))));
        assert!(!refusal_means_missing(&Err(io::Error::other("odd"))));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn other_systems_say_why_there_is_no_pane() {
        let e = open_full_disk_access_settings().unwrap_err();
        assert!(matches!(e, dyno_core::ports::PlatformError::Unsupported(m) if m.contains("macOS")));
    }

    #[test]
    fn the_real_probe_answers_without_changing_anything() {
        // Opens a system file of this machine (macOS) and reports; it does nothing unless the opt-in variable is set (ER-14).
        if std::env::var_os("DYNO_REAL_MACHINE_TESTS").is_none() {
            return;
        }
        let missing = full_disk_access_missing();
        #[cfg(not(target_os = "macos"))]
        assert!(!missing);
        let _ = missing;
    }
}
