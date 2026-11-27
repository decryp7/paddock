use crate::{FileSource, Graph, GraphError, Node, dirname, relative};
use serde_json::Value;

/// The port to `cargo metadata --no-deps` of one workspace. The adapter runs cargo; Pacenotes never does (cargo reads
/// the repository's `.cargo/config.toml`, which can name programs to run).
pub trait CargoMetadata {
    /// The JSON text for the workspace whose manifest is `manifest` (repository-relative), and the absolute repository
    /// root the paths in it start with.
    fn metadata(&self, manifest: &str) -> Result<String, GraphError>;
}

/// Adds the crates of one `cargo metadata --no-deps` JSON text. `root` is the absolute repository root and `workspace` the
/// workspace folder (repository-relative). Path dependencies (normal, build and dev) become edges.
pub fn parse_metadata(
    graph: &mut Graph,
    json: &str,
    root: &str,
    workspace: &str,
) -> Result<(), GraphError> {
    let manifest = if workspace.is_empty() {
        "Cargo.toml".to_string()
    } else {
        format!("{workspace}/Cargo.toml")
    };
    let bad = |reason: String| GraphError::Metadata {
        manifest: manifest.clone(),
        reason,
    };
    let v: Value = serde_json::from_str(json).map_err(|e| bad(e.to_string()))?;
    let packages = v
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("no \"packages\" list".to_string()))?;
    for p in packages {
        let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        let (Some(name), Some(manifest)) = (text(p, "name"), text(p, "manifest_path")) else {
            return Err(bad("a package without name or manifest_path".to_string()));
        };
        let dir = relative(root, &dirname(&manifest.replace('\\', "/")));
        let deps = p
            .get("dependencies")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|d| text(d, "path"))
            .map(|d| relative(root, &d))
            .collect();
        graph.insert(
            &dir,
            Node {
                name,
                workspace: workspace.to_string(),
                deps,
            },
        );
    }
    Ok(())
}

/// The files each package declares its tests read (`[package.metadata.paddock] test-reads`, ADR 0116 §2): package folder to the globs,
/// relative to the repository root, from one `cargo metadata --no-deps` JSON text. A package without the key is left out; a value that
/// is not a list of strings is an error (a declaration that cannot be read never counts as one).
pub fn parse_test_reads(
    json: &str,
    root: &str,
) -> Result<std::collections::BTreeMap<String, Vec<String>>, GraphError> {
    let bad = |reason: String| GraphError::Metadata {
        manifest: "cargo metadata".to_string(),
        reason,
    };
    let v: Value = serde_json::from_str(json).map_err(|e| bad(e.to_string()))?;
    let mut out = std::collections::BTreeMap::new();
    for p in v
        .get("packages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(declared) = p.pointer("/metadata/paddock/test-reads") else {
            continue;
        };
        let name = p.get("name").and_then(Value::as_str).unwrap_or("?");
        let globs: Option<Vec<String>> = declared
            .as_array()
            .and_then(|a| a.iter().map(|g| g.as_str().map(str::to_string)).collect());
        let Some(globs) = globs else {
            return Err(bad(format!(
                "package {name}: `[package.metadata.paddock] test-reads` is not a list of strings (globs relative to the repository root)"
            )));
        };
        let manifest = p
            .get("manifest_path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        out.insert(
            relative(root, &dirname(&manifest.replace('\\', "/"))),
            globs,
        );
    }
    Ok(out)
}

/// One Cargo target of `cargo metadata` (ADR 0117 §4 `tests`, review P2-5: test targets from `targets[].src_path` and `kind`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoTarget {
    /// The package name.
    pub package: String,
    /// The package folder (repository-relative).
    pub dir: String,
    /// The target name.
    pub name: String,
    /// Its kinds (`lib`, `bin`, `test`, `bench`, `example`, `proc-macro`, `custom-build`).
    pub kinds: Vec<String>,
    /// Its root source file (repository-relative).
    pub src_path: String,
    /// Whether `cargo test` runs its documentation examples (`doctest`, true by default for a library; the family sets it false,
    /// crates/README.md "Doctests").
    pub doctest: bool,
}

impl CargoTarget {
    /// The nextest binary id (`pkg` for a library's unit tests, `pkg::name` for an integration test, `pkg::bin/name` for a
    /// binary), or `None` for a target nextest does not run (a build script).
    pub fn binary_id(&self) -> Option<String> {
        let k = |x: &str| self.kinds.iter().any(|y| y == x);
        if k("custom-build") {
            None
        } else if k("lib")
            || k("rlib")
            || k("proc-macro")
            || k("cdylib")
            || k("staticlib")
            || k("dylib")
        {
            Some(self.package.clone())
        } else if k("test") {
            Some(format!("{}::{}", self.package, self.name))
        } else {
            let kind = self.kinds.first()?;
            Some(format!("{}::{kind}/{}", self.package, self.name))
        }
    }
}

