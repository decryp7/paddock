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
//! [[project]]                    # a project of the plan: its id and folder (an app is `app:<name>`, found by its contract)
//! id = "crates"
//! folder = "crates"
//!
//! [paths]                        # the folders and file names the adapters read
//! workflows = ".gitea/workflows/"
//! checkout = ".paddock/"         # the folder an app repository checks Paddock out to
//! root_spellings = [["$GITHUB_WORKSPACE/", ""]]  # [from, to]: spellings of the repository root in a step, replaced in order
//!
//! [ci]                           # required
//! setup_only = ["setup-rust"]    # actions that stay plumbing in a release job (adding one needs the user, ADR 0117 §14)
//! release_actions = ["tauri-bundle"]  # actions whose use makes a job a release job
//! release_gate_phases = ["checks"]    # contract phases a check of a release job runs beside the smoke set (no installers)
//!
//! [hash]                         # what an input hash reads besides a node's files (ADR 0117 §5)
//! runner_configs = ["**/.config/nextest.toml", "**/vitest.config.*", "**/playwright.config.*"]
//! contracts = ["paddock.ci.json"]
//! image_files = ["ci-image/VERSION"]
//!
//! [[owner]]                      # who owns a path no project claims
//! paths = ["spikes/**"]
//! node = "none"                  # none | doc | project:<id>
//! ```

use paddock_repo_graph::model::Reach;
use paddock_repo_graph::model::inputs::{HashDecl, Input, Inputs, Owner, OwnerRule, Paths, ProjectDecl, Prose, Target};
use std::path::Path;

/// The inputs file, relative to the repository root.
pub const FILE: &str = "docs/repo-graph.toml";

/// The repository's declarations. A missing or unreadable file is an error: without it nothing is owned (the crate holds no
/// defaults, ADR 0117 §14), so a caller that goes on must answer everything.
pub fn load(root: &Path) -> Result<Inputs, String> {
    match std::fs::read_to_string(root.join(FILE)) {
        Ok(text) => parse(&text),
        Err(e) => Err(format!("{FILE} cannot be read: {e}")),
    }
}

/// The declarations in `text`. `Err` names the entry that is wrong.
pub fn parse(text: &str) -> Result<Inputs, String> {
    read(text).map_err(|e| format!("{FILE}: {e}"))
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

/// Refuses every key of `table` that is not in `known`: a typo is an error naming it, never a silently ignored entry.
fn only_keys(table: &toml::Table, known: &[&str], what: &str) -> Result<(), String> {
    match table.keys().find(|k| !known.contains(&k.as_str())) {
        Some(k) => Err(format!("{what} has an unknown key {k:?} (known: {})", known.join(", "))),
        None => Ok(()),
    }
}

/// A list of `[from, to]` string pairs (absent: none).
fn pairs(v: Option<&toml::Value>, what: &str) -> Result<Vec<(String, String)>, String> {
    let Some(v) = v else { return Ok(Vec::new()) };
    v.as_array()
        .ok_or_else(|| format!("{what} is not a list"))?
        .iter()
        .map(|x| {
            match x
                .as_array()
                .map(|a| a.iter().map(toml::Value::as_str).collect::<Vec<_>>())
                .as_deref()
            {
                Some([Some(from), Some(to)]) => Ok((from.to_string(), to.to_string())),
                _ => Err(format!("{what} holds a value that is not a [from, to] pair of strings")),
            }
        })
        .collect()
}

fn table<'a>(v: &'a toml::Value, what: &str) -> Result<&'a toml::Table, String> {
    v.as_table().ok_or_else(|| format!("{what} is not a table"))
}

fn tables<'a>(t: &'a toml::Table, key: &str) -> Result<Vec<&'a toml::Table>, String> {
    match t.get(key) {
        None => Ok(Vec::new()),
        Some(v) => v
            .as_array()
            .ok_or_else(|| format!("{key} is not a list of [[{key}]] tables"))?
            .iter()
            .enumerate()
            .map(|(n, e)| table(e, &format!("{key} #{}", n + 1)))
            .collect(),
    }
}

fn text_of(t: &toml::Table, key: &str, what: &str) -> Result<String, String> {
    match t.get(key) {
        None => Err(format!("{what} has no {key}")),
        Some(v) => v
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| format!("{what}.{key} is not a string")),
    }
}

/// The projects a `project:<id>` may name: the declared ones, and any `app:<name>` (its app is checked against the apps found, see
/// [`check_apps`]).
fn known_project(id: &str, declared: &[ProjectDecl]) -> bool {
    declared.iter().any(|p| p.id == id) || id.strip_prefix("app:").is_some_and(|a| !a.is_empty())
}

