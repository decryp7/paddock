//! Fakes for tests of apps that hand off (feature `testing`): nothing here starts a process or reads the disk.

use crate::probe::{FileProbe, Spawner};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

/// A file system of the files you name.
#[derive(Debug, Default)]
pub struct FakeFiles(Mutex<BTreeSet<PathBuf>>);

impl FakeFiles {
    /// A file system with these files.
    pub fn with(files: impl IntoIterator<Item = impl Into<PathBuf>>) -> FakeFiles {
        FakeFiles(Mutex::new(files.into_iter().map(Into::into).collect()))
    }
}

impl FileProbe for FakeFiles {
    fn is_file(&self, path: &Path) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(path)
    }
}

/// One start the fake saw.
pub type Launch = (PathBuf, Vec<OsString>);

/// Records every start instead of making it; can be told to fail.
#[derive(Debug, Default)]
pub struct FakeSpawner {
    launched: Mutex<Vec<Launch>>,
    fail: Mutex<Option<String>>,
}

impl FakeSpawner {
    /// A spawner that works.
    pub fn new() -> FakeSpawner {
        FakeSpawner::default()
    }

    /// Makes every start fail with `why` (`None`: work again).
    pub fn fail_with(&self, why: Option<&str>) {
        *self.fail.lock().unwrap_or_else(PoisonError::into_inner) = why.map(str::to_string);
    }

    /// The starts so far.
    pub fn launched(&self) -> Vec<Launch> {
        self.launched
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Spawner for FakeSpawner {
    fn launch(&self, program: &Path, args: &[OsString]) -> Result<(), String> {
        if let Some(why) = self
            .fail
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        {
            return Err(why);
        }
        self.launched
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((program.to_path_buf(), args.to_vec()));
        Ok(())
    }
}
