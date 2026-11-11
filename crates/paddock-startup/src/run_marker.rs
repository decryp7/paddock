//! "Did the previous run end cleanly?" for a process that has no session to restore (a background agent, a command-line tool):
//! a small marker file holds the running process's id and start time; a clean exit removes it. At the next start a marker that is
//! still there and whose process is gone means the last run died (a crash, a kill, a power cut), and that is said in the log (ER-27).
//! The decision is the one the app lifecycle makes ([`previous_run`]); this is the thin file part for processes without a
//! [`Lifecycle`](crate::Lifecycle).

use crate::lifecycle::{Marker, MarkerState, PreviousRun, Probe, previous_run};
use std::path::{Path, PathBuf};

/// The marker file of `process` in `dir`.
pub fn marker_path(dir: &Path, process: &str) -> PathBuf {
    dir.join(format!("run-{process}.marker"))
}

/// The running process's marker; remove it with [`RunMarker::end`] on a clean exit.
#[derive(Debug)]
pub struct RunMarker {
    path: PathBuf,
}

fn parse(text: &str) -> MarkerState {
    let mut parts = text.split_whitespace();
    match (
        parts.next().and_then(|p| p.parse::<u32>().ok()),
        parts.next(),
    ) {
        (Some(pid), started) if pid != 0 => MarkerState::Found(Marker {
            pid,
            started_at: started.and_then(|s| s.parse().ok()),
        }),
        _ => MarkerState::Unreadable,
    }
}

/// Reads the previous marker, tells the log how the previous run ended and writes this run's marker. Never fails: a folder that
/// cannot be written costs only the marker (said once).
pub fn begin(
    dir: &Path,
    process: &str,
    own_pid: u32,
    probe: Probe<'_>,
) -> (RunMarker, PreviousRun) {
    let path = marker_path(dir, process);
    let state = match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => MarkerState::Missing,
        Err(_) => MarkerState::Unreadable,
    };
    let previous = previous_run(state, own_pid, probe);
    match previous {
        PreviousRun::Clean => log::debug!(Process = process; "Previous run ended cleanly"),
        PreviousRun::Crashed => log::warn!(
            Process = process;
            "Previous run did not end cleanly (no clean-exit mark was left): a crash, a power cut, or a stop that was not answered in time and ended in a kill (its 'stopping' line, if any, is just before this run in the log); look for a crash marker or the OS's crash report"
        ),
        PreviousRun::Unknown => log::warn!(
            Process = process;
            "Whether the previous run of this process ended cleanly is not known: the process table could not be read"
        ),
        PreviousRun::StillRunning => {
            log::info!(Process = process; "Another run of this process is still alive")
        }
    }
    let started = probe(own_pid)
        .started()
        .map(|t| t.to_string())
        .unwrap_or_default();
    if let Err(e) = std::fs::write(&path, format!("{own_pid} {started}\n")) {
        log::warn!(Process = process, Error = e.to_string().as_str(); "The run marker could not be written; the next start cannot tell whether this run ended cleanly");
    }
    (RunMarker { path }, previous)
}

impl RunMarker {
    /// Removes the marker: this run ended cleanly.
    pub fn end(&self) {
        match std::fs::remove_file(&self.path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                log::warn!(Error = e.to_string().as_str(); "The run marker could not be removed; the next start will report an unclean exit");
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::Look;

    /// A marker that cannot be read (here, a folder where the file belongs) is an unreadable one, not a crash of the
    /// start; and one that cannot be removed is reported, not raised.
    #[test]
    fn a_marker_that_cannot_be_read_or_removed_costs_only_its_report() {
        let dir = tempfile::tempdir().unwrap();
        let path = marker_path(dir.path(), "agent");
        std::fs::create_dir_all(path.join("inside")).unwrap();
        let (m, previous) = begin(dir.path(), "agent", 10, &probe_none);
        assert_eq!(
            previous,
            PreviousRun::Crashed,
            "an unreadable marker is an unknown end"
        );
        m.end();
        assert!(path.is_dir(), "the folder in the marker's place stays");
    }

    fn probe_none(_: u32) -> Look {
        Look::Gone
    }

    #[test]
    fn a_marker_left_by_a_dead_process_is_an_unclean_exit_and_a_clean_exit_leaves_none() {
        let dir = tempfile::tempdir().unwrap();
        let (m, first) = begin(dir.path(), "agent", 10, &probe_none);
        assert_eq!(first, PreviousRun::Clean, "nothing before the first run");
        assert!(marker_path(dir.path(), "agent").exists());
        // The run dies: no `end`. The next start finds the marker of a process that is gone.
        drop(m);
        let (m, second) = begin(dir.path(), "agent", 11, &probe_none);
        assert_eq!(second, PreviousRun::Crashed);
        m.end();
        assert!(!marker_path(dir.path(), "agent").exists());
        let (_m, third) = begin(dir.path(), "agent", 12, &probe_none);
        assert_eq!(
            third,
            PreviousRun::Clean,
            "a clean exit leaves nothing to report"
        );
    }

    #[test]
    fn a_live_process_is_not_a_crash_and_garbage_is() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(marker_path(dir.path(), "tool"), "4242 100\n").unwrap();
        let alive = |pid: u32| Look::from((pid == 4242).then_some(100u64));
        assert_eq!(
            begin(dir.path(), "tool", 1, &alive).1,
            PreviousRun::StillRunning
        );
        std::fs::write(marker_path(dir.path(), "tool"), "garbage").unwrap();
        assert_eq!(
            begin(dir.path(), "tool", 1, &probe_none).1,
            PreviousRun::Crashed
        );
        // A folder that cannot be written costs only the marker.
        let missing = dir.path().join("absent");
        let (m, p) = begin(&missing, "tool", 1, &probe_none);
        assert_eq!(p, PreviousRun::Clean);
        m.end();
    }
}
