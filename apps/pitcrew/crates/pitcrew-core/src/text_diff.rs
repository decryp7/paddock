//! A line diff for the preview shown before a configuration file is saved (ADR 0011).
//!
//! Pure and bounded: common lines at both ends are trimmed first, the middle is compared with a longest-common-
//! subsequence table limited to [`MAX_MIDDLE`] lines on each side (a bigger change is shown as one removal and one
//! addition), and the result is cut into hunks with [`CONTEXT`] unchanged lines around each change.

use serde::{Deserialize, Serialize};

/// Unchanged lines shown around a change.
pub const CONTEXT: usize = 3;
/// Most lines compared on each side after trimming the common ends.
pub const MAX_MIDDLE: usize = 1500;
/// Most lines one side of the diff may have.
pub const MAX_LINES: usize = 20_000;
/// Most hunk lines returned (the rest is summarised by `truncated`).
pub const MAX_OUTPUT_LINES: usize = 2000;

/// What happened to a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffKind {
    /// In both.
    Same,
    /// Only in the new text.
    Added,
    /// Only in the old text.
    Removed,
}

/// One line of a diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    /// What happened to it.
    pub kind: DiffKind,
    /// The line, without its line ending.
    pub text: String,
    /// Its number in the old text (1-based), for `Same` and `Removed`.
    pub old_no: Option<u32>,
    /// Its number in the new text (1-based), for `Same` and `Added`.
    pub new_no: Option<u32>,
}

/// A run of changes with its context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    /// The hunk's lines.
    pub lines: Vec<DiffLine>,
}

/// The difference between two texts.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDiff {
    /// The hunks, in order. Empty when the texts are the same.
    pub hunks: Vec<DiffHunk>,
    /// Lines only in the new text.
    pub added: u32,
    /// Lines only in the old text.
    pub removed: u32,
    /// The output was cut at [`MAX_OUTPUT_LINES`].
    pub truncated: bool,
}

impl TextDiff {
    /// Whether the texts differ.
    pub fn is_changed(&self) -> bool {
        self.added > 0 || self.removed > 0
    }
}

fn lines(s: &str) -> Vec<&str> {
    if s.is_empty() {
        return vec![];
    }
    s.strip_suffix('\n')
        .unwrap_or(s)
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect()
}

/// The difference between `old` and `new`, line by line (line endings are ignored).
pub fn diff(old: &str, new: &str) -> TextDiff {
    let a = lines(old);
    let b = lines(new);
    if a.len() > MAX_LINES || b.len() > MAX_LINES {
        return TextDiff {
            added: u32::try_from(b.len()).unwrap_or(u32::MAX),
            removed: u32::try_from(a.len()).unwrap_or(u32::MAX),
            hunks: vec![],
            truncated: true,
        };
    }
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let ma = &a[prefix..a.len() - suffix];
    let mb = &b[prefix..b.len() - suffix];

    let mut ops: Vec<(DiffKind, &str)> = a[..prefix].iter().map(|l| (DiffKind::Same, *l)).collect();
    ops.extend(middle(ma, mb));
    ops.extend(a[a.len() - suffix..].iter().map(|l| (DiffKind::Same, *l)));

    // Number the lines.
    let mut numbered = Vec::with_capacity(ops.len());
    let (mut o, mut n) = (0u32, 0u32);
    let (mut added, mut removed) = (0u32, 0u32);
    for (kind, text) in ops {
        let (old_no, new_no) = match kind {
            DiffKind::Same => {
                o += 1;
                n += 1;
                (Some(o), Some(n))
            }
            DiffKind::Removed => {
                o += 1;
                removed += 1;
                (Some(o), None)
            }
            DiffKind::Added => {
                n += 1;
                added += 1;
                (None, Some(n))
            }
        };
        numbered.push(DiffLine {
            kind,
            text: text.to_string(),
            old_no,
            new_no,
        });
    }
    if added == 0 && removed == 0 {
        return TextDiff::default();
    }

    // Keep the changes and CONTEXT lines around them.
    let mut keep = vec![false; numbered.len()];
    for (i, l) in numbered.iter().enumerate() {
        if l.kind != DiffKind::Same {
            let from = i.saturating_sub(CONTEXT);
            let to = (i + CONTEXT + 1).min(numbered.len());
            keep[from..to].iter_mut().for_each(|k| *k = true);
        }
    }
    let mut hunks: Vec<DiffHunk> = vec![];
    let mut total = 0usize;
    let mut truncated = false;
    let mut current: Vec<DiffLine> = vec![];
    for (l, k) in numbered.into_iter().zip(keep) {
        if k {
            if total >= MAX_OUTPUT_LINES {
                truncated = true;
                continue;
            }
            total += 1;
            current.push(l);
        } else if !current.is_empty() {
            hunks.push(DiffHunk {
                lines: std::mem::take(&mut current),
            });
        }
    }
    if !current.is_empty() {
        hunks.push(DiffHunk { lines: current });
    }
    TextDiff {
        hunks,
        added,
        removed,
        truncated,
    }
}

