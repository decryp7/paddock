//! Removal of a test's scratch folder that cannot hide a leak, shared by the unit tests (`maintain_tests`) and the
//! integration tests (`tests/common`, through the `test-support` feature).
//!
//! CI 7813 left `debrief-duck-diff-*` and `debrief-maintain-reorder-skew-*` on the Mac runner although all 4352 tests
//! passed. Suspected, not reproduced: a removal that failed (or raced a writer) in a `Drop` that ignored the error. The
//! stores' own threads are joined on drop and the fixtures drop the store first, so which writer it was is unknown;
//! tantivy's merge threads, which outlive the index writer, are one candidate. So a failed removal is not retried
//! blindly: it is reported where the gate and a reader can see it.
//!
//! - the message goes straight to the stderr handle with a `::warning::` prefix (libtest captures `eprintln!`);
//! - a marker file `debrief-leak-deferred-<folder>` in the temp folder names the folder, the error and what was left in
//!   it; `scripts/ci/check-temp-leaks.mjs` fails the job on it and prints it, and it stays after the folder is gone;
//! - the folder is removed again when the process exits, so one stuck test does not poison later local runs.

use std::cell::Cell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

thread_local! {
    static STAGED_FAILURE: Cell<bool> = const { Cell::new(false) };
}

/// Makes the next removal on this thread fail without touching the folder (the seam a test uses to stage a busy folder:
/// file modes cannot do it portably, root ignores them).
pub fn fail_next_removal() {
    STAGED_FAILURE.with(|f| f.set(true));
}

static DEFERRED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

extern "C" fn remove_deferred_at_exit() {
    remove_deferred();
}

/// Removes the folders [`remove_or_defer`] could not (what the exit handler does). A marker file stays.
pub fn remove_deferred() {
    let paths = std::mem::take(&mut *DEFERRED.lock().unwrap_or_else(|p| p.into_inner()));
    for path in paths {
        if let Err(e) = std::fs::remove_dir_all(&path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                let _ = writeln!(
                    std::io::stderr(),
                    "::warning::could not remove {} at exit either: {e}",
                    path.display()
                );
            }
        }
    }
}

/// The marker file for `dir`, in `markers`.
pub fn marker_file(dir: &Path, markers: &Path) -> PathBuf {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "folder".into());
    markers.join(format!("debrief-leak-deferred-{}", name.trim_start_matches("debrief-")))
}

/// The files and folders left under `dir`, relative, at most 50, for the marker.
fn left_in(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if out.len() < 50 {
                out.push(p.strip_prefix(dir).unwrap_or(&p).display().to_string());
            }
            if p.is_dir() {
                todo.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Removes `path` (a fixture's folder, when its owner is dropped); a failure is reported and marked (see the module).
pub fn remove_or_defer(path: &Path) {
    remove_or_defer_marking_in(path, &std::env::temp_dir());
}

/// [`remove_or_defer`] with the marker in `markers` (tests).
pub fn remove_or_defer_marking_in(path: &Path, markers: &Path) {
    let staged = STAGED_FAILURE.with(|f| f.replace(false));
    let removed = if staged {
        Err(std::io::Error::other("a removal failure staged by fail_next_removal"))
    } else {
        std::fs::remove_dir_all(path)
    };
    let error = match removed {
        Ok(()) => return,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => e,
    };
    let left = left_in(path);
    let marker = marker_file(path, markers);
    let text = format!(
        "folder: {}\nerror: {error}\npid: {}\nleft ({}{}):\n{}\n",
        path.display(),
        std::process::id(),
        left.len(),
        if left.len() >= 50 { "+" } else { "" },
        left.join("\n")
    );
    let wrote = std::fs::write(&marker, text);
    let _ = writeln!(
        std::io::stderr(),
        "::warning::could not remove {} ({error}); marker {} ({}); removing the folder when the test process exits",
        path.display(),
        marker.display(),
        match &wrote {
            Ok(()) => "written".to_string(),
            Err(e) => format!("not written: {e}"),
        }
    );
    static AT_EXIT: std::sync::Once = std::sync::Once::new();
    AT_EXIT.call_once(|| {
        // SAFETY: registers a plain `extern "C"` function with the C runtime; it runs once, at exit.
        let registered = unsafe { libc::atexit(remove_deferred_at_exit) };
        if registered != 0 {
            let _ = writeln!(
                std::io::stderr(),
                "::warning::atexit failed; a deferred folder stays until the next run"
            );
        }
    });
    DEFERRED.lock().unwrap_or_else(|p| p.into_inner()).push(path.to_path_buf());
}
