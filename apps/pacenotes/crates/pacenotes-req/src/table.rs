//! A table of text cells, and the mapping of its rows onto requirements.

use std::collections::BTreeSet;

use crate::model::{Requirement, RequirementSet, SourceRef};
use crate::pack::MappingPack;

/// The longest requirement text kept, in bytes.
pub const MAX_TEXT_BYTES: usize = 16 * 1024;
/// The longest other field kept, in bytes.
pub const MAX_FIELD_BYTES: usize = 1024;

/// A table read from a document: rows of cells (a missing cell is empty), and a name for messages.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Table {
    /// A sheet name, `table 2`, `csv` or `markdown`.
    pub place: String,
    /// The rows, top to bottom.
    pub rows: Vec<Vec<String>>,
    /// Cells whose reference was not a cell (row 0, no column), counted so the figure is not quietly smaller.
    pub bad_cells: u32,
}

/// What a mapping left out, counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Skipped {
    /// Rows with nothing in them.
    pub empty_rows: u32,
    /// Rows with text but no id.
    pub missing_id: u32,
    /// Rows with an id that an earlier row already had (the first is kept).
    pub duplicate_id: u32,
    /// Rows with an id but no text.
    pub missing_text: u32,
    /// Texts and fields cut to their limit.
    pub cut: u32,
    /// Spreadsheet cells with a reference that is not a cell.
    pub bad_cells: u32,
}

fn cut(s: &str, max: usize, skipped: &mut Skipped) -> String {
    let s = s.trim();
    if s.len() <= max {
        return s.to_owned();
    }
    skipped.cut += 1;
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

/// A column letter (`A`, `AB`) to a 0-based index.
fn letter(s: &str) -> Option<usize> {
    let t = s.trim();
    if t.is_empty() || t.len() > 3 || !t.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    Some(
        t.bytes()
            .fold(0usize, |n, b| n * 26 + usize::from(b.to_ascii_uppercase() - b'A' + 1))
            - 1,
    )
}

/// The column a name stands for: `column:C` is the third column, whatever the headers say; anything else is a header text (any case).
/// A column letter has to be written as one, so a header that happens to look like a letter (`ID`) is never taken for a column.
fn column(headers: &[String], name: &str, width: usize) -> Option<usize> {
    if let Some(letters) = name.trim().strip_prefix("column:") {
        return letter(letters).filter(|i| *i < width);
    }
    let want = name.trim().to_lowercase();
    headers.iter().position(|h| h.trim().to_lowercase() == want)
}

/// The mapping failed before any row was read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TableError {
    /// The table is shorter than the header row.
    #[error("the table has {rows} rows and the header row is {header}; check `table.header_row` in the mapping pack")]
    NoHeader {
        /// Rows there are.
        rows: usize,
        /// The pack's header row.
        header: usize,
    },
    /// A mapped column is not in the table.
    #[error("the column {name:?} ({field}) is not in the table; its headers are: {headers}")]
    NoColumn {
        /// The pack's name for the column.
        name: String,
        /// Which field it was for.
        field: String,
        /// The headers found, joined.
        headers: String,
    },
}

/// Maps the rows below the header onto requirements. An empty cell in a `fill_down` column takes the value above it.
pub fn apply(table: &Table, pack: &MappingPack) -> Result<RequirementSet, TableError> {
    let header = table.rows.get(pack.header_row.get() - 1).ok_or(TableError::NoHeader {
        rows: table.rows.len(),
        header: pack.header_row.get(),
    })?;
    let width = table.rows.iter().map(Vec::len).max().unwrap_or(0);
    let find = |name: &str, field: &str| {
        column(header, name, width).ok_or_else(|| TableError::NoColumn {
            name: name.to_owned(),
            field: field.to_owned(),
            headers: header.iter().map(|h| format!("{:?}", h.trim())).collect::<Vec<_>>().join(", "),
        })
    };
    let id_col = find(&pack.id, "id")?;
    let text_col = find(&pack.text, "text")?;
    let title_col = pack.title.as_deref().map(|t| find(t, "title")).transpose()?;
    let extra: Vec<(&str, usize)> = pack
        .extra
        .iter()
        .map(|(k, v)| find(v, k).map(|c| (k.as_str(), c)))
        .collect::<Result<_, _>>()?;
    let fill: Vec<usize> = pack.fill_down.iter().map(|f| find(f, "fill_down")).collect::<Result<_, _>>()?;
    let mut above: Vec<String> = vec![String::new(); fill.len()];
    let mut set = RequirementSet::default();
    set.skipped.bad_cells = table.bad_cells;
    let mut seen = BTreeSet::new();
    for (i, row) in table.rows.iter().enumerate().skip(pack.header_row.get()) {
        let mut row: Vec<String> = row.clone();
        for (k, c) in fill.iter().enumerate() {
            if row.get(*c).is_none_or(|v| v.trim().is_empty()) {
                if row.len() <= *c {
                    row.resize(*c + 1, String::new());
                }
                row[*c] = above[k].clone();
            } else {
                above[k] = row[*c].clone();
            }
        }
        let get = |c: usize| row.get(c).map_or("", String::as_str).trim();
        if row.iter().all(|c| c.trim().is_empty()) {
            set.skipped.empty_rows += 1;
            continue;
        }
        let id = get(id_col);
        let text = get(text_col);
        if id.is_empty() {
            set.skipped.missing_id += 1;
            continue;
        }
        if text.is_empty() {
            set.skipped.missing_text += 1;
            continue;
        }
        // the id is cut first and the cut id is what must be unique: two long ids that differ only past the cut are the same id
        let mut skipped = set.skipped;
        let id = cut(id, MAX_FIELD_BYTES, &mut skipped);
        if !seen.insert(id.clone()) {
            set.skipped.duplicate_id += 1;
            continue;
        }
        let requirement = Requirement {
            id,
            text: cut(text, MAX_TEXT_BYTES, &mut skipped),
            title: title_col
                .map(get)
                .filter(|t| !t.is_empty())
                .map(|t| cut(t, MAX_FIELD_BYTES, &mut skipped)),
            fields: extra
                .iter()
                .filter(|(_, c)| !get(*c).is_empty())
                .map(|(k, c)| ((*k).to_owned(), cut(get(*c), MAX_FIELD_BYTES, &mut skipped)))
                .collect(),
            source: SourceRef {
                place: table.place.clone(),
                row: i + 1,
            },
        };
        set.skipped = skipped;
        set.requirements.push(requirement);
    }
    Ok(set)
}
