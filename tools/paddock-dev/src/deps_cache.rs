//! The shared, content-keyed cache for `npm ci` and the `ui/dist` build: `./dev deps install` and `./dev deps ui`.
//!
//! Every worktree and train used to reinstall `node_modules` and rebuild `ui/dist` from nothing. Here each result is built once
//! per key and every later worktree gets a fast copy-on-write clone of it (APFS `clonefile`, a reflink where the file system has
//! one, else a plain copy). A clone is a real folder of its own: never a symlink into the cache and never a shared mutable
//! folder, so Angular, Vitest and anything that resolves real paths see an ordinary install, and nothing one worktree does reaches
//! another.
//!
//! Keys (the content of the inputs, never a time):
//!  - `node_modules`: the hash of `package-lock.json`, the Node version and the platform (OS and architecture), because the
//!    installed native binaries (esbuild, rollup) depend on both ([`node_modules_key`]);
//!  - `ui/dist` and the generated css: the content hashes the e2e build guard already computes (`sourceHash` of
//!    `@decryp7/paddock-e2e`'s `build-guard.ts`, asked from Node, not re-implemented here: ER-22), the `node_modules` key and
//!    the hash of the other files a build reads ([`ui_key`]).
//!
//! The cache is `<dev-targets>/cache/npm/` (ER-51):
//!  - owner: this tool; `owner.json` there says so, with the cap and the eviction rule;
//!  - one kernel file lock per key (`<name>.lock`, `std::fs::File::lock`) held for the whole use: exclusive while an entry is
//!    built, stored and cloned, shared while one is only cloned out, so a build is never cloned half made and an eviction never
//!    removes an entry being cloned. A second builder of the same key waits for the lock (bounded, ER-32), then hits;
//!  - an entry is `<name>/entry.json` plus `<name>/payload/<repo-relative folders>`, made in a staging folder and renamed into
//!    place, so there is never a half-written entry under its name. `entry.json` records the file count and the folders; a hit
//!    checks both, and an entry that fails the check is removed and rebuilt (never trusted, ER-33);
//!  - size: [`DEFAULT_CAP_GIB`] GiB (`PADDOCK_DEPS_CACHE_MAX_GIB` overrides it); when the entries pass it, the least recently used
//!    ones go first (their folder's modification time, set on every hit by [`crate::tool_cache::touch`]), never the entry just used
//!    and never one whose lock is held.
//!
//! Every deletion goes through [`crate::safe_remove::remove_under`].

use crate::ci_tool::{NETWORK_ENV, Output, ToolCall};
use crate::safe_remove::remove_under;
use clap::{Args, Subcommand};
use paddock_cli::{CliError, Report};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The size cap of the cache in GiB. Measured 2026-11-26 (docs/benchmarks.md, "Shared npm and ui/dist cache"): one `node_modules`
/// entry and one `ui/dist` entry are each well under 1 GiB, and a lockfile or ui change makes a new pair, so 10 GiB holds
/// about a week of changes before the least recently used go.
pub const DEFAULT_CAP_GIB: u64 = 10;
/// The variable that overrides [`DEFAULT_CAP_GIB`].
pub const CAP_ENV: &str = "PADDOCK_DEPS_CACHE_MAX_GIB";

/// How long `npm ci` may run: a hang detector (ER-12), set from the measurement in docs/benchmarks.md with a wide margin.
const NPM_CI_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// How long `npm run build -w ui` may run (same reason).
const UI_BUILD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// How long a short helper (`node --version`, the hash, a clone) may run.
const HELPER_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// How often a waiter looks at a held lock again.
const LOCK_POLL: Duration = Duration::from_millis(100);

/// The cache folder below the dev-targets folder.
pub fn cache_root(dev_targets: &Path) -> PathBuf {
    dev_targets.join("cache").join("npm")
}

