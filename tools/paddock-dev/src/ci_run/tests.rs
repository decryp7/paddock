//! The executor against the script it replaces (`conformance/ci/run-commands.sh`; its cases, from conformance/test/ci-actions.test.mjs,
//! are the oracle) and the contract rules. Fake commands only, in a private folder (ER-14).

use super::*;
use std::sync::atomic::AtomicBool;

struct Repo {
    dir: tempfile::TempDir,
}

impl Repo {
    fn new(contract_for: &str, json: &str) -> Repo {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        let path = contract::path_of(dir.path(), contract_for);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, json).unwrap();
        Repo { dir }
    }
    fn root(&self) -> &Path {
        self.dir.path()
    }
    fn run(&self, project: &str, phase: &str, extra: impl FnOnce(&mut RunArgs), env: &[(&str, &str)]) -> (i32, String) {
        let mut args = RunArgs {
            project: project.into(),
            phase: phase.into(),
            keep_going: false,
            list: false,
            max_secs: 60,
            heavy: false,
            root: Some(self.root().to_path_buf()),
        };
        extra(&mut args);
        let mut e: HashMap<String, String> = ["PATH", "HOME", "TMPDIR", "SystemRoot", "ComSpec"]
            .iter()
            .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
            .collect();
        e.extend(env.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        let mut out = Vec::new();
        let code = run_with(&args, &e, &Arc::new(AtomicBool::new(false)), &mut out, &mut Vec::new());
        (code, String::from_utf8(out).unwrap())
    }
}

fn json(lines: &[&str]) -> String {
    serde_json::json!({"checks": lines}).to_string()
}

// ---- the cases of run-commands.sh's own test ----

#[test]
fn the_failing_line_names_itself_stops_there_and_a_cd_holds_for_the_lines_after_it() {
    let r = Repo::new("ui", &json(&["cd sub", "pwd > here", "printf x", "exit 7", "touch never"]));
    let (code, log) = r.run("ui", "checks", |_| {}, &[]);
    assert_eq!(code, 7, "the failing command's exit code: {log}");
    assert!(
        log.contains("::error::command failed (exit 7): exit 7 (in sub)"),
        "it names itself: {log}"
    );
    assert!(r.root().join("sub/here").exists(), "a cd holds for the lines after it");
    assert!(!r.root().join("sub/never").exists(), "the lines after a failure do not run");
    assert!(log.contains("Next step:"), "ER-27: the next step is on the error line: {log}");
}

#[test]
fn shell_expansion_pipes_and_blank_lines_work_as_in_the_old_bash_c() {
    let r = Repo::new(
        "ui",
        &json(&[
            "echo one > o",
            "E2E_INDEX=2 bash -c 'echo $((4359 + E2E_INDEX))' > n",
            "",
            "  ",
            "echo a | tr a b > p",
        ]),
    );
    let (code, log) = r.run("ui", "checks", |_| {}, &[]);
    assert_eq!(code, 0, "{log}");
    assert_eq!(std::fs::read_to_string(r.root().join("n")).unwrap().trim(), "4361");
    assert_eq!(std::fs::read_to_string(r.root().join("p")).unwrap().trim(), "b");
}

#[test]
fn a_cd_to_a_missing_folder_fails() {
    let r = Repo::new("ui", &json(&["cd nowhere"]));
    let (code, log) = r.run("ui", "checks", |_| {}, &[]);
    assert_eq!(code, 1);
    assert!(log.contains("command failed (exit 1): cd nowhere"), "{log}");
}

#[test]
fn every_line_runs_with_errexit_nounset_and_pipefail_like_the_script() {
    // set -e inside the line: the first failing command of `a; b` ends the line.
    let r = Repo::new("ui", &json(&["false; touch after-false", "touch ok"]));
    let (code, _) = r.run("ui", "checks", |_| {}, &[]);
    assert_ne!(code, 0);
    assert!(!r.root().join("after-false").exists(), "set -e");
    // set -u: an unset variable is an error
    let r = Repo::new("ui", &json(&["echo $NOT_SET_ANYWHERE > u"]));
    assert_ne!(r.run("ui", "checks", |_| {}, &[]).0, 0, "set -u");
    // pipefail: a failing first stage of a pipeline fails the line
    let r = Repo::new("ui", &json(&["false | cat > /dev/null"]));
    assert_ne!(r.run("ui", "checks", |_| {}, &[]).0, 0, "pipefail");
}

