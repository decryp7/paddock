//! The push gate with a plan, a list of files and a fake machine: no process starts (ER-14).

use super::budget::Timing;
use super::exec::{Done, Exec, Kind, Limits, Row, RunOpts, run_all_timed, run_all_with, test_subject};
use super::gates::{AUDIT_PROFILE, Change, Facts, Gate, Need, Planned, plan_gates, windows_skip_reason};
use super::{accept_not_measured, parse_refs, render, render_timed};
use crate::throttle::QUEUE_STALL;
use paddock_cli::Exit;
use serde_json::json;
use std::collections::BTreeSet;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

fn plan() -> serde_json::Value {
    json!({
        "conformance": true,
        "apps": ["dyno", "pitwall"],
        "packages": {
            "crates": ["paddock-proc"],
            "apps/dyno": ["dyno-app"],
            "apps/pitwall": ["pitwall-app"],
            "apps/debrief": ["debrief-core"],
            "apps/debrief/fuzz": ["debrief-fuzz"],
        }
    })
}

fn facts() -> Facts {
    Facts {
        conformance: Some(Ok(["conformance/bin/paddock-conformance.mjs", "--app", "paddock", "--only", "C06"]
            .map(String::from)
            .to_vec())),
        conformance_env: None,
        windows_clippy: BTreeSet::from(["apps/dyno".to_string(), "apps/pitwall".to_string()]),
        member_gate: true,
        kept_targets: None,
        pitwall_tests: vec!["scripts/ci/a.test.mjs".into()],
        dev_exe: Ok("/x/dev".into()),
        target_dir: Some("/t/".into()),
        inactive: Ok(BTreeSet::new()),
        test_filters: Default::default(),
        nextest_config: Default::default(),
        reached: Default::default(),
    }
}

fn change(files: &[&str]) -> Change {
    Change {
        files: files.iter().map(|f| f.to_string()).collect(),
        ranges: vec!["abc..HEAD".into()],
        base: Some("origin/main".into()),
    }
}

fn run_gates(planned: &[Planned]) -> Vec<&Gate> {
    planned
        .iter()
        .filter_map(|p| if let Planned::Run(g) = p { Some(g) } else { None })
        .collect()
}

fn ids(planned: &[Planned]) -> Vec<String> {
    run_gates(planned).iter().map(|g| g.id.clone()).collect()
}

#[test]
fn a_crates_change_reaches_every_gate_of_ci_er_9() {
    let planned = plan_gates(
        &plan(),
        &change(&["crates/paddock-proc/src/lib.rs", "apps/dyno/src/a.rs", "apps/pitwall/src/a.rs"]),
        &facts(),
    );
    let got = ids(&planned);
    for want in [
        "fmt crates",
        "lockfile crates",
        "clippy crates",
        "clippy-windows crates",
        "conformance",
        "commit-lint",
        "fmt apps/dyno",
        "clippy-windows apps/dyno",
        "pitwall-scripts",
        "pitwall-deny",
    ] {
        assert!(
            got.iter()
                .any(|g| g == want || (want == "pitwall-deny" && g.starts_with("pitwall-deny"))),
            "{want} is missing from {got:?}"
        );
    }
    assert!(!got.iter().any(|g| g.contains("fuzz")), "a fuzz workspace has its own job: {got:?}");
    let by = |id: &str| run_gates(&planned).into_iter().find(|g| g.id == id).cloned().unwrap();
    // The build folder is cargo configuration right after the subcommand, never a variable (sccache hashes it).
    assert_eq!(&by("clippy crates").args[1..3], ["--config", "build.target-dir='/t'"]);
    assert_eq!(&by("clippy-windows crates").args[2..4], ["--config", "build.target-dir='/t/xwin'"]);
    assert!(
        run_gates(&planned)
            .iter()
            .all(|g| g.env.iter().all(|(k, _)| !k.starts_with("CARGO_")))
    );
    // CI's own command lines (ci.yml: crates lint with every feature; apps without), without that pair.
    let by = |id: &str| {
        let mut g = by(id);
        if let Some(i) = g.args.iter().position(|a| a == "--config") {
            g.args.drain(i..i + 2);
        }
        g
    };
    assert_eq!(
        by("clippy crates").args.join(" "),
        "clippy --workspace --all-targets --all-features --locked -- -D warnings"
    );
    assert_eq!(
        by("clippy apps/dyno").args.join(" "),
        "clippy --workspace --all-targets --locked -- -D warnings"
    );
    assert_eq!(by("fmt crates").args.join(" "), "fmt --all --check");
    assert_eq!(by("lockfile crates").args.join(" "), "metadata --locked --format-version 1");
    assert_eq!(
        by("clippy-windows crates").args.join(" "),
        "xwin clippy --target x86_64-pc-windows-msvc --workspace --all-targets --all-features --locked -- -D warnings"
    );
    assert!(by("clippy-windows crates").env.is_empty());
    assert_eq!(by("clippy-windows crates").need, Need::Tool("cargo-xwin".into()));
    assert!(
        by("clippy-windows crates")
            .next
            .contains("rustup target add x86_64-pc-windows-msvc --toolchain")
    );
    assert_eq!(
        (by("conformance").program.as_str(), by("conformance").args.join(" ")),
        ("/x/dev", "ci run conformance paddock".to_string())
    );
    assert_eq!(
        by("commit-lint").args.join(" "),
        "conformance/bin/paddock-conformance.mjs commit-lint --range abc..HEAD"
    );
}

#[test]
fn a_dyno_core_change_regenerates_the_wire_fixtures_and_others_skip_it() {
    // Was a lefthook entry beside the pre-push command (fix/test-temp-group-and-wire-fixtures); the hook is one command now.
    let planned = plan_gates(&plan(), &change(&["apps/dyno/crates/dyno-core/src/protocol.rs"]), &facts());
    let g = run_gates(&planned).into_iter().find(|g| g.id == "dyno-wire-fixtures").cloned();
    let g = g.expect("dyno-wire-fixtures is planned for a dyno-core change");
    assert_eq!(
        (g.program.as_str(), g.args.join(" ")),
        ("sh", "apps/dyno/scripts/regen-wire-fixtures.sh".to_string())
    );
    assert!(g.heavy, "it runs cargo test");
    let other = plan_gates(&plan(), &change(&["apps/dyno/src/a.rs"]), &facts());
    assert!(!run_gates(&other).iter().any(|g| g.id == "dyno-wire-fixtures"));
    assert!(
        other
            .iter()
            .any(|p| matches!(p, Planned::Skip { id, .. } if id == "dyno-wire-fixtures")),
        "skipped with a reason, not silently"
    );
}

/// C100 in the push gate (user, 2026-11-17): a new conformance check that no CI line runs landed unwired twice (four checks on
/// 2026-11-12, C138 on 2026-11-17). The row runs on every push, as the conformance row does (ER-41; review: a hand list of C100's
/// inputs missed apps/dyno/paddock.ci.json, so a change from `--only C13` to C57 was skipped and passed, ER-42).
#[test]
fn the_gates_check_runs_on_every_push_whatever_changed_and_names_the_fix() {
    let unrelated = json!({"conformance": false, "apps": [], "packages": {}});
    for file in [
        "apps/dyno/paddock.ci.json",
        "conformance/src/checks/C138-new.mjs",
        "conformance/paddock.ci.json",
        "docs/gates.toml",
        ".gitea/workflows/ci.yml",
        ".gitea/actions/app-gates/action.yml",
        "package.json",
        "crates/paddock-proc/src/lib.rs",
        "docs/x.md",
    ] {
        let planned = plan_gates(&unrelated, &change(&[file]), &facts());
        let g = run_gates(&planned)
            .into_iter()
            .find(|g| g.id == "gates-register")
            .cloned()
            .unwrap_or_else(|| panic!("gates-register is not planned for {file}: {planned:?}"));
        assert_eq!(
            (g.program.as_str(), g.args.join(" ")),
            ("/x/dev", "gates check".to_string()),
            "{file}"
        );
        assert!(!g.heavy && g.rank == 0, "a cheap gate: it starts before any Clippy");
        assert!(g.next.contains("::error") && g.next.contains("ADR 0060"), "{}", g.next);
    }
    let none = plan_gates(&unrelated, &change(&[]), &facts());
    assert!(ids(&none).contains(&"gates-register".to_string()), "also for an empty change");
}

/// The rows `./dev` runs itself never start a test program as paddock-dev (train 14, runs 8245 and 8248: `gates check` started the
/// unit-test harness, which ran every test named `gates` or `check` again inside the test that started it). In a test process the
/// program is unknown, so the rows are not measured, with the reason; never a guessed `./dev`.
#[test]
fn the_rows_dev_runs_itself_are_not_measured_when_this_program_is_a_test_program() {
    let why = crate::tool_cache::self_exe()
        .expect_err("a unit-test process is a test program")
        .to_string();
    let f = Facts {
        dev_exe: Err(why.clone()),
        ..facts()
    };
    let unrelated = json!({"conformance": false, "apps": [], "packages": {}});
    let planned = plan_gates(&unrelated, &change(&["crates/paddock-proc/src/lib.rs"]), &f);
    let rows = ["gates-register", "conformance", "conformance-tests"];
    assert!(!ids(&planned).iter().any(|i| rows.contains(&i.as_str())), "{planned:?}");
    for id in rows {
        assert!(
            planned
                .iter()
                .any(|p| matches!(p, Planned::NotMeasured { id: i, reason, .. } if i == id && reason.contains(&why))),
            "{id} is not 'not measured' with the reason: {planned:?}"
        );
    }
    // Positive control: with the program known, the same plan runs all three.
    let known = plan_gates(&unrelated, &change(&["crates/paddock-proc/src/lib.rs"]), &facts());
    for id in rows {
        assert!(ids(&known).contains(&id.to_string()), "{id}: {known:?}");
    }
}