/// The entry name of the `node_modules` of this lockfile on this Node and platform. Pure: the inputs are passed in.
pub fn node_modules_key(lock: &[u8], node_version: &str, os: &str, arch: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"node-modules\0");
    h.update(lock);
    h.update(format!("\0node={node_version}\0os={os}\0arch={arch}\0"));
    format!("node-modules-{}", &hex(&h.finalize())[..32])
}

/// The entry name of the `ui/dist` and css build: the two source hashes of the build guard, the `node_modules` entry it was built
/// with and the hash of the other files the build reads. Pure.
pub fn ui_key(dist_hash: &str, css_hash: &str, deps_name: &str, other_inputs: &str) -> String {
    let mut h = Sha256::new();
    h.update(format!("ui-dist\0{dist_hash}\0{css_hash}\0{deps_name}\0{other_inputs}\0"));
    format!("ui-dist-{}", &hex(&h.finalize())[..32])
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// What [`Cache::fetch`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fetched {
    /// The entry existed and was cloned into the tree: nothing was built.
    Hit,
    /// There was no usable entry: it was built in the tree and stored.
    Built,
}

/// Files, relative to the tree root, that a complete result always has: how a stored entry is told from a damaged one. The folders
/// of a result are not named here: the build says which it made, and the entry records them.
pub type Markers<'a> = &'a [&'a str];

/// The cache folder and its size cap.
pub struct Cache {
    root: PathBuf,
    cap_bytes: u64,
    /// The longest a waiter waits for a lock another holds (ER-32).
    wait: Duration,
}

impl Cache {
    /// The cache at `root` with a cap of `cap_gib` GiB (see [`cap_gib`]).
    pub fn new(root: PathBuf, cap_gib: u64, wait: Duration) -> Cache {
        Cache {
            root,
            cap_bytes: cap_gib.saturating_mul(1 << 30),
            wait,
        }
    }

    /// The same with a cap in bytes (tests).
    pub fn with_cap_bytes(root: PathBuf, cap_bytes: u64, wait: Duration) -> Cache {
        Cache { root, cap_bytes, wait }
    }

    fn entry_dir(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn lock_path(&self, name: &str) -> PathBuf {
        self.root.join(format!("{name}.lock"))
    }

    /// Makes the folder and its `owner.json` (the owner, the cap and the eviction rule, ER-51) once.
    fn prepare(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.root)
            .map_err(|e| format!("cannot make {}: {e}. Next: make the folder writable", self.root.display()))?;
        let owner = self.root.join("owner.json");
        if !owner.exists() {
            let text = json!({
                "owner": "./dev deps (tools/paddock-dev/src/deps_cache.rs)",
                "what": "content-keyed copies of node_modules and ui/dist; worktrees get copy-on-write clones",
                "cap_bytes": self.cap_bytes,
                "eviction": "least recently used entry first (folder modification time); never the entry just used or one whose lock is held",
            });
            // A concurrent first run writes the same text; a failure here is only a missing note.
            let _ = std::fs::write(&owner, format!("{text:#}\n"));
        }
        Ok(())
    }

    /// Opens (creating) the lock file of `name` and takes it, exclusive or shared, waiting at most `self.wait`.
    fn lock(&self, name: &str, exclusive: bool) -> Result<File, String> {
        let path = self.lock_path(name);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        let deadline = Instant::now() + self.wait;
        loop {
            let got = if exclusive { file.try_lock() } else { file.try_lock_shared() };
            match got {
                Ok(()) => return Ok(file),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => std::thread::sleep(LOCK_POLL),
                Err(TryLockError::WouldBlock) => {
                    return Err(format!(
                        "another run holds the lock {} and did not finish within {} s. Next: wait for it, or look for a stuck `./dev deps` (its pid is in `./dev worktree list` agents' runs)",
                        path.display(),
                        self.wait.as_secs()
                    ));
                }
                Err(TryLockError::Error(e)) => return Err(format!("cannot lock {}: {e}", path.display())),
            }
        }
    }

    /// The folders of the entry `name` when it is complete and undamaged, else `None`.
    fn valid(&self, name: &str, markers: Markers<'_>) -> Option<Vec<String>> {
        let dir = self.entry_dir(name);
        let text = std::fs::read_to_string(dir.join("entry.json")).ok()?;
        let entry: Value = serde_json::from_str(&text).ok()?;
        if entry["name"] != name {
            return None;
        }
        let roots: Vec<String> = entry["roots"]
            .as_array()?
            .iter()
            .filter_map(|r| r.as_str().map(str::to_string))
            .collect();
        // Only plain relative folders: the entry is data from disk and names where a clone lands (PRINCIPLES: never trust input).
        let plain = |r: &String| !r.is_empty() && Path::new(r).components().all(|c| matches!(c, std::path::Component::Normal(_)));
        let payload = dir.join("payload");
        if roots.is_empty()
            || !roots.iter().all(|r| plain(r) && payload.join(r).is_dir())
            || !markers.iter().all(|m| payload.join(m).exists())
        {
            return None;
        }
        (tree_files(&payload).0 == entry["files"].as_u64()?).then_some(roots)
    }

    /// Without waiting: clones the entry `name` into `tree` when a complete one exists and nobody is building it. `Ok(false)`: no
    /// hit (nothing was changed). For `./dev worktree add`, which never pays a build.
    pub fn try_hit(&self, name: &str, markers: Markers<'_>, tree: &Path) -> Result<bool, String> {
        if !self.entry_dir(name).is_dir() {
            return Ok(false);
        }
        let file = match OpenOptions::new().read(true).write(true).open(self.lock_path(name)) {
            Ok(f) => f,
            Err(_) => return Ok(false),
        };
        if file.try_lock_shared().is_err() {
            return Ok(false);
        }
        let Some(roots) = self.valid(name, markers) else { return Ok(false) };
        self.clone_out(name, &roots, tree)?;
        Ok(true)
    }

    /// The entry `name` in `tree`: cloned from the cache when it is there, else made by `build` (which works in `tree`, where the
    /// result is wanted) and stored. The lock of `name` is held from the first look to the end, so concurrent callers build once.
    pub fn fetch(
        &self,
        name: &str,
        markers: Markers<'_>,
        tree: &Path,
        build: impl FnOnce() -> Result<Vec<String>, String>,
    ) -> Result<Fetched, String> {
        self.prepare()?;
        let shared = self.lock(name, false)?;
        if let Some(roots) = self.valid(name, markers) {
            self.clone_out(name, &roots, tree)?;
            return Ok(Fetched::Hit);
        }
        // The upgrade is not atomic: release, take it exclusively and look again (another builder may have finished meanwhile).
        let _ = shared.unlock();
        drop(shared);
        let _exclusive = self.lock(name, true)?;
        if let Some(roots) = self.valid(name, markers) {
            self.clone_out(name, &roots, tree)?;
            return Ok(Fetched::Hit);
        }
        let entry = self.entry_dir(name);
        if entry.exists() {
            eprintln!("deps: the entry {name} is damaged or incomplete; removing it and building again");
            remove_entry(&self.root, &entry)?;
        }
        let roots = build()?;
        self.store(name, &roots, markers, tree)?;
        self.evict(name)?;
        Ok(Fetched::Built)
    }

    /// Clones the folders of the entry into `tree`, replacing what is there. A failure leaves nothing half-cloned in place.
    fn clone_out(&self, name: &str, roots: &[String], tree: &Path) -> Result<(), String> {
        let payload = self.entry_dir(name).join("payload");
        for rel in roots {
            // per-item: one clone per folder of the entry (at most a handful: the root and workspace node_modules, or ui/dist and css).
            let dest = tree.join(rel);
            if dest.symlink_metadata().is_ok() {
                remove_entry(tree, &dest)?;
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("cannot make {}: {e}", parent.display()))?;
            }
            if let Err(e) = clone_tree(tree, &payload.join(rel), &dest) {
                let _ = remove_entry(tree, &dest);
                return Err(e);
            }
        }
        crate::tool_cache::touch(&self.entry_dir(name));
        Ok(())
    }

    /// Stores the folders of `tree` as the entry `name`: clones them into a staging folder, writes `entry.json` last and renames the
    /// staging folder into place.
    fn store(&self, name: &str, roots: &[String], markers: Markers<'_>, tree: &Path) -> Result<(), String> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let staging = self.root.join(format!(".staging-{name}-{}-{nanos:x}", std::process::id()));
        let payload = staging.join("payload");
        let made = (|| {
            for rel in roots {
                // per-item: one clone per folder of the entry (see clone_out).
                let src = tree.join(rel);
                if !src.is_dir() {
                    return Err(format!("the build left no {} in {}", rel, tree.display()));
                }
                let dest = payload.join(rel);
                std::fs::create_dir_all(dest.parent().unwrap_or(&payload))
                    .map_err(|e| format!("cannot make a folder in {}: {e}", payload.display()))?;
                clone_tree(&staging, &src, &dest)?;
            }
            if let Some(missing) = markers.iter().find(|m| !payload.join(m).exists()) {
                return Err(format!("the build is incomplete: {missing} is missing, so it is not stored"));
            }
            let (files, bytes) = tree_files(&payload);
            let created = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
            let entry = json!({"name": name, "roots": roots, "files": files, "bytes": bytes, "created_unix": created});
            std::fs::write(staging.join("entry.json"), format!("{entry:#}\n")).map_err(|e| format!("cannot write entry.json: {e}"))?;
            std::fs::rename(&staging, self.entry_dir(name)).map_err(|e| format!("cannot move the entry into place: {e}"))
        })();
        if made.is_err() {
            let _ = remove_entry(&self.root, &staging);
        }
        made
    }

    /// Removes the least recently used entries until the cache is within its cap; never `keep` and never an entry whose lock
    /// another run holds. Staging folders older than a day (a killed run's) go too.
    pub fn evict(&self, keep: &str) -> Result<(), String> {
        let mut entries: Vec<(SystemTime, u64, String)> = Vec::new();
        let read = std::fs::read_dir(&self.root).map_err(|e| format!("cannot read {}: {e}", self.root.display()))?;
        for e in read.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Ok(meta) = e.metadata() else { continue };
            if name.starts_with(".staging-") {
                let old = meta
                    .modified()
                    .ok()
                    .and_then(|m| m.elapsed().ok())
                    .is_some_and(|age| age > Duration::from_secs(24 * 3600));
                if old {
                    let _ = remove_entry(&self.root, &e.path());
                }
            } else if meta.is_dir() {
                let bytes = std::fs::read_to_string(e.path().join("entry.json"))
                    .ok()
                    .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                    .and_then(|v| v["bytes"].as_u64())
                    .unwrap_or(0);
                entries.push((meta.modified().unwrap_or(UNIX_EPOCH), bytes, name));
            }
        }
        let mut total: u64 = entries.iter().map(|e| e.1).sum();
        entries.sort();
        for (_, bytes, name) in entries {
            if total <= self.cap_bytes {
                break;
            }
            if name == keep {
                continue;
            }
            let Ok(lock) = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(self.lock_path(&name))
            else {
                continue;
            };
            if lock.try_lock().is_err() {
                continue; // someone is using it: not now
            }
            remove_entry(&self.root, &self.entry_dir(&name))?;
            let _ = remove_entry(&self.root, &self.lock_path(&name));
            eprintln!(
                "deps: evicted {name} ({} MiB) to stay within the {} GiB cap",
                bytes >> 20,
                self.cap_bytes >> 30
            );
            total = total.saturating_sub(bytes);
        }
        Ok(())
    }
}

