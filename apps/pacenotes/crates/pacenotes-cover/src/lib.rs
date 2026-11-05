#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! Pacenotes' coverage readers (package CV1, ADR 0008): five report formats turned into one model of line and branch hits per
//! repository file.
//!
//! A report is untrusted input. Every reader is bounded (bytes, files, lines, XML nodes), never panics, refuses a document type
//! definition's external references (nothing is ever fetched), and says what it left out. The report does not know which revision it
//! measured, so the caller records it, and two runs of different revisions refuse to blend.

pub mod import;
pub mod mapping;
pub mod model;

pub use import::{CoverageSource, Format, ImportContext, ImportError, ImportLimits, ReportSource, import};
pub use mapping::PathMapper;
pub use model::{BlendError, BranchCount, CoverageReport, CoverageSet, FileCoverage, Skipped, Tool};
