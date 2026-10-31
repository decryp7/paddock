//! The packages that ship: Rust crates from cargo-about's JSON and the production npm packages from the root `package-lock.json`
//! with their licence files. Pure functions over parsed JSON, plus the bounded file reads for npm. Ports `generate.mjs`.

use super::error::NoticesError;
use super::text::{cmp16, normalise, spdx_ids};
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Largest `package-lock.json` read.
pub const MAX_LOCK_BYTES: u64 = 64 << 20;
/// Largest one licence file or `package.json` read.
pub const MAX_TEXT_BYTES: u64 = 1 << 20;

/// The ecosystem of a package. The order (npm before rust) is the order of the notices file's sorts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// An npm package.
    Npm,
    /// A Rust crate.
    Rust,
}

impl Kind {
    /// The name used in messages and in the sort.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Npm => "npm",
            Kind::Rust => "rust",
        }
    }
}

/// One licence text of a package and the licence id it is known by (None when the file name does not say).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    /// The licence id, when known.
    pub id: Option<String>,
    /// The normalised text.
    pub text: String,
}

/// A package that ships.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// The ecosystem.
    pub kind: Kind,
    /// The name.
    pub name: String,
    /// The version.
    pub version: String,
    /// The declared licence expression (`UNKNOWN` when none).
    pub license: String,
    /// The licence texts found.
    pub texts: Vec<Text>,
    /// The identifiers of the expression.
    pub spdx: Vec<String>,
}

/// JavaScript truthiness of a lock flag.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(_) => true,
    }
}

/// `a ?? b` for optional JSON: null counts as absent.
fn present(v: Option<&Value>) -> Option<&Value> {
    v.filter(|v| !v.is_null())
}

/// The licence expression a package.json or lock entry declares: a string, `{ type }`, or the legacy `licenses: [{ type }]` list
/// (several entries are alternatives, so OR). None when nothing usable is declared.
pub fn declared_license(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(super::text::js_trim(s)).filter(|s| !s.is_empty()).map(String::from),
        Value::Array(items) => {
            let all: Vec<String> = items.iter().filter_map(declared_license).collect();
            (!all.is_empty()).then(|| all.join(" OR "))
        }
        Value::Object(o) => o.get("type").and_then(declared_license),
        _ => None,
    }
}

/// A lock entry that ships.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockEntry {
    /// The lock key (`node_modules/a/node_modules/b`).
    pub path: String,
    /// The package name.
    pub name: String,
    /// The version.
    pub version: String,
    /// The licence the lock states, if any.
    pub license: Option<String>,
}

fn name_of(path: &str) -> String {
    path.rfind("node_modules/")
        .map_or(path, |i| &path[i + "node_modules/".len()..])
        .to_string()
}

fn version_of(entry: &Value) -> String {
    entry.get("version").and_then(Value::as_str).unwrap_or("0.0.0").to_string()
}

fn packages_of(lock: &Value) -> Vec<(&String, &Value)> {
    lock.get("packages")
        .and_then(Value::as_object)
        .map(|o| o.iter().collect())
        .unwrap_or_default()
}

/// Entries of a package-lock that ship in production (not dev-only), in lock order.
pub fn lock_production(lock: &Value) -> Vec<LockEntry> {
    packages_of(lock)
        .into_iter()
        .filter(|(path, p)| path.contains("node_modules/") && !truthy(p.get("dev")) && !truthy(p.get("devOptional")))
        .map(|(path, p)| LockEntry {
            path: path.clone(),
            name: name_of(path),
            version: version_of(p),
            license: present(p.get("license")).or_else(|| p.get("licenses")).and_then(declared_license),
        })
        .collect()
}

fn dep_names(entry: &Value, key: &str) -> Vec<String> {
    entry
        .get(key)
        .and_then(Value::as_object)
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}

