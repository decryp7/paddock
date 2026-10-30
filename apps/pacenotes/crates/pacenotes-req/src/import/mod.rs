//! The importers and what they share: formats, limits, errors and the port.

mod csv_table;
mod docx;
mod markdown;
mod xlsx;
mod zipx;

use crate::model::RequirementSet;
use crate::pack::MappingPack;
use crate::table::{Table, TableError, apply};

/// A document format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Excel workbook.
    Xlsx,
    /// Word document.
    Docx,
    /// Comma-separated values.
    Csv,
    /// Markdown with pipe tables.
    Markdown,
}

impl Format {
    /// The format a file name suggests, by its extension.
    pub fn from_name(name: &str) -> Option<Format> {
        match name.rsplit('.').next()?.to_ascii_lowercase().as_str() {
            "xlsx" | "xlsm" => Some(Format::Xlsx),
            "docx" | "docm" => Some(Format::Docx),
            "csv" => Some(Format::Csv),
            "md" | "markdown" => Some(Format::Markdown),
            _ => None,
        }
    }
}

/// Bounds of one import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportLimits {
    /// Largest document in bytes.
    pub max_bytes: usize,
    /// Most entries in a zip (xlsx, docx).
    pub max_zip_entries: usize,
    /// Largest single zip part, decompressed.
    pub max_part_bytes: u64,
    /// Largest decompressed total of the parts that are read.
    pub max_total_bytes: u64,
    /// Highest decompressed-to-compressed ratio of a part.
    pub max_ratio: u64,
    /// Most rows kept from a table.
    pub max_rows: usize,
    /// Most columns kept from a table.
    pub max_cols: usize,
    /// Most cells in a table.
    pub max_cells: usize,
    /// Most XML nodes in one part.
    pub max_xml_nodes: u32,
}

impl Default for ImportLimits {
    fn default() -> Self {
        ImportLimits {
            max_bytes: 64 * 1024 * 1024,
            max_zip_entries: 2_000,
            max_part_bytes: 64 * 1024 * 1024,
            max_total_bytes: 128 * 1024 * 1024,
            max_ratio: 100,
            max_rows: 100_000,
            max_cols: 256,
            max_cells: 2_000_000,
            max_xml_nodes: 8_000_000,
        }
    }
}

/// Why a document was refused. Each says what to do.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReqError {
    /// A bound was passed.
    #[error("the document passes a limit: {what} ({got} against at most {max}); split the document or raise the limit")]
    Limit {
        /// What was counted.
        what: &'static str,
        /// How many there were (or more).
        got: u64,
        /// The limit.
        max: u64,
    },
    /// A zip that unpacks to far more than it is: refused as it is read.
    #[error("the part {part:?} unpacks to more than {max_ratio} times its stored size; it looks like a zip bomb and was not read")]
    ZipBomb {
        /// The part.
        part: String,
        /// The ratio limit.
        max_ratio: u64,
    },
    /// The document is not that format, or is damaged.
    #[error("not a valid {format} document: {problem}")]
    Malformed {
        /// The format.
        format: &'static str,
        /// What is wrong.
        problem: String,
    },
    /// The XML has a document type definition, which Office files do not need and which can name files or expand without bound.
    #[error(
        "the {part} part has a document type definition (DOCTYPE), which can name outside files; it was refused and nothing was loaded"
    )]
    Dtd {
        /// The part.
        part: String,
    },
    /// The mapping pack does not fit the document.
    #[error(transparent)]
    Mapping(#[from] TableError),
    /// The sheet or table the pack names is not in the document.
    #[error("the document has no {what} {name:?}; it has: {have}")]
    NoTable {
        /// `sheet` or `table`.
        what: &'static str,
        /// What the pack asked for.
        name: String,
        /// What there is.
        have: String,
    },
}

pub(crate) fn malformed(format: &'static str, problem: impl Into<String>) -> ReqError {
    ReqError::Malformed {
        format,
        problem: problem.into(),
    }
}

/// Reads a requirements document with a mapping pack.
pub fn import(bytes: &[u8], format: Format, pack: &MappingPack, limits: &ImportLimits) -> Result<RequirementSet, ReqError> {
    if bytes.len() > limits.max_bytes {
        return Err(ReqError::Limit {
            what: "document size in bytes",
            got: bytes.len() as u64,
            max: limits.max_bytes as u64,
        });
    }
    let table: Table = match format {
        Format::Csv => csv_table::read(bytes, limits)?,
        Format::Markdown => markdown::read(bytes, pack, limits)?,
        Format::Xlsx => xlsx::read(bytes, pack, limits)?,
        Format::Docx => docx::read(bytes, pack, limits)?,
    };
    Ok(apply(&table, pack)?)
}

/// Where a requirements document comes from. An adapter reads the file the user chose; a test gives bytes.
pub trait RequirementSource: Send + Sync {
    /// The document's bytes and its format.
    fn fetch(&self) -> Result<(Vec<u8>, Format), ReqError>;
}

/// Reads whatever a source gives.
pub fn import_from(source: &dyn RequirementSource, pack: &MappingPack, limits: &ImportLimits) -> Result<RequirementSet, ReqError> {
    let (bytes, format) = source.fetch()?;
    import(&bytes, format, pack, limits)
}

/// Parses XML with the DTD off and the node count bounded.
pub(crate) fn xml<'a>(text: &'a str, part: &str, limits: &ImportLimits) -> Result<roxmltree::Document<'a>, ReqError> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: false,
        nodes_limit: limits.max_xml_nodes,
    };
    roxmltree::Document::parse_with_options(text, options).map_err(|e| match e {
        roxmltree::Error::DtdDetected => ReqError::Dtd { part: part.to_owned() },
        roxmltree::Error::NodesLimitReached => ReqError::Limit {
            what: "XML nodes",
            got: u64::from(options.nodes_limit),
            max: u64::from(options.nodes_limit),
        },
        other => malformed("XML", format!("{part}: {other}")),
    })
}
