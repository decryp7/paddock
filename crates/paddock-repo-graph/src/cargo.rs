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