/// The production closure of one workspace of a root npm workspace lock (Paddock's monorepo, ADR 0014 §1): its dependencies,
/// optional and peer dependencies and theirs, resolved in node's lookup order (nested first, then up to the root's node_modules).
/// The root lock's dev flags describe all workspaces together, so they cannot be used here.
pub fn workspace_production(lock: &Value, ws: &str) -> Result<Vec<LockEntry>, NoticesError> {
    let pk: HashMap<&str, &Value> = packages_of(lock).into_iter().map(|(k, v)| (k.as_str(), v)).collect();
    let entry = *pk.get(ws).ok_or_else(|| NoticesError::NoWorkspace(ws.to_string()))?;
    let resolve_from = |at: &str, name: &str| -> Option<String> {
        let mut base = at.to_string();
        loop {
            let cand = if base.is_empty() {
                format!("node_modules/{name}")
            } else {
                format!("{base}/node_modules/{name}")
            };
            if let Some(found) = pk.get(cand.as_str()) {
                return if truthy(found.get("link")) { None } else { Some(cand) };
            }
            if base.is_empty() {
                return None;
            }
            base = base.rfind("/node_modules/").map_or(String::new(), |i| base[..i].to_string());
        }
    };
    // A peer the workspace itself declares only as a devDependency (e.g. @angular/compiler-cli, a peer of @angular/localize) is a
    // build tool here, not shipped.
    let own: HashSet<String> = dep_names(entry, "dependencies")
        .into_iter()
        .chain(dep_names(entry, "optionalDependencies"))
        .collect();
    let dev_only: HashSet<String> = dep_names(entry, "devDependencies")
        .into_iter()
        .filter(|n| !own.contains(n))
        .collect();
    let deps = |e: &Value| -> Vec<String> {
        let mut all = dep_names(e, "dependencies");
        all.extend(dep_names(e, "optionalDependencies"));
        all.extend(dep_names(e, "peerDependencies").into_iter().filter(|n| {
            let optional = e
                .get("peerDependenciesMeta")
                .and_then(|m| m.get(n.as_str()))
                .and_then(|m| m.get("optional"));
            !truthy(optional) && !dev_only.contains(n)
        }));
        all
    };
    let mut seen: HashSet<String> = HashSet::new();
    let mut todo: Vec<Option<String>> = deps(entry).iter().map(|n| resolve_from(ws, n)).collect();
    while let Some(next) = todo.pop() {
        let Some(at) = next else { continue };
        if !seen.insert(at.clone()) {
            continue;
        }
        if let Some(e) = pk.get(at.as_str()) {
            todo.extend(deps(e).iter().map(|n| resolve_from(&at, n)));
        }
    }
    let mut paths: Vec<String> = seen.into_iter().collect();
    paths.sort_by(|a, b| cmp16(a, b));
    Ok(paths
        .into_iter()
        .map(|path| {
            let e = pk.get(path.as_str()).copied();
            LockEntry {
                name: name_of(&path),
                version: e.map(version_of).unwrap_or_default(),
                license: e.and_then(|e| e.get("license")).and_then(Value::as_str).map(String::from),
                path,
            }
        })
        .collect())
}

/// The folder with the package-lock.json that covers `dir`: `dir` itself, or the npm workspace root above it, looking up as far
/// as `stop` (the repository root; not above it when `dir` is inside it). None if none.
pub fn lock_root(dir: &Path, stop: &Path) -> Option<PathBuf> {
    let bounded = dir.starts_with(stop);
    let mut d = dir;
    loop {
        if d.join("package-lock.json").is_file() {
            return Some(d.to_path_buf());
        }
        if bounded && d == stop {
            return None;
        }
        d = d.parent()?;
    }
}

/// Reads a file as lossy UTF-8, refusing one over `max` bytes.
pub fn read_text(path: &Path, max: u64) -> Result<String, NoticesError> {
    let fail = |why: String| NoticesError::Read {
        path: path.display().to_string(),
        why,
    };
    let file = fs::File::open(path).map_err(|e| fail(e.to_string()))?;
    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes).map_err(|e| fail(e.to_string()))?;
    if bytes.len() as u64 > max {
        return Err(fail(format!("it is larger than {max} bytes")));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

static LICENCE_FILE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(licen[cs]e|licen[cs]ing|copying|notice|unlicense)([-_.].*)?$").unwrap_or_else(|e| panic!("static pattern: {e}"))
});
static CODE_FILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.(js|ts|json|css)$").unwrap_or_else(|e| panic!("static pattern: {e}")));

/// The licence id a licence file's name says.
fn text_id(file: &str) -> Option<&'static str> {
    let lower = file.to_lowercase();
    if lower.contains("apache") {
        Some("Apache-2.0")
    } else if lower.contains("mit") {
        Some("MIT")
    } else if lower.starts_with("notice") {
        Some("NOTICE")
    } else {
        None
    }
}

