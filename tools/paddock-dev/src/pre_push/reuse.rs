//! Reuse of a gate row's green result (common practice: Bazel's action cache, nx and turborepo's local task cache). An agent's own
//! `./dev check --pre-push` pass is not checked again when the same inputs are pushed (the hook) or member-gated
//! (`./dev merge check --gates`, which runs `check --pre-push --member-gate`): a row whose key matches an earlier green run of that
//! row on this machine is not run, and the table says `reused (green at <time>)`.
//!
//! **The key** (SHA-256 over length-prefixed fields) is the row's input hash plus the tool version:
//! - the row: its id, folder, program (by file name; this binary as `<dev>`, since it runs through a held link whose name carries the
//!   run's pid), its arguments with the checkout's path as `<root>` and the build folder left out (a throw-away worktree and the
//!   working tree agree), and its own variables;
//! - the tool version: the hash of this `dev` binary's bytes, and the versions of the programs the row starts (`cargo --version`,
//!   `rustc -vV`, `cargo xwin --version`, `docker --version`, `node --version`);
//! - the inputs, as git object ids at HEAD: a Cargo row's workspace folder, the folders of every path dependency the planner's Cargo
//!   graph names (followed transitively), each of those workspaces' manifest, lockfile and settings, the toolchain pin and the lint
//!   settings, plus the hashed build variables ([`crate::input_hash::is_hashed_var`]); a repository-wide row (conformance and the
//!   other rows of [`class_of`]) keys on the whole tree, the base it compares with (`PADDOCK_BASE_REF` or `origin/main`, resolved), the
//!   CI markers it reads and the day (waivers expire by date).
//!
//! **What is stored**: only a green result that ran: a `Clean` row this run measured (never a reused row, a failure, "not measured",
//! a broken check, an excused row or a skipped one, inactive rows included: ADR 0089). Rows that are never [`Planned::Run`] (skipped,
//! inactive, not measured at planning) never reach the lookup at all. Reading a record checks the schema, the key, the row and the
//! verdict again; anything else is a miss.
//!
//! **Soundness limits, said plainly**: an input counts only when git tracks it. A change to an ignored or untracked file
//! (`node_modules`, `ui/dist`) is not seen; a tracked input with uncommitted changes (or any untracked file under it) makes the row
//! run and not be stored. A row this module does not know is never reused. A probe that fails turns reuse off for that row (it
//! runs, with the cause printed). Probes never change what they check (ER-45: `RUSTUP_AUTO_INSTALL=0`).
//!
//! **Storage**: `<git common dir>/paddock-pre-push/reuse/<key>.json`, beside the gate's step history (`step-history.jsonl`) and
//! pass records, so every worktree of the repository sees it. One small file per record, written by temp file and rename, evicted
//! after every run by age ([`MAX_AGE`]) and count ([`KEEP`]).
//!
//! **Switch**: `--no-reuse`, or `PADDOCK_NO_REUSE=1` (which also reaches the member gate's child through the environment).

use super::exec::Row;
use super::gates::Gate;
use crate::input_hash::is_hashed_var;
use crate::plan_repo::git_text;
use paddock_proc::{Limits, command as proc_command, run_bounded};
use paddock_repo_graph::Graph;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// The variable that turns reuse off (the flag `--no-reuse` is the same switch).
pub const ENV_OFF: &str = "PADDOCK_NO_REUSE";

/// The folder of the records, in the pre-push state folder.
pub const DIR: &str = "reuse";

/// Bumped when the key or the record changes: an older record is then never read.
const SCHEMA: u32 = 1;

/// Records older than this go: 30 days; a green result of a month-old tree is not one anybody asks for again.
pub const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);

/// Records kept at most (one is under 1 KB, so the store stays under about a megabyte).
pub const KEEP: usize = 1000;

/// A version probe is bounded; a probe that does not answer turns reuse off for its row.
const PROBE_LIMIT: Duration = Duration::from_secs(30);

