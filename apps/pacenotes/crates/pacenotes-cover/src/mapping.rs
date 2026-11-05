//! Report paths onto repository paths.

use pacenotes_core::RepoPath;

/// How a report's file names become repository paths: the build machine's absolute prefixes are stripped, separators are made `/`, and
/// anything that is still absolute or leads out of the repository is unmapped (counted, never guessed).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathMapper {
    prefixes: Vec<String>,
    source_roots: Vec<String>,
}

fn normal(s: &str) -> String {
    let mut out = s.replace('\\', "/");
    while let Some(rest) = out.strip_prefix("./") {
        out = rest.to_owned();
    }
    out
}

fn is_absolute(s: &str) -> bool {
    s.starts_with('/') || s.as_bytes().get(1) == Some(&b':') && s.as_bytes()[0].is_ascii_alphabetic()
}

impl PathMapper {
    /// A mapper that strips nothing (reports whose paths are already relative to the repository).
    pub fn new() -> PathMapper {
        PathMapper::default()
    }

    /// Strips this absolute prefix (the checkout folder of the machine that ran the tests), in either separator style.
    pub fn strip_prefix(mut self, prefix: &str) -> PathMapper {
        let mut p = normal(prefix);
        if !p.ends_with('/') {
            p.push('/');
        }
        self.prefixes.push(p);
        self
    }

    /// A source folder to put in front of a JaCoCo package path (JaCoCo reports `com/acme/A.java`, not where it lives).
    pub fn source_root(mut self, root: &str) -> PathMapper {
        let mut p = normal(root);
        while p.ends_with('/') {
            p.pop();
        }
        self.source_roots.push(p);
        self
    }

    /// The configured source folders.
    pub fn source_roots(&self) -> &[String] {
        &self.source_roots
    }

    /// The longest configured prefix the name starts with, so `/work/repo/app` beats `/work/repo` whatever order they were given in.
    fn longest(&self, n: &str, lower: &str) -> Option<&String> {
        self.prefixes
            .iter()
            .filter(|p| n.starts_with(p.as_str()) || (p.as_bytes().get(1) == Some(&b':') && lower.starts_with(&p.to_ascii_lowercase())))
            .max_by_key(|p| p.len())
    }

    /// The repository path of a file name from a report, or `None`.
    pub fn map(&self, name: &str) -> Option<RepoPath> {
        let n = normal(name);
        let lower = n.to_ascii_lowercase();
        let stripped = self.longest(&n, &lower).map_or(n.as_str(), |p| &n[p.len()..]);
        if is_absolute(stripped) {
            return None;
        }
        RepoPath::parse(stripped).ok()
    }

    /// The repository folder of a directory name (a Cobertura `<source>`) as a prefix ending in `/` (empty for the root itself), or
    /// `None` when it lies outside the repository.
    pub fn map_dir(&self, name: &str) -> Option<String> {
        let mut n = normal(name);
        if !n.ends_with('/') {
            n.push('/');
        }
        let lower = n.to_ascii_lowercase();
        let stripped = self
            .longest(&n, &lower)
            .map(|p| n[p.len()..].to_owned())
            .or_else(|| (!is_absolute(&n)).then(|| n.clone()))?;
        (!is_absolute(&stripped) && !stripped.split('/').any(|s| s == "..")).then_some(stripped)
    }
}