/// Removes `target` below `root` through the one safe-remove path.
fn remove_entry(root: &Path, target: &Path) -> Result<(), String> {
    remove_under(root, target, std::env::var_os("HOME").map(PathBuf::from).as_deref())
        .map(|_| ())
        .map_err(|e| format!("cannot remove {}: {e:?}", target.display()))
}

/// Files and bytes under `dir`, links counted but not followed.
fn tree_files(dir: &Path) -> (u64, u64) {
    let (mut files, mut bytes) = (0u64, 0u64);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&d) else { continue };
        for e in read.flatten() {
            let Ok(meta) = std::fs::symlink_metadata(e.path()) else { continue };
            if meta.is_dir() {
                stack.push(e.path());
            } else {
                files += 1;
                bytes += meta.len();
            }
        }
    }
    (files, bytes)
}

/// `clonefile(2)` of `src` (a folder, with links kept as links) to the new `dest`.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // one FFI call with two NUL-terminated paths this function owns
fn clonefile(src: &Path, dest: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let cstr =
        |p: &Path| std::ffi::CString::new(p.as_os_str().as_bytes()).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e));
    let (s, d) = (cstr(src)?, cstr(dest)?);
    // SAFETY: both pointers are valid NUL-terminated strings that outlive the call; flags 0 follows no option.
    let r = unsafe { libc::clonefile(s.as_ptr(), d.as_ptr(), 0) };
    if r == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}

