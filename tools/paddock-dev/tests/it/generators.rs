//! `docs/generators.toml` and the commands that read it: the real registry is well formed and points at things that exist, and
//! `./dev gen check` and the generator step of `./dev merge check` run real (harmless) commands in an invented repository.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::generators::registry::Registry;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(paddock_env::winpath::real_for_tools(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap())
}

#[test]
fn the_registry_of_the_repository_is_well_formed_and_names_what_exists() {
    let root = repo_root();
    let reg =
        Registry::parse(&fs::read_to_string(root.join("docs/generators.toml")).unwrap()).unwrap_or_else(|e| panic!("{}", e.join("\n")));
    assert!(reg.generators.len() > 20, "{} entries", reg.generators.len());
    for g in &reg.generators {
        // An entry that is not on every tree says so with `requires`; the others name a folder and files that exist.
        if g.requires.is_empty() {
            assert!(root.join(&g.cwd).is_dir(), "{}: cwd {} is not a folder", g.id, g.cwd);
            for p in g.paths.iter().filter(|p| !p.contains(['*', '?'])) {
                assert!(root.join(p).exists(), "{}: path {p} does not exist", g.id);
            }
        }
        assert!(g.regenerate.len() > 8, "{}: no regenerate command", g.id);
    }
}

struct Repo {
    dir: tempfile::TempDir,
    /// The build-folder root (`PADDOCK_DEV_TARGET_DIR`): outside the repository, as the tool requires.
    builds: tempfile::TempDir,
    /// The tool's `HOME`: the build-folder guard refuses a root under `$HOME`, and a runner's temp folder can be under the real
    /// one (the macOS runner's is), so the tool under test gets a home of its own (ER-14).
    home: tempfile::TempDir,
}

impl Repo {
    fn new(registry: &str) -> Repo {
        let r = Repo {
            dir: tempfile::tempdir().unwrap(),
            builds: tempfile::tempdir().unwrap(),
            home: tempfile::tempdir().unwrap(),
        };
        r.git(&["init", "-q", "-b", "main"]);
        r.write("docs/generators.toml", registry);
        r.write("README.md", "x\n");
        r.commit("base");
        r
    }

    // Starts git and the tool under test.
    #[allow(clippy::disallowed_methods)]
    fn cmd(&self, program: &str) -> Command {
        let mut c = Command::new(program);
        let empty = if cfg!(windows) { "NUL" } else { "/dev/null" };
        c.current_dir(self.dir.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", empty)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid");
        c
    }

    fn git(&self, args: &[&str]) -> String {
        let out = self.cmd("git").arg("-c").arg("commit.gpgsign=false").args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn write(&self, file: &str, text: &str) {
        let p = self.dir.path().join(file);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "--all"]);
        self.git(&["commit", "-q", "-m", message]);
    }

    /// A merge of a branch that changed `file` (and a main that changed `other`), left checked out.
    fn merge_touching(&self, file: &str) {
        self.git(&["checkout", "-q", "-b", "side"]);
        self.write(file, "side\n");
        self.commit("side");
        self.git(&["checkout", "-q", "main"]);
        self.write("main-only.txt", "m\n");
        self.commit("main");
        self.git(&["merge", "-q", "--no-ff", "-m", "merge side", "side"]);
    }

    fn dev(&self, args: &[&str]) -> (i32, String) {
        let out = self
            .cmd(env!("CARGO_BIN_EXE_paddock-dev"))
            .env("PADDOCK_DEV_TARGET_DIR", self.builds.path())
            .env("HOME", self.home.path())
            .args(args)
            .arg("--root")
            .arg(self.dir.path())
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        (out.status.code().unwrap(), text)
    }
}

/// `git --version` passes, `git no-such-subcommand` fails, and a program that is not installed cannot be started.
fn registry(check: &str, requires: &str) -> String {
    format!(
        "version = 1\n[[generator]]\nid = \"gen\"\nwhat = \"a file\"\ncheck = [{check}]\nregenerate = \"make the file again\"\npaths = [\"gen/**\"]\ncost = \"fast\"\nrequires = [{requires}]\n"
    )
}

const PASSES: &str = "\"git\", \"--version\"";
const FAILS: &str = "\"git\", \"no-such-subcommand\"";
const MISSING: &str = "\"paddock-no-such-tool-xyz\", \"--check\"";

