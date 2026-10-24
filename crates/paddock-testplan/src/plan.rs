//! The `.sighterplan` model (ADR 0005). Values that exist have been parsed; [`Plan::validate`] checks what only the whole
//! plan can know (duplicates, references, limits), and [`parse_plan`](crate::parse_plan) runs it.

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};

use crate::ids::{
    CaseId, Column, DataName, PlanId, ReqRef, RequirementId, ScreenName, SecretRef, Tag,
};
use crate::step::{Step, Value};

/// The newest major version this crate reads and writes.
pub const FORMAT_VERSION: u32 = 1;
/// The oldest major it still reads (the last two majors are accepted).
pub const MIN_READABLE_VERSION: u32 = if FORMAT_VERSION > 1 {
    FORMAT_VERSION - 1
} else {
    1
};
/// Largest file in bytes.
pub const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
/// Most cases in a plan.
pub const MAX_CASES: usize = 5_000;
/// Most steps in a case.
pub const MAX_STEPS: usize = 200;
/// Longest string (ids are limited to 64 by their type).
pub const MAX_STRING: usize = 4 * 1024;
/// Largest wait or timeout a step may ask for: one hour.
pub const MAX_WAIT_MS: u32 = 3_600_000;
/// Most requirements, screens, data tables, tags per case and acceptance criteria per requirement.
pub const MAX_ENTRIES: usize = 10_000;
/// Most rows in a data table.
pub const MAX_ROWS: usize = 10_000;
/// Most columns in a data table.
pub const MAX_COLUMNS: usize = 64;

/// The app a plan is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRef {
    /// The app's name.
    pub name: String,
    /// The version the plan was drafted against.
    pub version: Option<String>,
}

/// The tool that drafted the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generator {
    /// Tool name, like `Pacenotes`.
    pub tool: String,
    /// Tool version.
    pub version: Option<String>,
}

/// A link to where a requirement lives (`http` or `https`); never the requirement's file contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLink(pub(crate) String);

impl SourceLink {
    /// Accepts an http(s) URL without whitespace or control characters.
    pub fn new(value: &str) -> Result<Self, crate::PlanError> {
        let ok = (value.starts_with("https://") || value.starts_with("http://"))
            && value.len() <= MAX_STRING
            && !value.chars().any(|c| c.is_whitespace() || c.is_control());
        if ok {
            Ok(Self(value.to_owned()))
        } else {
            Err(crate::PlanError::BadField {
                field: "source".to_owned(),
                msg: "must be an http(s) link without spaces".to_owned(),
            })
        }
    }

    /// The link.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A requirement the plan covers. Its acceptance criteria are `ID#AC-1`, `ID#AC-2`... in list order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    /// The tracker's id.
    pub id: RequirementId,
    /// Short title.
    pub title: String,
    /// The acceptance criteria, in order.
    pub acceptance: Vec<String>,
    /// Where it lives.
    pub source: Option<SourceLink>,
}

/// What a screen should look like when a step asserts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenExpect {
    /// The name steps refer to.
    pub name: ScreenName,
    /// The window title, when it is known.
    pub title: Option<String>,
    /// Texts that must be visible.
    pub contains: Vec<String>,
}

/// How urgent a case is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Priority {
    /// Nice to have.
    Low,
    /// The default.
    #[default]
    Normal,
    /// Important.
    High,
    /// Blocks a release.
    Critical,
}

impl Priority {
    /// The name used in files.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

/// One test case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    /// Unique in the plan.
    pub id: CaseId,
    /// Short title.
    pub title: String,
    /// The requirements and criteria it verifies.
    pub requirements: Vec<ReqRef>,
    /// Urgency.
    pub priority: Priority,
    /// Free tags.
    pub tags: Vec<Tag>,
    /// The data table whose rows it runs over, one iteration per row.
    pub data: Option<DataName>,
    /// The steps.
    pub steps: Vec<Step>,
}

/// One cell of a data table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    /// Plain text.
    Literal(String),
    /// A secret by reference (the cells of a secret column).
    Secret(SecretRef),
}

/// A column of a data table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataColumn {
    /// The name used as `{{data.NAME}}`.
    pub name: Column,
    /// A secret column holds [`Cell::Secret`] references.
    pub secret: bool,
}

/// Rows a case iterates over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataTable {
    /// The table's name.
    pub name: DataName,
    /// Its columns.
    pub columns: Vec<DataColumn>,
    /// Its rows, each as wide as `columns`.
    pub rows: Vec<Vec<Cell>>,
}

/// A parsed, validated test plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Unique id of the plan.
    pub id: PlanId,
    /// Title.
    pub title: String,
    /// The app under test.
    pub app: AppRef,
    /// The drafting tool, when generated.
    pub generated_by: Option<Generator>,
    /// The requirements covered.
    pub requirements: Vec<Requirement>,
    /// The screens steps may assert.
    pub screens: Vec<ScreenExpect>,
    /// The cases.
    pub cases: Vec<Case>,
    /// The data tables.
    pub data: Vec<DataTable>,
}

impl Plan {
    /// The SHA-256 (lowercase hex) of the canonical text: the `plan_hash` of a result, so "result is for an older plan" is exact.
    pub fn hash(&self) -> String {
        hex(&Sha256::digest(self.to_toml().as_bytes()))
    }

    /// Every secret the plan refers to, in order: steps first, then data cells.
    pub fn secret_refs(&self) -> BTreeSet<SecretRef> {
        let mut out = BTreeSet::new();
        for step in self.cases.iter().flat_map(|c| &c.steps) {
            for v in step.values() {
                if let Value::Secret(s) = v {
                    out.insert(s.clone());
                }
            }
        }
        for cell in self.data.iter().flat_map(|t| t.rows.iter().flatten()) {
            if let Cell::Secret(s) = cell {
                out.insert(s.clone());
            }
        }
        out
    }

    /// The case with this id.
    pub fn case(&self, id: &CaseId) -> Option<&Case> {
        self.cases.iter().find(|c| &c.id == id)
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}
