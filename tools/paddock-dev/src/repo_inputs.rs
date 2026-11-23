//! The one accessor of `docs/repo-graph.toml` (ADR 0117 §3, ER-42): reads the file and merges it over the defaults of
//! `paddock_repo_graph::model::inputs`. Every reader of declared inputs (the planner, the run scope, the test-result cache) goes
//! through [`load`] or [`parse`].
//!
//! ```toml
//! version = 1
//!
//! [[input]]                      # a named input: files every target reads
//! name = "rust-toolchain"
//! files = ["rust-toolchain.toml"]
//! reaches = ["all"]              # all | cargo | npm | apps | project:<id>
//! reach = "code"                 # code | release | test | plumbing | docs
//! extractor = "attributes"      # a content extractor narrows it: attributes | npm-lock | guards (ADR 0117 §3)
//! adds = false                   # true: the file keeps its owner and this input adds its targets
//! except = ["**/*.md"]           # globs taken back out of `files`
//!
//! [ci]                           # replaces the defaults when given
//! setup_only = ["setup-rust"]    # actions that stay plumbing in a release job (adding one needs the user, ADR 0117 §14)
//! release_actions = ["tauri-bundle"]  # actions whose use makes a job a release job
//!
//! [[owner]]                      # who owns a path no project claims
//! paths = ["spikes/**"]
//! node = "none"                  # none | doc | project:<id>
//! ```

use paddock_repo_graph::model::Reach;
use paddock_repo_graph::model::inputs::{Input, Inputs, Owner, OwnerRule, Target};
use std::path::Path;

/// The inputs file, relative to the repository root.
pub const FILE: &str = "docs/repo-graph.toml";

/// The defaults merged with the repository's file; a missing file is the defaults alone, an unreadable one an error.
pub fn load(root: &Path) -> Result<Inputs, String> {
    match std::fs::read_to_string(root.join(FILE)) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Inputs::defaults()),
        Err(e) => Err(format!("{FILE} cannot be read: {e}")),
    }
}

/// The defaults merged with `text`. `Err` names the entry that is wrong.
pub fn parse(text: &str) -> Result<Inputs, String> {
    Ok(Inputs::defaults().merged(read(text).map_err(|e| format!("{FILE}: {e}"))?))
}

fn strings(v: Option<&toml::Value>, what: &str) -> Result<Vec<String>, String> {
    let Some(v) = v else { return Ok(Vec::new()) };
    v.as_array()
        .ok_or_else(|| format!("{what} is not a list"))?
        .iter()
        .map(|x| {
            x.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{what} holds a value that is not a string"))
        })
        .collect()
}

