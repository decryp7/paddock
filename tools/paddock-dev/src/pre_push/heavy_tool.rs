//! The `heavy` binary for the push gate, built on demand like `./dev` builds `paddock-dev` (ADR 0022 §3.3): content-addressed by
//! [`input_hash::compute_for`] over `tools/heavy/inputs.txt`, cached under `<PADDOCK_DEV_HOME or ~/.paddock-dev>/bin/heavy-<hash>/`,
//! built with `cargo build --release --locked` and published by an atomic rename. The build folder is `./dev`'s own root
//! (`PADDOCK_DEV_TARGET_DIR`, else `tools/heavy/target` of the tree, never the internal disk's cache folder), one per build, and is
//! removed afterwards through `safe_remove` with the result printed. Inputs with uncommitted changes (a fix to heavy still in the
//! working tree) are published as `heavy-dirty`, replaced by each such build. Any failure is a message with the
//! cause and the next step; the caller turns it into "not measured" (ER-33).

use crate::ci_tool::{ToolCall, tail};
use crate::disk::record::{self, Kind, Record};
use crate::input_hash::{self, Real};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

const BUILD_LIMIT: Duration = Duration::from_secs(1200);
const PASS: &[&str] = &[
    "PATH",
    "HOME",
    "CARGO_*",
    "RUSTUP_*",
    "RUSTC*",
    "RUSTFLAGS",
    "SCCACHE_*",
    "TMPDIR",
    "SDKROOT",
    "CC",
    "CXX",
];

/// The cache folder: `PADDOCK_DEV_HOME`, else `~/.paddock-dev` (what `./dev` uses).
fn home(env: &HashMap<String, String>) -> Result<PathBuf, String> {
    match (env.get("PADDOCK_DEV_HOME"), env.get("HOME")) {
        (Some(h), _) if !h.is_empty() => Ok(PathBuf::from(h)),
        (_, Some(h)) if !h.is_empty() => Ok(Path::new(h).join(".paddock-dev")),
        _ => Err("neither PADDOCK_DEV_HOME nor HOME is set, so there is no cache folder for the heavy tool".into()),
    }
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// Whether `bin` runs and answers `--version` (an empty or cut-short file is not a tool).
fn runs(bin: &Path, env: &HashMap<String, String>) -> bool {
    ToolCall::new(bin.display().to_string())
        .args(["--version"])
        .pass(PASS.iter().copied())
        .timeout(Duration::from_secs(30))
        .run(env, &mut std::io::sink())
        .is_ok_and(|r| r.success && !r.stdout.trim().is_empty())
}

/// The path of a runnable `heavy` for the tree at `root`: cached, else built. `Err` is the whole explanation.
pub fn ensure(root: &Path, env: &HashMap<String, String>, left: Duration) -> Result<PathBuf, String> {
    let home = home(env)?;
    let lookup = |k: &str| env.get(k).cloned();
    let vars: Vec<(String, String)> = env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let h = input_hash::compute_for(&Real, &lookup, &vars, root, "tools/heavy")
        .map_err(|e| format!("the heavy tool's input hash could not be computed: {e:?}. Next: run `./dev check --pre-push` inside the Paddock repository with git and rustc working"))?;
    let (key, cached) = if h.dirty {
        ("dirty".to_string(), home.join("bin").join("heavy-dirty").join(exe("heavy")))
    } else {
        (
            h.hash.clone(),
            home.join("bin").join(format!("heavy-{}", h.hash)).join(exe("heavy")),
        )
    };
    // A cached binary that fails its probe is not removed: a probe that timed out on a loaded machine is no proof the file is
    // broken, and another agent may be running it. The build below replaces it by an atomic rename, which leaves every running
    // copy and every hold intact.
    if !h.dirty && cached.is_file() && runs(&cached, env) {
        // A hit is use: `./dev`'s prune of `bin/` spares a folder used within a day (best effort; a miss costs a rebuild).
        touch(cached.parent().unwrap_or(&home));
        return Ok(cached);
    }
    if left.is_zero() {
        return Err(format!(
            "the push gate's budget was spent before heavy ({key}) could be built. Next: re-run `./dev check --pre-push`"
        ));
    }
    let base = match env.get("PADDOCK_DEV_TARGET_DIR").filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => root.join("tools/heavy/target"),
    };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let target = base.join(format!("heavy-{}-{}-{nanos:x}", &key[..key.len().min(12)], std::process::id()));
    // The owner record is what lets `safe_remove` prove the folder is ours, so the removal below is allowed (and not silently refused).
    let recorded = std::fs::create_dir_all(&target)
        .map_err(|e| format!("cannot make the build folder {}: {e}", target.display()))
        .and_then(|()| record::write(&target, &Record::new(Kind::Build, "dev:pre-push-heavy")));
    let started = std::time::Instant::now();
    let result = recorded.and_then(|()| build(root, env, &target, &key, left.min(BUILD_LIMIT)));
    eprintln!(
        "pre-push: built heavy {}{} in {} s ({})",
        h.hash,
        if h.dirty { " (uncommitted inputs)" } else { "" },
        started.elapsed().as_secs(),
        if result.is_ok() { "ok" } else { "failed" }
    );
    let published = result.and_then(|built| publish(&built, &cached));
    // Never left behind, and never silent: what the removal did is printed (ER-27).
    match crate::safe_remove::remove(&target, std::slice::from_ref(&base)) {
        Ok(r) => eprintln!("pre-push: removed the heavy build folder {}", r.path.display()),
        Err(why) => eprintln!(
            "::warning::pre-push: the heavy build folder {} was not removed: {why}. Next: delete it by hand",
            target.display()
        ),
    }
    published.map(|()| cached)
}