/// A copy of `src` at `dest` (which must not exist) with links kept as links: a copy-on-write clone where the file system has one.
/// `root` is the folder under which a failed attempt's leftovers are removed. macOS: `clonefile(2)` of the whole folder, else
/// `cp -c`; elsewhere GNU `cp --reflink=auto`; a different volume or file system falls back to a plain copy, and a failed clone is
/// retried as a plain copy.
pub fn clone_tree(root: &Path, src: &Path, dest: &Path) -> Result<(), String> {
    if cfg!(windows) {
        return Err("the cache clone is not built for Windows (nothing runs it there); run npm ci".into());
    }
    // One `clonefile(2)` of the whole folder is about ten times faster than `cp -c` (0.5 s against 5.0 s for the 43,000 files of
    // node_modules, measured 2026-11-26: docs/benchmarks.md); it is atomic, so a failure leaves nothing to clean up.
    #[cfg(target_os = "macos")]
    if clonefile(src, dest).is_ok() {
        return Ok(());
    }
    let attempts: &[&[&str]] = if cfg!(target_os = "macos") {
        &[&["-cRP"], &["-RP"]]
    } else {
        &[&["-RP", "--reflink=auto"], &["-RP"]]
    };
    let env: HashMap<String, String> = crate::process_env::vars().collect();
    let mut last = String::new();
    for flags in attempts {
        // per-item: at most two attempts of one clone, the second only after the first failed.
        let ran = ToolCall::new("cp")
            .args(flags.iter().copied())
            .args([src.display().to_string(), dest.display().to_string()])
            .timeout(HELPER_TIMEOUT)
            .run(&env, &mut std::io::sink())?;
        if ran.success {
            return Ok(());
        }
        last = format!(
            "cp {}: {} {}",
            flags.join(" "),
            ran.how,
            crate::ci_tool::tail(&ran.stderr, 2).trim()
        );
        if dest.symlink_metadata().is_ok() {
            let _ = remove_entry(root, dest); // a partial copy of a failed attempt
        }
    }
    Err(format!("cannot copy {} to {}: {last}", src.display(), dest.display()))
}