fn read(text: &str) -> Result<Inputs, String> {
    let t: toml::Table = text.parse().map_err(|e| format!("not TOML: {e}"))?;
    match t.get("version").and_then(toml::Value::as_integer) {
        Some(1) => {}
        other => return Err(format!("version is {other:?}, this reader knows 1")),
    }
    let mut inputs = Vec::new();
    for (n, e) in t.get("input").and_then(toml::Value::as_array).into_iter().flatten().enumerate() {
        let name = e
            .get("name")
            .and_then(toml::Value::as_str)
            .ok_or(format!("input #{} has no name", n + 1))?
            .to_string();
        let what = format!("input {name:?}");
        let files = strings(e.get("files"), &format!("{what}.files"))?;
        if files.is_empty() {
            return Err(format!("{what} names no files"));
        }
        let reaches = strings(e.get("reaches"), &format!("{what}.reaches"))?
            .iter()
            .map(|s| Target::parse(s).map_err(|m| format!("{what}: {m}")))
            .collect::<Result<Vec<_>, _>>()?;
        let reach = Reach::parse(e.get("reach").and_then(toml::Value::as_str).unwrap_or("code")).map_err(|m| format!("{what}: {m}"))?;
        let extractor = match e.get("extractor") {
            None => None,
            Some(v) => {
                let n = v.as_str().ok_or(format!("{what}.extractor is not a string"))?;
                if !paddock_repo_graph::model::narrow::NAMES.contains(&n) {
                    return Err(format!(
                        "{what}: unknown extractor {n:?} ({})",
                        paddock_repo_graph::model::narrow::NAMES.join(", ")
                    ));
                }
                Some(n.to_string())
            }
        };
        let adds = e.get("adds").and_then(toml::Value::as_bool).unwrap_or(false);
        inputs.push(Input {
            name,
            files,
            reaches,
            reach,
            extractor,
            adds,
            except: strings(e.get("except"), &format!("{what}.except"))?,
        });
    }
    let mut owners = Vec::new();
    for (n, e) in t.get("owner").and_then(toml::Value::as_array).into_iter().flatten().enumerate() {
        let what = format!("owner #{}", n + 1);
        let paths = strings(e.get("paths"), &format!("{what}.paths"))?;
        if paths.is_empty() {
            return Err(format!("{what} names no paths"));
        }
        let owner = match e.get("node").and_then(toml::Value::as_str) {
            Some("none") => Owner::None,
            Some("doc") => Owner::Doc,
            Some(p) => match p.strip_prefix("project:") {
                Some(id) if !id.is_empty() => Owner::Project(id.to_string()),
                _ => return Err(format!("{what}: node {p:?} is not none, doc or project:<id>")),
            },
            None => return Err(format!("{what} has no node")),
        };
        owners.push(OwnerRule { paths, owner });
    }
    let ci = t.get("ci");
    let setup_only = strings(ci.and_then(|c| c.get("setup_only")), "ci.setup_only")?;
    let release_actions = strings(ci.and_then(|c| c.get("release_actions")), "ci.release_actions")?;
    Ok(Inputs {
        inputs,
        owners,
        setup_only,
        release_actions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_file_is_the_defaults_and_an_entry_replaces_its_default() {
        assert_eq!(parse("version = 1\n").unwrap(), Inputs::defaults());
        let i = parse(
            "version = 1\n[[input]]\nname = \"npm-root\"\nfiles = [\"package.json\"]\nreaches = [\"npm\"]\n\
             [[owner]]\npaths = [\"spikes/**\"]\nnode = \"project:tools\"\n",
        )
        .unwrap();
        assert_eq!(i.inputs.iter().find(|x| x.name == "npm-root").unwrap().reaches, [Target::Npm]);
        assert_eq!(i.owner_of("spikes/x"), Some(&Owner::Project("tools".into())));
    }

    #[test]
    fn a_wrong_entry_is_refused_with_its_name() {
        for (text, want) in [
            ("version = 2", "version"),
            ("version = 1\n[[input]]\nname = \"x\"\nfiles = []\n", "names no files"),
            (
                "version = 1\n[[input]]\nname = \"x\"\nfiles = [\"a\"]\nreaches = [\"everything\"]\n",
                "unknown target",
            ),
            (
                "version = 1\n[[input]]\nname = \"x\"\nfiles = [\"a\"]\nreach = \"sometimes\"\n",
                "unknown reach",
            ),
            ("version = 1\n[[owner]]\npaths = [\"a\"]\n", "has no node"),
            (
                "version = 1\n[[input]]\nname = \"x\"\nfiles = [\"a\"]\nextractor = \"magic\"\n",
                "unknown extractor",
            ),
            (
                "version = 1\n[[owner]]\npaths = [\"a\"]\nnode = \"somebody\"\n",
                "not none, doc or project",
            ),
            ("not toml [", "not TOML"),
        ] {
            let e = parse(text).unwrap_err();
            assert!(e.contains(want) && e.starts_with(FILE), "{text:?}: {e}");
        }
    }

    #[test]
    fn the_repository_file_reads() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let i = load(&root).unwrap();
        assert!(i.inputs.len() >= Inputs::defaults().inputs.len());
    }
}
