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

/// The mount with the longest mount point that is a prefix of `path`. Both sides are compared by components
/// ([`paddock_env::winpath::starts_with`]: the `\\?\` prefix ignored, names compared ignoring case, no length limit), so a
/// canonical path against the disk list's plain mount points matches however long the path is.
pub fn volume_of<'a>(path: &Path, mounts: &'a [Mount]) -> Option<&'a Mount> {
    volume_of_on(path, mounts, paddock_env::Os::current())
}

/// [`volume_of`] with the platform's path rules given, so every OS tests the Windows ones.
fn volume_of_on<'a>(path: &Path, mounts: &'a [Mount], os: paddock_env::Os) -> Option<&'a Mount> {
    mounts
        .iter()
        .filter(|m| paddock_env::winpath::starts_with(path, &m.path, os))
        .max_by_key(|m| paddock_env::winpath::components(&m.path, os).len())
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

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_env::Os;

    fn mount(path: &str) -> Mount {
        Mount {
            path: PathBuf::from(path),
            free: 1,
            total: 2,
        }
    }

    /// `\\?\C:\` followed by folders of 50 `a`, 300 UTF-16 units in all: past `plain`'s limit, so it keeps its prefix.
    fn long_verbatim() -> String {
        let mut s = String::from(r"\\?\C:\");
        while s.len() < 304 {
            s.push_str(&"a".repeat(50));
            s.push('\\');
        }
        s.truncate(304);
        s
    }

    #[test]
    fn a_verbatim_path_of_300_units_is_on_the_mount_with_the_plain_name() {
        let path = long_verbatim();
        assert!(path.encode_utf16().count() > 300);
        let mounts = [
            mount(r"C:\"),
            mount(r"C:\aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            mount(r"D:\"),
        ];
        let found = volume_of_on(Path::new(&path), &mounts, Os::Windows).expect("the mount of C:");
        assert_eq!(
            found.path,
            Path::new(&mounts[1].path),
            "the longest mount point wins"
        );
        let only_c = [mount(r"c:\"), mount(r"D:\")];
        assert_eq!(
            volume_of_on(Path::new(&path), &only_c, Os::Windows)
                .unwrap()
                .path,
            Path::new(r"c:\")
        );
        assert!(volume_of_on(Path::new(&path), &[mount(r"D:\")], Os::Windows).is_none());
    }
}