#[cfg(test)]
mod tests;

// ---- the commands -------------------------------------------------------------------------------------------------------------

/// A file that a complete `npm ci` always leaves (npm's hidden lockfile).
const NODE_MARKERS: Markers<'static> = &["node_modules/.package-lock.json"];
/// Files a complete `ui` build always leaves: the dist stamp and the generated css (`build-guard.ts` reads both).
const UI_MARKERS: Markers<'static> = &[
    "ui/dist/.source-hash",
    "ui/projects/paddock-ui/css/.source-hash",
    "ui/projects/paddock-ui/css/tokens.css",
];
/// The folders a `ui` build makes.
const UI_ROOTS: [&str; 2] = ["ui/dist", "ui/projects/paddock-ui/css"];

/// `./dev deps ...`
#[derive(Subcommand)]
pub enum DepsCommand {
    /// `node_modules` for this lockfile, Node and platform: a copy-on-write clone from the shared cache, or `npm ci` once (then stored).
    Install(TreeArgs),
    /// `ui/dist` and the generated css for these sources: `install` first, then a clone from the shared cache, or `npm run build -w ui`
    /// once (then stored).
    Ui(TreeArgs),
}

/// The tree to work in.
#[derive(Args)]
pub struct TreeArgs {
    /// The worktree root (default: the repository of the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
}

