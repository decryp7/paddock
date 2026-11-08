//! What counts as a check (ADR 0060 §1): one enumerator per kind, registered in [`enumerate`] (open/closed: a new kind is a
//! new enumerator there, with its fixtures). The register holds data about instances, never the list of kinds.

use super::scan;
use super::tree::{Tree, any_glob, glob_regex};
use serde_json::Value as Json;
use std::collections::BTreeSet;

/// The kinds of §1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// A `paddock-conformance` check (`C13`).
    Conformance,
    /// A check `./dev` implements (`C100`).
    DevCheck,
    /// An ignored Rust test.
    IgnoredTest,
    /// A test that is skipped or returns early without an environment variable.
    EnvGatedTest,
    /// A script named as a check.
    Script,
    /// An npm script named `check*`, `test*` or `lint*`.
    NpmCheck,
    /// A local action.
    Action,
    /// A reusable workflow.
    Workflow,
}

impl Kind {
    /// The word used in reports and the register.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Conformance => "conformance",
            Kind::DevCheck => "dev-check",
            Kind::IgnoredTest => "ignored-test",
            Kind::EnvGatedTest => "env-gated-test",
            Kind::Script => "script",
            Kind::NpmCheck => "npm-check",
            Kind::Action => "action",
            Kind::Workflow => "workflow",
        }
    }
}

/// Where a conformance or dev check runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The repository's own tree (`--app paddock --root .`).
    Root,
    /// Each app it applies to.
    Apps,
    /// The root run covers the apps' files, or every app runs it.
    Both,
}

/// What an app needs for an `apps`-scope check to apply to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needs {
    /// Every app with a contract.
    All,
    /// Apps with a Cargo workspace (`cargo` in `paddock.ci.json`).
    Rust,
    /// Apps with a UI (`ui` commands or `angularDir`).
    Ui,
}

/// One check.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    /// Its kind.
    pub kind: Kind,
    /// Its id (`C13`, `path::name`, a path, `ui:test:tokens`, `action:x`, `workflow:x.yml`).
    pub id: String,
    /// The file that defines it, if any.
    pub file: Option<String>,
    /// The line there.
    pub line: usize,
    /// Conformance and dev checks: where they run.
    pub scope: Scope,
    /// Conformance and dev checks: whether findings can fail a run.
    pub report_only: bool,
    /// For `apps` scope: which apps it applies to.
    pub needs: Needs,
    /// Ignored tests: the `cfg_attr` condition; env-gated tests: the variable.
    pub detail: Option<String>,
}

impl Check {
    fn new(kind: Kind, id: impl Into<String>, file: Option<&str>, line: usize) -> Check {
        Check {
            kind,
            id: id.into(),
            file: file.map(str::to_string),
            line,
            scope: Scope::Both,
            report_only: false,
            needs: Needs::All,
            detail: None,
        }
    }
}

/// A check `./dev` implements (`./dev gates list` prints these).
#[derive(Debug, Clone, Copy)]
pub struct CheckInfo {
    /// Its id.
    pub id: &'static str,
    /// The `./dev` subcommand that runs it.
    pub subcommand: &'static str,
    /// Where it runs.
    pub scope: Scope,
    /// Report-only?
    pub report_only: bool,
}

/// The one table of `./dev` checks (ADR 0060 §1): `./dev` has no `help --json`, and clap's command tree alone cannot tell a
/// check from a utility.
pub const CHECKS: &[CheckInfo] = &[CheckInfo {
    id: "C100",
    subcommand: "gates check",
    scope: Scope::Root,
    report_only: false,
}];

