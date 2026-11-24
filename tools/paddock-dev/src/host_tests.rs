//! `hostTests`: which tests run on the Windows and macOS legs (OS-scoped tests, docs/research/2026-11-22-os-scoped-tests.md, user
//! decision 2026-11-22: "a test runs on an OS only if its code behaves differently there").
//!
//! `hostTests`: which tests run on the Windows and macOS legs (OS-scoped tests, docs/research/2026-11-22-os-scoped-tests.md, user
//! decision 2026-11-22: "a test runs on an OS only if its code behaves differently there"; ADR 0117 §7, WP-6).
//!
//! The placement is **derived from the code** (`crate::os_placement`: OS `cfg`, OS-API crates, portable-layer entry points,
//! `build.rs`, `env::consts::OS`, `process.platform`, OS-behaviour dependencies; the `os` edges of the repository graph). The field of
//! a CI contract (`crates/paddock.ci.json`, `apps/<app>/paddock.ci.json`, `tools/paddock-dev/paddock.ci.json`) holds only the
//! **exceptions**, each with a reason, read through this one accessor by the planner ([`crate::plan_ci`]) and by the conformance check
//! C155 ([`crate::check::c155`]), never by grepping the file (ER-42):
//!
//! ```jsonc
//! "hostTests": {
//!   // os.add (free): tested on the hosts although no signal marks it (a package name, or a test-name prefix ending in `::`)
//!   "add":  [{"name": "paddock-diag", "reason": "its tests drive a spawned helper"}],
//!   // os.skip (the unsafe direction, needs the user, C155 enforces it against the merge base): kept on Linux although marked;
//!   // a package or prefix, or only some modules of a host package (`modules`)
//!   "skip": [{"name": "paddock-service", "modules": ["scenarios::"], "reason": "pure logic"}],
//!   // os.skip of one host (also needs the user): tested on this host only
//!   "only": [{"os": "macos-latest", "name": "pre_push::", "reason": "a tool of the dev Mac"}]
//! }
//! ```
//!
//! A workspace without the field keeps today's behaviour (every test on every OS); with it, the Linux leg still runs everything and a
//! host leg runs the derived placement with the exceptions applied. The planner turns it into a `-p` list and a nextest filterset
//! ([`HostScope`]). [`read`] is the one place the derived placement and the exceptions meet; [`HostTests`] is the resolved result.

use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

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

impl HostScope {
    /// The scope as one nextest filterset (its packages as `package(=a) | package(=b)`, and its filter); empty when it selects
    /// nothing in particular (every test).
    pub fn filterset(&self) -> String {
        let packages = self
            .packages
            .iter()
            .map(|p| format!("package(={p})"))
            .collect::<Vec<_>>()
            .join(" | ");
        and_filters(&packages, &self.filter)
    }
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
        if h.packages.is_empty() && h.modules.is_empty() && h.only.is_empty() {
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
        self.scope_with(self.only.get(os).map(Vec::as_slice).unwrap_or(&[]))
    }

    /// The packages and filterset of the code that behaves differently per OS, without any one host's own `only` entries: what the
    /// member gate's Linux container run tests beside its macOS run (user, 2026-11-25).
    pub fn common_scope(&self) -> HostScope {
        self.scope_with(&[])
    }

    fn scope_with(&self, extra: &[String]) -> HostScope {
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

/// The shortest reason an exception may give (the same floor as the allow lists').
pub const MIN_REASON: usize = 8;

/// What the code says about a workspace: the packages (or, for a one-crate workspace, the test-name prefixes) whose tests behave
/// differently per OS, and everything that exists (so a stale exception is told from a redundant one).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Derived {
    /// The member packages with an OS mark.
    pub packages: BTreeSet<String>,
    /// The test-name prefixes (`ci_host::`) of the modules with an OS mark, for a one-crate workspace.
    pub modules: BTreeSet<String>,
    /// Every member package.
    pub known_packages: BTreeSet<String>,
    /// Every module prefix of a one-crate workspace.
    pub known_modules: BTreeSet<String>,
}

/// One declared exception.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exception {
    /// A package name, or a test-name prefix ending in `::`.
    pub name: String,
    /// Modules of a host package kept on Linux (`skip` only).
    pub modules: Vec<String>,
    /// The one host it is tested on (`only` only).
    pub os: Option<String>,
    /// Why (at least [`MIN_REASON`] characters).
    pub reason: String,
}