/// Every `app:<name>` the declarations name must be an app found in the repository (a typo in an app name would otherwise
/// reach nothing, silently).
pub fn check_apps(inputs: &Inputs, apps: &std::collections::BTreeSet<String>) -> Result<(), String> {
    let mut named: Vec<&str> = Vec::new();
    for i in &inputs.inputs {
        for t in &i.reaches {
            if let Target::Project(p) = t {
                named.push(p);
            }
        }
    }
    for o in &inputs.owners {
        if let Owner::Project(p) = &o.owner {
            named.push(p);
        }
    }
    match named.iter().filter_map(|p| p.strip_prefix("app:")).find(|a| !apps.contains(*a)) {
        Some(a) => Err(format!("{FILE}: project app:{a} names an app that has no contract file")),
        None => Ok(()),
    }
}

fn read(text: &str) -> Result<Inputs, String> {
    let t: toml::Table = text.parse().map_err(|e| format!("not TOML: {e}"))?;
    only_keys(
        &t,
        &["version", "input", "owner", "project", "paths", "prose", "ci", "hash"],
        "the file",
    )?;
    match t.get("version").and_then(toml::Value::as_integer) {
        Some(1) => {}
        other => return Err(format!("version is {other:?}, this reader knows 1")),
    }
    let mut projects: Vec<ProjectDecl> = Vec::new();
    for (n, e) in tables(&t, "project")?.into_iter().enumerate() {
        let what = format!("project #{}", n + 1);
        only_keys(e, &["id", "folder"], &what)?;
        let id = text_of(e, "id", &what)?;
        if id.is_empty() || projects.iter().any(|p| p.id == id) {
            return Err(format!("{what}: id {id:?} is empty or declared twice"));
        }
        projects.push(ProjectDecl {
            folder: text_of(e, "folder", &format!("project {id:?}"))?,
            id,
        });
    }
    if projects.is_empty() {
        return Err("no [[project]] is declared (the plan's projects come from this file)".into());
    }
    let mut inputs = Vec::new();
    for (n, e) in tables(&t, "input")?.into_iter().enumerate() {
        only_keys(
            e,
            &["name", "files", "reaches", "reach", "extractor", "adds", "except"],
            &format!("input #{}", n + 1),
        )?;
        let name = e
            .get("name")
            .and_then(toml::Value::as_str)
            .ok_or(format!("input #{} has no name", n + 1))?
            .to_string();
        let what = format!("input {name:?}");
        if inputs.iter().any(|i: &Input| i.name == name) {
            return Err(format!("{what} is declared twice"));
        }
        let files = strings(e.get("files"), &format!("{what}.files"))?;
        if files.is_empty() {
            return Err(format!("{what} names no files"));
        }
        let reaches = strings(e.get("reaches"), &format!("{what}.reaches"))?
            .iter()
            .map(|s| {
                let t = Target::parse(s).map_err(|m| format!("{what}: {m}"))?;
                match &t {
                    Target::Project(p) if !known_project(p, &projects) => {
                        Err(format!("{what}: reaches project:{p}, which is not a declared project"))
                    }
                    _ => Ok(t),
                }
            })
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
        let adds = match e.get("adds") {
            None => false,
            Some(v) => v.as_bool().ok_or(format!("{what}.adds is not true or false"))?,
        };
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
    for (n, e) in tables(&t, "owner")?.into_iter().enumerate() {
        let what = format!("owner #{}", n + 1);
        only_keys(e, &["paths", "node"], &what)?;
        let paths = strings(e.get("paths"), &format!("{what}.paths"))?;
        if paths.is_empty() {
            return Err(format!("{what} names no paths"));
        }
        let owner = match e.get("node").and_then(toml::Value::as_str) {
            Some("none") => Owner::None,
            Some("doc") => Owner::Doc,
            Some(p) => match p.strip_prefix("project:") {
                Some(id) if known_project(id, &projects) => Owner::Project(id.to_string()),
                Some(id) => return Err(format!("{what}: node project:{id} is not a declared project")),
                _ => return Err(format!("{what}: node {p:?} is not none, doc or project:<id>")),
            },
            None => return Err(format!("{what} has no node")),
        };
        owners.push(OwnerRule { paths, owner });
    }
    let ci = table(
        t.get("ci").ok_or("the [ci] section is missing (setup_only and release_actions)")?,
        "ci",
    )?;
    only_keys(ci, &["setup_only", "release_actions", "release_gate_phases"], "[ci]")?;
    let setup_only = strings(ci.get("setup_only"), "ci.setup_only")?;
    let release_actions = strings(ci.get("release_actions"), "ci.release_actions")?;
    let gate_phases = strings(ci.get("release_gate_phases"), "ci.release_gate_phases")?;
    let prose_table = table(t.get("prose").ok_or("the [prose] section is missing")?, "prose")?;
    only_keys(
        prose_table,
        &["project_names", "project_folders", "doc_owner", "doc_assets"],
        "[prose]",
    )?;
    let list = |k: &str| strings(prose_table.get(k), &format!("prose.{k}"));
    let prose = Prose {
        project_names: list("project_names")?,
        project_folders: list("project_folders")?,
        doc_owner: list("doc_owner")?,
        doc_assets: list("doc_assets")?,
    };
    let p = table(t.get("paths").ok_or("the [paths] section is missing")?, "paths")?;
    only_keys(
        p,
        &[
            "workflows",
            "actions",
            "apps",
            "contract",
            "script_folders",
            "node_tests",
            "e2e",
            "checkout",
            "root_spellings",
        ],
        "[paths]",
    )?;
    let paths = Paths {
        workflows: text_of(p, "workflows", "[paths]")?,
        actions: text_of(p, "actions", "[paths]")?,
        apps: text_of(p, "apps", "[paths]")?,
        contract: text_of(p, "contract", "[paths]")?,
        script_folders: strings(p.get("script_folders"), "paths.script_folders")?,
        node_tests: text_of(p, "node_tests", "[paths]")?,
        e2e: text_of(p, "e2e", "[paths]")?,
        checkout: text_of(p, "checkout", "[paths]")?,
        root_spellings: pairs(p.get("root_spellings"), "paths.root_spellings")?,
    };
    let hash_table = t.get("hash");
    if let Some(h) = hash_table {
        only_keys(table(h, "hash")?, &["runner_configs", "contracts", "image_files"], "[hash]")?;
    }
    let hlist = |k: &str| strings(hash_table.and_then(|p| p.get(k)), &format!("hash.{k}"));
    let hash = HashDecl {
        runner_configs: hlist("runner_configs")?,
        contracts: hlist("contracts")?,
        image_files: hlist("image_files")?,
    };
    Ok(Inputs {
        inputs,
        owners,
        setup_only,
        release_actions,
        gate_phases,
        hash,
        prose,
        projects,
        paths,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "version = 1\n[[project]]\nid = \"tools\"\nfolder = \"tools\"\n[[project]]\nid = \"crates\"\nfolder = \"crates\"\n\
        [paths]\nworkflows = \"w/\"\nactions = \"a/\"\napps = \"x/\"\ncontract = \"c.json\"\nscript_folders = [\"s/\"]\nnode_tests = \"t/\"\ne2e = \"e\"\ncheckout = \"k/\"\n";

    /// A valid file around `body`: the head, the body, and the `[prose]` and `[ci]` sections the body does not give.
    fn doc(body: &str) -> String {
        let mut t = format!("{HEAD}{body}");
        if !body.contains("[prose]") {
            t.push_str("[prose]\n");
        }
        if !body.contains("[ci]") {
            t.push_str("[ci]\n");
        }
        t
    }

    #[test]
    fn a_minimal_file_owns_nothing_and_an_entry_reads_with_its_prose() {
        let min = parse(&doc("")).unwrap();
        assert!(min.inputs.is_empty() && min.owners.is_empty());
        assert_eq!(min.projects.len(), 2);
        assert_eq!(min.paths.workflows, "w/");
        let i = parse(&doc(
            "[[input]]\nname = \"npm-root\"\nfiles = [\"/package.json\"]\nreaches = [\"npm\"]\nadds = true\n\
             [[owner]]\npaths = [\"spikes/**\"]\nnode = \"project:tools\"\n[prose]\nproject_names = [\"README*\"]\n",
        ))
        .unwrap();
        let npm = i.inputs.iter().find(|x| x.name == "npm-root").unwrap();
        assert_eq!((npm.reaches.as_slice(), npm.adds), ([Target::Npm].as_slice(), true));
        assert_eq!(i.owner_of("spikes/x"), Some(&Owner::Project("tools".into())));
        assert_eq!(i.prose.project_names, ["README*"]);
        assert!(
            parse(&doc(
                "[[input]]\nname = \"a\"\nfiles = [\"x\"]\n[[input]]\nname = \"a\"\nfiles = [\"y\"]\n"
            ))
            .unwrap_err()
            .contains("declared twice")
        );
    }

    #[test]
    fn a_wrong_entry_is_refused_with_its_name() {
        for (body, want) in [
            ("[[input]]\nname = \"x\"\nfiles = []\n", "names no files"),
            (
                "[[input]]\nname = \"x\"\nfiles = [\"a\"]\nreaches = [\"everything\"]\n",
                "unknown target",
            ),
            ("[[input]]\nname = \"x\"\nfiles = [\"a\"]\nreach = \"sometimes\"\n", "unknown reach"),
            ("[[owner]]\npaths = [\"a\"]\n", "has no node"),
            (
                "[[input]]\nname = \"x\"\nfiles = [\"a\"]\nextractor = \"magic\"\n",
                "unknown extractor",
            ),
            ("[[owner]]\npaths = [\"a\"]\nnode = \"somebody\"\n", "not none, doc or project"),
        ] {
            let e = parse(&doc(body)).unwrap_err();
            assert!(e.contains(want) && e.starts_with(FILE), "{body:?}: {e}");
        }
        for (text, want) in [("version = 2", "version"), ("not toml [", "not TOML")] {
            let e = parse(text).unwrap_err();
            assert!(e.contains(want) && e.starts_with(FILE), "{text:?}: {e}");
        }
    }

    #[test]
    fn a_typo_is_refused_at_every_level_and_the_control_accepts_the_right_spelling() {
        // Failing first (P1-7): an unknown key was ignored without a word, so `extractr = "guards"` narrowed nothing and a
        // misspelt `project:` reached nothing.
        for (body, want) in [
            (
                "[[input]]\nname = \"x\"\nfiles = [\"a\"]\nextractr = \"guards\"\n",
                "unknown key \"extractr\"",
            ),
            (
                "[[input]]\nname = \"x\"\nfiles = [\"a\"]\nreaches = [\"project:crate\"]\n",
                "project:crate",
            ),
            ("[[owner]]\npaths = [\"a\"]\nnode = \"project:tool\"\n", "project:tool"),
            ("[[owner]]\npath = [\"a\"]\nnode = \"none\"\n", "unknown key \"path\""),
            ("[ci]\nsetup_onlyy = []\n", "unknown key \"setup_onlyy\""),
            ("[prose]\nproject_name = []\n", "unknown key \"project_name\""),
            (
                "[[input]]\nname = \"x\"\nfiles = [\"a\"]\nadds = \"yes\"\n",
                "adds is not true or false",
            ),
        ] {
            let e = parse(&doc(body)).unwrap_err();
            assert!(e.contains(want) && e.starts_with(FILE), "{body:?}: {e}");
        }
        let e = parse(&format!("{HEAD}[prose]\n[ci]\n").replace("[paths]", "[pathz]")).unwrap_err();
        assert!(e.contains("unknown key \"pathz\""), "{e}");
        // Control: the right spellings read.
        let ok = parse(&doc(
            "[[input]]\nname = \"x\"\nfiles = [\"a\"]\nextractor = \"guards\"\nreaches = [\"project:crates\", \"project:app:dyno\"]\n\
             [[owner]]\npaths = [\"a\"]\nnode = \"project:tools\"\n[ci]\nsetup_only = []\n",
        ))
        .unwrap();
        assert_eq!(ok.inputs[0].extractor.as_deref(), Some("guards"));
        // An app name is checked against the apps found.
        let apps: std::collections::BTreeSet<String> = ["dyno".to_string()].into_iter().collect();
        assert!(check_apps(&ok, &apps).is_ok());
        assert!(
            check_apps(&ok, &std::collections::BTreeSet::new())
                .unwrap_err()
                .contains("app:dyno")
        );
    }

    #[test]
    fn the_ci_section_is_required() {
        // Failing first: a file with no [ci] section read as "no setup-only and no release actions", silently.
        let no_ci = format!("{HEAD}[prose]\n");
        assert!(parse(&no_ci).unwrap_err().contains("[ci] section is missing"));
        assert!(parse(&format!("{no_ci}[ci]\n")).is_ok());
        assert!(parse(&format!("{HEAD}[ci]\n")).unwrap_err().contains("[prose] section is missing"));
        assert!(parse("version = 1\n[prose]\n[ci]\n").unwrap_err().contains("[[project]]"));
    }

    #[test]
    fn the_repository_file_reads() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let i = load(&root).unwrap();
        assert!(i.inputs.len() >= 40, "the declarations moved out of the crate (ADR 0117 §14)");
        assert!(!i.owners.is_empty() && !i.prose.project_names.is_empty() && !i.setup_only.is_empty());
        assert!(i.projects.iter().any(|p| p.id == "paddock-dev") && !i.paths.script_folders.is_empty());
        // A missing file is an error, never an empty set read as "nothing reaches anything".
        assert!(load(&root.join("no-such-folder")).unwrap_err().contains("cannot be read"));
    }
}