/// The targets of one `cargo metadata --no-deps` JSON text, paths relative to `root`.
pub fn parse_targets(json: &str, root: &str) -> Result<Vec<CargoTarget>, GraphError> {
    let bad = |reason: String| GraphError::Metadata {
        manifest: "cargo metadata".to_string(),
        reason,
    };
    let v: Value = serde_json::from_str(json).map_err(|e| bad(e.to_string()))?;
    let mut out = Vec::new();
    for p in v
        .get("packages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        let (Some(package), Some(manifest)) = (text(p, "name"), text(p, "manifest_path")) else {
            return Err(bad("a package without name or manifest_path".to_string()));
        };
        let dir = relative(root, &dirname(&manifest.replace('\\', "/")));
        for t in p
            .get("targets")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let (Some(name), Some(src)) = (text(t, "name"), text(t, "src_path")) else {
                continue;
            };
            let kinds = t
                .get("kind")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
            out.push(CargoTarget {
                package: package.clone(),
                dir: dir.clone(),
                name,
                kinds,
                src_path: relative(root, &src.replace('\\', "/")),
                doctest: t.get("doctest").and_then(Value::as_bool).unwrap_or(false),
            });
        }
    }
    Ok(out)
}

/// A [`CargoMetadata`] that answers from texts recorded earlier (the planner reads every workspace once and the model reuses the
/// answers: one `cargo metadata` per workspace per run, ADR 0117 §9).
#[derive(Debug, Default, Clone)]
pub struct RecordedMetadata(pub std::collections::BTreeMap<String, String>);

impl CargoMetadata for RecordedMetadata {
    fn metadata(&self, manifest: &str) -> Result<String, GraphError> {
        self.0
            .get(manifest)
            .cloned()
            .ok_or_else(|| GraphError::Metadata {
                manifest: manifest.to_string(),
                reason: "not recorded in this run".to_string(),
            })
    }
}

/// A [`CargoMetadata`] that records every answer of another (to replay it with [`RecordedMetadata`]).
pub struct Recording<'a> {
    inner: &'a dyn CargoMetadata,
    /// The answers so far, by manifest.
    pub texts: std::cell::RefCell<std::collections::BTreeMap<String, String>>,
}

impl<'a> Recording<'a> {
    /// Wraps a port.
    pub fn new(inner: &'a dyn CargoMetadata) -> Recording<'a> {
        Recording {
            inner,
            texts: std::cell::RefCell::new(std::collections::BTreeMap::new()),
        }
    }
}

impl CargoMetadata for Recording<'_> {
    fn metadata(&self, manifest: &str) -> Result<String, GraphError> {
        let t = self.inner.metadata(manifest)?;
        self.texts
            .borrow_mut()
            .insert(manifest.to_string(), t.clone());
        Ok(t)
    }
}

/// The Cargo graph of every workspace among `files`: a `Cargo.toml` with a `[workspace]` line is a root.
pub fn cargo_graph(
    files: &[String],
    source: &dyn FileSource,
    metadata: &dyn CargoMetadata,
    root: &str,
) -> Result<Graph, GraphError> {
    let mut g = Graph::new();
    for m in files.iter().filter(|f| f.ends_with("Cargo.toml")) {
        if !source
            .read_to_string(m)?
            .lines()
            .any(|l| l == "[workspace]")
        {
            continue;
        }
        let json = metadata.metadata(m)?;
        let dir = dirname(m);
        parse_metadata(&mut g, &json, root, if dir == "." { "" } else { &dir })?;
    }
    Ok(g)
}

/// `cargo metadata --no-deps --offline` through `paddock-proc`, run inside the workspace folder (cargo reads
/// `.cargo/config.toml` from the current folder upwards).
#[cfg(feature = "cargo-metadata")]
pub struct CargoCli {
    root: std::path::PathBuf,
    timeout: std::time::Duration,
    program: std::ffi::OsString,
}

#[cfg(feature = "cargo-metadata")]
impl CargoCli {
    /// For the repository at `root`, each call bounded by `timeout`.
    pub fn new(root: impl Into<std::path::PathBuf>, timeout: std::time::Duration) -> CargoCli {
        CargoCli {
            root: root.into(),
            timeout,
            program: "cargo".into(),
        }
    }

    /// The same with another `cargo` program (a full path, or a name that is not there, for tests).
    pub fn with_program(mut self, program: impl Into<std::ffi::OsString>) -> CargoCli {
        self.program = program.into();
        self
    }
}

