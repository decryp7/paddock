//! Resolution of the arguments and config, and the command's modes, on invented trees in scratch folders.
#![allow(clippy::disallowed_methods)]
use super::exec::{ToolRun, Tools};
use super::*;
use paddock_testkit::{FakeRunner, Reply};
use std::time::Duration;

pub(super) struct Fake(pub FakeRunner);
impl Tools for Fake {
    fn run(&self, program: &str, args: &[&str], _cwd: &Path, _timeout: Duration) -> Result<ToolRun, String> {
        match self.0.run(program, args) {
            Reply::Ran { success, stdout, stderr } => Ok(ToolRun { success, stdout, stderr }),
            Reply::Failed(m) => Err(m),
        }
    }
}

fn args(f: impl FnOnce(&mut Args)) -> Args {
    let mut a = Args::default();
    f(&mut a);
    a
}

fn no_files(_: &Path) -> bool {
    false
}

#[test]
fn paths_resolve_lexically_and_an_absolute_path_wins() {
    assert_eq!(
        resolve_path(Path::new("/r/apps/x"), Path::new("app/../ui/./y")),
        Path::new("/r/apps/x/ui/y")
    );
    assert_eq!(resolve_path(Path::new("/r/apps/x"), Path::new("/abs/p")), Path::new("/abs/p"));
    assert_eq!(resolve_path(Path::new("/r"), Path::new(".")), Path::new("/r"));
}

#[test]
fn an_app_name_picks_its_config_and_cannot_climb_out() {
    assert_eq!(
        config_path(&args(|a| a.app = Some("debrief".into()))).unwrap(),
        Some(PathBuf::from("apps/debrief/paddock.ci.json"))
    );
    assert_eq!(config_path(&args(|_| {})).unwrap(), None);
    for bad in ["", "../x", "A", "a/b", "a b"] {
        assert!(
            matches!(config_path(&args(|a| a.app = Some(bad.into()))), Err(NoticesError::Usage(_))),
            "{bad:?}"
        );
    }
}

#[test]
fn a_config_gives_the_facts_and_flags_override_it() {
    let doc = json!({"notices": {"name": "X", "manifests": ["m/Cargo.toml"], "npm": "web", "out": ["o1"], "rustJson": ["a.json"], "about": "my.toml"}});
    let cfg = Path::new("/r/apps/x/paddock.ci.json");
    let plan = resolve(&Args::default(), Path::new("/r"), Some((cfg, &doc)), &no_files).unwrap();
    assert_eq!(plan.name, "X");
    assert_eq!(plan.base, Path::new("/r/apps/x"));
    assert_eq!(plan.npm_dir, Path::new("/r/apps/x/web"));
    assert_eq!(
        (plan.outs, plan.manifests, plan.rust_json),
        (
            vec!["/r/apps/x/o1".into()],
            vec!["/r/apps/x/m/Cargo.toml".into()],
            vec!["/r/apps/x/a.json".into()]
        )
    );
    assert_eq!(plan.about_toml, Path::new("/r/apps/x/my.toml"));
    let over = args(|a| {
        a.name = Some("Y".into());
        a.npm = Some("n".into());
        a.out = vec!["o2".into(), "o3".into()];
        a.manifests = vec!["z/Cargo.toml".into()];
        a.rust_json = vec!["b.json".into()];
        a.about = Some("/abs/about.toml".into());
    });
    let plan = resolve(&over, Path::new("/r"), Some((cfg, &doc)), &no_files).unwrap();
    assert_eq!((plan.name.as_str(), plan.npm_dir.as_path()), ("Y", Path::new("/r/apps/x/n")));
    assert_eq!(plan.outs, [PathBuf::from("/r/apps/x/o2"), PathBuf::from("/r/apps/x/o3")]);
    assert_eq!(
        (plan.manifests, plan.rust_json, plan.about_toml),
        (
            vec!["/r/apps/x/z/Cargo.toml".into()],
            vec!["/r/apps/x/b.json".into()],
            PathBuf::from("/abs/about.toml")
        )
    );
}

