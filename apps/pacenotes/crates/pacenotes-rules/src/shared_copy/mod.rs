//! The shared half of the joint rules model, kept here until C3a moves it to `paddock-rules` (note section 3.1). Types the core
//! already owns (`Severity`, `FindingState`, `Confidence`, `Position`, `Span`, `Location`, `RepoPath`) are used from `pacenotes-core`.
//!
//! TODO(C3a, paddock-rules): move this module, then delete it. `tests/it/shared_copy.rs` fails while this module and the shared crate
//! both exist.

mod baseline;
mod engine;
mod fingerprint;
mod glob;
mod model;
mod newtypes;
mod rdjson;
mod report;
mod rule_id;
mod sarif;
mod waiver;

pub use baseline::{Baseline, BaselineEntry, BaselineError, Compared, assign_ordinals, compare};
pub use engine::{Applied, Hidden, HiddenBy, apply_waivers};
pub use fingerprint::{ALGORITHM_VERSION, FindingFingerprint, normalise_snippet};
pub use glob::path_matches;
pub use model::{Finding, FixSource, Rule, Scope};
pub use newtypes::{FixSentence, IsoDate, Reason, TextError};
pub use rdjson::read_rdjson;
pub use report::{Counts, Imported, Report, Skipped, exit_code};
pub use rule_id::{ExternalRuleId, RuleId, RuleIdError};
pub use sarif::{FINGERPRINT_KEY, ReadStats, SarifError, SarifLimits, read_sarif, write_sarif};
pub use waiver::{Origin, Suppression, SuppressionFile, SuppressionScope, Waiver, WaiverError};
