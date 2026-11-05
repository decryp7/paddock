//! The helper's lifecycle file: a few lines saying when it started, served and stopped, and why. `helper.log` is root's (0600), so a
//! standard user's problem report could not show why a helper is down; this file is world-readable and holds no user content (a
//! time, an event word, the process id, the version and a refusal's kind), which is why it can be.
//!
//! Best effort: a helper that cannot write it still runs (the failure is logged). The file is bounded: it keeps the newest
//! [`MAX_LINES`] lines.

use std::io::Write;
use std::path::Path;

/// Lines kept.
pub const MAX_LINES: usize = 60;

/// One line: `t=<unix seconds> <event> pid=<pid> version=<version>`. `event` is a word from a fixed set (no user content):
/// `starting`, `serving`, `stopped`, `refused:<failure kind>`.
pub fn line(at: i64, event: &str, pid: u32, version: &str) -> String {
    format!("t={at} {event} pid={pid} version={version}")
}

/// The most bytes of the file ever read (its newest part): a planted huge file costs a bounded read.
const MAX_READ: u64 = 32 * 1024;

/// Opens (creating, 0644) for reading and writing and never follows a link at the last component: the kernel refuses (`O_NOFOLLOW`), so a
/// link planted between the checks of [`open_checked`] and this open is still not followed.
fn open_nofollow(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o644).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

/// Opens the file for reading and writing without following a link and refuses anything but a regular file with one name (a hard
/// link would let a write reach a file elsewhere). The parent folder must not be a link either.
fn open_checked(path: &Path) -> std::io::Result<std::fs::File> {
    let refuse = |why: &str| std::io::Error::new(std::io::ErrorKind::InvalidInput, why.to_string());
    if let Some(parent) = path.parent() {
        if std::fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(refuse("the lifecycle file's folder is a link; it was not written"));
        }
    }
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(refuse("the lifecycle file is a link; it was not written"));
    }
    let file = open_nofollow(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(refuse("the lifecycle file is not a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() > 1 {
            return Err(refuse(
                "the lifecycle file has more than one name (a hard link); it was not written",
            ));
        }
    }
    Ok(file)
}

/// Opens the file for reading only if it is a regular file: never blocks on a pipe someone put in its place (`O_NONBLOCK`), never
/// follows a link at the last component.
fn open_for_reading(path: &Path) -> Option<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
    }
    let file = options.open(path).ok()?;
    file.metadata().ok()?.is_file().then_some(file)
}

/// The newest part of the file (at most [`MAX_READ`] bytes), cut to whole lines.
fn read_bounded(file: &mut std::fs::File) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let len = file.metadata().map_or(0, |m| m.len());
    let start = len.saturating_sub(MAX_READ);
    let mut bytes = Vec::new();
    if file.seek(SeekFrom::Start(start)).is_err() || file.take(MAX_READ).read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if start > 0 {
        text.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default()
    } else {
        text
    }
}

/// Appends `line` to `path`, keeping the newest [`MAX_LINES`], and makes the file readable by everyone (Unix 0644). Never through a
/// link, never to a file with another name, never in a folder that is a link.
///
/// # Errors
/// The I/O error; callers log it and carry on.
pub fn append(path: &Path, line: &str) -> std::io::Result<()> {
    use std::io::{Seek, SeekFrom};
    let mut file = open_checked(path)?;
    let existing = read_bounded(&mut file);
    let mut lines: Vec<&str> = existing.lines().collect();
    lines.push(line);
    let keep = lines.len().saturating_sub(MAX_LINES);
    let mut text = lines[keep..].join("\n");
    text.push('\n');
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(text.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o644))?;
    }
    Ok(())
}

