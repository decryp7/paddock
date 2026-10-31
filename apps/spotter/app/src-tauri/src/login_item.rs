//! Where Spotter may register itself to start at login. The rule is pure (so every OS runs its tests) and takes the facts an adapter
//! read from the OS; `commands.rs` applies it before the autostart plugin writes its entry.

use std::path::{Path, PathBuf};

/// Whether the volume the running copy sits on can be written: a mounted disk image is read-only; a second internal disk or an
/// external SSD is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Volume {
    /// Can be written (an internal disk, an external SSD).
    Writable,
    /// Read-only (a mounted disk image).
    ReadOnly,
}

/// What the message says to do.
const MOVE_TO_APPLICATIONS: &str =
    "Spotter can't start at login from here (a disk image or a download macOS has moved aside). Move Spotter to \
     Applications, open it from there, then turn on Launch at login.";

/// Refuses a registration for an executable that will not exist after the next login: Gatekeeper path randomisation
/// (`/AppTranslocation/`), or a copy on a read-only volume under `/Volumes/` (a mounted disk image). A writable volume (a second
/// internal disk, an external SSD, `/Volumes/Macintosh HD`) is allowed.
pub fn refuse_unstable_location(exe: &Path, volume: Volume) -> Result<(), String> {
    let path = exe.to_string_lossy();
    if path.contains("/AppTranslocation/") || (path.starts_with("/Volumes/") && volume == Volume::ReadOnly) {
        return Err(MOVE_TO_APPLICATIONS.into());
    }
    Ok(())
}

/// The running executable and its volume, read from the OS; `None` when the executable is not known (nothing is refused then).
pub fn current_location() -> Option<(PathBuf, Volume)> {
    let exe = std::env::current_exe().ok()?;
    let volume = volume_of(&exe);
    Some((exe, volume))
}

#[cfg(unix)]
fn volume_of(path: &Path) -> Volume {
    match rustix::fs::statvfs(path) {
        Ok(st) if st.f_flag.contains(rustix::fs::StatVfsMountFlags::RDONLY) => Volume::ReadOnly,
        _ => Volume::Writable,
    }
}

#[cfg(not(unix))]
fn volume_of(_path: &Path) -> Volume {
    Volume::Writable
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(path: &str, volume: Volume) -> Option<String> {
        refuse_unstable_location(&PathBuf::from(path), volume).err()
    }

    #[test]
    fn a_translocated_copy_may_not_register_and_is_told_to_move_to_applications() {
        let err = refused(
            "/private/var/folders/ab/xyz/T/AppTranslocation/78F4-1/d/Spotter.app/Contents/MacOS/spotter-app",
            Volume::Writable,
        );
        let err = err.expect("refused");
        assert!(
            err.contains("Move Spotter to") && err.contains("Applications") && err.contains("Launch at login"),
            "{err}"
        );
    }

    #[test]
    fn a_disk_image_mount_is_refused_but_a_second_writable_disk_is_allowed() {
        assert!(
            refused("/Volumes/Spotter/Spotter.app/Contents/MacOS/spotter-app", Volume::ReadOnly).is_some(),
            "a dmg mount"
        );
        assert!(
            refused(
                "/Volumes/Work SSD/Applications/Spotter.app/Contents/MacOS/spotter-app",
                Volume::Writable
            )
            .is_none(),
            "a second disk"
        );
        assert!(refused(
            "/Volumes/Macintosh HD/Applications/Spotter.app/Contents/MacOS/spotter-app",
            Volume::Writable
        )
        .is_none());
    }

    #[test]
    fn a_copy_in_applications_or_another_user_folder_may_register() {
        for ok in [
            "/Applications/Spotter.app/Contents/MacOS/spotter-app",
            "/Users/me/Applications/Spotter.app/Contents/MacOS/spotter-app",
            r"C:\Users\me\AppData\Local\Spotter\spotter-app.exe",
            "/usr/bin/spotter-app",
        ] {
            assert_eq!(refused(ok, Volume::Writable), None, "{ok}");
        }
        // Only /Volumes/ paths are judged by the volume: a read-only /usr is not a disk image.
        assert_eq!(refused("/usr/bin/spotter-app", Volume::ReadOnly), None);
    }
}