/// Builds heavy into `target` and returns the binary's path there.
fn build(root: &Path, env: &HashMap<String, String>, target: &Path, key: &str, limit: Duration) -> Result<PathBuf, String> {
    let built = target.join("release").join(exe("heavy"));
    let call = ToolCall::new("cargo")
        .args(["build", "--release", "--locked", "--quiet"])
        .pass(PASS.iter().copied())
        .cwd(root.join("tools/heavy"))
        .env("CARGO_TARGET_DIR", target.display().to_string())
        .env("CARGO_BUILD_JOBS", "2")
        .timeout(limit);
    let ran = call.run(env, &mut std::io::sink()).map_err(|e| {
        format!("heavy ({key}) could not be built: {e}. Next: run `cargo build --release --locked` in tools/heavy to see why")
    })?;
    if !ran.success {
        return Err(format!(
            "heavy ({key}) could not be built ({}): {}. Next: run `cargo build --release --locked` in tools/heavy; a stale tools/heavy/Cargo.lock is the usual cause",
            ran.how,
            tail(&ran.stderr, 6).trim()
        ));
    }
    if !runs(&built, env) {
        return Err(format!(
            "cargo left no runnable heavy at {} (a cargo config with build.target puts it elsewhere). Next: remove that setting for the push gate",
            built.display()
        ));
    }
    Ok(built)
}

/// Marks `dir` as used now (its modification time), so an age-based prune keeps it. Errors are ignored: it is only a hint.
fn touch(dir: &Path) {
    let _ = std::fs::File::open(dir).and_then(|f| f.set_modified(std::time::SystemTime::now()));
}

/// A run's own hold on a cached `heavy`: a hard link (else a copy) under `<cache>/held/`, outside `bin/`, which `./dev` prunes
/// and other agents' builds replace. A run resolves heavy once and starts it many times over minutes or hours; holding its own
/// name means no other agent's prune, probe or publish can take it away between those starts (2026-11-14: six generators of
/// one `gen check` found the shared binary gone). Removed when dropped.
#[derive(Debug)]
pub struct Held {
    path: PathBuf,
}

impl Held {
    /// The held binary: start this, never the shared cache path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        // Our own unique name, made by `hold`: nothing else is removed (ER-14).
        let _ = std::fs::remove_file(&self.path);
    }
}

/// How old a hold must be before a later `hold` removes it as left behind by a run that was killed: no run lasts a week.
const STALE_HOLD: Duration = Duration::from_secs(7 * 24 * 3600);

