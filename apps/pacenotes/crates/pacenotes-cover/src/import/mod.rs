//! The readers and what they share: formats, limits, errors, and the port.

mod cobertura;
mod istanbul;
mod jacoco;
mod lcov;
mod llvm;

use pacenotes_core::{CommitHash, RepoPath};

use crate::mapping::PathMapper;
use crate::model::{CoverageReport, Tool};

/// A report format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// LCOV tracefile.
    Lcov,
    /// Cobertura XML.
    Cobertura,
    /// JaCoCo XML.
    JaCoCo,
    /// llvm-cov JSON export.
    LlvmCov,
    /// istanbul `coverage-final.json`.
    Istanbul,
}

impl Format {
    /// Guesses the format from the first bytes of the text; `None` when it looks like none of them.
    pub fn detect(text: &str) -> Option<Format> {
        let head: String = text.chars().take(4096).collect();
        let trimmed = head.trim_start_matches('\u{feff}').trim_start();
        if trimmed.starts_with('<') {
            if head.contains("<coverage") {
                return Some(Format::Cobertura);
            }
            if head.contains("<report") {
                return Some(Format::JaCoCo);
            }
            return None;
        }
        if trimmed.starts_with('{') {
            if head.contains("llvm.coverage.json.export") || head.contains("\"data\"") && head.contains("\"files\"") {
                return Some(Format::LlvmCov);
            }
            if head.contains("statementMap") || head.contains("\"s\"") && head.contains("\"path\"") {
                return Some(Format::Istanbul);
            }
            return None;
        }
        (trimmed.starts_with("TN:") || trimmed.starts_with("SF:")).then_some(Format::Lcov)
    }

    fn tool(self) -> Tool {
        match self {
            Format::Lcov => Tool::Lcov,
            Format::Cobertura => Tool::Cobertura,
            Format::JaCoCo => Tool::JaCoCo,
            Format::LlvmCov => Tool::LlvmCov,
            Format::Istanbul => Tool::Istanbul,
        }
    }
}

/// Bounds of one import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportLimits {
    /// Largest report in bytes.
    pub max_bytes: usize,
    /// Most files in a report.
    pub max_files: usize,
    /// Most instrumented lines in one file.
    pub max_lines_per_file: usize,
    /// Most instrumented lines in the whole report (a small llvm-cov input can expand into millions of lines, so this is checked while
    /// gaps are filled).
    pub max_total_lines: usize,
    /// For llvm-cov: the report may describe at most this many lines per byte of input (plus 100,000), under `max_total_lines`. Lines
    /// between two segments cost no bytes, so a tiny report can describe millions; real reports stay far under 64 per byte.
    pub max_lines_per_input_byte: usize,
    /// Most XML nodes.
    pub max_xml_nodes: u32,
}

impl Default for ImportLimits {
    fn default() -> Self {
        ImportLimits {
            max_bytes: 128 * 1024 * 1024,
            max_files: 200_000,
            max_lines_per_file: 2_000_000,
            max_total_lines: 10_000_000,
            max_lines_per_input_byte: 64,
            max_xml_nodes: 5_000_000,
        }
    }
}

/// Why a report was refused. Each says what to do.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    /// A bound was passed.
    #[error("the report passes a limit: {what} ({got} against at most {max}); split the run or raise the limit")]
    Limit {
        /// What was counted.
        what: &'static str,
        /// How many there were (or more).
        got: usize,
        /// The limit.
        max: usize,
    },
    /// The text is not that format.
    #[error("not a valid {format} report{at}: {problem}")]
    Malformed {
        /// The format.
        format: &'static str,
        /// ` (line N)` or empty.
        at: String,
        /// What is wrong.
        problem: String,
    },
    /// The format could not be told.
    #[error("the report is in none of the formats read here (LCOV, Cobertura, JaCoCo, llvm-cov JSON, istanbul); name the format")]
    UnknownFormat,
}

pub(crate) fn malformed(format: &'static str, line: Option<usize>, problem: impl Into<String>) -> ImportError {
    ImportError::Malformed {
        format,
        at: line.map(|l| format!(" (line {l})")).unwrap_or_default(),
        problem: problem.into(),
    }
}

/// Everything an import needs besides the text.
pub struct ImportContext<'a> {
    /// How the report's file names become repository paths.
    pub mapper: &'a PathMapper,
    /// The revision the run measured (a report does not say).
    pub revision: Option<CommitHash>,
    /// Bounds.
    pub limits: ImportLimits,
    /// Whether a repository file exists. With it, every format picks the candidate path that exists (JaCoCo's source folders, Cobertura's
    /// sources) and a file that exists nowhere is counted in `Skipped::missing_files` and left out; without it the first candidate is used.
    pub exists: Option<&'a dyn Fn(&RepoPath) -> bool>,
    /// The identity of the artefact being read (a digest or a CI run id), recorded on the report so the same one cannot be blended twice.
    pub run_id: Option<String>,
}