/// Doctests are off (user 2026-11-24; crates/README.md, "Doctests"): no push or member gate plans `cargo test --doc` for any workspace,
/// the foundation's included, and the table names the row as off, a skip, never a pass.
#[test]
fn no_doctest_row_is_planned_and_the_doctests_row_says_they_are_off() {
    let files = [
        "crates/paddock-proc/src/lib.rs",
        "apps/dyno/src/a.rs",
        "apps/pitwall/src/a.rs",
        "crates/Cargo.toml",
    ];
    for f in [
        facts(),
        Facts {
            member_gate: false,
            ..facts()
        },
    ] {
        let planned = plan_gates(&plan(), &change(&files), &f);
        for g in run_gates(&planned) {
            assert!(
                !g.id.starts_with("doctest") && !(g.args.iter().any(|a| a == "--doc")),
                "{} runs doctests: {}",
                g.id,
                g.rerun()
            );
        }
        let reason = planned
            .iter()
            .find_map(|p| match p {
                Planned::Skip { id, reason } if id == "doctests" => Some(reason.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no doctests row: {planned:?}"));
        assert_eq!(reason, super::gates::DOCTESTS_OFF);
        let rows = run_all(&planned, &Fake::default(), 2);
        let report = render("h", &rows);
        let row = report.data["gates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == "doctests")
            .cloned()
            .unwrap();
        assert_eq!(row["verdict"], "skipped", "{row}");
    }
    // Positive control: a change that reaches no Cargo workspace has no doctests row (it is named only where a workspace is reached).
    let none = json!({"conformance": false, "apps": [], "packages": {}});
    let planned = plan_gates(&none, &change(&["docs/x.md"]), &facts());
    assert!(
        !planned.iter().any(|p| matches!(p, Planned::Skip { id, .. } if id == "doctests")),
        "{planned:?}"
    );
}

/// ROW_SET is what a pass record is compared with: it must list every row the planner can add besides the per-workspace ones, so a
/// new row cannot leave an older pass usable.
#[test]
fn the_row_set_of_a_pass_record_is_every_fixed_row_the_planner_can_plan() {
    let c = Change {
        files: vec![
            "crates/paddock-proc/src/lib.rs".into(),
            "crates/paddock-proc/Cargo.toml".into(),
            "apps/dyno/crates/dyno-core/src/a.rs".into(),
        ],
        ranges: vec!["abc..HEAD".into()],
        ..Change::default()
    };
    let planned = plan_gates(&plan(), &c, &facts());
    let mut fixed: Vec<String> = ids(&planned)
        .into_iter()
        .filter(|id| !super::gates::PER_WORKSPACE_PREFIXES.iter().any(|p| id.starts_with(p)))
        .collect();
    fixed.sort();
    let mut want: Vec<String> = super::gates::ROW_SET.iter().map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(fixed, want, "add the new row to gates::ROW_SET");
}

/// The shared-crate dependency guard runs when a manifest, a lockfile, the hack config or the pins file changed, with `./dev locks guard`
/// (ER-41: the files that can bring in its class), and a push of anything else skips it with the reason.
#[test]
fn the_locks_guard_row_runs_for_a_manifest_a_lockfile_the_hack_config_or_the_pins_file() {
    for file in [
        "crates/paddock-repo-graph/Cargo.toml",
        "apps/dyno/Cargo.lock",
        "crates/.config/hakari.toml",
        "docs/shared-crate-features.toml",
    ] {
        let planned = plan_gates(&plan(), &change(&[file]), &facts());
        let row = planned
            .iter()
            .find_map(|p| match p {
                Planned::Run(g) if g.id == "locks-guard" => Some(g),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{file}: {:?}", ids(&planned)));
        assert_eq!(row.args, ["locks", "guard"], "{file}");
    }
    let planned = plan_gates(&plan(), &change(&["crates/paddock-repo-graph/src/lib.rs", "docs/x.md"]), &facts());
    assert!(
        planned
            .iter()
            .any(|p| matches!(p, Planned::Skip { id, reason } if id == "locks-guard" && reason.contains("Cargo.toml"))),
        "{:?}",
        ids(&planned)
    );
}

#[test]
fn what_the_change_does_not_reach_is_skipped_with_a_reason() {
    let docs = json!({"conformance": false, "apps": [], "packages": {}});
    let c = Change {
        files: vec!["docs/x.md".into()],
        ranges: vec![],
        ..Change::default()
    };
    let planned = plan_gates(&docs, &c, &facts());
    // Conformance, its tests and the gate register run on every push, whatever the plan says (ER-41); the rest of the fixed rows
    // either run too (a row that runs whatever changed, like the member audits) or are skipped with a reason. The list is read
    // from ROW_SET and the planner's own answer, never typed here (ER-42): a new row cannot break this test.
    let run = ids(&planned);
    for always in ["conformance", "conformance-tests", "gates-register"] {
        assert!(run.contains(&always.to_string()), "{always} runs on every push: {run:?}");
    }
    let skipped_ids: Vec<String> = planned
        .iter()
        .filter_map(|p| {
            if let Planned::Skip { id, .. } = p {
                Some(id.as_str())
            } else {
                None
            }
        })
        .flat_map(|id| id.split(", "))
        .map(String::from)
        .collect();
    for row in super::gates::ROW_SET {
        assert!(
            run.iter().any(|r| r == row) || skipped_ids.iter().any(|s| s == row),
            "{row} is neither run nor skipped with a reason for a docs change: run {run:?}, skipped {skipped_ids:?}"
        );
    }
    assert!(
        run.iter().all(|r| super::gates::ROW_SET.contains(&r.as_str())),
        "a docs change plans only fixed rows: {run:?}"
    );
    let skipped: Vec<&String> = planned
        .iter()
        .filter_map(|p| if let Planned::Skip { reason, .. } = p { Some(reason) } else { None })
        .collect();
    assert!(skipped.len() >= 5 && skipped.iter().all(|r| !r.is_empty()), "{skipped:?}");
    // An app-only change: Windows only where CI lints it.
    let app = plan_gates(&plan(), &change(&["apps/debrief/src/a.rs"]), &facts());
    let got = ids(&app);
    assert!(
        !got.contains(&"clippy-windows apps/debrief".to_string()),
        "debrief has no windowsClippy: {got:?}"
    );
}

/// A workspace without a `paddock.ci.json` (tools/paddock-dev) says why it has no Windows lint: CI's own job lints it natively on
/// its Windows leg. The app wording stays for apps, which can opt in with `windowsClippy`.
#[test]
fn the_windows_skip_reason_names_the_real_cause_per_workspace() {
    assert!(windows_skip_reason("tools/paddock-dev").contains("Windows leg natively"));
    assert!(!windows_skip_reason("tools/paddock-dev").contains("windowsClippy"));
    assert!(windows_skip_reason("apps/debrief").contains("windowsClippy"));
}

/// The Windows Clippy (xwin, native on the Mac) also covers the developer tools (user, 2026-11-26: four Windows-only CI failures in
/// `tools/paddock-dev` in one day, the latest an import used only by `cfg(unix)` tests). The plan selects it like the other rows: a tool
/// the change reaches gets the row, as a heavy row (so it runs beside the others and reuses sccache through the gate's wrapper) in its
/// own `xwin` build folder; a tool the change does not reach gets none.
#[test]
fn the_windows_lint_runs_for_paddock_dev_and_throttle_when_the_plan_reaches_them() {
    let mut p = plan();
    p["packages"]["tools/paddock-dev"] = json!(["paddock-dev"]);
    p["packages"]["tools/throttle"] = json!(["throttle"]);
    let f = facts();
    for (tool, files) in [
        ("tools/paddock-dev", &["tools/paddock-dev/src/pre_push/tests.rs"][..]),
        ("tools/throttle", &["tools/throttle/src/main.rs"][..]),
    ] {
        let planned = plan_gates(&p, &change(files), &f);
        let row = run_gates(&planned)
            .into_iter()
            .find(|g| g.id == format!("clippy-windows {tool}"))
            .unwrap_or_else(|| panic!("no Windows lint planned for {tool}: {:?}", ids(&planned)));
        // The flags that make an import used only by `cfg(unix)` tests red: the tests are compiled (`--all-targets`) and warnings deny.
        assert_eq!(
            row.args.join(" "),
            "xwin clippy --config build.target-dir='/t/xwin' --target x86_64-pc-windows-msvc --workspace --all-targets --locked -- -D warnings",
            "{tool}"
        );
        assert_eq!(
            (row.cwd.as_str(), row.heavy),
            (tool, true),
            "a heavy row in the tool's own folder: it runs beside the others"
        );
        assert_eq!(row.need, Need::Tool("cargo-xwin".into()));
        assert!(
            row.build_dir.as_deref().is_some_and(|d| d.ends_with("/xwin")),
            "{:?}",
            row.build_dir
        );
        assert!(
            row.env.iter().all(|(k, _)| !k.starts_with("CARGO_")),
            "sccache hashes CARGO_* variables"
        );
    }
    // Selected by the plan: a change that reaches neither tool plans neither row; an app without `windowsClippy` keeps its own reason.
    let planned = plan_gates(&p, &change(&["tools/paddock-dev/src/a.rs", "apps/debrief/src/a.rs"]), &f);
    assert!(
        !ids(&planned).iter().any(|i| i == "clippy-windows tools/throttle"),
        "{:?}",
        ids(&planned)
    );
    let app = planned
        .iter()
        .find_map(|g| match g {
            Planned::Skip { id, reason } if id == "clippy-windows apps/debrief" => Some(reason.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no Windows skip for debrief: {planned:?}"));
    assert!(app.contains("windowsClippy"), "{app}");
}

/// ER-41 (user, 2026-11-14): a branch's touched-file tests passed and the full conformance run failed (lib-usage.test.mjs on
/// fix/spotter-release-plan-verdict; train 8's registry entry that sliced ci.yml with sed). The push gate runs the conformance row
/// and the conformance tests on every push, even one the plan says does not reach them.
#[test]
fn conformance_and_its_tests_run_on_every_push_whatever_changed() {
    let unrelated = json!({"conformance": false, "apps": [], "packages": {}});
    for files in [&["docs/x.md"][..], &["apps/dyno/app/src/a.ts"], &["Mystery"], &[]] {
        let planned = plan_gates(&unrelated, &change(files), &facts());
        let by = |id: &str| run_gates(&planned).into_iter().find(|g| g.id == id).cloned();
        let row = by("conformance").unwrap_or_else(|| panic!("conformance is not run for {files:?}: {planned:?}"));
        assert_eq!(
            (row.program.as_str(), row.args.join(" ")),
            ("/x/dev", "ci run conformance paddock".to_string())
        );
        let tests = by("conformance-tests").unwrap_or_else(|| panic!("the conformance tests are not run for {files:?}: {planned:?}"));
        assert_eq!(
            (tests.program.as_str(), tests.args.join(" "), tests.cwd.as_str()),
            ("/x/dev", "ci run conformance tests".to_string(), ""),
            "CI's own step (`./dev ci run conformance tests`, the `tests` list of conformance/paddock.ci.json)"
        );
        assert_eq!(tests.rank, 0, "a cheap gate: it starts before any Clippy");
        assert_eq!(
            tests.env,
            [
                ("PADDOCK_TEST_CONCURRENCY".to_string(), "4".to_string()),
                ("PADDOCK_GATE_TARGET_DIR".to_string(), "/t".to_string())
            ],
            "a share of the machine beside the Clippy rows (ER-24), not one test process per core"
        );
    }
}

/// The member gate of `./dev merge check --gates` runs this push gate on the merge commit, so the audits are in the member gate
/// when they are in this plan for every change (ER-41; user, 2026-11-17).
#[test]
fn the_member_audits_run_in_the_member_gate_whatever_changed_through_the_one_declared_profile_and_block() {
    let unrelated = json!({"conformance": false, "apps": [], "packages": {}});
    for files in [&["docs/x.md"][..], &["tools/paddock-dev/src/a.rs"], &["Mystery"], &[]] {
        let planned = plan_gates(&unrelated, &change(files), &facts());
        let g = run_gates(&planned)
            .into_iter()
            .find(|g| g.id == "member-audits")
            .cloned()
            .unwrap_or_else(|| panic!("the member audits are not run for {files:?}: {planned:?}"));
        assert_eq!(
            (g.program.as_str(), g.cwd.as_str(), g.args.join(" ")),
            (
                "cargo",
                "tools/paddock-dev",
                format!("nextest run --config build.target-dir='/t' --profile {AUDIT_PROFILE} --locked --no-tests=fail")
            ),
            "the audits are selected by the profile's declared filter, not by a list here (ER-42); an empty selection fails (ER-33)"
        );
        assert!(
            !g.env.iter().any(|(k, _)| k == "CARGO_TARGET_DIR" || k == "CARGO_BUILD_TARGET_DIR"),
            "the build folder is cargo configuration, never a CARGO_TARGET_DIR variable (sccache hashes it; cargo_target_lint): {:?}",
            g.env
        );
        assert!(g.heavy, "it compiles the test binary: through heavy with its core cap (ER-24)");
        assert_eq!(
            g.need,
            Need::Tool("cargo-nextest".into()),
            "no nextest is not measured, never a pass"
        );
        assert!(
            g.env.contains(&("RUSTUP_AUTO_INSTALL".to_string(), "0".to_string())),
            "ER-45: {:?}",
            g.env
        );
        assert!(g.next.contains(AUDIT_PROFILE), "{}", g.next);
    }
}

/// The source audits (the tool's own tests named `no_source_*`) are in the member gate by one name rule in the declared profile, not
/// by a list (ER-42): the rule selects every one of them. No process is started (a test restarting its own harness is a use the
/// self_exe audit rightly lists): the audits are found by reading the tool's own `src` for their names, as the audits themselves
/// read it, and matched against the filter's regexes. The positive control takes the rule's clause out of the REAL filter: both audits
/// drop out of the selection, so the gate would run none of them (and `--no-tests=fail` does not notice while the `it` audits run).
#[test]
fn the_member_gate_runs_the_source_audits_by_their_name_rule() {
    use super::gates::split_audit_filter;
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    let mut stack = vec![src];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                files.push(p);
            }
        }
    }
    assert!(files.len() > 50, "the scan read {} files: wrong folder?", files.len());
    let found = regex::Regex::new(concat!("fn ", "(no_source_file_[a-z0-9_]+)\\(")).unwrap();
    let mut names: Vec<String> = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        names.extend(
            found
                .captures_iter(&text)
                .map(|c| format!("{}::{}", f.file_stem().unwrap().to_string_lossy(), &c[1])),
        );
    }
    assert!(
        [
            "tool_cache::no_source_file_reads_its_own_path",
            "git_env::no_source_file_starts_git"
        ]
        .iter()
        .all(|n| names.iter().any(|x| x.starts_with(n))),
        "the scan found {names:?}"
    );
    let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".config/nextest.toml")).unwrap();
    let doc: toml::Table = text.parse().unwrap();
    let whole = doc["profile"][AUDIT_PROFILE]["default-filter"].as_str().unwrap().to_string();
    let selected = |filter: &str| -> Vec<String> {
        let (_, rest) = split_audit_filter(filter).expect("the filter starts with its binary(it) group");
        let re: Vec<regex::Regex> = rest
            .split("test(/")
            .skip(1)
            .map(|c| regex::Regex::new(c.split("/)").next().unwrap()).unwrap())
            .collect();
        names.iter().filter(|n| re.iter().any(|r| r.is_match(n))).cloned().collect()
    };
    assert_eq!(
        selected(&whole).len(),
        names.len(),
        "every source audit is selected: {:?}",
        selected(&whole)
    );
    // The control: the real filter without the rule's clause selects none of them.
    let clause = "| test(/::no_source_file_/)";
    assert!(whole.contains(clause), "the rule's clause is in the profile: {whole}");
    let without = whole.replacen(clause, "", 1);
    assert!(
        selected(&without).is_empty(),
        "without the clause the audits drop out: {:?}",
        selected(&without)
    );
}

/// Every `kind(lib) & test(/regex/)` clause of the member-audits filter selects at least one test of the lib, so a renamed lib audit
/// (the ownership test, train 2026-11-26-c) cannot drop out of the member gate silently: `--no-tests=fail` only sees an empty selection
/// as a whole (ER-33). The lib's test names are read from its sources (a test never starts its own test program: tool_cache's
/// `no_source_file_reads_its_own_path_except_through_self_exe`): each `#[test]` function as `<module path>::<name>` and
/// `<module path>::tests::<name>`. The control: a clause whose regex names no test is found empty.
#[test]
fn every_lib_clause_of_the_member_audits_selects_a_test_of_the_lib() {
    use super::gates::split_audit_filter;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(root.join(".config/nextest.toml")).unwrap();
    let doc: toml::Table = text.parse().unwrap();
    let whole = doc["profile"][AUDIT_PROFILE]["default-filter"].as_str().unwrap().to_string();
    let (_, rest) = split_audit_filter(&whole).expect("the filter starts with its binary(it) group");
    let clauses: Vec<String> = rest
        .split("kind(lib)")
        .skip(1)
        .filter_map(|c| c.split("test(/").nth(1).and_then(|r| r.split("/)").next()).map(str::to_string))
        .collect();
    assert!(!clauses.is_empty(), "the filter has a kind(lib) clause: {rest}");
    let src = root.join("src");
    let test_fn = regex::Regex::new(r"#\[test\]\s*(?:#\[[^\]]*\]\s*)*fn ([a-z0-9_]+)\(").unwrap();
    let mut names = Vec::new();
    let mut stack = vec![src.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if p.extension().is_none_or(|x| x != "rs") {
                continue;
            }
            let rel = p.strip_prefix(&src).unwrap().with_extension("");
            let mut parts: Vec<String> = rel.iter().map(|c| c.to_string_lossy().to_string()).collect();
            if parts.last().is_some_and(|l| l == "mod" || l == "lib" || l == "main") {
                parts.pop();
            }
            let module = parts.join("::");
            for c in test_fn.captures_iter(&std::fs::read_to_string(&p).unwrap()) {
                let prefix = if module.is_empty() { String::new() } else { format!("{module}::") };
                names.push(format!("{prefix}{}", &c[1]));
                names.push(format!("{prefix}tests::{}", &c[1]));
            }
        }
    }
    assert!(names.len() > 200, "the lib's tests were read: {}", names.len());
    let selects = |re: &str| {
        let re = regex::Regex::new(re).unwrap_or_else(|e| panic!("`{re}` is not a regex: {e}"));
        names.iter().any(|n| re.is_match(n))
    };
    for c in &clauses {
        assert!(
            selects(c),
            "the {AUDIT_PROFILE} lib clause `{c}` matches no test of the lib: renamed or removed (.config/nextest.toml)"
        );
    }
    assert!(
        selects("^repo_model::tests::graph_every_tracked_file_of_the_tree_has_an_owner"),
        "the reader finds the ownership test"
    );
    assert!(
        !selects("^repo_model::tests::graph_every_tracked_file_renamed_away"),
        "the control finds an empty clause"
    );
}

/// A developer's push does not run the audits (minutes would be added to every push); it says why. The member gate does.
#[test]
fn a_developers_push_skips_the_member_audits_with_a_reason_and_the_member_gate_runs_them() {
    let off = Facts {
        member_gate: false,
        ..facts()
    };
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &off);
    assert!(!ids(&planned).contains(&"member-audits".to_string()), "{:?}", ids(&planned));
    assert!(
        planned
            .iter()
            .any(|p| matches!(p, Planned::Skip { id, reason } if id == "member-audits" && reason.contains("--member-gate"))),
        "skipped with its reason: {planned:?}"
    );
    let on = Facts {
        member_gate: true,
        ..facts()
    };
    assert!(ids(&plan_gates(&plan(), &change(&["crates/a.rs"]), &on)).contains(&"member-audits".to_string()));
}

/// ADR 0123 §7: the job-model check is a member-gate row (a layout change fails before a train, ER-9, ER-53), runs `./dev ci job-model check`
/// whatever changed (ER-41: ci.yml's layout can be broken from the planner or an action), and a developer's push skips it with the
/// reason. Removing the row fails this test (ER-33).
#[test]
fn the_job_model_row_runs_in_the_member_gate_whatever_changed_and_a_developers_push_skips_it() {
    let on = plan_gates(
        &plan(),
        &change(&["crates/a.rs"]),
        &Facts {
            member_gate: true,
            ..facts()
        },
    );
    let row = on
        .iter()
        .find_map(|p| match p {
            Planned::Run(g) if g.id == "job-model" => Some(g),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the member gate has a job-model row: {:?}", ids(&on)));
    assert_eq!(row.args, ["ci", "job-model", "check"]);
    let off = plan_gates(
        &plan(),
        &change(&["crates/a.rs"]),
        &Facts {
            member_gate: false,
            ..facts()
        },
    );
    assert!(
        off.iter()
            .any(|p| matches!(p, Planned::Skip { id, reason } if id == "job-model" && reason.contains("--member-gate"))),
        "{off:?}"
    );
}

/// A failing audit fails the gate (exit 1, one `::error::` line with the rerun command), and a runner without nextest or one that
/// cannot start it is never a pass.
#[test]
fn a_failing_member_audit_fails_the_gate_and_a_missing_nextest_is_not_measured() {
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &facts());
    let rows = run_all(
        &planned,
        &Fake {
            fail: vec!["member-audits".into()],
            ..Fake::default()
        },
        2,
    );
    let report = render("h", &rows);
    assert_eq!(report.exit, Exit::Findings, "{}", report.text);
    let line = report
        .text
        .lines()
        .find(|l| l.contains("::error::member-audits"))
        .unwrap_or_else(|| panic!("{}", report.text));
    assert!(
        line.contains(&format!(
            "Re-run alone: cd tools/paddock-dev && cargo nextest run --config {} --profile member-audits",
            super::gates::shell_quote("build.target-dir='/t'")
        )),
        "{line}"
    );
    let rows = run_all(
        &planned,
        &Fake {
            no_tools: vec!["cargo-nextest".into()],
            ..Fake::default()
        },
        2,
    );
    assert!(nm(&verdicts(&rows)["member-audits"]).is_some(), "not measured, never passed");
    assert_eq!(render("h", &rows).exit, Exit::NotMeasured);
    // The positive control: with every gate green the row passes, so the two verdicts above come from the audit and not from the harness.
    let rows = run_all(&planned, &Fake::default(), 2);
    assert!(passed(&verdicts(&rows)["member-audits"]));
}

#[test]
fn a_unreadable_conformance_line_makes_the_check_broken_not_passed() {
    let mut f = facts();
    f.conformance = Some(Err("ci.yml has no line".into()));
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &f);
    assert!(
        planned
            .iter()
            .any(|p| matches!(p, Planned::Broken { id, .. } if id == "conformance"))
    );
    let rows = run_all(&planned, &Fake::default(), 2);
    assert_eq!(render("h", &rows).exit, Exit::CheckBroken);
}

#[derive(Default)]
struct Fake {
    no_tools: Vec<String>,
    fail: Vec<String>,
    unstartable: Vec<String>,
    panics: Vec<String>,
    cut_short: Vec<String>,
    /// What every gate that ran reports as its queue and run time (as `heavy`'s timing line would).
    timing: Option<Timing>,
    /// Gates that never got a slot: `how` is heavy's queue-limit text.
    no_slot: Vec<String>,
    /// Gates whose queue stalled: heavy's log of the step (with the line that names the holder) is this file.
    stalled: Option<(Vec<String>, String)>,
    ran: Mutex<Vec<String>>,
    /// The stand-in for this program that the rows `./dev` runs itself start (None: the real answer, a test program is refused).
    dev_program: Option<String>,
    /// The limits every gate was started with.
    limits: Mutex<Vec<(String, Limits)>>,
}

impl Exec for Fake {
    fn dev_program(&self) -> Result<String, String> {
        self.dev_program.clone().map_or_else(
            || {
                crate::tool_cache::self_exe()
                    .map(|p| p.to_string_lossy().to_string())
                    .map_err(|e| e.to_string())
            },
            Ok,
        )
    }
    fn has_tool(&self, p: &str) -> bool {
        !self.no_tools.iter().any(|t| t == p)
    }
    fn run(&self, g: &Gate, limits: Limits) -> Result<Done, String> {
        self.limits.lock().unwrap().push((g.id.clone(), limits));
        self.ran.lock().unwrap().push(g.id.clone());
        assert!(!self.panics.contains(&g.id), "a runner bug");
        if self.unstartable.contains(&g.id) {
            return Err(format!("{} is not installed on this runner", g.program));
        }
        if self.cut_short.contains(&g.id) {
            return Ok(Done {
                success: false,
                how: "timed out after 900 s".into(),
                tail: String::new(),
                log: Some("/logs/x.log".into()),
                timing: None,
                ending: crate::pre_push::exec::Ending::RowCap,
            });
        }
        if let Some((ids, log)) = &self.stalled
            && ids.contains(&g.id)
        {
            return Ok(Done {
                success: false,
                how: "no progress of the queue for 1800 s".into(),
                tail: String::new(),
                log: Some(log.clone()),
                timing: Some(Timing { queued: 1801.2, ran: 0.0 }),
                ending: crate::pre_push::exec::Ending::QueueLimit,
            });
        }
        if self.no_slot.contains(&g.id) {
            return Ok(Done {
                success: false,
                how: "queue limit: no slot within 30 s".into(),
                tail: String::new(),
                log: Some("/logs/q.log".into()),
                timing: Some(Timing { queued: 31.0, ran: 0.0 }),
                ending: crate::pre_push::exec::Ending::QueueLimit,
            });
        }
        let fail = self.fail.iter().any(|f| f == &g.id);
        Ok(Done {
            success: !fail,
            how: if fail { "exit 1".into() } else { "exit 0".into() },
            tail: if fail { "error: boom".into() } else { String::new() },
            log: fail.then(|| format!("/logs/{}.log", g.id)),
            timing: self.timing,
            ending: crate::pre_push::exec::Ending::Exited,
        })
    }
}

fn run_all(planned: &[Planned], exec: &dyn Exec, workers: usize) -> Vec<(String, Row)> {
    super::exec::run_all(planned, exec, workers, &test_subject())
}

/// A row's reason and next step when it is a plain not-measured row.
fn nm(row: &Row) -> Option<(String, String)> {
    (row.kind() == Kind::NotMeasured).then(|| (row.reason(), row.next.clone()))
}

fn passed(row: &Row) -> bool {
    row.kind() == Kind::Passed
}

fn verdicts(rows: &[(String, Row)]) -> std::collections::BTreeMap<String, Row> {
    rows.iter().cloned().collect()
}

#[test]
fn a_missing_xwin_is_not_measured_with_the_install_line() {
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &facts());
    let rows = run_all(
        &planned,
        &Fake {
            no_tools: vec!["cargo-xwin".into()],
            ..Fake::default()
        },
        2,
    );
    assert!(matches!(nm(&verdicts(&rows)["clippy-windows crates"]), Some((_, next)) if next.contains("cargo install cargo-xwin")));
}

/// The JSON a reader gets names a failed row's cause and its step log, not the verdict alone (ER-27): `merge sync-member` read only
/// the verdict, so train z's failed `gates-register` printed no reason and no log. A pass carries neither.
#[test]
fn a_failed_row_names_its_cause_and_its_log_in_the_json() {
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &facts());
    let rows = run_all(
        &planned,
        &Fake {
            fail: vec!["fmt crates".into()],
            ..Fake::default()
        },
        2,
    );
    let report = render("h", &rows);
    let gates = report.data["gates"].as_array().expect("gates");
    let row = |id: &str| gates.iter().find(|g| g["id"] == id).unwrap_or_else(|| panic!("{id}: {gates:?}"));
    let failed = row("fmt crates");
    assert_eq!(failed["verdict"], "failed");
    assert!(failed["reason"].as_str().is_some_and(|r| r.contains("exit 1")), "{failed}");
    assert_eq!(failed["log"], "/logs/fmt crates.log", "{failed}");
    assert!(failed["next"].as_str().is_some_and(|n| n.contains("Re-run alone")), "{failed}");
    let passed = row("lockfile crates");
    assert!(passed.get("reason").is_none() && passed.get("log").is_none(), "{passed}");
}

#[test]
fn a_failure_is_one_line_with_the_cause_the_next_step_and_the_rerun_command() {
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &facts());
    let rows = run_all(
        &planned,
        &Fake {
            fail: vec!["fmt crates".into()],
            ..Fake::default()
        },
        2,
    );
    let report = render("h", &rows);
    assert_eq!(report.exit, Exit::Findings);
    let line = report
        .text
        .lines()
        .find(|l| l.contains("::error::fmt crates"))
        .unwrap_or_else(|| panic!("{}", report.text));
    assert!(
        line.contains("exit 1")
            && line.contains("Next step: format your own files")
            && line.contains("Re-run alone: cd crates && cargo fmt --all --check"),
        "{line}"
    );
    assert!(report.text.contains("error: boom"), "the tail of the output is shown");
    // A failure beats a not-measured row; a broken check beats both.
    let rows = run_all(
        &planned,
        &Fake {
            fail: vec!["fmt crates".into()],
            ..Fake::default()
        },
        2,
    );
    assert_eq!(render("h", &rows).exit, Exit::Findings);
}

#[test]
fn everything_green_exits_zero_and_cheap_gates_start_first() {
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &facts());
    let fake = Fake::default();
    let rows = run_all(&planned, &fake, 1);
    assert_eq!(render("h", &rows).exit, Exit::Ok);
    let ran = fake.ran.lock().unwrap().clone();
    let first_clippy = ran.iter().position(|r| r.starts_with("clippy")).unwrap();
    let last_cheap = ran
        .iter()
        .rposition(|r| r.starts_with("fmt") || r == "conformance" || r == "commit-lint")
        .unwrap();
    assert!(
        last_cheap < first_clippy,
        "fmt, conformance and commit-lint before any Clippy: {ran:?}"
    );
}

#[test]
fn the_conformance_gate_runs_the_contracts_list_and_a_broken_contract_is_broken_never_a_pass() {
    // The real tree: the arguments the gate runs are the contract's (the one copy CI's `./dev ci run conformance paddock` runs).
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let args = crate::ci_run::contract::conformance_args_at(&root).unwrap();
    let f = Facts {
        conformance: Some(Ok(args.clone())),
        ..facts()
    };
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &f);
    let g = run_gates(&planned).into_iter().find(|g| g.id == "conformance").unwrap();
    assert_eq!(
        (g.program.as_str(), g.args.clone()),
        ("/x/dev", ["ci", "run", "conformance", "paddock"].map(String::from).to_vec()),
        "through the executor, like CI"
    );
    assert!(!args.is_empty());
    // An unreadable contract: the gate is BROKEN (exit 7), it does not silently run nothing.
    let f = Facts {
        conformance: Some(Err("conformance/paddock.ci.json cannot be read".into())),
        ..facts()
    };
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &f);
    assert!(
        planned
            .iter()
            .any(|p| matches!(p, Planned::Broken { id, .. } if id == "conformance"))
    );
}