#[test]
fn defaults_are_the_generators_defaults() {
    let doc = json!({"notices": {"name": "X"}});
    let cfg = Path::new("/r/apps/x/paddock.ci.json");
    let plan = resolve(&Args::default(), Path::new("/r"), Some((cfg, &doc)), &no_files).unwrap();
    assert_eq!(plan.npm_dir, Path::new("/r/apps/x/app"));
    assert_eq!(plan.outs, [PathBuf::from("/r/apps/x/app/src-tauri/resources")]);
    assert_eq!(
        plan.manifests,
        [PathBuf::from("/r/apps/x/app/src-tauri/Cargo.toml")],
        "with a config the default manifest is named even if absent"
    );
    assert_eq!(plan.about_toml, Path::new("/r/templates/repo/about.toml"));
    let flagged = args(|a| a.name = Some("N".into()));
    let none = resolve(&flagged, Path::new("/r"), None, &no_files).unwrap();
    assert!(none.manifests.is_empty() && none.base == Path::new("/r"));
    let present = resolve(&flagged, Path::new("/r"), None, &|p| {
        p.ends_with("Cargo.toml") || p.ends_with("about.toml")
    })
    .unwrap();
    assert_eq!(present.manifests, [PathBuf::from("/r/app/src-tauri/Cargo.toml")]);
    assert_eq!(present.about_toml, Path::new("/r/about.toml"));
}

