//! The `.sighterresult` model (ADR 0005): what a run found, with evidence by reference.

use crate::ids::{CaseId, PlanId, RelPath, ReqRef, ScreenName, Timestamp};

/// Largest number of case results in one file (cases times data-row iterations).
pub const MAX_CASE_RESULTS: usize = 50_000;

/// The outcome of a case or a step. Never a bool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Status {
    /// Everything held.
    Pass,
    /// An assertion or a step failed.
    Fail,
    /// Could not run (a screen never appeared, a secret was missing).
    Blocked,
    /// A step needs a recording before it can be replayed.
    NeedsRecording,
    /// Not run on purpose.
    Skipped,
}

impl Status {
    /// The name used in files.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Blocked => "blocked",
            Self::NeedsRecording => "needs_recording",
            Self::Skipped => "skipped",
        }
    }

    /// Parses a name used in files.
    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Pass,
            Self::Fail,
            Self::Blocked,
            Self::NeedsRecording,
            Self::Skipped,
        ]
        .into_iter()
        .find(|s| s.as_str() == value)
    }

    /// Badness for roll-ups: `Fail` is worst, then `Blocked`, `NeedsRecording`, `Skipped`, `Pass`.
    pub(crate) fn severity(self) -> u8 {
        match self {
            Self::Pass => 0,
            Self::Skipped => 1,
            Self::NeedsRecording => 2,
            Self::Blocked => 3,
            Self::Fail => 4,
        }
    }

    /// The worse of two statuses.
    pub fn worst(self, other: Self) -> Self {
        if other.severity() > self.severity() {
            other
        } else {
            self
        }
    }
}

/// What kind of file a piece of evidence is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvidenceKind {
    /// A screenshot.
    Screenshot,
    /// A screen recording.
    Recording,
    /// Text read from the screen by OCR.
    OcrText,
}

impl EvidenceKind {
    /// The name used in files.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Screenshot => "screenshot",
            Self::Recording => "recording",
            Self::OcrText => "ocr_text",
        }
    }

    /// Parses a name used in files.
    pub fn parse(value: &str) -> Option<Self> {
        [Self::Screenshot, Self::Recording, Self::OcrText]
            .into_iter()
            .find(|k| k.as_str() == value)
    }
}

/// A file next to the result, by relative path and hash. Never inline bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRef {
    /// What it is.
    pub kind: EvidenceKind,
    /// Relative to the result file.
    pub path: RelPath,
    /// SHA-256, lowercase hex.
    pub sha256: String,
}

/// The machine a run happened on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Environment {
    /// Operating system.
    pub os: String,
    /// Display scale in dots per inch.
    pub dpi: Option<u32>,
    /// `light`, `dark` or the app's own name.
    pub theme: Option<String>,
    /// UI language tag.
    pub language: Option<String>,
    /// Screen resolution, like `1920x1080`.
    pub resolution: Option<String>,
    /// The app build under test.
    pub app_build: Option<String>,
    /// The runner's version.
    pub sighter_version: Option<String>,
}

/// The outcome of one step.
#[derive(Debug, Clone, PartialEq)]
pub struct StepResult {
    /// Zero-based position in the case.
    pub index: u32,
    /// The outcome.
    pub status: Status,
    /// The screen the runner recognised.
    pub screen_seen: Option<ScreenName>,
    /// Match score from 0 to 1.
    pub score: Option<f32>,
    /// Evidence of this step.
    pub evidence: Vec<EvidenceRef>,
}

/// The outcome of one case (one iteration for a data-driven case).
#[derive(Debug, Clone, PartialEq)]
pub struct CaseResult {
    /// The case.
    pub case: CaseId,
    /// The requirements it covers, copied from the plan so the result stands alone.
    pub requirements: Vec<ReqRef>,
    /// The outcome.
    pub status: Status,
    /// The data row, zero-based.
    pub iteration: Option<u32>,
    /// How long it took.
    pub duration_ms: u64,
    /// Per-step outcomes.
    pub steps: Vec<StepResult>,
    /// Evidence of the case.
    pub evidence: Vec<EvidenceRef>,
    /// Why, for anything but a pass.
    pub reason: Option<String>,
}

/// A run of a plan.
#[derive(Debug, Clone, PartialEq)]
pub struct RunResult {
    /// The plan that ran.
    pub plan: PlanId,
    /// SHA-256 of the canonical plan, so a changed plan is noticed.
    pub plan_hash: String,
    /// Unique id of the run.
    pub run_id: String,
    /// When it started.
    pub started: Timestamp,
    /// Where it ran.
    pub env: Environment,
    /// The case results.
    pub cases: Vec<CaseResult>,
}

impl RunResult {
    /// Whether the result was made for this exact plan.
    pub fn is_for(&self, plan: &crate::Plan) -> bool {
        self.plan == plan.id && self.plan_hash == plan.hash()
    }
}