impl<'a> ImportContext<'a> {
    /// A context with default limits and no existence check.
    pub fn new(mapper: &'a PathMapper, revision: Option<CommitHash>) -> ImportContext<'a> {
        ImportContext {
            mapper,
            revision,
            limits: ImportLimits::default(),
            exists: None,
            run_id: None,
        }
    }
}

/// Reads a report of a known format.
pub fn import(text: &str, format: Format, cx: &ImportContext<'_>) -> Result<CoverageReport, ImportError> {
    if text.len() > cx.limits.max_bytes {
        return Err(ImportError::Limit {
            what: "report size in bytes",
            got: text.len(),
            max: cx.limits.max_bytes,
        });
    }
    let mut report = CoverageReport::new(format.tool(), cx.revision.clone());
    report.run_id.clone_from(&cx.run_id);
    match format {
        Format::Lcov => lcov::read(text, cx, &mut report)?,
        Format::Cobertura => cobertura::read(text, cx, &mut report)?,
        Format::JaCoCo => jacoco::read(text, cx, &mut report)?,
        Format::LlvmCov => llvm::read(text, cx, &mut report)?,
        Format::Istanbul => istanbul::read(text, cx, &mut report)?,
    }
    Ok(report)
}

/// Where a coverage report comes from. The port is owned here; an adapter reads a file the user chose, a CI artifact, or a test double.
pub trait CoverageSource: Send + Sync {
    /// The report's text and, when the source knows it, its format.
    fn fetch(&self) -> Result<(String, Option<Format>), ImportError>;
}

/// A report already in memory.
pub struct ReportSource {
    /// The text.
    pub text: String,
    /// The format, when known.
    pub format: Option<Format>,
}

impl CoverageSource for ReportSource {
    fn fetch(&self) -> Result<(String, Option<Format>), ImportError> {
        Ok((self.text.clone(), self.format))
    }
}

/// Reads whatever a source gives, detecting the format when the source does not say.
pub fn import_from(source: &dyn CoverageSource, cx: &ImportContext<'_>) -> Result<CoverageReport, ImportError> {
    let (text, format) = source.fetch()?;
    let format = format.or_else(|| Format::detect(&text)).ok_or(ImportError::UnknownFormat)?;
    import(&text, format, cx)
}

impl From<pacenotes_xml::XmlError> for ImportError {
    fn from(e: pacenotes_xml::XmlError) -> ImportError {
        match e {
            pacenotes_xml::XmlError::Depth { max, .. } => ImportError::Limit {
                what: "XML nesting depth",
                got: max + 1,
                max,
            },
            pacenotes_xml::XmlError::Nodes { max, .. } => ImportError::Limit {
                what: "XML nodes",
                got: max as usize,
                max: max as usize,
            },
            other => malformed("XML", None, other.to_string()),
        }
    }
}

/// Picks the repository path of a file among `candidates` (in order of preference) and counts what cannot be placed: none at all is
/// `unmapped_files`; none that exists, when the caller can say what exists, is `missing_files`.
pub(crate) fn resolve(cx: &ImportContext<'_>, candidates: Vec<RepoPath>, report: &mut CoverageReport) -> Option<RepoPath> {
    if candidates.is_empty() {
        report.skipped.unmapped_files = report.skipped.unmapped_files.saturating_add(1);
        return None;
    }
    match cx.exists {
        None => candidates.into_iter().next(),
        Some(exists) => {
            let found = candidates.into_iter().find(|p| exists(p));
            if found.is_none() {
                report.skipped.missing_files = report.skipped.missing_files.saturating_add(1);
            }
            found
        }
    }
}

/// Checks the instrumented lines of the whole report against the limit; `so_far` is what the files read before this one hold.
pub(crate) fn check_total(so_far: usize, in_file: usize, cx: &ImportContext<'_>) -> Result<(), ImportError> {
    check_cap(so_far, in_file, cx.limits.max_total_lines)
}

/// As [`check_total`] against a cap the caller worked out.
pub(crate) fn check_cap(so_far: usize, in_file: usize, cap: usize) -> Result<(), ImportError> {
    let total = so_far.saturating_add(in_file);
    if total > cap {
        return Err(ImportError::Limit {
            what: "instrumented lines in the report",
            got: total,
            max: cap,
        });
    }
    Ok(())
}

/// Checks the file count against the limit.
pub(crate) fn check_files(report: &CoverageReport, cx: &ImportContext<'_>) -> Result<(), ImportError> {
    if report.files.len() > cx.limits.max_files {
        return Err(ImportError::Limit {
            what: "files",
            got: report.files.len(),
            max: cx.limits.max_files,
        });
    }
    Ok(())
}

/// Checks one file's line count against the limit.
pub(crate) fn check_lines(count: usize, cx: &ImportContext<'_>) -> Result<(), ImportError> {
    if count > cx.limits.max_lines_per_file {
        return Err(ImportError::Limit {
            what: "instrumented lines in one file",
            got: count,
            max: cx.limits.max_lines_per_file,
        });
    }
    Ok(())
}

/// A 1-based line number, or `None` (counted by the caller) for 0 and anything past `u32`.
pub(crate) fn line_no(n: u64) -> Option<u32> {
    u32::try_from(n).ok().filter(|l| *l > 0)
}
