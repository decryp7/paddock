//! What every reader produces.

use std::collections::BTreeMap;

use pacenotes_core::{CommitHash, RepoPath};

/// Which tool made a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tool {
    /// LCOV `.info` tracefiles (gcov, lcov, many others).
    Lcov,
    /// Cobertura XML.
    Cobertura,
    /// JaCoCo XML.
    JaCoCo,
    /// `llvm-cov export -format=text` JSON.
    LlvmCov,
    /// istanbul's `coverage-final.json`.
    Istanbul,
}

/// Branches on one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BranchCount {
    /// Branches that ran.
    pub taken: u32,
    /// Branches there are.
    pub total: u32,
}

/// Line and branch hits of one repository file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCoverage {
    /// The file.
    pub path: RepoPath,
    /// Hits per instrumented line (1-based). A line that is not in the map is not instrumented.
    pub lines: BTreeMap<u32, u64>,
    /// Branch counts per line.
    pub branches: BTreeMap<u32, BranchCount>,
}

impl FileCoverage {
    /// An empty record.
    pub fn new(path: RepoPath) -> FileCoverage {
        FileCoverage {
            path,
            lines: BTreeMap::new(),
            branches: BTreeMap::new(),
        }
    }

    /// Adds hits to a line (saturating).
    pub fn hit(&mut self, line: u32, hits: u64) {
        let slot = self.lines.entry(line).or_insert(0);
        *slot = slot.saturating_add(hits);
    }

    /// Sets a line's hits to the larger of what it has and `hits` (istanbul lists several statements per line; the line ran if any did).
    pub fn hit_max(&mut self, line: u32, hits: u64) {
        let slot = self.lines.entry(line).or_insert(0);
        *slot = (*slot).max(hits);
    }

    /// Adds branch counts to a line.
    pub fn branch(&mut self, line: u32, taken: u32, total: u32) {
        let slot = self.branches.entry(line).or_default();
        slot.taken = slot.taken.saturating_add(taken);
        slot.total = slot.total.saturating_add(total);
    }

    /// Lines that are instrumented.
    pub fn instrumented(&self) -> usize {
        self.lines.len()
    }

    /// Lines that ran at least once.
    pub fn covered(&self) -> usize {
        self.lines.values().filter(|h| **h > 0).count()
    }

    fn merge(&mut self, other: &FileCoverage) {
        for (l, h) in &other.lines {
            self.hit(*l, *h);
        }
        for (l, b) in &other.branches {
            self.branch(*l, b.taken, b.total);
        }
    }
}

/// What a reader left out, counted, so a number is never quietly smaller than the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Skipped {
    /// Files whose path does not map onto a repository file (outside the repository, or an unusable path).
    pub unmapped_files: u32,
    /// Lines with a number that is not a line (0, or past `u32`).
    pub bad_lines: u32,
    /// Files that map to a repository path that does not exist (when the caller gave an existence check).
    pub missing_files: u32,
    /// llvm-cov segments that were not arrays of the expected shape.
    pub dropped_segments: u32,
    /// istanbul hit counts that were not non-negative integers (read as 0).
    pub non_integer_hits: u32,
}

/// One run's coverage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageReport {
    /// The tool that wrote the report.
    pub tool: Tool,
    /// The revision the run measured, as the caller recorded it (a report does not say).
    pub revision: Option<CommitHash>,
    /// Coverage per file, by path.
    pub files: BTreeMap<RepoPath, FileCoverage>,
    /// What was left out.
    pub skipped: Skipped,
    /// The identity of the artefact this report was read from (a digest or a run id the caller gives), so the same report added twice
    /// to a [`CoverageSet`] is refused instead of doubling its hits.
    pub run_id: Option<String>,
}

impl CoverageReport {
    /// An empty report.
    pub fn new(tool: Tool, revision: Option<CommitHash>) -> CoverageReport {
        CoverageReport {
            tool,
            revision,
            files: BTreeMap::new(),
            skipped: Skipped::default(),
            run_id: None,
        }
    }

    /// The record of `path`, created when it is not there.
    pub fn file_mut(&mut self, path: RepoPath) -> &mut FileCoverage {
        self.files.entry(path.clone()).or_insert_with(|| FileCoverage::new(path))
    }

    /// Instrumented and covered lines over the whole report.
    pub fn totals(&self) -> (usize, usize) {
        self.files
            .values()
            .fold((0, 0), |(i, c), f| (i + f.instrumented(), c + f.covered()))
    }
}

/// Why two runs cannot be blended.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BlendError {
    /// The runs measured different revisions.
    #[error(
        "these runs measured different revisions ({a} and {b}); coverage of different code cannot be added up; run both on one revision"
    )]
    DifferentRevisions {
        /// The set's revision.
        a: CommitHash,
        /// The new run's revision.
        b: CommitHash,
    },
    /// The same run (artefact) was added twice.
    #[error("the run {0:?} was already added; adding it again would double its hits")]
    SameRunTwice(String),
    /// A run does not say which revision it measured.
    #[error("a run without a recorded revision cannot be blended with others; record the revision when importing the report")]
    UnknownRevision,
}

/// Runs of one revision, blended: hits add up per line. Hits are executions where the tool counts them (LCOV, Cobertura, llvm-cov,
/// istanbul) and 0 or 1 where it counts instructions (JaCoCo), so a blended number is a sum of what each tool calls a hit: use it as
/// "covered or not" ([`FileCoverage::covered`]), not as a count of executions. A report with a `run_id` can be added once.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoverageSet {
    revision: Option<CommitHash>,
    tools: Vec<Tool>,
    files: BTreeMap<RepoPath, FileCoverage>,
    runs: usize,
    run_ids: Vec<String>,
}

impl CoverageSet {
    /// An empty set.
    pub fn new() -> CoverageSet {
        CoverageSet::default()
    }

    /// Adds a run. The first run may leave the revision unknown only if it is the only run; any second run, and any run joining a
    /// known revision, must say which revision it measured, and it must be the same.
    pub fn add(&mut self, run: &CoverageReport) -> Result<(), BlendError> {
        if let Some(id) = &run.run_id
            && self.run_ids.contains(id)
        {
            return Err(BlendError::SameRunTwice(id.clone()));
        }
        if self.runs > 0 {
            match (&self.revision, &run.revision) {
                (Some(a), Some(b)) if a == b => {}
                (Some(a), Some(b)) => {
                    return Err(BlendError::DifferentRevisions {
                        a: a.clone(),
                        b: b.clone(),
                    });
                }
                _ => return Err(BlendError::UnknownRevision),
            }
        } else {
            self.revision = run.revision.clone();
        }
        for (path, file) in &run.files {
            self.files
                .entry(path.clone())
                .or_insert_with(|| FileCoverage::new(path.clone()))
                .merge(file);
        }
        if !self.tools.contains(&run.tool) {
            self.tools.push(run.tool);
        }
        self.runs += 1;
        self.run_ids.extend(run.run_id.clone());
        Ok(())
    }

    /// The revision all runs measured.
    pub fn revision(&self) -> Option<&CommitHash> {
        self.revision.as_ref()
    }

    /// The tools that contributed.
    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    /// How many runs were added.
    pub fn runs(&self) -> usize {
        self.runs
    }

    /// The blended coverage per file.
    pub fn files(&self) -> &BTreeMap<RepoPath, FileCoverage> {
        &self.files
    }
}