/// `./dev deps ...`
pub fn command(c: DepsCommand) -> Result<Report, CliError> {
    let (ui, args) = match c {
        DepsCommand::Install(a) => (false, a),
        DepsCommand::Ui(a) => (true, a),
    };
    let root = match args.root {
        Some(r) => r,
        None => top_level(&std::env::current_dir().map_err(|e| CliError::failure(format!("cannot read the current folder: {e}.")))?)?,
    };
    let cache = cache_for(&root)?;
    let started = Instant::now();
    let env: HashMap<String, String> = crate::process_env::vars().collect();
    let mut lines = Vec::new();
    let mut data = serde_json::Map::new();
    let mut step = |what: &str, r: Result<(Fetched, String), String>| -> Result<(), CliError> {
        let (how, name) = r.map_err(|e| CliError::failure(format!("{what}: {e}")))?;
        let said = if how == Fetched::Hit {
            "cloned from the shared cache"
        } else {
            "built, then stored in the shared cache"
        };
        eprintln!("deps: {what} {said} ({name}, {:.1} s)", started.elapsed().as_secs_f64());
        lines.push(format!("{what}: {said} ({name})"));
        data.insert(what.to_string(), json!({"name": name, "hit": how == Fetched::Hit}));
        Ok(())
    };
    step("node_modules", install(&root, &cache, &env))?;
    if ui {
        step("ui", build_ui(&root, &cache, &env))?;
    }
    data.insert("seconds".into(), json!(started.elapsed().as_secs_f64()));
    Ok(Report::new(Value::Object(data), lines.join("\n")))
}

fn top_level(from: &Path) -> Result<PathBuf, CliError> {
    crate::worktree::git::Git::at(from)
        .text(&["rev-parse", "--show-toplevel"])
        .map(|s| PathBuf::from(s.trim()))
        .map_err(|e| {
            CliError::failure(format!(
                "{} is not in a git repository ({e:?}). Next: pass --root <worktree>",
                from.display()
            ))
        })
}

/// The cache of this machine: `<dev-targets>/cache/npm`, with the cap from [`CAP_ENV`] or [`DEFAULT_CAP_GIB`].
fn cache_for(root: &Path) -> Result<Cache, CliError> {
    let targets = crate::places::dev_targets(root, &|k| std::env::var(k).ok())?;
    let cap = std::env::var(CAP_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|g| *g > 0)
        .unwrap_or(DEFAULT_CAP_GIB);
    Ok(Cache::new(cache_root(&targets), cap, NPM_CI_TIMEOUT + UI_BUILD_TIMEOUT))
}

/// The environment variables an npm run keeps besides the restricting ones (as `./dev worktree add`'s install does).
const PASS: &[&str] = &["npm_config_*", "NPM_CONFIG_*", "NODE_*", "NG_*"];

fn tool(program: &str, root: &Path, timeout: Duration) -> ToolCall {
    ToolCall::new(program)
        .cwd(root)
        .pass(NETWORK_ENV.iter().copied().chain(PASS.iter().copied()))
        .timeout(timeout)
}