/// What every Cargo row reads besides its workspaces: the cargo configuration, the toolchain pin, the format and lint settings
/// (AGENTS.md: `.cargo/` and `lint/` mark the Rust builds).
const CARGO_GLOBAL: &[&str] = &[
    ".cargo",
    "rust-toolchain.toml",
    "rustfmt.toml",
    ".rustfmt.toml",
    "clippy.toml",
    ".clippy.toml",
    "lint",
];

/// Per workspace root: its own settings files (a dependency's workspace contributes these, not its whole folder).
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

/// The variables a repository-wide row reads besides the tree (conformance's base ref and its CI markers).
const WHOLE_VARS: &[&str] = &["PADDOCK_BASE_REF", "CI", "GITEA_ACTIONS", "GITHUB_ACTIONS"];

/// What a row reads, as far as reuse knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Class {
    /// A Cargo row of these workspaces (the Linux Clippy lints several in one container run).
    Cargo(Vec<String>),
    /// A repository-wide row: the whole tree and the base.
    Whole,
}

/// The class of a row, or `None` when its result is never reused: the commit lint (it reads commits), the audits and the locks
/// guard (history and the base's lockfiles), the deny row (the advisory database moves without the tree), the wire fixtures (the row
/// writes files) and any row this module does not know.
pub fn class_of(g: &Gate) -> Option<Class> {
    let first = g.id.split([' ', ',']).next().unwrap_or("");
    match first {
        "lockfile" | "fmt" | "clippy" | "clippy-windows" if !g.cwd.is_empty() => Some(Class::Cargo(vec![g.cwd.clone()])),
        "clippy-linux" if !g.workspaces.is_empty() => Some(Class::Cargo(g.workspaces.clone())),
        "conformance" | "conformance-tests" | "gates-register" | "pitwall-scripts" => Some(Class::Whole),
        _ => None,
    }
}

/// The version commands of the programs a row starts (one `Vec` per command).
pub fn probes_of(g: &Gate) -> Vec<Vec<String>> {
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if class_of(g).is_none() {
        return Vec::new();
    }
    match g.program.as_str() {
        p if crate::cargo_target::is_cargo(p) => {
            let mut v = vec![a(&["cargo", "--version"]), a(&["rustc", "-vV"])];
            if g.args.first().is_some_and(|x| x == "xwin") {
                v.push(a(&["cargo", "xwin", "--version"]));
            }
            v
        }
        "bash" => vec![a(&["docker", "--version"])],
        "node" => vec![a(&["node", "--version"])],
        _ => Vec::new(),
    }
}

/// `args` as the key sees them: the checkout's path is `<root>`, and the build folder (`--config build.target-dir=...`, which
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

/// The paths a Cargo row of `workspaces` reads, from the planner's Cargo graph: each workspace folder, every crate folder its crates
/// depend on by path (transitively), each such crate's workspace settings, and [`CARGO_GLOBAL`]. `Err` when a workspace has no
/// crate in the graph (the key cannot be made, so the row runs).
pub fn cargo_inputs(graph: &Graph, workspaces: &[String]) -> Result<BTreeSet<String>, String> {
    let join = |dir: &str, name: &str| {
        if dir.is_empty() {
            name.to_string()
        } else {
            format!("{dir}/{name}")
        }
    };
    let mut out: BTreeSet<String> = CARGO_GLOBAL.iter().map(|s| s.to_string()).collect();
    let mut todo: Vec<String> = Vec::new();
    for ws in workspaces {
        let members: Vec<String> = graph
            .iter()
            .filter(|(_, n)| n.workspace == *ws)
            .map(|(d, _)| d.to_string())
            .collect();
        if members.is_empty() {
            return Err(format!("the Cargo graph has no crate in the workspace {ws}"));
        }
        out.insert(ws.clone());
        out.extend(CARGO_PER_WORKSPACE.iter().map(|n| join(ws, n)));
        todo.extend(members);
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    while let Some(dir) = todo.pop() {
        if !seen.insert(dir.clone()) {
            continue;
        }
        let node = graph.get(&dir).ok_or_else(|| format!("the Cargo graph has no crate at {dir}"))?;
        out.insert(dir.clone());
        out.extend(CARGO_PER_WORKSPACE.iter().map(|n| join(&node.workspace, n)));
        todo.extend(node.deps.iter().filter(|d| graph.contains(d)).cloned());
    }
    Ok(out)
}

/// SHA-256 over length-prefixed fields, as lower-case hex: two field lists differ in the key whenever they differ in any field or in
/// where the fields split.
pub fn digest(fields: &[String]) -> String {
    let mut h = Sha256::new();
    for f in fields {
        h.update((f.len() as u64).to_le_bytes());
        h.update(f.as_bytes());
    }
    hex(&h.finalize())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// When and in which run a reused row was green.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reused {
    /// When it ran, in Unix seconds.
    pub at: u64,
    /// The run it came from (the run's token).
    pub run: String,
}

impl Reused {
    /// The time it was green, as RFC 3339 in UTC.
    pub fn time(&self) -> String {
        i64::try_from(self.at)
            .ok()
            .and_then(|s| jiff::Timestamp::from_second(s).ok())
            .map_or_else(|| format!("unix {}", self.at), |t| t.to_string())
    }

    /// What the table says: `reused (green at <time>)`.
    pub fn text(&self) -> String {
        format!("reused (green at {})", self.time())
    }
}

/// The record of a green run, as its file holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// The row.
    pub row: String,
    /// When and where it was green.
    pub green: Reused,
    /// How long it ran, seconds.
    pub secs: f64,
}

