//! Local reuse of a gate row's green result (docs/benchmarks.md, "Check-result reuse"; common practice: Bazel's action cache, nx and
//! turborepo's local task cache). The push gate, the member gate (`./dev merge check --gates`, which runs `check --pre-push
//! --member-gate`) and a person's `./dev check --pre-push` run a row; when its inputs hash the same as an earlier green run of that
//! row on this machine, the row is not run again, the table says which run it rests on, and a row whose inputs changed runs.
//!
//! **The key** (SHA-256 over length-prefixed fields): the cache schema; the row id; its program (file name only), arguments and its own
//! variables, with the checkout's path and the build folder left out so a throw-away worktree and the working tree agree; the hash of
//! this `dev` binary; the versions of the programs the row starts (`cargo`, `rustc`, `cargo xwin`, `docker`, `node`); the hashed
//! build variables of [`crate::input_hash::is_hashed_var`] (`RUSTFLAGS`, `CARGO_BUILD_*`, `CARGO_PROFILE_*`, `CARGO_TARGET_*`); and
//! the row's declared inputs ([`Inputs`]) as git tree and blob hashes. A Cargo row's inputs are its workspace, every path dependency
//! cargo names (read from `cargo metadata`, followed through the workspaces they sit in), each of those workspaces' manifest and
//! lockfile, the toolchain pin and the lint and format settings. A repository-wide row (conformance, the register, the audits of C102)
//! keys on the whole tree and on the day (waivers expire by date).
//!
//! **What is stored**: only a green result that ran: a `Clean` row that this run measured (never a reused one, never a known-commit
//! pass, never a failure, "not measured", a broken check or an excused row, ADR 0089), with its verdict, the run it came from and the
//! time. Reading a record checks the schema, the key, the row and the verdict again; anything else is a miss.
//!
//! **Soundness limits, said plainly**: an input is hashed only when git tracks it. A change to an ignored or untracked file
//! (`node_modules`, `ui/dist`) is not seen; a dirty tracked or untracked file under an input makes the row run (and not stored),
//! never reused. The checks never change what they check (ER-45): reading git and starting `--version` probes only; a probe
//! that fails turns reuse off for that row, with the cause printed, and the row runs.
//!
//! **Storage**: `<dev-targets>/check-reuse/<key>.json`, one small JSON file per record, written by temp file and rename, evicted after
//! every run ([`Reuse::evict`]) by age ([`MAX_AGE`]) and count ([`KEEP`]). Never under HOME or `/private/tmp` (ER-14).
//!
//! **Escape**: `--no-reuse` (or `PADDOCK_NO_REUSE=1`, which also reaches the member gate's child through the environment).

use super::exec::Row;
use super::gates::Gate;
use crate::input_hash::is_hashed_var;
use crate::plan_repo::git_text;
use paddock_cli::{CheckId, Outcome, Subject, UnixSeconds, measure};
use paddock_proc::{Limits, command as proc_command, run_bounded};
use paddock_repo_graph::{CargoCli, CargoMetadata};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// The variable that turns reuse off (the flag `--no-reuse` is the same switch).
pub const ENV_OFF: &str = "PADDOCK_NO_REUSE";

/// Bumped when the key or the record changes: an older record is then never read.
const SCHEMA: u32 = 1;

/// Records older than this go ([`Reuse::evict`]): 30 days; a green result of a month-old tree is not one anybody asks for again.
pub const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);

/// Records kept at most (one is about 600 bytes, so the store stays under a few hundred KB).
pub const KEEP: usize = 1000;

/// Programs that report their version for the key (a probe is bounded, and its failure turns reuse off for the row).
const PROBE_LIMIT: Duration = Duration::from_secs(30);

/// Repository-relative paths every Cargo row reads besides its workspace: the cargo config, the toolchain pin, the lint and format
/// settings (AGENTS.md: `.cargo/` and `lint/` mark the Rust builds).
const CARGO_GLOBAL: &[&str] = &[
    ".cargo",
    "rust-toolchain.toml",
    "rustfmt.toml",
    ".rustfmt.toml",
    "clippy.toml",
    ".clippy.toml",
    "lint",
];