/// `node --version`, the part of the key that says which native binaries `npm ci` fetched.
fn node_version(root: &Path, env: &HashMap<String, String>) -> Result<String, String> {
    let ran = tool("node", root, HELPER_TIMEOUT)
        .args(["--version"])
        .run(env, &mut std::io::sink())?;
    if !ran.success {
        return Err(format!(
            "`node --version` failed ({}). Next: install the Node version in .nvmrc",
            ran.how
        ));
    }
    Ok(ran.stdout.trim().to_string())
}

/// The entry name for `root`'s `node_modules`.
pub fn npm_name(root: &Path, env: &HashMap<String, String>) -> Result<String, String> {
    let lock = std::fs::read(root.join("package-lock.json"))
        .map_err(|e| format!("cannot read {}: {e}", root.join("package-lock.json").display()))?;
    Ok(node_modules_key(
        &lock,
        &node_version(root, env)?,
        std::env::consts::OS,
        std::env::consts::ARCH,
    ))
}

/// The `node_modules` folders under `root` (the root's and each workspace's) that exist, relative to `root`.
pub fn node_roots(root: &Path) -> Vec<String> {
    let mut found = vec!["node_modules".to_string()];
    let patterns: Vec<String> = std::fs::read_to_string(root.join("package.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| {
            v["workspaces"]
                .as_array()
                .map(|a| a.iter().filter_map(|w| w.as_str().map(str::to_string)).collect())
        })
        .unwrap_or_default();
    for p in patterns {
        for ws in expand(root, &p) {
            if root.join(&ws).join("node_modules").is_dir() {
                found.push(format!("{ws}/node_modules"));
            }
        }
    }
    found.retain(|r| root.join(r).is_dir());
    found.sort();
    found.dedup();
    found
}

/// The folders a workspace pattern names: literal segments and `*` for any folder.
fn expand(root: &Path, pattern: &str) -> Vec<String> {
    let mut paths = vec![String::new()];
    for seg in pattern.split('/').filter(|s| !s.is_empty()) {
        let mut next = Vec::new();
        for base in &paths {
            if seg == "*" {
                let Ok(read) = std::fs::read_dir(root.join(base)) else { continue };
                for e in read.flatten() {
                    if e.file_type().is_ok_and(|t| t.is_dir()) && e.file_name() != "node_modules" {
                        next.push(join_rel(base, &e.file_name().to_string_lossy()));
                    }
                }
            } else {
                next.push(join_rel(base, seg));
            }
        }
        paths = next;
    }
    paths.retain(|p| root.join(p).is_dir());
    paths
}

fn join_rel(base: &str, seg: &str) -> String {
    if base.is_empty() {
        seg.to_string()
    } else {
        format!("{base}/{seg}")
    }
}

/// `node_modules`: a hit clones it, a miss runs `npm ci` in `root` and stores the result.
pub fn install(root: &Path, cache: &Cache, env: &HashMap<String, String>) -> Result<(Fetched, String), String> {
    let name = npm_name(root, env)?;
    let how = cache.fetch(&name, NODE_MARKERS, root, || {
        eprintln!("deps: no cached node_modules for {name}: running npm ci (once; the next worktree clones it)");
        let ran = tool("npm", root, NPM_CI_TIMEOUT)
            .args(["ci", "--no-audit", "--no-fund"])
            .output(Output::Stream)
            .run(env, &mut std::io::stderr())?;
        if !ran.success {
            return Err(format!(
                "npm ci failed ({}). Next: run `npm ci` in {} and read its output",
                ran.how,
                root.display()
            ));
        }
        Ok(node_roots(root))
    })?;
    Ok((how, name))
}

