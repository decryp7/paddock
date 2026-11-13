//! A helper process for tests that must name a program their code under test ends: the test binary itself, re-executed to run
//! [`serve`], which reports its own identity (pid and start time) to a file the moment it runs and then sleeps. The test reads
//! the identity from that file, so it holds the process it means (rule 53) and never a pid it looked up after the process ended.

use paddock_proc::{ProcessId, StartTime};
use std::path::Path;
use std::time::Duration;

/// The variable that names the file the helper writes its identity to.
pub(crate) const FILE_VAR: &str = "PADDOCK_TEST_SLEEPER_FILE";
/// Where the helper test is, for the re-executed `--exact` filter.
const TEST: &str = "test_sleeper::the_sleeper_helper";
/// The helper never outlives this, so a failing test cannot leave it behind (ER-14).
const LIFETIME: Duration = Duration::from_secs(120);

/// The shell words that start the helper in the background of a script, its output discarded (the file is the report).
pub(crate) fn shell_command() -> String {
    let exe = std::env::current_exe().expect("the test binary has a path");
    format!("'{}' --exact {TEST} --nocapture >/dev/null 2>&1", exe.display())
}

/// The identity the helper reported to `file`: `None` until the file is complete.
pub(crate) fn reported(file: &Path) -> Option<ProcessId> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut words = text.split_whitespace();
    let pid = words.next()?.parse::<u32>().ok()?;
    let micros = words.next()?.parse::<u64>().ok()?;
    words.next().is_none().then(|| ProcessId::new(pid, StartTime::from_micros(micros)))
}

/// What the helper does: report, then sleep. Nothing when [`FILE_VAR`] is unset (the test then passes at once).
fn serve() {
    let Some(file) = std::env::var_os(FILE_VAR) else {
        return;
    };
    let me = ProcessId::of(std::process::id()).expect("this process has a start time");
    let file = std::path::PathBuf::from(file);
    let part = file.with_extension("part");
    std::fs::write(&part, format!("{} {}\n", me.pid(), me.started().as_micros())).expect("write the report");
    std::fs::rename(&part, &file).expect("publish the report");
    std::thread::sleep(LIFETIME);
}

#[test]
fn the_sleeper_helper() {
    serve();
}

#[cfg(test)]
mod tests {
    use super::reported;

    #[test]
    fn a_report_is_read_only_when_complete() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("r");
        assert!(reported(&f).is_none(), "no file yet");
        std::fs::write(&f, "12 ").unwrap();
        assert!(reported(&f).is_none(), "half a report");
        std::fs::write(&f, "12 345\n").unwrap();
        let id = reported(&f).unwrap();
        assert_eq!((id.pid(), id.started().as_micros()), (12, 345));
    }
}
