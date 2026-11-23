//! The generator registry `docs/generators.toml`: one declared list of the files that are generated or pinned (lockfiles,
//! catalogues, notices, baselines, bindings), each with the command that checks it, the command that regenerates it, the paths
//! that mark it and what it costs. Parsed once into typed values at the edge (parse, don't validate); the checks and the merge
//! check read only these types.

use crate::gates::tree::{any_glob, glob_regex};
use regex::Regex;
use std::collections::BTreeSet;

/// Where the registry lives, relative to the repository root.
pub const PATH: &str = "docs/generators.toml";

/// What a check costs: `Fast` ones run side by side, `Heavy` ones (a build, the network, a long tool) one after another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cost {
    /// Seconds: a script over files.
    Fast,
    /// Minutes, or the network, or a build.
    Heavy,
}

impl Cost {
    /// The word in the registry.
    pub fn name(self) -> &'static str {
        match self {
            Cost::Fast => "fast",
            Cost::Heavy => "heavy",
        }
    }
}

/// One generator: a family of generated or pinned files and how to check it.
#[derive(Debug, Clone)]
pub struct Generator {
    /// Short unique name, for `--only` and for the output.
    pub id: String,
    /// What it generates, one line.
    pub what: String,
    /// The check: a program and its arguments (no shell unless it names one). Exit status 0 means current. Empty for a manual entry.
    pub check: Vec<String>,
    /// The command that compiles what `check` runs (for example `cargo test --no-run ...`), run first as a phase of its own with
    /// its own bound, so a cold build never eats the check's hang guard. Required when `check` is a cargo command that compiles
    /// (`test`, `run`, `bench`); empty otherwise.
    pub build: Vec<String>,
    /// The command that regenerates the files, as the text shown in the error (run from `cwd`).
    pub regenerate: String,
    /// The folder the commands run in, relative to the repository root.
    pub cwd: String,
    /// Paths that mark it: a change to any of them (an input or an output) makes the check due. The hand list: what the graph
    /// cannot derive (a lockfile, a data file); what `reads` names is not repeated here.
    pub paths: Vec<String>,
    /// Model node ids it reads (`script:scripts/notices.sh`, `workflow:ci.yml`): the graph derives everything these import, run or
    /// depend on, so a change to a module a script imports makes it due without a hand path (ADR 0117 §5, WP-7).
    pub reads: Vec<String>,
    /// The files it writes: what the conformance check C116 matches generated files against. May be empty (a check only).
    pub outputs: Vec<String>,
    /// Fast or heavy.
    pub cost: Cost,
    /// Files that must exist for the check to be possible on this tree; a missing one is NOT MEASURED ("not on this tree").
    pub requires: Vec<String>,
    /// Text a failing check prints when the files really are stale (any one of them, searched in all its output). When the entry
    /// names some, a failure without one is NOT MEASURED: a build error or a crash is not drift. Empty: any plain failure is drift.
    pub drift_marker: Vec<String>,
    /// Why there is no automatic check (an entry with `manual` has no `check` and never runs; it is listed when its paths change).
    pub manual: Option<String>,
    path_res: Vec<Regex>,
    output_res: Vec<Regex>,
}

impl Generator {
    /// Whether a change to `path` makes this check due.
    pub fn marks(&self, path: &str) -> bool {
        any_glob(&self.path_res, path)
    }

    /// Whether this generator declares `path` as a file it writes.
    pub fn writes(&self, path: &str) -> bool {
        any_glob(&self.output_res, path)
    }

    /// The command line as a person would type it, for logs and errors.
    pub fn check_line(&self) -> String {
        self.line_of(&self.check)
    }

    /// The build command line as a person would type it (empty when the entry has no build).
    pub fn build_line(&self) -> String {
        if self.build.is_empty() {
            String::new()
        } else {
            self.line_of(&self.build)
        }
    }

    fn line_of(&self, argv: &[String]) -> String {
        let args: Vec<String> = argv.iter().map(|a| quote(a)).collect();
        if self.cwd == "." {
            args.join(" ")
        } else {
            format!("cd {} && {}", self.cwd, args.join(" "))
        }
    }

