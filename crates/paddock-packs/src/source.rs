use crate::PackError;
use paddock_env::AppDirs;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Where user packs are read from: a port, so the loader's behaviour on files that vanish or fail is testable.
pub trait PackSource: Send + Sync {
    /// The names of the candidate pack files (`*.toml`, regular files only), in any order; a missing folder is empty.
    fn list(&self) -> io::Result<Vec<String>>;

    /// Reads at most `max + 1` bytes of the file, so the caller can tell "too large" without reading it all.
    /// A file deleted since [`list`](Self::list) is `ErrorKind::NotFound`.
    fn read(&self, name: &str, max: u64) -> io::Result<Vec<u8>>;
}

/// The user's packs folder inside the app's data folder (`<data>/packs`), created by the caller when it wants one.
pub fn user_packs_dir(dirs: &AppDirs) -> PathBuf {
    dirs.data().join("packs")
}

/// A real folder. Symbolic links, sub-folders, dot-files and editor temp files are not packs.
#[derive(Debug, Clone)]
pub struct DirSource {
    dir: PathBuf,
}

impl DirSource {
    /// A source over `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

fn is_pack_name(name: &str) -> bool {
    !name.starts_with('.')
        && !name.starts_with('~')
        && name.len() > ".toml".len()
        && name.to_ascii_lowercase().ends_with(".toml")
}

impl PackSource for DirSource {
    fn list(&self) -> io::Result<Vec<String>> {
        let rd = match fs::read_dir(&self.dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut out = Vec::new();
        for entry in rd {
            let Ok(entry) = entry else { continue };
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let is_file = entry.file_type().is_ok_and(|t| t.is_file());
            if is_file && is_pack_name(&name) {
                out.push(name);
            }
        }
        Ok(out)
    }

    fn read(&self, name: &str, max: u64) -> io::Result<Vec<u8>> {
        if name.contains(['/', '\\']) || name == ".." {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a file name",
            ));
        }
        let file = fs::File::open(self.dir.join(name))?;
        let mut buf = Vec::new();
        file.take(max.saturating_add(1)).read_to_end(&mut buf)?;
        Ok(buf)
    }
}

pub(crate) fn io_error(name: &str, e: &io::Error) -> PackError {
    PackError::Io(format!("could not read {name}: {}", e.kind()))
}