/// The declared `hostTests` exceptions of one workspace, by direction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Exceptions {
    /// os.add: free.
    pub add: Vec<Exception>,
    /// os.skip: needs the user.
    pub skip: Vec<Exception>,
    /// os.skip of one host: needs the user.
    pub only: Vec<Exception>,
}

fn entries(v: &Value, at: &str, key: &str) -> Result<Vec<Exception>, String> {
    let Some(list) = v.as_array() else {
        return Err(format!("{at} must be an array of {{name, reason}} objects"));
    };
    let mut out = Vec::new();
    for (i, e) in list.iter().enumerate() {
        let here = format!("{at}[{i}]");
        let Some(o) = e.as_object() else {
            return Err(format!("{here} must be an object"));
        };
        for k in o.keys() {
            let known: &[&str] = match key {
                "skip" => &["name", "modules", "reason"],
                "only" => &["name", "os", "reason"],
                _ => &["name", "reason"],
            };
            if !known.contains(&k.as_str()) {
                return Err(format!("{here}: unknown `{k}` (known: {})", known.join(", ")));
            }
        }
        let name = o.get("name").and_then(Value::as_str).filter(|n| !n.is_empty());
        let Some(name) = name else {
            return Err(format!("{here}: `name` must be a non-empty string"));
        };
        if is_prefix(name) && !prefix_ok(name) {
            return Err(format!("{here}: `{name}` is not a valid test-name prefix"));
        }
        let reason = o.get("reason").and_then(Value::as_str).map(str::trim).unwrap_or("");
        if reason.chars().count() < MIN_REASON {
            return Err(format!(
                "{here}: `{name}` needs a `reason` of at least {MIN_REASON} characters (why this exception, not the derived placement)"
            ));
        }
        let modules = match o.get("modules") {
            None => Vec::new(),
            Some(m) => {
                let list = strings(m, &format!("{here}.modules"))?;
                if let Some(bad) = list.iter().find(|p| !prefix_ok(p)) {
                    return Err(format!("{here}.modules: `{bad}` must be a test-name prefix ending in `::`"));
                }
                list
            }
        };
        let os = match o.get("os") {
            None => None,
            Some(v) => match v.as_str().filter(|s| HOST_OSES.contains(s)) {
                Some(s) => Some(s.to_string()),
                None => return Err(format!("{here}: `os` must be one of {}", HOST_OSES.join(", "))),
            },
        };
        if key == "only" && os.is_none() {
            return Err(format!("{here}: `only` needs an `os` ({})", HOST_OSES.join(", ")));
        }
        out.push(Exception {
            name: name.to_string(),
            modules,
            os,
            reason: reason.to_string(),
        });
    }
    Ok(out)
}

impl Exceptions {
    /// The declared exceptions of a parsed contract: `Ok(None)` when it has no `hostTests` (the workspace is not scoped), an error
    /// naming `what` (the contract's path) when the field is malformed (a typo must never silently turn the scope off).
    pub fn parse(contract: &Value, what: &str) -> Result<Option<Exceptions>, String> {
        let Some(v) = contract.get("hostTests") else {
            return Ok(None);
        };
        let Some(obj) = v.as_object() else {
            return Err(format!("{what}: `hostTests` must be an object"));
        };
        let mut out = Exceptions::default();
        for (k, val) in obj {
            let at = format!("{what}: `hostTests.{k}`");
            match k.as_str() {
                "add" => out.add = entries(val, &at, "add")?,
                "skip" => out.skip = entries(val, &at, "skip")?,
                "only" => out.only = entries(val, &at, "only")?,
                other => {
                    return Err(format!(
                        "{what}: unknown `hostTests.{other}` (the placement is derived from the code; known: add, skip, only)"
                    ));
                }
            }
        }
        Ok(Some(out))
    }