/// The host and xwin Clippy read cargo's JSON messages for progress and name the build folder they use (so a cold one is
/// told from a warm one); every other row reads output lines. The folder is the one the row is given (`--config build.target-dir`).
#[test]
fn the_cargo_rows_read_json_progress_and_name_their_build_folder() {
    use crate::pre_push::progress::Progress;
    let mut f = facts();
    f.target_dir = None;
    f.kept_targets = Some("/kept".into());
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &f);
    let get = |id: &str| {
        planned
            .iter()
            .find_map(|p| match p {
                Planned::Run(g) if g.id == id => Some(g.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no row {id}"))
    };
    for id in ["clippy crates", "clippy-windows crates"] {
        assert_eq!(get(id).progress, Progress::CargoJson, "{id}");
    }
    assert_eq!(get("clippy crates").build_dir.as_deref(), Some("/kept/crates"));
    assert_eq!(get("clippy-windows crates").build_dir.as_deref(), Some("/kept/crates/xwin"));
    assert_eq!(get("conformance-tests").progress, Progress::Lines);
    assert_eq!(get("conformance-tests").build_dir, None);
}

fn crates_gates() -> Vec<Planned> {
    plan_gates(&plan(), &change(&["crates/a.rs"]), &facts())
}

#[test]
fn a_gate_that_cannot_start_is_not_measured_never_passed() {
    let rows = run_all(
        &crates_gates(),
        &Fake {
            unstartable: vec!["fmt crates".into()],
            ..Fake::default()
        },
        2,
    );
    assert!(matches!(nm(&verdicts(&rows)["fmt crates"]), Some((reason, _)) if reason.contains("not installed")));
    assert_eq!(render("h", &rows).exit, Exit::NotMeasured);
}

#[test]
fn a_crashed_runner_and_a_row_that_never_ran_are_not_measured() {
    let fake = Fake {
        panics: vec!["fmt crates".into()],
        ..Fake::default()
    };
    let rows = run_all(&crates_gates(), &fake, 2);
    let v = verdicts(&rows);
    assert!(matches!(nm(&v["fmt crates"]), Some((reason, _)) if reason.contains("crashed")));
    assert!(passed(&v["lockfile apps/dyno"]), "the other gates still ran");
    // A signal stops the rest: nothing starts, every gate that would run is not measured, none passes.
    let fake = Fake::default();
    let rows = run_all_with(
        &crates_gates(),
        &fake,
        &RunOpts {
            workers: 1,
            queue_stall: QUEUE_STALL,
            stop: &AtomicBool::new(true),
            subject: &test_subject(),
            reuse: None,
        },
    );
    assert!(fake.ran.lock().unwrap().is_empty());
    assert!(matches!(nm(&verdicts(&rows)["fmt crates"]), Some((reason, _)) if reason.contains("interrupted")));
}

#[test]
fn a_timeout_is_not_measured_and_does_not_tell_you_to_fix_lints() {
    let rows = run_all(
        &crates_gates(),
        &Fake {
            cut_short: vec!["clippy crates".into()],
            ..Fake::default()
        },
        2,
    );
    let Some((reason, next)) = nm(&verdicts(&rows)["clippy crates"]) else {
        panic!("{rows:?}")
    };
    assert!(
        reason.contains("timed out")
            && reason.contains("/logs/x.log")
            && next.contains("re-run it alone")
            && !next.contains("fix the lints"),
        "{reason} {next}"
    );
    assert_eq!(render("h", &rows).exit, Exit::NotMeasured);
}

/// A gate that ran and exited 0, as a row.
fn ran_ok(id: &str, secs: f64) -> Row {
    let done = Done {
        success: true,
        how: "exit 0".into(),
        tail: String::new(),
        log: None,
        timing: None,
        ending: crate::pre_push::exec::Ending::Exited,
    };
    Row::from_run(id, &test_subject(), &done, "", secs)
}

/// ADR 0089: the row reads how the run ended; a failed run is a finding, never a pass (delete `done.success` and this goes red).
#[test]
fn a_row_from_a_run_reads_its_exit() {
    let run = |success: bool| Done {
        success,
        how: if success { "exit 0" } else { "exit 1" }.into(),
        tail: String::new(),
        log: None,
        timing: None,
        ending: crate::pre_push::exec::Ending::Exited,
    };
    assert_eq!(
        Row::from_run("g", &test_subject(), &run(true), "g (exit 1)", 1.0).kind(),
        Kind::Passed
    );
    let failed = Row::from_run("g", &test_subject(), &run(false), "g (exit 1)", 1.0);
    assert_eq!(failed.kind(), Kind::Failed);
    assert!(failed.reason().contains("g (exit 1)"));
}

#[test]
fn the_worst_finding_decides_the_exit_code_7_then_5_then_6() {
    let nm = Row::not_measured("r", "n");
    let failed = Row::failed("g", &test_subject(), "c".into(), String::new(), "n".into(), 1.0);
    let broken = Row::broken("c", "n");
    let row = |id: &str, v: &Row| (id.to_string(), v.clone());
    let all = [row("a", &nm), row("b", &failed), row("c", &broken)];
    assert_eq!(render("h", &all).exit, Exit::CheckBroken);
    assert_eq!(render("h", &all[..2]).exit, Exit::Findings);
    assert_eq!(render("h", &all[..1]).exit, Exit::NotMeasured);
    assert_eq!(render("h", &[row("x", &ran_ok("x", 0.1))]).exit, Exit::Ok);
}

#[test]
fn pitwalls_deny_needs_cargo_deny_and_an_empty_script_list_is_a_broken_check() {
    let planned = crates_gates();
    let deny = run_gates(&planned).into_iter().find(|g| g.id.starts_with("pitwall-deny")).unwrap();
    assert_eq!(deny.need, Need::Tool("cargo-deny".into()));
    let rows = run_all(
        &planned,
        &Fake {
            no_tools: vec!["cargo-deny".into()],
            ..Fake::default()
        },
        2,
    );
    assert!(matches!(nm(&verdicts(&rows)["pitwall-deny (licences, bans, sources)"]), Some((reason, _)) if reason.contains("cargo-deny")));
    let mut f = facts();
    f.pitwall_tests.clear();
    let planned = plan_gates(&plan(), &change(&["apps/pitwall/a.rs"]), &f);
    assert!(
        planned
            .iter()
            .any(|p| matches!(p, Planned::Broken { id, .. } if id == "pitwall-scripts"))
    );
    assert!(!ids(&planned).contains(&"pitwall-scripts".to_string()));
}

#[test]
fn the_table_names_what_ci_runs_that_this_gate_does_not() {
    let report = render("h", &run_all(&crates_gates(), &Fake::default(), 2));
    for what in [
        "hakari",
        "machete",
        "cargo deny outside Pitwall",
        "TypeScript",
        "Linux Clippy and Linux tests",
        "reached only through dependencies",
    ] {
        assert!(report.text.contains(what), "{what} missing:\n{}", report.text);
    }
}

#[test]
fn a_named_not_measured_gate_may_pass_and_the_use_is_logged() {
    let state = tempfile::tempdir().unwrap();
    // A developer's push, on a machine without cargo-xwin: the Windows Clippy is the one row that cannot be measured.
    let push = Facts {
        member_gate: false,
        ..facts()
    };
    let mut rows = run_all(
        &plan_gates(&plan(), &change(&["crates/a.rs"]), &push),
        &Fake {
            no_tools: vec!["cargo-xwin".into()],
            ..Fake::default()
        },
        2,
    );
    assert_eq!(render("h", &rows).exit, Exit::NotMeasured);
    // A name that is not the gate's accepts nothing.
    accept_not_measured(&mut rows, &["clippy-windows".into()], state.path());
    assert_eq!(render("h", &rows).exit, Exit::NotMeasured);
    accept_not_measured(&mut rows, &["clippy-windows crates".into()], state.path());
    let report = render("h", &rows);
    assert_eq!(report.exit, Exit::Ok);
    assert!(report.text.contains("ACCEPTED      clippy-windows crates") && report.text.contains("logged"));
    assert!(
        report.text.contains("Pushed with 1 accepted not-measured row(s)") && !report.text.contains("Safe to push"),
        "{}",
        report.text
    );
    let log = std::fs::read_to_string(state.path().join("overrides.log")).unwrap();
    assert!(log.contains("gate=clippy-windows crates") && log.contains("cargo-xwin"), "{log}");
}

#[test]
fn the_pushed_commits_are_read_from_gits_pre_push_lines() {
    let zero = "0".repeat(40);
    let input = format!(
        "refs/heads/a aaa111 refs/heads/a {zero}\nrefs/heads/gone {zero} refs/heads/gone bbb222\nrefs/heads/c ccc333 refs/heads/c ddd444\n"
    );
    assert_eq!(parse_refs(&input), ["aaa111", "ccc333"], "a deletion pushes no commit");
    assert!(parse_refs("").is_empty());
}

#[test]
fn several_pushed_ranges_are_linted_each() {
    let c = Change {
        files: vec!["crates/a.rs".into()],
        ranges: vec!["m1..a".into(), "m2..b".into()],
        ..Change::default()
    };
    let got = ids(&plan_gates(&plan(), &c, &facts()));
    assert!(
        got.contains(&"commit-lint m1..a".to_string()) && got.contains(&"commit-lint m2..b".to_string()),
        "{got:?}"
    );
}

#[test]
fn only_workspaces_with_a_changed_file_of_their_own_are_linted() {
    let planned = plan_gates(&plan(), &change(&["crates/paddock-proc/src/lib.rs"]), &facts());
    let got = ids(&planned);
    for want in [
        "fmt crates",
        "clippy crates",
        "clippy-windows crates",
        "lockfile apps/dyno",
        "lockfile apps/pitwall",
    ] {
        assert!(got.contains(&want.to_string()), "{want}: {got:?}");
    }
    for not in [
        "fmt apps/dyno",
        "clippy apps/dyno",
        "clippy-windows apps/dyno",
        "clippy apps/pitwall",
    ] {
        assert!(!got.contains(&not.to_string()), "{not} is a dependent's: CI covers it: {got:?}");
    }
    let skip = planned
        .iter()
        .find_map(|p| {
            if let Planned::Skip { id, reason } = p {
                (id == "clippy dependents").then_some(reason.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert!(skip.contains("apps/dyno") && skip.contains("CI covers"), "{skip}");
    // The longest workspace folder owns a file: a fuzz or sub-workspace file is not its parent's.
    let own = super::gates::own_workspaces(
        &plan(),
        &[
            "apps/debrief/src/a.rs".to_string(),
            "apps/dyno/notes.txt".to_string(),
            "docs/x.md".to_string(),
        ],
    );
    assert_eq!(own.into_iter().collect::<Vec<_>>(), ["apps/debrief", "apps/dyno"]);
}

#[test]
fn a_shell_word_is_quoted_so_the_printed_rerun_can_be_pasted() {
    assert_eq!(super::gates::shell_quote("a b"), "'a b'");
    assert_eq!(super::gates::shell_quote("it's"), "'it'\\''s'");
    assert_eq!(super::gates::shell_quote("plain-1.2/x"), "plain-1.2/x");
}

#[test]
fn the_override_names_full_row_ids_and_is_announced() {
    let state = tempfile::tempdir().unwrap();
    let mut rows = run_all(
        &crates_gates(),
        &Fake {
            no_tools: vec!["cargo-xwin".into()],
            ..Fake::default()
        },
        2,
    );
    // A first word (`clippy-windows`) or another row's id accepts nothing: only the exact row.
    accept_not_measured(
        &mut rows,
        &["clippy-windows".into(), "clippy-windows apps/dyno".into()],
        state.path(),
    );
    assert_eq!(render("h", &rows).exit, Exit::NotMeasured);
    let ids = super::accepted_ids(
        &["a b".into()],
        &std::collections::HashMap::from([("PADDOCK_ACCEPT_NOT_MEASURED".to_string(), " x , y,,".to_string())]),
    );
    assert_eq!(ids, ["a b", "x", "y"]);
}

/// A pushed commit is checked out on its own and removed after: a dirty main tree cannot change what is checked.
#[test]
fn a_pushed_commit_is_checked_out_in_a_throw_away_worktree_that_is_removed() {
    use crate::plan_repo::git_text;
    let d = tempfile::tempdir().unwrap();
    let main = d.path().join("main");
    std::fs::create_dir(&main).unwrap();
    let git = |args: &[&str]| git_text(&main, args).unwrap_or_else(|| panic!("git {args:?}"));
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    // The test is about which commit is checked out, not line endings: a Windows machine's `core.autocrlf=true` would give CRLF.
    git(&["config", "core.autocrlf", "false"]);
    std::fs::write(main.join("a.txt"), "pushed\n").unwrap();
    git(&["add", "a.txt"]);
    git(&["commit", "-q", "-m", "one"]);
    let sha = git(&["rev-parse", "HEAD"]).trim().to_string();
    std::fs::write(main.join("a.txt"), "dirty\n").unwrap();
    let path;
    {
        let c = super::Detached::new(&main, &d.path().join("wts"), &sha).unwrap();
        path = c.path.clone();
        assert_eq!(
            std::fs::read_to_string(path.join("a.txt")).unwrap(),
            "pushed\n",
            "the commit, not the dirty tree"
        );
    }
    assert!(!path.exists(), "removed after the run");
    assert_eq!(
        std::fs::read_to_string(main.join("a.txt")).unwrap(),
        "dirty\n",
        "the main tree was not touched"
    );
}

#[test]
fn a_kept_build_folder_per_workspace_is_used_when_the_caller_sets_none() {
    let mut f = facts();
    f.target_dir = None;
    f.kept_targets = Some("/work/pre-push/targets/".into());
    let planned = plan_gates(&plan(), &change(&["crates/a.rs", "apps/dyno/src/a.rs"]), &f);
    let args = |id: &str| run_gates(&planned).into_iter().find(|g| g.id == id).unwrap().args.join(" ");
    assert!(args("clippy crates").contains("--config build.target-dir='/work/pre-push/targets/crates'"));
    assert!(args("clippy apps/dyno").contains("--config build.target-dir='/work/pre-push/targets/apps-dyno'"));
    assert!(args("clippy-windows apps/dyno").contains("--config build.target-dir='/work/pre-push/targets/apps-dyno/xwin'"));
    assert!(
        run_gates(&planned)
            .iter()
            .all(|g| g.env.iter().all(|(k, _)| !k.starts_with("CARGO_"))),
        "never a CARGO_TARGET_DIR variable"
    );
    // The caller's own CARGO_TARGET_DIR is used as the configured folder (and dropped from the child's environment, see exec).
    f.target_dir = Some("/mine".into());
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &f);
    let g = run_gates(&planned).into_iter().find(|g| g.id == "clippy crates").unwrap().clone();
    assert!(g.env.is_empty() && g.args.join(" ").contains("--config build.target-dir='/mine'"));
}

#[test]
fn worktree_folders_name_their_run_by_pid_and_start_time_and_only_ended_runs_are_swept() {
    use super::{parse_tree_name, stale_trees, tree_name};
    let name = tree_name(4242, 1_700_000_000_123, "0123456789abcdef");
    assert_eq!(name, "pp-4242-1700000000123-0123456789ab");
    assert_eq!(parse_tree_name(&name), Some((4242, 1_700_000_000_123)));
    assert_eq!(parse_tree_name("not-ours"), None);
    let names = vec![name.clone(), tree_name(7, 99, "aaa"), "other".to_string()];
    // A pid that is alive with another start time is a different process: the leftover is stale.
    let alive = |pid: u32, start: u64| pid == 4242 && start == 1_700_000_000_123;
    assert_eq!(stale_trees(&names, &alive), [tree_name(7, 99, "aaa")]);
}

#[test]
fn the_pre_push_folder_is_never_in_the_home_folder() {
    use super::refuse_home;
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let h = home.to_str().unwrap();
    assert!(refuse_home(&home.join("work/pre-push"), Some(h)).is_err());
    assert!(refuse_home(&home, Some(h)).is_err());
    assert!(refuse_home(&d.path().join("disk/pre-push"), Some(h)).is_ok());
    assert!(refuse_home(&d.path().join("anywhere"), None).is_ok());
}

/// A crashed run's locked worktree is found and removed by the next run, a live one is left alone.
#[test]
fn a_leftover_locked_worktree_of_an_ended_run_is_swept_and_a_live_ones_is_kept() {
    use crate::plan_repo::git_text;
    let d = tempfile::tempdir().unwrap();
    let main = d.path().join("main");
    std::fs::create_dir(&main).unwrap();
    let git = |args: &[&str]| git_text(&main, args).unwrap_or_else(|| panic!("git {args:?}"));
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    std::fs::write(main.join("a.txt"), "x\n").unwrap();
    git(&["add", "a.txt"]);
    git(&["commit", "-q", "-m", "one"]);
    let sha = git(&["rev-parse", "HEAD"]).trim().to_string();
    let trees = d.path().join("trees");
    let crashed = super::Detached::new(&main, &trees, &sha).unwrap();
    let path = crashed.path.clone();
    std::mem::forget(crashed); // the process died: no Drop ran
    assert!(
        git(&["worktree", "list", "--porcelain"]).contains("locked"),
        "it is locked while its run lives"
    );
    assert_eq!(
        super::sweep_stale(&main, &trees, &|_, _| true),
        0,
        "a run that may be alive keeps its folder"
    );
    assert!(path.exists());
    assert_eq!(super::sweep_stale(&main, &trees, &|_, _| false), 1);
    assert!(!path.exists());
    assert!(!git(&["worktree", "list", "--porcelain"]).contains("pp-"));
}

#[test]
fn extra_pushed_refs_are_not_measured_rows_and_never_passes() {
    let rows = super::extra_ref_rows(&["aaaaaaaaaaaaaaaa".to_string(), "bbb".to_string()]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, "pushed commit aaaaaaaaaaaa");
    assert!(
        rows.iter()
            .all(|(_, v)| matches!(nm(v), Some((_, next)) if next.contains("one at a time")))
    );
    assert_eq!(render("h", &rows).exit, Exit::NotMeasured);
}

/// The sweep touches only its own `pp-*` entries under its own base: another agent's worktree whose folder is missing (an unmounted
/// drive) keeps its record, and a leftover of ours whose folder is gone is removed.
#[test]
fn the_sweep_never_drops_a_foreign_worktree_whose_folder_is_missing() {
    use crate::plan_repo::git_text;
    let d = tempfile::tempdir().unwrap();
    let main = d.path().join("main");
    std::fs::create_dir(&main).unwrap();
    let git = |args: &[&str]| git_text(&main, args).unwrap_or_else(|| panic!("git {args:?}"));
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    std::fs::write(main.join("a.txt"), "x\n").unwrap();
    git(&["add", "a.txt"]);
    git(&["commit", "-q", "-m", "one"]);
    let sha = git(&["rev-parse", "HEAD"]).trim().to_string();
    // Somebody else's worktree, unlocked, on a drive that is not mounted now.
    let foreign = d.path().join("other-drive").join("agent-wt");
    git(&["worktree", "add", "--detach", foreign.to_str().unwrap(), &sha]);
    std::fs::remove_dir_all(&foreign).unwrap();
    // Our own leftover, whose folder is gone too.
    let trees = d.path().join("trees");
    let crashed = super::Detached::new(&main, &trees, &sha).unwrap();
    let ours = crashed.path.clone();
    std::mem::forget(crashed);
    std::fs::remove_dir_all(&ours).unwrap();
    assert_eq!(super::sweep_stale(&main, &trees, &|_, _| false), 1);
    let list = git(&["worktree", "list", "--porcelain"]);
    assert!(list.contains("agent-wt"), "the foreign worktree's record survives:\n{list}");
    assert!(!list.contains("pp-"), "our leftover is removed:\n{list}");
}

/// A symlink, a relative path and `~` cannot hide a folder inside the home folder.
#[cfg(unix)]
#[test]
fn a_symlink_into_home_a_relative_path_and_a_tilde_are_refused() {
    use super::refuse_home;
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    std::fs::create_dir_all(home.join("work")).unwrap();
    let link = d.path().join("elsewhere");
    std::os::unix::fs::symlink(home.join("work"), &link).unwrap();
    let h = home.to_str().unwrap();
    assert!(refuse_home(&link.join("pre-push"), Some(h)).is_err(), "through a symlink");
    assert!(refuse_home(std::path::Path::new("relative/pre-push"), Some(h)).is_err());
    assert!(refuse_home(std::path::Path::new("~/pre-push"), Some(h)).is_err());
    let outside = d.path().join("disk").join("pre-push");
    assert!(refuse_home(&outside, Some(h)).is_ok());
    assert!(outside.exists(), "an allowed folder is created");
    // A refused folder is not created, not even empty.
    let inside = home.join("work/never/pre-push");
    assert!(refuse_home(&inside, Some(h)).is_err());
    assert!(!home.join("work/never").exists(), "nothing was made inside home");
    let through_link = link.join("also-never");
    assert!(refuse_home(&through_link, Some(h)).is_err());
    assert!(!home.join("work/also-never").exists());
}

mod known_commit {
    use super::super::known::{self, Evidence, Lookups};
    use super::super::{PrePushArgs, command_in, node_env_problem};
    use crate::plan_repo::git_text;
    use paddock_cli::Exit;
    use std::path::{Path, PathBuf};

    struct Fake {
        ls: Option<String>,
        ci: Result<u64, String>,
        asked: std::cell::RefCell<Vec<String>>,
    }
    impl Lookups for Fake {
        fn ls_remote(&self, remote: &str) -> Option<String> {
            self.asked.borrow_mut().push(format!("ls-remote {remote}"));
            self.ls.clone()
        }
        fn ci_green(&self, sha: &str) -> Result<u64, String> {
            self.asked.borrow_mut().push(format!("ci {sha}"));
            self.ci.clone()
        }
    }
    fn fake(ls: Option<&str>, ci: Result<u64, &str>) -> Fake {
        Fake {
            ls: ls.map(String::from),
            ci: ci.map_err(String::from),
            asked: Default::default(),
        }
    }

    fn args() -> PrePushArgs {
        PrePushArgs {
            root: None,
            base: None,
            remote: Some("origin".into()),
            only: vec![],
            dry_run: false,
            files: vec![],
            refs_from_stdin: true,
            member_gate: false,
            all_apps: false,
            accept_not_measured: vec![],
            no_reuse: false,
            set: Default::default(),
            cancel_ci_run: None,
            priority: Default::default(),
        }
    }

    /// A repository with two commits; main is at the first, origin/train at the second (the 2026-11-13 shape).
    fn repo() -> (tempfile::TempDir, PathBuf, String, String) {
        let d = tempfile::tempdir().unwrap();
        let r = d.path().join("r");
        std::fs::create_dir(&r).unwrap();
        let git = |a: &[&str]| git_text(&r, a).unwrap_or_else(|| panic!("git {a:?}"));
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "t"]);
        std::fs::write(r.join("a.txt"), "1\n").unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-q", "-m", "one"]);
        let one = git(&["rev-parse", "HEAD"]).trim().to_string();
        git(&["update-ref", "refs/remotes/origin/main", &one]);
        std::fs::write(r.join("a.txt"), "2\n").unwrap();
        git(&["commit", "-q", "-am", "two"]);
        let two = git(&["rev-parse", "HEAD"]).trim().to_string();
        git(&["update-ref", "refs/remotes/origin/train", &two]);
        (d, r, one, two)
    }

    fn state(r: &Path) -> PathBuf {
        r.join(".git/paddock-pre-push")
    }

    /// A pass of the push gate for `sha`, made the only way a pass is made: through `measure`.
    fn pass_for(r: &Path, sha: &str) -> paddock_cli::Outcome {
        let subject = crate::pre_push::outcome::subject_of(r, Some(sha));
        paddock_cli::measure(
            paddock_cli::CheckId::new("pre-push"),
            subject,
            crate::pre_push::exec::tool(),
            paddock_cli::outcome::UnixSeconds(1),
            || Ok(Vec::new()),
        )
    }

    fn push_line(sha: &str) -> String {
        format!("refs/heads/x {sha} refs/heads/main {}\n", "0".repeat(40))
    }

    #[test]
    fn a_commit_on_a_remote_branch_without_evidence_is_checked_not_passed() {
        let (_d, r, _one, two) = repo();
        let l = fake(None, Err("no run"));
        let (found, _) = known::find(&r, &state(&r), None, std::slice::from_ref(&two), &["origin/main"], &l);
        assert!(found.is_none(), "origin/train holds it, but that is no evidence");
    }

    #[test]
    fn a_pass_record_of_the_exact_sha_and_tree_is_the_evidence() {
        let (_d, r, one, two) = repo();
        let l = fake(None, Err("no run"));
        known::write_record(
            &r,
            &state(&r),
            &two,
            "origin/main",
            &["fmt crates".into()],
            1_700_000_000,
            &pass_for(&r, &two),
        )
        .unwrap();
        let (found, trail) = known::find(&r, &state(&r), None, std::slice::from_ref(&two), &["origin/main"], &l);
        let found = found.unwrap();
        assert!(
            matches!(&found[..], [Evidence::Record { id, outcome: paddock_cli::Outcome::KnownCommit(_) }] if *id == format!("{}@1700000000", &two[..12])),
            "{found:?}"
        );
        assert!(trail[0].contains("pass record"), "{trail:?}");
        // The record of another sha is no evidence; a record whose tree changed is none either.
        let (found, _) = known::find(&r, &state(&r), None, std::slice::from_ref(&one), &["origin/main"], &l);
        assert!(found.is_none());
        std::fs::copy(
            state(&r).join("records").join(format!("{two}.json")),
            state(&r).join("records").join(format!("{one}.json")),
        )
        .unwrap();
        let (found, _) = known::find(&r, &state(&r), None, &[one], &["origin/main"], &l);
        assert!(found.is_none(), "a record is read only for its own sha");
        let rec = state(&r).join("records").join(format!("{two}.json"));
        let text = std::fs::read_to_string(&rec).unwrap();
        let tree = text.split("\"tree\":\"").nth(1).unwrap().split('"').next().unwrap().to_string();
        std::fs::write(&rec, text.replace(&tree, &"0".repeat(40))).unwrap();
        let (found, _) = known::find(&r, &state(&r), None, &[two], &["origin/main"], &l);
        assert!(found.is_none(), "the tree id no longer matches");
    }

    /// A pass made by a tool with fewer rows is no evidence for one with more (review: a record from before `gates-register` existed
    /// would have let a push skip it). The record carries the tool's fixed row set; a different one is refused with the reason.
    #[test]
    fn a_record_made_with_another_set_of_rows_is_no_evidence() {
        let (_d, r, _one, two) = repo();
        let l = fake(None, Err("no run"));
        known::write_record(&r, &state(&r), &two, "origin/main", &["fmt crates".into()], 1, &pass_for(&r, &two)).unwrap();
        let rec = state(&r).join("records").join(format!("{two}.json"));
        let text = std::fs::read_to_string(&rec).unwrap();
        assert!(text.contains("\"gate_rows\""), "the record names the rows: {text}");
        let (found, _) = known::find(&r, &state(&r), None, std::slice::from_ref(&two), &["origin/main"], &l);
        assert!(found.is_some(), "positive control: the same row set is evidence");
        let old = text.replace("gates-register,", "");
        assert_ne!(old, text, "the control changed the record");
        std::fs::write(&rec, old).unwrap();
        let (found, trail) = known::find(&r, &state(&r), None, &[two], &["origin/main"], &l);
        assert!(found.is_none(), "a record of an older row set is refused");
        assert!(trail.iter().any(|t| t.contains("rows")), "{trail:?}");
    }

    #[test]
    fn a_complete_green_ci_run_counts_only_when_the_pushed_remote_has_the_sha_as_a_tip() {
        let (_d, r, _one, two) = repo();
        let tip = format!("{two}\trefs/heads/train/x\n");
        let green = fake(Some(&tip), Ok(8053));
        let (found, trail) = known::find(&r, &state(&r), Some("origin"), std::slice::from_ref(&two), &["origin/main"], &green);
        assert!(
            matches!(
                found.as_deref(),
                Some([Evidence::CiRun {
                    run: 8053,
                    outcome: paddock_cli::Outcome::KnownCommit(_)
                }])
            ),
            "{found:?}"
        );
        assert!(trail.iter().any(|t| t.contains("git ls-remote origin (live)")), "{trail:?}");
        for (ls, remote) in [
            (None, Some("origin")),
            (Some("1111\trefs/heads/other\n"), Some("origin")),
            (Some(tip.as_str()), None),
        ] {
            let l = fake(ls, Ok(8053));
            let (found, _) = known::find(&r, &state(&r), remote, std::slice::from_ref(&two), &["origin/main"], &l);
            assert!(found.is_none(), "{ls:?} {remote:?}");
            assert!(
                !l.asked.borrow().iter().any(|a| a.starts_with("ci ")),
                "no CI lookup without a confirmed tip"
            );
        }
        let red = fake(Some(&tip), Err("red or partial"));
        let (found, trail) = known::find(&r, &state(&r), Some("origin"), &[two], &["origin/main"], &red);
        assert!(
            found.is_none() && trail.last().unwrap().contains("no complete green CI run"),
            "{trail:?}"
        );
    }

    #[test]
    fn evidence_applies_only_to_the_hooks_push_with_nothing_overridden() {
        let pushed = vec!["a".repeat(40)];
        assert!(known::applicable(&args(), &pushed));
        assert!(!known::applicable(&args(), &[]), "a delete-only push has no commit");
        for tweak in [
            (|a: &mut PrePushArgs| a.base = Some("origin/main".into())) as fn(&mut PrePushArgs),
            |a| a.files = vec!["x".into()],
            |a| a.only = vec!["fmt".into()],
            |a| a.dry_run = true,
            |a| a.refs_from_stdin = false,
        ] {
            let mut a = args();
            tweak(&mut a);
            assert!(!known::applicable(&a, &pushed));
            // Only the no-stdin tweak is a full run (a person's working-tree run may leave a record); every other tweak is partial.
            assert_eq!(
                known::recordable(&a, 0, 0, Exit::Ok),
                !a.refs_from_stdin,
                "no record from a partial run"
            );
        }
        assert!(known::recordable(&args(), 0, 0, Exit::Ok));
        assert!(!known::recordable(&args(), 1, 0, Exit::Ok) && !known::recordable(&args(), 0, 1, Exit::Ok));
        assert!(!known::recordable(&args(), 0, 0, Exit::NotMeasured) && !known::recordable(&args(), 0, 0, Exit::Findings));
    }

    #[test]
    fn the_known_commit_report_is_its_own_verdict_and_says_not_re_checked() {
        let (_d, r, _one, two) = repo();
        known::write_record(&r, &state(&r), &two, "origin/main", &[], 5, &pass_for(&r, &two)).unwrap();
        let rep = command_in(
            &PrePushArgs {
                root: Some(r.clone()),
                ..args()
            },
            &r,
            &push_line(&two),
            &fake(None, Err("x")),
            crate::local_sccache::Lazy::off(),
            Some(&super::Fake::default()),
        )
        .unwrap();
        assert_eq!(rep.exit, Exit::Ok);
        assert_eq!(rep.data["verdict"], "known_commit");
        assert_eq!(rep.data["not_rechecked"], true);
        assert_eq!(rep.data["commits"][0]["sha"], two.as_str());
        assert_eq!(rep.data["commits"][0]["evidence"]["kind"], "record");
        assert_eq!(rep.data["remote"], "origin");
        assert!(rep.data.get("passed").is_none(), "it is not a measured pass: {}", rep.data);
        assert!(rep.text.starts_with("pre-push: not re-checked: known commit"), "{}", rep.text);
        let log = std::fs::read_to_string(state(&r).join("known-commits.log")).unwrap();
        assert!(
            log.contains(&two) && log.contains("remote=origin") && log.contains("pass record"),
            "{log}"
        );
    }

    /// ADR 0089 amendment (rows 3 and 7): the known commit is an outcome that names the commit and tree it stands on, exits 0 and
    /// says `not_rechecked`; a consumer asking about another tree (merge check) refuses it, and a record without evidence is none.
    #[test]
    fn the_known_commit_outcome_names_its_tree_and_a_consumer_about_another_tree_refuses_it() {
        use paddock_cli::outcome::{Sha, TreeId};
        use paddock_cli::{Outcome, Subject};
        let (_d, r, _one, two) = repo();
        known::write_record(&r, &state(&r), &two, "origin/main", &[], 5, &pass_for(&r, &two)).unwrap();
        let rep = command_in(
            &PrePushArgs {
                root: Some(r.clone()),
                ..args()
            },
            &r,
            &push_line(&two),
            &fake(None, Err("x")),
            crate::local_sccache::Lazy::off(),
            Some(&super::Fake::default()),
        )
        .unwrap();
        let tree = git_text(&r, &["rev-parse", &format!("{two}^{{tree}}")]).unwrap().trim().to_string();
        let ask = Subject::Commit {
            sha: Sha::new(two.as_str()),
            tree: TreeId::new(tree.as_str()),
        };
        let wire = &rep.data["outcome"];
        assert_eq!(wire["status"], "known-commit");
        assert_eq!(wire["not_rechecked"], true);
        assert!(
            matches!(Outcome::from_wire(wire, &ask, true), Outcome::KnownCommit(_)),
            "the pre-push hook accepts it"
        );
        assert!(
            matches!(Outcome::from_wire(wire, &ask, false), Outcome::NotMeasured(_)),
            "merge check does not"
        );
        let other = Subject::Commit {
            sha: Sha::new("0".repeat(40)),
            tree: TreeId::new(tree),
        };
        assert!(
            matches!(Outcome::from_wire(wire, &other, true), Outcome::NotMeasured(_)),
            "another commit"
        );
        // A record written before evidence existed (no `evidence`) proves nothing.
        let rec = state(&r).join("records").join(format!("{two}.json"));
        let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&rec).unwrap()).unwrap();
        v.as_object_mut().unwrap().remove("evidence");
        std::fs::write(&rec, v.to_string()).unwrap();
        let (found, _) = known::find(
            &r,
            &state(&r),
            None,
            std::slice::from_ref(&two),
            &["origin/main"],
            &fake(None, Err("x")),
        );
        assert!(found.is_none());
    }

    /// Only a `Clean` outcome can leave a record: findings, not measured and an empty run cannot.
    #[test]
    fn only_a_pass_leaves_a_record() {
        let (_d, r, _one, two) = repo();
        for not_a_pass in [
            paddock_cli::Outcome::NotMeasured(paddock_cli::Reason::new("x")),
            paddock_cli::Outcome::NotApplicable(paddock_cli::Reason::new("x")),
        ] {
            assert!(known::write_record(&r, &state(&r), &two, "b", &[], 1, &not_a_pass).is_err());
        }
        assert!(!state(&r).join("records").join(format!("{two}.json")).exists());
    }

    #[test]
    fn graph_a_delete_only_push_and_an_explicit_base_never_use_evidence() {
        let (_d, r, _one, two) = repo();
        known::write_record(&r, &state(&r), &two, "origin/main", &[], 5, &pass_for(&r, &two)).unwrap();
        let l = fake(None, Err("x"));
        let delete = format!("(delete) {} refs/heads/old {}\n", "0".repeat(40), two);
        // The gates of the run are measured afresh, never read from the record: gates-register reaches the executor, started as
        // the stand-in dev program (a test process is never started as paddock-dev, 48fa2449a).
        let gates = standin_dev();
        let got = command_in(
            &PrePushArgs {
                root: Some(r.clone()),
                ..args()
            },
            &r,
            &delete,
            &l,
            crate::local_sccache::Lazy::off(),
            Some(&gates),
        );
        assert!(got.as_ref().map_or(true, |rep| rep.data["verdict"] != "known_commit"), "{got:?}");
        assert_measured_afresh(&got, &gates);
        let a = PrePushArgs {
            root: Some(r.clone()),
            base: Some("origin/main".into()),
            ..args()
        };
        let gates = standin_dev();
        let got = command_in(&a, &r, &push_line(&two), &l, crate::local_sccache::Lazy::off(), Some(&gates));
        assert!(got.as_ref().map_or(true, |rep| rep.data["verdict"] != "known_commit"), "{got:?}");
        assert_measured_afresh(&got, &gates);
    }

    /// The executor of the gates with a stand-in for this program: in a test process the real one is the test harness, which the
    /// rows `./dev` runs itself must never start (`tool_cache::self_exe`).
    fn standin_dev() -> super::Fake {
        super::Fake {
            dev_program: Some("/x/dev".into()),
            ..Default::default()
        }
    }

    /// The gates of this run were measured afresh, none read from the record: commit-lint passed in process and gates-register
    /// went to the executor (started as the stand-in) and passed.
    fn assert_measured_afresh<E: std::fmt::Debug>(got: &Result<paddock_cli::Report, E>, gates: &super::Fake) {
        let rep = got.as_ref().unwrap_or_else(|e| panic!("no report: {e:?}"));
        let rows = rep.data["gates"].as_array().expect("the report lists the gates of the run");
        let verdict = |id: &str| rows.iter().find(|g| g["id"] == id).map(|g| g["verdict"].clone());
        assert_eq!(verdict("commit-lint"), Some(serde_json::json!("passed")), "{rep:?}");
        assert_eq!(verdict("gates-register"), Some(serde_json::json!("passed")), "{rep:?}");
        assert!(gates.ran.lock().unwrap().contains(&"gates-register".to_string()), "{rep:?}");
    }

    /// A record measured from a nearer base (a local train, a branch's old commit) checked less than a push from an older base
    /// needs: it counts only for a push measured from its own base or a newer one, or through the records of the bases between.
    #[test]
    fn a_pass_record_counts_only_for_a_push_measured_from_its_base_or_an_older_one() {
        let (_d, r, one, two) = repo();
        std::fs::write(r.join("a.txt"), "3\n").unwrap();
        git_text(&r, &["commit", "-q", "-am", "three"]).unwrap();
        let three = git_text(&r, &["rev-parse", "HEAD"]).unwrap().trim().to_string();
        let l = fake(None, Err("no run"));
        known::write_record(&r, &state(&r), &three, &two, &[], 5, &pass_for(&r, &three)).unwrap();
        let (found, trail) = known::find(&r, &state(&r), None, std::slice::from_ref(&three), &[one.as_str()], &l);
        assert!(found.is_none(), "{trail:?}");
        assert!(
            trail[0].contains("measured from") && trail[0].contains(&two[..12]) && trail[0].contains("no valid pass record of its own"),
            "{trail:?}"
        );
        let (found, _) = known::find(&r, &state(&r), None, std::slice::from_ref(&three), &[two.as_str()], &l);
        assert!(found.is_some(), "its own base");
        // A base no remote-tracking ref holds (a local commit) takes no record, even a newer one.
        let (found, trail) = known::find(&r, &state(&r), None, std::slice::from_ref(&three), &[three.as_str()], &l);
        assert!(found.is_none() && trail[0].contains("on no remote-tracking ref"), "{trail:?}");
        // Through the record of the base in between: two was measured from one, so three's record covers a push from one.
        known::write_record(&r, &state(&r), &two, &one, &[], 5, &pass_for(&r, &two)).unwrap();
        let (found, trail) = known::find(&r, &state(&r), None, std::slice::from_ref(&three), &[one.as_str()], &l);
        assert!(found.is_some(), "{trail:?}");
        // A record that does not name its base commit proves nothing.
        let rec = state(&r).join("records").join(format!("{three}.json"));
        let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&rec).unwrap()).unwrap();
        v.as_object_mut().unwrap().remove("base_commit");
        std::fs::write(&rec, v.to_string()).unwrap();
        let (found, _) = known::find(&r, &state(&r), None, std::slice::from_ref(&three), &[two.as_str()], &l);
        assert!(found.is_none());
    }

    fn reads<'a>(files: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |p| files.iter().find(|(n, _)| *n == p).map(|(_, t)| t.to_string())
    }
    const MANIFEST: &str = r#"{"dependencies":{"yaml":"^2.9.1"}}"#;
    const LOCK: &str = r#"{"packages":{"node_modules/yaml":{"version":"2.9.1"}}}"#;

    #[test]
    fn the_node_install_is_stale_when_a_version_differs_missing_or_unreadable_and_ready_when_equal() {
        let ok = [
            ("node_modules/.package-lock.json", LOCK),
            ("package-lock.json", LOCK),
            ("conformance/package.json", MANIFEST),
        ];
        assert_eq!(node_env_problem(false, &reads(&ok)), None);
        let old = r#"{"packages":{"node_modules/yaml":{"version":"2.8.0"}}}"#;
        let stale = [
            ("node_modules/.package-lock.json", old),
            ("package-lock.json", LOCK),
            ("conformance/package.json", MANIFEST),
        ];
        let why = node_env_problem(false, &reads(&stale)).unwrap();
        assert!(why.contains("2.8.0") && why.contains("2.9.1"), "{why}");
        let gone = [
            ("node_modules/.package-lock.json", r#"{"packages":{}}"#),
            ("package-lock.json", LOCK),
            ("conformance/package.json", MANIFEST),
        ];
        assert!(node_env_problem(false, &reads(&gone)).unwrap().contains("missing"));
        assert!(node_env_problem(false, &reads(&[])).unwrap().contains("nothing is installed"));
        assert!(
            node_env_problem(false, &reads(&ok[..2])).is_some(),
            "an unreadable manifest is a problem, not a pass"
        );
        assert!(
            node_env_problem(true, &reads(&ok))
                .unwrap()
                .contains("throw-away worktree has no node_modules")
        );
    }
}

