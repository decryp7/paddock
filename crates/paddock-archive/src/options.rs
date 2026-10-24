use std::sync::Arc;

/// What a single archive may do. Defaults suit a runtime download (a few thousand files, a few gigabytes).
#[derive(Debug, Clone)]
pub struct Limits {
    /// Entries in the archive, wanted or not (a bomb of empty files is still a bomb).
    pub max_entries: usize,
    /// Bytes of one file.
    pub max_file_bytes: u64,
    /// Bytes of everything written.
    pub max_total_bytes: u64,
    /// Written bytes may be at most this many times the archive's size (a zip bomb is 1000:1 or more)...
    pub max_ratio: u64,
    /// ...but never limited below this many bytes, so a tiny archive of a few honest files is not refused.
    pub ratio_floor_bytes: u64,
    /// Folders deep.
    pub max_depth: usize,
    /// Characters of a whole path.
    pub max_path_len: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_entries: 10_000,
            max_file_bytes: 4 << 30,
            max_total_bytes: 8 << 30,
            max_ratio: 200,
            ratio_floor_bytes: 64 << 20,
            max_depth: 32,
            max_path_len: 1024,
        }
    }
}

/// Where entries land.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    /// The archive's tree as it is.
    #[default]
    Keep,
    /// Drops this many leading folders (`tar --strip-components`); entries with fewer components are skipped.
    Strip(usize),
    /// Every file lands directly in the folder under its own name; entries deeper than `max_depth` are skipped (the
    /// runtime archives: one leading folder, maybe `bin/`).
    Flat {
        /// Deepest entry that is taken (a file at `a/b/c.dll` is depth 3).
        max_depth: usize,
    },
}

/// What happens to symbolic and hard links.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Links {
    /// The archive is refused.
    #[default]
    Refuse,
    /// Ignored (counted in the report).
    Skip,
    /// Become copies of their target, which must be a file of the archive inside the tree (a target that was not unpacked
    /// drops the link). The same on every OS; no link is ever created on disk.
    Copy,
    /// Created as relative symbolic links whose target has no `..`, so a link can never point out of the tree (Unix; Windows
    /// falls back to [`Links::Copy`]).
    Keep,
}

/// File permissions of what is written (Unix; setuid and write bits for others are never kept).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Modes {
    /// Executable when the archive says so, otherwise a plain file.
    #[default]
    FromArchive,
    /// Every file executable (runtime folders whose files are programs and libraries).
    AllExecutable,
}

/// A name filter: receives the path after the layout (`bin/ffmpeg`) and says whether to unpack it.
pub type Filter = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// How to unpack.
#[derive(Clone, Default)]
pub struct Options {
    /// The limits.
    pub limits: Limits,
    /// Where entries land.
    pub layout: Layout,
    /// What to do with links.
    pub links: Links,
    /// Permissions.
    pub modes: Modes,
    /// Which entries to unpack (all when `None`).
    pub filter: Option<Filter>,
}

impl std::fmt::Debug for Options {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Options")
            .field("limits", &self.limits)
            .field("layout", &self.layout)
            .field("links", &self.links)
            .field("modes", &self.modes)
            .field("filter", &self.filter.as_ref().map(|_| "<fn>"))
            .finish()
    }
}

impl Options {
    /// Only entries `keep` accepts.
    pub fn filter(mut self, keep: impl Fn(&str) -> bool + Send + Sync + 'static) -> Options {
        self.filter = Some(Arc::new(keep));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_print_without_the_closure_and_filter_by_name() {
        let o = Options::default();
        assert!(format!("{o:?}").contains("filter: None"));
        let o = o.filter(|n| n.ends_with(".dll"));
        assert!(format!("{o:?}").contains("<fn>"));
        assert!(o.filter.as_ref().is_some_and(|f| f("a.dll") && !f("a.txt")));
    }
}
