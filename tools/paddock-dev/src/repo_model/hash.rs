//! `input_hash(node)` (ADR 0117 §5, review P1-8): the content address of a node, built on the mechanism of [`crate::input_hash`]
//! (`git ls-tree` object ids, the `commit-hash:` and `release:` lines of `rustc -vV`, the hashed environment variables and a digest
//! of each cargo config) and widened by the inputs §5 adds: the build-script environment, the contract's command lines, the nextest,
//! Vitest and Playwright configuration files, the CI image and the OS.
//!
//! One bulk read for every node asked: one `git ls-tree` over the union of the files, one `git status`, one `rustc -vV`; each node's
//! text is then assembled from that map ([`paddock_repo_graph::model::hash::canonical`]) and addressed with SHA-256. A node's
//! dependencies' hashes are part of its text, computed once each in dependency order.
//!
//! Limits, said here and in the output: the contract enters as its whole text (a superset of its command lines, so a change to
//! any phase is certainly in the hash and an unrelated edit over-invalidates, the safe direction); a build script that reads a
//! variable by a name built at run time is not seen.

use super::World;
use crate::input_hash::{Exec, toolchain};
use paddock_cli::CliError;
use paddock_repo_graph::model::glob;
use paddock_repo_graph::model::hash::{HashInputs, build_script_vars, canonical, dependencies, files_of, read_set};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// One node's hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeHash {
    /// The node.
    pub node: String,
    /// `sha256:` and the hex digest of the canonical text.
    pub hash: String,
    /// A file of the node has uncommitted changes: the hash describes HEAD, not the working tree.
    pub dirty: bool,
}

/// The pieces of the repository this module reads, so a test can supply them: the tracked files and the text of any of them.
pub struct Tree<'a> {
    /// The tracked files.
    pub files: &'a [String],
    /// A tracked file's text at HEAD (`None`: absent).
    pub text: &'a dyn Fn(&str) -> Option<String>,
}

/// The folder a runner configuration file configures: its own folder, without the `.config` of nextest's.
fn config_dir(path: &str) -> &str {
    let dir = path.rsplit_once('/').map_or("", |(d, _)| d);
    dir.strip_suffix("/.config").or_else(|| dir.strip_suffix(".config")).unwrap_or(dir)
}

/// The runner configuration files that select and run a node whose files are under `paths`: every declared config in a folder
/// that holds one of them (a workspace's own and the ones above it).
fn runner_configs_for(configs: &[String], paths: &[String]) -> Vec<String> {
    configs
        .iter()
        .filter(|c| {
            let dir = config_dir(c);
            paths
                .iter()
                .any(|p| dir.is_empty() || p == dir || p.strip_prefix(dir).is_some_and(|r| r.starts_with('/')))
        })
        .cloned()
        .collect()
}

/// The `git ls-tree` line for each path, by path (a folder entry is the folder's tree id).
fn tree_map(exec: &dyn Exec, root: &Path, paths: &[String]) -> Result<BTreeMap<String, String>, CliError> {
    let mut out = BTreeMap::new();
    if paths.is_empty() {
        return Ok(out);
    }
    let mut args: Vec<&str> = vec!["ls-tree", "HEAD", "--"];
    args.extend(paths.iter().map(String::as_str));
    for line in exec.run("git", &args, root, None)?.lines() {
        if let Some((_, path)) = line.split_once('\t') {
            out.insert(path.to_string(), line.to_string());
        }
    }
    Ok(out)
}

/// What the process around the hash supplies: its environment, the same as pairs (the hashed variables) and the operating system.
pub struct HashEnv<'a> {
    /// Looks up one environment variable.
    pub env: &'a dyn Fn(&str) -> Option<String>,
    /// The whole environment, as pairs.
    pub vars: &'a [(String, String)],
    /// The operating system the hash is for.
    pub os: &'a str,
}

type Build<'a> = dyn Fn(&str, &BTreeMap<String, NodeHash>) -> (String, bool) + 'a;

