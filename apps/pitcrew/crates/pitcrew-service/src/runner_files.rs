//! Reading and writing a runner's `config.yaml` (ADR 0011): bounded reads, a timestamped backup of the previous file
//! (the newest [`MAX_BACKUPS`] are kept), and an atomic replace that keeps the file's permissions and follows a
//! symbolic link to the real file. The text is never logged.

use crate::error::Error;
use pitcrew_core::runner_config::{backup_name, backups_to_prune, is_backup, MAX_BACKUPS};
use pitcrew_core::runner_schema::MAX_CONFIG_TEXT;
use pitcrew_core::time::Timestamp;
use std::io::Write;
use std::path::{Path, PathBuf};

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.display().to_string(),
        source,
    }
}

/// The file's text; empty when it does not exist yet. A file larger than the editor accepts is an error.
pub fn read(path: &Path) -> Result<String, Error> {
    match crate::store::read_bounded(path, MAX_CONFIG_TEXT)? {
        None => Ok(String::new()),
        Some(t) if t.len() > MAX_CONFIG_TEXT => Err(Error::Os {
            what: "reading the runner configuration",
            reason: format!(
                "{} is larger than {} KB; edit it with your own editor",
                path.display(),
                MAX_CONFIG_TEXT / 1024
            ),
        }),
        Some(t) => Ok(t),
    }
}

/// What a save did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    /// The backup's file name (in the same folder); `None` when there was no file before.
    pub backup: Option<String>,
}

/// The names of the files next to `path` (for backup pruning and counting).
fn siblings(path: &Path) -> Vec<String> {
    let Some(dir) = path.parent() else { return vec![] };
    std::fs::read_dir(dir)
        .map(|it| it.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect())
        .unwrap_or_default()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| "config.yaml".to_string(), |n| n.to_string_lossy().into_owned())
}

/// How many Pitcrew backups of `path` exist.
pub fn backups(path: &Path) -> u32 {
    let name = file_name(path);
    u32::try_from(siblings(path).iter().filter(|n| is_backup(&name, n)).count()).unwrap_or(u32::MAX)
}

/// Replaces `path` with `text` after copying the old file to a timestamped backup next to it, then deletes the
/// oldest backups beyond [`MAX_BACKUPS`]. If `path` is a symbolic link the file it points to is replaced.
pub fn save(path: &Path, text: &str, now: Timestamp) -> Result<Saved, Error> {
    let real: PathBuf = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = real.parent().map(Path::to_path_buf).unwrap_or_default();
    if !dir.as_os_str().is_empty() {
        std::fs::create_dir_all(&dir).map_err(io(&dir))?;
    }
    let name = file_name(&real);
    let backup = if real.exists() {
        let b = backup_name(&name, now);
        std::fs::copy(&real, dir.join(&b)).map_err(io(&real))?;
        Some(b)
    } else {
        None
    };
    // Write next to the target and rename, so a crash leaves the old file or the new one, never half.
    let tmp = dir.join(format!(".{name}.pitcrew-tmp"));
    {
        let mut f = std::fs::File::create(&tmp).map_err(io(&tmp))?;
        f.write_all(text.as_bytes()).map_err(io(&tmp))?;
        f.sync_all().map_err(io(&tmp))?;
    }
    if let Ok(meta) = std::fs::metadata(&real) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    if let Err(e) = std::fs::rename(&tmp, &real) {
        let _ = std::fs::remove_file(&tmp);
        return Err(io(&real)(e));
    }
    for old in backups_to_prune(&name, &siblings(&real), MAX_BACKUPS) {
        let _ = std::fs::remove_file(dir.join(old));
    }
    Ok(Saved { backup })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(test: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pitcrew-files-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_missing_file_reads_as_empty_and_a_huge_one_is_refused() {
        let d = dir("read");
        let f = d.join("config.yaml");
        assert_eq!(read(&f).unwrap(), "");
        std::fs::write(&f, "a: 1\n").unwrap();
        assert_eq!(read(&f).unwrap(), "a: 1\n");
        std::fs::write(&f, "#".repeat(MAX_CONFIG_TEXT + 10)).unwrap();
        let err = read(&f).unwrap_err().to_string();
        assert!(err.contains("larger than") && err.contains("your own editor"), "{err}");
        assert!(read(&d).is_err(), "a folder is not a file");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn saving_backs_up_the_old_file_and_keeps_only_the_newest_backups() {
        let d = dir("save");
        let f = d.join("config.yaml");
        // The first save has nothing to back up.
        let first = save(&f, "one\n", Timestamp(1_792_683_000_000)).unwrap();
        assert_eq!(first.backup, None);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "one\n");
        // The runner's own plain backup is not ours and stays.
        std::fs::write(d.join("config.yaml.bak"), "theirs").unwrap();
        for n in 1..=12u64 {
            let saved = save(&f, &format!("v{n}\n"), Timestamp(1_792_683_000_000 + n * 1000)).unwrap();
            assert!(saved.backup.is_some());
        }
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "v12\n");
        assert_eq!(backups(&f), MAX_BACKUPS as u32);
        assert_eq!(std::fs::read_to_string(d.join("config.yaml.bak")).unwrap(), "theirs");
        let newest = backup_name("config.yaml", Timestamp(1_792_683_000_000 + 12_000));
        assert_eq!(
            std::fs::read_to_string(d.join(newest)).unwrap(),
            "v11\n",
            "the backup holds the previous file"
        );
        assert!(!std::fs::read_dir(&d)
            .unwrap()
            .any(|e| e.unwrap().file_name().to_string_lossy().contains("pitcrew-tmp")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn permissions_and_symbolic_links_are_respected() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir("link");
        let real = d.join("real.yaml");
        std::fs::write(&real, "a: 1\n").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o640)).unwrap();
        let link = d.join("config.yaml");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        save(&link, "a: 2\n", Timestamp(1_792_683_000_000)).unwrap();
        assert!(
            std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(),
            "the link stays a link"
        );
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "a: 2\n");
        assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o640);
        assert!(std::fs::read_dir(&d)
            .unwrap()
            .any(|e| e.unwrap().file_name().to_string_lossy().starts_with("real.yaml.bak-")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_folder_that_cannot_be_written_is_an_error_with_the_path() {
        let d = dir("fail");
        let blocker = d.join("file");
        std::fs::write(&blocker, "x").unwrap();
        let err = save(&blocker.join("config.yaml"), "a: 1\n", Timestamp(1)).unwrap_err().to_string();
        assert!(err.contains("file"), "{err}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
