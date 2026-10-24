//! The file shapes as serde sees them: plain strings and numbers, unknown keys refused. [`crate::parse`] turns them into the
//! typed model; nothing outside the crate sees these.

use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct Header {
    pub format: Option<String>,
    pub version: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPlan {
    #[serde(rename = "format")]
    pub _format: String,
    #[serde(rename = "version")]
    pub _version: u32,
    pub id: String,
    pub title: String,
    pub app: RawApp,
    pub generated_by: Option<RawGenerator>,
    #[serde(default)]
    pub requirements: Vec<RawRequirement>,
    #[serde(default)]
    pub screens: Vec<RawScreen>,
    #[serde(default)]
    pub cases: Vec<RawCase>,
    #[serde(default)]
    pub data: Vec<RawData>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawApp {
    pub name: String,
    pub version: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawGenerator {
    pub tool: String,
    pub version: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawRequirement {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub acceptance: Vec<String>,
    pub source: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawScreen {
    pub name: String,
    pub title: Option<String>,
    #[serde(default)]
    pub contains: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawCase {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub requirements: Vec<String>,
    pub priority: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub data: Option<String>,
    #[serde(default)]
    pub steps: Vec<RawStep>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawStep {
    pub action: String,
    pub target: Option<RawTarget>,
    pub value: Option<String>,
    pub keys: Option<String>,
    pub option: Option<String>,
    pub timeout_ms: Option<u32>,
    pub expect_screen: Option<String>,
    pub text: Option<String>,
    pub ms: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawTarget {
    pub text: Option<String>,
    pub name: Option<String>,
    pub class: Option<String>,
    pub inside: Option<String>,
    pub near: Option<Box<RawTarget>>,
    pub nth: Option<u16>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawData {
    pub name: String,
    #[serde(default)]
    pub columns: Vec<RawColumn>,
    #[serde(default)]
    pub rows: Vec<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawColumn {
    pub name: String,
    #[serde(default)]
    pub secret: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawResult {
    #[serde(rename = "format")]
    pub _format: String,
    #[serde(rename = "version")]
    pub _version: u32,
    pub plan: String,
    pub plan_hash: String,
    pub run_id: String,
    pub started: String,
    pub env: RawEnv,
    #[serde(default)]
    pub cases: Vec<RawCaseResult>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawEnv {
    pub os: String,
    pub dpi: Option<u32>,
    pub theme: Option<String>,
    pub language: Option<String>,
    pub resolution: Option<String>,
    pub app_build: Option<String>,
    pub sighter_version: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawCaseResult {
    pub case: String,
    #[serde(default)]
    pub requirements: Vec<String>,
    pub status: String,
    pub iteration: Option<u32>,
    pub duration_ms: u64,
    #[serde(default)]
    pub steps: Vec<RawStepResult>,
    #[serde(default)]
    pub evidence: Vec<RawEvidence>,
    pub reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawStepResult {
    pub index: u32,
    pub status: String,
    pub screen_seen: Option<String>,
    pub score: Option<f64>,
    #[serde(default)]
    pub evidence: Vec<RawEvidence>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawEvidence {
    pub kind: String,
    pub path: String,
    pub sha256: String,
}