/// A stale npm install made conformance fail with "yaml cannot be loaded", which counted as a finding (2026-11-13).
#[test]
fn a_missing_conformance_install_is_environment_not_ready_never_a_failure_or_pass() {
    let mut f = facts();
    f.conformance_env = Some("node_modules/yaml is missing".into());
    let planned = plan_gates(&plan(), &change(&["conformance/a.mjs"]), &f);
    let Some(Planned::NotMeasured { reason, next, .. }) = planned
        .iter()
        .find(|p| matches!(p, Planned::NotMeasured { id, .. } if id == "conformance"))
    else {
        panic!("conformance must be NotMeasured (exit 6, ADR 0089 A2), not run: {planned:?}");
    };
    assert!(reason.starts_with("environment not ready"), "{reason}");
    assert!(next.contains("npm ci --ignore-scripts -w conformance"), "{next}");
    assert!(!ids(&planned).contains(&"conformance".to_string()), "it does not run");
    // The conformance tests need the same install: not measured too, never a failure.
    assert!(
        planned
            .iter()
            .any(|p| matches!(p, Planned::NotMeasured { id, .. } if id == "conformance-tests")),
        "{planned:?}"
    );
    assert!(!ids(&planned).contains(&"conformance-tests".to_string()), "it does not run");
}

