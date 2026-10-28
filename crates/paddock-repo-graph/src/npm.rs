use crate::{FileSource, Graph, GraphError, Node, dirname, join_normalized};
use serde_json::Value;
use std::collections::BTreeMap;

fn parse(files: &dyn FileSource, path: &str) -> Result<Value, GraphError> {
    serde_json::from_str(&files.read_to_string(path)?).map_err(|e| GraphError::Parse {
        path: path.to_string(),
        reason: e.to_string(),
    })
}

fn is_named(path: &str, name: &str) -> bool {
    path == name || path.ends_with(&format!("/{name}"))
}

/// The quoted `../..` references of a config file: `"../../ui/dist/x"` or `'../eslint/src/index.cjs'`. The reference runs to
/// the next quote or `*`, and must start with `../` and have a character after it.
fn path_refs(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let start = i + 1;
            let len = bytes[start..]
                .iter()
                .take_while(|b| !matches!(**b, b'"' | b'\'' | b'*'))
                .count();
            let run = &text[start..start + len];
            if run.starts_with("../") && run.len() > 3 {
                out.push(run.to_string());
                i = start + len;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn dependency_names(pkg: &Value) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for key in ["dependencies", "devDependencies", "peerDependencies"] {
        if let Some(obj) = pkg.get(key).and_then(Value::as_object) {
            for n in obj.keys() {
                if !names.contains(n) {
                    names.push(n.clone());
                }
            }
        }
    }
    names
}

/// The npm projects among `files` (tracked paths): every `package.json` below the root (the root one is the workspace
/// manifest and is not a project), with an edge for each dependency named like another project, plus two kinds of edge
/// that bypass npm:
///
/// - a Tauri shell (`tauri.conf.json`) whose `build.frontendDist` points into another project depends on it (the shell's
///   folder becomes a project of its own);
/// - `tsconfig*.json`, `angular.json` and `eslint.config.{js,mjs,cjs}` that name a path outside their project (`"../x"`)
///   make their project depend on the project that owns it.
pub fn npm_graph(files: &[String], source: &dyn FileSource) -> Result<Graph, GraphError> {
    let mut g = Graph::new();
    let mut raw: BTreeMap<String, Value> = BTreeMap::new();
    for f in files
        .iter()
        .filter(|f| f.ends_with("package.json") && f.as_str() != "package.json")
    {
        let j = parse(source, f)?;
        let name = j
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        g.insert(
            &dirname(f),
            Node {
                name,
                workspace: String::new(),
                deps: Vec::new(),
            },
        );
        raw.insert(dirname(f), j);
    }
    let by_name: BTreeMap<String, String> = g
        .iter()
        .map(|(d, n)| (n.name.clone(), d.to_string()))
        .collect();
    for (dir, pkg) in &raw {
        for n in dependency_names(pkg) {
            g.add_edge(dir, by_name.get(&n).map(String::as_str));
        }
    }
    for f in files.iter().filter(|f| is_named(f, "tauri.conf.json")) {
        let dist = parse(source, f)?
            .pointer("/build/frontendDist")
            .and_then(Value::as_str)
            .map(str::to_string);
        let Some(dist) = dist.filter(|d| !(d.starts_with("http://") || d.starts_with("https://")))
        else {
            continue;
        };
        let own = dirname(f);
        let target = join_normalized(&own, &dist).and_then(|p| g.node_of(&p).map(str::to_string));
        let Some(target) = target else { continue };
        if own == target || own.starts_with(&format!("{target}/")) {
            continue;
        }
        if !g.contains(&own) {
            g.insert(
                &own,
                Node {
                    name: own.clone(),
                    workspace: String::new(),
                    deps: Vec::new(),
                },
            );
        }
        g.add_edge(&own, Some(&target));
    }
    for f in files.iter().filter(|f| is_config(f)) {
        let Some(from) = g.node_of(f).map(str::to_string) else {
            continue;
        };
        let text = source.read_to_string(f)?;
        for r in path_refs(&text) {
            let to =
                join_normalized(&dirname(f), &r).and_then(|p| g.node_of(&p).map(str::to_string));
            g.add_edge(&from, to.as_deref());
        }
    }
    Ok(g)
}

fn is_config(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let tsconfig = name.starts_with("tsconfig") && name.ends_with(".json");
    let eslint = ["eslint.config.js", "eslint.config.mjs", "eslint.config.cjs"].contains(&name);
    tsconfig || name == "angular.json" || eslint
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryFiles;

    fn graph(m: &MemoryFiles) -> Graph {
        npm_graph(&m.paths(), m).unwrap()
    }

    #[test]
    fn packages_depend_on_each_other_by_name() {
        let m = MemoryFiles::new()
            .with("package.json", r#"{"workspaces":["ui","apps/x"]}"#)
            .with("ui/package.json", r#"{"name":"@d/ui"}"#)
            .with("apps/x/package.json", r#"{"name":"x","dependencies":{"@d/ui":"1","react":"1"},"devDependencies":{"@d/ui":"1"}}"#)
            .with("apps/y/package.json", r#"{"peerDependencies":{"x":"1"}}"#);
        let g = graph(&m);
        assert_eq!(g.get("apps/x").unwrap().deps, ["ui"]);
        assert_eq!(g.get("apps/y").unwrap().deps, ["apps/x"]);
        assert!(!g.contains("."));
    }

    #[test]
    fn a_shell_embedding_a_ui_built_elsewhere_depends_on_it() {
        let m = MemoryFiles::new()
            .with("ui/package.json", r#"{"name":"ui"}"#)
            .with(
                "apps/s/src-tauri/tauri.conf.json",
                r#"{"build":{"frontendDist":"../../../ui/dist"}}"#,
            )
            .with(
                "apps/t/src-tauri/tauri.conf.json",
                r#"{"build":{"frontendDist":"http://localhost:1420"}}"#,
            )
            .with("apps/u/tauri.conf.json", r#"{"build":{}}"#)
            .with(
                "apps/v/tauri.conf.json",
                r#"{"build":{"frontendDist":"../nowhere"}}"#,
            )
            .with("apps/w/package.json", r#"{"name":"w"}"#)
            .with(
                "apps/w/tauri.conf.json",
                r#"{"build":{"frontendDist":"dist"}}"#,
            )
            .with(
                "apps/z/tauri.conf.json",
                r#"{"build":{"frontendDist":"../../../../outside"}}"#,
            );
        let g = graph(&m);
        assert_eq!(g.get("apps/s/src-tauri").unwrap().deps, ["ui"]);
        assert!(
            !g.contains("apps/t/src-tauri")
                && !g.contains("apps/u")
                && !g.contains("apps/v")
                && !g.contains("apps/z")
        );
        assert!(
            g.get("apps/w").unwrap().deps.is_empty(),
            "a shell inside its own package depends on nothing"
        );
    }

    #[test]
    fn config_files_that_name_outside_paths_add_edges() {
        let m = MemoryFiles::new()
            .with("ui/package.json", r#"{"name":"ui"}"#)
            .with("eslint/package.json", r#"{"name":"eslint"}"#)
            .with("apps/a/package.json", r#"{"name":"a"}"#)
            .with(
                "ui/eslint.config.mjs",
                "import x from '../eslint/src/index.cjs';",
            )
            .with(
                "apps/a/tsconfig.app.json",
                r#"{"paths":{"@d/ui":["../../ui/dist/*"]},"extends":"./tsconfig.json"}"#,
            )
            .with("apps/a/angular.json", r#"{"a":"../../ui/dist"}"#)
            .with("loose/tsconfig.json", r#"{"a":"../ui/x"}"#);
        let g = graph(&m);
        assert_eq!(g.get("ui").unwrap().deps, ["eslint"]);
        assert_eq!(g.get("apps/a").unwrap().deps, ["ui"]);
    }

    #[test]
    fn path_refs_follow_the_quoting_rules() {
        assert_eq!(
            path_refs(r#""../a/b", '../c' "x" "../" "..""#),
            ["../a/b", "../c"]
        );
        assert_eq!(path_refs(r#""../a/*.ts" "../b""#), ["../a/", "../b"]);
        assert!(path_refs("no quotes ../a").is_empty());
        assert_eq!(path_refs("'../a"), ["../a"]);
    }

    #[test]
    fn bad_json_and_missing_files_are_typed_errors() {
        let m = MemoryFiles::new().with("a/package.json", "{nope");
        let err = npm_graph(&m.paths(), &m).unwrap_err();
        assert!(
            matches!(err, GraphError::Parse { .. }) && err.to_string().contains("a/package.json")
        );
        let err = npm_graph(&["a/package.json".to_string()], &MemoryFiles::new()).unwrap_err();
        assert!(matches!(err, GraphError::Read { .. }));
    }
}