/// The production npm packages of an app folder (itself an npm root, or a workspace of the lock above it, searched up to `stop`)
/// with their licence texts: (packages, warnings). Needs `npm ci --omit=dev` (or a full `npm ci`) first for the texts.
pub fn npm_packages(app_dir: &Path, stop: &Path) -> Result<(Vec<Package>, Vec<String>), NoticesError> {
    let root = lock_root(app_dir, stop).ok_or_else(|| NoticesError::NoLock(app_dir.join("package-lock.json").display().to_string()))?;
    let lock_path = root.join("package-lock.json");
    let lock: Value = serde_json::from_str(&read_text(&lock_path, MAX_LOCK_BYTES)?).map_err(|e| NoticesError::Read {
        path: lock_path.display().to_string(),
        why: format!("it is not valid JSON: {e}"),
    })?;
    let ws = app_dir
        .strip_prefix(&root)
        .map(|r| {
            r.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    let list = if ws.is_empty() {
        lock_production(&lock)
    } else {
        workspace_production(&lock, &ws)?
    };
    let mut warnings = Vec::new();
    let mut seen = HashSet::new();
    let mut packages = Vec::new();
    for p in list {
        if !seen.insert(format!("{}@{}", p.name, p.version)) {
            continue;
        }
        let dir = root.join(&p.path);
        let manifest = dir.join("package.json");
        let pkg_json = if manifest.is_file() {
            match read_text(&manifest, MAX_TEXT_BYTES).and_then(|t| {
                serde_json::from_str::<Value>(&t).map_err(|e| NoticesError::Read {
                    path: String::new(),
                    why: e.to_string(),
                })
            }) {
                Ok(v) => v,
                Err(_) => {
                    warnings.push(format!("npm {}@{}: package.json is not valid JSON", p.name, p.version));
                    Value::Null
                }
            }
        } else {
            Value::Null
        };
        let license = p
            .license
            .clone()
            .or_else(|| {
                present(pkg_json.get("license"))
                    .or_else(|| pkg_json.get("licenses"))
                    .and_then(declared_license)
            })
            .unwrap_or_else(|| "UNKNOWN".to_string());
        let mut files: Vec<String> = fs::read_dir(&dir)
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .filter(|e| e.path().is_file())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|f| LICENCE_FILE.is_match(f) && !CODE_FILE.is_match(f))
                    .collect()
            })
            .unwrap_or_default();
        files.sort_by(|a, b| cmp16(a, b));
        let mut texts = Vec::new();
        for f in &files {
            match read_text(&dir.join(f), MAX_TEXT_BYTES) {
                Ok(raw) => {
                    let text = normalise(&raw);
                    if !text.is_empty() {
                        texts.push(Text {
                            id: text_id(f).map(String::from),
                            text,
                        });
                    }
                }
                Err(NoticesError::Read { why, .. }) => warnings.push(format!("npm {}@{}: {f} cannot be read: {why}", p.name, p.version)),
                Err(other) => warnings.push(format!("npm {}@{}: {f}: {other}", p.name, p.version)),
            }
        }
        if texts.is_empty() {
            warnings.push(format!("npm {}@{}: no licence file found (declared {license})", p.name, p.version));
        }
        packages.push(Package {
            kind: Kind::Npm,
            name: p.name,
            version: p.version,
            spdx: spdx_ids(&license),
            license,
            texts,
        });
    }
    Ok((packages, warnings))
}

