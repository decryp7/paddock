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
    read(text).map_err(|e| if e.starts_with(FILE) { e } else { format!("{FILE}: {e}") })
}

/// Which planner decides the plans of `./dev plan --ci` (the ci.yml plan job and the app workflows' plan jobs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannerMode {
    /// The repository graph's selection (ADR 0117 §5): the user's decision of 2026-11-23, the default.
    Graph,
    /// The rule tables of `plan.rs` (the emergency switch back; `--compare` and `--replay` read them whatever this says).
    Rules,
}

/// The `[planner]` table: the mode and, for a switch back to the rule tables, the stated reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planner {
    /// Which planner decides.
    pub mode: PlannerMode,
    /// Why (required for `rules`: flipping back is a commit whose reason is stated in the file).
    pub reason: String,
    /// The contract phases that lint and format a project (`fmt`, `lint`): a test-only change keeps them (review round 2 P2-1).
    pub lint_phases: Vec<String>,
}

impl Planner {
    /// The plan job's log line: `planner: graph` or `planner: rules (<reason>)`.
    pub fn line(&self) -> String {
        match self.mode {
            PlannerMode::Graph => "planner: graph".to_string(),
            PlannerMode::Rules => format!("planner: rules ({})", self.reason),
        }
    }
}

/// The `[planner]` table of the inputs file text: no table is the graph; `mode = "rules"` needs a non-empty `reason`; anything else
/// is an error naming it (a typo never silently picks a planner).
pub fn planner(text: &str) -> Result<Planner, String> {
    let file: crate::register_schema::RepoGraphFile = crate::register_schema::parse(FILE, text)?;
    let Some(p) = file.planner else {
        return Ok(Planner {
            mode: PlannerMode::Graph,
            reason: String::new(),
            lint_phases: Vec::new(),
        });
    };
    let reason = p.reason.as_deref().unwrap_or("").trim().to_string();
    let lint_phases = p.lint_phases.unwrap_or_default();
    match p.mode.as_deref() {
        Some("graph") => Ok(Planner {
            mode: PlannerMode::Graph,
            reason,
            lint_phases,
        }),
        Some("rules") if reason.is_empty() => Err(format!(
            "{FILE}: [planner] mode = \"rules\" needs a `reason` (switching back to the rule tables is a commit that says why)"
        )),
        Some("rules") => Ok(Planner {
            mode: PlannerMode::Rules,
            reason,
            lint_phases,
        }),
        other => Err(format!("{FILE}: [planner] mode is {other:?}, not \"graph\" or \"rules\"")),
    }
}

/// [`planner`] of the inputs file under `root` (the checkout the plan job runs in).
pub fn load_planner(root: &Path) -> Result<Planner, String> {
    let text = std::fs::read_to_string(root.join(FILE)).map_err(|e| format!("{FILE} cannot be read: {e}"))?;
    planner(&text)
}