/// The `paddock-conformance list --json` output as checks. The JSON is
/// `[{"id": "C13", "scope": "apps", "mode": "gate", "needs": "rust", "file": "conformance/src/checks/C13-rust-lints.mjs"}, …]`.
pub fn conformance_checks(list_json: &str) -> Result<Vec<Check>, String> {
    let v: Json = serde_json::from_str(list_json).map_err(|e| format!("`paddock-conformance list --json` is not JSON: {e}"))?;
    let arr = v.as_array().ok_or("`paddock-conformance list --json` is not an array")?;
    let mut out = Vec::new();
    for item in arr {
        let id = item
            .get("id")
            .and_then(Json::as_str)
            .ok_or("a check without an id in `list --json`")?;
        let scope = match item.get("scope").and_then(Json::as_str) {
            Some("root") => Scope::Root,
            Some("apps") => Scope::Apps,
            Some("both") => Scope::Both,
            other => {
                return Err(format!(
                    "check {id}: scope {other:?} is not root, apps or both (add it to its module)"
                ));
            }
        };
        let report_only = match item.get("mode").and_then(Json::as_str) {
            Some("gate") => false,
            Some("report-only") => true,
            other => {
                return Err(format!(
                    "check {id}: mode {other:?} is not gate or report-only (add it to its module)"
                ));
            }
        };
        let needs = match item.get("needs").and_then(Json::as_str) {
            None | Some("all") => Needs::All,
            Some("rust") => Needs::Rust,
            Some("ui") => Needs::Ui,
            Some(other) => return Err(format!("check {id}: needs {other:?} is not all, rust or ui")),
        };
        let mut c = Check::new(Kind::Conformance, id, item.get("file").and_then(Json::as_str), 1);
        c.scope = scope;
        c.report_only = report_only;
        c.needs = needs;
        out.push(c);
    }
    Ok(out)
}

/// The `./dev` checks of [`CHECKS`].
pub fn dev_checks() -> Vec<Check> {
    CHECKS
        .iter()
        .map(|i| {
            let mut c = Check::new(Kind::DevCheck, i.id, Some("tools/paddock-dev/src/gates/checks.rs"), 1);
            c.scope = i.scope;
            c.report_only = i.report_only;
            c
        })
        .collect()
}

const SCRIPT_ROOTS: &[&str] = &["scripts/**", "conformance/**", "apps/*/scripts/**", "tools/**"];
const SCRIPT_NAMES: &[&str] = &["check-*.mjs", "check-*.sh", "check-*.ps1", "*.test.mjs"];

fn ignored_path(p: &str) -> bool {
    ["/node_modules/", "/fixtures/", "/vendor/", "/target/", "/spikes/"]
        .iter()
        .any(|x| p.contains(x))
        || p.starts_with("node_modules/")
}

/// Scripts named as checks: `check-*.{mjs,sh,ps1}` and `*.test.mjs` under the script folders.
pub fn script_checks(tree: &dyn Tree) -> Vec<Check> {
    let mut globs = Vec::new();
    for r in SCRIPT_ROOTS {
        for n in SCRIPT_NAMES {
            globs.push(glob_regex(&format!("{}/{}", r.trim_end_matches("/**"), n)));
            globs.push(glob_regex(&format!("{r}/{n}")));
        }
    }
    tree.files()
        .iter()
        .filter(|p| !ignored_path(p) && any_glob(&globs, p))
        .map(|p| Check::new(Kind::Script, p.clone(), Some(p), 1))
        .collect()
}

