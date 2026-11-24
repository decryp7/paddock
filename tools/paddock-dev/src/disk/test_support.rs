//! What the disk tests share: setting a folder's modification time on every OS.

use std::path::Path;
use std::time::SystemTime;

/// Sets the modification time of the folder `dir` (a test ages a folder this way).
///
/// A plain `File::open` of a folder fails on Windows with `ERROR_ACCESS_DENIED` (os error 5): `CreateFileW` opens a directory
/// only with `FILE_FLAG_BACKUP_SEMANTICS` ("You must set this flag to obtain a handle to a directory", the CreateFileW documentation,
/// learn.microsoft.com), and setting its time (`SetFileInformationByHandle`, `FileBasicInfo`) needs `FILE_WRITE_ATTRIBUTES`, which a
/// read-only open does not ask for. So on Windows the folder is opened for attribute writes only, with that flag (as Debrief's
/// `app_dirs::set_folder_mtime` does); elsewhere a plain open is enough.
/// CI run 8976 (job 48913, windows-latest) failed two `disk::temp` tests on both tries with that error before they used this.
pub(crate) fn set_folder_time(dir: &Path, time: SystemTime) {
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        std::fs::OpenOptions::new()
            .access_mode(FILE_WRITE_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(dir)
    };
    #[cfg(not(windows))]
    let file = std::fs::File::open(dir);
    file.and_then(|f| f.set_modified(time))
        .unwrap_or_else(|e| panic!("cannot set the time of {}: {e}", dir.display()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_folder_gets_the_time_it_is_given() {
        let d = tempfile::tempdir().unwrap();
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        set_folder_time(d.path(), t);
        assert_eq!(std::fs::metadata(d.path()).unwrap().modified().unwrap(), t);
    }
}
