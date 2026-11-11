//! Free space of the volume holding a path (the four copies: Debrief's agent, Pitcrew's probe, Pitwall's system, Dyno's
//! sensors).

use std::path::{Path, PathBuf};

/// One mounted volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    /// Where it is mounted.
    pub path: PathBuf,
    /// Free bytes available to this user.
    pub free: u64,
    /// Size in bytes.
    pub total: u64,
}

/// Free and total bytes of a volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Space {
    /// Free bytes.
    pub free: u64,
    /// Size in bytes.
    pub total: u64,
}

/// The nearest existing ancestor of `path`, canonicalised (an output folder may not exist yet).
pub fn existing_ancestor(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find_map(|p| paddock_env::winpath::real(p).ok())
}

/// `\\?\C:\x` as `C:\x` and `\\?\UNC\server\share\x` as `\\server\share\x` (what `canonicalize` returns on Windows
/// against what the disk list reports); other paths unchanged.
fn plain(path: &Path) -> std::borrow::Cow<'_, Path> {
    paddock_env::winpath::plain(path, paddock_env::Os::current())
}

/// The mount with the longest mount point that is a prefix of `path`.
pub fn volume_of<'a>(path: &Path, mounts: &'a [Mount]) -> Option<&'a Mount> {
    let path = plain(path);
    mounts
        .iter()
        .filter(|m| path.starts_with(plain(&m.path)))
        .max_by_key(|m| m.path.components().count())
}

/// Free and total space of the volume holding `path`, from `mounts`.
pub fn volume_free(path: &Path, mounts: &[Mount]) -> Option<Space> {
    volume_of(path, mounts).map(|m| Space {
        free: m.free.min(m.total),
        total: m.total,
    })
}

/// Free space of the volume holding `path` now (the nearest existing ancestor is used); `None` when unknown. A
/// sampler that asks often caches the mounts instead ([`Sampler::disk_space`](crate::Sampler::disk_space)).
pub fn free_bytes(path: &Path) -> Option<Space> {
    let path = existing_ancestor(path)?;
    volume_free(&path, &crate::native::mounts())
}
