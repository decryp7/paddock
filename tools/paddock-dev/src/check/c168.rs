//! C168: the flaky-test quarantine register (`docs/flaky-quarantine.toml`, ER-12 as graded by `./dev ci verdict`) grows only through the
//! tool (ER-46, rule 83). The register's reader and writer is [`crate::ci_runs::quarantine`]; this check reads it through that reader
//! (ER-42) and reports:
//!
//! 1. **unreadable**: a register the reader refuses (not TOML, an unknown key, a policy other than shrink-only, a repeated id). One
//!    unwaivable check-broken finding.
//! 2. **grown by hand**: an entry the register at the merge base lacks, without the lessons bullet of its run added in the same diff
//!    (`./dev ci fix-items` writes both in one step; [`crate::ci_runs::quarantine::growth`]).
//! 3. **first use by hand**: a `[first_use]` the merge base lacks, or a changed one, whose run has no fix-item bullet in docs/lessons.md
//!    (`./dev ci quarantine-record` refuses a run without one, so the switch is only ever made on a run the tool validated and recorded;
//!    [`crate::ci_runs::quarantine::first_use_growth`]).
//!
//! The comparison is the target ref's ([`super::baseline::target_ref`], `PADDOCK_BASE_REF`), with the first parent on the target branch
//! itself, as the other growth checks measure. A guard rail against hand edits, not a security boundary. It runs on a checkout only;
//! where git gives no merge base it warns (a not-measured error in CI), never silence.
//!
//! Positive control ([`Check::control`], ER-33): a register in another policy is reported; the growth rule's controls are the
//! tests of [`crate::ci_runs::quarantine`] (a hand entry, a forged test, another recorder, a hand `[first_use]`, each red; the tool's
//! own step not), on a real git checkout too.
//!
//! **Report-only (warnings) until its first green run on the real tree is recorded here and in docs/gates.toml (ER-41).** Recorded: not
//! yet.

use super::baseline::{self, Unmeasured};
use super::finding::Finding;
use super::outcome::Outcome;
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use crate::ci_runs::quarantine::{self, REGISTER, Register};

const RULE: u32 = 83;
const FIX: &str = "The register grows only through `./dev ci fix-items --workflow <file> --sha <landed sha>` (entries) and `./dev ci quarantine-record` (first use): remove the hand-written lines, or record the run with the tool (ER-46).";

/// The check.
pub struct C168;

impl Check for C168 {
    fn id(&self) -> &'static str {
        "C168"
    }

    fn rule(&self) -> u32 {
        RULE
    }

    fn title(&self) -> &'static str {
        "the flaky-test quarantine register grows only through ./dev ci fix-items"
    }

    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }

    fn mode(&self) -> Mode {
        Mode::ReportOnly
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                (REGISTER, "version = 1\npolicy = \"free\"\nrelease_after = 5\n"),
                ("docs/lessons.md", "# Lessons\n"),
            ],
            expect: vec![(REGISTER, 1)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let output = |findings: Vec<Finding>, probes: usize, detail: &str| CheckOutput {
            findings,
            probes,
            completed: true,
            precondition: Precondition {
                met: true,
                detail: detail.to_string(),
            },
        };
        let Some(text) = ctx.read(REGISTER) else {
            return CheckOutput::not_applicable("no docs/flaky-quarantine.toml in this tree");
        };
        if let Err(why) = Register::parse(&text) {
            let f = Finding::warn("C168", RULE, REGISTER, 1, format!("the check cannot look: {why}"), FIX)
                .unwaivable()
                .with_outcome(Outcome::CheckBroken);
            return output(vec![f], 1, "the register is read through its one reader");
        }
        if ctx.is_in_memory() || !baseline::is_work_tree(ctx.root()) {
            return output(
                Vec::new(),
                1,
                "the register's shape is read; its growth is measured on a checkout only",
            );
        }
        let findings = match quarantine::growth_in(ctx.root(), &ctx.base_ref()) {
            Ok(found) => found
                .into_iter()
                .map(|why| Finding::warn("C168", RULE, REGISTER, 1, why, FIX).unwaivable())
                .collect(),
            Err(why) => vec![baseline::unmeasured_finding(
                ctx.in_ci(),
                "C168",
                RULE,
                REGISTER,
                "the flaky-test quarantine register",
                &Unmeasured::NoBase(why),
            )],
        };
        output(findings, 1, "the register is compared with the merge base")
    }
}