impl Record {
    fn json(&self, key: &str, inputs: &[String]) -> Value {
        json!({"schema": SCHEMA, "key": key, "row": self.row, "verdict": "clean", "run": self.green.run, "at": self.green.at,
               "secs": self.secs, "inputs": inputs})
    }

    /// The record out of its file text, when it is a green record for exactly this key and row.
    pub fn parse(text: &str, key: &str, row: &str) -> Option<Record> {
        let v: Value = serde_json::from_str(text).ok()?;
        let ok = v["schema"] == json!(SCHEMA) && v["key"] == json!(key) && v["row"] == json!(row) && v["verdict"] == json!("clean");
        ok.then(|| Record {
            row: row.to_string(),
            green: Reused {
                run: v["run"].as_str().unwrap_or("?").to_string(),
                at: v["at"].as_u64().unwrap_or(0),
            },
            secs: v["secs"].as_f64().unwrap_or(0.0),
        })
    }
}

/// The key of a row, made before it runs: the run stores its result under it, so an input changed while it ran is not blessed.
#[derive(Debug, Clone)]
pub struct Prepared {
    key: String,
    inputs: Vec<String>,
}

/// What looking a row up gave.
#[derive(Debug)]
pub enum Lookup {
    /// The row was green in an earlier run with this same key.
    Hit(Reused),
    /// Run it; the key to store a green result under, or `None` when the row is never stored.
    Miss(Option<Prepared>),
}

/// This program as the key names it: the path its rows start it by (left out of the key) and the hash of its bytes (in the key).
#[derive(Debug, Clone)]
pub struct Dev {
    /// The path the gates start it by.
    pub path: String,
    /// The tool version: SHA-256 of the binary's bytes.
    pub hash: String,
}

impl Dev {
    /// The binary at `path`, hashed.
    pub fn of(path: &Path) -> Result<Dev, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{} cannot be read: {e}", path.display()))?;
        Ok(Dev {
            path: path.to_string_lossy().to_string(),
            hash: hex(&Sha256::digest(&bytes)),
        })
    }
}

/// The reuse of one run.
pub struct Reuse {
    root: PathBuf,
    store: PathBuf,
    run: String,
    dev: Result<Dev, String>,
    cargo: Graph,
    build_vars: Vec<String>,
    base_ref: String,
    whole_vars: Vec<String>,
    probes: Mutex<HashMap<String, Result<String, String>>>,
    whole: OnceLock<Result<Vec<String>, String>>,
}

