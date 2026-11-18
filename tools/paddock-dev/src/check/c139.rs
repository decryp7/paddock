//! C139: the exceptions to `paddock/e2e-ready-reads` only shrink. The rule is an error in every app's and the ui's e2e lint block (so
//! the existing `--max-warnings 0` lint steps block a new unready read), and that block sets `noInlineConfig`: no inline comment, in any of
//! ESLint's spellings, can switch it off (ESLint's own grammar decides; we do not parse it). The one way to except a read is an entry of the
//! project's `eslint-suppressions.json` (ESLint bulk suppressions: `{ "<spec>": { "<rule>": { "count": n } } }`), and the user approves a new
//! one (ER-46). ESLint's CLI fails on an entry that no longer occurs (never run with the flag that ignores unused ones; `eslint/test/wiring.test.cjs`
//! runs it and forbids the flag), so a fixed read leaves no slack for a new one. This check is the other half: each entry, counted `count`
//! times, is compared with the file at the merge base (`baseline::growth`, the one comparison C53 and C115 use; "not measured" is an error in
//! CI, ADR 0089), and an entry the merge base lacks, or a raised count, is one unwaivable finding at that file.
//!
//! Read: the `eslint-suppressions.json` of an app (`apps/<app>/app/`) and of the ui. A malformed file is one check-broken finding at it.
//! A run that finds no e2e spec at all (`apps/<app>/app/e2e/*.ts`, `ui/e2e/*.ts`) did not look at anything: one not-measured error, never a pass (ER-33).
//!
//! **Report-only (warnings) until its first green CI run that includes it (ER-41); it turns blocking then (mode `Gate`, `--strict C139`),
//! with that run id recorded here, in `C139-e2e-ready-directives.mjs` and in docs/gates.toml.** Recorded: not yet (train 15's integrator fills it in).
//! The lint rule itself is an error already: it had its report-only phase, and the real tree is at 0 sites outside the suppressions files.
//! Rust host; `conformance/src/checks/C139-e2e-ready-directives.mjs` is the port and `tests/it/check_c139.rs` compares them.

use super::baseline::{self, Growth};
use super::finding::Finding;
use super::jsre::rx;
use super::outcome::Outcome;
use super::record::Precondition;
use super::{Check, CheckOutput, Control, Ctx};
use regex::Regex;
use std::sync::LazyLock;

/// The rule whose exceptions are counted (others in the file are shrink-only too).
pub const RULE_ID: &str = "paddock/e2e-ready-reads";

static E2E_TS: LazyLock<Regex> = LazyLock::new(|| rx(r"^(?:apps/[^/]+/app|ui)/e2e/.+\.ts$"));
static SUPPRESSIONS: LazyLock<Regex> = LazyLock::new(|| rx(r"^(?:apps/[^/]+/app|ui)/eslint-suppressions\.json$"));
static NOT_READ: LazyLock<Regex> = LazyLock::new(|| rx(r"(?:^|/)(?:node_modules|[^/]*-snapshots)/"));

const FIX_GROW: &str = "Wait with a ready helper from @decryp7/paddock-e2e (ui/projects/paddock-e2e/src/ready.ts) instead. An exception is an entry of the project's eslint-suppressions.json, and the file only shrinks: a new entry needs the user's approval (ER-46).";
const FIX_BROKEN: &str = "Restore the file: ESLint bulk suppressions, `{ \"e2e/a.spec.ts\": { \"paddock/e2e-ready-reads\": { \"count\": 1 } } }` (`npx eslint e2e --suppress-rule paddock/e2e-ready-reads` writes it).";

/// The entries of an `eslint-suppressions.json` text, each as `count` copies of the key `file NUL rule`; `None` when it is not that shape.
pub fn entries_of(text: &str) -> Option<Vec<String>> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let mut out = Vec::new();
    for (file, rules) in v.as_object()? {
        for (rule, body) in rules.as_object()? {
            let count = body.get("count")?.as_u64()?;
            if count == 0 || count > 10_000 {
                return None;
            }
            for _ in 0..count {
                out.push(format!("{file}\u{0}{rule}"));
            }
        }
    }
    Some(out)
}

/// The check.
pub struct C139;

