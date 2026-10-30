//! Excel workbooks (OOXML): the sheet the pack names, its shared strings, its cells read sparsely, and merged ranges filled with their
//! top-left value.

use std::collections::BTreeMap;

use super::zipx::Archive;
use super::{ImportLimits, ReqError, malformed, xml};
use crate::pack::MappingPack;
use crate::table::Table;

const FORMAT: &str = "xlsx";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// `B3` to (row 3, column 2), both 1-based.
fn cell_ref(r: &str) -> Option<(usize, usize)> {
    let letters: String = r.chars().take_while(char::is_ascii_alphabetic).collect();
    let digits = &r[letters.len()..];
    if letters.is_empty() || letters.len() > 3 || digits.is_empty() {
        return None;
    }
    let col = letters
        .bytes()
        .fold(0usize, |n, b| n * 26 + usize::from(b.to_ascii_uppercase() - b'A' + 1));
    Some((digits.parse().ok()?, col))
}

fn plain_text(n: roxmltree::Node<'_, '_>) -> String {
    n.descendants()
        .filter(|d| d.has_tag_name("t") && !d.ancestors().any(|a| a.has_tag_name("rPh")))
        .filter_map(|d| d.text())
        .collect()
}

pub(super) fn read(bytes: &[u8], pack: &MappingPack, limits: &ImportLimits) -> Result<Table, ReqError> {
    let mut zip = Archive::open(bytes, FORMAT, limits)?;
    let workbook_text = zip
        .text("xl/workbook.xml", limits)?
        .ok_or_else(|| malformed(FORMAT, "xl/workbook.xml is missing; this is not a workbook"))?;
    let workbook = xml(&workbook_text, "xl/workbook.xml", limits)?;
    let sheets: Vec<(String, String)> = workbook
        .descendants()
        .filter(|n| n.has_tag_name("sheet"))
        .filter_map(|n| {
            Some((
                n.attribute("name")?.to_owned(),
                n.attributes()
                    .find(|a| a.name() == "id" && a.namespace() == Some(REL_NS))?
                    .value()
                    .to_owned(),
            ))
        })
        .collect();
    let chosen = match &pack.table_name {
        Some(want) => sheets.iter().find(|(n, _)| n.eq_ignore_ascii_case(want)),
        None => sheets.first(),
    }
    .ok_or_else(|| ReqError::NoTable {
        what: "sheet",
        name: pack.table_name.clone().unwrap_or_default(),
        have: sheets.iter().map(|(n, _)| format!("{n:?}")).collect::<Vec<_>>().join(", "),
    })?;
    let rels_text = zip
        .text("xl/_rels/workbook.xml.rels", limits)?
        .ok_or_else(|| malformed(FORMAT, "xl/_rels/workbook.xml.rels is missing"))?;
    let rels = xml(&rels_text, "xl/_rels/workbook.xml.rels", limits)?;
    let target = rels
        .descendants()
        .find(|n| n.has_tag_name("Relationship") && n.attribute("Id") == Some(chosen.1.as_str()))
        .and_then(|n| n.attribute("Target"))
        .ok_or_else(|| malformed(FORMAT, format!("sheet {:?} has no file in the package", chosen.0)))?;
    let sheet_path = match target.strip_prefix('/') {
        Some(abs) => abs.to_owned(),
        None => format!("xl/{target}"),
    };
    let shared: Vec<String> = match zip.text("xl/sharedStrings.xml", limits)? {
        Some(text) => xml(&text, "xl/sharedStrings.xml", limits)?
            .descendants()
            .filter(|n| n.has_tag_name("si"))
            .map(plain_text)
            .collect(),
        None => Vec::new(),
    };
    let sheet_text = zip
        .text(&sheet_path, limits)?
        .ok_or_else(|| malformed(FORMAT, format!("{sheet_path} is missing")))?;
    let sheet = xml(&sheet_text, &sheet_path, limits)?;
    let mut cells: BTreeMap<(usize, usize), String> = BTreeMap::new();
    for c in sheet.descendants().filter(|n| n.has_tag_name("c")) {
        let Some((row, col)) = c.attribute("r").and_then(cell_ref) else {
            continue;
        };
        if row > limits.max_rows {
            return Err(ReqError::Limit {
                what: "rows",
                got: row as u64,
                max: limits.max_rows as u64,
            });
        }
        if col > limits.max_cols {
            return Err(ReqError::Limit {
                what: "columns",
                got: col as u64,
                max: limits.max_cols as u64,
            });
        }
        let value = c.children().find(|n| n.has_tag_name("v")).and_then(|v| v.text()).unwrap_or("");
        let text = match c.attribute("t") {
            Some("s") => value
                .trim()
                .parse::<usize>()
                .ok()
                .and_then(|i| shared.get(i))
                .cloned()
                .unwrap_or_default(),
            Some("inlineStr") => c.children().find(|n| n.has_tag_name("is")).map(plain_text).unwrap_or_default(),
            Some("b") => (if value.trim() == "1" { "TRUE" } else { "FALSE" }).to_owned(),
            _ => value.to_owned(),
        };
        if !text.is_empty() {
            cells.insert((row, col), text);
            if cells.len() > limits.max_cells {
                return Err(ReqError::Limit {
                    what: "cells",
                    got: cells.len() as u64,
                    max: limits.max_cells as u64,
                });
            }
        }
    }
    // merged ranges: every cell of the range shows the top-left value
    let mut filled = 0usize;
    for m in sheet.descendants().filter(|n| n.has_tag_name("mergeCell")) {
        let Some((a, b)) = m.attribute("ref").and_then(|r| r.split_once(':')) else {
            continue;
        };
        let (Some((r1, c1)), Some((r2, c2))) = (cell_ref(a), cell_ref(b)) else {
            continue;
        };
        let Some(value) = cells.get(&(r1, c1)).cloned() else { continue };
        let (r2, c2) = (r2.min(limits.max_rows), c2.min(limits.max_cols));
        for r in r1..=r2 {
            for c in c1..=c2 {
                filled += 1;
                if filled > limits.max_cells {
                    return Err(ReqError::Limit {
                        what: "merged cells",
                        got: filled as u64,
                        max: limits.max_cells as u64,
                    });
                }
                cells.entry((r, c)).or_insert_with(|| value.clone());
            }
        }
    }
    let max_row = cells.keys().map(|(r, _)| *r).max().unwrap_or(0);
    let mut rows: Vec<Vec<String>> = vec![Vec::new(); max_row];
    for ((r, c), v) in cells {
        let row = &mut rows[r - 1];
        if row.len() < c {
            row.resize(c, String::new());
        }
        row[c - 1] = v;
    }
    Ok(Table {
        place: chosen.0.clone(),
        rows,
    })
}