/// `check*`, `test*` and `lint*` scripts of every tracked `package.json`.
pub fn npm_checks(tree: &dyn Tree) -> Vec<Check> {
    let mut out = Vec::new();
    for p in tree
        .files()
        .iter()
        .filter(|p| (p.as_str() == "package.json" || p.ends_with("/package.json")) && !ignored_path(p))
    {
        let Some(text) = tree.read(p) else { continue };
        let Ok(json) = serde_json::from_str::<Json>(&text) else { continue };
        let dir = p.strip_suffix("/package.json").unwrap_or("root");
        if let Some(scripts) = json.get("scripts").and_then(Json::as_object) {
            for name in scripts
                .keys()
                .filter(|n| ["check", "test", "lint"].iter().any(|k| n.starts_with(k)))
            {
                out.push(Check::new(Kind::NpmCheck, format!("{dir}:{name}"), Some(p), 1));
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Every local action.
pub fn action_checks(action_dirs: &BTreeSet<String>) -> Vec<Check> {
    action_dirs
        .iter()
        .map(|d| {
            let name = d.rsplit('/').next().unwrap_or(d);
            Check::new(Kind::Action, format!("action:{name}"), Some(&format!("{d}/action.yml")), 1)
        })
        .collect()
}

/// Every workflow with `workflow_call`.
pub fn workflow_checks(callable: &BTreeSet<String>) -> Vec<Check> {
    callable
        .iter()
        .map(|p| {
            Check::new(
                Kind::Workflow,
                format!("workflow:{}", p.rsplit('/').next().unwrap_or(p)),
                Some(p),
                1,
            )
        })
        .collect()
}

/// Ignored and env-gated tests of every Rust and TypeScript test file.
pub fn test_checks(tree: &dyn Tree) -> Vec<Check> {
    let mut out = Vec::new();
    for p in tree.files() {
        if ignored_path(p) || p.contains("/dist/") {
            continue;
        }
        let rust = p.ends_with(".rs");
        let ts = p.ends_with(".spec.ts") || p.ends_with(".test.ts") || p.ends_with(".e2e.ts");
        if !(rust || ts) {
            continue;
        }
        let Some(text) = tree.read(p) else { continue };
        let (ign, env) = if rust {
            (scan::ignored_tests(p, &text), scan::env_gated_rust(p, &text))
        } else {
            (Vec::new(), scan::env_gated_ts(p, &text))
        };
        for (kind, sites) in [(Kind::IgnoredTest, ign), (Kind::EnvGatedTest, env)] {
            for s in sites {
                let mut c = Check::new(kind, format!("{}::{}", s.path, s.name), Some(&s.path), s.line);
                c.detail = s.condition;
                out.push(c);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gates::tree::MemTree;

    #[test]
    fn conformance_list_good_and_bad() {
        let good = r#"[{"id":"C13","scope":"apps","mode":"gate","needs":"rust","file":"conformance/src/checks/C13-rust-lints.mjs"},{"id":"C51","scope":"both","mode":"report-only"}]"#;
        let c = conformance_checks(good).unwrap();
        assert_eq!((c[0].scope, c[0].needs, c[0].report_only), (Scope::Apps, Needs::Rust, false));
        assert!(c[1].report_only);
        // a module that declares no scope is an error, not a default
        let bad = r#"[{"id":"C99","mode":"gate"}]"#;
        assert!(conformance_checks(bad).unwrap_err().contains("scope"));
        assert!(conformance_checks("not json").is_err());
    }

    #[test]
    fn scripts_and_npm_scripts() {
        let t = MemTree::new(&[
            ("scripts/check-x.mjs", ""),
            ("scripts/affected.test.mjs", ""),
            ("apps/dyno/scripts/check-bundle.sh", ""),
            ("apps/dyno/scripts/other.sh", ""),
            ("conformance/test/c13.test.mjs", ""),
            ("conformance/test/fixtures/a.test.mjs", ""),
            ("ui/package.json", r#"{"scripts":{"test:tokens":"x","build":"y","lint":"z"}}"#),
            ("package.json", r#"{"scripts":{"test:affected":"node --test"}}"#),
        ]);
        let ids: Vec<_> = script_checks(&t).into_iter().map(|c| c.id).collect();
        assert_eq!(
            ids,
            vec![
                "apps/dyno/scripts/check-bundle.sh",
                "conformance/test/c13.test.mjs",
                "scripts/affected.test.mjs",
                "scripts/check-x.mjs"
            ]
        );
        let ids: Vec<_> = npm_checks(&t).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, vec!["root:test:affected", "ui:lint", "ui:test:tokens"]);
    }

    #[test]
    fn tests_of_both_kinds_are_found() {
        let t = MemTree::new(&[
            (
                "crates/a/tests/it.rs",
                "#[test]\n#[ignore = \"slow\"]\nfn slow() {}\n#[test]\nfn vm() {\n    if std::env::var(\"PADDOCK_VM\").is_err() { return; }\n}\n",
            ),
            (
                "e2e/s.spec.ts",
                "const OUT = process.env.X ?? '';\ntest('t', async () => { test.skip(!OUT, 'x'); });\n",
            ),
        ]);
        let ids: Vec<_> = test_checks(&t).into_iter().map(|c| (c.kind.name(), c.id)).collect();
        assert_eq!(
            ids,
            vec![
                ("ignored-test", "crates/a/tests/it.rs::slow".to_string()),
                ("env-gated-test", "crates/a/tests/it.rs::vm".to_string()),
                ("env-gated-test", "e2e/s.spec.ts::t".to_string())
            ]
        );
    }
}