/// Per workspace root: its own settings files.
const CARGO_PER_WORKSPACE: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    ".cargo",
    "rust-toolchain.toml",
    "rustfmt.toml",
    ".rustfmt.toml",
    "clippy.toml",
    ".clippy.toml",
];

/// What a row reads, as far as the cache knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Class {
    /// A Cargo row of these workspaces (the Linux Clippy lints several in one container run).
    Cargo(Vec<String>),
    /// A repository-wide row: the whole tree.
    Whole,
}

/// The class of a row, or `None` when its result must never be reused: the commit lint (it reads commits, not the tree), the audits
/// (they read history and the base), the deny rows (the advisory database moves without the tree), the wire fixtures (the row writes
/// files) and any row this module does not know. An unknown row id is never cached.
pub fn class_of(g: &Gate) -> Option<Class> {
    let first = g.id.split([' ', ',']).next().unwrap_or("");
    match first {
        "lockfile" | "fmt" | "clippy" | "clippy-windows" | "doctest" if !g.cwd.is_empty() => Some(Class::Cargo(vec![g.cwd.clone()])),
        "clippy-linux" if !g.workspaces.is_empty() => Some(Class::Cargo(g.workspaces.clone())),
        "conformance" | "conformance-tests" | "gates-register" | "ipc-endpoints" | "pitwall-scripts" => Some(Class::Whole),
        _ => None,
    }
}

/// The paths a row reads, as the key hashes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inputs {
    /// These repository-relative paths (a folder or a file).
    Paths(BTreeSet<String>),
    /// The whole tree.
    Whole,
}

/// The programs a row starts, as the version commands the key runs (one `Vec` per command).
pub fn probes_of(g: &Gate) -> Vec<Vec<String>> {
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    match (class_of(g), g.program.as_str()) {
        (None, _) => Vec::new(),
        (Some(_), "cargo") => {
            let mut v = vec![a(&["cargo", "--version"]), a(&["rustc", "-vV"])];
            if g.args.first().is_some_and(|x| x == "xwin") {
                v.push(a(&["cargo", "xwin", "--version"]));
            }
            v
        }
        (Some(_), "bash") => vec![a(&["docker", "--version"])],
        (Some(_), "node") => vec![a(&["node", "--version"])],
        (Some(_), _) => Vec::new(),
    }
}

/// `args` as the key sees them: the checkout's path is `<root>`, and the build folder (cargo configuration the executor adds, which
/// differs between a throw-away worktree and the working tree and changes no result) is left out.
pub fn normalised_args(args: &[String], root: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip = false;
    for (i, a) in args.iter().enumerate() {
        if skip {
            skip = false;
            continue;
        }
        if a == "--config" && args.get(i + 1).is_some_and(|v| v.starts_with("build.target-dir=")) {
            skip = true;
            continue;
        }
        out.push(if root.is_empty() { a.clone() } else { a.replace(root, "<root>") });
    }
    out
}