// ---- what the executor adds ----

#[test]
fn keep_going_runs_every_line_and_exits_1_with_one_error_per_failure() {
    let r = Repo::new("ui", &json(&["exit 3", "touch second", "exit 4"]));
    let (code, log) = r.run("ui", "checks", |a| a.keep_going = true, &[]);
    assert_eq!(code, 1);
    assert!(r.root().join("second").exists());
    assert_eq!(log.matches("::error::").count(), 2, "{log}");
    assert!(log.contains("3 command(s), 2 failed"), "{log}");
}

#[test]
fn every_line_is_logged_with_its_duration_and_how_it_ended() {
    let r = Repo::new("ui", &json(&["true", "exit 2"]));
    let (_, log) = r.run("ui", "checks", |_| {}, &[]);
    assert!(log.contains("::group::true") && log.contains("ran: true (exit 0, "), "{log}");
    assert!(log.contains("ran: exit 2 (exit 2, "), "{log}");
}

#[test]
fn a_line_that_overruns_is_ended_at_the_limit_with_its_whole_tree() {
    let r = Repo::new("ui", &json(&["sleep 30 & wait; touch never"]));
    let started = Instant::now();
    let (code, log) = r.run("ui", "checks", |a| a.max_secs = 1, &[]);
    assert_eq!(code, 124, "{log}");
    assert!(started.elapsed() < Duration::from_secs(20), "bounded");
    assert!(log.contains("timed out after 1 s"), "{log}");
    assert!(!r.root().join("never").exists());
}

#[test]
fn an_unknown_phase_or_project_is_an_error_never_a_pass() {
    let r = Repo::new("ui", &json(&["true"]));
    let (code, log) = r.run("ui", "typo", |_| {}, &[]);
    assert_eq!(code, 1);
    assert!(log.contains("has no phase `typo` (phases: checks)"), "{log}");
    let (code, log) = r.run("nosuch", "checks", |_| {}, &[]);
    assert_eq!(code, 1);
    assert!(log.contains("apps/nosuch/paddock.ci.json cannot be read"), "{log}");
}

#[test]
fn an_unholdable_cd_is_refused_when_the_phase_is_read() {
    for bad in ["cd $X", "cd \"a b\"", "cd a && ls", "cd ..", "cd /tmp", "cd sub/../../x"] {
        let r = Repo::new("ui", &json(&[bad]));
        let (code, log) = r.run("ui", "checks", |_| {}, &[]);
        assert_eq!(code, 1, "{bad}: {log}");
        assert!(log.contains("::error::"), "{bad}: {log}");
    }
}

#[test]
fn list_prints_the_lines_with_their_folders_and_runs_nothing() {
    let r = Repo::new("ui", &json(&["touch a", "cd sub", "touch b", "cd ../sub", "touch c"]));
    let (code, log) = r.run("ui", "checks", |a| a.list = true, &[]);
    assert_eq!(code, 0);
    assert_eq!(log, "[.] touch a\ncd sub\n[sub] touch b\ncd sub\n[sub] touch c\n");
    assert!(!r.root().join("a").exists());
}

#[test]
fn the_conformance_field_becomes_the_argument_variable_of_every_line() {
    let c = serde_json::json!({
        "conformance": {"app": "paddock", "root": ".", "only": ["C06", "C14"], "strict": ["C36"]},
        "paddock": ["echo \"$PADDOCK_CONFORMANCE_ARGS\" > args"]
    })
    .to_string();
    let r = Repo::new("conformance", &c);
    let (code, log) = r.run("conformance", "paddock", |_| {}, &[]);
    assert_eq!(code, 0, "{log}");
    assert_eq!(
        std::fs::read_to_string(r.root().join("args")).unwrap().trim(),
        "--app paddock --root . --only C06,C14 --strict C36"
    );
}

