//! Markdown pipe tables (GitHub style): a header row, a separator row of dashes, then body rows.

use super::{ImportLimits, ReqError, malformed};
use crate::pack::MappingPack;
use crate::table::Table;

fn cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cur.push('|');
                chars.next();
            }
            '|' => out.push(std::mem::take(&mut cur).trim().to_owned()),
            _ => cur.push(c),
        }
    }
    out.push(cur.trim().to_owned());
    out
}

/// A separator row (`|:--|---|`): only pipes, dashes, colons and spaces, with at least one of each of `|` and `-`. A scan, so a huge line
/// costs nothing but its length.
fn is_separator(line: &str) -> bool {
    line.contains('|') && line.contains('-') && line.bytes().all(|b| matches!(b, b'|' | b'-' | b':' | b' ' | b'\t'))
}

/// The cells of a line, after checking from the count of `|` (a byte scan, nothing allocated) that there are not more columns than the
/// limit allows: 4 MB of `|` would otherwise become four million strings before any bound was looked at.
fn bounded_cells(line: &str, limits: &ImportLimits) -> Result<Vec<String>, ReqError> {
    // the count is exact (the same trimming and escapes as `cells`) and allocates nothing, so a line of 4 MB of pipes is counted, not split
    let n = cell_count(line);
    if n > limits.max_cols {
        return Err(ReqError::Limit {
            what: "columns",
            got: n as u64,
            max: limits.max_cols as u64,
        });
    }
    Ok(cells(line))
}

/// How many cells [`cells`] would return, without making them: a row may have outer pipes or not (`| a | b |` and `a | b` are two cells).
fn cell_count(line: &str) -> usize {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    let mut n = 1;
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                chars.next();
            }
            '|' => n += 1,
            _ => {}
        }
    }
    n
}

pub(super) fn read(bytes: &[u8], pack: &MappingPack, limits: &ImportLimits) -> Result<Table, ReqError> {
    let text = std::str::from_utf8(bytes).map_err(|_| malformed("Markdown", "the file is not UTF-8 text"))?;
    let lines: Vec<&str> = text.lines().collect();
    let wanted = pack.table_index.get();
    let (mut found, mut chosen, mut cell_count) = (0usize, Vec::<Vec<String>>::new(), 0usize);
    let mut i = 0;
    let mut fenced = false;
    while i < lines.len() {
        let line = lines[i];
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            fenced = !fenced;
        }
        let pipes = line.bytes().filter(|b| *b == b'|').count();
        if !fenced && pipes > 0 && lines.get(i + 1).is_some_and(|l| is_separator(l)) {
            found += 1;
            // only the table the pack asks for is collected; the others are walked past
            let keep = found == wanted;
            let mut rows = Vec::new();
            if keep {
                rows.push(bounded_cells(line, limits)?);
            }
            i += 2;
            while i < lines.len() && lines[i].contains('|') && !lines[i].trim().is_empty() {
                if keep {
                    let row = bounded_cells(lines[i], limits)?;
                    cell_count += row.len();
                    if rows.len() >= limits.max_rows {
                        return Err(ReqError::Limit {
                            what: "rows",
                            got: rows.len() as u64 + 1,
                            max: limits.max_rows as u64,
                        });
                    }
                    if cell_count > limits.max_cells {
                        return Err(ReqError::Limit {
                            what: "cells",
                            got: cell_count as u64,
                            max: limits.max_cells as u64,
                        });
                    }
                    rows.push(row);
                }
                i += 1;
            }
            if keep {
                chosen = rows;
            }
            continue;
        }
        i += 1;
    }
    if found < wanted {
        return Err(ReqError::NoTable {
            what: "table",
            name: wanted.to_string(),
            have: format!("{found} pipe table(s)"),
        });
    }
    Ok(Table {
        place: "markdown".to_owned(),
        rows: chosen,
        ..Table::default()
    })
}