mod base_choice {
    use super::super::known::Lookups;
    use super::super::{PrePushArgs, Pushed, command_in};
    use crate::plan_repo::git_text;
    use std::path::{Path, PathBuf};

    /// A new ref `w` at `tip` pushed to `remote`: the shape most tests here need.
    fn choose_base(root: &Path, remote: Option<&str>, tip: &str, look: &dyn Lookups) -> (String, String) {
        let p = Pushed {
            local_ref: "refs/heads/w".into(),
            sha: tip.into(),
            remote_ref: "refs/heads/w".into(),
            remote_sha: "0".repeat(40),
        };
        super::super::choose_base(root, &p, remote, "origin/main", look)
    }

    /// `git ls-remote` as the remote would answer it: a fixed text or unreachable.
    pub struct Remote(pub Option<String>);
    impl Lookups for Remote {
        fn ls_remote(&self, _: &str) -> Option<String> {
            self.0.clone()
        }
        fn ci_green(&self, _: &str) -> Result<u64, String> {
            Err("no run".into())
        }
    }
    /// The remote's branches as `ls-remote` text, from refs/remotes/origin/* of the fixture (the sha each ref points at).
    pub fn live(r: &Path) -> Remote {
        let refs = git_text(r, &["for-each-ref", "--format=%(objectname)\t%(refname)", "refs/remotes/origin/"]).unwrap();
        Remote(Some(
            refs.lines()
                .map(|l| l.replacen("refs/remotes/origin/", "refs/heads/", 1) + "\n")
                .collect(),
        ))
    }

    /// main: m1; train (origin/train/t): m1-t1-t2.
    fn repo() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let r = d.path().join("r");
        std::fs::create_dir(&r).unwrap();
        for a in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.email", "t@t"],
            &["config", "user.name", "t"],
        ] {
            git_text(&r, a).unwrap();
        }
        commit(&r, "m1");
        git_text(&r, &["update-ref", "refs/remotes/origin/main", "HEAD"]).unwrap();
        git_text(&r, &["checkout", "-q", "-b", "train"]).unwrap();
        commit(&r, "t1");
        commit(&r, "t2");
        git_text(&r, &["update-ref", "refs/remotes/origin/train/t", "HEAD"]).unwrap();
        (d, r)
    }
    fn commit(r: &Path, f: &str) {
        std::fs::write(r.join(f), f).unwrap();
        git_text(r, &["add", f]).unwrap();
        git_text(r, &["commit", "-q", "-m", f]).unwrap();
    }
    fn sha(r: &Path, rev: &str) -> String {
        git_text(r, &["rev-parse", rev]).unwrap().trim().to_string()
    }

    #[test]
    fn a_branch_made_from_a_train_branch_is_measured_from_the_trains_live_tip() {
        let (_d, r) = repo();
        let train = sha(&r, "origin/train/t");
        commit(&r, "w1");
        let (base, why) = choose_base(&r, Some("origin"), &sha(&r, "HEAD"), &live(&r));
        assert_eq!(base, train, "{why}");
        assert!(
            why.contains("train/t") && why.contains("fewest new commits, 1") && why.contains("live"),
            "{why}"
        );
    }

    #[test]
    fn a_branch_made_from_main_is_measured_from_main() {
        let (_d, r) = repo();
        let main = sha(&r, "origin/main");
        git_text(&r, &["checkout", "-q", "-b", "w", "origin/main"]).unwrap();
        commit(&r, "w1");
        let (base, _) = choose_base(&r, Some("origin"), &sha(&r, "HEAD"), &live(&r));
        // Main and the train are equally far (one new commit): the train wins the tie, and both give main as the merge base.
        let mb = git_text(&r, &["merge-base", &base, "HEAD"]).unwrap();
        assert_eq!(mb.trim(), main);
    }

    #[test]
    fn main_merged_into_a_train_based_branch_is_still_measured_from_the_train() {
        let (_d, r) = repo();
        let train = sha(&r, "origin/train/t");
        git_text(&r, &["checkout", "-q", "main"]).unwrap();
        commit(&r, "m2");
        git_text(&r, &["update-ref", "refs/remotes/origin/main", "HEAD"]).unwrap();
        git_text(&r, &["checkout", "-q", "-b", "w", "origin/train/t"]).unwrap();
        commit(&r, "w1");
        git_text(&r, &["merge", "-q", "--no-edit", "origin/main"]).unwrap();
        let (base, why) = choose_base(&r, Some("origin"), &sha(&r, "HEAD"), &live(&r));
        assert_eq!(base, train, "{why}");
    }

    #[test]
    fn an_unreachable_remote_or_no_remote_uses_the_remote_tracking_refs_and_says_so() {
        let (_d, r) = repo();
        let train = sha(&r, "origin/train/t");
        commit(&r, "w1");
        let tip = sha(&r, "HEAD");
        let main = sha(&r, "origin/main");
        // Not checked live, a train ref main does not hold may be gone from the remote: only main (or the upstream) counts.
        let (base, why) = choose_base(&r, Some("origin"), &tip, &Remote(None));
        assert_eq!(base, main, "{why}");
        assert!(
            why.contains("could not be read, so the remote-tracking refs are not checked live")
                && why.contains("origin/main (remote-tracking ref, not checked live; main or the upstream holds it)")
                && why.contains("skipped 1 remote-tracking ref(s)"),
            "{why}"
        );
        let (base, why) = choose_base(&r, None, &tip, &live(&r));
        assert_eq!((base.as_str(), why.contains("no remote was named")), (main.as_str(), true), "{why}");
        // The train as the branch's upstream: it counts.
        git_text(&r, &["config", "remote.origin.url", "/nowhere"]).unwrap();
        git_text(&r, &["config", "remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*"]).unwrap();
        git_text(&r, &["config", "branch.train.remote", "origin"]).unwrap();
        git_text(&r, &["config", "branch.train.merge", "refs/heads/train/t"]).unwrap();
        let p = Pushed {
            local_ref: "refs/heads/train".into(),
            sha: tip.clone(),
            remote_ref: "refs/heads/w".into(),
            remote_sha: "0".repeat(40),
        };
        let (base, why) = super::super::choose_base(&r, &p, Some("origin"), "origin/main", &Remote(None));
        assert_eq!(base, train, "{why}");
        git_text(&r, &["config", "--unset", "branch.train.merge"]).unwrap();
        // A remote-tracking ref the live list no longer has (deleted or force-pushed) is stale: skipped, said so; then origin/main.
        let (base, why) = choose_base(
            &r,
            Some("origin"),
            &tip,
            &Remote(Some(format!("{}\trefs/heads/x\n", "1".repeat(40)))),
        );
        assert_eq!(base, sha(&r, "origin/main"), "{why}");
        assert!(
            why.contains("skipped 2 remote-tracking ref(s) the remote may no longer have"),
            "{why}"
        );
        // The remote moved a branch on to a commit this clone has: its older remote-tracking ref still counts.
        let (base, why) = choose_base(&r, Some("origin"), &tip, &Remote(Some(format!("{tip}\trefs/heads/train/t\n"))));
        assert_eq!(base, train, "{why}");
        assert!(why.contains("the remote has moved on from it (live)"), "{why}");
        // Nothing usable at all: origin/main, said so.
        git_text(&r, &["update-ref", "-d", "refs/remotes/origin/train/t"]).unwrap();
        git_text(&r, &["update-ref", "-d", "refs/remotes/origin/main"]).unwrap();
        let (base, why) = choose_base(&r, Some("origin"), &tip, &Remote(None));
        assert_eq!(base, "origin/main", "{why}");
        assert!(why.contains("measuring from origin/main"), "{why}");
    }

    /// A branch deleted on the remote keeps its remote-tracking ref until a prune. With the remote unreadable it must not be the
    /// base: its commits may be gone, and measuring from it would leave them unchecked.
    #[test]
    fn a_stale_ref_of_a_branch_deleted_on_the_remote_is_no_base_when_the_remote_cannot_be_read() {
        let (_d, r) = repo();
        commit(&r, "gone1");
        let gone = sha(&r, "HEAD");
        git_text(&r, &["update-ref", "refs/remotes/origin/feat/gone", &gone]).unwrap();
        commit(&r, "w1");
        let tip = sha(&r, "HEAD");
        let (base, why) = choose_base(&r, Some("origin"), &tip, &Remote(None));
        assert_ne!(base, gone, "{why}");
        assert_eq!(base, sha(&r, "origin/main"), "{why}");
        assert!(why.contains("feat/gone"), "the skipped ref is named: {why}");
        // Live, the listing without it says the same.
        let listing = format!("{}\trefs/heads/main\n", sha(&r, "origin/main"));
        let (base, why) = choose_base(&r, Some("origin"), &tip, &Remote(Some(listing)));
        assert_eq!(base, sha(&r, "origin/main"), "{why}");
    }

    #[test]
    fn every_pushed_commit_already_on_the_remote_is_never_an_empty_pass_it_measures_from_the_next_base() {
        let (_d, r) = repo();
        let two = sha(&r, "origin/train/t");
        let (base, why) = choose_base(&r, Some("origin"), &two, &live(&r));
        assert_eq!(base, sha(&r, "origin/main"), "{why}");
        assert!(why.contains("already on origin/train/t") && why.contains("no evidence"), "{why}");
        // A commit already pushed under another branch name keeps the base that branch was measured from (its pass record stays
        // valid), never main.
        commit(&r, "w1");
        let w1 = sha(&r, "HEAD");
        git_text(&r, &["update-ref", "refs/remotes/origin/fix/x", &w1]).unwrap();
        let (base, why) = choose_base(&r, Some("origin"), &w1, &live(&r));
        assert_eq!(base, two, "{why}");
    }

    #[test]
    fn graph_a_push_already_on_the_remote_without_evidence_runs_the_checks_instead_of_nothing_to_check() {
        let (_d, r) = repo();
        let two = sha(&r, "origin/train/t");
        let args = PrePushArgs {
            root: Some(r.clone()),
            base: None,
            remote: Some("origin".into()),
            only: vec![],
            dry_run: true,
            files: vec![],
            refs_from_stdin: true,
            member_gate: false,
            all_apps: false,
            accept_not_measured: vec![],
            no_reuse: false,
            set: Default::default(),
            cancel_ci_run: None,
            priority: Default::default(),
        };
        let line = format!("refs/heads/x {two} refs/heads/main {}\n", "0".repeat(40));
        let got = command_in(
            &args,
            &r,
            &line,
            &live(&r),
            crate::local_sccache::Lazy::off(),
            Some(&super::Fake::default()),
        );
        assert!(got.as_ref().map_or(true, |rep| !rep.text.contains("nothing to check")), "{got:?}");
    }
}

/// A scratch repository with a helper to commit and to fake remote-tracking refs (no network).
struct Repo {
    _d: tempfile::TempDir,
    root: std::path::PathBuf,
}

impl Repo {
    fn new() -> Repo {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("r");
        std::fs::create_dir(&root).unwrap();
        let r = Repo { _d: d, root };
        r.git(&["init", "-q", "-b", "main"]);
        r.git(&["config", "user.email", "t@t"]);
        r.git(&["config", "user.name", "t"]);
        r
    }

    fn git(&self, args: &[&str]) -> String {
        crate::plan_repo::git_text(&self.root, args).unwrap_or_else(|| panic!("git {args:?}"))
    }

    /// A commit that changes `file`, on the current branch; its sha.
    fn commit(&self, file: &str) -> String {
        std::fs::write(self.root.join(file), "x\n").unwrap();
        self.git(&["add", file]);
        self.git(&["commit", "-q", "-m", file]);
        self.git(&["rev-parse", "HEAD"]).trim().to_string()
    }

    fn remote(&self, name: &str, sha: &str) {
        self.git(&["update-ref", &format!("refs/remotes/origin/{name}"), sha]);
    }

    /// What `git ls-remote origin` would print for the branches faked with [`Repo::remote`].
    fn live(&self) -> base_choice::Remote {
        base_choice::live(&self.root)
    }

    fn choose(&self, p: &super::Pushed) -> (String, String) {
        super::choose_base(&self.root, p, Some("origin"), "origin/main", &self.live())
    }
}

fn new_ref(local: &str, sha: &str) -> super::Pushed {
    let zero = "0".repeat(40);
    super::Pushed {
        local_ref: format!("refs/heads/{local}"),
        sha: sha.into(),
        remote_ref: format!("refs/heads/{local}"),
        remote_sha: zero,
    }
}

#[test]
fn a_new_branch_cut_from_a_train_is_measured_against_the_train_not_main() {
    let r = Repo::new();
    let c1 = r.commit("a");
    r.git(&["checkout", "-q", "-b", "train"]);
    r.commit("t1");
    let t2 = r.commit("t2");
    r.remote("main", &c1);
    r.remote("train/2026-11-12-7", &t2);
    r.git(&["checkout", "-q", "-b", "fix"]);
    let f1 = r.commit("f1");
    let (base, why) = r.choose(&new_ref("fix", &f1));
    assert_eq!(base, t2, "{why}");
    assert!(
        why.contains("train/2026-11-12-7") && why.contains("fewest new commits, 1") && why.contains(" ms"),
        "{why}"
    );
}

#[test]
fn an_existing_remote_ref_is_measured_from_its_old_commit() {
    let r = Repo::new();
    let c1 = r.commit("a");
    r.remote("main", &c1);
    r.git(&["checkout", "-q", "-b", "fix"]);
    let f1 = r.commit("f1");
    let f2 = r.commit("f2");
    let p = super::Pushed {
        local_ref: "refs/heads/fix".into(),
        sha: f2,
        remote_ref: "refs/heads/fix".into(),
        remote_sha: f1.clone(),
    };
    let (base, why) = r.choose(&p);
    assert_eq!(base, f1);
    assert!(why.contains("on the remote: its old commit"), "{why}");
    // Its old commit is not here: say so and measure from the nearest live remote branch tip (main here).
    let gone = super::Pushed {
        remote_sha: "1".repeat(40),
        ..p
    };
    let (base, why) = r.choose(&gone);
    assert_eq!(base, c1);
    assert!(why.contains("not here (fetch it)"), "{why}");
}

#[test]
fn with_no_usable_remote_branch_the_base_is_main_and_says_so() {
    let r = Repo::new();
    let c1 = r.commit("a");
    // Nothing listed by the remote: origin/main, said so.
    let (base, why) = r.choose(&new_ref("main", &c1));
    assert_eq!(base, "origin/main");
    assert!(
        why.contains("no commit of origin that the pushed commit contains is usable") && why.contains("measuring from origin/main"),
        "{why}"
    );
    // A sibling feature branch is no candidate either.
    r.git(&["checkout", "-q", "--orphan", "other"]);
    let o1 = r.commit("o");
    r.git(&["checkout", "-q", "main"]);
    r.remote("other", &o1);
    let (base, _) = r.choose(&new_ref("main", &c1));
    assert_eq!(base, "origin/main");
}

#[test]
fn a_tracking_upstream_wins_only_a_tie() {
    let r = Repo::new();
    let c1 = r.commit("a");
    r.remote("main", &c1);
    r.git(&["checkout", "-q", "-b", "train"]);
    let t1 = r.commit("t1");
    r.remote("train/x", &t1);
    r.git(&["checkout", "-q", "-b", "fix"]);
    let f1 = r.commit("f1");
    // Tracking origin/main, but the train is nearer: the nearest wins, the upstream only breaks a tie.
    r.git(&["config", "remote.origin.url", "/nowhere"]);
    r.git(&["config", "remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*"]);
    r.git(&["config", "branch.fix.remote", "origin"]);
    r.git(&["config", "branch.fix.merge", "refs/heads/main"]);
    let (base, why) = r.choose(&new_ref("fix", &f1));
    assert_eq!(base, t1, "an upstream that is main must not beat a nearer train: {why}");
    // At the same distance the tracking upstream wins.
    let r3 = Repo::new();
    let c3 = r3.commit("a");
    r3.remote("main", &c3);
    r3.remote("train/z", &c3);
    r3.git(&["config", "remote.origin.url", "/nowhere"]);
    r3.git(&["config", "remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*"]);
    r3.git(&["checkout", "-q", "-b", "fix"]);
    let f3 = r3.commit("f");
    r3.git(&["config", "branch.fix.remote", "origin"]);
    r3.git(&["config", "branch.fix.merge", "refs/heads/main"]);
    let (base, why) = r3.choose(&new_ref("fix", &f3));
    assert_eq!(
        (
            base.as_str(),
            why.contains("origin/main (") && why.contains("its tracking upstream")
        ),
        (c3.as_str(), true),
        "{why}"
    );
}

#[test]
fn a_push_of_only_reservation_refs_has_nothing_to_check() {
    let zero = "0".repeat(40);
    let paddock = format!("refs/paddock/check-ids/C9 aaa refs/paddock/check-ids/C9 {zero}\n");
    let blob = |sha: &str| sha == "aaa";
    assert!(super::only_reservation_refs(&paddock, &blob));
    let deletion = format!("(delete) {zero} refs/paddock/check-ids/C9 aaa\n");
    assert!(super::only_reservation_refs(&deletion, &blob), "a deletion is a reservation push");
    let mixed = format!("{paddock}refs/heads/a bbb refs/heads/a {zero}\n");
    assert!(
        !super::only_reservation_refs(&mixed, &blob),
        "a branch in the same push is still checked"
    );
    assert!(!super::only_reservation_refs("", &blob), "no lines is not a reservation push");
    let commit = format!("refs/paddock/check-ids/C9 ccc refs/paddock/check-ids/C9 {zero}\n");
    assert!(
        !super::only_reservation_refs(&commit, &blob),
        "a commit under check-ids goes through the gates"
    );
    let other = format!("refs/paddock/x aaa refs/paddock/x {zero}\n");
    assert!(
        !super::only_reservation_refs(&other, &blob),
        "only refs/paddock/check-ids/ is exempt"
    );
}

#[test]
fn git_pre_push_lines_become_pushed_refs_and_the_not_measured_ids_end_the_output() {
    let zero = "0".repeat(40);
    let input = format!("refs/heads/a aaa refs/heads/a {zero}\nrefs/heads/gone {zero} refs/heads/gone bbb\nbad line\n");
    let got = super::parse_push(&input);
    assert_eq!(got.len(), 1);
    assert_eq!((got[0].sha.as_str(), got[0].remote_ref.as_str()), ("aaa", "refs/heads/a"));
    let nm = |id: &str| (id.to_string(), Row::not_measured("r", "n"));
    let report = render(
        "h",
        &[
            nm("clippy-windows apps/dyno"),
            ("fmt crates".into(), ran_ok("fmt crates", 1.0)),
            nm("clippy crates"),
        ],
    );
    let last = report.text.lines().last().unwrap();
    assert_eq!(last, "NOT MEASURED (2): clippy-windows apps/dyno; clippy crates");
}

#[test]
fn a_branch_that_merged_its_train_in_is_measured_from_the_train_not_its_own_old_tip() {
    let r = Repo::new();
    let c1 = r.commit("a");
    r.git(&["checkout", "-q", "-b", "fix"]);
    let f1 = r.commit("f1");
    // The train moves on after the branch was pushed; the branch then merges it in and adds a commit.
    r.git(&["checkout", "-q", "-b", "train", &c1]);
    r.commit("t1");
    let t2 = r.commit("t2");
    r.remote("main", &c1);
    r.remote("train/9", &t2);
    r.git(&["checkout", "-q", "fix"]);
    r.git(&["merge", "-q", "--no-edit", "train"]);
    let m = r.commit("f2");
    let p = super::Pushed {
        local_ref: "refs/heads/fix".into(),
        sha: m,
        remote_ref: "refs/heads/fix".into(),
        remote_sha: f1,
    };
    let (base, why) = r.choose(&p);
    assert_eq!(base, t2, "the old tip would count t1 and t2 as new: {why}");
}