#[test]
fn the_end_to_end_index_comes_from_the_shard() {
    let r = Repo::new("ui", &json(&["echo $E2E_INDEX > i"]));
    let (code, _) = r.run("ui", "checks", |_| {}, &[("E2E_SHARD", "2/2")]);
    assert_eq!(code, 0);
    assert_eq!(std::fs::read_to_string(r.root().join("i")).unwrap().trim(), "2");
}

#[test]
fn the_end_to_end_index_is_empty_without_a_shard_and_the_download_token_is_not_passed_on() {
    let r = Repo::new("ui", &json(&["echo \"[$E2E_INDEX][${PADDOCK_DEV_TOKEN-gone}]\" > i"]));
    let (code, log) = r.run("ui", "checks", |_| {}, &[("PADDOCK_DEV_TOKEN", "secret")]);
    assert_eq!(code, 0, "set -u must not meet E2E_INDEX unset: {log}");
    assert_eq!(std::fs::read_to_string(r.root().join("i")).unwrap().trim(), "[][gone]");
}

#[test]
fn an_export_does_not_carry_to_the_next_line() {
    // Documented: every line is its own shell (as `run-commands.sh`'s `( set -e; eval line )` was).
    let r = Repo::new("ui", &json(&["export X_ONE=1", "echo \"[${X_ONE-unset}]\" > x"]));
    assert_eq!(r.run("ui", "checks", |_| {}, &[]).0, 0);
    assert_eq!(std::fs::read_to_string(r.root().join("x")).unwrap().trim(), "[unset]");
}

#[test]
fn the_job_environment_reaches_the_lines_and_nothing_is_cleared() {
    let r = Repo::new("ui", &json(&["echo \"$GITHUB_STEP_SUMMARY_FAKE-$PW_PORT\" > e"]));
    let (code, _) = r.run("ui", "checks", |_| {}, &[("GITHUB_STEP_SUMMARY_FAKE", "x"), ("PW_PORT", "4300")]);
    assert_eq!(code, 0);
    assert_eq!(std::fs::read_to_string(r.root().join("e")).unwrap().trim(), "x-4300");
}

#[test]
fn a_set_stop_flag_ends_the_phase_before_the_next_line() {
    let r = Repo::new("ui", &json(&["touch one", "touch two"]));
    let mut args = RunArgs {
        project: "ui".into(),
        phase: "checks".into(),
        keep_going: false,
        list: false,
        max_secs: 60,
        heavy: false,
        root: Some(r.root().to_path_buf()),
    };
    args.max_secs = 5;
    let env: HashMap<String, String> = std::env::vars().collect();
    let stop = Arc::new(AtomicBool::new(true));
    let mut out = Vec::new();
    let code = run_with(&args, &env, &stop, &mut out, &mut Vec::new());
    assert_eq!(code, 130);
    assert!(!r.root().join("one").exists());
}

// ---- parity with what CI ran before the executor (golden from the YAML and scripts at 11c365fc4) ----

/// The root jobs of ci.yml the golden covers. `train-gates` is not modelled (its `if` needs a dispatch with a `base` input, which the
/// model's contexts do not give), so its call of the executor is asserted as text below.
const JOBS: [&str; 7] = ["ui", "e2e", "conformance", "tools", "icons", "eslint-preset", "crates"];

/// The commands of the root jobs of ci.yml and of the app legs' contract lists, as the CI model reads them from the real tree.
fn modelled_commands() -> std::collections::BTreeSet<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let src = crate::gates::edge::load_sources(&root).unwrap();
    let model = crate::gates::ci::build(&src);
    assert!(model.problems.is_empty(), "{:?}", model.problems);
    let jobs = [
        "ui",
        "e2e",
        "conformance",
        "tools",
        "icons",
        "eslint-preset",
        "crates",
        "train-gates",
    ];
    let mut out = std::collections::BTreeSet::new();
    for l in model.lines.iter().filter(|l| l.root == ".gitea/workflows/ci.yml") {
        let row = |kind: &str, who: &str| {
            format!(
                "{kind}\t{who}\t{}\t{}\t{}",
                l.os.clone().unwrap_or_default(),
                l.cwd.clone().unwrap_or_default(),
                l.text
            )
        };
        if jobs.contains(&l.job.as_str()) {
            out.insert(row("job", &l.job));
        } else if l.job == "app" && l.origin.is_some() {
            out.insert(row("app", &l.app.clone().unwrap_or_default()));
        }
    }
    out
}

