//! Approvals of exceptions, as one pure core (ER-46, ER-39, ER-41): no file, git or process access. A caller reads the registers and the
//! merge-base diff and hands in text and sets.
//!
//! - A typed record of each approval kind the repository has: the `approved` field of a gate-register entry ([`EntryRecord`], read by
//!   [`parse_gates`]), a growth approval ([`GrowthRecord`], `[[register_growth]]`) and an OS-removal approval ([`OsRemovalRecord`],
//!   [`parse_os_removals`]), each read with a real TOML parser, never a regex (ER-42).
//! - One [`Verdict`] per thing that needs approval: Approved, Missing (with the line to add), Mismatch or Stale (the exception it
//!   approves is already on the target branch, so the line goes).
//! - One [`Stage`] policy ([`severity`]): a Missing approval on an entry the branch itself adds is a warning at a branch push and an
//!   error at the member gate and landing; Stale is a warning on a branch and a finding at landing; Mismatch is always an error.
//!
//! The crate never writes an approval: [`Verdict::Missing`] carries the line the approver records, nothing more (ER-39).

mod approver;
mod gates;
mod growth;
mod os_removals;
mod problem;
mod stage;
mod verdict;

pub use approver::{Approver, Claim};
pub use gates::{ENTRY_TABLES, EntryRecord, GatesApprovals, Judgement, judge_entries, parse_gates};
pub use growth::{GrowthRecord, growth_digest, growth_line, judge_growth};
pub use os_removals::{
    KeyVerdict, OsRemovalRecord, OsRemovals, judge_os_removals, parse_os_removals,
};
pub use problem::Problem;
pub use stage::{Severity, Stage, severity};
pub use verdict::Verdict;
