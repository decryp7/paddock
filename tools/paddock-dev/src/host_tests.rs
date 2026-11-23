//! `hostTests`: which tests run on the Windows and macOS legs (OS-scoped tests, docs/research/2026-11-22-os-scoped-tests.md, user
//! decision 2026-11-22: "a test runs on an OS only if its code behaves differently there").
//!
//! One declared field of a CI contract (`crates/paddock.ci.json`, `apps/<app>/paddock.ci.json`, `tools/paddock-dev/paddock.ci.json`),
//! read through this one accessor by the planner ([`crate::plan_ci`]) and by the conformance check C155 ([`crate::check::c155`]),
//! never by grepping the file (ER-42):
//!
//! ```jsonc
//! "hostTests": {
//!   "packages": ["paddock-proc", "paddock-service"],           // Cargo packages compiled and tested on the Windows and macOS legs
//!   "skip":     {"paddock-service": ["model::", "scenarios::"]}, // modules of a host package that are pure logic: Linux only
//!   "modules":  ["ci_host::", "slots::"],                      // one-crate workspaces (paddock-dev): test-name prefixes
//!   "only":     {"macos-latest": ["pre_push::"]}               // per-OS additions (a package name, or a prefix ending in `::`)
//! }
//! ```
//!
//! A workspace without the field keeps today's behaviour (every test on every OS); with it, the Linux leg still runs everything
//! and a host leg runs only what the field names. The planner turns it into a `-p` list and a nextest filterset ([`HostScope`]).

use serde_json::Value;
use std::collections::BTreeMap;

/// The runner that runs every test (never scoped).
pub const LINUX: &str = "ubuntu-latest";

/// The host runners a `only` key may name.
pub const HOST_OSES: [&str; 2] = ["windows-latest", "macos-latest"];

/// The declared `hostTests` of one workspace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostTests {
    /// Cargo packages tested on every host runner.
    pub packages: Vec<String>,
    /// Per host package: test-name prefixes (`model::`) that stay on Linux.
    pub skip: BTreeMap<String, Vec<String>>,
    /// Test-name prefixes (`ci_host::`) tested on every host runner, for a one-crate workspace.
    pub modules: Vec<String>,
    /// Per host runner: more packages (a plain name) or prefixes (ending in `::`).
    pub only: BTreeMap<String, Vec<String>>,
}

/// What one host leg runs: the packages to build and test (`-p`, empty: the workspace's) and the nextest filterset (empty: none).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostScope {
    /// The Cargo packages (`cargo nextest run -p a -p b`), sorted; empty when the workspace is scoped by modules only.
    pub packages: Vec<String>,
    /// The nextest filterset that applies the declared modules and skips; empty when none.
    pub filter: String,
}

fn is_prefix(entry: &str) -> bool {
    entry.ends_with("::")
}

fn strings(v: &Value, what: &str) -> Result<Vec<String>, String> {
    let Some(a) = v.as_array() else {
        return Err(format!("{what} must be an array of strings"));
    };
    let mut out = Vec::new();
    for e in a {
        let Some(s) = e.as_str().filter(|s| !s.is_empty()) else {
            return Err(format!("{what} must hold only non-empty strings"));
        };
        out.push(s.to_string());
    }
    Ok(out)
}

fn prefix_ok(p: &str) -> bool {
    is_prefix(p) && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
}