    /// The resolved placement: the derived one with the exceptions applied, as the object [`HostTests::parse`] reads.
    pub fn resolved(&self, derived: &Derived) -> Value {
        let by_module = !derived.known_modules.is_empty();
        let mut common: BTreeSet<String> = if by_module {
            derived.modules.clone()
        } else {
            derived.packages.clone()
        };
        // `skip` without modules takes the whole entry off the hosts; `only` takes it off the common list and puts it on its host.
        // A prefix entry takes everything below it (`pre_push::` covers `pre_push::exec::`), as it does in a nextest filterset.
        let take_off = |common: &mut BTreeSet<String>, name: &str| {
            if is_prefix(name) {
                common.retain(|c| !c.starts_with(name));
            } else {
                common.remove(name);
            }
        };
        for s in self.skip.iter().filter(|s| s.modules.is_empty()) {
            take_off(&mut common, &s.name);
        }
        let mut only: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for o in &self.only {
            take_off(&mut common, &o.name);
            if let Some(os) = &o.os {
                only.entry(os.clone()).or_default().push(o.name.clone());
            }
        }
        // `add` comes last: it puts a name back that a wider `skip` or `only` prefix took off (a module the host code calls inside a
        // dev-Mac folder), and adds what no signal marks.
        for a in &self.add {
            common.insert(a.name.clone());
        }
        let skip: BTreeMap<String, Vec<String>> = self
            .skip
            .iter()
            .filter(|s| !s.modules.is_empty())
            .map(|s| (s.name.clone(), s.modules.clone()))
            .collect();
        let mut out = serde_json::Map::new();
        out.insert(if by_module { "modules" } else { "packages" }.into(), json!(common));
        if !skip.is_empty() {
            out.insert("skip".into(), json!(skip));
        }
        if !only.is_empty() {
            out.insert("only".into(), json!(only));
        }
        Value::Object(out)
    }
}

/// Whether the resolved placement `resolved` (an [`Exceptions::resolved`] result) runs `name` (a package, or a test-name prefix) on every
/// host: the name is listed, or a listed prefix is above it.
pub fn resolved_covers(resolved: &Value, name: &str) -> bool {
    ["packages", "modules"]
        .iter()
        .filter_map(|k| resolved.get(k)?.as_array())
        .flatten()
        .filter_map(Value::as_str)
        .any(|l| l == name || (is_prefix(l) && name.starts_with(l)))
}

