//! The fast static rows `./dev merge sync-member` runs on the sync tip (the member with the train merged in): rustfmt, conformance, the
//! gate register, Clippy and the Windows Clippy of the workspaces the member touches.
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

/// The `--only` selectors: whole row names or a row's first word. `fmt`, `clippy` and `clippy-windows` are per workspace, and the plan
/// keeps only the workspaces the change reaches. `fmt` is in because a formatting failure is the cheapest to find here and was the
/// one static row train z's members lacked.
pub const SELECTORS: &[&str] = &["fmt", "conformance", "gates-register", "clippy", "clippy-windows"];

/// The arguments after `check --pre-push` for the rows: the train's lane of `throttle` (`--priority train`, so the rows go before
/// normal waiters instead of waiting 50 minutes in the queue, train z) and the selected rows.
fn extra_args() -> Vec<String> {
    ["--priority", "train", "--only", &SELECTORS.join(",")].map(String::from).to_vec()
}

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
        let program = program_for_tip(root, std::env::var_os("PADDOCK_MERGE_CHECK_GATES"));
        let extra = extra_args();
        run_pre_push_with(&program, root, base, &extra.iter().map(String::as_str).collect::<Vec<_>>())
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
    let cmd = format!("./dev check --pre-push --priority train --only {} --base {base}", SELECTORS.join(","));
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
    // A row's one-line cause and the log of its step, as `check --pre-push --json` names them (ER-27): without them a failed row is
    // only a name (train z's failed `gates-register`). An older sync tip's tool prints neither, and the line says so.
    let why = |id: &str| -> String {
        let row = gates.iter().find(|g| g.get("id").and_then(|i| i.as_str()) == Some(id));
        let field = |k: &str| row.and_then(|r| r.get(k)).and_then(|v| v.as_str()).filter(|v| !v.is_empty());
        let cause = field("reason").map_or("no cause in the row's answer".to_string(), |r| r.lines().next().unwrap_or(r).to_string());
        match field("log") {
            Some(log) => format!("{id}: {cause} (log: {log})"),
            None => format!("{id}: {cause}"),
        }
    };
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
        let described: Vec<String> = broken.iter().map(|id| why(id)).collect();
        fail(described.iter().map(String::as_str).collect(), Exit::CheckBroken, "CHECK BROKEN", &mut text)
    } else if !failed.is_empty() {
        let described: Vec<String> = failed.iter().map(|id| why(id)).collect();
        fail(described.iter().map(String::as_str).collect(), Exit::Findings, "FAILED", &mut text)
    } else if !unproven.is_empty() || !missing.is_empty() || run.code != Some(0) {
        let mut ids: Vec<String> = unproven.iter().map(|id| why(id)).collect();
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
    fn the_rows_asked_are_rustfmt_conformance_the_gate_register_and_both_clippy_kinds() {
        assert_eq!(SELECTORS, ["fmt", "conformance", "gates-register", "clippy", "clippy-windows"]);
    }

    /// The rows go through the train's lane of `throttle`: run outside it, they waited 50 minutes in a 20-entry queue on train z.
    #[test]
    fn the_rows_run_in_the_trains_lane_of_the_queue() {
        let args = extra_args();
        let at = args.iter().position(|a| a == "--priority").expect("--priority");
        assert_eq!(args[at + 1], "train", "{args:?}");
        let only = args.iter().position(|a| a == "--only").expect("--only");
        assert_eq!(args[only + 1], SELECTORS.join(","), "{args:?}");
    }

    /// A failed row prints its one-line cause and its step's log, not its name alone (ER-27, train z: a failed gates-register with no
    /// reason). A tool that prints no cause is said so, and the first line of a long cause is the one shown.
    #[test]
    fn a_failing_row_prints_its_cause_and_its_log() {
        let gates = vec![
            json!({"id": "conformance", "verdict": "passed"}),
            json!({"id": "gates-register", "verdict": "failed", "reason": "gates check (exit 1)\nsecond line", "log": "/runs/gr.log"}),
            json!({"id": "fmt crates", "verdict": "failed"}),
        ];
        let answer = Ok(GateRun { code: Some(5), text: String::new(), wire: Some(json!({"gates": gates})) });
        let s = interpret(answer, "abc");
        assert_eq!(s.exit, Exit::Findings);
        let text = s.text.join("\n");
        assert_eq!(text.matches("::error::").count(), 1, "{text}");
        assert!(text.contains("gates-register: gates check (exit 1) (log: /runs/gr.log)"), "{text}");
        assert!(!text.contains("second line"), "{text}");
        assert!(text.contains("fmt crates: no cause in the row's answer"), "{text}");
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