impl Reuse {
    /// Reuse for the checkout at `root` (the one the gates run in), records in `store`, the run's variables `env`, its token `run`,
    /// this program `dev` ([`Dev::of`]; an error turns reuse off for every row, with the cause) and the planner's Cargo graph. `None`
    /// when it is turned off (`off`: `--no-reuse`; or [`ENV_OFF`] set to anything but empty or `0`).
    pub fn new(
        root: &Path,
        store: PathBuf,
        env: &HashMap<String, String>,
        run: String,
        dev: Result<Dev, String>,
        cargo: Graph,
        off: bool,
    ) -> Option<Reuse> {
        if off || env.get(ENV_OFF).is_some_and(|v| !v.trim().is_empty() && v.trim() != "0") {
            return None;
        }
        let mut build_vars: Vec<String> = env
            .iter()
            .filter(|(k, v)| is_hashed_var(k) && !v.is_empty())
            .map(|(k, v)| format!("env {k}={v}"))
            .collect();
        build_vars.sort();
        let whole_vars = WHOLE_VARS
            .iter()
            .map(|k| format!("env {k}={}", env.get(*k).map_or("", String::as_str)))
            .collect();
        let base_ref = env
            .get("PADDOCK_BASE_REF")
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .unwrap_or("origin/main")
            .to_string();
        Some(Reuse {
            root: root.to_path_buf(),
            store,
            run,
            dev,
            cargo,
            build_vars,
            base_ref,
            whole_vars,
            probes: Default::default(),
            whole: OnceLock::new(),
        })
    }