/// The `hostTests` of a contract as the planner uses them: the placement derived from the code with the declared exceptions applied.
/// `Ok(None)` when the contract declares no `hostTests` (not scoped: every test everywhere) or when nothing is marked and nothing is
/// added (a host leg with nothing to test would be read as "everything"; the Linux leg already tests it).
pub fn read(contract: &Value, what: &str, derived: &Derived) -> Result<Option<HostTests>, String> {
    let Some(ex) = Exceptions::parse(contract, what)? else {
        return Ok(None);
    };
    // The packages the contract keeps off the host legs by an older decision (`lastPackages`: a Tauri shell whose build overwrites a
    // binary the other tests start) are not host packages, whatever the code says.
    let last: BTreeSet<String> = contract
        .get("lastPackages")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let mut derived = derived.clone();
    derived.packages.retain(|p| !last.contains(p));
    let resolved = ex.resolved(&derived);
    let empty = resolved.as_object().is_some_and(|o| {
        o.values()
            .all(|v| v.as_array().is_some_and(Vec::is_empty) || v.as_object().is_some_and(serde_json::Map::is_empty))
    });
    if empty {
        return Ok(None);
    }
    HostTests::parse(&json!({ "hostTests": resolved }), what)
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

    fn derived(packages: &[&str], modules: &[&str]) -> Derived {
        Derived {
            packages: packages.iter().map(|s| s.to_string()).collect(),
            modules: modules.iter().map(|s| s.to_string()).collect(),
            known_packages: packages.iter().map(|s| s.to_string()).collect(),
            known_modules: modules.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn exceptions_are_read_with_their_reasons_and_a_typo_or_a_missing_reason_is_refused() {
        assert_eq!(Exceptions::parse(&json!({}), "x.json").unwrap(), None);
        assert_eq!(
            Exceptions::parse(&json!({"hostTests": {}}), "x.json").unwrap(),
            Some(Exceptions::default())
        );
        let r = "a reason that is long enough";
        for (bad, why) in [
            (json!([]), "must be an object"),
            (json!({"packages": ["a"]}), "unknown `hostTests.packages`"),
            (json!({"add": "a"}), "array of {name, reason} objects"),
            (json!({"add": ["a"]}), "must be an object"),
            (json!({"add": [{"name": "a"}]}), "needs a `reason`"),
            (json!({"add": [{"name": "a", "reason": "short"}]}), "at least 8 characters"),
            (json!({"add": [{"name": "", "reason": r}]}), "`name` must be a non-empty string"),
            (json!({"add": [{"name": "a b::", "reason": r}]}), "not a valid test-name prefix"),
            (json!({"add": [{"name": "a", "reason": r, "os": "macos-latest"}]}), "unknown `os`"),
            (json!({"skip": [{"name": "a", "modules": ["m"], "reason": r}]}), "ending in `::`"),
            (json!({"only": [{"name": "a", "reason": r}]}), "`only` needs an `os`"),
            (
                json!({"only": [{"os": "ubuntu-latest", "name": "a", "reason": r}]}),
                "`os` must be one of",
            ),
        ] {
            let e = Exceptions::parse(&json!({ "hostTests": bad }), "x.json").unwrap_err();
            assert!(e.contains(why) && e.starts_with("x.json"), "{bad}: {e}");
        }
    }

    #[test]
    fn the_derived_placement_with_the_exceptions_applied_is_what_the_hosts_test() {
        let r = "a reason that is long enough";
        let d = derived(&["a", "b", "c"], &[]);
        // Derived alone: every marked package on every host.
        let plain = read(&json!({"hostTests": {}}), "x.json", &d).unwrap().unwrap();
        assert_eq!(plain.scope("windows-latest").packages, ["a", "b", "c"]);
        assert_eq!(plain.scope(LINUX), HostScope::default());
        // `add` is free, `skip` takes a package off both hosts, `only` leaves one host, a module `skip` stays a filter.
        let h = read(
            &json!({"hostTests": {
                "add": [{"name": "z", "reason": r}],
                "skip": [{"name": "b", "reason": r}, {"name": "a", "modules": ["pure::"], "reason": r}],
                "only": [{"os": "macos-latest", "name": "c", "reason": r}]
            }}),
            "x.json",
            &d,
        )
        .unwrap()
        .unwrap();
        assert_eq!(h.scope("windows-latest").packages, ["a", "z"]);
        assert_eq!(h.scope("macos-latest").packages, ["a", "c", "z"]);
        assert_eq!(h.scope("windows-latest").filter, "not ((package(=a) & test(/^(?:pure::)/)))");
        // Positive control: without the exceptions the same code gives the plain placement again.
        assert_eq!(plain.scope("macos-latest").packages, ["a", "b", "c"]);
    }

    #[test]
    fn a_prefix_exception_takes_everything_below_it_and_an_add_puts_a_module_back() {
        let r = "a reason that is long enough";
        let d = derived(&[], &["pre::", "pre::exec::", "pre::kept::", "other::"]);
        let h = read(
            &json!({"hostTests": {
                "add": [{"name": "pre::exec::", "reason": r}],
                "only": [{"os": "macos-latest", "name": "pre::", "reason": r}]
            }}),
            "x.json",
            &d,
        )
        .unwrap()
        .unwrap();
        assert_eq!(h.module_prefixes("windows-latest"), ["other::", "pre::exec::"]);
        assert_eq!(h.module_prefixes("macos-latest"), ["other::", "pre::", "pre::exec::"]);
    }

    #[test]
    fn a_workspace_with_nothing_marked_and_nothing_declared_is_not_scoped_and_last_packages_stay_off_the_hosts() {
        assert_eq!(read(&json!({"hostTests": {}}), "x.json", &derived(&[], &[])).unwrap(), None);
        assert_eq!(
            read(&json!({}), "x.json", &derived(&["a"], &[])).unwrap(),
            None,
            "no field, not scoped"
        );
        let h = read(
            &json!({"hostTests": {}, "lastPackages": ["shell"]}),
            "x.json",
            &derived(&["a", "shell"], &[]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(h.scope("windows-latest").packages, ["a"], "the shell is tested last on Linux");
        assert!(resolved_covers(&json!({"modules": ["a::"]}), "a::b::") && !resolved_covers(&json!({"modules": ["a::"]}), "b::"));
        assert!(resolved_covers(&json!({"packages": ["p"]}), "p") && !resolved_covers(&json!({"packages": ["p"]}), "q"));
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
