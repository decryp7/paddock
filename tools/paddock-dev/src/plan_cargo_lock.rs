//! The workspace packages a `Cargo.lock` change touched (decision 95, user 2026-11-26; ER-12).
//!
//! A lockfile is no source of any package: a change to it reaches a package only when the resolved dependencies of that package
//! changed. This reads the lock before and after with the TOML parser (ER-42) and names the workspace packages (the entries with
//! no `source`: path packages) whose dependency closure differs: a dependency whose version, source or checksum moved, was added
//! or was removed anywhere below the package, or a package whose own entry changed. A bump of a crate only another package uses
//! touches that package and its dependents (the planner adds them), never the rest of the workspace.
//!
//! Fail closed: a lock that is missing on either side or cannot be read gives `None`, and the planner then treats the lockfile as a
//! change that reaches the whole workspace, as before.

use std::collections::{BTreeMap, BTreeSet};

/// A package's key in the lock: `(name, version)`, the pair a dependency line can always be resolved to.
type Key = (String, String);

/// One `[[package]]` entry as the closure compares it: everything the lock records about it, dependencies included.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    source: Option<String>,
    checksum: Option<String>,
    deps: Vec<String>,
}

/// The entries of a lock text, `None` when the text is not a lockfile this reads.
fn entries(text: &str) -> Option<BTreeMap<Key, Entry>> {
    let table: toml::Table = text.parse().ok()?;
    let mut out = BTreeMap::new();
    for p in table.get("package")?.as_array()? {
        let get = |k: &str| p.get(k).and_then(toml::Value::as_str).map(str::to_string);
        let (name, version) = (get("name")?, get("version")?);
        let deps = p
            .get("dependencies")
            .and_then(toml::Value::as_array)
            .map(|a| a.iter().filter_map(|d| d.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        out.insert(
            (name, version),
            Entry {
                source: get("source"),
                checksum: get("checksum"),
                deps,
            },
        );
    }
    Some(out)
}

/// The key a dependency line (`name`, `name version` or `name version (source)`) names; a bare name resolves when the lock holds one
/// version of it, as Cargo writes it. `None` when it names nothing in the lock.
fn resolve(all: &BTreeMap<Key, Entry>, line: &str) -> Option<Key> {
    let mut words = line.split_whitespace();
    let name = words.next()?;
    match words.next() {
        Some(version) => {
            let key = (name.to_string(), version.to_string());
            all.contains_key(&key).then_some(key)
        }
        None => {
            let mut found = all.keys().filter(|(n, _)| n == name);
            let first = found.next()?;
            found.next().is_none().then(|| first.clone())
        }
    }
}

/// Every entry reachable from `start` (itself included), as the closure of one package. A dependency line that names nothing in the
/// lock is kept as a marker, so a lock that gains or loses it differs.
fn closure(all: &BTreeMap<Key, Entry>, start: &Key) -> BTreeMap<Key, Entry> {
    let mut seen: BTreeMap<Key, Entry> = BTreeMap::new();
    let mut todo = vec![start.clone()];
    while let Some(key) = todo.pop() {
        let Some(entry) = all.get(&key) else { continue };
        if seen.insert(key, entry.clone()).is_some() {
            continue;
        }
        for line in &entry.deps {
            match resolve(all, line) {
                Some(next) => todo.push(next),
                None => {
                    let marker = Entry {
                        source: None,
                        checksum: None,
                        deps: Vec::new(),
                    };
                    seen.insert((format!("unresolved {line}"), String::new()), marker);
                }
            }
        }
    }
    seen
}

/// The names of the workspace packages whose dependency closure differs between the lock texts `before` and `after`, sorted;
/// `None` when either side is missing or unreadable (fail closed: the caller treats the whole workspace as reached).
pub fn changed_members(before: Option<&str>, after: Option<&str>) -> Option<Vec<String>> {
    let (a, b) = (entries(before?)?, entries(after?)?);
    let members: BTreeSet<&Key> = a
        .iter()
        .chain(b.iter())
        .filter(|(_, e)| e.source.is_none())
        .map(|(k, _)| k)
        .collect();
    let changed: BTreeSet<String> = members
        .into_iter()
        .filter(|key| closure(&a, key) != closure(&b, key))
        .map(|key| key.0.clone())
        .collect();
    Some(changed.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A workspace of `app` (uses serde and the library `lib`, which uses log) and `solo` (uses nothing).
    fn lock(serde: &str, log: &str, extra: &str) -> String {
        format!(
            r#"version = 4

[[package]]
name = "app"
version = "0.1.0"
dependencies = ["lib", "serde"]

[[package]]
name = "lib"
version = "0.1.0"
dependencies = ["log"]

[[package]]
name = "solo"
version = "0.1.0"

[[package]]
name = "log"
version = "{log}"
source = "registry+https://example.invalid/index"
checksum = "log-{log}"

[[package]]
name = "serde"
version = "{serde}"
source = "registry+https://example.invalid/index"
checksum = "serde-{serde}"
{extra}"#
        )
    }

    fn changed(a: &str, b: &str) -> Vec<String> {
        changed_members(Some(a), Some(b)).expect("readable locks")
    }

    #[test]
    fn the_same_lock_touches_no_package() {
        let a = lock("1.0.0", "0.4.0", "");
        assert!(changed(&a, &a).is_empty());
    }

    #[test]
    fn a_bump_touches_the_packages_that_reach_the_dependency_and_no_other() {
        let base = lock("1.0.0", "0.4.0", "");
        // serde is used by app alone.
        assert_eq!(changed(&base, &lock("1.0.1", "0.4.0", "")), ["app"]);
        // log is used by lib, which app uses: both reach it; solo never does.
        assert_eq!(changed(&base, &lock("1.0.0", "0.4.1", "")), ["app", "lib"]);
    }

    #[test]
    fn an_added_dependency_edge_and_a_changed_checksum_touch_the_package() {
        let base = lock("1.0.0", "0.4.0", "");
        let solo = "name = \"solo\"\nversion = \"0.1.0\"\n";
        let with_edge = base.replace(solo, &format!("{solo}dependencies = [\"serde\"]\n"));
        assert_ne!(with_edge, base);
        assert_eq!(changed(&base, &with_edge), ["solo"]);
        assert_eq!(changed(&with_edge, &base), ["solo"]);
        let sum = base.replace("checksum = \"serde-1.0.0\"", "checksum = \"other\"");
        assert_ne!(sum, base);
        assert_eq!(changed(&base, &sum), ["app"]);
    }

    #[test]
    fn a_package_added_or_removed_with_the_lock_is_changed() {
        let base = lock("1.0.0", "0.4.0", "");
        let more = lock("1.0.0", "0.4.0", "\n[[package]]\nname = \"newcomer\"\nversion = \"0.1.0\"\n");
        assert_eq!(changed(&base, &more), ["newcomer"]);
        assert_eq!(changed(&more, &base), ["newcomer"]);
    }

    #[test]
    fn a_missing_or_unreadable_lock_is_not_measured_and_fails_closed() {
        let a = lock("1.0.0", "0.4.0", "");
        assert_eq!(changed_members(None, Some(&a)), None);
        assert_eq!(changed_members(Some(&a), None), None);
        assert_eq!(changed_members(Some(&a), Some("not = [toml")), None);
        assert_eq!(changed_members(Some(&a), Some("version = 4\n")), None);
    }

    #[test]
    fn two_versions_of_one_crate_resolve_by_their_version_and_a_dangling_edge_differs() {
        let two = |v: &str| {
            format!(
                "version = 4\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\"dup {v}\"]\n\
                 [[package]]\nname = \"dup\"\nversion = \"1.0.0\"\nsource = \"s\"\nchecksum = \"1\"\n\
                 [[package]]\nname = \"dup\"\nversion = \"2.0.0\"\nsource = \"s\"\nchecksum = \"2\"\n"
            )
        };
        assert_eq!(changed(&two("1.0.0"), &two("2.0.0")), ["app"]);
        assert!(changed(&two("1.0.0"), &two("1.0.0")).is_empty());
        // `dup 3.0.0` names nothing: the closure holds a marker, so the package differs from one that resolves.
        assert_eq!(changed(&two("1.0.0"), &two("3.0.0")), ["app"]);
    }
}