/// A `[from, to]` pair of the typed `root_spellings` list.
fn pairs(v: &[toml::Value], what: &str) -> Result<Vec<(String, String)>, String> {
    v.iter()
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

/// A required text field of a typed table.
fn need(v: Option<String>, key: &str, what: &str) -> Result<String, String> {
    v.ok_or_else(|| format!("{what} has no {key}"))
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
    let t: crate::register_schema::RepoGraphFile = crate::register_schema::parse(FILE, text)?;
    match t.version {
        Some(1) => {}
        other => return Err(format!("version is {other:?}, this reader knows 1")),
    }
    let mut projects: Vec<ProjectDecl> = Vec::new();
    for (n, e) in t.project.into_iter().enumerate() {
        let what = format!("project #{}", n + 1);
        let id = need(e.id, "id", &what)?;
        if id.is_empty() || projects.iter().any(|p| p.id == id) {
            return Err(format!("{what}: id {id:?} is empty or declared twice"));
        }
        projects.push(ProjectDecl {
            folder: need(e.folder, "folder", &format!("project {id:?}"))?,
            id,
        });
    }
    if projects.is_empty() {
        return Err("no [[project]] is declared (the plan's projects come from this file)".into());
    }
    let mut inputs = Vec::new();
    for (n, e) in t.input.into_iter().enumerate() {
        let name = e.name.ok_or(format!("input #{} has no name", n + 1))?;
        let what = format!("input {name:?}");
        if inputs.iter().any(|i: &Input| i.name == name) {
            return Err(format!("{what} is declared twice"));
        }
        let files = e.files.unwrap_or_default();
        if files.is_empty() {
            return Err(format!("{what} names no files"));
        }
        let reaches = e
            .reaches
            .unwrap_or_default()
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
        let reach = Reach::parse(e.reach.as_deref().unwrap_or("code")).map_err(|m| format!("{what}: {m}"))?;
        let extractor = match e.extractor {
            None => None,
            Some(n) => {
                if !paddock_repo_graph::model::narrow::NAMES.contains(&n.as_str()) {
                    return Err(format!(
                        "{what}: unknown extractor {n:?} ({})",
                        paddock_repo_graph::model::narrow::NAMES.join(", ")
                    ));
                }
                Some(n)
            }
        };
        inputs.push(Input {
            name,
            files,
            reaches,
            reach,
            extractor,
            adds: e.adds.unwrap_or(false),
            except: e.except.unwrap_or_default(),
        });
    }
    let mut owners = Vec::new();
    for (n, e) in t.owner.into_iter().enumerate() {
        let what = format!("owner #{}", n + 1);
        let paths = e.paths.unwrap_or_default();
        if paths.is_empty() {
            return Err(format!("{what} names no paths"));
        }
        let owner = match e.node.as_deref() {
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
    let ci = t.ci.ok_or("the [ci] section is missing (setup_only and release_actions)")?;
    let pr = t.prose.ok_or("the [prose] section is missing")?;
    let prose = Prose {
        project_names: pr.project_names.unwrap_or_default(),
        project_folders: pr.project_folders.unwrap_or_default(),
        doc_owner: pr.doc_owner.unwrap_or_default(),
        doc_assets: pr.doc_assets.unwrap_or_default(),
    };
    let p = t.paths.ok_or("the [paths] section is missing")?;
    let paths = Paths {
        workflows: need(p.workflows, "workflows", "[paths]")?,
        actions: need(p.actions, "actions", "[paths]")?,
        apps: need(p.apps, "apps", "[paths]")?,
        contract: need(p.contract, "contract", "[paths]")?,
        script_folders: p.script_folders.unwrap_or_default(),
        node_tests: need(p.node_tests, "node_tests", "[paths]")?,
        e2e: need(p.e2e, "e2e", "[paths]")?,
        checkout: need(p.checkout, "checkout", "[paths]")?,
        root_spellings: pairs(&p.root_spellings.unwrap_or_default(), "paths.root_spellings")?,
    };
    let h = t.hash.unwrap_or_default();
    let hash = HashDecl {
        runner_configs: h.runner_configs.unwrap_or_default(),
        contracts: h.contracts.unwrap_or_default(),
        image_files: h.image_files.unwrap_or_default(),
    };
    Ok(Inputs {
        inputs,
        owners,
        setup_only: ci.setup_only.unwrap_or_default(),
        release_actions: ci.release_actions.unwrap_or_default(),
        gate_phases: ci.release_gate_phases.unwrap_or_default(),
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
    fn the_planner_switch_defaults_to_the_graph_and_a_switch_back_states_its_reason() {
        // No table, and `mode = "graph"`: the graph decides.
        assert_eq!(planner(&doc("")).unwrap().mode, PlannerMode::Graph);
        let g = planner(&doc("[planner]\nmode = \"graph\"\n")).unwrap();
        assert_eq!((g.mode, g.line().as_str()), (PlannerMode::Graph, "planner: graph"));
        // `rules` with its reason: the rule tables decide, and the log line says why.
        let r = planner(&doc("[planner]\nmode = \"rules\"\nreason = \"run 9001 planned too little\"\n")).unwrap();
        assert_eq!(r.mode, PlannerMode::Rules);
        assert_eq!(r.line(), "planner: rules (run 9001 planned too little)");
        // Controls: `rules` without a reason, an unknown mode and an unknown key are refused.
        assert!(planner(&doc("[planner]\nmode = \"rules\"\n")).unwrap_err().contains("reason"));
        assert!(planner(&doc("[planner]\nmode = \"rule\"\n")).is_err());
        assert!(planner(&doc("[planner]\nmode = \"graph\"\nmod = 1\n")).is_err());
        // The inputs reader accepts the table.
        assert!(parse(&doc("[planner]\nmode = \"graph\"\n")).is_ok());
    }

    #[test]
    fn the_repository_plans_from_the_graph() {
        // The user's decision of 2026-11-23: the graph decides CI plans from landing.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let p = load_planner(&root).unwrap();
        assert_eq!(p.mode, PlannerMode::Graph);
        assert!(
            p.lint_phases.iter().any(|l| l == "fmt") && p.lint_phases.iter().any(|l| l == "lint"),
            "{p:?}"
        );
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
        for (text, want) in [("version = 2", "version"), ("not toml [", "Fix the key or the value there")] {
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
                "unknown field `extractr`",
            ),
            (
                "[[input]]\nname = \"x\"\nfiles = [\"a\"]\nreaches = [\"project:crate\"]\n",
                "project:crate",
            ),
            ("[[owner]]\npaths = [\"a\"]\nnode = \"project:tool\"\n", "project:tool"),
            ("[[owner]]\npath = [\"a\"]\nnode = \"none\"\n", "unknown field `path`"),
            ("[ci]\nsetup_onlyy = []\n", "unknown field `setup_onlyy`"),
            ("[prose]\nproject_name = []\n", "unknown field `project_name`"),
            ("[[input]]\nname = \"x\"\nfiles = [\"a\"]\nadds = \"yes\"\n", "expected a boolean"),
        ] {
            let e = parse(&doc(body)).unwrap_err();
            assert!(e.contains(want) && e.starts_with(FILE), "{body:?}: {e}");
        }
        let e = parse(&format!("{HEAD}[prose]\n[ci]\n").replace("[paths]", "[pathz]")).unwrap_err();
        assert!(e.contains("unknown field `pathz`"), "{e}");
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
