//! Word documents (OOXML): the table the pack names. `gridSpan` leaves empty cells to its right; a vertically merged cell that
//! continues takes the value above.

use super::zipx::Archive;
use super::{ImportLimits, ReqError, malformed, xml};
use crate::pack::MappingPack;
use crate::table::Table;

const FORMAT: &str = "docx";

fn cell_text(tc: roxmltree::Node<'_, '_>) -> String {
    let mut paragraphs = Vec::new();
    for p in tc.children().filter(|n| n.has_tag_name("p")) {
        let mut s = String::new();
        for n in p.descendants() {
            if n.has_tag_name("t") {
                s.push_str(n.text().unwrap_or(""));
            } else if n.has_tag_name("tab") {
                s.push('\t');
            } else if n.has_tag_name("br") {
                s.push('\n');
            }
        }
        paragraphs.push(s);
    }
    paragraphs.join("\n")
}

pub(super) fn read(bytes: &[u8], pack: &MappingPack, limits: &ImportLimits) -> Result<Table, ReqError> {
    let mut zip = Archive::open(bytes, FORMAT, limits)?;
    let text = zip
        .text("word/document.xml", limits)?
        .ok_or_else(|| malformed(FORMAT, "word/document.xml is missing; this is not a Word document"))?;
    let doc = xml(&text, "word/document.xml", limits)?;
    let tables: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name("tbl") && !n.ancestors().skip(1).any(|a| a.has_tag_name("tbl")))
        .collect();
    let tbl = tables.get(pack.table_index - 1).ok_or_else(|| ReqError::NoTable {
        what: "table",
        name: pack.table_index.to_string(),
        have: format!("{} table(s)", tables.len()),
    })?;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut cells = 0usize;
    for tr in tbl.children().filter(|n| n.has_tag_name("tr")) {
        if rows.len() >= limits.max_rows {
            return Err(ReqError::Limit {
                what: "rows",
                got: rows.len() as u64 + 1,
                max: limits.max_rows as u64,
            });
        }
        let mut row: Vec<String> = Vec::new();
        for tc in tr.children().filter(|n| n.has_tag_name("tc")) {
            let props = tc.children().find(|n| n.has_tag_name("tcPr"));
            let span = props
                .and_then(|p| p.children().find(|n| n.has_tag_name("gridSpan")))
                .and_then(|g| g.attributes().find(|a| a.name() == "val"))
                .and_then(|a| a.value().parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, limits.max_cols);
            let vmerge = props.and_then(|p| p.children().find(|n| n.has_tag_name("vMerge")));
            let continues = vmerge.is_some_and(|v| v.attributes().find(|a| a.name() == "val").is_none_or(|a| a.value() == "continue"));
            let column = row.len();
            let value = if continues {
                rows.last()
                    .and_then(|above: &Vec<String>| above.get(column))
                    .cloned()
                    .unwrap_or_default()
            } else {
                cell_text(tc)
            };
            row.push(value);
            row.extend(std::iter::repeat_n(String::new(), span - 1));
            if row.len() > limits.max_cols {
                return Err(ReqError::Limit {
                    what: "columns",
                    got: row.len() as u64,
                    max: limits.max_cols as u64,
                });
            }
        }
        cells += row.len();
        if cells > limits.max_cells {
            return Err(ReqError::Limit {
                what: "cells",
                got: cells as u64,
                max: limits.max_cells as u64,
            });
        }
        rows.push(row);
    }
    Ok(Table {
        place: format!("table {}", pack.table_index),
        rows,
    })
}
