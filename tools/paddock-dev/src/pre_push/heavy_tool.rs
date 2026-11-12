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
    if !h.dirty && cached.is_file() {
        if runs(&cached, env) {
            return Ok(cached);
        }
        let _ = std::fs::remove_file(&cached);
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

/// Publishes `built` as `bin` by an atomic rename from a name of our own beside it.
fn publish(built: &Path, bin: &Path) -> Result<(), String> {
    let dir = bin.parent().map(Path::to_path_buf).unwrap_or_default();
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot write the cache folder {}: {e}", dir.display()))?;
    let tmp = dir.join(format!(".heavy.{}", std::process::id()));
    std::fs::copy(built, &tmp)
        .and_then(|_| std::fs::rename(&tmp, bin))
        .map_err(|e| format!("cannot publish heavy into {}: {e}", dir.display()))
}
