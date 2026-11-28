//! A parser for `git diff -U0` (and `git log -p -U0`) output: per file, the hunks with their added and removed lines.

/// One hunk: the lines it adds and removes.
#[derive(Debug, Default, Clone)]
pub struct Hunk {
    /// Added lines, without the `+`.
    pub added: Vec<String>,
    /// Removed lines, without the `-`.
    pub removed: Vec<String>,
    /// The first line of the hunk on the new side (the line numbers of the revision the diff leads to), and how many it has.
    pub new_start: usize,
    /// The number of lines the hunk has on the new side.
    pub new_len: usize,
}

/// The hunks of one file (the path after the change).
#[derive(Debug, Clone)]
pub struct FileDiff {
    /// The path in the new revision (the old one for a deleted file).
    pub path: String,
    /// The path in the old revision when the file was renamed.
    pub old_path: Option<String>,
    /// The file is gone in the new revision.
    pub deleted: bool,
    /// Its hunks, in order (a pure rename has none).
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    /// The file's path in the old revision: where a reader of that side finds it.
    pub fn old(&self) -> &str {
        self.old_path.as_deref().unwrap_or(&self.path)
    }
}

/// Arguments that make `git diff` output parseable whatever the user's config says; renames are detected so that a moved file
/// is not read as a deletion plus an addition.
pub const DIFF_FLAGS: [&str; 7] = [
    "-U0",
    "--no-color",
    "-M",
    "--no-ext-diff",
    "--no-textconv",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];

/// Lines shorter than four characters once trimmed (blank lines, braces) are never evidence.
pub fn trivial(line: &str) -> bool {
    line.trim().chars().count() < 4
}

fn plain(path: &str) -> Result<String, String> {
    if path.starts_with('"') {
        return Err(format!("git quoted the path {path}; it has a character this check cannot match"));
    }
    Ok(path.trim_end_matches('\t').to_string())
}

/// Parses the diff. A path git had to quote (a quote, backslash or control character in it) is an error: it cannot be matched
/// to a file reliably, so the caller reports "not measured". So is a deletion whose old path could not be read.
pub fn parse(text: &str) -> Result<Vec<FileDiff>, String> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut current: Option<usize> = None;
    let mut old_path: Option<String> = None;
    let mut rename_from: Option<String> = None;
    let mut lines = text.split('\n');
    while let Some(line) = lines.next() {
        if line.starts_with("diff --git ") {
            old_path = None;
            rename_from = None;
            current = None;
        } else if let Some(p) = line.strip_prefix("rename from ") {
            rename_from = Some(plain(p)?);
        } else if let Some(p) = line.strip_prefix("rename to ") {
            files.push(FileDiff {
                path: plain(p)?,
                old_path: rename_from.take(),
                deleted: false,
                hunks: Vec::new(),
            });
            current = Some(files.len() - 1);
        } else if let Some(path) = line.strip_prefix("--- ") {
            old_path = if path == "/dev/null" {
                None
            } else {
                Some(plain(path.strip_prefix("a/").unwrap_or(path))?)
            };
        } else if let Some(path) = line.strip_prefix("+++ ") {
            if path == "/dev/null" {
                // A deleted file: its removed lines belong to the old path.
                let old = old_path
                    .take()
                    .filter(|p| !p.is_empty())
                    .ok_or("a deletion whose old path could not be read")?;
                files.push(FileDiff {
                    path: old,
                    old_path: None,
                    deleted: true,
                    hunks: Vec::new(),
                });
                current = Some(files.len() - 1);
            } else {
                let p = plain(path.strip_prefix("b/").unwrap_or(path))?;
                // A renamed file that also changed already has its entry.
                match files.last() {
                    Some(f) if current.is_some() && f.path == p && f.old_path.is_some() => {}
                    _ => {
                        files.push(FileDiff {
                            path: p,
                            old_path: None,
                            deleted: false,
                            hunks: Vec::new(),
                        });
                        current = Some(files.len() - 1);
                    }
                }
            }
        } else if line.starts_with("@@ ") {
            let (mut old_n, mut new_n, new_start) = counts(line).ok_or_else(|| format!("an unreadable hunk header: {line}"))?;
            let mut hunk = Hunk {
                new_start,
                new_len: new_n,
                ..Hunk::default()
            };
            while old_n > 0 || new_n > 0 {
                let Some(l) = lines.next() else { break };
                if let Some(a) = l.strip_prefix('+') {
                    new_n = new_n.saturating_sub(1);
                    hunk.added.push(a.to_string());
                } else if let Some(r) = l.strip_prefix('-') {
                    old_n = old_n.saturating_sub(1);
                    hunk.removed.push(r.to_string());
                }
            }
            if let Some(i) = current {
                files[i].hunks.push(hunk);
            }
        }
    }
    Ok(files)
}

/// `@@ -a[,b] +c[,d] @@`: the old and new line counts, and the first new line `c`.
fn counts(header: &str) -> Option<(usize, usize, usize)> {
    let mut parts = header.split(' ');
    parts.next()?;
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let len = |s: &str| -> Option<usize> {
        match s.split_once(',') {
            Some((_, n)) => n.parse().ok(),
            None => Some(1),
        }
    };
    let start = new.split(',').next()?.parse().ok()?;
    Some((len(old)?, len(new)?, start))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_content_lines_that_look_like_headers() {
        let d = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -3,2 +3,2 @@\n-+++ not a header\n- b\n++++ nor this\n+ c\n@@ -9 +9,0 @@\n-gone\n";
        let f = parse(d).unwrap();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].hunks.len(), 2);
        assert_eq!(f[0].hunks[0].added, vec!["+++ nor this", " c"]);
        assert_eq!(f[0].hunks[1].removed, vec!["gone"]);
        assert_eq!((f[0].hunks[0].new_start, f[0].hunks[0].new_len), (3, 2));
        assert_eq!((f[0].hunks[1].new_start, f[0].hunks[1].new_len), (9, 0));
    }

    #[test]
    fn deleted_files_and_quoted_paths() {
        let gone = parse("--- a/x\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\n").unwrap();
        assert!(gone[0].deleted && gone[0].path == "x" && gone[0].hunks[0].removed == vec!["x"]);
        assert!(parse("+++ \"b/a\\tb\"\n").is_err());
        // A quoted old path, and a deletion that names no old path, are errors, not an entry with an empty name.
        assert!(parse("--- \"a/we\\\"ird\"\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\n").is_err());
        assert!(parse("+++ /dev/null\n@@ -1 +0,0 @@\n-x\n").is_err());
        assert!(parse("@@ nonsense\n").is_err());
    }

    #[test]
    fn renames_with_and_without_changes() {
        let d = "diff --git a/a b/b\nsimilarity index 100%\nrename from a\nrename to b\ndiff --git a/c b/d\nsimilarity index 90%\nrename from c\nrename to d\nindex 1..2 100644\n--- a/c\n+++ b/d\n@@ -2 +2 @@\n-old\n+new\n";
        let f = parse(d).unwrap();
        assert_eq!(f.len(), 2);
        assert!(f[0].hunks.is_empty() && f[0].old() == "a" && f[0].path == "b");
        assert!(f[1].old() == "c" && f[1].path == "d" && f[1].hunks.len() == 1);
    }

    #[test]
    fn trivial_lines() {
        assert!(trivial("  }  ") && trivial("") && !trivial("let x"));
    }
}