impl Check for C139 {
    fn id(&self) -> &'static str {
        "C139"
    }

    fn rule(&self) -> u32 {
        49
    }

    fn title(&self) -> &'static str {
        "the exceptions to paddock/e2e-ready-reads (eslint-suppressions.json) only shrink"
    }

    fn scope(&self) -> super::CheckScope {
        super::CheckScope::Root
    }

    fn mode(&self) -> super::Mode {
        super::Mode::ReportOnly
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                ("ui/e2e/a.spec.ts", "const a = 1;\n"),
                (
                    "ui/eslint-suppressions.json",
                    "{\"e2e/a.spec.ts\":{\"paddock/e2e-ready-reads\":{\"count\":1}}}",
                ),
                (
                    "apps/dyno/app/eslint-suppressions.json",
                    "{\"e2e/a.spec.ts\":{\"paddock/e2e-ready-reads\":{\"count\":0}}}",
                ),
                ("ui/src/eslint-suppressions.json", "not read: outside the two places"),
            ],
            expect: vec![("apps/dyno/app/eslint-suppressions.json", 1)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let files = ctx.files();
        let specs = files.iter().filter(|f| E2E_TS.is_match(f) && !NOT_READ.is_match(f)).count();
        if specs == 0 {
            return CheckOutput {
                findings: vec![
                    Finding::error(
                        "C139",
                        49,
                        "ui/e2e",
                        1,
                        "not measured: the run read no e2e spec (apps/<app>/app/e2e, ui/e2e), so no exception list was looked at"
                            .to_string(),
                        "Run the check over the whole repository.",
                    )
                    .unwaivable()
                    .with_outcome(Outcome::NotMeasured),
                ],
                // One probe (looking for specs): zero probes would make the runner add a second not-measured finding.
                probes: 1,
                completed: true,
                // The not-measured finding above is the whole answer; an unmet precondition would add a second one.
                precondition: Precondition {
                    met: true,
                    detail: "no e2e spec in the run (reported as not measured)".to_string(),
                },
            };
        }
        let mut findings = Vec::new();
        let mut probes = specs;
        for f in files.iter().filter(|f| SUPPRESSIONS.is_match(f)) {
            probes += 1;
            let Some(text) = ctx.read(f) else {
                findings.push(
                    Finding::error(
                        "C139",
                        49,
                        f,
                        1,
                        format!("not measured: {f} could not be read, so its exceptions cannot be seen"),
                        "Make the file readable (permissions) and run the check again.",
                    )
                    .unwaivable()
                    .with_outcome(Outcome::NotMeasured),
                );
                continue;
            };
            let Some(current) = entries_of(&text) else {
                findings.push(
                    Finding::warn(
                        "C139",
                        49,
                        f,
                        1,
                        "the check cannot look: not ESLint bulk suppressions (an object of files, rules and counts of at least 1)"
                            .to_string(),
                        FIX_BROKEN,
                    )
                    .unwaivable()
                    .with_outcome(Outcome::CheckBroken),
                );
                continue;
            };
            if ctx.is_in_memory() || !baseline::is_work_tree(ctx.root()) {
                continue;
            }
            let of = |t: &str| entries_of(t);
            let exact = |k: &String| k.clone();
            match baseline::growth(ctx.root(), &ctx.base_ref(), f, &current, &of, &exact, None) {
                Growth::Unmeasured(why) => findings.push(baseline::unmeasured_finding(ctx.in_ci(), "C139", 49, f, "the exception list", &why)),
                Growth::Added { added: 0, .. } => {}
                Growth::Added { base, added, .. } => findings.push(
                    Finding::warn(
                        "C139",
                        49,
                        f,
                        1,
                        format!(
                            "the exception list grew: {added} entr{} (counting each `count`) not in {f} at the merge base {}; it may only shrink",
                            if added == 1 { "y is" } else { "ies are" },
                            &base[..base.len().min(9)]
                        ),
                        FIX_GROW,
                    )
                    .unwaivable(),
                ),
            }
        }
        CheckOutput {
            findings,
            probes,
            completed: true,
            precondition: Precondition {
                met: true,
                detail: format!("{specs} e2e specs and the exception lists are read"),
            },
        }
    }
}
