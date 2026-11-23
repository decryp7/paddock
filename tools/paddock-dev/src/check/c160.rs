//! C160: every input of `ci.yml`'s `workflow_dispatch` and `workflow_call` triggers is declared in the run's scope or said not to narrow
//! it (ADR 0113, ADR 0117 §6; ER-21 class "an input narrowed the run and the scope line did not say so": the `tree` and `os` inputs
//! widened what "complete" meant, a run on one OS or a subtree counted as the complete run of its commit).
//!
//! The declaration is [`crate::run_scope::CI_INPUTS`]: each input is `Declared(<key of the scope line>)` (`rerun`, `base`, `os`,
//! `ran.engines`, `ran.os_full`, `ran.apps`) or `NonNarrowing(<reason>)`. The inputs are read through the CI model
//! ([`crate::gates::ci::workflow_inputs`], ER-42), the keys from a scope line the module writes ([`crate::run_scope::line_keys`]).
//! Flagged: an input the table does not name (decide, and add it in the change that adds the input); an entry that names no input of
//! the file (stale); a `Declared` key the scope line does not carry; a `NonNarrowing` reason shorter than 8 characters.
//!
//! **Report-only (warnings) until its first green run on the real tree is recorded here and in docs/gates.toml (ER-41).** Recorded: not
//! yet (2026-11-23). No baseline or allow list (ER-46): the table is the list, and an input that is not in it is a finding.

use super::finding::Finding;
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use crate::gates::ci::workflow_inputs;
#[cfg(test)]
use crate::gates::ci::workflow_inputs_at;
use crate::run_scope::{CI_INPUTS, InputScope, line_keys};
use std::collections::BTreeSet;

/// The workflow this check reads.
pub const WORKFLOW: &str = ".gitea/workflows/ci.yml";
/// The file that holds the declaration.
pub const TABLE_FILE: &str = "tools/paddock-dev/src/run_scope.rs";

const FIX_UNDECLARED: &str = "Add the input to CI_INPUTS in tools/paddock-dev/src/run_scope.rs: `Declared(<key of the scope line>)` when it narrows the run (and put the key on the line), or `NonNarrowing(<why it changes neither jobs nor tests>)`.";

/// One problem with the declaration, `(file, message, fix)`.
pub fn judge(inputs: &[String], table: &[(&str, InputScope)], keys: &BTreeSet<String>) -> Vec<(&'static str, String, String)> {
    let mut out = Vec::new();
    for input in inputs {
        if !table.iter().any(|(name, _)| name == input) {
            out.push((
                WORKFLOW,
                format!("input `{input}` is neither declared in the run's scope nor listed as non-narrowing"),
                FIX_UNDECLARED.to_string(),
            ));
        }
    }
    for (name, how) in table {
        if !inputs.iter().any(|i| i == name) {
            out.push((
                TABLE_FILE,
                format!("CI_INPUTS names `{name}`, which is no input of ci.yml (stale)"),
                "Remove the entry.".to_string(),
            ));
            continue;
        }
        match how {
            InputScope::Declared(key) if !keys.contains(*key) => out.push((
                TABLE_FILE,
                format!("input `{name}` is declared as `{key}`, which the scope line does not carry"),
                format!(
                    "Use a key of the line ({}), or add the key to the line.",
                    keys.iter().cloned().collect::<Vec<_>>().join(", ")
                ),
            )),
            InputScope::NonNarrowing(reason) if reason.trim().chars().count() < 8 => out.push((
                TABLE_FILE,
                format!("input `{name}` is listed as non-narrowing without a reason"),
                "Say why it changes neither the jobs nor the tests of the run.".to_string(),
            )),
            _ => {}
        }
    }
    out
}

/// The check.
pub struct C160;

