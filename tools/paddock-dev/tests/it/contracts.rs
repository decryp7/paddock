//! The repository contracts the retired `scripts/affected.test.mjs` also held, moved so the script can be retired (ADR 0022 R4): the
//! apps' CI contracts (`paddock.ci.json`) and the host-cleanup process patterns they hand to `job-isolation.sh`, the markdown
//! that code reads (declared doc inputs), CODEOWNERS, Renovate and the one npm workspace.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::plan::{is_declared_doc_input, is_docs};
use paddock_testkit::TestCommand;
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    crate::full_tree(Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap())
}

fn tracked() -> Vec<String> {
    let out = paddock_proc::command("git")
        .current_dir(root())
        .args(["ls-files"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).lines().map(String::from).collect()
}

fn read(path: &str) -> String {
    // A Windows checkout may turn line ends into CRLF; the contracts are about the text.
    std::fs::read_to_string(root().join(path)).unwrap().replace("\r\n", "\n")
}

fn json(path: &str) -> Value {
    serde_json::from_str(&read(path)).unwrap()
}

/// Every app's `paddock.ci.json` (with its `dir`, as the planner reads it), straight from the tracked files: no `cargo metadata`,
/// which on a loaded host runner took most of the 60 s budget by itself.
fn configs() -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for f in tracked() {
        let Some(app) = f.strip_prefix("apps/").and_then(|r| r.strip_suffix("/paddock.ci.json")) else {
            continue;
        };
        if app.contains('/') {
            continue;
        }
        let mut config = json(&f);
        config["dir"] = Value::String(format!("apps/{app}"));
        out.push((app.to_string(), config));
    }
    out
}

