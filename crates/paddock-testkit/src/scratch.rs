//! [`Scratch`]: one temporary folder per test with a leftover check.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use tempfile::TempDir;

/// The environment variable that keeps the folder of a failed test for inspection (any non-empty value except `0`).
pub const KEEP_VAR: &str = "PADDOCK_TESTKIT_KEEP";

/// Why a path or folder operation of a [`Scratch`] failed.
#[derive(Debug)]
pub enum ScratchError {
    /// The relative path is absolute, empty or climbs out of the folder (`..`).
    Escapes(String),
    /// The scratch folder could not be made or read.
    Io(io::Error),
}

impl fmt::Display for ScratchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScratchError::Escapes(p) => write!(
                f,
                "{p:?} is not a plain relative path inside the scratch folder"
            ),
            ScratchError::Io(e) => write!(f, "scratch folder: {e}"),
        }
    }
}

impl std::error::Error for ScratchError {}

impl From<io::Error> for ScratchError {
    fn from(e: io::Error) -> Self {
        ScratchError::Io(e)
    }
}

/// What was left in the scratch `tmp` folder when the test ended (relative names, sorted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leftovers(pub Vec<String>);

impl fmt::Display for Leftovers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} item(s) left in the temp folder: {}",
            self.0.len(),
            self.0.join(", ")
        )
    }
}

impl std::error::Error for Leftovers {}

/// A temporary folder for one test, made of three folders: [`work`](Scratch::work) (what the code under test is given),
/// [`home`](Scratch::home) (a stand-in for the user's home) and [`tmp`](Scratch::tmp) (a stand-in for the shared temp
/// folder, see [`crate::EnvLock::hermetic`]). It is deleted on drop. **`tmp` must be empty when the test ends**: if code
/// left something there, dropping the scratch panics with the names (unless the test is already panicking), and
/// [`finish`](Scratch::finish) returns them as an error for a test that wants to assert it.
#[derive(Debug)]
pub struct Scratch {
    dir: Option<TempDir>,
    root: PathBuf,
    checked: bool,
}

impl Scratch {
    /// A new scratch folder.
    ///
    /// # Panics
    /// When the operating system cannot make a temporary folder (the test cannot run).
    pub fn new() -> Scratch {
        Scratch::try_new().expect("cannot make the scratch folder")
    }

    /// A new scratch folder, or why it could not be made.
    pub fn try_new() -> Result<Scratch, ScratchError> {
        Scratch::make(
            tempfile::Builder::new()
                .prefix("paddock-testkit-")
                .tempdir()?,
        )
    }

    /// A new scratch folder under `parent` instead of the system temp folder (for a test of the folder's own group, and for a
    /// scratch on a chosen disk).
    pub fn try_new_in(parent: &Path) -> Result<Scratch, ScratchError> {
        Scratch::make(
            tempfile::Builder::new()
                .prefix("paddock-testkit-")
                .tempdir_in(parent)?,
        )
    }

    fn make(dir: TempDir) -> Result<Scratch, ScratchError> {
        // Canonical, so paths compare equal to what the code under test resolves (macOS /var vs /private/var).
        let root = dir.path().canonicalize()?;
        // A new folder takes its parent's group on macOS (and on Linux under a setgid parent): on a disk whose folders are
        // group `admin` the scratch would be `admin` too, and code that refuses a folder whose group is not the user's own
        // (Dyno's data folder check) fails for a reason that is the machine's, not the code's. The scratch is the caller's
        // egid, whatever the parent is.
        own_group(&root)?;
        for sub in ["work", "home", "tmp"] {
            fs::create_dir(root.join(sub))?;
            own_group(&root.join(sub))?;
        }
        Ok(Scratch {
            dir: Some(dir),
            root,
            checked: false,
        })
    }

    /// The folder to give the code under test.
    pub fn work(&self) -> PathBuf {
        self.root.join("work")
    }

    /// A stand-in for the user's home folder.
    pub fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    /// A stand-in for the shared temp folder; it is checked to be empty at the end.
    pub fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }

    /// `rel` inside [`work`](Scratch::work), or an error when it is absolute, empty or contains `..`.
    pub fn try_path(&self, rel: &str) -> Result<PathBuf, ScratchError> {
        let p = Path::new(rel);
        let plain = !rel.is_empty()
            && p.components()
                .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
        if plain {
            Ok(self.work().join(p))
        } else {
            Err(ScratchError::Escapes(rel.to_string()))
        }
    }

    /// `rel` inside [`work`](Scratch::work).
    ///
    /// # Panics
    /// When `rel` is absolute, empty or contains `..` (a mistake in the test).
    pub fn path(&self, rel: &str) -> PathBuf {
        match self.try_path(rel) {
            Ok(p) => p,
            Err(e) => panic!("{e}"),
        }
    }

    /// Writes a file inside [`work`](Scratch::work), making its folders, and returns its path.
    pub fn write(&self, rel: &str, contents: impl AsRef<[u8]>) -> Result<PathBuf, ScratchError> {
        let p = self.try_path(rel)?;
        fs::create_dir_all(p.parent().unwrap_or(&self.root))?;
        fs::write(&p, contents)?;
        Ok(p)
    }

    /// Reads a file inside [`work`](Scratch::work) as text.
    pub fn read(&self, rel: &str) -> Result<String, ScratchError> {
        Ok(fs::read_to_string(self.try_path(rel)?)?)
    }

    /// Every file and folder under [`work`](Scratch::work) as sorted `/`-separated relative paths (folders end in `/`).
    pub fn listing(&self) -> Result<Vec<String>, ScratchError> {
        list_tree(&self.work())
    }

    /// The leftover check now: `Ok` when `tmp` is empty. After this the drop does not check again.
    pub fn finish(mut self) -> Result<(), Leftovers> {
        self.checked = true;
        let left = self.leftovers();
        if left.0.is_empty() { Ok(()) } else { Err(left) }
    }

    fn leftovers(&self) -> Leftovers {
        // A tmp folder that cannot be read counts as a leftover: the check never passes by accident.
        Leftovers(list_tree(&self.tmp()).unwrap_or_else(|e| vec![format!("(unreadable: {e})")]))
    }
}

impl Default for Scratch {
    fn default() -> Self {
        Scratch::new()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let panicking = std::thread::panicking();
        let left = (!self.checked && !panicking).then(|| self.leftovers());
        let keep =
            panicking && std::env::var_os(KEEP_VAR).is_some_and(|v| !v.is_empty() && v != "0");
        if let Some(dir) = self.dir.take()
            && keep
        {
            // Handing the folder over to the person debugging.
            eprintln!("paddock-testkit: kept {}", dir.keep().display());
        }
        if let Some(left) = left.filter(|l| !l.0.is_empty()) {
            panic!("{left}");
        }
    }
}

/// Puts `path` in the caller's effective group when it is in another (a no-op where there are no groups, and when it already is).
#[cfg(unix)]
fn own_group(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::{MetadataExt, chown};
    // SAFETY: getegid has no arguments and cannot fail.
    let egid = unsafe { libc::getegid() };
    if fs::metadata(path)?.gid() != egid {
        chown(path, None, Some(egid))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn own_group(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn list_tree(root: &Path) -> Result<Vec<String>, ScratchError> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let mut rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            // A symlink is listed, never followed.
            if entry.file_type()?.is_dir() {
                rel.push('/');
                out.push(rel);
                walk(root, &path, out)?;
            } else {
                out.push(rel);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    out.sort();
    Ok(out)
}
