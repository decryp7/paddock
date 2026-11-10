//! A parser for `git diff -U0` (and `git log -p -U0`) output: per file, the hunks with their added and removed lines.

/// One hunk: the lines it adds and removes.
#[derive(Debug, Default, Clone)]
pub struct Hunk {
    /// Added lines, without the `+`.
    pub added: Vec<String>,
    /// Removed lines, without the `-`.
    pub removed: Vec<String>,
}

/// The hunks of one file (the path after the change).
#[derive(Debug, Clone)]
pub struct FileDiff {
    /// The path in the new revision (the old one for a deleted file).
    pub path: String,
    /// The file is gone in the new revision.
    pub deleted: bool,
    /// Its hunks, in order.
    pub hunks: Vec<Hunk>,
}

/// Arguments that make `git diff` output parseable whatever the user's config says.
pub const DIFF_FLAGS: [&str; 6] = [
    "-U0",
    "--no-color",
    "--no-renames",
    "--no-ext-diff",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];

/// Lines shorter than four characters once trimmed (blank lines, braces) are never evidence.
pub fn trivial(line: &str) -> bool {
    line.trim().chars().count() < 4
}

/// Parses the diff. A path git had to quote (a quote, backslash or control character in it) is an error: it cannot be matched
/// to a file reliably, so the caller reports "not measured".
pub fn parse(text: &str) -> Result<Vec<FileDiff>, String> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut current: Option<usize> = None;
    let mut old_path: Option<String> = None;
    let mut lines = text.split('\n');
    while let Some(line) = lines.next() {
        if let Some(path) = line.strip_prefix("--- ") {
            old_path = path.strip_prefix("a/").map(|p| p.trim_end_matches('\t').to_string());
        } else if let Some(path) = line.strip_prefix("+++ ") {
            if path == "/dev/null" {
                // A deleted file: its removed lines belong to the old path.
                files.push(FileDiff {
                    path: old_path.take().unwrap_or_default(),
                    deleted: true,
                    hunks: Vec::new(),
                });
                current = Some(files.len() - 1);
            } else if path.starts_with('"') {
                return Err(format!("git quoted the path {path}; it has a character this check cannot match"));
            } else {
                let p = path.strip_prefix("b/").unwrap_or(path).trim_end_matches('\t');
                files.push(FileDiff {
                    path: p.to_string(),
                    deleted: false,
                    hunks: Vec::new(),
                });
                current = Some(files.len() - 1);
            }
        } else if line.starts_with("@@ ") {
            let (mut old_n, mut new_n) = counts(line).ok_or_else(|| format!("an unreadable hunk header: {line}"))?;
            let mut hunk = Hunk::default();
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

/// `@@ -a[,b] +c[,d] @@`: the old and new line counts.
fn counts(header: &str) -> Option<(usize, usize)> {
    let mut parts = header.split(' ');
    parts.next()?;
    let one = |s: &str, sign: char| -> Option<usize> {
        let s = s.strip_prefix(sign)?;
        match s.split_once(',') {
            Some((_, n)) => n.parse().ok(),
            None => Some(1),
        }
    };
    Some((one(parts.next()?, '-')?, one(parts.next()?, '+')?))
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
    }

    #[test]
    fn deleted_files_and_quoted_paths() {
        let gone = parse("--- a/x\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\n").unwrap();
        assert!(gone[0].deleted && gone[0].path == "x" && gone[0].hunks[0].removed == vec!["x"]);
        assert!(parse("+++ \"b/a\\tb\"\n").is_err());
        assert!(parse("@@ nonsense\n").is_err());
    }

    #[test]
    fn trivial_lines() {
        assert!(trivial("  }  ") && trivial("") && !trivial("let x"));
    }
}
