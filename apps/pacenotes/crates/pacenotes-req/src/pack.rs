//! The mapping pack: which table of a document holds the requirements and which column is which field.
//!
//! ```toml
//! version = 1
//! name = "Acme requirements sheet"
//! [table]
//! name = "Requirements"   # a sheet name (xlsx); for docx and Markdown, `index = 2` (1-based) picks the table; csv has one
//! header_row = 1
//! fill_down = ["Section"] # empty cells take the value above (merged or continued cells)
//! [columns]
//! id = "Req ID"           # a header text (any case) or a column letter ("A")
//! text = "Description"
//! title = "Title"
//! [columns.extra]
//! priority = "Priority"
//! ```

use std::collections::BTreeMap;

use serde::Deserialize;

/// Why a pack was refused. Each says what to fix.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackError {
    /// Not TOML, too big, or an unknown key.
    #[error("the mapping pack cannot be read: {0}")]
    Syntax(String),
    /// A version this program does not read.
    #[error("the mapping pack says version {0}; this program reads version 1")]
    Version(u32),
    /// A field is wrong.
    #[error("mapping pack, {field}: {problem}")]
    Field {
        /// The key.
        field: &'static str,
        /// What to fix.
        problem: String,
    },
}

/// The largest pack read.
pub const MAX_PACK_BYTES: usize = 64 * 1024;
/// The most extra columns.
pub const MAX_EXTRA_COLUMNS: usize = 64;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPack {
    version: u32,
    name: String,
    #[serde(default)]
    table: RawTable,
    columns: RawColumns,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawTable {
    name: Option<String>,
    index: Option<usize>,
    header_row: Option<usize>,
    #[serde(default)]
    fill_down: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawColumns {
    id: String,
    text: String,
    title: Option<String>,
    #[serde(default)]
    extra: BTreeMap<String, String>,
}

/// A validated mapping pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappingPack {
    /// A name for messages.
    pub name: String,
    /// The sheet to read (xlsx).
    pub table_name: Option<String>,
    /// The table to read (docx, Markdown), 1-based.
    pub table_index: usize,
    /// The row that holds the headers, 1-based.
    pub header_row: usize,
    /// Columns whose empty cells take the value above.
    pub fill_down: Vec<String>,
    /// The id column.
    pub id: String,
    /// The text column.
    pub text: String,
    /// The title column.
    pub title: Option<String>,
    /// Other columns by field name.
    pub extra: BTreeMap<String, String>,
}

fn field(field: &'static str, problem: impl Into<String>) -> PackError {
    PackError::Field {
        field,
        problem: problem.into(),
    }
}

fn check(name: &'static str, value: &str) -> Result<(), PackError> {
    let t = value.trim();
    if t.is_empty() || t.len() > 200 || t.chars().any(char::is_control) {
        return Err(field(
            name,
            "name a header text or a column letter (not empty, one line, at most 200 bytes)",
        ));
    }
    Ok(())
}

impl MappingPack {
    /// Reads and checks a pack.
    pub fn from_toml(text: &str) -> Result<MappingPack, PackError> {
        if text.len() > MAX_PACK_BYTES {
            return Err(PackError::Syntax(format!(
                "the file is {} bytes; at most {MAX_PACK_BYTES} are read",
                text.len()
            )));
        }
        let raw: RawPack = toml::from_str(text).map_err(|e| PackError::Syntax(e.message().to_owned()))?;
        if raw.version != 1 {
            return Err(PackError::Version(raw.version));
        }
        if raw.name.trim().is_empty() || raw.name.len() > 200 {
            return Err(field("name", "give the pack a short name"));
        }
        check("columns.id", &raw.columns.id)?;
        check("columns.text", &raw.columns.text)?;
        if let Some(t) = &raw.columns.title {
            check("columns.title", t)?;
        }
        if raw.columns.extra.len() > MAX_EXTRA_COLUMNS {
            return Err(field("columns.extra", format!("more than {MAX_EXTRA_COLUMNS} extra columns")));
        }
        for (k, v) in &raw.columns.extra {
            if k.is_empty() || k.len() > 64 || !k.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
                return Err(field(
                    "columns.extra",
                    format!("{k:?} is not a field name; use lowercase letters, digits and `_`"),
                ));
            }
            check("columns.extra", v)?;
        }
        let header_row = raw.table.header_row.unwrap_or(1);
        if header_row == 0 || header_row > 10_000 {
            return Err(field("table.header_row", "rows count from 1, at most 10000"));
        }
        let table_index = raw.table.index.unwrap_or(1);
        if table_index == 0 || table_index > 10_000 {
            return Err(field("table.index", "tables count from 1"));
        }
        if let Some(n) = &raw.table.name
            && (n.trim().is_empty() || n.len() > 200)
        {
            return Err(field("table.name", "a sheet name, not empty, at most 200 bytes"));
        }
        for f in &raw.table.fill_down {
            check("table.fill_down", f)?;
        }
        Ok(MappingPack {
            name: raw.name,
            table_name: raw.table.name,
            table_index,
            header_row,
            fill_down: raw.table.fill_down,
            id: raw.columns.id,
            text: raw.columns.text,
            title: raw.columns.title,
            extra: raw.columns.extra,
        })
    }
}
