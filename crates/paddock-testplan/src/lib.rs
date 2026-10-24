//! `.sighterplan` and `.sighterresult` (ADR 0005, plan X52): the test-plan formats Pacenotes drafts and Sighter runs.
//!
//! Pure: this crate reads bytes and writes strings, nothing else. Both files are TOML, reviewable in a pull request.
//!
//! - [`parse_plan`] / [`Plan::to_toml`]: bytes to a typed, validated [`Plan] and back to canonical text; [`Plan::hash`] is the
//!   `plan_hash` of results. Limits: 4 MiB, 5,000 cases, 200 steps a case, strings 4 KiB, ids 64. Unknown keys are an error naming
//!   the line; a newer major is refused with "needs Sighter/Pacenotes N or newer"; the last two majors are read.
//! - Typed ids ([`RequirementId`], [`AcceptanceRef`], [`CaseId`], [`ScreenName`], [`PlanId`]...) and a closed [`Step`] enum.
//! - Secrets only by reference ([`SecretRef`], `{{secret:NAME}}`); [`lint`] warns about literals typed into secret-looking
//!   fields; the app implements [`Resolver`], writers take a [`Redactor`] ([`SecretRedactor`] replaces resolved values).
//! - [`parse_result`] / [`RunResult::to_toml`]: results with evidence by relative path and hash, never inline.
//! - [`Coverage::from`]: the requirement x case x last-status matrix; [`diff()`]: what changed between two plans; [`junit::write`].
//!
//! Documents are never executed; nothing here resolves a secret, opens a file or touches the network.

mod coverage;
mod diff;
mod error;
mod ids;
pub mod junit;
mod parse;
mod plan;
mod raw;
mod result;
mod result_check;
mod secret;
mod step;
mod validate;
mod write;

pub use coverage::{AcceptanceRow, CaseCell, Coverage, Matrix, RequirementRow};
pub use diff::{CaseDiff, ChangeKind, PlanDiff, StepChange, diff};
pub use error::PlanError;
pub use ids::{
    AcceptanceRef, CaseId, Column, DataName, MAX_ID_LEN, MAX_REL_PATH, PlanId, RelPath, ReqRef,
    RequirementId, ScreenName, SecretRef, Tag, Timestamp,
};
pub use parse::{parse_plan, parse_result};
pub use plan::{
    AppRef, Case, Cell, DataColumn, DataTable, FORMAT_VERSION, Generator, MAX_CASES, MAX_COLUMNS,
    MAX_ENTRIES, MAX_FILE_BYTES, MAX_ROWS, MAX_STEPS, MAX_STRING, MAX_WAIT_MS,
    MIN_READABLE_VERSION, Plan, Priority, Requirement, ScreenExpect, SourceLink,
};
pub use result::{
    CaseResult, Environment, EvidenceKind, EvidenceRef, MAX_CASE_RESULTS, RunResult, Status,
    StepResult,
};
pub use result_check::validate_result;
pub use secret::{NoRedaction, Redactor, Resolver, SecretRedactor};
pub use step::{ControlClass, Step, Target, Value};
pub use validate::{Lint, MAX_TARGET_DEPTH, lint};