/// The two content hashes of the build guard (dist, css), asked from Node so the one definition stays in `build-guard.ts`.
fn source_hashes(root: &Path, env: &HashMap<String, String>) -> Result<(String, String), String> {
    const CODE: &str = "import { sourceHash } from './ui/projects/paddock-e2e/src/build-guard.ts'; \
                        console.log(sourceHash('ui', 'dist')); console.log(sourceHash('ui', 'css'));";
    let ran = tool("node", root, HELPER_TIMEOUT)
        .args(["--disable-warning=MODULE_TYPELESS_PACKAGE_JSON", "--input-type=module", "-e", CODE])
        .run(env, &mut std::io::sink())?;
    let out = ran.stdout.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>();
    match (ran.success, out.as_slice()) {
        (true, [d, c]) => Ok(((*d).to_string(), (*c).to_string())),
        _ => Err(format!(
            "the build guard's hashes were not read ({}): {}",
            ran.how,
            crate::ci_tool::tail(&ran.stderr, 3).trim()
        )),
    }
}

/// The hash of the files, besides `projects/paddock-ui`, that the ui build reads.
fn other_inputs(root: &Path) -> String {
    let mut files: Vec<PathBuf> = ["ui/angular.json", "ui/package.json", "ui/tsconfig.json"]
        .iter()
        .map(|f| root.join(f))
        .collect();
    for dir in ["ui/scripts", "tsconfig"] {
        if let Ok(read) = std::fs::read_dir(root.join(dir)) {
            files.extend(
                read.flatten()
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                    .map(|e| e.path()),
            );
        }
    }
    files.sort();
    let mut h = Sha256::new();
    for f in files {
        h.update(f.strip_prefix(root).unwrap_or(&f).to_string_lossy().as_bytes());
        h.update([0]);
        h.update(std::fs::read(&f).unwrap_or_default());
        h.update([0]);
    }
    hex(&h.finalize())
}

/// `ui/dist` and the css: a hit clones them, a miss runs `npm run build -w ui` in `root` and stores the result. A tree whose
/// stamps already equal the sources' is left alone.
pub fn build_ui(root: &Path, cache: &Cache, env: &HashMap<String, String>) -> Result<(Fetched, String), String> {
    let deps = npm_name(root, env)?;
    let (dist, css) = source_hashes(root, env)?;
    let name = ui_key(&dist, &css, &deps, &other_inputs(root));
    if stamp_is(root, "ui/dist/.source-hash", &dist)
        && stamp_is(root, "ui/projects/paddock-ui/css/.source-hash", &css)
        && root.join("ui/projects/paddock-ui/css/tokens.css").is_file()
    {
        return Ok((Fetched::Hit, name));
    }
    let how = cache.fetch(&name, UI_MARKERS, root, || {
        eprintln!("deps: no cached ui build for {name}: running npm run build -w ui (once; the next worktree clones it)");
        let ran = tool("npm", root, UI_BUILD_TIMEOUT)
            .args(["run", "build", "-w", "ui"])
            .output(Output::Stream)
            .run(env, &mut std::io::stderr())?;
        if !ran.success {
            return Err(format!(
                "the ui build failed ({}). Next: run `npm run build -w ui` in {} and read its output",
                ran.how,
                root.display()
            ));
        }
        Ok(UI_ROOTS.iter().map(|r| (*r).to_string()).collect())
    })?;
    Ok((how, name))
}

fn stamp_is(root: &Path, rel: &str, hash: &str) -> bool {
    std::fs::read_to_string(root.join(rel)).is_ok_and(|t| t == hash)
}

/// For `./dev worktree add`: when the shared cache already holds this lockfile's `node_modules` and nobody is building it, clone it
/// into the new worktree and say so; `None` otherwise (the caller installs what it needs itself). Never waits, never builds.
pub fn warm_note(root: &Path) -> Option<String> {
    let started = Instant::now();
    let env: HashMap<String, String> = crate::process_env::vars().collect();
    let name = npm_name(root, &env).ok()?;
    let cache = cache_for(root).ok()?;
    match cache.try_hit(&name, NODE_MARKERS, root) {
        Ok(true) => Some(format!(
            "\nnode_modules cloned from the shared cache ({name}, {:.1} s).",
            started.elapsed().as_secs_f64()
        )),
        Ok(false) => None,
        Err(e) => {
            eprintln!("deps: the shared node_modules cache was not used ({e}); the caller installs what it needs");
            None
        }
    }
}