#[test]
fn bad_configs_name_the_file_and_the_field() {
    let cfg = Path::new("/r/apps/x/paddock.ci.json");
    let err = |doc: Value| {
        resolve(&Args::default(), Path::new("/r"), Some((cfg, &doc)), &no_files)
            .unwrap_err()
            .to_string()
    };
    assert!(err(json!({})).contains(r#"has no "notices" block"#));
    assert!(err(json!({"notices": {}})).contains("notices.name"));
    assert!(err(json!({"notices": {"name": 3}})).contains("notices.name must be a string"));
    assert!(err(json!({"notices": {"name": "X", "out": "o"}})).contains("notices.out must be a list of strings"));
    assert!(err(json!({"notices": {"name": "X", "manifests": [1]}})).contains("notices.manifests must be a list"));
    assert!(matches!(
        resolve(&Args::default(), Path::new("/r"), None, &no_files),
        Err(NoticesError::Usage(_))
    ));
}

/// A repository root with a monorepo-style lock, one production package and an app config pointing at `about.json`.
fn repo(licence: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let put = |p: &str, c: &str| {
        let f = dir.path().join(p);
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        fs::write(f, c).unwrap();
    };
    put(
        "package-lock.json",
        r#"{"packages":{"":{"workspaces":["apps/x/app"]},"apps/x/app":{"dependencies":{"left":"1"}},"node_modules/left":{"version":"1.0.0","license":"MIT"}}}"#,
    );
    put("node_modules/left/LICENSE", "MIT License\r\n\r\nInvented.\r\n");
    put("apps/x/app/package.json", "{}");
    put(
        "apps/x/paddock.ci.json",
        r#"{"notices":{"name":"Xapp","npm":"app","out":["app/res"],"rustJson":["app/about.json"]}}"#,
    );
    put(
        "apps/x/app/about.json",
        &format!(
            r#"{{"crates":[{{"package":{{"name":"serde-like","version":"1.0.0","source":"r"}},"license":"{licence}"}}],"licenses":[{{"id":"MIT","text":"MIT text","used_by":[{{"crate":{{"name":"serde-like","version":"1.0.0"}}}}]}}]}}"#
        ),
    );
    dir
}

fn run(dir: &Path, f: impl FnOnce(&mut Args)) -> Result<Report, CliError> {
    command(&args(|a| {
        a.root = Some(dir.to_path_buf());
        a.app = Some("x".into());
        f(a);
    }))
}

#[test]
fn write_then_check_passes_a_crlf_checkout_compares_equal_and_a_new_dependency_makes_it_stale() {
    let dir = repo("MIT");
    let file = dir.path().join("apps/x/app/res").join(FILE_NAME);
    let missing = run(dir.path(), |a| a.check = true).unwrap();
    assert_eq!(missing.exit, Exit::Findings);
    assert!(missing.text.contains("is missing: run `./dev notices`"), "{}", missing.text);
    let wrote = run(dir.path(), |_| {}).unwrap();
    assert_eq!(wrote.exit, Exit::Ok);
    assert!(wrote.text.contains("notices: 1 Rust crates, 1 npm packages"), "{}", wrote.text);
    let md = fs::read_to_string(&file).unwrap();
    assert!(md.starts_with("# Third-party notices\n\nXapp is dual-licensed") && md.contains("- left 1.0.0 — MIT [MIT-"));
    assert_eq!(run(dir.path(), |a| a.check = true).unwrap().exit, Exit::Ok);
    fs::write(&file, md.replace('\n', "\r\n")).unwrap();
    assert_eq!(
        run(dir.path(), |a| a.check = true).unwrap().exit,
        Exit::Ok,
        "a CRLF checkout compares equal"
    );
    fs::write(&file, "edited by hand\n").unwrap();
    let stale = run(dir.path(), |a| a.check = true).unwrap();
    assert_eq!(stale.exit, Exit::Findings);
    assert_eq!(stale.json()["files"][0]["state"], "stale");
    assert!(stale.text.contains("is out of date"));
    assert_eq!(fs::read_to_string(&file).unwrap(), "edited by hand\n", "check never writes");
}

#[test]
fn a_refused_licence_is_named_and_fails_both_modes_but_the_file_is_still_written() {
    let dir = repo("GPL-3.0-only");
    let wrote = run(dir.path(), |_| {}).unwrap();
    assert_eq!(wrote.exit, Exit::Findings);
    assert!(wrote.text.contains("NON-PERMISSIVE: rust serde-like@1.0.0: GPL-3.0-only"));
    assert_eq!(wrote.json()["nonPermissive"][0], "rust serde-like@1.0.0: GPL-3.0-only");
    assert!(dir.path().join("apps/x/app/res").join(FILE_NAME).is_file());
    assert_eq!(run(dir.path(), |a| a.check = true).unwrap().exit, Exit::Findings);
}

#[test]
fn problems_exit_with_the_right_kind_and_a_next_step() {
    let dir = repo("MIT");
    let err = run(dir.path(), |a| a.app = Some("nope".into())).unwrap_err();
    assert_eq!(err.exit, Exit::Failure, "a missing config file is a failed read: {err:?}");
    fs::write(dir.path().join("apps/x/paddock.ci.json"), "{}").unwrap();
    let err = run(dir.path(), |_| {}).unwrap_err();
    assert_eq!(err.exit, Exit::Usage);
    assert!(err.text().contains("notices"));
    fs::write(dir.path().join("apps/x/paddock.ci.json"), "not json").unwrap();
    assert_eq!(run(dir.path(), |_| {}).unwrap_err().exit, Exit::Usage);
    fs::write(
        dir.path().join("apps/x/paddock.ci.json"),
        r#"{"notices":{"name":"X","manifests":[]}}"#,
    )
    .unwrap();
    let err = run(dir.path(), |_| {}).unwrap_err();
    assert!(err.text().contains("no Rust manifest"), "{}", err.text());
}

#[test]
fn a_missing_lock_and_a_missing_workspace_are_failures_that_say_what_to_pass() {
    let dir = repo("MIT");
    fs::remove_file(dir.path().join("package-lock.json")).unwrap();
    let err = run(dir.path(), |_| {}).unwrap_err();
    assert_eq!(err.exit, Exit::Failure);
    assert!(
        err.text().contains("package-lock.json not found") && err.text().contains("--npm"),
        "{}",
        err.text()
    );
    fs::write(dir.path().join("package-lock.json"), r#"{"packages":{"":{}}}"#).unwrap();
    assert!(
        run(dir.path(), |_| {})
            .unwrap_err()
            .text()
            .contains("the lock has no workspace apps/x/app")
    );
}

#[test]
fn the_lock_is_not_looked_for_above_the_repository_root() {
    let outer = tempfile::tempdir().unwrap();
    fs::write(outer.path().join("package-lock.json"), "{}").unwrap();
    let inner = outer.path().join("repo/app");
    fs::create_dir_all(&inner).unwrap();
    assert_eq!(sources::lock_root(&inner, &outer.path().join("repo")), None);
    assert_eq!(sources::lock_root(&inner, outer.path()), Some(outer.path().to_path_buf()));
}

#[test]
fn cargo_about_runs_only_without_rust_json_after_the_prerequisites() {
    let dir = repo("MIT");
    let doc = fs::read_to_string(dir.path().join("apps/x/app/about.json")).unwrap();
    let plan = resolve(
        &args(|a| a.rust_json = vec![]),
        dir.path(),
        Some((
            &dir.path().join("apps/x/paddock.ci.json"),
            &json!({"notices": {"name": "X", "npm": "app", "out": ["app/res"], "manifests": ["app/Cargo.toml"]}}),
        )),
        &no_files,
    )
    .unwrap();
    let runner = FakeRunner::new();
    runner.on("cargo", &["about", "--version"]).reply(Reply::ok("cargo-about 0.9.2"));
    runner.on("cargo", &["about", "generate"]).reply(Reply::ok(&doc));
    let outcome = generate(&plan, dir.path(), false, true, &Fake(runner.clone())).unwrap();
    assert_eq!(outcome.counts.rust, 1);
    let lines = runner.lines();
    assert_eq!(lines[0], "cargo about --version");
    assert!(
        lines[1].starts_with("cargo about generate --locked -c ") && lines[1].contains("templates/repo/about.toml"),
        "{lines:?}"
    );
    assert_eq!(lines.len(), 2, "node_modules exists, so no npm ci is expected... ");
}