/// Rust crates from cargo-about JSON documents (workspace crates are the app itself and are skipped): (packages, warnings).
pub fn rust_crates(documents: &[Value]) -> (Vec<Package>, Vec<String>) {
    let mut crates: Vec<Package> = Vec::new();
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    for doc in documents {
        for c in doc.get("crates").and_then(Value::as_array).into_iter().flatten() {
            let Some(p) = c.get("package") else { continue };
            if !truthy(p.get("source")) {
                continue;
            }
            let (Some(name), Some(version)) = (p.get("name").and_then(Value::as_str), p.get("version").and_then(Value::as_str)) else {
                continue;
            };
            let key = (name.to_string(), version.to_string());
            if index.contains_key(&key) {
                continue;
            }
            let license = c
                .get("license")
                .and_then(Value::as_str)
                .filter(|l| !l.is_empty())
                .unwrap_or("UNKNOWN")
                .to_string();
            index.insert(key, crates.len());
            crates.push(Package {
                kind: Kind::Rust,
                name: name.into(),
                version: version.into(),
                spdx: spdx_ids(&license),
                license,
                texts: vec![],
            });
        }
        for l in doc.get("licenses").and_then(Value::as_array).into_iter().flatten() {
            let Some(text) = l.get("text").and_then(Value::as_str).map(normalise) else {
                continue;
            };
            let id = l.get("id").and_then(Value::as_str).map(String::from);
            for u in l.get("used_by").and_then(Value::as_array).into_iter().flatten() {
                let user = u.get("crate");
                let (Some(name), Some(version)) = (
                    user.and_then(|c| c.get("name")).and_then(Value::as_str),
                    user.and_then(|c| c.get("version")).and_then(Value::as_str),
                ) else {
                    continue;
                };
                if let Some(&i) = index.get(&(name.to_string(), version.to_string()))
                    && !crates[i].texts.iter().any(|t| t.text == text)
                {
                    crates[i].texts.push(Text {
                        id: id.clone(),
                        text: text.clone(),
                    });
                }
            }
        }
    }
    let warnings = crates
        .iter()
        .filter(|c| c.texts.is_empty())
        .map(|c| format!("rust {}@{}: no licence text found (declared {})", c.name, c.version, c.license))
        .collect();
    (crates, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn declared_licenses_come_from_strings_type_objects_and_legacy_lists() {
        assert_eq!(declared_license(&json!(" MIT ")), Some("MIT".into()));
        assert_eq!(declared_license(&json!({"type": "ISC"})), Some("ISC".into()));
        assert_eq!(
            declared_license(&json!([{"type": "MIT"}, "BSD-3-Clause", 7])),
            Some("MIT OR BSD-3-Clause".into())
        );
        assert_eq!(declared_license(&json!("  ")), None);
        assert_eq!(declared_license(&json!([])), None);
        assert_eq!(declared_license(&json!(7)), None);
    }

    #[test]
    fn lock_production_skips_dev_entries_and_keeps_lock_order() {
        let lock = json!({"packages": {"": {}, "node_modules/b": {"version": "1"}, "node_modules/a": {"dev": true},
            "node_modules/c": {"devOptional": true}, "node_modules/d/node_modules/e": {"version": "2", "licenses": [{"type": "MIT"}]}}});
        let names: Vec<_> = lock_production(&lock).into_iter().map(|e| (e.name, e.license)).collect();
        assert_eq!(names, [("b".to_string(), None), ("e".to_string(), Some("MIT".into()))]);
    }

    #[test]
    fn a_workspace_closure_resolves_nested_first_follows_peers_and_skips_links_dev_peers_and_missing() {
        let lock = json!({"packages": {
            "": {},
            "ws": {"dependencies": {"a": "1", "lnk": "1", "gone": "1"}, "peerDependencies": {"p": "1", "po": "1", "dp": "1"},
                   "peerDependenciesMeta": {"po": {"optional": true}}, "devDependencies": {"dp": "1", "t": "1"}},
            "node_modules/a": {"version": "1", "dependencies": {"n": "2"}},
            "node_modules/a/node_modules/n": {"version": "2", "license": "ISC"},
            "node_modules/n": {"version": "9", "dev": true},
            "node_modules/lnk": {"link": true},
            "node_modules/p": {"version": "3"}, "node_modules/po": {"version": "1"}, "node_modules/dp": {"version": "1"},
            "node_modules/t": {"version": "1"}}});
        let got: Vec<_> = workspace_production(&lock, "ws").unwrap().into_iter().map(|e| e.path).collect();
        assert_eq!(got, ["node_modules/a", "node_modules/a/node_modules/n", "node_modules/p"]);
        assert_eq!(workspace_production(&lock, "nope"), Err(NoticesError::NoWorkspace("nope".into())));
    }

    #[test]
    fn rust_crates_skip_workspace_crates_merge_documents_dedupe_texts_and_warn_about_missing_ones() {
        let doc = |text: &str| {
            json!({"crates": [{"package": {"name": "app", "version": "1", "source": null}, "license": "MIT"},
                {"package": {"name": "c", "version": "1", "source": "r"}, "license": "MIT"},
                {"package": {"name": "d", "version": "1", "source": "r"}}],
                "licenses": [{"id": "MIT", "text": text, "used_by": [{"crate": {"name": "c", "version": "1"}}, {"crate": {"name": "zz", "version": "1"}}, {"nope": 1}]}]})
        };
        let (crates, warnings) = rust_crates(&[doc("MIT\r\ntext  \r\n"), doc("MIT\ntext")]);
        assert_eq!(crates.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["c", "d"]);
        assert_eq!(
            crates[0].texts,
            [Text {
                id: Some("MIT".into()),
                text: "MIT\ntext".into()
            }]
        );
        assert_eq!(crates[1].license, "UNKNOWN");
        assert_eq!(warnings, ["rust d@1: no licence text found (declared UNKNOWN)"]);
    }

    #[test]
    fn hostile_documents_do_not_panic() {
        let (c, w) = rust_crates(&[
            json!(null),
            json!({"crates": 5, "licenses": [1, {"text": 3}]}),
            json!({"crates": [null, {"package": 1}]}),
        ]);
        assert!((c.is_empty(), w.is_empty()) == (true, true));
        assert!(lock_production(&json!([1])).is_empty());
        assert!(workspace_production(&json!({"packages": []}), "x").is_err());
    }

    #[test]
    fn licence_file_names_choose_the_text_id() {
        assert_eq!(text_id("LICENSE-APACHE"), Some("Apache-2.0"));
        assert_eq!(text_id("LICENSE-MIT"), Some("MIT"));
        assert_eq!(text_id("NOTICE.md"), Some("NOTICE"));
        assert_eq!(text_id("COPYING"), None);
        assert!(LICENCE_FILE.is_match("licensing.js") && CODE_FILE.is_match("licensing.js"));
        assert!(!LICENCE_FILE.is_match("my-license"));
    }
}
