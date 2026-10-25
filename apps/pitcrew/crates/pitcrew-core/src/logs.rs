//! Runner logs: a bounded in-memory tail for the log view, search over it, and the rotation plan for log files.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Most lines one search returns.
pub const MAX_SEARCH_RESULTS: usize = 1000;
/// Longest search query.
pub const MAX_QUERY_LEN: usize = 256;

/// One kept line with its sequence number (so the UI can page and follow without duplicates).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    /// Increasing number of the line since the service started.
    pub seq: u64,
    /// The text (cut at [`crate::logline::MAX_LINE_LEN`]).
    pub text: String,
}

/// The newest lines of a runner's log, bounded by line count and total bytes.
#[derive(Debug, Clone)]
pub struct LogTail {
    max_lines: usize,
    max_bytes: usize,
    bytes: usize,
    next_seq: u64,
    lines: VecDeque<LogLine>,
}

impl LogTail {
    /// An empty tail keeping at most `max_lines` lines and `max_bytes` bytes of text.
    pub fn new(max_lines: usize, max_bytes: usize) -> Self {
        LogTail {
            max_lines: max_lines.max(1),
            max_bytes: max_bytes.max(1),
            bytes: 0,
            next_seq: 0,
            lines: VecDeque::new(),
        }
    }

    /// Adds a line, dropping the oldest ones beyond the bounds.
    pub fn push(&mut self, text: &str) {
        let mut end = text.len().min(crate::logline::MAX_LINE_LEN).min(self.max_bytes);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let text = text[..end].to_string();
        self.bytes += text.len();
        self.lines.push_back(LogLine { seq: self.next_seq, text });
        self.next_seq += 1;
        while self.lines.len() > self.max_lines || self.bytes > self.max_bytes {
            match self.lines.pop_front() {
                Some(old) => self.bytes -= old.text.len(),
                None => break,
            }
        }
    }

    /// Lines after `after` (all when `None`), at most `limit`.
    pub fn since(&self, after: Option<u64>, limit: usize) -> Vec<LogLine> {
        self.lines
            .iter()
            .filter(|l| after.is_none_or(|a| l.seq > a))
            .take(limit)
            .cloned()
            .collect()
    }

    /// Lines containing `query` (case-insensitive), newest last, at most `limit` (capped at [`MAX_SEARCH_RESULTS`]).
    /// The query is plain text, never a pattern: nothing the user types can make a search expensive.
    pub fn search(&self, query: &str, limit: usize) -> Vec<LogLine> {
        let q: String = query.chars().take(MAX_QUERY_LEN).collect::<String>().to_lowercase();
        let limit = limit.min(MAX_SEARCH_RESULTS);
        let mut hits: Vec<LogLine> = self
            .lines
            .iter()
            .rev()
            .filter(|l| l.text.to_lowercase().contains(&q))
            .take(limit)
            .cloned()
            .collect();
        hits.reverse();
        hits
    }

    /// Number of kept lines.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether no line is kept.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// When and how log files are rotated (`runner.log` → `runner.log.1` … `runner.log.<keep>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RotationPolicy {
    /// Rotate when the file is larger than this.
    pub max_bytes: u64,
    /// Rotated files to keep.
    pub keep: u32,
}

impl Default for RotationPolicy {
    /// 10 MB and five old files, as the PowerShell supervisor did.
    fn default() -> Self {
        RotationPolicy {
            max_bytes: 10 * 1024 * 1024,
            keep: 5,
        }
    }
}

/// One step of a rotation, carried out in order by the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RotationStep {
    /// Delete `<base>.<n>`.
    Delete(u32),
    /// Rename `<base>.<from>` to `<base>.<to>` (0 is the live file itself).
    Rename {
        /// The source suffix (0 = the live file).
        from: u32,
        /// The target suffix.
        to: u32,
    },
}

impl RotationPolicy {
    /// The steps to rotate a file of `size` bytes, or none when it is small enough.
    pub fn plan(&self, size: u64) -> Vec<RotationStep> {
        if size <= self.max_bytes {
            return vec![];
        }
        if self.keep == 0 {
            return vec![RotationStep::Delete(0)];
        }
        let mut steps = vec![RotationStep::Delete(self.keep)];
        for n in (0..self.keep).rev() {
            steps.push(RotationStep::Rename { from: n, to: n + 1 });
        }
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tail_is_bounded_by_lines_and_bytes() {
        let mut t = LogTail::new(3, 1000);
        for i in 0..5 {
            t.push(&format!("line {i}"));
        }
        assert_eq!(t.len(), 3);
        assert_eq!(t.since(None, 10).iter().map(|l| l.seq).collect::<Vec<_>>(), vec![2, 3, 4]);
        assert_eq!(t.since(Some(3), 10).len(), 1);

        let mut b = LogTail::new(100, 10);
        b.push("12345");
        b.push("67890");
        b.push("x");
        assert_eq!(
            b.since(None, 10).iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            vec!["67890", "x"]
        );
        // A line larger than the whole budget is cut, never kept whole.
        b.push(&"é".repeat(20));
        assert!(b.since(None, 10).iter().all(|l| l.text.len() <= 10));
        // A budget that falls inside a character cuts before it.
        let mut odd = LogTail::new(100, 9);
        odd.push(&"é".repeat(20));
        assert_eq!(odd.since(None, 1)[0].text, "é".repeat(4));
        assert!(!b.is_empty());
    }

    #[test]
    fn search_is_plain_case_insensitive_text_and_bounded() {
        let mut t = LogTail::new(100, 100_000);
        t.push(r#"level=error msg="failed to fetch task""#);
        t.push("level=info msg=\"task started\"");
        t.push("Fetch again");
        let hits = t.search("FETCH", 10);
        assert_eq!(hits.iter().map(|l| l.seq).collect::<Vec<_>>(), vec![0, 2]);
        assert_eq!(t.search("fetch", 1)[0].seq, 2, "the newest hits win");
        assert!(t.search(".*", 10).is_empty(), "no patterns");
        assert_eq!(t.search(&"x".repeat(10_000), 10).len(), 0);
    }

    #[test]
    fn rotation_shifts_old_files_and_drops_the_oldest() {
        let p = RotationPolicy { max_bytes: 100, keep: 2 };
        assert!(p.plan(100).is_empty());
        assert_eq!(
            p.plan(101),
            vec![
                RotationStep::Delete(2),
                RotationStep::Rename { from: 1, to: 2 },
                RotationStep::Rename { from: 0, to: 1 }
            ]
        );
        assert_eq!(RotationPolicy { max_bytes: 0, keep: 0 }.plan(1), vec![RotationStep::Delete(0)]);
    }
}