#[test]
fn every_folder_under_apps_has_a_ci_contract_whose_cargo_workspace_and_commands_exist() {
    let tracked = tracked();
    let have: BTreeSet<String> = configs().into_iter().map(|(a, _)| a).collect();
    let cfg = configs();
    for entry in std::fs::read_dir(root().join("apps"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_dir())
    {
        let name = entry.file_name().to_string_lossy().to_string();
        if !tracked.iter().any(|f| f.starts_with(&format!("apps/{name}/"))) {
            continue;
        }
        assert!(
            have.contains(&name),
            "apps/{name}/paddock.ci.json is missing (the CI contract of its matrix leg)"
        );
        let app = &cfg.iter().find(|(a, _)| *a == name).unwrap().1;
        if let Some(c) = app["cargo"].as_str().filter(|c| !c.is_empty()) {
            assert!(
                root().join("apps").join(&name).join(c).join("Cargo.toml").exists(),
                "{name}: cargo folder {c} has no Cargo.toml"
            );
        }
        if let Some(n) = app["nextestConfig"].as_str() {
            assert!(root().join(n).exists(), "{name}: {n} is missing");
        }
        for k in ["prepare", "checks", "ui"] {
            assert!(app.get(k).is_none_or(Value::is_array), "{name}: {k} is a list of commands");
        }
    }
}

#[test]
fn a_legs_nextest_config_names_only_packages_of_its_cargo_workspace_and_its_release_workflow_runs_the_same_one() {
    let tracked = tracked();
    let package = Regex::new(r"package\(([^)]+)\)").unwrap();
    let member = Regex::new(r#"(?m)^name = "([^"]+)"$"#).unwrap();
    let used = Regex::new(r"--config-file (\S+nextest[^\s]*\.toml)").unwrap();
    for (name, app) in configs() {
        let (Some(config), Some(cargo)) = (app["nextestConfig"].as_str(), app["cargo"].as_str()) else {
            continue;
        };
        if cargo.is_empty() || !tracked.iter().any(|f| f.starts_with(&format!("apps/{name}/"))) {
            continue;
        }
        let lock = read(&format!("apps/{name}/{cargo}/Cargo.lock"));
        let members: BTreeSet<String> = member.captures_iter(&lock).map(|c| c[1].to_string()).collect();
        for c in package.captures_iter(&read(config)) {
            assert!(
                members.contains(&c[1]),
                "{name}: {config} names package {}, which its Cargo workspace does not have",
                &c[1]
            );
        }
        let release = format!(".gitea/workflows/{name}-release.yml");
        if root().join(&release).exists() {
            for c in used.captures_iter(&read(&release)) {
                let file = c[1].trim_start_matches("../");
                assert_eq!(file, config, "{name}-release.yml runs a different nextest config than its CI leg");
            }
        }
    }
}

/// `job-isolation.sh pattern <pattern>` the way the host-cleanup action runs it: bash to bash, the pattern in an environment variable
/// (the action's `PATTERNS`), never as an argument of a native process, which Windows' command line parsing rewrites (`\\` became `\`
/// in run 7724's log, so a `[^/\\]` class reached the script as `[^/\]`).
fn pattern_command(pattern: &str, env: &[(&str, String)]) -> std::process::Output {
    let mut cmd = TestCommand::new("bash");
    cmd.arg("-c")
        .arg(r#"exec bash "$0" pattern "$PADDOCK_TEST_PATTERN""#)
        .arg(root().join("conformance/ci/job-isolation.sh"))
        .env("PATH", "/usr/bin:/bin")
        .env("PADDOCK_TEST_PATTERN", pattern);
    if let Ok(v) = std::env::var("SYSTEMROOT") {
        cmd.env("SYSTEMROOT", v);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

const JOB_TMP: &str = "/Users/x/gitea-runner/work/0123456789abcdef/tmp";
const OTHER_TMP: &str = "/Users/x/gitea-runner/work/fedcba9876543210/tmp";

/// A paddock.ci.json process pattern as host-cleanup runs it: `{tmp}`, `{build}` and `{slot}` filled in by the real
/// `conformance/ci/job-isolation.sh` for a sample job (slot s1, its runner folder, its build folder), as a regular expression;
/// `None` when the script refuses it (not anchored on a folder of the job).
fn scoped(pattern: &str, app: &str, os: &str) -> Option<Regex> {
    let win = if os == "Windows" { "C:" } else { "" };
    let out = pattern_command(
        pattern,
        &[
            ("RUNNER_OS", os.to_string()),
            ("PADDOCK_CI_SLOT", "s1".to_string()),
            ("PADDOCK_JOB_TEMP", format!("{win}{JOB_TMP}")),
            ("PADDOCK_BUILD_FOLDER", format!("{win}/Users/x/.paddock-ci/target-{app}-app-s1")),
        ],
    );
    if !out.status.success() {
        // Shown with a failing test: why the script refused this pattern on this host.
        eprintln!(
            "job-isolation.sh refused {pattern:?} ({os}): {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    out.status
        .success()
        .then(|| Regex::new(String::from_utf8_lossy(&out.stdout).trim()).unwrap())
}

fn scoped_all(app: &str, patterns: &[&str]) -> Vec<Option<Regex>> {
    patterns
        .iter()
        .flat_map(|p| [scoped(p, app, "macOS"), scoped(p, app, "Windows")])
        .collect()
}

fn patterns_of(app: &Value) -> Vec<&str> {
    app["processPatterns"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

#[test]
fn an_apps_host_cleanup_patterns_match_its_own_paths_and_never_the_real_runner_or_another_job() {
    let real = [
        "gitea-runner daemon",
        "/usr/local/bin/gitea-runner daemon",
        "/Users/x/runners/main/bin/gitea-runner daemon",
        "/opt/act_runner/act_runner daemon",
        "C:\\runner\\act_runner.exe daemon",
        "/Users/x/actions-runner/Runner.Listener run",
    ];
    let ours: Vec<(&str, Vec<String>)> = vec![
        (
            "dyno",
            vec![
                "/Users/x/.paddock-ci/target-dyno-app-s1/debug/dyno-test-plugin echo".into(),
                "C:\\Users\\x\\.paddock-ci\\target-dyno-app-s1\\debug\\dyno-test-plugin.exe".into(),
                format!("{JOB_TMP}/dyno-it-plugins-Xk3a9Q/p1/plugin"),
            ],
        ),
        (
            "pitwall",
            vec!["/Users/x/.paddock-ci/target-pitwall-app-s1/debug/pitwall-fake-engine --port 4455".into()],
        ),
        (
            "pitcrew",
            vec![
                format!("{JOB_TMP}/pitcrew-it-stop-4242/runner/bin/gitea-runner daemon"),
                "C:\\Users\\x\\gitea-runner\\work\\0123456789abcdef\\tmp\\pitcrew-it-stop-7\\runner\\bin\\gitea-runner.exe daemon".into(),
                "/Users/x/.paddock-ci/target-pitcrew-app-s1/debug/examples/fake_runner --exit-after 5".into(),
            ],
        ),
        (
            "spotter",
            vec![
                "/Users/x/.paddock-ci/test-browser-spotter-s1/chrome-mac/Google Chrome for Testing --headless".into(),
                "C:\\Users\\x\\.paddock-ci\\test-browser-spotter-s1\\chrome-win64\\chrome.exe --headless".into(),
            ],
        ),
    ];
    let other_job: Vec<(&str, Vec<String>)> = vec![
        (
            "dyno",
            vec![
                "/Users/x/.paddock-ci/target-dyno-app-s2/debug/dyno-test-plugin echo".into(),
                format!("{OTHER_TMP}/dyno-it-plugins-Xk3a9Q/p1/plugin"),
            ],
        ),
        (
            "pitwall",
            vec!["/Users/x/.paddock-ci/target-pitwall-app-s2/debug/pitwall-fake-engine".into()],
        ),
        (
            "pitcrew",
            vec![
                format!("{OTHER_TMP}/pitcrew-it-stop-4242/runner/bin/gitea-runner daemon"),
                "/Users/x/.paddock-ci/target-pitcrew-app-s2/debug/examples/fake_runner".into(),
            ],
        ),
        (
            "spotter",
            vec![
                "/Users/x/.paddock-ci/test-browser-spotter-s2/chrome-mac/Google Chrome for Testing".into(),
                "/Users/x/.paddock-ci/test-browser-spotter-s12/chrome-mac/Google Chrome for Testing".into(),
                "C:\\Users\\x\\.paddock-ci\\test-browser-spotter-s2\\chrome.exe".into(),
            ],
        ),
    ];
    let mut checked = 0;
    let anchored = Regex::new(r"\{(tmp|build|slot)\}").unwrap();
    for (name, app) in configs() {
        let pats = patterns_of(&app);
        if pats.is_empty() {
            continue;
        }
        let mine = &ours
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("{name} has process patterns: list its own and another job's command lines here"))
            .1;
        let theirs = &other_job.iter().find(|(n, _)| *n == name).unwrap().1;
        let res = scoped_all(&name, &pats);
        assert!(
            res.iter().all(Option::is_some),
            "{name}: every pattern is anchored on a folder of the job ({{tmp}}, {{build}} or {{slot}})"
        );
        let res: Vec<Regex> = res.into_iter().flatten().collect();
        for line in mine {
            assert!(res.iter().any(|r| r.is_match(line)), "{name}: no pattern matches {line}");
        }
        for r in &res {
            for line in real.iter().map(|s| s.to_string()).chain(theirs.iter().cloned()) {
                assert!(!r.is_match(&line), "{name}: {r} would kill {line}");
            }
            checked += 1;
        }
        assert!(
            app.get("processes").is_none(),
            "{name}: no exact process names (they match every job's processes)"
        );
        for p in pats {
            assert!(anchored.is_match(p), "{name}: {p} names no folder of the job");
        }
    }
    assert!(checked >= 2);
}

#[test]
fn pitwalls_and_dynos_patterns_never_reach_an_installed_app_another_job_or_the_users_own_tools() {
    let cfg = configs();
    let pitwall = patterns_of(&cfg.iter().find(|(a, _)| a == "pitwall").unwrap().1)
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    let pitwall: Vec<&str> = pitwall.iter().map(String::as_str).collect();
    let ours = "/Users/x/.paddock-ci/target-pitwall-app-s1/debug/pitwall-fake-engine --port 4455";
    let near = [
        "/Applications/Pitwall.app/Contents/MacOS/pitwall-server --port 8080",
        "/Applications/Pitwall.app/Contents/MacOS/pitwall-fake-engine",
        "/Users/x/.paddock-ci/target-spotter-app-s1/debug/pitwall-fake-engine",
        "/Users/x/.paddock-ci/target-pitwall-bundle-s1/debug/pitwall-fake-engine",
        "/Users/x/.paddock-ci/target-pitwall-app-s2/debug/pitwall-fake-engine",
        "/Users/x/.paddock/engines/llama-server --port 8081 -m model.gguf",
        "/Users/x/dev/pitwall/target/debug/pitwall-fake-engine",
        "/opt/homebrew/bin/llama-server --port 8080",
    ];
    for r in scoped_all("pitwall", &pitwall).into_iter().flatten() {
        // Only the macOS spelling is a command line of this sample; the Windows one anchors on C:/.
        if r.is_match(ours) {
            for l in near {
                assert!(!r.is_match(l), "{r} would kill {l}");
            }
        }
    }
    assert!(
        scoped_all("pitwall", &pitwall).into_iter().flatten().any(|r| r.is_match(ours)),
        "pitwall's pattern misses its own server"
    );
    let dyno = patterns_of(&cfg.iter().find(|(a, _)| a == "dyno").unwrap().1)
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    let dyno: Vec<&str> = dyno.iter().map(String::as_str).collect();
    let near = [
        "/Users/x/Library/Application Support/Dyno/plugins/p1/plugin".to_string(),
        format!("{JOB_TMP}/other-it-plugins-Xk3a9Q/p1/plugin"),
        format!("{JOB_TMP}/dyno-it-plugins-Xk3a9Q/p1/plugin.toml"),
        "/var/folders/ab/T/dyno-it-plugins-Xk3a9Q/p1/plugin".to_string(),
        format!("{OTHER_TMP}/dyno-it-plugins-Xk3a9Q/p1/plugin"),
        "/Applications/Dyno.app/Contents/MacOS/dyno-agent run".to_string(),
        "/Applications/Dyno.app/Contents/MacOS/dyno-helper".to_string(),
        "/Users/x/.paddock-ci/target-dyno-bundle-s1/debug/dyno-test-plugin echo".to_string(),
        "/Users/x/.paddock-ci/target-spotter-app-s1/debug/dyno-test-plugin echo".to_string(),
        "/Users/x/.paddock-ci/target-dyno-app-s1/debug/dyno-agent run".to_string(),
        "/Users/x/.paddock-ci/target-dyno-app-s2/debug/dyno-bench cpu".to_string(),
        "/Users/x/.paddock-ci/target-dyno-app2/debug/dyno-bench cpu".to_string(),
        "/Users/x/dev/dyno/target/debug/dyno-test-plugin echo".to_string(),
        "C:\\Program Files\\Dyno\\dyno-agent.exe".to_string(),
    ];
    for r in scoped_all("dyno", &dyno).into_iter().flatten() {
        for l in &near {
            assert!(!r.is_match(l), "{r} would kill {l}");
        }
    }
}

#[test]
fn job_isolation_refuses_a_process_pattern_that_names_no_folder_of_the_job() {
    assert!(
        scoped(r"[/\\]pitcrew-it-[^/]+[/\\]gitea-runner", "pitcrew", "macOS").is_none(),
        "a temp prefix alone matches every job"
    );
    assert!(
        scoped("fake_runner", "pitcrew", "macOS").is_none(),
        "an exact name matches every job"
    );
    let out = pattern_command("{tmp}/x", &[("RUNNER_OS", "macOS".to_string())]);
    assert!(!out.status.success(), "no job temp folder: nothing to anchor on");
}

/// The markdown files that code under `root` reads (a quoted repo path or an `include_str!`), and the ones no DOC_INPUTS entry
/// declares. Whole-tree: `git grep` over the tracked code files, so it runs in the conformance job, not by a planner mark.
fn markdown_that_code_reads(root: &Path) -> (BTreeSet<String>, Vec<String>) {
    // include_str!("rel.md") is resolved from its file; quoted repo paths ('docs/x.md', "apps/a/README.md") are taken as is.
    let pattern = r#"include_str!\("[^"]+\.md"\)|['"](docs|apps|crates|ui)/[A-Za-z0-9_./-]+\.md['"]"#;
    let out = paddock_proc::command("git")
        .current_dir(root)
        .args(["grep", "-E", pattern, "--", "*.rs", "*.mjs", "*.ts", "*.cjs"])
        .output()
        .unwrap();
    let ls = paddock_proc::command("git").current_dir(root).args(["ls-files"]).output().unwrap();
    let tracked: BTreeSet<String> = String::from_utf8_lossy(&ls.stdout).lines().map(String::from).collect();
    let re = Regex::new(pattern).unwrap();
    let include = Regex::new(r#"include_str!\("([^"]+)"\)"#).unwrap();
    let comment = Regex::new(r"^\s*(//|\*|#)").unwrap();
    let mut read = BTreeSet::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Some((file, text)) = line.split_once(':') else { continue };
        if comment.is_match(text) || is_docs(file) {
            continue; // a comment names the file, nothing reads it; a docs tool reading docs is docs
        }
        if matches!(
            file,
            "tools/paddock-dev/tests/it/contracts.rs" | "tools/paddock-dev/tests/it/tree.rs"
        ) {
            continue; // these files name docs as test data
        }
        if file == "conformance/test/union-merge.test.mjs" {
            continue; // it asks git for the docs' merge attribute (`git check-attr`) and never reads their text
        }
        for m in re.find_iter(text) {
            let path = match include.captures(m.as_str()) {
                Some(c) => normalize(&format!("{}/{}", file.rsplit_once('/').map_or(".", |(d, _)| d), &c[1])),
                None => m.as_str()[1..m.as_str().len() - 1].to_string(),
            };
            if tracked.contains(&path) && is_docs(&path) {
                read.insert(path);
            }
        }
    }
    let undeclared = read.iter().filter(|f| !is_declared_doc_input(f)).cloned().collect();
    (read, undeclared)
}

#[test]
fn every_markdown_file_that_code_reads_is_a_declared_doc_input() {
    let (read, undeclared) = markdown_that_code_reads(&root());
    assert!(
        read.contains("apps/shell-demo/CHANGELOG.md"),
        "the grep finds the shell demo What's New"
    );
    assert!(
        undeclared.is_empty(),
        "markdown that code reads but no DOC_INPUTS entry declares: {undeclared:?}"
    );
    for f in ["docs/rules.md", "docs/blocks/first-launch.md", "docs/blocks/readme-skeleton.md"] {
        assert!(is_declared_doc_input(f), "{f}");
    }
}

/// The check goes red for a script that reads a markdown file no DOC_INPUTS entry declares (the reviewed case: scripts/new-app.mjs).
#[test]
fn a_script_reading_an_undeclared_markdown_file_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let out = paddock_proc::command("git").current_dir(dir.path()).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "-q"]);
    std::fs::create_dir_all(dir.path().join("scripts")).unwrap();
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/zz-undeclared-input.md"), "x\n").unwrap();
    std::fs::write(dir.path().join("docs/rules.md"), "x\n").unwrap();
    std::fs::write(
        dir.path().join("scripts/new-app.mjs"),
        "const a = readFileSync('docs/zz-undeclared-input.md');\nconst b = readFileSync('docs/rules.md');\n",
    )
    .unwrap();
    git(&["add", "-A"]);
    let (read, undeclared) = markdown_that_code_reads(dir.path());
    assert_eq!(read.len(), 2, "{read:?}");
    assert_eq!(
        undeclared,
        vec!["docs/zz-undeclared-input.md".to_string()],
        "the declared file passes, the other is found"
    );
}

/// Every test of this file runs in the conformance job's step (ER-41: that job is planned for every change, so the whole-tree
/// contracts do not depend on which project a change marks). The step's filter is read from the conformance contract.
#[test]
fn every_contracts_test_runs_in_the_conformance_step() {
    let contract = json("conformance/paddock.ci.json");
    let step = contract["tests"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .find(|l| l.contains("nextest") && l.contains("contracts::"))
        .expect("conformance/paddock.ci.json `tests` has a nextest step for contracts::");
    let filter = step
        .split("-E ")
        .nth(1)
        .expect("the step passes a filter with -E")
        .trim()
        .trim_matches('\'');
    // The clauses of `test(/a/) | test(/b/)`: each is one regex.
    let clauses: Vec<Regex> = filter
        .split(" | ")
        .map(|c| {
            Regex::new(
                c.strip_prefix("test(/")
                    .and_then(|f| f.strip_suffix("/)"))
                    .expect("the filter is `test(/regex/)` clauses joined by ` | `"),
            )
            .unwrap()
        })
        .collect();
    let selected = |name: &str| clauses.iter().any(|r| r.is_match(name));
    // This file's tests, listed from this binary.
    let exe = std::env::current_exe().unwrap();
    let out = paddock_proc::command(exe).args(["--list", "--format", "terse"]).output().unwrap();
    let all: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.strip_suffix(": test").map(String::from))
        .collect();
    let mine: Vec<&String> = all.iter().filter(|n| n.starts_with("contracts::")).collect();
    assert!(
        mine.len() >= 8,
        "a positive control: this file's tests were listed ({})",
        mine.len()
    );
    let missed: Vec<&&String> = mine.iter().filter(|n| !selected(n)).collect();
    assert!(missed.is_empty(), "contracts tests the conformance step does not run: {missed:?}");
    assert!(
        !all.iter().filter(|n| !n.starts_with("contracts::")).any(|n| selected(n)),
        "the filter selects no other test of this binary"
    );
    // The audit tests that read the shared fixtures or the family's allow list (whole-tree files of scripts/ and the repository):
    // found in the source of the library's own audit tests (they are not in this binary), and selected exactly.
    let source = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/audit/tests.rs")).unwrap();
    let mut reading = BTreeSet::new();
    let mut others = BTreeSet::new();
    for chunk in source.split("#[test]\n").skip(1) {
        let Some(name) = chunk.strip_prefix("fn ").and_then(|c| c.split('(').next()) else {
            continue;
        };
        let name = format!("audit::tests::{name}");
        let body = chunk.split("\n}\n").next().unwrap_or(chunk);
        if body.contains("cases(") || body.contains("fixtures(") || body.contains(r#".join("../..")"#) {
            reading.insert(name);
        } else {
            others.insert(name);
        }
    }
    assert!(
        reading.len() >= 3,
        "a positive control: the fixture-reading audit tests were found: {reading:?}"
    );
    let unselected: Vec<&String> = reading.iter().filter(|n| !selected(n)).collect();
    assert!(
        unselected.is_empty(),
        "audit tests that read whole-tree fixtures but are not in the conformance step: {unselected:?}"
    );
    let extra: Vec<&String> = others.iter().filter(|n| selected(n)).collect();
    assert!(extra.is_empty(), "the step selects audit tests that read no fixture: {extra:?}");
}

fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            s => parts.push(s), // a leading `..` stays, as in posix.normalize
        }
    }
    parts.join("/")
}

#[test]
fn codeowners_is_in_gitea_form_and_owns_every_tracked_file() {
    let text = read("CODEOWNERS");
    let unescaped_dot = Regex::new(r"(^|[^\\])\.([^*+?]|$)").unwrap();
    let mut rules = Vec::new();
    for l in text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')) {
        let mut parts = l.split_whitespace();
        let pattern = parts.next().unwrap();
        let who: Vec<&str> = parts.collect();
        assert!(!who.is_empty() && who.iter().all(|w| w.starts_with('@')), "owners in {l:?}");
        assert!(!unescaped_dot.is_match(pattern), "unescaped dot in {pattern}");
        assert!(!pattern.contains("**"), "{pattern} is a glob, not a Go regular expression");
        rules.push(Regex::new(&format!("^{pattern}$")).unwrap());
    }
    for f in tracked() {
        assert!(rules.iter().any(|r| r.is_match(&f)), "{f} has no owner");
    }
}

#[test]
fn renovate_covers_every_workspace_and_one_npm_workspace_has_one_lock() {
    let r = json("renovate.json");
    let ignore: Vec<&str> = r["ignorePaths"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(ignore.contains(&"**/node_modules/**") && ignore.contains(&"apps/*/target/**"));
    let groups: Vec<&str> = r["packageRules"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["groupName"].as_str())
        .collect();
    for g in ["Angular", "Tauri"] {
        assert!(groups.contains(&g), "group {g}");
    }
    assert_eq!(r["lockFileMaintenance"]["enabled"], true);
    assert_eq!(r["lockFileMaintenance"]["additionalBranchPrefix"], "{{packageFileDir}}-");
    let extra: Vec<String> = tracked()
        .into_iter()
        .filter(|f| f.ends_with("package-lock.json") && f != "package-lock.json" && !f.starts_with("templates/") && !f.contains("/golden/"))
        .collect();
    assert!(extra.is_empty(), "{extra:?}");
    let root = json("package.json");
    let ws: Vec<&str> = root["workspaces"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    for w in ["ui", "apps/shell-demo", "apps/*/app"] {
        assert!(ws.contains(&w), "{w}");
    }
    let names: Vec<String> = ws
        .iter()
        .filter(|w| !w.contains('*'))
        .map(|w| json(&format!("{w}/package.json"))["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        names.iter().collect::<BTreeSet<_>>().len(),
        names.len(),
        "workspace package names are unique"
    );
}

#[test]
fn the_root_agents_md_links_the_rules_and_carries_every_common_doc_block_verbatim() {
    let agents = read("AGENTS.md");
    assert!(agents.contains("[docs/rules.md](docs/rules.md)"));
    assert!(
        !agents.contains("1. **One app at a time.**"),
        "the rules are linked, not copied (ADR 0014 §9)"
    );
    let blocks = json("docs/blocks/blocks.json");
    for name in blocks["defaultTargets"]["AGENTS.md"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .filter(|n| *n != "rules")
    {
        let file = blocks["blocks"].as_array().unwrap().iter().find(|b| b["name"] == name).unwrap()["file"]
            .as_str()
            .unwrap();
        let text = read(&format!("docs/blocks/{file}"));
        assert!(agents.contains(text.trim()), "docs/blocks/{file} is in AGENTS.md as it is");
    }
}

/// The branch ledger (docs/plans/merge-queue.md): pull requests are the ledger, so opening or updating one must start no CI
/// run. No workflow listens on `pull_request` or `pull_request_target`; branches and trains run by dispatch. The `on:` keys
/// are read by C100's workflow parser, so every form (`on: [push, pull_request]`, a mapping, a list) is seen.
#[test]
fn no_workflow_runs_on_a_pull_request() {
    let mut found = Vec::new();
    for f in tracked()
        .into_iter()
        .filter(|f| f.starts_with(".gitea/workflows/") && (f.ends_with(".yml") || f.ends_with(".yaml")))
    {
        let events = paddock_dev::gates::ci::workflow_events(&f, &read(&f)).unwrap_or_else(|e| panic!("{f}: {e}"));
        if events.contains("pull_request") || events.contains("pull_request_target") {
            found.push(f);
        }
    }
    assert!(
        found.is_empty(),
        "these workflows run on pull requests, which the ledger opens for every branch: {found:?}"
    );
}

/// Pair P3 (ADR 0022 amendment 2026-11-19): the checks only the Rust host has run in the `paddock` phase of conformance's contract
/// through `./dev conformance`, and that phase is the conformance job's: a retired side takes its trigger with it (the JS runner ran them
/// on every plan), so each input a check reads, every file of its control tree, must mark the conformance job in the plan. The plan has
/// the job in every plan ("conformance always runs", user 2026-11-12); this test says so for these inputs and fails when that rule is
/// narrowed without them.
#[test]
fn the_checks_the_rust_host_runs_in_ci_are_in_the_plan_for_every_input_they_read() {
    let root = root();
    let lines = paddock_dev::ci_run::contract::phase_lines(
        &paddock_dev::ci_run::contract::load(&root, "conformance").unwrap(),
        "conformance",
        "paddock",
    )
    .unwrap();
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("./dev conformance ") && l.contains("$PADDOCK_CONFORMANCE_RUST_ARGS")),
        "the paddock phase runs the Rust host: {lines:?}"
    );
    let args = paddock_dev::ci_run::contract::conformance_rust_args_at(&root).unwrap();
    let ids: Vec<&str> = args[args.iter().position(|a| a == "--only").unwrap() + 1].split(',').collect();
    assert!(ids.len() >= 12, "the 12 checks of pair P3: {ids:?}");
    // An empty graph: no `cargo metadata` (this test runs in the conformance job), and the conformance job does not depend on it.
    let graph = paddock_dev::plan::RepoGraph {
        cargo: paddock_repo_graph::Graph::new(),
        npm: paddock_repo_graph::Graph::new(),
        apps: BTreeSet::new(),
        configs: Default::default(),
    };
    let registry = paddock_dev::check::registry();
    let mut inputs = BTreeSet::new();
    for id in &ids {
        let check = registry
            .iter()
            .find(|c| c.id() == *id)
            .unwrap_or_else(|| panic!("{id} is in `conformance.rust` and not in the Rust registry"));
        inputs.extend(check.control().files.iter().map(|(p, _)| (*p).to_string()));
    }
    assert!(inputs.len() >= 20, "the controls name the inputs the checks read: {inputs:?}");
    for path in &inputs {
        let opts = paddock_dev::plan::Options {
            explain: true,
            ..Default::default()
        };
        let plan = paddock_dev::plan::affected(std::slice::from_ref(path), &graph, &opts).unwrap();
        assert_eq!(
            plan["conformance"], true,
            "a change to {path} (read by a check of the Rust host) must run the conformance job"
        );
    }
}