    /// The regenerate command with its folder.
    pub fn regenerate_line(&self) -> String {
        if self.cwd == "." || self.regenerate.starts_with("none") {
            self.regenerate.clone()
        } else {
            format!("cd {} && {}", self.cwd, self.regenerate)
        }
    }
}

fn quote(a: &str) -> String {
    if a.is_empty() || a.contains(|c: char| c.is_whitespace() || "&|;<>()$`\"'".contains(c)) {
        format!("'{}'", a.replace('\'', "'\\''"))
    } else {
        a.to_string()
    }
}

/// The parsed registry, in file order.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    /// The generators.
    pub generators: Vec<Generator>,
}

impl Registry {
    /// Parses and validates the registry text. Every problem is listed (one run shows them all), none is skipped.
    pub fn parse(text: &str) -> Result<Registry, Vec<String>> {
        // One parse: a key the registry does not know, a value of the wrong type or text that is not TOML is the file's one problem.
        let file: crate::register_schema::GeneratorsFile = crate::register_schema::parse(PATH, text).map_err(|e| vec![e])?;
        let mut errors = Vec::new();
        match file.version {
            Some(1) => {}
            _ => errors.push(format!("{PATH}: `version = 1` is required")),
        }
        let mut generators = Vec::new();
        let mut seen = BTreeSet::new();
        if file.generator.is_empty() {
            errors.push(format!("{PATH}: no [[generator]] entry"));
        }
        for (i, t) in file.generator.into_iter().enumerate() {
            let id = t.id.clone().unwrap_or_default();
            let at = if id.is_empty() {
                format!("generator #{}", i + 1)
            } else {
                format!("generator `{id}`")
            };
            if id.is_empty() || !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
                errors.push(format!("{at}: `id` must be lower-case letters, digits and `-`"));
            } else if !seen.insert(id.clone()) {
                errors.push(format!("{at}: the id is used twice"));
            }
            let check = t.check.clone().unwrap_or_default();
            let manual = t.manual.clone().filter(|m| !m.is_empty());
            match (&manual, check.is_empty()) {
                (None, true) => errors.push(format!(
                    "{at}: `check` must be a non-empty array (the program and its arguments), or `manual = \"<why there is no check>\"`"
                )),
                (Some(_), false) => errors.push(format!("{at}: `manual` and `check` exclude each other")),
                _ => {}
            }
            let build = t.build.clone().unwrap_or_default();
            if build.is_empty() && compiles(&check) {
                errors.push(format!(
                    "{at}: `build` is required: `check` is a cargo command that compiles, so name the command that builds what it runs (for example [\"cargo\", \"test\", \"--no-run\", ...]); gen check runs it first with its own bound, so a cold build does not count against the check's hang guard"
                ));
            }
            if !build.is_empty() && compiles(&check) && (selectors(&build) != selectors(&check) || !build.iter().any(|a| a == "--no-run")) {
                errors.push(format!(
                    "{at}: `build` must build exactly what `check` runs: the same cargo subcommand, packages, targets and features as the check, plus `--no-run` (build {:?}, check {:?})",
                    build, check
                ));
            }
            let regenerate = t.regenerate.clone().unwrap_or_default();
            if regenerate.is_empty() {
                errors.push(format!(
                    "{at}: `regenerate` is required: the command that fixes drift (or `none: <what to do instead>`)"
                ));
            }
            let what = t.what.clone().unwrap_or_default();
            if what.is_empty() {
                errors.push(format!("{at}: `what` is required: one line saying what it generates"));
            }
            let paths = t.paths.clone().unwrap_or_default();
            let reads = t.reads.clone().unwrap_or_default();
            if paths.is_empty() && reads.is_empty() {
                errors.push(format!("{at}: `paths` (globs) or `reads` (model node ids) must name what marks it"));
            }
            for r in reads.iter().filter(|r| !r.contains(':') || r.starts_with(':') || r.ends_with(':')) {
                errors.push(format!(
                    "{at}: `reads` entry {r:?} is not a model node id (`kind:name`, for example `script:scripts/notices.sh`)"
                ));
            }
            let cost = match t.cost.clone().unwrap_or_default().as_str() {
                "fast" => Cost::Fast,
                "heavy" => Cost::Heavy,
                other => {
                    errors.push(format!("{at}: `cost` must be \"fast\" or \"heavy\", not {other:?}"));
                    Cost::Heavy
                }
            };
            let cwd = match t.cwd.clone().unwrap_or_default() {
                c if c.is_empty() => ".".to_string(),
                c => c,
            };
            if cwd.starts_with('/') || cwd.split('/').any(|p| p == "..") {
                errors.push(format!("{at}: `cwd` is a folder inside the repository, not {cwd:?}"));
            }
            let outputs = t.outputs.clone().unwrap_or_default();
            let (path_res, bad_paths) = globs(&paths);
            let (output_res, bad_outputs) = globs(&outputs);
            for g in bad_paths.into_iter().chain(bad_outputs) {
                errors.push(format!("{at}: the glob {g:?} is not a repository-relative slash path"));
            }
            generators.push(Generator {
                id,
                what,
                check,
                build,
                regenerate,
                cwd,
                paths,
                reads,
                outputs,
                cost,
                requires: t.requires.clone().unwrap_or_default(),
                manual,
                drift_marker: t.drift_marker.clone().unwrap_or_default(),
                path_res,
                output_res,
            });
        }
        if errors.is_empty() {
            Ok(Registry { generators })
        } else {
            Err(errors)
        }
    }

    /// The generators a change to `changed` makes due, in registry order.
    pub fn due<'a>(&'a self, changed: &[String]) -> Vec<&'a Generator> {
        self.generators.iter().filter(|g| changed.iter().any(|p| g.marks(p))).collect()
    }

    /// The generator with this id.
    pub fn get(&self, id: &str) -> Option<&Generator> {
        self.generators.iter().find(|g| g.id == id)
    }
}