/// SHA-256 over length-prefixed fields, as lower-case hex: two field lists differ in the key whenever they differ in any field or in
/// where the fields split.
pub fn digest(fields: &[String]) -> String {
    let mut h = Sha256::new();
    for f in fields {
        h.update((f.len() as u64).to_le_bytes());
        h.update(f.as_bytes());
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The paths a Cargo row reads, from the `cargo metadata --no-deps` JSON of one workspace (`ws`, repository-relative):
/// `(the workspace root, its own paths, the folders of its path dependencies)`. The folders are what the caller follows into their
/// own workspaces.
pub fn cargo_paths(metadata: &str, root: &str, ws: &str) -> Result<(String, BTreeSet<String>, BTreeSet<String>), String> {
    let v: Value = serde_json::from_str(metadata).map_err(|e| format!("cargo metadata for {ws} is not JSON: {e}"))?;
    let root = root.replace('\\', "/");
    let root = root.trim_end_matches('/');
    let rel = |abs: &str| -> Result<String, String> {
        let abs = abs.replace('\\', "/");
        abs.strip_prefix(root)
            .map(|r| r.trim_start_matches('/').to_string())
            .ok_or_else(|| format!("{abs} is outside the repository {root}"))
    };
    let ws_root = rel(v["workspace_root"].as_str().ok_or("cargo metadata has no workspace_root")?)?;
    let join = |dir: &str, name: &str| {
        if dir.is_empty() {
            name.to_string()
        } else {
            format!("{dir}/{name}")
        }
    };
    let mut own: BTreeSet<String> = CARGO_PER_WORKSPACE.iter().map(|n| join(&ws_root, n)).collect();
    own.insert(ws.to_string());
    let mut deps = BTreeSet::new();
    for p in v["packages"].as_array().ok_or("cargo metadata has no packages")? {
        for d in p["dependencies"].as_array().into_iter().flatten() {
            if let Some(path) = d["path"].as_str() {
                deps.insert(rel(path)?);
            }
        }
    }
    Ok((ws_root, own, deps))
}

/// The record of a green run.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// The row.
    pub row: String,
    /// The run it came from (the run's token and the commit's first 12 characters).
    pub run: String,
    /// When it ran, in Unix seconds.
    pub at: u64,
    /// How long it ran, seconds.
    pub secs: f64,
}

impl Record {
    fn json(&self, key: &str, dev: &str, inputs: &[String]) -> Value {
        json!({"schema": SCHEMA, "key": key, "row": self.row, "verdict": "clean", "run": self.run, "at": self.at, "secs": self.secs,
               "dev": dev, "inputs": inputs})
    }

    /// The record out of its file text, when it is a green record for exactly this key and row.
    pub fn parse(text: &str, key: &str, row: &str) -> Option<Record> {
        let v: Value = serde_json::from_str(text).ok()?;
        let ok = v["schema"] == json!(SCHEMA) && v["key"] == json!(key) && v["row"] == json!(row) && v["verdict"] == json!("clean");
        ok.then(|| Record {
            row: row.to_string(),
            run: v["run"].as_str().unwrap_or("?").to_string(),
            at: v["at"].as_u64().unwrap_or(0),
            secs: v["secs"].as_f64().unwrap_or(0.0),
        })
    }
}

/// A key computed before a row ran: the run stores its result under it, so an input changed while it ran is not blessed.
#[derive(Debug, Clone)]
pub struct Prepared {
    key: String,
    inputs: Vec<String>,
}

/// What looking a row up gave.
pub enum Lookup {
    /// The row's green result of an earlier run, for these same inputs.
    Hit(Box<Row>),
    /// Run it; the key to store a green result under, when the row can be cached at all.
    Miss(Option<Prepared>),
}

/// The reuse of one run.
pub struct Reuse {
    root: PathBuf,
    store: PathBuf,
    env: Vec<(String, String)>,
    run: String,
    dev: std::sync::OnceLock<Result<String, String>>,
    probes: Mutex<HashMap<String, Result<String, String>>>,
    graphs: Mutex<HashMap<String, Result<BTreeSet<String>, String>>>,
    whole: Mutex<Option<Result<String, String>>>,
}

impl Reuse {
    /// Reuse for the checkout at `root`, records in `store`, the run's variables `env` and its token `run`. `None` when it is turned off
    /// (`--no-reuse`, [`ENV_OFF`]).
    pub fn new(root: &Path, store: PathBuf, env: &HashMap<String, String>, run: String, off: bool) -> Option<Reuse> {
        if off || env.get(ENV_OFF).is_some_and(|v| !v.trim().is_empty() && v.trim() != "0") {
            return None;
        }
        let mut vars: Vec<(String, String)> = env
            .iter()
            .filter(|(k, v)| is_hashed_var(k) && !v.is_empty())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        vars.sort();
        Some(Reuse {
            root: root.to_path_buf(),
            store,
            env: vars,
            run,
            dev: Default::default(),
            probes: Default::default(),
            graphs: Default::default(),
            whole: Default::default(),
        })
    }

    fn path_of(&self, key: &str) -> PathBuf {
        self.store.join(format!("{key}.json"))
    }

    fn dev_hash(&self) -> Result<String, String> {
        self.dev
            .get_or_init(|| {
                // The running binary itself (a test program is hashed too: what runs the gates is what the key names).
                let exe = std::env::current_exe().map_err(|e| format!("this program cannot be found: {e}"))?;
                let bytes = std::fs::read(&exe).map_err(|e| format!("{} cannot be read: {e}", exe.display()))?;
                Ok(hex_of(&bytes))
            })
            .clone()
    }

    fn probe(&self, cmdline: &[String]) -> Result<String, String> {
        let name = cmdline.join(" ");
        if let Some(r) = self.probes.lock().ok().and_then(|m| m.get(&name).cloned()) {
            return r;
        }
        let mut cmd = proc_command(&cmdline[0]);
        cmd.args(&cmdline[1..]).current_dir(&self.root).env("RUSTUP_AUTO_INSTALL", "0");
        let r = match run_bounded(cmd, Limits::new(PROBE_LIMIT)) {
            Ok(o) if o.success() => Ok(o.stdout_text().trim().to_string()),
            Ok(o) => Err(format!("`{name}` exited with an error: {}", o.stderr_text().trim())),
            Err(e) => Err(format!("`{name}` could not run: {e}")),
        };
        if let Ok(mut m) = self.probes.lock() {
            m.insert(name, r.clone());
        }
        r
    }

    /// The workspace folders and path dependencies a Cargo row of `workspaces` reads, followed through every workspace they sit in.
    fn cargo_inputs(&self, workspaces: &[String]) -> Result<BTreeSet<String>, String> {
        let mut all: BTreeSet<String> = CARGO_GLOBAL.iter().map(|s| s.to_string()).collect();
        for ws in workspaces {
            all.extend(self.graph(ws)?);
        }
        Ok(all)
    }

    fn graph(&self, ws: &str) -> Result<BTreeSet<String>, String> {
        if let Some(r) = self.graphs.lock().ok().and_then(|m| m.get(ws).cloned()) {
            return r;
        }
        let r = self.graph_of(ws);
        if let Ok(mut m) = self.graphs.lock() {
            m.insert(ws.to_string(), r.clone());
        }
        r
    }

    fn graph_of(&self, ws: &str) -> Result<BTreeSet<String>, String> {
        let cli = CargoCli::new(&self.root, Duration::from_secs(120));
        let root = self.root.to_string_lossy().to_string();
        let mut out = BTreeSet::new();
        let mut done: BTreeSet<String> = BTreeSet::new();
        let mut todo = vec![ws.to_string()];
        while let Some(dir) = todo.pop() {
            // A folder inside a workspace already read is covered by that read (its members are in the workspace folder, which is hashed).
            if done.iter().any(|d| d.is_empty() || dir == *d || dir.starts_with(&format!("{d}/"))) {
                out.insert(dir);
                continue;
            }
            let json = cli
                .metadata(&format!("{dir}/Cargo.toml"))
                .map_err(|e| format!("cargo metadata for {dir} failed: {e}"))?;
            let (ws_root, own, deps) = cargo_paths(&json, &root, &dir)?;
            out.extend(own);
            out.insert(ws_root.clone());
            done.insert(ws_root);
            todo.extend(deps);
        }
        Ok(out)
    }

    fn whole_tree(&self) -> Result<String, String> {
        let mut g = self.whole.lock().map_err(|_| "poisoned".to_string())?;
        if g.is_none() {
            *g = Some(self.read_whole());
        }
        g.clone().unwrap_or_else(|| Err("not read".into()))
    }

    fn read_whole(&self) -> Result<String, String> {
        let dirty = git_text(&self.root, &["status", "--porcelain", "--untracked-files=all"]).ok_or("git status failed")?;
        if !dirty.trim().is_empty() {
            return Err("the tree has uncommitted changes".into());
        }
        git_text(&self.root, &["rev-parse", "HEAD^{tree}"])
            .map(|t| t.trim().to_string())
            .ok_or_else(|| "git rev-parse failed".into())
    }

    /// `path hash` lines of `paths` at HEAD, sorted (a folder as its entries); a path with no entry is `absent`. An error when a path has uncommitted changes.
    fn hash_paths(&self, paths: &BTreeSet<String>) -> Result<Vec<String>, String> {
        let list: Vec<&str> = paths.iter().map(String::as_str).collect();
        let mut st = vec!["status", "--porcelain", "--untracked-files=all", "--"];
        st.extend(&list);
        let dirty = git_text(&self.root, &st).ok_or("git status failed")?;
        if !dirty.trim().is_empty() {
            let first = dirty.lines().next().unwrap_or("").trim();
            return Err(format!("an input has uncommitted changes ({first})"));
        }
        let mut ls = vec!["ls-tree", "HEAD", "--"];
        ls.extend(&list);
        let out = git_text(&self.root, &ls).ok_or("git ls-tree failed")?;
        // `ls-tree` of a folder lists its entries, a subfolder by its tree hash, which covers everything below it; of a file, its blob.
        // Every entry line is kept, so any change under an input changes the key.
        let mut lines: Vec<String> = Vec::new();
        for l in out.lines() {
            // `<mode> <type> <hash>\t<path>`
            if let Some((meta, path)) = l.split_once('\t') {
                lines.push(format!("{path} {}", meta.split_whitespace().nth(2).unwrap_or("")));
            }
        }
        for p in paths {
            let below = format!("{p}/");
            if !lines
                .iter()
                .any(|l| l.split(' ').next().is_some_and(|f| f == p || f.starts_with(&below)))
            {
                lines.push(format!("{p} absent"));
            }
        }
        lines.sort();
        Ok(lines)
    }

    /// The key of `g`, or why it cannot be reused (it is run, and not stored).
    pub fn prepare(&self, g: &Gate) -> Result<Prepared, String> {
        let class = class_of(g).ok_or("this row is never reused (it reads more than the tree, or its result is not stable)")?;
        let root = self.root.to_string_lossy().to_string();
        let mut f: Vec<String> = vec![format!("schema {SCHEMA}"), format!("row {}", g.id), format!("cwd {}", g.cwd)];
        f.push(format!(
            "program {}",
            Path::new(&g.program)
                .file_name()
                .map_or(g.program.clone(), |n| n.to_string_lossy().to_string())
        ));
        f.extend(normalised_args(&g.args, &root).into_iter().map(|a| format!("arg {a}")));
        let mut own: Vec<String> = g
            .env
            .iter()
            .filter(|(k, _)| k != crate::ci_run::GATE_TARGET_DIR)
            .map(|(k, v)| format!("gate-env {k}={}", v.replace(&root, "<root>")))
            .collect();
        own.sort();
        f.extend(own);
        f.push(format!("dev {}", self.dev_hash()?));
        for p in probes_of(g) {
            f.push(format!("tool {} = {}", p.join(" "), self.probe(&p)?));
        }
        f.extend(self.env.iter().map(|(k, v)| format!("env {k}={v}")));
        let inputs = match &class {
            Class::Cargo(ws) => Inputs::Paths(self.cargo_inputs(ws)?),
            Class::Whole => Inputs::Whole,
        };
        let lines = match inputs {
            Inputs::Paths(mut paths) => {
                if g.id.starts_with("clippy-linux") {
                    // The container script and the image it runs in are read by this row alone.
                    paths.extend(["tools/linux-container.sh".to_string(), "ci-image".to_string()]);
                }
                self.hash_paths(&paths)?
            }
            Inputs::Whole => vec![format!("tree {}", self.whole_tree()?), format!("day {}", unix_now() / 86_400)],
        };
        f.extend(lines.iter().map(|l| format!("input {l}")));
        Ok(Prepared {
            key: digest(&f),
            inputs: lines,
        })
    }

    /// Looks `g` up: a hit is a `Clean` row for `subject` that says which run it rests on; the line naming that run is printed.
    pub fn lookup(&self, g: &Gate, subject: &Subject) -> Lookup {
        let prep = match self.prepare(g) {
            Ok(p) => p,
            Err(why) => {
                if class_of(g).is_some() {
                    eprintln!("pre-push: {}: not reused ({why}); running it", g.id);
                }
                return Lookup::Miss(None);
            }
        };
        let hit = std::fs::read_to_string(self.path_of(&prep.key))
            .ok()
            .and_then(|t| Record::parse(&t, &prep.key, &g.id));
        match hit {
            Some(rec) => {
                let note = format!(
                    "reused the green result of run {} ({} ago, it took {:.1} s): the inputs hash the same; --no-reuse runs it again",
                    rec.run,
                    ago(unix_now().saturating_sub(rec.at)),
                    rec.secs
                );
                eprintln!("pre-push: {}: {note}", g.id);
                Lookup::Hit(Box::new(Row::reused(&g.id, subject, note)))
            }
            None => Lookup::Miss(Some(prep)),
        }
    }

    /// Stores `row` (decided by a real run) under the key made before it ran, when and only when it is a clean pass.
    pub fn store(&self, g: &Gate, prep: &Prepared, row: &Row) {
        if !row.is_fresh_pass() {
            return;
        }
        let rec = Record {
            row: g.id.clone(),
            run: self.run.clone(),
            at: unix_now(),
            secs: row.secs,
        };
        let text = rec.json(&prep.key, &self.dev_hash().unwrap_or_default(), &prep.inputs).to_string();
        if let Err(e) = write_atomic(&self.path_of(&prep.key), &text) {
            eprintln!("::warning::pre-push: the result of {} was not stored for reuse: {e}", g.id);
        }
    }

    /// Drops records older than [`MAX_AGE`] and all but the newest [`KEEP`]; returns how many went.
    pub fn evict(&self) -> usize {
        evict(&self.store, std::time::SystemTime::now(), MAX_AGE, KEEP)
    }
}

fn hex_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn unix_now() -> u64 {
    UnixSeconds::now().0
}

fn ago(secs: u64) -> String {
    match secs {
        s if s < 90 => format!("{s} s"),
        s if s < 5400 => format!("{} min", s / 60),
        s if s < 172_800 => format!("{} h", s / 3600),
        s => format!("{} days", s / 86_400),
    }
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".tmp-{}-{}", std::process::id(), unix_now()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Evicts by age and count from `store`; `.tmp-*` files of crashed writers go with the old ones. Returns how many files went.
pub fn evict(store: &Path, now: std::time::SystemTime, max_age: Duration, keep: usize) -> usize {
    let Ok(rd) = std::fs::read_dir(store) else { return 0 };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .filter(|(_, p)| {
            p.extension().is_some_and(|x| x == "json") || p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(".tmp-"))
        })
        .collect();
    files.sort_by_key(|(m, _)| std::cmp::Reverse(*m));
    let mut gone = 0;
    for (i, (m, p)) in files.iter().enumerate() {
        let old = now.duration_since(*m).is_ok_and(|d| d > max_age);
        if (old || i >= keep) && std::fs::remove_file(p).is_ok() {
            gone += 1;
        }
    }
    gone
}

/// The clean outcome of a reused row: made by [`measure`] for `subject` like every pass, with `CheckId` `pre-push:<id>`.
pub(super) fn clean_for(id: &str, subject: &Subject) -> Outcome {
    measure(
        CheckId::new(format!("pre-push:{id}")),
        subject.clone(),
        super::exec::tool(),
        UnixSeconds::now(),
        || Ok(Vec::new()),
    )
}

#[cfg(test)]
#[path = "reuse_tests.rs"]
mod tests;