#[test]
fn queue_time_and_run_time_are_reported_apart_and_the_pass_counts_only_the_run() {
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &facts());
    let fake = Fake {
        timing: Some(Timing { queued: 12.0, ran: 3.0 }),
        ..Fake::default()
    };
    let (rows, timings) = run_all_timed(
        &planned,
        &fake,
        &RunOpts {
            workers: 2,
            queue_stall: QUEUE_STALL,
            stop: &AtomicBool::new(false),
            subject: &test_subject(),
            reuse: None,
        },
    );
    assert!(
        rows.iter()
            .all(|(_, v)| v.kind() == Kind::Skipped || (v.kind() == Kind::Passed && (v.secs - 3.0).abs() < 1e-9)),
        "a pass shows the run time, not the wall time: {rows:?}"
    );
    assert_eq!(timings["fmt crates"], Timing { queued: 12.0, ran: 3.0 });
    let report = render_timed("h", &rows, &timings);
    assert!(
        report.text.contains("ok            fmt crates") && report.text.contains("3.0 s run, 12.0 s queued"),
        "{}",
        report.text
    );
    let gate = report.data["gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["id"] == "fmt crates")
        .unwrap()
        .clone();
    assert_eq!((gate["run_secs"].as_f64(), gate["queued_secs"].as_f64()), (Some(3.0), Some(12.0)));
}

#[test]
fn a_step_that_never_got_a_slot_is_not_measured_with_its_queue_time_never_passed() {
    let planned = plan_gates(&plan(), &change(&["crates/a.rs"]), &facts());
    let fake = Fake {
        no_slot: vec!["clippy crates".into()],
        ..Fake::default()
    };
    let (rows, timings) = run_all_timed(
        &planned,
        &fake,
        &RunOpts {
            workers: 2,
            queue_stall: QUEUE_STALL,
            stop: &AtomicBool::new(false),
            subject: &test_subject(),
            reuse: None,
        },
    );
    let v = verdicts(&rows);
    let Some((reason, next)) = nm(&v["clippy crates"]) else {
        panic!("{:?}", v["clippy crates"])
    };
    assert!(
        reason.starts_with("queue stalled, held by") && reason.contains("31.0 s"),
        "{reason}"
    );
    assert!(next.contains("throttle status") && next.contains("PADDOCK_QUEUE_STALL"), "{next}");
    let report = render_timed("h", &rows, &timings);
    assert_eq!(report.exit, Exit::NotMeasured);
    assert!(report.text.contains("NOT MEASURED  clippy crates"), "{}", report.text);
}

/// 2026-11-21 (three refused pushes): a row's wait for the heavy slot is queue time, never run time. A long wait followed by a short
/// run passes, and the table shows both numbers. No clock: the numbers are what heavy's timing line would say.
#[test]
fn a_long_queue_wait_followed_by_a_short_run_passes() {
    let planned = vec![heavy_gate("clippy a", 1), heavy_gate("clippy b", 2), heavy_gate("clippy c", 3)];
    let fake = Fake {
        timing: Some(Timing { queued: 5400.0, ran: 3.0 }),
        ..Fake::default()
    };
    let (rows, timings) = run_all_timed(
        &planned,
        &fake,
        &RunOpts {
            workers: 3,
            queue_stall: QUEUE_STALL,
            stop: &AtomicBool::new(false),
            subject: &test_subject(),
            reuse: None,
        },
    );
    assert!(
        rows.iter().all(|(_, v)| passed(v) && (v.secs - 3.0).abs() < 1e-9),
        "an hour and a half queued is not run time: {rows:?}"
    );
    let report = render_timed("h", &rows, &timings);
    assert!(report.text.contains("3.0 s run, 5400.0 s queued"), "{}", report.text);
}

/// The other half of the same evidence: there is no total shared by the rows. Sixteen rows that each run 700 s (3 h 7 min together,
/// over the old 15-minute budget twelve times) all get to run and pass.
#[test]
fn rows_that_together_run_far_longer_than_the_old_total_all_pass() {
    let planned: Vec<Planned> = (0..16).map(|i| heavy_gate(&format!("clippy w{i}"), 1)).collect();
    let fake = Fake {
        timing: Some(Timing { queued: 30.0, ran: 700.0 }),
        ..Fake::default()
    };
    let rows = run_all_with(
        &planned,
        &fake,
        &RunOpts {
            workers: 3,
            queue_stall: QUEUE_STALL,
            stop: &AtomicBool::new(false),
            subject: &test_subject(),
            reuse: None,
        },
    );
    assert_eq!(rows.len(), 16);
    assert!(rows.iter().all(|(_, v)| passed(v)), "{rows:?}");
    assert_eq!(fake.ran.lock().unwrap().len(), 16, "every row started");
}

/// A queue that made no progress is "not measured" with the holder named and the next step, never a pass (ER-33).
#[test]
fn a_queue_with_no_progress_is_not_measured_and_names_the_holder() {
    let d = tempfile::tempdir().unwrap();
    let log = d.path().join("step.log");
    std::fs::write(
        &log,
        "pre-push-7-1 queue stalled: the queue made no progress for 1800 s (queued 1801.2 s, held by agent-9 (pid 4242)), so the command never ran\n",
    )
    .unwrap();
    let planned = vec![heavy_gate("clippy a", 1), heavy_gate("clippy b", 2)];
    let fake = Fake {
        stalled: Some((vec!["clippy a".into()], log.display().to_string())),
        ..Fake::default()
    };
    let (rows, timings) = run_all_timed(
        &planned,
        &fake,
        &RunOpts {
            workers: 1,
            queue_stall: QUEUE_STALL,
            stop: &AtomicBool::new(false),
            subject: &test_subject(),
            reuse: None,
        },
    );
    let v = verdicts(&rows);
    let Some((reason, next)) = nm(&v["clippy a"]) else {
        panic!("{:?}", v["clippy a"])
    };
    assert!(
        reason.starts_with("queue stalled, held by agent-9 (pid 4242)") && reason.contains("1801.2 s"),
        "{reason}"
    );
    assert!(
        next.contains("--only clippy") && next.contains("throttle status") && next.contains("30 min"),
        "{next}"
    );
    assert!(
        passed(&v["clippy b"]),
        "the control: the other row's queue moved, it ran: {:?}",
        v["clippy b"]
    );
    assert_eq!(render_timed("h", &rows, &timings).exit, Exit::NotMeasured);
}

/// A row that really runs past its own cap is not measured and is never a pass (ER-33): the cap is the guard that stays.
#[test]
fn a_row_over_its_own_cap_is_still_not_measured() {
    let planned = vec![heavy_gate("clippy a", 1), heavy_gate("clippy b", 2)];
    let fake = Fake {
        cut_short: vec!["clippy a".into()],
        ..Fake::default()
    };
    let rows = run_all(&planned, &fake, 1);
    let v = verdicts(&rows);
    let Some((reason, _)) = nm(&v["clippy a"]) else {
        panic!("{:?}", v["clippy a"])
    };
    assert!(reason.contains("timed out"), "{reason}");
    assert!(passed(&v["clippy b"]), "the control: the row that ended by itself passes");
}

/// ADR 0089 amendment A2: an environment that is not ready is not measured (exit 6), not a broken check (exit 7).
#[test]
fn an_environment_not_ready_ends_the_run_with_exit_6_not_7() {
    let mut f = facts();
    f.conformance_env = Some("node_modules/yaml is missing".into());
    let planned = plan_gates(&plan(), &change(&["conformance/a.mjs"]), &f);
    let rows = run_all(
        &planned.into_iter().filter(|p| !matches!(p, Planned::Run(_))).collect::<Vec<_>>(),
        &Fake::default(),
        1,
    );
    let report = render("h", &rows);
    assert_eq!(report.exit, Exit::NotMeasured, "{}", report.text);
    assert!(
        report.text.contains("NOT MEASURED") && report.text.contains("not a finding in your change"),
        "{}",
        report.text
    );
}

/// A heavy step that reports `queued` of queue time and then runs at once; it records how many heavy steps of the push were in flight
/// together and the limits each started with.
struct Queued {
    queued: Duration,
    in_flight: std::sync::atomic::AtomicUsize,
    most: std::sync::atomic::AtomicUsize,
    limits: Mutex<Vec<(String, Limits)>>,
    /// The steps in flight, and for each step the ones that were in flight when it entered.
    running: Mutex<Vec<String>>,
    beside: Mutex<Vec<(String, Vec<String>)>>,
    /// Steps that have entered `run`, and a signal for each entry.
    entered: Mutex<usize>,
    entry: std::sync::Condvar,
    /// With a number: every step waits until that many have entered (a first batch that is in flight together by construction, not by
    /// luck of timing under load, ER-12); the wait is bounded only as a hang detector. Zero: the older wait for one more entry.
    batch: usize,
}

impl Queued {
    fn new(queued: Duration) -> Queued {
        Queued {
            queued,
            in_flight: Default::default(),
            most: Default::default(),
            limits: Mutex::new(Vec::new()),
            running: Mutex::new(Vec::new()),
            beside: Mutex::new(Vec::new()),
            entered: Mutex::new(0),
            entry: std::sync::Condvar::new(),
            batch: 0,
        }
    }

    /// Holds every step until `n` of them are in flight together.
    fn with_batch(mut self, n: usize) -> Queued {
        self.batch = n;
        self
    }
}

impl Exec for Queued {
    fn has_tool(&self, _: &str) -> bool {
        true
    }
    fn load(&self) -> Option<f64> {
        Some(28.0)
    }
    fn run(&self, g: &Gate, limits: Limits) -> Result<Done, String> {
        use std::sync::atomic::Ordering::SeqCst;
        self.limits.lock().unwrap().push((g.id.clone(), limits));
        {
            let mut running = self.running.lock().unwrap();
            self.beside.lock().unwrap().push((g.id.clone(), running.clone()));
            running.push(g.id.clone());
        }
        let now = self.in_flight.fetch_add(1, SeqCst) + 1;
        self.most.fetch_max(now, SeqCst);
        // Wait for a second step to enter while this one is in flight: it does at once unless the push holds it back. The
        // bound only ends the wait when it is held back (the expected case), it settles nothing.
        let mut n = self.entered.lock().unwrap();
        *n += 1;
        self.entry.notify_all();
        let mine = *n;
        let _held = if self.batch > 0 {
            // A hang detector, not a timing: it ends only if a step of the batch never arrives (a push that holds it back).
            self.entry
                .wait_timeout_while(n, Duration::from_secs(60), |n| *n < self.batch)
                .unwrap()
                .0
        } else {
            self.entry
                .wait_timeout_while(n, Duration::from_millis(200), |n| *n == mine)
                .unwrap()
                .0
        };
        drop(_held);
        self.running.lock().unwrap().retain(|id| *id != g.id);
        self.in_flight.fetch_sub(1, SeqCst);
        Ok(Done {
            success: true,
            how: "exit 0".into(),
            tail: String::new(),
            log: None,
            timing: Some(Timing {
                queued: self.queued.as_secs_f64(),
                ran: 0.0,
            }),
            ending: crate::pre_push::exec::Ending::Exited,
        })
    }
}

fn heavy_gate(id: &str, rank: u8) -> Planned {
    Planned::Run(Gate {
        id: id.into(),
        cwd: String::new(),
        program: "true".into(),
        args: vec![],
        env: Vec::new(),
        heavy: true,
        need: Need::Nothing,
        rank,
        next: "fix it".into(),
        workspaces: Vec::new(),
        notice: None,
        progress: crate::pre_push::progress::Progress::Lines,
        build_dir: None,
        ci_env: false,
    })
}

/// A push runs as many heavy steps at once as it has workers (ADR 0102 amendment 2026-11-26: the user asked for parallel rows; heavy's
/// slots and the load decide the rest), and the run's own measure says what really ran. The executor counts the steps really in flight;
/// `peak_heavy` and each row's concurrency must agree, and reach the worker count (a positive control: a measure that never saw a step
/// would also read "never above the limit", ER-33). The queued seconds of a row stay apart from its run time (the row-timings table).
#[test]
fn a_push_runs_as_many_heavy_steps_at_once_as_it_has_workers_and_measures_it() {
    use crate::pre_push::exec::run_all_detailed;
    let fake = Queued::new(Duration::from_millis(30)).with_batch(3);
    let planned = vec![
        heavy_gate("clippy a", 1),
        heavy_gate("clippy b", 1),
        heavy_gate("clippy c", 1),
        heavy_gate("clippy d", 1),
    ];
    let detail = run_all_detailed(
        &planned,
        &fake,
        &RunOpts {
            workers: 3,
            queue_stall: Duration::from_secs(60),
            stop: &AtomicBool::new(false),
            subject: &test_subject(),
            reuse: None,
        },
    );
    assert!(detail.rows.iter().all(|(_, v)| v.kind() == Kind::Passed), "{:?}", detail.rows);
    let seen = fake.most.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(seen, 3, "three workers, three heavy steps in flight at once");
    assert_eq!(detail.peak_heavy, seen, "the run's measure agrees with what really ran");
    assert_eq!(detail.concurrency.len(), 4, "every row that ran has its concurrency");
    assert!(
        detail.concurrency.values().all(|&c| (1..=3).contains(&c)),
        "{:?}",
        detail.concurrency
    );
    assert_eq!(
        detail.concurrency.values().copied().max(),
        Some(3),
        "a row started beside two other heavy ones"
    );
    let lines = crate::pre_push::row_timings::lines(&detail);
    assert_eq!(lines.len(), 4);
    assert!(crate::pre_push::row_timings::table(&lines, 3).is_some_and(|t| t.contains("clippy d") && t.contains("at most 3")));
}

/// The limit is the worker count unless `PADDOCK_HEAVY_AT_ONCE` (1 or more) says otherwise, for a measurement; the run honours the limit
/// it is given, at 1 (the behaviour before 2026-11-26) and 2.
#[test]
fn the_heavy_limit_is_the_worker_count_unless_a_measurement_sets_it_and_is_honoured() {
    use crate::pre_push::exec::{heavy_limit, run_all_limited};
    let env = |v: &str| std::collections::HashMap::from([("PADDOCK_HEAVY_AT_ONCE".to_string(), v.to_string())]);
    assert_eq!(heavy_limit(&std::collections::HashMap::new(), 3), 3);
    assert_eq!(heavy_limit(&env("1"), 3), 1);
    assert_eq!(heavy_limit(&env("5"), 3), 5);
    for bad in ["0", "two", "", "-1"] {
        assert_eq!(heavy_limit(&env(bad), 3), 3, "{bad:?}");
    }
    assert_eq!(heavy_limit(&std::collections::HashMap::new(), 0), 1, "never below one");
    for limit in [1, 2] {
        let fake = Queued::new(Duration::from_millis(30)).with_batch(limit);
        let planned: Vec<Planned> = (0..5).map(|i| heavy_gate(&format!("clippy {i}"), 1)).collect();
        let detail = run_all_limited(
            &planned,
            &fake,
            &RunOpts {
                workers: 5,
                queue_stall: Duration::from_secs(60),
                stop: &AtomicBool::new(false),
                subject: &test_subject(),
                reuse: None,
            },
            limit,
        );
        assert_eq!(fake.most.load(std::sync::atomic::Ordering::SeqCst), limit);
        assert_eq!(detail.peak_heavy, limit);
    }
}

/// The 2026-11-14/15 shape: train 10 is being built locally (partly pushed or not at all), a fix branch is stacked on it. The push
/// sends train 10's unpushed commits too, so the check covers them: the base is the nearest commit of the remote (origin's train 9,
/// or train 10 as far as it is pushed), never the local train, whatever a local ref is called (review of c4e32e462, P1-1).
#[test]
fn a_branch_stacked_on_a_local_train_checks_the_trains_unpushed_commits_too() {
    let r = Repo::new();
    let c1 = r.commit("a");
    r.remote("main", &c1);
    r.git(&["checkout", "-q", "-b", "train/2026-11-13-9"]);
    let t1 = r.commit("t1");
    r.remote("train/2026-11-13-9", &t1);
    r.git(&["checkout", "-q", "-b", "train/2026-11-14-10"]);
    let t2 = r.commit("t2");
    r.commit("t3");
    r.git(&["checkout", "-q", "-b", "fix"]);
    let f1 = r.commit("f1");
    let (base, why) = r.choose(&new_ref("fix", &f1));
    assert_eq!(base, t1, "{why}");
    assert!(
        why.contains("origin/train/2026-11-13-9") && why.contains("fewest new commits, 3"),
        "{why}"
    );
    let (change, _, _) = super::discover(&r.root, &base, &[], std::slice::from_ref(&f1)).unwrap();
    assert_eq!(
        change.files,
        ["f1", "t2", "t3"].map(String::from).to_vec(),
        "the train's unpushed commits are checked"
    );
    // Partly pushed (origin has train 10 at t2): measured from there, t3 still checked.
    r.remote("train/2026-11-14-10", &t2);
    let (base, why) = r.choose(&new_ref("fix", &f1));
    assert_eq!(base, t2, "{why}");
    let (change, _, _) = super::discover(&r.root, &base, &[], std::slice::from_ref(&f1)).unwrap();
    assert_eq!(change.files, ["f1", "t3"].map(String::from).to_vec());
}

/// 2026-11-17: the remote's train was amended after the local train was cut (the two share t1, then differ), so `--merged` dropped
/// the remote train and the base fell back to main: 162 commits of unrelated work. The merge base of the pushed commit and a
/// remote main or train is on the remote too, so it is a candidate: the nearest one wins.
#[test]
fn a_remote_train_that_diverged_still_gives_its_merge_base_as_the_base() {
    let r = Repo::new();
    let c1 = r.commit("a");
    r.remote("main", &c1);
    r.git(&["checkout", "-q", "-b", "train/x"]);
    let t1 = r.commit("t1");
    let t2 = r.commit("t2");
    r.git(&["checkout", "-q", "-b", "train-remote", &t1]);
    let rt = r.commit("t2-amended");
    r.remote("train/2026-11-17-14", &rt);
    r.git(&["checkout", "-q", "-b", "fix", &t2]);
    let f1 = r.commit("f1");
    let (base, why) = r.choose(&new_ref("fix", &f1));
    assert_eq!(base, t1, "{why}");
    assert!(
        why.contains("merge base with origin/train/2026-11-17-14") && why.contains("fewest new commits, 2"),
        "{why}"
    );
    // Positive control: without the merge-base candidates (only refs the pushed commit contains) the base is the far one, main.
    let contained = r.git(&[
        "for-each-ref",
        &format!("--merged={f1}"),
        "--format=%(refname)",
        "refs/remotes/origin/",
    ]);
    assert_eq!(contained.trim(), "refs/remotes/origin/main", "the old selection sees only main");
    // A diverged remote branch that is neither main nor a train is no candidate.
    let r2 = Repo::new();
    let c = r2.commit("a");
    r2.remote("main", &c);
    r2.git(&["checkout", "-q", "-b", "feat/x"]);
    let x1 = r2.commit("x1");
    r2.git(&["checkout", "-q", "-b", "feat/y", &c]);
    let y1 = r2.commit("y1");
    r2.remote("feat/x", &x1);
    r2.git(&["checkout", "-q", "-b", "fix", &y1]);
    let f = r2.commit("f");
    let (base, why) = r2.choose(&new_ref("fix", &f));
    assert_eq!(base, c, "{why}");
}

#[test]
fn the_first_line_counts_the_commits_beyond_the_base() {
    let r = Repo::new();
    let c1 = r.commit("a");
    r.commit("b");
    r.commit("c");
    assert_eq!(super::commit_count(&r.root, &c1, "HEAD"), "2");
    assert_eq!(
        super::commit_count(&r.root, "no-such-ref", "HEAD"),
        "?",
        "an unreadable base is shown, not counted as 0"
    );
}

