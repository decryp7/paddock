//! The fast static rows `./dev merge sync-member` runs on the sync tip (the member with the train merged in): conformance, the gate
//! register, Clippy and the Windows Clippy of the workspaces the member touches.
//!
//! The failure it prevents (trains p and q, C171 and C173): a member built on an older main failed checks that landed on main since,
//! and the train's long member gate was the first place anyone saw it. These rows are the cheap part of that gate; they run through
//! `./dev check --pre-push --only <rows> --base <train>`, so the plan is the member's own change and a green run on the same
//! commit and rows is reused (ER-49). `pre_push` does the work; this module starts it and reads the per-row answer.
//!
//! A pass needs both always-present rows (`conformance`, `gates-register`) to say `passed`: a run that printed no rows, left one out
//! or could not look is NOT MEASURED, never a pass (ER-33). The escape is `--no-static "<reason>"`, logged like `--old-base-anyway`.

use super::gates::{GateRun, run_pre_push_with};
use paddock_cli::Exit;
use serde_json::json;
use std::path::{Path, PathBuf};

/// The `--only` selectors: whole row names or a row's first word. `clippy` and `clippy-windows` are per workspace, and the plan
/// keeps only the workspaces the change reaches.
pub const SELECTORS: &[&str] = &["conformance", "gates-register", "clippy", "clippy-windows"];

/// Rows that are always in the plan: a pass without both of them is not measured.
const REQUIRED: &[&str] = &["conformance", "gates-register"];

/// Starts the static rows of a checkout against a base.
pub trait StaticGates {
    /// Runs `check --pre-push --only <SELECTORS> --base <base>` in `root`; an error is a failure to start.
    fn run(&self, root: &Path, base: &str) -> Result<GateRun, String>;
}

/// The program that runs the rows: the sync tip's own `./dev` (its wrapper builds or fetches the tool that tip's inputs name), so a
/// command the member adds exists for its own rows. `stand_in` (`PADDOCK_MERGE_CHECK_GATES`, the tests') wins when set.
fn program_for_tip(root: &Path, stand_in: Option<std::ffi::OsString>) -> PathBuf {
    stand_in.map_or_else(|| root.join("dev"), PathBuf::from)
}

/// The real run: the sync tip's own `./dev` (or `PADDOCK_MERGE_CHECK_GATES`, the stand-in the tests use).
pub struct Real;

impl StaticGates for Real {
    fn run(&self, root: &Path, base: &str) -> Result<GateRun, String> {
        let only = SELECTORS.join(",");
        let program = program_for_tip(root, std::env::var_os("PADDOCK_MERGE_CHECK_GATES"));
        run_pre_push_with(&program, root, base, &["--only", &only])
    }
}

/// What the rows said: the lines to print, the exit and the data.
pub struct Step {
    /// One line per row, then the verdict (and the one `::error::` line when it is not a pass).
    pub text: Vec<String>,
    /// The structured answer.
    pub data: serde_json::Value,
    /// `Ok`, `Findings` (a row failed), `NotMeasured` or `CheckBroken`.
    pub exit: Exit,
}

/// The line printed when `--no-static` lets the member through without the rows.
pub fn skipped(reason: &str) -> Step {
    Step {
        text: vec![format!("::warning::merge sync-member: static rows NOT run (--no-static: {reason})")],
        data: json!({"outcome": "skipped", "reason": reason}),
        exit: Exit::Ok,
    }
}

/// Said out loud when there is no sync tip yet; not a failure.
pub fn not_yet(train: &str) -> Step {
    Step {
        text: vec![format!(
            "static rows not run: {train} is not merged into HEAD yet; after `git merge {train}` run this command again and it runs them on the sync tip"
        )],
        data: json!({"outcome": "not run", "why": "the train is not merged"}),
        exit: Exit::Ok,
    }
}

