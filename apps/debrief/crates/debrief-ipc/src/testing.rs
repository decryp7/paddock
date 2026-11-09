//! Helpers for tests (feature `test-support`): [`scratch_dir`], and on Unix the helpers for tests that kill or abandon a
//! host on purpose.
//!
//! A host that ends without running destructors cannot remove the private endpoint folders it made before its children
//! connected. A test that does this on purpose makes the host (and its children) use a folder of its own, so the folder
//! goes with the test instead of staying in a shared one.

#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;

thread_local! {
    static KEPT: std::cell::RefCell<Vec<tempfile::TempDir>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A fresh empty folder named `debrief-<prefix>-...` in the OS temp folder, removed when the calling (test) thread ends,
/// also after a panic (thread-local values are dropped on unwind). The one helper of Debrief's crates for a fixture that
/// hands out a plain path; CI fails on `debrief-*` folders left behind (docs/testing/temp-leaks.md). Test-only: it panics
/// when the folder cannot be made, which fails the test that asked for it.
pub fn scratch_dir(prefix: &str) -> PathBuf {
    let dir = tempfile::Builder::new()
        .prefix(&format!("debrief-{prefix}-"))
        .tempdir()
        .expect("create a scratch folder");
    let path = dir.path().to_path_buf();
    KEPT.with(|k| k.borrow_mut().push(dir));
    path
}

/// A short folder (in the job's socket base, else under `/tmp`) that is the only place a process started with [`Confined::env`] makes endpoint folders.
///
/// It is short because an endpoint's socket path is limited to [`crate::endpoint::MAX_SOCKET_PATH`] bytes: in macOS' long
/// temp folder a path does not fit, and `create_named` would go on to `/tmp`, outside the folder. Removed when dropped.
#[cfg(unix)]
pub struct Confined {
    dir: tempfile::TempDir,
    path: PathBuf,
}

/// Makes a [`Confined`] folder in the job's socket base when there is one (CI's job-tmpdir sets
/// [`crate::endpoint::SOCKET_BASE_VAR`] to a short folder of the job's own), else in `/tmp`. Test-only: it panics (`expect`)
/// when the folder cannot be made or vanishes, which fails the test that asked for it, the right outcome there; product code
/// must not call it.
#[cfg(unix)]
pub fn confined_tmp() -> Confined {
    confined_tmp_in(&confined_base(std::env::var_os(crate::endpoint::SOCKET_BASE_VAR)))
}

/// Where [`confined_tmp`] makes its folder: the socket base when it names a folder, else `/tmp`.
#[cfg(unix)]
pub fn confined_base(socket_base: Option<std::ffi::OsString>) -> PathBuf {
    socket_base
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|d| d.is_dir())
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// A [`Confined`] folder in `base` (which must be short: see [`Confined`]).
#[cfg(unix)]
pub fn confined_tmp_in(base: &Path) -> Confined {
    let dir = tempfile::Builder::new()
        .prefix("dbw")
        .tempdir_in(base)
        .expect("a confined folder in the socket base");
    let path = dir.path().canonicalize().expect("the folder exists");
    Confined { dir, path }
}

#[cfg(unix)]
impl Confined {
    /// The folder, with symlinks resolved (as `create_named` resolves its bases).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `TMPDIR` and `XDG_RUNTIME_DIR`, the two places an endpoint is tried before `/tmp`, both set to the folder.
    pub fn env(&self) -> Vec<(String, String)> {
        let p = self.path.display().to_string();
        vec![("TMPDIR".into(), p.clone()), ("XDG_RUNTIME_DIR".into(), p)]
    }

    /// Removes the folder now and returns where it was.
    pub fn remove(self) -> PathBuf {
        let path = self.dir.path().to_path_buf();
        drop(self.dir);
        path
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// CI (2026-10-29): confined folders went straight to the shared `/tmp`; a job's own socket folder comes first.
    #[test]
    fn a_confined_folder_goes_in_the_job_socket_base_when_there_is_one() {
        let base = tempfile::tempdir().unwrap();
        assert_eq!(confined_base(Some(base.path().as_os_str().to_owned())), base.path());
        assert_eq!(confined_base(None), Path::new("/tmp"));
        assert_eq!(confined_base(Some("".into())), Path::new("/tmp"));
        assert_eq!(confined_base(Some(base.path().join("gone").into_os_string())), Path::new("/tmp"));
        let c = confined_tmp_in(base.path());
        assert!(c.path().starts_with(base.path().canonicalize().unwrap()));
        let gone = c.remove();
        assert!(!gone.exists());
    }
}