/// A branch stacked on another pushed branch is measured from that branch (a remote-tracking ref it contains), not from main.
#[test]
fn a_branch_stacked_on_a_pushed_sibling_is_measured_from_the_sibling() {
    let r = Repo::new();
    let c1 = r.commit("a");
    r.remote("main", &c1);
    r.git(&["checkout", "-q", "-b", "feat/a"]);
    r.commit("a1");
    let a2 = r.commit("a2");
    r.remote("feat/a", &a2);
    r.git(&["checkout", "-q", "-b", "feat/b"]);
    let b1 = r.commit("b1");
    let (base, why) = r.choose(&new_ref("feat/b", &b1));
    assert_eq!(base, a2, "{why}");
    assert!(why.contains("origin/feat/a (remote-tracking ref"), "{why}");
}

/// A person's run ahead of a push plans what the push will: HEAD's nearest commit of the remote, not always origin/main.
#[test]
fn a_persons_run_is_measured_from_the_nearest_base_like_its_push() {
    let r = Repo::new();
    let c1 = r.commit("a");
    r.remote("main", &c1);
    r.git(&["checkout", "-q", "-b", "train/2026-11-14-10"]);
    let t1 = r.commit("t1");
    r.remote("train/2026-11-14-10", &t1);
    r.git(&["checkout", "-q", "-b", "fix"]);
    r.commit("f1");
    let (base, why) = super::person_base(&r.root, None, "origin/main", &r.live());
    assert_eq!(base, t1, "{why}");
    assert!(
        why.starts_with("a person's run: HEAD of branch fix") && why.contains("origin/train/2026-11-14-10"),
        "{why}"
    );
    // Its own pushed copy at HEAD is the base (the uncommitted changes are what is checked), not a reason to measure from main.
    let f1 = r.git(&["rev-parse", "HEAD"]).trim().to_string();
    r.remote("fix", &f1);
    assert_eq!(super::person_base(&r.root, None, "origin/main", &r.live()).0, f1);
}

/// While a heavy step is in flight, a free worker runs the next light row instead of waiting on a heavy one (review P2-4); at a limit of
/// one heavy step (`PADDOCK_HEAVY_AT_ONCE=1`, the behaviour before the 2026-11-26 amendment) never two heavy steps at once.
#[test]
fn a_free_worker_runs_light_rows_while_a_heavy_step_is_in_flight() {
    let fake = Queued::new(Duration::from_millis(10));
    let light = Planned::Run(Gate {
        heavy: false,
        ..match heavy_gate("fmt x", 2) {
            Planned::Run(g) => g,
            _ => unreachable!(),
        }
    });
    let planned = vec![heavy_gate("clippy a", 1), heavy_gate("clippy b", 1), light];
    let rows = crate::pre_push::exec::run_all_limited(
        &planned,
        &fake,
        &RunOpts {
            workers: 2,
            queue_stall: Duration::from_secs(60),
            stop: &AtomicBool::new(false),
            subject: &test_subject(),
            reuse: None,
        },
        1,
    )
    .rows;
    assert!(rows.iter().all(|(_, v)| v.kind() == Kind::Passed), "{rows:?}");
    let beside = fake.beside.lock().unwrap().clone();
    // The first heavy step and the light row overlapped (whichever entered first); held behind the second heavy row, the
    // light row would start only after the first heavy step ended.
    let with = |a: &str, b: &str| beside.iter().any(|(id, w)| id == a && w.iter().any(|x| x == b));
    assert!(with("fmt x", "clippy a") || with("clippy a", "fmt x"), "{beside:?}");
    let heavy_beside: Vec<_> = beside.iter().filter(|(id, _)| id.starts_with("clippy")).collect();
    assert!(
        heavy_beside.iter().all(|(_, with)| !with.iter().any(|w| w.starts_with("clippy"))),
        "never two heavy steps at once: {beside:?}"
    );
}

/// One stack shape of the property tests below.
struct Shape {
    /// Commits on the local train, of which the first `pushed` are on the remote's `train/x`.
    train: usize,
    pushed: usize,
    /// A pushed sibling branch at the last train commit (so the remote holds the train through it).
    sibling: bool,
    /// A file added and removed again on the branch.
    undone: bool,
    /// An unpushed local side branch merged in with `--no-ff`.
    merge: bool,
    /// A file added and removed again inside that side branch.
    merge_undone: bool,
    /// The remote has an old `fix` the pushed commit does not contain (a force push).
    force: bool,
    /// The branch moves a file of apps/dyno (that main has) into tools/ with `git mv`.
    rename: bool,
    /// The branch adds a file whose path is not ASCII (git quotes it unless told not to).
    non_ascii: bool,
}

/// Property (review of c4e32e462, P1-1): for the stack `sh`, live and with `git ls-remote` unreadable, every commit the remote
/// lacks has its files in the planned diff, and a commit the remote lacks in apps/dyno (an unpushed train commit, or the old name
/// of a moved file) plans that workspace's Clippy. The oracle lists the files itself (`git log --no-renames -z`), independent of
/// the code under test. The shapes are enumerated, each train state once and each flag on and off in at least one of them, one
/// `#[test]` each so nextest runs them in parallel and a failure names its shape: 2026-11-15 the first version (24 random cases
/// in one test, 1,651 git processes) hit nextest's 60 s limit on the Windows runner (run 8138).
fn check_shape(name: &str, sh: &Shape) {
    let r = Repo::new();
    let add = |path: &str| {
        let full = r.root.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, "x\n").unwrap();
        r.git(&["add", path]);
        r.git(&["commit", "-q", "-m", path]);
        r.git(&["rev-parse", "HEAD"]).trim().to_string()
    };
    let undo = |path: &str| {
        add(path);
        r.git(&["rm", "-q", path]);
        r.git(&["commit", "-q", "-m", "undo"]);
    };
    add("README");
    let c0 = add("apps/dyno/src/m.rs");
    r.remote("main", &c0);
    r.git(&["checkout", "-q", "-b", "train/x"]);
    let train: Vec<String> = (0..sh.train).map(|i| add(&format!("apps/dyno/src/t{i}.rs"))).collect();
    if sh.pushed > 0 {
        r.remote("train/x", &train[sh.pushed - 1]);
    }
    if sh.sibling {
        r.remote("feat/sibling", train.last().unwrap());
    }
    let train_tip = train.last().cloned().unwrap_or_else(|| c0.clone());
    r.git(&["checkout", "-q", "-b", "fix"]);
    add("tools/x/f0.rs");
    if sh.undone {
        undo("apps/pitwall/src/undone.rs");
    }
    if sh.rename {
        r.git(&["mv", "apps/dyno/src/m.rs", "tools/x/m.rs"]);
        r.git(&["commit", "-q", "-m", "move"]);
    }
    if sh.non_ascii {
        add("tools/x/n\u{e4}me-\u{fc}.rs");
    }
    let mut side = None;
    if sh.merge {
        r.git(&["checkout", "-q", "-b", "side"]);
        side = Some(add("apps/pitwall/src/side.rs"));
        if sh.merge_undone {
            undo("apps/pitcrew/src/side-undone.rs");
        }
        r.git(&["checkout", "-q", "fix"]);
        add("tools/x/before-merge.rs");
        r.git(&["merge", "-q", "--no-ff", "--no-edit", "side"]);
    }
    let mut push = new_ref("fix", "");
    if sh.force {
        r.git(&["checkout", "-q", "-b", "old-fix", &c0]);
        push.remote_sha = add("tools/x/old.rs");
        r.remote("fix", &push.remote_sha);
        r.git(&["checkout", "-q", "fix"]);
    }
    push.sha = r.git(&["rev-parse", "HEAD"]).trim().to_string();
    let tip = push.sha.clone();
    // The oracle: every commit the remote lacks with its files, in one git process, both names of a move, paths unquoted.
    let log = r.git(&[
        "log",
        "--no-merges",
        "--no-renames",
        "-z",
        "--name-only",
        "--format=commit %H",
        &tip,
        "--not",
        "--remotes=origin",
    ]);
    let (mut lacks, mut wanted) = (Vec::<String>::new(), Vec::<(String, String)>::new());
    for t in log.split('\0').map(|t| t.trim_start_matches('\n')).filter(|t| !t.is_empty()) {
        match t.strip_prefix("commit ") {
            Some(c) => lacks.push(c.to_string()),
            None => wanted.push((lacks.last().cloned().unwrap_or_default(), t.to_string())),
        }
    }
    // The shape is what it says.
    let unpushed_train = train.iter().any(|t| lacks.contains(t));
    assert_eq!(unpushed_train, sh.pushed < sh.train && !sh.sibling, "{name}: train state");
    if let Some(side) = &side {
        assert!(lacks.contains(side), "{name}: the oracle saw the side branch's commit");
    }
    if sh.rename {
        assert!(
            wanted.iter().any(|(_, f)| f == "apps/dyno/src/m.rs"),
            "{name}: the oracle lists the moved file's old name"
        );
    }
    if sh.non_ascii {
        assert!(
            wanted.iter().any(|(_, f)| f == "tools/x/n\u{e4}me-\u{fc}.rs"),
            "{name}: the oracle lists the path unquoted"
        );
    }
    let dyno = unpushed_train || sh.rename;
    for (how, look) in [("live", r.live()), ("unreadable", base_choice::Remote(None))] {
        let (base, why) = super::choose_base(&r.root, &push, Some("origin"), "origin/main", &look);
        if sh.force {
            assert!(why.contains("(a force push)"), "{name} ({how}): {why}");
        }
        let (change, _, _) = super::discover(&r.root, &base, &[], std::slice::from_ref(&tip)).unwrap();
        for (c, f) in &wanted {
            assert!(
                change.files.contains(f),
                "{name} ({how}): {f} of {c} is not planned; base {base} ({why})"
            );
        }
        if dyno {
            let got = ids(&plan_gates(&plan(), &change, &facts()));
            assert!(
                got.contains(&"clippy apps/dyno".to_string()),
                "{name} ({how}): {got:?}; base {base} ({why})"
            );
        }
    }
    if unpushed_train && !sh.rename {
        // The branch's own diff (from the train's local tip) does not reach apps/dyno; the push's check must.
        let (own, _, _) = super::discover(&r.root, &train_tip, &[], std::slice::from_ref(&tip)).unwrap();
        assert!(
            !ids(&plan_gates(&plan(), &own, &facts())).contains(&"clippy apps/dyno".to_string()),
            "{name}"
        );
    }
}

#[rustfmt::skip]
const NONE: Shape = Shape { train: 0, pushed: 0, sibling: false, undone: false, merge: false, merge_undone: false, force: false, rename: false, non_ascii: false };

#[test]
fn shape_unpushed_train_with_undone_changes_a_merge_and_a_force_push() {
    check_shape(
        "unpushed train",
        &Shape {
            train: 2,
            undone: true,
            merge: true,
            merge_undone: true,
            force: true,
            ..NONE
        },
    );
}

#[test]
fn shape_partly_pushed_train() {
    check_shape(
        "partly pushed train",
        &Shape {
            train: 2,
            pushed: 1,
            ..NONE
        },
    );
}

#[test]
fn shape_wholly_pushed_train_with_a_merge() {
    check_shape(
        "wholly pushed train",
        &Shape {
            train: 2,
            pushed: 2,
            merge: true,
            ..NONE
        },
    );
}

#[test]
fn shape_no_train_with_a_move_out_of_a_workspace_and_a_force_push() {
    check_shape(
        "no train",
        &Shape {
            undone: true,
            force: true,
            rename: true,
            ..NONE
        },
    );
}

#[test]
fn shape_train_on_the_remote_through_a_sibling_with_a_non_ascii_path() {
    check_shape(
        "train via sibling",
        &Shape {
            train: 1,
            sibling: true,
            merge: true,
            non_ascii: true,
            ..NONE
        },
    );
}

/// A `cargo` and `cargo-deny` stand-in (ER-14: the real tool never runs, nothing reaches the network): `cargo deny ...` records its
/// arguments and fails like the real one when the checks it was asked for have a finding. `adv` and `lic` are files in its folder
/// that switch an advisory and a licence finding on; a run with no category named checks all four.
#[cfg(unix)]
mod deny_row {
    use super::*;
    use crate::pre_push::exec::{RealExec, decide};
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Arc;

    fn stand_in(d: &Path) -> RealExec {
        let bin = d.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(d.join("apps/pitwall")).unwrap();
        use paddock_testkit::{Actions, Case, StandIn};
        let calls = d.join("calls").display().to_string();
        let categories = ["advisories", "licenses", "bans", "sources"];
        for name in ["cargo", "cargo-deny"] {
            let mut tool = StandIn::new(&bin, name).answers(Actions::new().appends_with(&calls, "{args}\n"));
            // Licences first and advisories after, so the later case is asked first (as the script checked advisories first). A
            // finding fails a `deny` call that names its category, or none (all four are checked then).
            for (file, category, message) in [
                ("lic", "licenses", "error[rejected]: licence GPL-3.0 is not allowed"),
                ("adv", "advisories", "error[vulnerability]: RUSTSEC-2026-9999 lettre"),
            ] {
                let fails = Actions::new().appends_with(&calls, "{args}\n").stderr(message).exit(1);
                let deny = Case::new().args_start_with(&["deny"]).file_exists(&d.join(file));
                tool = tool
                    .when(&deny.clone().has_none_of(&categories), fails.clone())
                    .when(&deny.has_arg(category), fails);
            }
            tool.install().unwrap();
        }
        RealExec {
            root: d.to_path_buf(),
            env: HashMap::from([
                (
                    "PATH".to_string(),
                    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()),
                ),
                ("HOME".to_string(), d.display().to_string()),
            ]),
            logs: d.join("logs"),
            stop: Arc::new(AtomicBool::new(false)),
            built_throttle: Default::default(),
            live_slot: Default::default(),
            priority: false,
            wrapper: crate::local_sccache::Lazy::off(),
            watchdog: Arc::new(crate::pre_push::watchdog::Never),
            row_cap: crate::throttle::ROW_CAP,
            state: d.join("state"),
        }
    }

    /// The deny row of a push that changes `files` (base known unless `base` is None), run on the stand-in.
    fn push(d: &Path, files: &[&str], base: Option<&str>) -> (Row, Vec<Planned>) {
        let ex = stand_in(d);
        let mut c = change(files);
        c.base = base.map(String::from);
        let planned = plan_gates(&plan(), &c, &facts());
        let g = run_gates(&planned)
            .into_iter()
            .find(|g| g.id.starts_with("pitwall-deny"))
            .unwrap()
            .clone();
        (decide(&g, &ex, &test_subject()), planned)
    }

    fn notice(planned: &[Planned]) -> Option<String> {
        run_gates(planned)
            .into_iter()
            .find(|g| g.id.starts_with("pitwall-deny"))
            .unwrap()
            .notice
            .clone()
    }

    #[test]
    fn a_push_without_a_lockfile_change_skips_advisories_says_so_and_still_fails_on_a_licence() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("adv"), "").unwrap();
        // The published advisory does not turn a push red that touches no dependency.
        let (row, planned) = push(d.path(), &["apps/pitwall/src/a.rs"], Some("origin/main"));
        assert_eq!(row.kind(), Kind::Passed, "{row:?}");
        let n = notice(&planned).expect("a skip prints its line");
        assert!(
            n.contains("advisories skipped") && n.contains("nightly") && n.contains("apps/pitwall"),
            "{n}"
        );
        let calls = std::fs::read_to_string(d.path().join("calls")).unwrap();
        assert!(
            calls.contains("deny --workspace check licenses bans sources") && !calls.contains("advisories"),
            "{calls}"
        );
        // Positive control: the same push with a licence violation is red.
        std::fs::write(d.path().join("lic"), "").unwrap();
        let (row, _) = push(d.path(), &["apps/pitwall/src/a.rs"], Some("origin/main"));
        assert_eq!(row.kind(), Kind::Failed, "licences still run: {row:?}");
    }

    #[test]
    fn a_push_that_changes_a_lockfile_or_manifest_runs_advisories_and_fails_on_one() {
        for file in [
            "apps/pitwall/app/src-tauri/Cargo.lock",
            "apps/pitwall/crates/x/Cargo.toml",
            "apps/pitwall/deny.toml",
            "crates/paddock-proc/Cargo.toml",
            ".cargo/config.toml",
            "apps/pitwall/app/.cargo/config.toml",
        ] {
            let d = tempfile::tempdir().unwrap();
            let (row, planned) = push(d.path(), &[file], Some("origin/main"));
            assert_eq!(row.kind(), Kind::Passed, "no advisory published: {file}: {row:?}");
            assert!(notice(&planned).is_none(), "{file}");
            std::fs::write(d.path().join("adv"), "").unwrap();
            let (row, _) = push(d.path(), &[file], Some("origin/main"));
            assert_eq!(row.kind(), Kind::Failed, "{file}: {row:?}");
            assert!(std::fs::read_to_string(d.path().join("calls")).unwrap().lines().last().unwrap() == "deny --workspace check");
        }
    }

    #[test]
    fn an_unknown_base_runs_advisories() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("adv"), "").unwrap();
        let (row, planned) = push(d.path(), &["apps/pitwall/src/a.rs"], None);
        assert_eq!(row.kind(), Kind::Failed, "not measured never passes: {row:?}");
        assert!(notice(&planned).is_none());
    }

    #[test]
    fn the_next_step_names_the_command_that_ran() {
        let d = tempfile::tempdir().unwrap();
        for (file, want) in [
            ("apps/pitwall/src/a.rs", "cargo deny --workspace check licenses bans sources"),
            ("apps/pitwall/Cargo.lock", "cargo deny --workspace check`"),
        ] {
            let (_, planned) = push(d.path(), &[file], Some("origin/main"));
            let g = run_gates(&planned).into_iter().find(|g| g.id.starts_with("pitwall-deny")).unwrap();
            assert!(g.next.contains(want), "{}", g.next);
        }
    }
}

/// A file list the caller gave is not measured from the base, so what judges only "what the push adds" (the deny advisories) must
/// run in full; a measured push and a person's run with no list keep the base.
#[test]
fn a_given_file_list_has_no_base_and_a_measured_change_has_one() {
    let r = Repo::new();
    let c1 = r.commit("a");
    r.remote("main", &c1);
    let c2 = r.commit("b");
    let (given, _, _) = super::discover(&r.root, &c1, &["apps/pitwall/src/a.rs".to_string()], &[]).unwrap();
    assert_eq!(given.base, None);
    let (measured, _, _) = super::discover(&r.root, &c1, &[], &[]).unwrap();
    assert_eq!(measured.base.as_deref(), Some(c1.as_str()));
    let (pushed, _, _) = super::discover(&r.root, &c1, &[], std::slice::from_ref(&c2)).unwrap();
    assert_eq!(pushed.base.as_deref(), Some(c1.as_str()));
}

/// The contracts line of the conformance `tests` phase compiles paddock-dev's tests, so the row hands the executor the build folder
/// (as configuration, through `PADDOCK_GATE_TARGET_DIR`, never a CARGO_* variable) and runs through heavy (ER-24).
#[test]
fn the_conformance_tests_row_passes_the_build_folder_to_the_executor_and_runs_heavy() {
    let unrelated = json!({"conformance": false, "apps": [], "packages": {}});
    let planned = plan_gates(&unrelated, &change(&["docs/x.md"]), &facts());
    let g = run_gates(&planned)
        .into_iter()
        .find(|g| g.id == "conformance-tests")
        .cloned()
        .unwrap_or_else(|| panic!("{planned:?}"));
    assert!(
        g.env.contains(&("PADDOCK_GATE_TARGET_DIR".to_string(), "/t".to_string())),
        "{:?}",
        g.env
    );
    assert!(g.env.iter().all(|(k, _)| !k.starts_with("CARGO_")), "{:?}", g.env);
    assert!(g.heavy);
}

/// Rule 50 for the member gate: `./dev merge check --gates` plans the rows from the first parent's diff (`--base <merge>^1`), so a
/// docs-only member plans no per-workspace test, Clippy or fmt row. The only test-time rows left are the two repository-wide
/// ones the user put on every change (ER-41, 2026-11-14 and 2026-11-17): `conformance-tests` and `member-audits`. They are the
/// named exceptions; a third one would fail this test.
#[test]
fn a_docs_only_change_plans_no_per_workspace_test_clippy_or_fmt_row_in_the_member_gate() {
    let docs = ids(&plan_gates(&plan(), &change(&["docs/adr/0999-x.md"]), &facts()));
    let heavy_words = ["test", "nextest", "doctest", "clippy", "fmt", "vitest", "playwright"];
    let found: Vec<&str> = docs
        .iter()
        .map(String::as_str)
        .filter(|id| heavy_words.iter().any(|w| id.contains(w)))
        .collect();
    assert_eq!(found, ["conformance-tests"], "test-time rows of a docs-only change: {docs:?}");
    assert!(docs.iter().any(|id| id == "member-audits"), "{docs:?}");
    // The positive control (ER-33): the same probe sees the rows a code change plans.
    let code = ids(&plan_gates(&plan(), &change(&["crates/paddock-proc/src/lib.rs"]), &facts()));
    assert!(
        code.iter().any(|id| id.contains("clippy")) && code.iter().any(|id| id.starts_with("fmt")),
        "{code:?}"
    );
}