impl Check for C160 {
    fn id(&self) -> &'static str {
        "C160"
    }

    fn rule(&self) -> u32 {
        90
    }

    fn title(&self) -> &'static str {
        "every input of ci.yml is in the run's declared scope or listed as non-narrowing"
    }

    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }

    fn mode(&self) -> Mode {
        Mode::ReportOnly
    }

    fn control(&self) -> Control {
        Control {
            files: vec![(
                WORKFLOW,
                "name: ci\non:\n  workflow_dispatch:\n    inputs:\n      base:\n        default: \"\"\n      mystery:\n        default: \"\"\n  workflow_call:\n    inputs:\n      webkit:\n        type: boolean\n",
            )],
            expect: vec![(WORKFLOW, 1)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let Some(text) = ctx.read(WORKFLOW) else {
            return CheckOutput::not_applicable("the tree has no ci.yml");
        };
        let inputs = match workflow_inputs(WORKFLOW, &text) {
            Ok(i) => i,
            Err(why) => {
                return CheckOutput {
                    findings: vec![
                        Finding::warn(
                            "C160",
                            90,
                            WORKFLOW,
                            1,
                            format!("the CI model cannot read the workflow: {why}"),
                            "Fix the YAML.",
                        )
                        .unwaivable(),
                    ],
                    probes: 1,
                    completed: true,
                    precondition: Precondition {
                        met: true,
                        detail: "ci.yml was read".into(),
                    },
                };
            }
        };
        let all: Vec<String> = inputs
            .dispatch
            .iter()
            .chain(&inputs.call)
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        // The real table is checked against the real file; the in-memory control checks the same function on its own file.
        let findings = judge(&all, &CI_INPUTS, &line_keys())
            .into_iter()
            .filter(|(file, _, _)| *file == WORKFLOW || !ctx.is_in_memory())
            .map(|(file, message, fix)| Finding::warn("C160", 90, file, 1, message, fix))
            .collect();
        CheckOutput {
            findings,
            probes: all.len().max(1),
            completed: true,
            precondition: Precondition {
                met: !all.is_empty(),
                detail: format!("{} inputs of ci.yml read", all.len()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn keys() -> BTreeSet<String> {
        ["rerun", "base", "os", "ran.engines"].iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn an_input_the_table_does_not_name_is_a_finding_at_the_workflow() {
        let table = [("base", InputScope::Declared("base"))];
        let out = judge(&names(&["base", "tree"]), &table, &keys());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, WORKFLOW);
        assert!(out[0].1.contains("`tree`"), "{}", out[0].1);
    }

    #[test]
    fn a_declared_input_and_a_non_narrowing_one_with_a_reason_are_clean() {
        let table = [
            ("base", InputScope::Declared("base")),
            (
                "sccache_backend",
                InputScope::NonNarrowing("the compile cache only: no job is added or dropped"),
            ),
        ];
        assert!(judge(&names(&["base", "sccache_backend"]), &table, &keys()).is_empty());
    }

    #[test]
    fn a_stale_entry_a_key_the_line_lacks_and_a_bare_non_narrowing_are_findings_at_the_table() {
        let table = [
            ("gone", InputScope::Declared("base")),
            ("os", InputScope::Declared("ran.nothing")),
            ("app", InputScope::NonNarrowing("")),
        ];
        let out = judge(&names(&["os", "app"]), &table, &keys());
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(|f| f.0 == TABLE_FILE));
    }

    #[test]
    fn the_real_table_covers_the_real_inputs_of_ci_yml_and_every_key_is_on_the_line() {
        // The positive control on the real tree: the declaration in the code against the file in the repository.
        let i = workflow_inputs_at(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")), WORKFLOW).unwrap();
        let all: Vec<String> = i
            .dispatch
            .iter()
            .chain(&i.call)
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        assert!(all.len() >= 6, "the real ci.yml has inputs ({all:?})");
        assert_eq!(judge(&all, &CI_INPUTS, &line_keys()), vec![]);
    }

    #[test]
    fn dropping_an_entry_of_the_real_table_goes_red() {
        let i = workflow_inputs_at(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")), WORKFLOW).unwrap();
        let all: Vec<String> = i
            .dispatch
            .iter()
            .chain(&i.call)
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let without: Vec<(&str, InputScope)> = CI_INPUTS.iter().filter(|(n, _)| *n != "since_run").copied().collect();
        let out = judge(&all, &without, &line_keys());
        assert!(out.iter().any(|f| f.1.contains("`since_run`")), "{out:?}");
    }
}