#[test]
fn the_contracts_run_exactly_the_commands_ci_ran_before_the_executor() {
    let golden = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/ci-run/commands.tsv")).unwrap();
    let want: std::collections::BTreeSet<String> = golden.lines().filter(|l| !l.starts_with('#')).map(str::to_string).collect();
    let got = modelled_commands();
    for job in JOBS {
        let rows = |set: &std::collections::BTreeSet<String>| set.iter().filter(|r| r.starts_with(&format!("job\t{job}\t"))).count();
        assert!(
            rows(&want) > 0 && rows(&got) > 0,
            "job {job} has no row in the golden or in the model: the comparison would prove nothing"
        );
    }
    let ci = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.gitea/workflows/ci.yml")).unwrap();
    let train = ci.split("\n  train-gates:\n").nth(1).unwrap().split("\n  app:\n").next().unwrap();
    assert!(
        train.contains("run: ./dev ci run conformance paddock"),
        "train-gates runs the conformance contract"
    );
    let missing: Vec<&String> = want.difference(&got).collect();
    let extra: Vec<&String> = got.difference(&want).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "CI no longer runs {missing:#?}\nand now also runs {extra:#?}"
    );
}

/// The probe can see a difference (ER-33): a contract line that was changed shows up as a missing and an extra command.
#[test]
fn the_parity_probe_sees_a_changed_contract_line() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut src = crate::gates::edge::load_sources(&root).unwrap();
    let ui = src.root_contracts.get_mut("ui").unwrap();
    *ui = ui.replace("npm run test:tokens", "npm run test:tokens -- --changed");
    let model = crate::gates::ci::build(&src);
    assert!(model.lines.iter().any(|l| l.text == "npm run test:tokens -- --changed"));
    assert!(!model.lines.iter().any(|l| l.job == "ui" && l.text == "npm run test:tokens"));
}

#[test]
fn every_phase_the_workflows_call_exists_in_its_contract() {
    // The model turns an unknown phase into a problem; the real tree has none.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let src = crate::gates::edge::load_sources(&root).unwrap();
    assert!(crate::gates::ci::build(&src).problems.is_empty());
}

#[test]
fn the_conformance_list_has_one_copy_the_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let args = contract::conformance_args_at(&root).unwrap();
    assert!(
        args.contains(&"--strict".to_string()) && args.iter().any(|a| a.starts_with("C06,")),
        "{args:?}"
    );
    let ci = std::fs::read_to_string(root.join(".gitea/workflows/ci.yml")).unwrap();
    assert!(!ci.contains("--only C06"), "ci.yml holds no copy of the list");
    assert!(!ci.contains("PADDOCK_CONFORMANCE_ARGS:"), "ci.yml sets no copy of the list");
}

#[cfg(unix)]
#[test]
fn heavy_puts_every_line_through_the_machine_wide_slot_and_refuses_without_one() {
    use std::os::unix::fs::PermissionsExt;
    let r = Repo::new("ui", &json(&["echo ran > out"]));
    let wrap = r.root().join("wrap.sh");
    std::fs::write(&wrap, "#!/bin/sh\necho slot >> slot-log\nexec \"$@\"\n").unwrap();
    std::fs::set_permissions(&wrap, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (code, log) = r.run("ui", "checks", |a| a.heavy = true, &[("PADDOCK_HEAVY", wrap.to_str().unwrap())]);
    assert_eq!(code, 0, "{log}");
    assert_eq!(
        std::fs::read_to_string(r.root().join("slot-log")).unwrap().trim(),
        "slot",
        "the line went through the slot"
    );
    assert!(r.root().join("out").exists());
    // No slot tool at all: refused with the next step, nothing runs (ER-33: not a silent pass without the slot).
    let r = Repo::new("ui", &json(&["echo ran > out"]));
    let (code, log) = r.run("ui", "checks", |a| a.heavy = true, &[("PATH", "/usr/bin:/bin")]);
    assert_eq!(code, 1, "{log}");
    assert!(
        log.contains("::error::") && log.contains("Next step:") && !r.root().join("out").exists(),
        "{log}"
    );
}