/// Holds the cached binary `bin` for this run (see [`Held`]). `Err` is the whole explanation.
pub fn hold(bin: &Path, env: &HashMap<String, String>) -> Result<Held, String> {
    let dir = home(env)?.join("held");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot hold heavy: cannot make {}: {e}. Next: make it writable", dir.display()))?;
    sweep(&dir);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let path = dir.join(exe(&format!("heavy-{}-{nanos:x}", std::process::id())));
    // A hard link costs nothing and keeps the file alive whatever happens to the cache's name; a copy where links do not work.
    std::fs::hard_link(bin, &path)
        .or_else(|_| std::fs::copy(bin, &path).map(|_| ()))
        .map_err(|e| {
            format!(
                "cannot hold heavy: {} could not be linked or copied to {}: {e}. Next: run the command again (another agent may have replaced the cache at that moment)",
                bin.display(),
                path.display()
            )
        })?;
    // ER-27: which binary this run starts, so a later "not found" names the hold, not the shared cache.
    eprintln!("heavy: this run holds {} as {}", bin.display(), path.display());
    Ok(Held { path })
}

/// Removes holds older than [`STALE_HOLD`] (a run that was killed before its hold was dropped).
fn sweep(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > STALE_HOLD);
        if old && e.file_name().to_string_lossy().starts_with("heavy-") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Publishes `built` as `bin` by an atomic rename from a name of our own beside it.
fn publish(built: &Path, bin: &Path) -> Result<(), String> {
    let dir = bin.parent().map(Path::to_path_buf).unwrap_or_default();
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot write the cache folder {}: {e}", dir.display()))?;
    let tmp = dir.join(format!(".heavy.{}", std::process::id()));
    std::fs::copy(built, &tmp)
        .and_then(|_| std::fs::rename(&tmp, bin))
        .map_err(|e| format!("cannot publish heavy into {}: {e}", dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_heavy(home: &Path) -> PathBuf {
        let dir = home.join("bin").join("heavy-abc");
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join(exe("heavy"));
        std::fs::write(&bin, "#!/bin/sh\necho held\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        bin
    }

    #[test]
    fn a_held_heavy_survives_another_agent_removing_or_replacing_the_cache() {
        let home = tempfile::tempdir().unwrap();
        let env = HashMap::from([("PADDOCK_DEV_HOME".to_string(), home.path().display().to_string())]);
        let cached = fake_heavy(home.path());
        let held = hold(&cached, &env).unwrap();
        assert_ne!(held.path(), cached, "the run uses its own name, not the shared cache's");
        assert!(
            !held.path().starts_with(home.path().join("bin")),
            "outside the folder `./dev` prunes"
        );
        // Another agent's `./dev` prunes the cache folder (or a failed `--version` probe removes the binary) mid-run. The test takes
        // the cache's path away by removing the binary and moving its folder aside: the same loss for the run, with no folder
        // deletion outside `safe_remove` (tests/it/safe_remove_lint.rs reads this file's text too).
        std::fs::remove_file(&cached).unwrap();
        std::fs::rename(home.path().join("bin"), home.path().join("pruned")).unwrap();
        assert!(!cached.exists(), "the shared cache's name is gone");
        assert!(held.path().is_file(), "the held binary is still there for the later steps");
        #[cfg(unix)]
        {
            let out = paddock_proc::command(held.path()).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "held");
        }
        // Dropping the hold removes the run's own name, and only it.
        let path = held.path().to_path_buf();
        drop(held);
        assert!(!path.exists(), "the hold is removed when the run ends");
        assert!(home.path().join("held").is_dir());
    }

    #[test]
    fn a_missing_cached_binary_cannot_be_held_and_says_why() {
        let home = tempfile::tempdir().unwrap();
        let env = HashMap::from([("PADDOCK_DEV_HOME".to_string(), home.path().display().to_string())]);
        let why = hold(&home.path().join("bin/heavy-x/heavy"), &env).err().unwrap();
        assert!(why.contains("cannot hold heavy") && why.contains("Next:"), "{why}");
    }
}