mod only_selection {
    use super::super::{gates::Planned, select_rows};

    fn rows() -> Vec<Planned> {
        ["lockfile crates", "clippy crates", "fmt"]
            .iter()
            .map(|id| Planned::Skip {
                id: (*id).into(),
                reason: "not reached".into(),
            })
            .collect()
    }

    fn ids(p: &[Planned]) -> Vec<String> {
        p.iter()
            .map(|r| match r {
                Planned::Skip { id, .. } => id.clone(),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    /// Positive control: the same rows with a selector that names them are kept, so the error below is the selection's, not the rows'.
    #[test]
    fn a_whole_row_name_and_a_first_word_both_select() {
        let got = select_rows(rows(), &["lockfile crates".into()]).unwrap();
        assert_eq!(ids(&got), ["lockfile crates"]);
        let got = select_rows(rows(), &["clippy".into(), "fmt".into()]).unwrap();
        assert_eq!(ids(&got), ["clippy crates", "fmt"]);
        assert_eq!(select_rows(rows(), &[]).unwrap().len(), 3, "no selector keeps every row");
    }

    #[test]
    fn a_selection_that_matches_no_row_is_an_error_naming_it_and_the_valid_rows_never_an_empty_pass() {
        let e = select_rows(rows(), &["no such gate".into()]).unwrap_err();
        let shown = e.text();
        assert!(shown.contains("no such gate"), "{shown}");
        for valid in ["lockfile crates", "clippy crates", "fmt"] {
            assert!(shown.contains(valid), "{valid} missing from {shown}");
        }
        assert!(
            select_rows(Vec::new(), &["fmt".into()]).is_err(),
            "no rows at all is also not a pass"
        );
    }
}

/// The active apps (docs/ci-active-apps.toml, user 2026-11-24): the push and member gate run the foundation's rows and the active
/// apps' rows; an inactive app's own build checks are named as inactive, never run and never a pass.
mod active_apps {
    use super::*;
    use crate::pre_push::gates::{inactive_label, inactive_reason};
    use crate::pre_push::{inactive_apps, inactive_note};

    fn off(apps: &[&str]) -> Facts {
        Facts {
            inactive: Ok(apps.iter().map(|a| a.to_string()).collect()),
            ..facts()
        }
    }

    const FILES: &[&str] = &["crates/paddock-proc/src/lib.rs", "apps/dyno/src/a.rs", "apps/pitwall/src/a.rs"];

    fn skipped<'a>(planned: &'a [Planned], id: &str) -> Option<&'a str> {
        planned.iter().find_map(|p| match p {
            Planned::Skip { id: i, reason } if i == id => Some(reason.as_str()),
            _ => None,
        })
    }

    #[test]
    fn an_inactive_apps_own_rows_are_skipped_and_named_as_inactive() {
        let planned = plan_gates(&plan(), &change(FILES), &off(&["dyno", "pitwall", "debrief"]));
        let ran = ids(&planned);
        for ws in ["apps/dyno", "apps/pitwall"] {
            for kind in ["fmt", "clippy", "clippy-windows"] {
                let id = format!("{kind} {ws}");
                assert!(!ran.iter().any(|r| r == &id), "{id} ran: {ran:?}");
                let reason = skipped(&planned, &id).unwrap_or_else(|| panic!("{id} is not named: {planned:?}"));
                assert!(reason.starts_with(&inactive_label()), "{id}: {reason}");
            }
            // The lockfile is a generated file: it stays current whether the app is active or not (ER-40).
            assert!(ran.contains(&format!("lockfile {ws}")), "{ran:?}");
        }
        assert_eq!(inactive_label(), "inactive (docs/ci-active-apps.toml)");
        assert_eq!(
            skipped(&planned, "pitwall-scripts, pitwall-deny"),
            Some(inactive_reason("pitwall").as_str())
        );
        assert!(!ran.iter().any(|r| r.starts_with("pitwall-")), "{ran:?}");
    }

    /// Positive control: the same change with the apps active runs their rows (the skip above is the set's, not the plan's).
    #[test]
    fn an_active_apps_rows_run() {
        let planned = plan_gates(&plan(), &change(FILES), &off(&["debrief", "pitwall"]));
        let ran = ids(&planned);
        for id in ["fmt apps/dyno", "clippy apps/dyno", "clippy-windows apps/dyno"] {
            assert!(ran.contains(&id.to_string()), "{id} did not run: {ran:?}");
        }
        assert!(skipped(&planned, "clippy apps/pitwall").is_some_and(|r| r.starts_with(&inactive_label())));
    }

    #[test]
    fn the_foundation_rows_and_the_generators_always_run() {
        let files = ["crates/paddock-proc/src/lib.rs", "apps/dyno/crates/dyno-core/src/a.rs"];
        let planned = plan_gates(&plan(), &change(&files), &off(&["dyno", "pitwall", "debrief"]));
        let ran = ids(&planned);
        for id in [
            "fmt crates",
            "clippy crates",
            "clippy-windows crates",
            "clippy-windows crates",
            "conformance",
            "conformance-tests",
            "member-audits",
            "gates-register",
            "dyno-wire-fixtures",
        ] {
            assert!(ran.contains(&id.to_string()), "{id} did not run: {ran:?}");
        }
    }

    #[test]
    fn a_set_that_cannot_be_read_runs_every_row_and_breaks_the_active_apps_row() {
        let f = Facts {
            inactive: Err("docs/ci-active-apps.toml cannot be read".into()),
            ..facts()
        };
        let planned = plan_gates(&plan(), &change(FILES), &f);
        assert!(ids(&planned).contains(&"clippy apps/dyno".to_string()));
        assert!(
            planned
                .iter()
                .any(|p| matches!(p, Planned::Broken { id, .. } if id == "active-apps")),
            "{planned:?}"
        );
        // Positive control: a set that was read has no such row.
        assert!(!format!("{:?}", plan_gates(&plan(), &change(FILES), &facts())).contains("active-apps"));
    }

    fn tree(active: &str) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("docs")).unwrap();
        std::fs::write(
            d.path().join(crate::active_apps::FILE),
            format!("decided = \"2026-11-24\"\nby = \"user\"\nreason = \"foundation first\"\nactive = {active}\nnext = []\n"),
        )
        .unwrap();
        d
    }

    #[test]
    fn the_set_is_read_through_its_accessor_and_all_apps_runs_every_app() {
        let apps: BTreeSet<String> = ["dyno", "pitwall"].map(String::from).into();
        let d = tree("[\"dyno\"]");
        assert_eq!(inactive_apps(d.path(), &apps, false).unwrap(), ["pitwall".to_string()].into());
        assert!(
            inactive_apps(d.path(), &apps, true).unwrap().is_empty(),
            "--all-apps leaves no app out"
        );
        let planned = plan_gates(
            &plan(),
            &change(FILES),
            &Facts {
                inactive: inactive_apps(d.path(), &apps, true),
                ..facts()
            },
        );
        assert!(
            ids(&planned).contains(&"clippy apps/pitwall".to_string()),
            "--all-apps runs an inactive app's rows"
        );
        assert!(inactive_note(&Ok(BTreeSet::new()), true).unwrap().contains("--all-apps"));
        let note = inactive_note(&inactive_apps(d.path(), &apps, false), false).unwrap();
        assert!(note.contains("pitwall") && note.contains("not passes"), "{note}");
        // A typo or an unreadable file is an error, never "no app" or "every app".
        assert!(inactive_apps(tree("[\"dyon\"]").path(), &apps, false).unwrap_err().contains("dyon"));
        assert!(inactive_apps(tempfile::tempdir().unwrap().path(), &apps, false).is_err());
    }

    #[test]
    fn the_summary_counts_inactive_rows_apart_and_never_as_passed() {
        let planned = plan_gates(&plan(), &change(FILES), &off(&["dyno", "pitwall", "debrief"]));
        let rows = run_all(&planned, &Fake::default(), 2);
        let n = rows.iter().filter(|(_, r)| r.reason().starts_with(&inactive_label())).count();
        // fmt, clippy and clippy-windows of apps/dyno and apps/pitwall, Pitwall's scripts and deny, and the member gate's host tests
        // of both.
        assert_eq!(n, 9, "{rows:?}");
        let report = render("h", &rows);
        let last = report.text.lines().find(|l| l.starts_with("pre-push: ")).unwrap_or_default();
        assert!(last.contains(&format!(", {n} inactive (not checked")), "{last}");
        assert_eq!(report.data["inactive"], json!(n));
        let passed = rows.iter().filter(|(_, r)| r.kind() == Kind::Passed).count();
        assert_eq!(report.data["passed"], json!(passed));
        for row in report.data["gates"].as_array().unwrap() {
            if row["id"] == "clippy apps/dyno" {
                assert_eq!(row["verdict"], "inactive");
            }
        }
        assert!(report.text.contains("inactive      clippy apps/dyno"), "{}", report.text);
        // A run of nothing but inactive rows proved nothing: its outcome is not a pass (ADR 0089).
        let only: Vec<(String, Row)> = rows
            .into_iter()
            .filter(|(_, r)| r.reason().starts_with(&inactive_label()))
            .collect();
        let (_, run) = super::super::outcome::fold(&only, &test_subject());
        assert!(!super::super::outcome::leaves_record(&run), "{run:?}");
        // Positive control: with every app active nothing is counted inactive.
        let all = run_all(&plan_gates(&plan(), &change(FILES), &facts()), &Fake::default(), 2);
        assert_eq!(render("h", &all).data["inactive"], json!(0));
    }
}

#[test]
fn the_conformance_tests_run_in_a_push_only_when_the_change_reaches_conformance_and_always_in_the_member_gate() {
    let always = json!({"conformance": true, "apps": [], "packages": {}, "why": {"conformance": [crate::plan::CONFORMANCE_ALWAYS]}});
    let change = change(&["apps/dyno/src/a.rs"]);
    let push = Facts {
        member_gate: false,
        ..facts()
    };
    let skipped = plan_gates(&always, &change, &push);
    assert!(!ids(&skipped).contains(&"conformance-tests".to_string()), "{skipped:?}");
    assert!(
        skipped
            .iter()
            .any(|p| matches!(p, Planned::Skip { id, .. } if id == "conformance-tests")),
        "the row says why it did not run: {skipped:?}"
    );
    // The conformance row itself still runs on every push (ER-41).
    assert!(ids(&skipped).contains(&"conformance".to_string()));
    // Positive controls: a change the planner says reaches conformance runs them; the member gate runs them whatever changed; a plan
    // without `why` (not made with explain) runs them too (fail closed).
    let reached = json!({"conformance": true, "apps": [], "packages": {},
        "why": {"conformance": ["conformance/src/checks/C79-orphan-guard-hooks.mjs", crate::plan::CONFORMANCE_ALWAYS]}});
    assert!(ids(&plan_gates(&reached, &change, &push)).contains(&"conformance-tests".to_string()));
    assert!(
        ids(&plan_gates(&always, &change, &facts())).contains(&"conformance-tests".to_string()),
        "the member gate"
    );
    assert!(ids(&plan_gates(&plan(), &change, &push)).contains(&"conformance-tests".to_string()));
}

/// The member gate runs the full nextest `ci` profile of every Cargo workspace the member touches, on this host and in the Linux CI
/// container, and the orphan guardian's acting tests as uid 65534 when the guardian changed (user, 2026-11-25: six members passed their
/// own checks and then failed on another OS in CI; the report-only other-OS row never ran). Windows stays CI's.
mod member_tests {
    use super::*;
    use crate::pre_push::gates::inactive_reason;

    const FILES: &[&str] = &["crates/paddock-proc/src/lib.rs", "tools/paddock-dev/src/a.rs"];

    fn plan2() -> serde_json::Value {
        json!({
            "conformance": true,
            "apps": ["dyno"],
            "packages": {
                "crates": ["paddock-proc", "paddock-chromium"],
                "tools/paddock-dev": ["paddock-dev"],
                "apps/dyno": ["dyno-app", "dyno-core"],
            }
        })
    }

    fn member() -> Facts {
        let mut f = facts();
        f.test_filters
            .insert("crates".into(), "package(=paddock-proc) | package(=paddock-chromium)".into());
        f
    }

    #[test]
    fn a_developers_push_and_a_docs_only_change_run_no_tests_and_say_why() {
        let dev = Facts {
            member_gate: false,
            ..member()
        };
        let planned = plan_gates(&plan2(), &change(FILES), &dev);
        assert!(!ids(&planned).iter().any(|i| i.starts_with("tests")), "{:?}", ids(&planned));
        assert!(
            planned
                .iter()
                .any(|p| matches!(p, Planned::Skip { id, reason } if id == "tests" && reason.contains("--member-gate"))),
            "{planned:?}"
        );
        let planned = plan_gates(&plan2(), &change(&["crates/paddock-proc/README.md"]), &member());
        assert!(!ids(&planned).iter().any(|i| i.starts_with("tests")), "{:?}", ids(&planned));
        assert!(planned.iter().any(|p| matches!(p, Planned::Skip { id, .. } if id == "tests")));
    }

    /// The train's split (ER-53): the fast set is every row but the host tests, and holds the rows the plan names (fmt, lockfiles,
    /// commit-lint, gates-register, conformance, Clippy, conformance-tests, member-audits); the slow set is the host tests alone; the two
    /// together are the plan.
    #[test]
    fn the_fast_and_slow_sets_split_the_member_gate_at_the_host_tests() {
        use crate::pre_push::train_split::{RowSet, select};
        let planned = plan_gates(&plan2(), &change(FILES), &member());
        let all = ids_of_all(&planned);
        let fast = ids_of_all(&select(planned.clone(), RowSet::Fast));
        let slow = ids_of_all(&select(planned, RowSet::Slow));
        assert!(
            !slow.is_empty() && slow.iter().all(|i| i == "tests" || i.starts_with("tests ")),
            "{slow:?}"
        );
        assert!(fast.iter().all(|i| i != "tests" && !i.starts_with("tests ")), "{fast:?}");
        for want in [
            "fmt ",
            "lockfile ",
            "clippy ",
            "clippy-windows ",
            "commit-lint",
            "gates-register",
            "conformance",
            "conformance-tests",
            "member-audits",
        ] {
            assert!(fast.iter().any(|i| i.starts_with(want)), "{want} is a fast row: {fast:?}");
        }
        assert_eq!(fast.len() + slow.len(), all.len(), "nothing is dropped between the two sets");
    }

    fn ids_of_all(planned: &[Planned]) -> Vec<String> {
        planned
            .iter()
            .map(|p| match p {
                Planned::Run(g) => g.id.clone(),
                Planned::Skip { id, .. } | Planned::NotMeasured { id, .. } | Planned::Broken { id, .. } => id.clone(),
            })
            .collect()
    }

    #[test]
    fn an_inactive_apps_tests_are_skipped_and_named_as_inactive() {
        let off = Facts {
            inactive: Ok(BTreeSet::from(["dyno".to_string()])),
            ..member()
        };
        let planned = plan_gates(
            &plan2(),
            &change(&["apps/dyno/crates/dyno-core/src/a.rs", "crates/paddock-proc/src/lib.rs"]),
            &off,
        );
        assert!(
            !ids(&planned).iter().any(|i| i.starts_with("tests") && i.contains("apps/dyno")),
            "{:?}",
            ids(&planned)
        );
        assert!(
            planned
                .iter()
                .any(|p| matches!(p, Planned::Skip { id, reason } if id == "tests apps/dyno" && *reason == inactive_reason("dyno"))),
            "{planned:?}"
        );
        assert!(ids(&planned).contains(&"tests crates".to_string()));
    }

    #[test]
    fn a_failing_test_fails_the_gate_and_a_missing_nextest_is_not_measured() {
        let planned = plan_gates(&plan2(), &change(FILES), &member());
        let rows = run_all(
            &planned,
            &Fake {
                fail: vec!["tests crates".into()],
                ..Fake::default()
            },
            2,
        );
        let report = render("h", &rows);
        assert_eq!(report.exit, Exit::Findings, "{}", report.text);
        assert!(report.text.contains("::error::tests crates"), "{}", report.text);
        let rows = run_all(
            &planned,
            &Fake {
                no_tools: vec!["cargo-nextest".into()],
                ..Fake::default()
            },
            2,
        );
        assert!(nm(&verdicts(&rows)["tests crates"]).is_some());
        // The positive control: with every row green it passes, so the verdicts above come from the row.
        let rows = run_all(&planned, &Fake::default(), 2);
        assert!(passed(&verdicts(&rows)["tests crates"]));
    }

    /// No VM or container starts on this Mac (user, 2026-11-26; docs/rules.md rule 101): no planned row is a Linux row, and
    /// no row's command is a container or VM tool, for a change that reaches every kind of row, in a developer's push and the member
    /// gate. The Linux checks are CI's Linux jobs; the register names them CI-owned.
    #[test]
    fn no_row_starts_a_container_or_a_vm_and_the_linux_rows_are_ci_owned() {
        const FORBIDDEN: &[&str] = &["docker", "podman", "nerdctl", "orb", "orbctl", "vmrun", "linux-container.sh"];
        let files = [
            "crates/paddock-proc/src/guardian/a.rs",
            "tools/paddock-dev/src/a.rs",
            "apps/dyno/crates/dyno-core/src/a.rs",
        ];
        for member_gate in [false, true] {
            let f = Facts { member_gate, ..member() };
            let planned = plan_gates(&plan2(), &change(&files), &f);
            for g in run_gates(&planned) {
                assert!(
                    !g.id.contains("linux"),
                    "{}: a Linux row runs on this Mac (member_gate {member_gate})",
                    g.id
                );
                for word in std::iter::once(&g.program).chain(g.args.iter()) {
                    let base = word.rsplit('/').next().unwrap_or(word);
                    assert!(!FORBIDDEN.contains(&base), "{}: starts {base} (member_gate {member_gate})", g.id);
                }
            }
            assert!(
                planned
                    .iter()
                    .all(|p| !matches!(p, Planned::NotMeasured { id, .. } | Planned::Broken { id, .. } if id.contains("linux"))),
                "no Linux row is left behind as not measured"
            );
        }
        assert!(
            crate::pre_push::gates::NOT_RUN
                .iter()
                .any(|(what, why)| what.contains("Linux") && why.contains("CI-owned")),
            "the register names the Linux rows CI-owned"
        );
    }

    fn by<'a>(planned: &'a [Planned], prefix: &str) -> &'a Gate {
        run_gates(planned)
            .into_iter()
            .find(|g| g.id.starts_with(prefix))
            .unwrap_or_else(|| panic!("no row {prefix}: {:?}", ids(planned)))
    }

    #[test]
    fn a_touched_workspace_gets_its_ci_tests_on_the_host_with_heavys_thread_share() {
        let planned = plan_gates(&plan2(), &change(FILES), &member());
        let host = by(&planned, "tests crates");
        assert_eq!(
            host.cwd, "",
            "the test-cache command runs from the root and enters the workspace itself"
        );
        assert_ne!(
            host.program, "cargo",
            "the row is this program's `ci test-cache run`, never a bare nextest (ADR 0116)"
        );
        assert_eq!(
            host.args.join(" "),
            "ci test-cache run --workspace crates --build-dir /t --filter package(=paddock-proc) | package(=paddock-chromium)",
            "the plan's reached packages as CI's member run filters them; the command adds CI's profile, every feature in crates, the exclusion of reused packages and the record of passes"
        );
        assert!(host.heavy, "it compiles: through heavy (ER-24)");
        assert_eq!(
            host.need,
            Need::Tool("cargo-nextest".into()),
            "no nextest is not measured, never a pass"
        );
        assert!(host.env.contains(&("RUSTUP_AUTO_INSTALL".to_string(), "0".to_string())), "ER-45");
        assert!(
            host.env.iter().all(|(k, _)| k != "NEXTEST_TEST_THREADS"),
            "the thread count is heavy's share of the machine (step_env), never a fixed number here; the caller's env wins (ER-24, ER-47, ER-48)"
        );
        assert!(
            host.env.iter().all(|(k, _)| !k.starts_with("CARGO_")),
            "sccache hashes CARGO_* variables"
        );
        let tool = by(&planned, "tests tools/paddock-dev");
        assert!(
            !tool.args.contains(&"--filter".to_string()) && tool.args.starts_with(&["ci".to_string(), "test-cache".to_string()]),
            "{:?}",
            tool.args
        );
    }
}
