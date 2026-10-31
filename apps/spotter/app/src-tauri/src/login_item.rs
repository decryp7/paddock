//! Where Spotter may register itself to start at login. The pure rule lives here so every OS runs its tests; `commands.rs` applies
//! it before the autostart plugin writes its entry.

use std::path::Path;

/// Refuses a registration for an executable that will not exist after the next login: Gatekeeper path randomisation
/// (`/AppTranslocation/`) or a mounted disk image or another volume (`/Volumes/`). The message says what to do.
pub fn refuse_unstable_location(exe: &Path) -> Result<(), String> {
    let path = exe.to_string_lossy();
    if path.contains("/AppTranslocation/") || path.starts_with("/Volumes/") {
        return Err("Spotter is running from a temporary location (a disk image, another disk or a quarantined download). Move Spotter to the \
             Applications folder on this Mac's startup disk (not another disk or a disk image), open it from there, and turn on Start at login again."
            .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_translocated_or_mounted_copy_may_not_register_and_is_told_to_move_to_applications() {
        for bad in [
            "/private/var/folders/ab/xyz/T/AppTranslocation/78F4-1/d/Spotter.app/Contents/MacOS/spotter-app",
            "/Volumes/Spotter/Spotter.app/Contents/MacOS/spotter-app",
            "/Volumes/Other Disk/Applications/Spotter.app/Contents/MacOS/spotter-app",
        ] {
            let err = refuse_unstable_location(&PathBuf::from(bad)).unwrap_err();
            assert!(err.contains("Move Spotter to the Applications folder"), "{bad}: {err}");
        }
    }

    #[test]
    fn a_copy_in_applications_or_another_user_folder_may_register() {
        for ok in [
            "/Applications/Spotter.app/Contents/MacOS/spotter-app",
            "/Users/me/Applications/Spotter.app/Contents/MacOS/spotter-app",
            r"C:\Users\me\AppData\Local\Spotter\spotter-app.exe",
            "/usr/bin/spotter-app",
        ] {
            assert_eq!(refuse_unstable_location(&PathBuf::from(ok)), Ok(()), "{ok}");
        }
    }
}