impl HostTests {
    /// The `hostTests` of a parsed contract: `Ok(None)` when it has none (the workspace is not scoped), an error naming `what` (the
    /// contract's path) when the field is malformed (a typo must never silently turn the scope off).
    pub fn parse(contract: &Value, what: &str) -> Result<Option<HostTests>, String> {
        let Some(v) = contract.get("hostTests") else {
            return Ok(None);
        };
        let Some(obj) = v.as_object() else {
            return Err(format!("{what}: `hostTests` must be an object"));
        };
        let mut h = HostTests::default();
        for (k, val) in obj {
            let at = format!("{what}: `hostTests.{k}`");
            match k.as_str() {
                "packages" => h.packages = strings(val, &at)?,
                "modules" => {
                    h.modules = strings(val, &at)?;
                    if let Some(bad) = h.modules.iter().find(|m| !prefix_ok(m)) {
                        return Err(format!(
                            "{at}: `{bad}` must be a test-name prefix ending in `::` (letters, digits, `_`, `:`)"
                        ));
                    }
                }
                "skip" => {
                    let Some(m) = val.as_object() else {
                        return Err(format!("{at} must be an object of package to prefixes"));
                    };
                    for (pkg, prefixes) in m {
                        let list = strings(prefixes, &format!("{at}.{pkg}"))?;
                        if let Some(bad) = list.iter().find(|p| !prefix_ok(p)) {
                            return Err(format!("{at}.{pkg}: `{bad}` must be a test-name prefix ending in `::`"));
                        }
                        h.skip.insert(pkg.clone(), list);
                    }
                }
                "only" => {
                    let Some(m) = val.as_object() else {
                        return Err(format!("{at} must be an object of runner to entries"));
                    };
                    for (os, entries) in m {
                        if !HOST_OSES.contains(&os.as_str()) {
                            return Err(format!("{at}: `{os}` is not a host runner ({})", HOST_OSES.join(", ")));
                        }
                        let list = strings(entries, &format!("{at}.{os}"))?;
                        if let Some(bad) = list.iter().find(|e| is_prefix(e) && !prefix_ok(e)) {
                            return Err(format!("{at}.{os}: `{bad}` is not a valid test-name prefix"));
                        }
                        h.only.insert(os.clone(), list);
                    }
                }
                other => {
                    return Err(format!(
                        "{what}: unknown `hostTests.{other}` (known: packages, skip, modules, only)"
                    ));
                }
            }
        }
        if let Some(pkg) = h.skip.keys().find(|p| !h.packages.contains(p)) {
            return Err(format!(
                "{what}: `hostTests.skip` names `{pkg}`, which is not in `hostTests.packages`"
            ));
        }
        // A workspace is scoped by packages (`-p`) or by test-name prefixes (a one-crate workspace), never both: a prefix over a package
        // list would restrict the listed packages to those modules instead of adding them.
        let only_prefix = h.only.values().flatten().any(|e| is_prefix(e));
        if !h.packages.is_empty() && (!h.modules.is_empty() || only_prefix) {
            return Err(format!(
                "{what}: `hostTests` mixes `packages` with test-name prefixes (`modules` or an `only` entry ending in `::`); a workspace is scoped by one or the other"
            ));
        }
        if h.packages.is_empty() && h.modules.is_empty() {
            return Err(format!(
                "{what}: `hostTests` names no package and no module (a host leg would test nothing; delete the field to test everything)"
            ));
        }
        Ok(Some(h))
    }

    /// The packages and filterset a host leg on `os` runs; a Linux runner is never scoped (the default scope).
    pub fn scope(&self, os: &str) -> HostScope {
        if os == LINUX {
            return HostScope::default();
        }
        let extra = self.only.get(os).map(Vec::as_slice).unwrap_or(&[]);
        let mut packages = self.packages.clone();
        let mut modules = self.modules.clone();
        for e in extra {
            if is_prefix(e) {
                modules.push(e.clone());
            } else {
                packages.push(e.clone());
            }
        }
        packages.sort();
        packages.dedup();
        modules.sort();
        modules.dedup();
        let mut parts: Vec<String> = Vec::new();
        if !modules.is_empty() {
            parts.push(format!("test(/^(?:{})/)", modules.join("|")));
        }
        let skipped: Vec<String> = self
            .skip
            .iter()
            .filter(|(p, _)| packages.contains(p))
            .map(|(p, prefixes)| format!("(package(={p}) & test(/^(?:{})/))", prefixes.join("|")))
            .collect();
        if !skipped.is_empty() {
            parts.push(format!("not ({})", skipped.join(" | ")));
        }
        HostScope {
            packages,
            filter: parts.join(" & "),
        }
    }

    /// The test-name prefixes (`ci_host::`) tested on `os`: the common modules and that runner's own, sorted.
    pub fn module_prefixes(&self, os: &str) -> Vec<String> {
        let mut out = self.modules.clone();
        out.extend(self.only.get(os).into_iter().flatten().filter(|e| is_prefix(e)).cloned());
        out.sort();
        out.dedup();
        out
    }

    /// Whether `package` is tested on `os` (a host runner): listed for every host, or for that one.
    pub fn package_runs_on(&self, package: &str, os: &str) -> bool {
        self.packages.iter().any(|p| p == package) || self.only.get(os).is_some_and(|l| l.iter().any(|e| e == package))
    }

    /// Whether `package` is tested on a host runner (on every one, or on one of them).
    pub fn lists_package(&self, package: &str) -> bool {
        self.packages.iter().any(|p| p == package) || self.only.values().flatten().any(|e| e == package)
    }

    /// Whether the test-name prefix `module` (`ci_host::`) is tested on a host runner.
    pub fn lists_module(&self, module: &str) -> bool {
        self.modules.iter().any(|m| m == module) || self.only.values().flatten().any(|e| e == module)
    }
}