    fn path_of(&self, key: &str) -> PathBuf {
        self.store.join(format!("{key}.json"))
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

    /// The key lines of a repository-wide row: the tree (clean only), the base it compares with, the CI markers and the day.
    fn whole_lines(&self) -> Result<Vec<String>, String> {
        self.whole
            .get_or_init(|| {
                let dirty = git_text(&self.root, &["status", "--porcelain", "--untracked-files=all"]).ok_or("git status failed")?;
                if !dirty.trim().is_empty() {
                    return Err("the tree has uncommitted changes".to_string());
                }
                let tree = git_text(&self.root, &["rev-parse", "HEAD^{tree}"]).ok_or("git rev-parse HEAD^{tree} failed")?;
                let base_ref = &self.base_ref;
                let base = git_text(&self.root, &["rev-parse", "--verify", "--quiet", &format!("{base_ref}^{{commit}}")])
                    .map_or_else(|| "absent".to_string(), |t| t.trim().to_string());
                let mut lines = vec![
                    format!("tree {}", tree.trim()),
                    format!("base {base_ref} {base}"),
                    format!("day {}", paddock_cli::UnixSeconds::now().0 / 86_400),
                ];
                lines.extend(self.whole_vars.iter().cloned());
                Ok(lines)
            })
            .clone()
    }

    /// `path id` lines of `paths` at HEAD, sorted (a folder as its tree id); a path git does not track is `absent`. `Err` when a path
    /// has uncommitted changes.
    fn hash_paths(&self, paths: &BTreeSet<String>) -> Result<Vec<String>, String> {
        let list: Vec<&str> = paths.iter().map(String::as_str).collect();
        let mut st = vec!["status", "--porcelain", "--untracked-files=all", "--"];
        st.extend(&list);
        let dirty = git_text(&self.root, &st).ok_or("git status failed")?;
        if let Some(first) = dirty.lines().find(|l| !l.trim().is_empty()) {
            return Err(format!("an input has uncommitted changes ({})", first.trim()));
        }
        let mut ls = vec!["ls-tree", "HEAD", "--"];
        ls.extend(&list);
        let out = git_text(&self.root, &ls).ok_or("git ls-tree failed")?;
        // `<mode> <type> <id>\t<path>`: a folder is listed by its tree id, which covers everything below it.
        let mut lines: Vec<String> = Vec::new();
        let mut found = BTreeSet::new();
        for l in out.lines() {
            if let Some((meta, path)) = l.split_once('\t') {
                lines.push(format!("{path} {}", meta.split_whitespace().nth(2).unwrap_or("")));
                found.insert(path.to_string());
            }
        }
        lines.extend(paths.iter().filter(|p| !found.contains(*p)).map(|p| format!("{p} absent")));
        lines.sort();
        Ok(lines)
    }

    /// The key of `g`, or why it cannot be reused (it runs, and is not stored).
    pub fn prepare(&self, g: &Gate) -> Result<Prepared, String> {
        let class = class_of(g).ok_or("this row is never reused (it reads more than the tree, or its result is not stable)")?;
        let dev = self.dev.as_ref().map_err(|e| format!("this program's version is not known: {e}"))?;
        let root = self.root.to_string_lossy().to_string();
        let mut f: Vec<String> = vec![format!("schema {SCHEMA}"), format!("row {}", g.id), format!("cwd {}", g.cwd)];
        let program = if g.program == dev.path {
            "<dev>".to_string()
        } else {
            Path::new(&g.program)
                .file_name()
                .map_or(g.program.clone(), |n| n.to_string_lossy().to_string())
        };
        f.push(format!("program {program}"));
        f.extend(normalised_args(&g.args, &root).into_iter().map(|a| format!("arg {a}")));
        let mut own: Vec<String> = g
            .env
            .iter()
            .filter(|(k, _)| k != crate::ci_run::GATE_TARGET_DIR)
            .map(|(k, v)| format!("gate-env {k}={}", v.replace(&root, "<root>")))
            .collect();
        own.sort();
        f.extend(own);
        f.push(format!("tool dev {}", dev.hash));
        for p in probes_of(g) {
            f.push(format!("tool {} = {}", p.join(" "), self.probe(&p)?));
        }
        let lines = match class {
            Class::Cargo(ws) => {
                let mut paths = cargo_inputs(&self.cargo, &ws)?;
                if g.id.starts_with("clippy-linux") {
                    // The container script and the image it runs in are read by this row alone.
                    paths.extend(["tools/linux-container.sh".to_string(), "ci-image".to_string()]);
                }
                let mut l = self.hash_paths(&paths)?;
                l.extend(self.build_vars.iter().cloned());
                l
            }
            Class::Whole => self.whole_lines()?,
        };
        f.extend(lines.iter().map(|l| format!("input {l}")));
        Ok(Prepared {
            key: digest(&f),
            inputs: lines,
        })
    }

    /// Looks `g` up. A row that cannot be keyed is a miss with no key (it runs and is not stored), and the cause is printed.
    pub fn lookup(&self, g: &Gate) -> Lookup {
        let prep = match self.prepare(g) {
            Ok(p) => p,
            Err(why) => {
                if class_of(g).is_some() {
                    eprintln!("pre-push: {}: not reused ({why}); running it", g.id);
                }
                return Lookup::Miss(None);
            }
        };
        match std::fs::read_to_string(self.path_of(&prep.key))
            .ok()
            .and_then(|t| Record::parse(&t, &prep.key, &g.id))
        {
            Some(rec) => Lookup::Hit(rec.green),
            None => Lookup::Miss(Some(prep)),
        }
    }

    /// Stores `row` (decided by a real run) under the key made before it ran, when and only when it is a fresh clean pass.
    pub fn store(&self, g: &Gate, prep: &Prepared, row: &Row) {
        if !row.is_fresh_pass() {
            return;
        }
        let rec = Record {
            row: g.id.clone(),
            green: Reused {
                at: paddock_cli::UnixSeconds::now().0,
                run: self.run.clone(),
            },
            secs: row.secs,
        };
        if let Err(e) = write_atomic(&self.path_of(&prep.key), &rec.json(&prep.key, &prep.inputs).to_string()) {
            eprintln!("::warning::pre-push: the green result of {} was not stored for reuse: {e}", g.id);
        }
    }

    /// Drops records older than [`MAX_AGE`] and all but the newest [`KEEP`]; returns how many went.
    pub fn evict(&self) -> usize {
        evict(&self.store, std::time::SystemTime::now(), MAX_AGE, KEEP)
    }
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".tmp-{}-{}", std::process::id(), paddock_cli::UnixSeconds::now().0));
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

#[cfg(test)]
#[path = "reuse_tests.rs"]
mod tests;
