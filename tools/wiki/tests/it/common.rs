//! Shared helpers: a temporary wiki, lint runs, and a temporary git repository.
use paddock_wiki::lint::{self, Outcome, Request, RuleSource};
use std::path::Path;
use tempfile::TempDir;

pub const RULES: &str =
    "The coordinator watches load and free disk continuously and keeps the machine busy but not overloaded: start more agents.\n";

pub fn page(verified: &str, sources: &str, body: &str) -> String {
    format!("---\ntitle: T\nverified: {verified}\nsources: {sources}\n---\nSummary.\n\n{body}")
}

/// A wiki in a temp folder: two linked pages, an index and a log; `over` replaces or adds files (`None` removes).
pub fn wiki(over: &[(&str, Option<String>)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let mut files: Vec<(String, Option<String>)> = vec![
        (
            "one.md".into(),
            Some(page("2026-10-20", "[a.md]", "## Current state\n- One [two](two.md).\n")),
        ),
        (
            "two.md".into(),
            Some(page("2026-10-20", "[a.md]", "## Current state\n- Two [one](one.md).\n")),
        ),
        ("index.md".into(), Some("# Index\n- [One](one.md)\n- [Two](two.md)\n".into())),
        ("log.md".into(), Some("# Log\n## [2026-10-31] ingest | one | seed\n".into())),
        ("README.md".into(), Some("# Wiki\n".into())),
    ];
    for (name, text) in over {
        files.retain(|(n, _)| n != name);
        files.push((name.to_string(), text.clone()));
    }
    for (name, text) in files {
        if let Some(text) = text {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
    }
    dir
}

pub fn lint_with(dir: &Path, rules: &[RuleSource], today: &str, max_age: i64) -> Outcome {
    let cwd = std::env::current_dir().unwrap();
    lint::lint(&Request {
        wiki_dir: dir,
        cwd: &cwd,
        rules,
        today: paddock_wiki::date::parse_date(today).unwrap(),
        max_age,
        cite_by_line: &["merge-steward".to_string()],
    })
    .unwrap()
}

/// Lints a sample wiki with `over` applied, against RULES, on 2026-10-31.
pub fn run(over: &[(&str, Option<String>)]) -> Outcome {
    let dir = wiki(over);
    lint_with(
        dir.path(),
        &[RuleSource {
            name: "rules.txt".into(),
            text: RULES.into(),
        }],
        "2026-10-31",
        30,
    )
}

pub fn has(list: &[lint::Finding], text: &str) -> bool {
    list.iter().any(|f| f.to_string().contains(text))
}

pub fn s(x: &str) -> Option<String> {
    Some(x.to_string())
}

/// An empty settings file, so a test never picks up the repository's own wiki-lint.json.
pub fn empty_config() -> String {
    format!("{}/tests/golden/cases/config.json", env!("CARGO_MANIFEST_DIR"))
}

pub fn fixed_clock() -> paddock_wiki::date::Day {
    paddock_wiki::date::parse_date("2026-05-17").unwrap()
}

/// Runs `wiki` with `args` (no CI mode, the real clock; the empty settings file unless `--config` or `--repo` is given);
/// returns (exit code, stdout, stderr).
pub fn cli(args: &[&str]) -> (i32, String, String) {
    cli_in(
        args,
        &paddock_wiki::Context {
            clock: paddock_wiki::date::today_local,
            ci: false,
            log_dir: None,
        },
    )
}

pub fn cli_in(args: &[&str], ctx: &paddock_wiki::Context) -> (i32, String, String) {
    let mut args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    if !args.iter().any(|a| a == "--config" || a == "--repo") {
        args.extend(["--config".to_string(), empty_config()]);
    }
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = paddock_wiki::run_with(&args, &mut out, &mut err, ctx);
    (
        code,
        String::from_utf8_lossy(&out).into_owned(),
        String::from_utf8_lossy(&err).into_owned(),
    )
}

/// Runs git in `dir` against nothing of the user's machine.
pub fn git(dir: &Path, args: &[&str]) {
    let mut cmd = paddock_proc::command("git");
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    cmd.args([
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@example.invalid",
        "-c",
        "commit.gpgsign=false",
    ])
    .args(args)
    .current_dir(dir)
    .env("GIT_CONFIG_GLOBAL", null)
    .env("GIT_CONFIG_SYSTEM", null);
    let out = paddock_proc::run_bounded(cmd, paddock_proc::Limits::new(std::time::Duration::from_secs(30))).unwrap();
    assert!(out.success(), "git {args:?}: {}", out.stderr_text());
}

/// A temporary git repository with `docs/rules.md` committed on `main`.
pub fn git_repo(rules: &str) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/rules.md"), rules).unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "rules"]);
    dir
}