/// Whether `check` is a cargo command that compiles before it runs (`cargo [+toolchain] test|run|bench`).
fn compiles(check: &[String]) -> bool {
    let mut words = check.iter().map(String::as_str);
    if words.next() != Some("cargo") {
        return false;
    }
    let sub = words.find(|w| !w.starts_with('+'));
    matches!(sub, Some("test" | "run" | "bench"))
}

/// What a cargo command builds: its subcommand and the flags that choose packages, targets, features and profile (with their
/// values), sorted. A test filter or `--no-run` does not change what is built.
fn selectors(argv: &[String]) -> Vec<String> {
    const VALUED: [&str; 8] = [
        "-p",
        "--package",
        "--test",
        "--bin",
        "--example",
        "--bench",
        "--features",
        "--profile",
    ];
    const BARE: [&str; 9] = [
        "--lib",
        "--bins",
        "--tests",
        "--workspace",
        "--all-features",
        "--all-targets",
        "--no-default-features",
        "--release",
        "--locked",
    ];
    let mut out = Vec::new();
    let mut words = argv.iter().skip(1).filter(|w| !w.starts_with('+'));
    if let Some(sub) = words.next() {
        out.push(format!("sub {sub}"));
    }
    while let Some(w) = words.next() {
        if VALUED.contains(&w.as_str()) {
            out.push(format!("{w} {}", words.next().map_or("", String::as_str)));
        } else if BARE.contains(&w.as_str()) || w.starts_with("--features=") || w.starts_with("--package=") {
            out.push(w.clone());
        }
    }
    out.sort();
    out
}