/// The step for what `check --pre-push` answered, read per row; the exit code alone is never read as a pass.
pub fn interpret(answer: Result<GateRun, String>, base: &str) -> Step {
    let cmd = format!("./dev check --pre-push --only {} --base {base}", SELECTORS.join(","));
    let not_measured = |why: String, mut text: Vec<String>| {
        text.push(format!(
            "::error::merge sync-member: static rows NOT MEASURED: {why}. Next step: run `{cmd}` yourself in this worktree, fix what it prints on the member, then re-sync; a sync tip whose static rows did not run is not one that passed."
        ));
        Step {
            data: json!({"outcome": "not measured", "base": base, "why": why}),
            text,
            exit: Exit::NotMeasured,
        }
    };
    let run = match answer {
        Err(why) => return not_measured(why, Vec::new()),
        Ok(run) => run,
    };
    let Some(gates) = run.wire.as_ref().and_then(|w| w.get("gates")).and_then(|g| g.as_array()) else {
        let how = run
            .code
            .map_or("was ended before it finished".to_string(), |c| format!("exited {c}"));
        return not_measured(format!("`check --pre-push` {how} and printed no rows"), vec![run.text.clone()]);
    };
    let rows: Vec<(String, String)> = gates
        .iter()
        .filter_map(|g| Some((g.get("id")?.as_str()?.to_string(), g.get("verdict")?.as_str()?.to_string())))
        .collect();
    let mut text: Vec<String> = rows.iter().map(|(id, v)| format!("static row  {v:<13} {id}")).collect();
    let named = |bad: &dyn Fn(&str) -> bool| rows.iter().filter(|(_, v)| bad(v)).map(|(id, _)| id.as_str()).collect::<Vec<_>>();
    let broken = named(&|v| v == "broken");
    let failed = named(&|v| v == "failed");
    let unproven = named(&|v| matches!(v, "not measured" | "accepted"));
    let missing: Vec<&str> = REQUIRED
        .iter()
        .copied()
        .filter(|r| !rows.iter().any(|(id, v)| id == r && v == "passed"))
        .collect();
    let data = |outcome: &str| json!({"outcome": outcome, "base": base, "rows": rows.iter().map(|(id, v)| json!({"id": id, "verdict": v})).collect::<Vec<_>>()});
    let fail = |ids: Vec<&str>, exit: Exit, outcome: &str, text: &mut Vec<String>| {
        text.push(format!(
            "::error::merge sync-member: static rows {outcome}: {}. Next step: fix on the member, then re-sync (merge the train again and run this command again); `{cmd}` reproduces it.",
            ids.join("; ")
        ));
        exit
    };
    let exit = if !broken.is_empty() {
        fail(broken, Exit::CheckBroken, "CHECK BROKEN", &mut text)
    } else if !failed.is_empty() {
        fail(failed, Exit::Findings, "FAILED", &mut text)
    } else if !unproven.is_empty() || !missing.is_empty() || run.code != Some(0) {
        let mut ids: Vec<String> = unproven.iter().map(|s| s.to_string()).collect();
        ids.extend(missing.iter().map(|r| format!("{r} (no passed row)")));
        if ids.is_empty() {
            ids.push(format!("`check --pre-push` exited {:?} though no row failed", run.code));
        }
        fail(
            ids.iter().map(String::as_str).collect(),
            Exit::NotMeasured,
            "NOT MEASURED",
            &mut text,
        )
    } else {
        text.push(format!("static rows: passed ({} row(s), `{cmd}`)", rows.len()));
        Exit::Ok
    };
    let outcome = match exit {
        Exit::Ok => "passed",
        Exit::Findings => "failed",
        Exit::CheckBroken => "broken",
        _ => "not measured",
    };
    Step {
        text,
        data: data(outcome),
        exit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(code: Option<i32>, rows: &[(&str, &str)]) -> Result<GateRun, String> {
        let gates: Vec<_> = rows.iter().map(|(id, v)| json!({"id": id, "verdict": v})).collect();
        Ok(GateRun {
            code,
            text: String::new(),
            wire: Some(json!({"ok": true, "gates": gates})),
        })
    }

    const GREEN: &[(&str, &str)] = &[
        ("conformance", "passed"),
        ("gates-register", "passed"),
        ("clippy crates", "passed"),
        ("clippy-windows crates", "passed"),
    ];

    /// The rows run the SYNC TIP's own `./dev`, not the train's: a member that adds a `./dev` command would otherwise fail its own
    /// static row ("does not exist"). Only the stand-in override (the tests') wins.
    #[test]
    fn the_rows_run_the_sync_tips_own_dev_unless_the_stand_in_is_set() {
        let root = Path::new("wt/member");
        assert_eq!(program_for_tip(root, None), root.join("dev"));
        let stand_in = std::ffi::OsString::from("x/rows");
        assert_eq!(program_for_tip(root, Some(stand_in)), Path::new("x/rows"));
    }

    #[test]
    fn the_rows_asked_are_conformance_the_gate_register_and_both_clippy_kinds() {
        assert_eq!(SELECTORS, ["conformance", "gates-register", "clippy", "clippy-windows"]);
    }

    #[test]
    fn every_row_is_printed_and_all_passed_is_a_pass() {
        let s = interpret(run(Some(0), GREEN), "abc");
        assert_eq!(s.exit, Exit::Ok);
        let text = s.text.join("\n");
        for (id, _) in GREEN {
            assert!(text.contains(id), "{text}");
        }
        assert!(text.contains("static rows: passed"), "{text}");
        assert!(!text.contains("::error::"), "{text}");
    }

    /// A failing row stops it: non-zero, one `::error::` naming the row and the next step.
    #[test]
    fn a_failing_row_ends_with_one_error_naming_it_and_the_next_step() {
        let mut rows = GREEN.to_vec();
        rows[0] = ("conformance", "failed");
        let s = interpret(run(Some(5), &rows), "abc");
        assert_eq!(s.exit, Exit::Findings);
        let text = s.text.join("\n");
        assert_eq!(text.matches("::error::").count(), 1, "{text}");
        assert!(
            text.contains("conformance") && text.contains("fix on the member, then re-sync"),
            "{text}"
        );
    }

    /// Positive control (ER-33): hide the signal and it goes red. No rows, a missing required row, a skipped required row, a
    /// not-measured row, an exit that disagrees and a failure to start are never a pass.
    #[test]
    fn hiding_the_signal_is_not_a_pass() {
        assert_eq!(interpret(Err("no tool".into()), "b").exit, Exit::NotMeasured);
        let none = Ok(GateRun {
            code: Some(0),
            text: String::new(),
            wire: None,
        });
        assert_eq!(interpret(none, "b").exit, Exit::NotMeasured);
        assert_eq!(interpret(run(Some(0), &[]), "b").exit, Exit::NotMeasured);
        assert_eq!(interpret(run(Some(0), &GREEN[1..]), "b").exit, Exit::NotMeasured);
        let mut skipped_row = GREEN.to_vec();
        skipped_row[1] = ("gates-register", "skipped");
        assert_eq!(interpret(run(Some(0), &skipped_row), "b").exit, Exit::NotMeasured);
        let mut nm = GREEN.to_vec();
        nm[2] = ("clippy crates", "not measured");
        assert_eq!(interpret(run(Some(6), &nm), "b").exit, Exit::NotMeasured);
        assert_eq!(interpret(run(Some(1), GREEN), "b").exit, Exit::NotMeasured);
        let mut broken = GREEN.to_vec();
        broken[3] = ("clippy-windows crates", "broken");
        assert_eq!(interpret(run(Some(7), &broken), "b").exit, Exit::CheckBroken);
    }

    #[test]
    fn the_escape_is_logged_with_its_reason_and_passes() {
        let s = skipped("hotfix");
        assert_eq!(s.exit, Exit::Ok);
        assert!(s.text.join("\n").contains("--no-static: hotfix"));
    }
}