#[cfg(feature = "cargo-metadata")]
impl CargoMetadata for CargoCli {
    fn metadata(&self, manifest: &str) -> Result<String, GraphError> {
        let path = self.root.join(manifest);
        let fail = |reason: String| GraphError::Metadata {
            manifest: manifest.to_string(),
            reason,
        };
        let mut cmd = paddock_proc::command(&self.program);
        cmd.args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
            "--manifest-path",
        ])
        .arg(&path);
        cmd.current_dir(path.parent().unwrap_or(&self.root));
        let out = paddock_proc::run_bounded(
            cmd,
            paddock_proc::Limits::new(self.timeout).max_output(1 << 28),
        )
        .map_err(|e| fail(e.to_string()))?;
        if !out.success() {
            let err = out.stderr_text();
            let tail: Vec<&str> = err.trim().lines().rev().take(3).collect();
            return Err(fail(tail.into_iter().rev().collect::<Vec<_>>().join(" ")));
        }
        Ok(out.stdout_text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryFiles;

    struct Canned(Vec<(&'static str, String)>);

    impl CargoMetadata for Canned {
        fn metadata(&self, manifest: &str) -> Result<String, GraphError> {
            self.0
                .iter()
                .find(|(m, _)| *m == manifest)
                .map(|(_, j)| j.clone())
                .ok_or_else(|| GraphError::Metadata {
                    manifest: manifest.into(),
                    reason: "no".into(),
                })
        }
    }

    fn meta(pkgs: &[(&str, &str, &[&str])]) -> String {
        let list: Vec<String> = pkgs
            .iter()
            .map(|(n, dir, deps)| {
                let mut deps: Vec<String> = deps
                    .iter()
                    .map(|d| format!(r#"{{"name":"x","path":"{d}"}}"#))
                    .collect();
                deps.insert(0, r#"{"name":"serde"}"#.to_string());
                format!(
                    r#"{{"name":"{n}","manifest_path":"{dir}/Cargo.toml","dependencies":[{}]}}"#,
                    deps.join(",")
                )
            })
            .collect();
        format!(r#"{{"packages":[{}]}}"#, list.join(","))
    }

    #[test]
    fn test_reads_come_from_the_package_metadata_and_a_bad_value_is_refused() {
        let json = |metadata: &str| {
            format!(
                r#"{{"packages":[{{"name":"a","manifest_path":"/r/crates/a/Cargo.toml","metadata":{metadata}}},{{"name":"b","manifest_path":"/r/crates/b/Cargo.toml","metadata":null}}]}}"#
            )
        };
        let got = parse_test_reads(
            &json(r#"{"paddock":{"test-reads":["docs/x.toml","crates/a/tests/**"]}}"#),
            "/r",
        )
        .unwrap();
        assert_eq!(
            got.len(),
            1,
            "a package without the key is left out: {got:?}"
        );
        assert_eq!(
            got["crates/a"],
            vec!["docs/x.toml".to_string(), "crates/a/tests/**".to_string()]
        );
        // A declaration that cannot be read never counts as one.
        for bad in [
            r#"{"paddock":{"test-reads":"docs/x"}}"#,
            r#"{"paddock":{"test-reads":[1]}}"#,
        ] {
            let e = parse_test_reads(&json(bad), "/r").unwrap_err().to_string();
            assert!(e.contains("test-reads") && e.contains("package a"), "{e}");
        }
        // Control: other metadata (another tool's table) declares nothing.
        assert!(
            parse_test_reads(&json(r#"{"docs":{"rs":{}}}"#), "/r")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn workspaces_become_crates_with_path_edges() {
        let m = MemoryFiles::new()
            .with("crates/Cargo.toml", "[workspace]\nmembers=[]\n")
            .with("crates/a/Cargo.toml", "[package]\nname=\"a\"\n")
            .with("apps/x/app/src-tauri/Cargo.toml", "[workspace]\n")
            .with("Cargo.toml", "[package]\n");
        let canned = Canned(vec![
            (
                "crates/Cargo.toml",
                meta(&[
                    ("a", "/r/crates/a", &[]),
                    ("b", "/r/crates/b", &["/r/crates/a"]),
                ]),
            ),
            (
                "apps/x/app/src-tauri/Cargo.toml",
                meta(&[("x", "/r/apps/x/app/src-tauri", &["/r/crates/b"])]),
            ),
        ]);
        let g = cargo_graph(&m.paths(), &m, &canned, "/r").unwrap();
        assert_eq!(g.get("crates/b").unwrap().deps, ["crates/a"]);
        assert_eq!(g.get("crates/b").unwrap().workspace, "crates");
        assert_eq!(g.get("apps/x/app/src-tauri").unwrap().deps, ["crates/b"]);
        assert_eq!(g.iter().count(), 3);
    }

    #[test]
    fn a_root_workspace_has_an_empty_workspace_folder() {
        let m = MemoryFiles::new().with("Cargo.toml", "[workspace]\n");
        let canned = Canned(vec![("Cargo.toml", meta(&[("r", "/r", &[])]))]);
        assert_eq!(
            cargo_graph(&m.paths(), &m, &canned, "/r")
                .unwrap()
                .get("")
                .unwrap()
                .workspace,
            ""
        );
    }

    #[test]
    fn broken_metadata_is_a_typed_error() {
        let mut g = Graph::new();
        for bad in ["{", r#"{"x":1}"#, r#"{"packages":[{"name":"a"}]}"#] {
            let err = parse_metadata(&mut g, bad, "/r", "crates").unwrap_err();
            assert!(
                matches!(err, GraphError::Metadata { .. })
                    && err.to_string().contains("crates/Cargo.toml")
            );
        }
        let m = MemoryFiles::new().with("w/Cargo.toml", "[workspace]\n");
        assert!(cargo_graph(&m.paths(), &m, &Canned(vec![]), "/r").is_err());
        assert!(cargo_graph(&["gone/Cargo.toml".to_string()], &m, &Canned(vec![]), "/r").is_err());
    }
}
