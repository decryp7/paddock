//! The repository contracts `scripts/affected.test.mjs` also held, moved so the script can be retired (ADR 0022 R4): the
//! apps' CI contracts (`paddock.ci.json`) and the host-cleanup process patterns they hand to `job-isolation.sh`, the markdown
//! that code reads (declared doc inputs), CODEOWNERS, Renovate and the one npm workspace.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::plan::{is_declared_doc_input, is_docs, read_graph};
use paddock_repo_graph::GitHistory;
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the program under test (git, sh, bash); nothing here ends a process by pid"
)]
fn tracked() -> Vec<String> {
    let out = Command::new("git").current_dir(root()).args(["ls-files"]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).lines().map(String::from).collect()
}

fn read(path: &str) -> String {
    std::fs::read_to_string(root().join(path)).unwrap()
}

fn json(path: &str) -> Value {
    serde_json::from_str(&read(path)).unwrap()
}

fn configs() -> Vec<(String, Value)> {
    let g = read_graph(&root(), &GitHistory::new(root(), Duration::from_secs(120))).unwrap();
    g.apps.iter().map(|a| (a.clone(), g.config(a).clone())).collect()
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

const JOB_TMP: &str = "/Users/x/gitea-runner/work/0123456789abcdef/tmp";
const OTHER_TMP: &str = "/Users/x/gitea-runner/work/fedcba9876543210/tmp";

/// A paddock.ci.json process pattern as host-cleanup runs it: `{tmp}`, `{build}` and `{slot}` filled in by the real
/// `conformance/ci/job-isolation.sh` for a sample job (slot s1, its runner folder, its build folder), as a regular expression;
/// `None` when the script refuses it (not anchored on a folder of the job).
#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the program under test (git, sh, bash); nothing here ends a process by pid"
)]
fn scoped(pattern: &str, app: &str, os: &str) -> Option<Regex> {
    let win = if os == "Windows" { "C:" } else { "" };
    let out = Command::new("bash")
        .arg(root().join("conformance/ci/job-isolation.sh"))
        .args(["pattern", pattern])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("RUNNER_OS", os)
        .env("PADDOCK_CI_SLOT", "s1")
        .env("PADDOCK_JOB_TEMP", format!("{win}{JOB_TMP}"))
        .env("PADDOCK_BUILD_FOLDER", format!("{win}/Users/x/.paddock-ci/target-{app}-app-s1"))
        .output()
        .unwrap();
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
            vec!["/Users/x/.paddock-ci/target-pitwall-app-s1/debug/pitwall-server --die-after-ms 500 --port 4455 --fake-engine".into()],
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
            vec!["/Users/x/.paddock-ci/target-pitwall-app-s2/debug/pitwall-server --fake-engine".into()],
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
    let ours = "/Users/x/.paddock-ci/target-pitwall-app-s1/debug/pitwall-server --die-after-ms 500 --port 4455 --fake-engine";
    let near = [
        "/Applications/Pitwall.app/Contents/MacOS/pitwall-server --port 8080",
        "/Applications/Pitwall.app/Contents/MacOS/pitwall-server --fake-engine",
        "/Users/x/.paddock-ci/target-spotter-app-s1/debug/pitwall-server --fake-engine",
        "/Users/x/.paddock-ci/target-pitwall-bundle-s1/debug/pitwall-server --fake-engine",
        "/Users/x/.paddock-ci/target-pitwall-app-s2/debug/pitwall-server --fake-engine",
        "/Users/x/.paddock/engines/llama-server --port 8081 -m model.gguf",
        "/Users/x/dev/pitwall/target/debug/pitwall-server --fake-engine",
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

#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the program under test (git, sh, bash); nothing here ends a process by pid"
)]
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
    let out = Command::new("bash")
        .arg(root().join("conformance/ci/job-isolation.sh"))
        .args(["pattern", "{tmp}/x"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("RUNNER_OS", "macOS")
        .output()
        .unwrap();
    assert!(!out.status.success(), "no job temp folder: nothing to anchor on");
}

#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the program under test (git, sh, bash); nothing here ends a process by pid"
)]
#[test]
fn every_markdown_file_that_code_reads_is_a_declared_doc_input() {
    // include_str!("rel.md") is resolved from its file; quoted repo paths ('docs/x.md', "apps/a/README.md") are taken as is.
    let pattern = r#"include_str!\("[^"]+\.md"\)|['"](docs|apps|crates|ui)/[A-Za-z0-9_./-]+\.md['"]"#;
    let out = Command::new("git")
        .current_dir(root())
        .args(["grep", "-E", pattern, "--", "*.rs", "*.mjs", "*.ts", "*.cjs"])
        .output()
        .unwrap();
    let tracked: BTreeSet<String> = tracked().into_iter().collect();
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
            "scripts/affected.test.mjs" | "tools/paddock-dev/tests/it/contracts.rs" | "tools/paddock-dev/tests/it/tree.rs"
        ) {
            continue; // these files name docs as test data
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
    assert!(
        read.contains("apps/shell-demo/CHANGELOG.md"),
        "the grep finds the shell demo What's New"
    );
    let undeclared: Vec<&String> = read.iter().filter(|f| !is_declared_doc_input(f)).collect();
    assert!(
        undeclared.is_empty(),
        "markdown that code reads but no DOC_INPUTS entry declares: {undeclared:?}"
    );
    for f in ["docs/rules.md", "docs/blocks/first-launch.md", "docs/blocks/readme-skeleton.md"] {
        assert!(is_declared_doc_input(f), "{f}");
    }
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