/// The hashes of `nodes` (and, inside them, their dependencies).
pub fn node_hashes(
    exec: &dyn Exec,
    ctx: &HashEnv,
    root: &Path,
    world: &World,
    tree: &Tree,
    nodes: &[String],
) -> Result<BTreeMap<String, NodeHash>, CliError> {
    let (env, vars, os) = (ctx.env, ctx.vars, ctx.os);
    let decl = &world.inputs;
    let model = &world.model;
    for n in nodes {
        if model.node(n).is_none() {
            return Err(CliError::failure(format!(
                "::error::input hash: the model has no node {n}. Next step: name a node of `./dev plan --compare`'s model (for example `crate:crates/paddock-proc`)."
            )));
        }
    }
    // Every node whose hash is needed: the asked ones and their dependencies.
    let mut needed: BTreeSet<String> = nodes.iter().cloned().collect();
    for n in nodes {
        needed.extend(dependencies(model, n));
    }
    let needed: Vec<String> = needed.into_iter().collect();
    // Files: each needed node's read set, the runner configs, the image files, the build scripts, the contracts.
    let configs: Vec<String> = tree
        .files
        .iter()
        .filter(|f| decl.hash.runner_configs.iter().any(|g| glob::matches(g, f)))
        .cloned()
        .collect();
    let contracts: Vec<String> = tree
        .files
        .iter()
        .filter(|f| decl.hash.contracts.iter().any(|g| glob::matches(g, f)))
        .cloned()
        .collect();
    let mut own: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut all: BTreeSet<String> = decl.hash.image_files.iter().cloned().collect();
    for n in &needed {
        let files = files_of(model, &read_set(model, n));
        all.extend(files.iter().cloned());
        own.insert(n.clone(), files);
    }
    all.extend(configs.iter().cloned());
    all.extend(contracts.iter().cloned());
    let all: Vec<String> = all.into_iter().collect();
    let lines = tree_map(exec, root, &all)?;
    let status = {
        let mut a: Vec<&str> = vec!["status", "--porcelain", "--untracked-files=all", "--"];
        a.extend(all.iter().map(String::as_str));
        exec.run("git", &a, root, None)?
    };
    let dirty_paths: Vec<String> = status.lines().filter_map(|l| l.get(3..)).map(str::to_string).collect();
    let (compiler, flags) = toolchain(exec, env, vars, root, root)?;
    let toolchain_text = format!("{compiler}{}", flags.trim_end_matches('\n'));
    let image: String = decl
        .hash
        .image_files
        .iter()
        .filter_map(|f| lines.get(f))
        .map(|l| format!("{l}\n"))
        .collect();
    let lines_of = |paths: &[String]| -> String { paths.iter().filter_map(|p| lines.get(p)).map(|l| format!("{l}\n")).collect() };
    let mut done: BTreeMap<String, NodeHash> = BTreeMap::new();
    // Dependencies first: a node's text holds the hashes of its direct dependencies; the order is by depth, a cycle is cut at the
    // node that closes it (its id stays in the text, its hash does not).
    fn visit(
        id: &str,
        stack: &mut Vec<String>,
        done: &mut BTreeMap<String, NodeHash>,
        build: &Build,
        direct: &dyn Fn(&str) -> Vec<String>,
    ) {
        if done.contains_key(id) || stack.iter().any(|s| s == id) {
            return;
        }
        stack.push(id.to_string());
        for d in direct(id) {
            visit(&d, stack, done, build, direct);
        }
        stack.pop();
        let (hash, dirty) = build(id, done);
        done.insert(
            id.to_string(),
            NodeHash {
                node: id.to_string(),
                hash,
                dirty,
            },
        );
    }
    let direct = |id: &str| -> Vec<String> {
        model
            .targets(id, paddock_repo_graph::model::nodes::EdgeKind::Depends)
            .into_iter()
            .map(str::to_string)
            .collect()
    };
    let build = |id: &str, done: &BTreeMap<String, NodeHash>| -> (String, bool) {
        let files = own.get(id).cloned().unwrap_or_default();
        let deps: Vec<(String, String)> = direct(id)
            .into_iter()
            .map(|d| {
                let h = done.get(&d).map_or_else(|| "cycle".to_string(), |h| h.hash.clone());
                (d, h)
            })
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .collect();
        // Build scripts: of the node's crate and of every crate it depends on.
        let mut crates = dependencies(model, id);
        crates.push(id.to_string());
        let mut vars_set: BTreeSet<String> = BTreeSet::new();
        for c in &crates {
            if let Some(dir) = c.strip_prefix("crate:") {
                let script = format!("{dir}/build.rs");
                if let Some(text) = (tree.text)(&script) {
                    for name in build_script_vars(&text) {
                        if let Some(v) = env(&name).filter(|v| !v.is_empty()) {
                            vars_set.insert(format!("{name}={v}"));
                        }
                    }
                }
            }
        }
        let cfgs = runner_configs_for(&configs, &files);
        let contract: Vec<String> = runner_configs_for(&contracts, &files)
            .iter()
            .filter_map(|c| (tree.text)(c).map(|t| format!("{c}: {}", t.split_whitespace().collect::<Vec<_>>().join(" "))))
            .collect();
        let inputs = HashInputs {
            node: id.to_string(),
            tree: lines_of(&files),
            deps,
            build_env: vars_set.into_iter().collect(),
            contract,
            runner_configs: lines_of(&cfgs),
            image: image.clone(),
            os: os.to_string(),
            toolchain: toolchain_text.clone(),
        };
        let digest = Sha256::digest(canonical(&inputs).as_bytes());
        let dirty = files
            .iter()
            .chain(cfgs.iter())
            .any(|f| dirty_paths.iter().any(|d| d == f || d.starts_with(&format!("{f}/"))));
        (
            format!("sha256:{}", digest.iter().map(|b| format!("{b:02x}")).collect::<String>()),
            dirty,
        )
    };
    for n in &needed {
        visit(n, &mut Vec::new(), &mut done, &build, &direct);
    }
    Ok(done.into_iter().filter(|(k, _)| nodes.contains(k)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_repo_graph::model::inputs::{HashDecl, Inputs};
    use paddock_repo_graph::model::nodes::{Edge, EdgeKind, Model, ModelNode, NodeKind};
    use std::cell::RefCell;

    /// Git and rustc as fixed answers: object ids by path, the status, the compiler lines.
    struct Fake {
        oids: RefCell<BTreeMap<String, String>>,
        status: RefCell<String>,
        rustc: RefCell<String>,
        calls: RefCell<Vec<String>>,
    }

    impl Fake {
        fn new() -> Fake {
            let oids = [
                ("crates/a", "aaaa"),
                ("crates/b", "bbbb"),
                ("crates/a/.config/nextest.toml", "n111"),
                ("crates/a/paddock.ci.json", "c111"),
                ("ci-image/VERSION", "v111"),
                ("data/table.toml", "d111"),
            ];
            Fake {
                oids: RefCell::new(oids.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()),
                status: RefCell::new(String::new()),
                rustc: RefCell::new("commit-hash: abc\nrelease: 1.100.0\n".into()),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn set(&self, path: &str, oid: &str) {
            self.oids.borrow_mut().insert(path.into(), oid.into());
        }
    }

    impl Exec for Fake {
        fn run(&self, program: &str, args: &[&str], _cwd: &Path, _stdin: Option<&[u8]>) -> Result<String, CliError> {
            self.calls.borrow_mut().push(format!("{program} {}", args.join(" ")));
            Ok(match (program, args.first().copied()) {
                ("git", Some("ls-tree")) => args[3..]
                    .iter()
                    .filter_map(|p| self.oids.borrow().get(*p).map(|o| format!("040000 tree {o}\t{p}\n")))
                    .collect(),
                ("git", Some("status")) => self.status.borrow().clone(),
                (_, Some("-vV")) => self.rustc.borrow().clone(),
                _ => String::new(),
            })
        }
    }

    fn world() -> World {
        let mut model = Model::new();
        model.add_node(ModelNode::new("crate:crates/a", NodeKind::Project, &["crates/a/"]));
        model.add_node(ModelNode::new("crate:crates/b", NodeKind::Project, &["crates/b/"]));
        model.add_node(ModelNode::new("data:table", NodeKind::Doc, &["data/table.toml"]));
        model.add_edge(Edge::new("crate:crates/a", "crate:crates/b", EdgeKind::Depends));
        model.add_edge(Edge::new("crate:crates/a", "data:table", EdgeKind::Reads));
        World {
            inputs: Inputs {
                hash: HashDecl {
                    runner_configs: vec!["**/.config/nextest.toml".into()],
                    contracts: vec!["paddock.ci.json".into()],
                    image_files: vec!["ci-image/VERSION".into()],
                },
                ..Inputs::default()
            },
            files: vec![
                "crates/a/src/lib.rs".into(),
                "crates/a/build.rs".into(),
                "crates/a/.config/nextest.toml".into(),
                "crates/a/paddock.ci.json".into(),
            ],
            model,
            smoke: Default::default(),
            errors: Vec::new(),
        }
    }

    struct Run<'a> {
        fake: &'a Fake,
        env: Vec<(&'a str, &'a str)>,
        contract: &'a str,
        os: &'a str,
    }

    fn hashes(r: &Run, ids: &[&str]) -> BTreeMap<String, NodeHash> {
        let w = world();
        let text = |f: &str| match f {
            "crates/a/build.rs" => Some("println!(\"cargo:rerun-if-env-changed=PADDOCK_SETUP_PRODUCT\");".to_string()),
            "crates/a/paddock.ci.json" => Some(r.contract.to_string()),
            _ => None,
        };
        let tree = Tree {
            files: &w.files,
            text: &text,
        };
        let env = |k: &str| r.env.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
        let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        let tmp = tempfile::tempdir().unwrap();
        let ctx = HashEnv {
            env: &env,
            vars: &[],
            os: r.os,
        };
        node_hashes(r.fake, &ctx, tmp.path(), &w, &tree, &ids).unwrap()
    }

    fn base(fake: &Fake) -> Run<'_> {
        Run {
            fake,
            env: Vec::new(),
            contract: "{\"checks\":[\"cargo test\"]}",
            os: "macos",
        }
    }

    fn hash_a(r: &Run) -> String {
        hashes(r, &["crate:crates/a"])["crate:crates/a"].hash.clone()
    }

    #[test]
    fn the_hash_is_stable_and_every_added_input_moves_it_and_putting_it_back_restores_it() {
        let fake = Fake::new();
        let b = hash_a(&base(&fake));
        assert_eq!(b, hash_a(&base(&fake)), "the same inputs, the same address");
        assert!(b.starts_with("sha256:"));
        // each input, changed then put back (the control: the hash returns to the base)
        fake.set("crates/a", "a222");
        assert_ne!(hash_a(&base(&fake)), b, "the node's own tree");
        fake.set("crates/a", "aaaa");
        fake.set("data/table.toml", "d222");
        assert_ne!(hash_a(&base(&fake)), b, "a file its reads edge names");
        fake.set("data/table.toml", "d111");
        fake.set("crates/a/.config/nextest.toml", "n222");
        assert_ne!(hash_a(&base(&fake)), b, "the nextest configuration");
        fake.set("crates/a/.config/nextest.toml", "n111");
        fake.set("ci-image/VERSION", "v222");
        assert_ne!(hash_a(&base(&fake)), b, "the CI image");
        fake.set("ci-image/VERSION", "v111");
        *fake.rustc.borrow_mut() = "commit-hash: def\nrelease: 1.101.0\n".into();
        assert_ne!(hash_a(&base(&fake)), b, "the compiler");
        *fake.rustc.borrow_mut() = "commit-hash: abc\nrelease: 1.100.0\n".into();
        let mut r = base(&fake);
        r.os = "linux";
        assert_ne!(hash_a(&r), b, "the operating system");
        let mut r = base(&fake);
        r.contract = "{\"checks\":[\"cargo nextest run\"]}";
        assert_ne!(hash_a(&r), b, "a command line of the contract");
        let mut r = base(&fake);
        r.env = vec![("PADDOCK_SETUP_PRODUCT", "dyno")];
        let with_env = hash_a(&r);
        assert_ne!(with_env, b, "a variable the build script declares");
        r.env = vec![("UNRELATED_VARIABLE", "x")];
        assert_eq!(hash_a(&r), b, "a variable no build script reads changes nothing");
        assert_eq!(hash_a(&base(&fake)), b, "everything put back: the base again");
    }

    #[test]
    fn a_dependency_moves_its_dependents_hash_and_not_the_other_way() {
        let fake = Fake::new();
        let both = hashes(&base(&fake), &["crate:crates/a", "crate:crates/b"]);
        fake.set("crates/b", "b222");
        let after = hashes(&base(&fake), &["crate:crates/a", "crate:crates/b"]);
        assert_ne!(both["crate:crates/b"], after["crate:crates/b"]);
        assert_ne!(both["crate:crates/a"], after["crate:crates/a"], "a depends on b");
        fake.set("crates/b", "bbbb");
        fake.set("crates/a", "a222");
        let changed_a = hashes(&base(&fake), &["crate:crates/a", "crate:crates/b"]);
        assert_eq!(both["crate:crates/b"], changed_a["crate:crates/b"], "b does not depend on a");
    }

    #[test]
    fn one_ls_tree_one_status_and_one_compiler_run_serve_every_node_and_dirty_files_are_said() {
        let fake = Fake::new();
        *fake.status.borrow_mut() = " M crates/a/src/lib.rs\n".into();
        let h = hashes(&base(&fake), &["crate:crates/a", "crate:crates/b"]);
        assert!(h["crate:crates/a"].dirty, "a changed file under the node");
        assert!(!h["crate:crates/b"].dirty, "control: the other node is clean");
        let calls = fake.calls.borrow();
        let count = |p: &str| calls.iter().filter(|c| c.contains(p)).count();
        assert_eq!((count("ls-tree"), count("status"), count("-vV")), (1, 1, 1), "{calls:?}");
    }

    #[test]
    fn a_node_the_model_lacks_is_an_error_naming_it() {
        let fake = Fake::new();
        let w = world();
        let text = |_: &str| None;
        let tree = Tree {
            files: &w.files,
            text: &text,
        };
        let tmp = tempfile::tempdir().unwrap();
        let none = |_: &str| None;
        let ctx = HashEnv {
            env: &none,
            vars: &[],
            os: "macos",
        };
        let err = node_hashes(&fake, &ctx, tmp.path(), &w, &tree, &["crate:gone".to_string()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("crate:gone"), "{err}");
    }
}
