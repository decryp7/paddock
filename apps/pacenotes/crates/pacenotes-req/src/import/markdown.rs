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

fn is_separator(line: &str) -> bool {
    let c = cells(line);
    !c.is_empty()
        && c.iter()
            .all(|s| !s.is_empty() && s.trim_matches(':').bytes().all(|b| b == b'-') && s.contains('-'))
}

pub(super) fn read(bytes: &[u8], pack: &MappingPack, limits: &ImportLimits) -> Result<Table, ReqError> {
    let text = std::str::from_utf8(bytes).map_err(|_| malformed("Markdown", "the file is not UTF-8 text"))?;
    let lines: Vec<&str> = text.lines().collect();
    let mut tables: Vec<Vec<Vec<String>>> = Vec::new();
    let mut i = 0;
    let mut fenced = false;
    while i < lines.len() {
        let line = lines[i];
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            fenced = !fenced;
        }
        if !fenced && line.contains('|') && lines.get(i + 1).is_some_and(|l| is_separator(l)) {
            let mut rows = vec![cells(line)];
            i += 2;
            while i < lines.len() && lines[i].contains('|') && !lines[i].trim().is_empty() {
                rows.push(cells(lines[i]));
                if rows.len() > limits.max_rows {
                    return Err(ReqError::Limit {
                        what: "rows",
                        got: rows.len() as u64,
                        max: limits.max_rows as u64,
                    });
                }
                i += 1;
            }
            tables.push(rows);
            continue;
        }
        i += 1;
    }
    let have = tables.len();
    let rows = tables.into_iter().nth(pack.table_index - 1).ok_or_else(|| ReqError::NoTable {
        what: "table",
        name: pack.table_index.to_string(),
        have: format!("{have} pipe table(s)"),
    })?;
    if rows.iter().any(|r| r.len() > limits.max_cols) {
        return Err(ReqError::Limit {
            what: "columns",
            got: rows.iter().map(Vec::len).max().unwrap_or(0) as u64,
            max: limits.max_cols as u64,
        });
    }
    Ok(Table {
        place: "markdown".to_owned(),
        rows,
    })
}