fn globs(list: &[String]) -> (Vec<Regex>, Vec<String>) {
    let (good, bad): (Vec<&String>, Vec<&String>) = list
        .iter()
        .partition(|g| !g.starts_with('/') && !g.contains('\\') && !g.contains(".."));
    (good.iter().map(|g| glob_regex(g)).collect(), bad.into_iter().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
version = 1
[[generator]]
id = "icons"
what = "installed icon sets"
check = ["node", "icons/tools/install.mjs", "--check"]
regenerate = "node icons/tools/install.mjs"
paths = ["icons/**", "apps/*/app/src-tauri/icons/**"]
outputs = ["apps/*/app/src-tauri/icons/**"]
cost = "fast"

[[generator]]
id = "hakari"
what = "workspace hack"
check = ["sh", "-c", "cargo hakari verify"]
regenerate = "cargo hakari generate"
cwd = "crates"
paths = ["crates/**/Cargo.toml", "crates/Cargo.lock"]
cost = "heavy"
requires = ["crates/.config/hakari.toml"]
"#;

    #[test]
    fn a_registry_parses_into_typed_entries() {
        let r = Registry::parse(GOOD).unwrap();
        assert_eq!(r.generators.len(), 2);
        let h = r.get("hakari").unwrap();
        assert_eq!(h.cost, Cost::Heavy);
        assert_eq!(h.check_line(), "cd crates && sh -c 'cargo hakari verify'");
        assert_eq!(h.regenerate_line(), "cd crates && cargo hakari generate");
        assert!(r.get("icons").unwrap().writes("apps/x/app/src-tauri/icons/a.png"));
        assert!(!r.get("hakari").unwrap().writes("crates/Cargo.lock"), "paths mark, outputs declare");
    }

    #[test]
    fn due_picks_only_the_generators_whose_paths_a_change_touches() {
        let r = Registry::parse(GOOD).unwrap();
        let ids = |c: &[&str]| {
            r.due(&c.iter().map(|s| s.to_string()).collect::<Vec<_>>())
                .iter()
                .map(|g| g.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&["crates/paddock-x/Cargo.toml"]), ["hakari"]);
        assert_eq!(ids(&["icons/tools/build.mjs", "crates/Cargo.lock"]), ["icons", "hakari"]);
        assert_eq!(ids(&["README.md", "apps/x/src/lib.rs"]), Vec::<String>::new());
    }

    #[test]
    fn every_problem_is_listed_not_only_the_first() {
        let bad = r#"
[[generator]]
id = "Bad Id"
check = []
cost = "slow"
paths = ["/abs"]
[[generator]]
id = "icons"
what = "x"
check = ["a"]
regenerate = "b"
paths = ["a"]
cost = "fast"
cwd = "../x"
[[generator]]
id = "icons"
what = "x"
check = ["a"]
regenerate = "b"
paths = ["a"]
cost = "fast"
"#;
        let errs = Registry::parse(bad).unwrap_err().join("\n");
        for needle in [
            "`version = 1` is required",
            "`id` must be lower-case",
            "`check` must be a non-empty array",
            "`regenerate` is required",
            "`what` is required",
            "`cost` must be",
            "is not a repository-relative slash path",
            "`cwd` is a folder inside the repository",
            "the id is used twice",
        ] {
            assert!(errs.contains(needle), "{needle} missing from:\n{errs}");
        }
    }

    #[test]
    fn a_manual_entry_has_no_check_and_a_check_entry_has_no_manual() {
        let entry = |extra: &str| {
            format!("version = 1\n[[generator]]\nid = \"a\"\nwhat = \"w\"\nregenerate = \"r\"\npaths = [\"p\"]\ncost = \"fast\"\n{extra}\n")
        };
        let ok = Registry::parse(&entry("manual = \"needs the network\"")).unwrap();
        assert_eq!(ok.generators[0].manual.as_deref(), Some("needs the network"));
        assert!(ok.generators[0].check.is_empty());
        assert!(Registry::parse(&entry("manual = \"x\"\ncheck = [\"a\"]")).unwrap_err()[0].contains("exclude each other"));
        assert!(Registry::parse(&entry("")).unwrap_err()[0].contains("or `manual ="));
    }

    #[test]
    fn a_cargo_check_that_compiles_names_its_build_and_the_build_is_kept() {
        let entry = |check: &str, extra: &str| {
            format!(
                "version = 1\n[[generator]]\nid = \"a\"\nwhat = \"w\"\nregenerate = \"r\"\npaths = [\"p\"]\ncost = \"heavy\"\ncheck = {check}\n{extra}\n"
            )
        };
        for check in [
            r#"["cargo", "test", "-p", "x"]"#,
            r#"["cargo", "run", "--bin", "y"]"#,
            r#"["cargo", "+nightly", "test"]"#,
        ] {
            let errs = Registry::parse(&entry(check, "")).unwrap_err().join("\n");
            assert!(errs.contains("`build` is required"), "{check}: {errs}");
        }
        let ok = Registry::parse(&entry(
            r#"["cargo", "test", "-p", "x", "t"]"#,
            r#"build = ["cargo", "test", "--no-run", "-p", "x"]"#,
        ))
        .unwrap();
        assert_eq!(ok.generators[0].build, ["cargo", "test", "--no-run", "-p", "x"]);
        assert_eq!(ok.generators[0].build_line(), "cargo test --no-run -p x");
        // A build that builds something else than the check runs is refused.
        let errs = Registry::parse(&entry(
            r#"["cargo", "test", "-p", "x", "--lib", "t"]"#,
            r#"build = ["cargo", "test", "--no-run", "-p", "y", "--lib"]"#,
        ))
        .unwrap_err()
        .join("\n");
        assert!(errs.contains("must build exactly what `check` runs"), "{errs}");
        let errs = Registry::parse(&entry(
            r#"["cargo", "test", "-p", "x", "t"]"#,
            r#"build = ["cargo", "test", "-p", "x"]"#,
        ))
        .unwrap_err()
        .join("\n");
        assert!(errs.contains("plus `--no-run`"), "{errs}");
        // Checks that compile nothing need no build.
        assert!(Registry::parse(&entry(r#"["cargo", "metadata", "--locked"]"#, "")).is_ok());
        assert!(Registry::parse(&entry(r#"["sh", "-c", "cargo hakari verify"]"#, "")).is_ok());
        assert!(Registry::parse(&entry(r#"["node", "x.mjs"]"#, "build = []")).is_ok());
    }

    #[test]
    fn reads_name_model_nodes_and_stand_in_for_paths_and_a_bad_id_is_refused() {
        let entry = |extra: &str| {
            format!("version = 1\n[[generator]]\nid = \"a\"\nwhat = \"w\"\nregenerate = \"r\"\ncheck = [\"x\"]\ncost = \"fast\"\n{extra}\n")
        };
        let ok = Registry::parse(&entry("reads = [\"script:scripts/notices.sh\"]")).unwrap();
        assert_eq!(ok.generators[0].reads, ["script:scripts/notices.sh"]);
        assert!(ok.generators[0].paths.is_empty(), "reads alone is enough");
        let both = Registry::parse(&entry("reads = [\"wf:ci.yml\"]\npaths = [\"a/**\"]")).unwrap();
        assert!(both.generators[0].marks("a/b") && both.generators[0].reads == ["wf:ci.yml"]);
        // controls: neither paths nor reads, and an id that is not `kind:name`, are refused
        assert!(
            Registry::parse(&entry(""))
                .unwrap_err()
                .join("\n")
                .contains("`paths` (globs) or `reads`")
        );
        assert!(
            Registry::parse(&entry("reads = [\"scripts/notices.sh\"]"))
                .unwrap_err()
                .join("\n")
                .contains("not a model node id")
        );
    }

    #[test]
    fn text_that_is_not_toml_or_has_no_entry_is_an_error() {
        assert!(Registry::parse("[[generator").unwrap_err()[0].contains("docs/generators.toml:1"));
        let colour = format!("version = 1\n[[generator]]\nid = \"a\"\ncolour = 1\n");
        assert!(Registry::parse(&colour).unwrap_err()[0].contains("unknown field `colour`"));
        assert!(Registry::parse("version = 1").unwrap_err()[0].contains("no [[generator]] entry"));
    }
}
