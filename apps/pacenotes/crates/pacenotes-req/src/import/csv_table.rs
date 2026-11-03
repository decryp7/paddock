//! CSV.

use super::{ImportLimits, ReqError, malformed};
use crate::table::Table;

pub(super) fn read(bytes: &[u8], limits: &ImportLimits) -> Result<Table, ReqError> {
    let text = std::str::from_utf8(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes))
        .map_err(|_| malformed("CSV", "the file is not UTF-8 text; save it as UTF-8"))?;
    let delimiter = {
        // the delimiter that is most common in the first lines; a comma on a tie
        let head: String = text.lines().take(20).collect::<Vec<_>>().join("\n");
        b"\t;,"
            .iter()
            .copied()
            .max_by_key(|d| head.bytes().filter(|b| b == d).count())
            .unwrap_or(b',')
    };
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .from_reader(text.as_bytes());
    let mut rows = Vec::new();
    let mut cells = 0usize;
    for (i, record) in reader.records().enumerate() {
        let record = record.map_err(|e| malformed("CSV", format!("line {}: {e}", e.position().map_or(i + 1, |p| p.line() as usize))))?;
        if rows.len() >= limits.max_rows {
            return Err(ReqError::Limit {
                what: "rows",
                got: rows.len() as u64 + 1,
                max: limits.max_rows as u64,
            });
        }
        if record.len() > limits.max_cols {
            return Err(ReqError::Limit {
                what: "columns",
                got: record.len() as u64,
                max: limits.max_cols as u64,
            });
        }
        cells += record.len();
        if cells > limits.max_cells {
            return Err(ReqError::Limit {
                what: "cells",
                got: cells as u64,
                max: limits.max_cells as u64,
            });
        }
        rows.push(record.iter().map(str::to_owned).collect());
    }
    Ok(Table {
        place: "csv".to_owned(),
        rows,
        ..Table::default()
    })
}