/// Removes the file (Uninstall); a missing file is fine, a link is removed as a link and never followed.
///
/// # Errors
/// The I/O error other than "not found".
pub fn remove(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Whether the lifecycle file says the previous run did not finish: its last event is `starting` or `serving` (a run that ends writes
/// `stopped` or `refused:<kind>` last). A missing or empty file is a first run, not a crash.
pub fn previous_run_unclean(text: &str) -> bool {
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .and_then(|l| l.split_whitespace().nth(1))
        .is_some_and(|event| event == "starting" || event == "serving")
}

/// Logs how the previous run ended (a warning when it did not finish); call before recording `starting`.
pub fn log_previous_run(path: &Path) {
    let text = open_for_reading(path).map(|mut f| read_bounded(&mut f));
    match text {
        Some(text) if previous_run_unclean(&text) => {
            log::warn!("Helper: the previous run did not end cleanly (its lifecycle file ends in starting or serving); a power cut, a kill or a crash")
        }
        _ => log::debug!("Helper: the previous run ended cleanly or this is the first"),
    }
}

/// Records `event` now; a failure is logged, never raised.
pub fn record(path: &Path, event: &str) {
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    if let Err(e) = append(path, &line(at, event, std::process::id(), crate::VERSION)) {
        log::warn!(Error = e.to_string().as_str(); "Helper: the lifecycle file could not be written");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_append_keep_the_newest_and_are_world_readable() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("lifecycle.log");
        for i in 0..(MAX_LINES + 5) {
            append(&p, &line(1_762_333_200, &format!("e{i}"), 7, "1.1.0")).unwrap();
        }
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(text.lines().count(), MAX_LINES);
        assert!(text.lines().last().unwrap().contains(&format!("e{}", MAX_LINES + 4)));
        assert!(!text.contains(" e0 "));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o644);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_hard_link_a_planted_folder_and_a_huge_file_are_handled() {
        let dir = tempfile::tempdir().unwrap();
        // A hard link to a file elsewhere: refused, the other file is untouched.
        let other = dir.path().join("other");
        std::fs::write(&other, "keep\n").unwrap();
        let p = dir.path().join("lifecycle.log");
        std::fs::hard_link(&other, &p).unwrap();
        assert!(append(&p, "x").is_err());
        assert_eq!(std::fs::read_to_string(&other).unwrap(), "keep\n");
        // A folder planted as a link to somewhere else: nothing is written through it.
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let planted = dir.path().join("planted");
        std::os::unix::fs::symlink(&real, &planted).unwrap();
        assert!(append(&planted.join("lifecycle.log"), "x").is_err());
        assert!(!real.join("lifecycle.log").exists());
        // A huge file costs a bounded read and is cut back to the newest lines.
        let big = dir.path().join("big.log");
        std::fs::write(&big, "t=1 starting pid=1 version=1\n".repeat(200_000)).unwrap();
        append(&big, "t=2 stopped pid=1 version=1").unwrap();
        let text = std::fs::read_to_string(&big).unwrap();
        assert!(
            text.lines().count() <= MAX_LINES && text.ends_with("t=2 stopped pid=1 version=1\n"),
            "{}",
            text.len()
        );
        remove(&big).unwrap();
        remove(&big).unwrap();
        assert!(!big.exists());
    }

    #[cfg(unix)]
    #[test]
    fn the_kernel_refuses_a_link_even_when_the_checks_before_the_open_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("elsewhere");
        std::fs::write(&target, "keep\n").unwrap();
        let p = dir.path().join("lifecycle.log");
        std::os::unix::fs::symlink(&target, &p).unwrap();
        assert!(
            open_nofollow(&p).is_err(),
            "O_NOFOLLOW is what closes the window between the check and the open"
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep\n");
    }

    #[test]
    fn the_read_is_bounded_to_the_newest_part_at_a_line() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.log");
        std::fs::write(&p, "t=1 starting pid=1 version=1\n".repeat(50_000)).unwrap();
        let mut f = std::fs::File::open(&p).unwrap();
        let text = read_bounded(&mut f);
        assert!(text.len() as u64 <= MAX_READ, "{} bytes read", text.len());
        assert!(text.starts_with("t=1 "), "cut at a line start: {:?}", &text[..20.min(text.len())]);
    }

    #[cfg(unix)]
    #[test]
    fn only_a_regular_file_is_written() {
        // /dev/null opens for writing, has one name and is no link: only the regular-file check refuses it.
        assert!(open_checked(Path::new("/dev/null")).is_err());
    }

    #[cfg(unix)]
    #[test]
    #[allow(unsafe_code)]
    fn a_pipe_in_the_place_of_the_file_does_not_hang_the_start() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("lifecycle.log");
        let c = std::ffi::CString::new(p.to_str().unwrap()).unwrap();
        // SAFETY: a NUL-terminated path that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o644) }, 0);
        let (tx, rx) = std::sync::mpsc::channel();
        let path = p.clone();
        std::thread::spawn(move || {
            log_previous_run(&path);
            let _ = tx.send(());
        });
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok(),
            "reading how the last run ended blocked on a pipe: the helper would never serve"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_never_written_through() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("elsewhere");
        std::fs::write(&target, "keep\n").unwrap();
        let p = dir.path().join("lifecycle.log");
        std::os::unix::fs::symlink(&target, &p).unwrap();
        assert!(append(&p, "x").is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep\n");
    }

    #[test]
    fn a_run_that_never_wrote_stopped_is_an_unclean_previous_run() {
        assert!(previous_run_unclean("t=1 starting pid=4 version=1\nt=2 serving pid=4 version=1\n"));
        assert!(previous_run_unclean("t=1 starting pid=4 version=1\n"));
        assert!(!previous_run_unclean(
            "t=1 starting pid=4 version=1\nt=2 serving pid=4 version=1\nt=3 stopped pid=4 version=1\n"
        ));
        assert!(!previous_run_unclean(
            "t=1 starting pid=4 version=1\nt=2 refused:NotProtected pid=4 version=1\n"
        ));
        assert!(!previous_run_unclean(""), "a first run is not a crash");
    }

    #[test]
    fn the_line_has_no_free_text() {
        assert_eq!(line(5, "serving", 3, "1.1.0"), "t=5 serving pid=3 version=1.1.0");
    }
}
