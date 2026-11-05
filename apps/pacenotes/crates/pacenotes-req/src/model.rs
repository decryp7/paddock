//! What every importer produces.

use std::collections::BTreeMap;

/// Where in the document a requirement came from, for "open the source" and for error messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRef {
    /// A sheet name, a table number (`table 2`) or `csv` / `markdown`.
    pub place: String,
    /// The 1-based row in that table.
    pub row: usize,
}

/// One requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    /// The requirement's own identifier (`REQ-12`), unique in its set.
    pub id: String,
    /// What is required.
    pub text: String,
    /// A short title, when the document has one.
    pub title: Option<String>,
    /// The other mapped columns (priority, status, owner...), by the field name the pack gave.
    pub fields: BTreeMap<String, String>,
    /// Where it was read.
    pub source: SourceRef,
}

/// The requirements of one document and what was left out, counted.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RequirementSet {
    /// The requirements, in document order.
    pub requirements: Vec<Requirement>,
    /// Rows left out and why.
    pub skipped: crate::table::Skipped,
}
