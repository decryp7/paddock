//! GFM table checks over Markdown text (what Gitea renders, GFM spec section 4.10). Pure: text in, findings out. A table is a
//! header row, a delimiter row with the same cell count, then body rows up to the first blank line or other block. A row splits at
//! every `|` not escaped as `\|`; backticks do not protect a pipe. The port of `util/md-tables.mjs`.

use crate::check::jsre::{is_js_space, rx, trim, units};
use regex::Regex;
use std::sync::LazyLock;

/// One problem: the 1-based line, what is wrong and the fix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableFinding {
    /// The 1-based line.
    pub line: usize,
    /// What is wrong.
    pub problem: String,
    /// The fix.
    pub fix: String,
}

static DELIM_CELL: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*:?-+:?\s*$"));
static BLOCK_START: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s{0,3}([-*+]\s|\d+[.)]\s|#{1,6}(\s|$)|>|<|(`{3,}|~{3,})|([-*_]\s*){3,}$)"));
static HAS_PIPE: LazyLock<Regex> = LazyLock::new(|| rx(r"(^|[^\\])\|"));
static QUOTE: LazyLock<Regex> = LazyLock::new(|| rx(r"^(\s*>)+"));
static FENCE: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*(`{3,}|~{3,})"));
static ONLY_PIPES: LazyLock<Regex> = LazyLock::new(|| rx(r"^\|+$"));
static LINE_END: LazyLock<Regex> = LazyLock::new(|| rx(r"\r?\n"));

/// Cells of a table row, GFM way: outer pipes optional, `\|` is text.
pub fn split_row(line: &str) -> Vec<String> {
    let mut s = trim(line);
    s = s.strip_prefix('|').unwrap_or(s);
    if s.ends_with('|') && !s.ends_with("\\|") {
        s = &s[..s.len() - 1];
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut cells, mut cur) = (Vec::new(), String::new());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && chars.get(i + 1) == Some(&'|') {
            cur.push_str("\\|");
            i += 1;
        } else if chars[i] == '|' {
            cells.push(std::mem::take(&mut cur));
        } else {
            cur.push(chars[i]);
        }
        i += 1;
    }
    cells.push(cur);
    cells
}

/// True for a delimiter row (`| --- | :-: |`): a pipe, and every cell dashes with optional colons.
pub fn is_delimiter_row(line: &str) -> bool {
    line.contains('|') && split_row(line).iter().all(|c| DELIM_CELL.is_match(c))
}

fn has_pipe(l: &str) -> bool {
    HAS_PIPE.is_match(l)
}

fn strip(l: &str) -> String {
    QUOTE.replace(l, "").into_owned()
}

fn trim_end(l: &str) -> &str {
    l.trim_end_matches(is_js_space)
}

struct Table {
    cols: usize,
    trailing: bool,
}

/// Findings for the tables in `text`.
pub fn table_findings(text: &str) -> Vec<TableFinding> {
    let lines: Vec<&str> = LINE_END.split(text).collect();
    let mut out = Vec::new();
    let mut fence: Option<String> = None;
    let mut table: Option<Table> = None;
    let mut add = |line: usize, problem: String, fix: &str| {
        out.push(TableFinding {
            line,
            problem,
            fix: fix.to_string(),
        })
    };
    let mut i = 0;
    while i < lines.len() {
        let l = strip(lines[i]);
        let fm = FENCE.captures(&l).map(|c| c[1].to_string());
        if let Some(f) = &fence {
            if fm
                .as_ref()
                .is_some_and(|m| m.chars().next() == f.chars().next() && units(m) >= units(f))
            {
                fence = None;
            }
            i += 1;
            continue;
        }
        if let Some(m) = fm {
            fence = Some(m);
            table = None;
            i += 1;
            continue;
        }
        let blank = trim(&l).is_empty();
        if let Some(t) = &table {
            if blank || (!has_pipe(&l) && BLOCK_START.is_match(&l)) {
                table = None;
            } else if !has_pipe(&l) {
                add(
                    i + 1,
                    "a plain text line right after a table becomes one more row of it (GFM)".into(),
                    "Put a blank line between the table and the text.",
                );
                table = None;
            } else {
                let n = split_row(&l).len();
                if n != t.cols {
                    add(
                        i + 1,
                        format!(
                            "row has {n} cells, the header has {} (an unescaped | inside a cell, a missing cell or a row split over two lines)",
                            t.cols
                        ),
                        "Escape a literal pipe as \\|, join split rows, and give the row exactly the header's cell count.",
                    );
                }
                let te = trim_end(&l);
                if t.trailing && !(te.ends_with('|') && !te.ends_with("\\|")) {
                    add(
                        i + 1,
                        "row does not end with | where the header does".into(),
                        "Add the closing | (a row that lost it often means a following row was merged into it).",
                    );
                }
            }
            i += 1;
            continue;
        }
        // A table starts at a header line followed by a delimiter row.
        let next = lines.get(i + 1).map(|n| strip(n)).unwrap_or_default();
        if !blank && has_pipe(&l) && is_delimiter_row(&next) {
            let (cols, dcols) = (split_row(&l).len(), split_row(&next).len());
            if cols != dcols {
                add(
                    i + 1,
                    format!("header has {cols} cells but the delimiter row has {dcols}: GFM does not render a table"),
                    "Make the header and delimiter rows the same cell count.",
                );
                i += 2;
                continue;
            }
            table = Some(Table {
                cols,
                trailing: trim_end(&l).ends_with('|'),
            });
            i += 2;
            continue;
        }
        if !blank && is_delimiter_row(&l) {
            add(
                i + 1,
                "delimiter row without a header row above it: not rendered as a table".into(),
                "Add the header row directly above, with no blank line between.",
            );
            i += 1;
            continue;
        }
        // A pipe-bordered line outside a table: a row cut off from its table (usually by a blank line).
        let t = trim(&l);
        if t.starts_with('|') && t.ends_with('|') && !t.ends_with("\\|") && split_row(t).len() >= 2 && !ONLY_PIPES.is_match(t) {
            add(
                i + 1,
                "table row outside a table (a blank line or a broken row above ends the table, so this shows as plain text)".into(),
                "Remove the blank line above, or restore the header and delimiter rows.",
            );
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_good_table_has_no_finding_and_a_short_row_is_one() {
        assert!(table_findings("| a | b |\n|---|---|\n| 1 | 2 |\n").is_empty());
        let f = table_findings("| a | b |\n|---|---|\n| 1 |\n");
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].line, 3);
    }

    #[test]
    fn an_escaped_pipe_is_text_and_a_fence_hides_its_lines() {
        assert!(table_findings("| a | b |\n|---|---|\n| x \\| y | 2 |\n").is_empty());
        assert!(table_findings("```\n| a | b |\n|---|\n```\n").is_empty());
    }

    #[test]
    fn split_row_handles_outer_pipes() {
        assert_eq!(split_row("| a | b \\| c |"), [" a ", " b \\| c "]);
    }
}