fn middle<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<(DiffKind, &'a str)> {
    if a.is_empty() || b.is_empty() || a.len() > MAX_MIDDLE || b.len() > MAX_MIDDLE {
        return a
            .iter()
            .map(|l| (DiffKind::Removed, *l))
            .chain(b.iter().map(|l| (DiffKind::Added, *l)))
            .collect();
    }
    // lcs[i][j]: the longest common subsequence of a[i..] and b[j..].
    let w = b.len() + 1;
    let mut lcs = vec![0u16; (a.len() + 1) * w];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i * w + j] = if a[i] == b[j] {
                lcs[(i + 1) * w + j + 1] + 1
            } else {
                lcs[(i + 1) * w + j].max(lcs[i * w + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = vec![];
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            out.push((DiffKind::Same, a[i]));
            i += 1;
            j += 1;
        } else if lcs[(i + 1) * w + j] >= lcs[i * w + j + 1] {
            out.push((DiffKind::Removed, a[i]));
            i += 1;
        } else {
            out.push((DiffKind::Added, b[j]));
            j += 1;
        }
    }
    out.extend(a[i..].iter().map(|l| (DiffKind::Removed, *l)));
    out.extend(b[j..].iter().map(|l| (DiffKind::Added, *l)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(d: &TextDiff) -> Vec<(DiffKind, &str)> {
        d.hunks
            .iter()
            .flat_map(|h| h.lines.iter().map(|l| (l.kind, l.text.as_str())))
            .collect()
    }

    #[test]
    fn equal_texts_have_no_hunks() {
        assert!(!diff("a\nb\n", "a\nb\n").is_changed());
        assert_eq!(diff("a\r\nb\r\n", "a\nb\n"), TextDiff::default(), "line endings are ignored");
        assert_eq!(diff("", ""), TextDiff::default());
    }

    #[test]
    fn a_changed_line_is_a_removal_and_an_addition_with_context() {
        let d = diff("1\n2\n3\n4\n5\n6\n7\n8\n9\n", "1\n2\n3\n4\nfive\n6\n7\n8\n9\n");
        assert_eq!((d.added, d.removed), (1, 1));
        assert_eq!(d.hunks.len(), 1);
        let k = kinds(&d);
        assert_eq!(k.first(), Some(&(DiffKind::Same, "2")));
        assert!(k.contains(&(DiffKind::Removed, "5")) && k.contains(&(DiffKind::Added, "five")));
        assert_eq!(k.last(), Some(&(DiffKind::Same, "8")));
        let l = &d.hunks[0].lines;
        assert_eq!(l.iter().find(|x| x.text == "5").unwrap().old_no, Some(5));
        assert_eq!(l.iter().find(|x| x.text == "five").unwrap().new_no, Some(5));
    }

    #[test]
    fn distant_changes_make_separate_hunks() {
        let old: String = (1..=30).map(|n| format!("L{n}\n")).collect();
        let new = old.replace("L2\n", "two\n").replace("L29\n", "twenty-nine\n");
        let d = diff(&old, &new);
        assert_eq!(d.hunks.len(), 2);
        assert_eq!((d.added, d.removed), (2, 2));
    }

    #[test]
    fn insertions_and_deletions_in_the_middle() {
        let d = diff("a\nb\nc\n", "a\nx\nb\nc\ny\n");
        assert_eq!((d.added, d.removed), (2, 0));
        let d = diff("a\nb\nc\n", "c\n");
        assert_eq!((d.added, d.removed), (0, 2));
        let d = diff("", "a\n");
        assert_eq!((d.added, d.removed), (1, 0));
        let d = diff("a\nb\nc\nd\n", "d\nc\nb\na\n");
        assert!(d.added > 0 && d.removed > 0);
    }

    #[test]
    fn huge_changes_are_summarised_not_computed() {
        let old: String = (0..MAX_MIDDLE + 5).map(|n| format!("a{n}\n")).collect();
        let new: String = (0..MAX_MIDDLE + 5).map(|n| format!("b{n}\n")).collect();
        let d = diff(&old, &new);
        assert_eq!(d.added as usize, MAX_MIDDLE + 5);
        assert!(d.truncated, "the output is capped");
        let big: String = (0..MAX_LINES + 1).map(|_| "x\n").collect();
        let d = diff(&big, "x\n");
        assert!(d.truncated && d.hunks.is_empty());
    }
}

#[cfg(test)]
mod properties {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn counts_match_and_equal_texts_have_no_diff(a in proptest::collection::vec("[a-c]{0,2}", 0..30), b in proptest::collection::vec("[a-c]{0,2}", 0..30)) {
            let (ta, tb) = (a.join("\n"), b.join("\n"));
            let d = diff(&ta, &tb);
            let (la, lb) = (lines(&ta).len() as i64, lines(&tb).len() as i64);
            prop_assert_eq!(i64::from(d.added) - i64::from(d.removed), lb - la);
            prop_assert!(!diff(&ta, &ta).is_changed());
            // Every kept line number is valid.
            for h in &d.hunks {
                for l in &h.lines {
                    prop_assert!(l.old_no.is_none_or(|n| i64::from(n) <= la) && l.new_no.is_none_or(|n| i64::from(n) <= lb));
                }
            }
        }
    }
}
