#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! Pacenotes' requirement importers (package RQ1, ADR 0009): a requirements document in a spreadsheet, a Word file, a CSV or a Markdown
//! table, turned into [`Requirement`]s by a mapping pack that says which column is which field.
//!
//! The documents are untrusted. A zip is read with bounds on entries, bytes and the compression ratio (a zip bomb is refused as it
//! is read, whatever the archive claims about its sizes); XML is read with the document type definition switched off, so an external
//! entity is refused and nothing is ever fetched; sheets are read sparsely, so a cell at the far corner does not allocate a grid.

pub mod import;
pub mod model;
pub mod pack;
pub mod table;

pub use import::{Format, ImportLimits, ReqError, RequirementSource, import, import_from};
pub use model::{Requirement, RequirementSet, SourceRef};
pub use pack::{MappingPack, PackError};
pub use table::{Skipped, Table, apply};