/// `a` and `b` as one nextest filterset that needs both (`(a) & (b)`); either may be empty.
pub fn and_filters(a: &str, b: &str) -> String {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => String::new(),
        (false, true) => a.to_string(),
        (true, false) => b.to_string(),
        (false, false) => format!("({a}) & ({b})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(v: Value) -> HostTests {
        HostTests::parse(&json!({ "hostTests": v }), "x.json").unwrap().unwrap()
    }

    #[test]
    fn a_contract_without_the_field_is_not_scoped_and_a_malformed_one_is_refused() {
        assert_eq!(HostTests::parse(&json!({}), "x.json").unwrap(), None);
        for (bad, why) in [
            (json!([]), "must be an object"),
            (json!({"packages": "a"}), "array of strings"),
            (json!({"packages": ["a", ""]}), "non-empty"),
            (json!({"pakages": ["a"]}), "unknown `hostTests.pakages`"),
            (json!({"packages": ["a"], "skip": {"b": ["m::"]}}), "not in `hostTests.packages`"),
            (json!({"packages": ["a"], "skip": {"a": ["m"]}}), "ending in `::`"),
            (json!({"modules": ["m"]}), "ending in `::`"),
            (json!({"packages": ["a"], "only": {"ubuntu-latest": ["b"]}}), "not a host runner"),
            (json!({}), "names no package and no module"),
            (
                json!({"packages": ["a"], "modules": ["m::"]}),
                "mixes `packages` with test-name prefixes",
            ),
            (
                json!({"packages": ["a"], "only": {"macos-latest": ["m::"]}}),
                "mixes `packages` with test-name prefixes",
            ),
            (json!({"modules": ["a b::"]}), "ending in `::`"),
        ] {
            let e = HostTests::parse(&json!({ "hostTests": bad }), "x.json").unwrap_err();
            assert!(e.contains(why) && e.starts_with("x.json"), "{bad}: {e}");
        }
    }

    #[test]
    fn linux_is_never_scoped_and_a_host_runner_gets_its_packages_and_its_own_additions() {
        let h = parse(json!({"packages": ["b", "a"], "only": {"macos-latest": ["c"]}}));
        assert_eq!(h.scope(LINUX), HostScope::default());
        let win = h.scope("windows-latest");
        assert_eq!(
            (win.packages.as_slice(), win.filter.as_str()),
            (["a".to_string(), "b".to_string()].as_slice(), "")
        );
        let mac = h.scope("macos-latest");
        assert_eq!(mac.packages, ["a", "b", "c"]);
        assert_eq!(mac.filter, "");
        let by_module = parse(json!({"modules": ["a::"], "only": {"macos-latest": ["m::"]}}));
        assert_eq!(by_module.scope("macos-latest").filter, "test(/^(?:a::|m::)/)");
        assert_eq!(by_module.scope("windows-latest").filter, "test(/^(?:a::)/)");
    }

    #[test]
    fn modules_become_one_anchored_test_name_filter_and_skips_become_a_negated_one() {
        let h = parse(json!({"modules": ["b::", "a_b::"]}));
        assert_eq!(h.scope("windows-latest").filter, "test(/^(?:a_b::|b::)/)");
        assert!(h.scope("windows-latest").packages.is_empty());
        let s = parse(json!({"packages": ["p", "q"], "skip": {"p": ["model::", "x::y::"]}}));
        assert_eq!(s.scope("macos-latest").filter, "not ((package(=p) & test(/^(?:model::|x::y::)/)))");
        assert_eq!(
            and_filters("package(=p)", &s.scope("macos-latest").filter),
            "(package(=p)) & (not ((package(=p) & test(/^(?:model::|x::y::)/))))"
        );
    }

    #[test]
    fn and_filters_keeps_either_side_alone_and_joins_two() {
        assert_eq!(and_filters("", ""), "");
        assert_eq!(and_filters("a", ""), "a");
        assert_eq!(and_filters("", "b"), "b");
        assert_eq!(and_filters("a | b", "c"), "(a | b) & (c)");
    }

    #[test]
    fn lists_say_whether_a_package_or_module_runs_on_any_host() {
        let h = parse(json!({"modules": ["m::"], "only": {"macos-latest": ["n::"]}}));
        assert!(h.lists_module("m::") && h.lists_module("n::") && !h.lists_module("o::"));
        let h = parse(json!({"packages": ["a"], "only": {"macos-latest": ["b"]}}));
        assert!(h.lists_package("a") && h.lists_package("b") && !h.lists_package("c"));
    }
}