#[test]
fn gen_check_runs_what_the_files_name_and_reports_drift_with_the_regenerate_command() {
    let ok = Repo::new(&registry(PASSES, ""));
    let (code, out) = ok.dev(&["gen", "check", "--files", "gen/a.txt"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("gen gen: current"), "{out}");

    let stale = Repo::new(&registry(FAILS, ""));
    let (code, out) = stale.dev(&["gen", "check", "--files", "gen/a.txt"]);
    assert_eq!(code, 5, "{out}");
    assert!(out.contains("::error::gen gen: STALE"), "{out}");
    assert!(out.contains("run `make the file again`"), "{out}");

    let (code, out) = stale.dev(&["gen", "check", "--files", "README.md"]);
    assert_eq!(code, 0, "an untouched generator does not run: {out}");
    assert!(out.contains("no generator is due"), "{out}");
}

#[test]
fn gen_check_cannot_pass_when_it_cannot_look() {
    let missing = Repo::new(&registry(MISSING, ""));
    let (code, out) = missing.dev(&["gen", "check", "--all"]);
    assert_eq!(code, 6, "{out}");
    assert!(out.contains("::error::gen gen: NOT MEASURED"), "{out}");

    let elsewhere = Repo::new(&registry(PASSES, "\"scripts/not-on-this-tree.mjs\""));
    let (code, out) = elsewhere.dev(&["gen", "check", "--all"]);
    assert_eq!(code, 6, "{out}");
    assert!(out.contains("not on this tree: scripts/not-on-this-tree.mjs is missing"), "{out}");

    let no_registry = Repo::new("version = 1\n");
    let (code, out) = no_registry.dev(&["gen", "check", "--all"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("no [[generator]] entry"), "{out}");
}

#[test]
fn merge_check_runs_the_generators_a_merge_makes_due_and_only_those() {
    let stale = Repo::new(&registry(FAILS, ""));
    stale.merge_touching("gen/input.txt");
    let (code, out) = stale.dev(&["merge", "check", "HEAD"]);
    assert_eq!(code, 5, "{out}");
    assert!(out.contains("::error::gen gen: STALE"), "{out}");

    let untouched = Repo::new(&registry(FAILS, ""));
    untouched.merge_touching("other/input.txt");
    let (code, out) = untouched.dev(&["merge", "check", "HEAD"]);
    assert_eq!(code, 0, "a merge that touches no registered path runs no generator: {out}");

    let (code, out) = stale.dev(&["merge", "check", "HEAD", "--no-generators"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("generator checks not run (--no-generators)"), "{out}");
    let (code, out) = stale.dev(&["--json", "merge", "check", "HEAD", "--no-generators"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("\"generators\":{\"outcome\":\"not run\""), "{out}");
    let (code, out) = stale.dev(&["merge", "check", "--branch", "side", "--target", "main"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("generator checks not run (a branch is checked before it is merged"),
        "{out}"
    );
}

#[test]
fn merge_check_is_not_measured_when_the_checkout_is_not_the_merge_result() {
    let r = Repo::new(&registry(PASSES, ""));
    r.merge_touching("gen/input.txt");
    let merge = r.git(&["rev-parse", "HEAD"]);
    r.git(&["checkout", "-q", "--detach", "HEAD^"]);
    let (code, out) = r.dev(&["merge", "check", &merge]);
    assert_eq!(code, 6, "{out}");
    assert!(out.contains("the checkout is not the merge result"), "{out}");
}

#[test]
fn merge_check_without_a_registry_runs_as_before() {
    let r = Repo::new(&registry(FAILS, ""));
    fs::remove_file(r.dir.path().join("docs/generators.toml")).unwrap();
    r.commit("drop the registry");
    r.merge_touching("gen/input.txt");
    let (code, out) = r.dev(&["merge", "check", "HEAD"]);
    assert_eq!(code, 0, "{out}");
}

// Starts git, never a process by pid.
#[allow(clippy::disallowed_methods)]
fn git_files(root: &Path) -> Vec<String> {
    let out = Command::new("git").arg("-C").arg(root).args(["ls-files"]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect()
}

fn flag_value<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    argv.iter()
        .position(|a| a == flag)
        .and_then(|i| argv.get(i + 1))
        .map(String::as_str)
}

/// A cargo check that names a package or a test target that is not there fails the same way a build error does, which is not
/// drift: every name in the registry must exist (the real run is `./dev gen check --all`, every night).
#[test]
fn every_cargo_test_target_and_package_the_registry_names_exists() {
    let root = repo_root();
    let reg = Registry::parse(&fs::read_to_string(root.join("docs/generators.toml")).unwrap()).unwrap();
    let files = git_files(&root);
    let manifests: Vec<String> = files
        .iter()
        .filter(|f| f.ends_with("/Cargo.toml") || *f == "Cargo.toml")
        .map(|f| fs::read_to_string(root.join(f)).unwrap_or_default())
        .collect();
    let mut checked = 0;
    for g in reg
        .generators
        .iter()
        .filter(|g| g.check.first().map(String::as_str) == Some("cargo"))
    {
        let argv = &g.check;
        for flag in ["-p", "--package"] {
            if let Some(pkg) = flag_value(argv, flag) {
                assert!(
                    manifests.iter().any(|m| m.contains(&format!("name = \"{pkg}\""))),
                    "{}: no package {pkg}",
                    g.id
                );
                checked += 1;
            }
        }
        if let Some(target) = flag_value(argv, "--test") {
            let dir = format!("{}/tests", g.cwd);
            let found = files
                .iter()
                .any(|f| f == &format!("{dir}/{target}.rs") || f == &format!("{dir}/{target}/main.rs"));
            assert!(found, "{}: no test target {target} in {dir}", g.id);
            // The filter after the target names a test module of it.
            if let Some(filter) = argv.iter().skip_while(|a| a.as_str() != target).nth(1) {
                let module = format!("{dir}/{target}/{filter}.rs");
                assert!(
                    files.contains(&module) || files.contains(&format!("{dir}/{target}.rs")),
                    "{}: no module {filter}",
                    g.id
                );
            }
            checked += 1;
        }
    }
    assert!(checked >= 8, "{checked} names checked");
}

/// The tools' lockfiles go stale when a shared crate they path-depend on changes its dependencies: the entry's paths must name
/// each such crate's manifest, taken from `cargo metadata` and not typed by hand.
#[test]
#[allow(clippy::disallowed_methods)] // starts cargo and rustc, never a process by pid
fn a_tool_lockfile_entry_marks_the_manifest_of_every_shared_crate_the_tool_depends_on() {
    let root = repo_root();
    let reg = Registry::parse(&fs::read_to_string(root.join("docs/generators.toml")).unwrap()).unwrap();
    let rustc = Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into()))
        .arg("-vV")
        .output()
        .unwrap();
    let host = String::from_utf8_lossy(&rustc.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("host: ").map(str::to_string))
        .unwrap();
    for tool in ["tools/paddock-dev", "tools/heavy", "tools/wiki"] {
        let out = Command::new(env!("CARGO"))
            .args([
                "metadata",
                "--format-version",
                "1",
                "--locked",
                "--offline",
                "--filter-platform",
                &host,
            ])
            .current_dir(root.join(tool))
            .output()
            .unwrap();
        assert!(out.status.success(), "{tool}: {}", String::from_utf8_lossy(&out.stderr));
        let meta: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let entry = reg
            .get(&format!("cargo-lock-{}", tool.replace('/', "-")))
            .unwrap_or_else(|| panic!("no entry for {tool}"));
        let mut shared = 0;
        for p in meta["packages"].as_array().unwrap() {
            // cargo spells a Windows path with `\`, the root is spelled for tools (`/`): compare with one separator.
            let manifest = p["manifest_path"].as_str().unwrap().replace('\\', "/");
            let base = root.to_str().unwrap().replace('\\', "/");
            let Some(rel) = manifest.strip_prefix(base.as_str()).map(|r| r.trim_start_matches('/')) else {
                continue;
            };
            if p["source"].is_null() && rel.starts_with("crates/") {
                assert!(entry.marks(rel), "{}: its paths do not mark {rel}", entry.id);
                shared += 1;
            }
        }
        assert!(shared > 0, "{tool} depends on no shared crate?");
    }
}
