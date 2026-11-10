//! The `.crate` archives against the tagged lock files, and the Tauri tool files against their pins.
//!
//! A registry archive in `CARGO_HOME/registry/cache/<index>/<name>-<version>.crate` is trusted only when its SHA-256 equals
//! the `checksum` of that package in one of the `Cargo.lock` files of the tagged tree that this job builds (cargo itself
//! does not re-verify an archive that is already in the cache, ADR 0085 §2.6).

use super::ledger::Kind;
use super::verify::{Ctx, Probe};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::path::Path;

/// How many findings of one probe are listed before the rest are counted.
const LIST_LIMIT: usize = 20;

/// The SHA-256 of a file as lower-case hex.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    std::io::copy(&mut File::open(path)?, &mut hasher)?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// `name-version` to every checksum a lock file of the union gives it.
pub fn lock_union(lock_files: &[std::path::PathBuf]) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    let mut union: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for path in lock_files {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{} cannot be read: {e}", path.display()))?;
        let doc: toml::Table = text.parse().map_err(|e| format!("{} is not valid TOML: {e}", path.display()))?;
        let packages = doc
            .get("package")
            .and_then(toml::Value::as_array)
            .ok_or_else(|| format!("{} has no [[package]] entries", path.display()))?;
        for p in packages {
            let (Some(name), Some(version), Some(sum)) = (
                p.get("name").and_then(toml::Value::as_str),
                p.get("version").and_then(toml::Value::as_str),
                p.get("checksum").and_then(toml::Value::as_str),
            ) else {
                continue;
            };
            union.entry(format!("{name}-{version}")).or_default().insert(sum.to_string());
        }
    }
    Ok(union)
}

fn listed(items: &[String]) -> String {
    if items.len() <= LIST_LIMIT {
        items.join(", ")
    } else {
        format!("{} and {} more", items[..LIST_LIMIT].join(", "), items.len() - LIST_LIMIT)
    }
}

pub(crate) fn crates_probe(ctx: &mut Ctx) {
    if !ctx.opts.expect.contains(&Kind::CargoHome) {
        ctx.verdict
            .notices
            .push("crates: not checked (the job does not expect `cargo-home`: it compiles nothing).".to_string());
        return;
    }
    let Some(home) = ctx.folder(Kind::CargoHome) else { return };
    let Some(clone) = ctx.folder(Kind::Workspace) else {
        ctx.add(
            Probe::Crates,
            "check broken: the ledger has no clone, so the lock files cannot be found.",
            "Record the `workspace` kind.",
        );
        return;
    };
    if ctx.opts.cargo_workspaces.is_empty() {
        ctx.add(
            Probe::Crates,
            "check broken: no Cargo workspace was named, so there is no lock file to compare the archives with.",
            "Pass `--cargo-workspace <folder relative to the clone>` for the app's workspace and for tools/paddock-dev.",
        );
        return;
    }
    let locks: Vec<_> = ctx.opts.cargo_workspaces.iter().map(|w| clone.join(w).join("Cargo.lock")).collect();
    let union = match lock_union(&locks) {
        Ok(u) => u,
        Err(e) => {
            ctx.add(
                Probe::Crates,
                format!("check broken: {e}."),
                "Name the Cargo workspaces of the tagged tree that this job builds.",
            );
            return;
        }
    };
    let mut archives = Vec::new();
    if let Ok(indexes) = std::fs::read_dir(home.join("registry").join("cache")) {
        for index in indexes.flatten() {
            if let Ok(files) = std::fs::read_dir(index.path()) {
                archives.extend(
                    files
                        .flatten()
                        .map(|f| f.path())
                        .filter(|p| p.extension().is_some_and(|e| e == "crate")),
                );
            }
        }
    }
    archives.sort();
    if archives.is_empty() {
        ctx.add(
            Probe::Crates,
            format!("check broken: 0 .crate archives under {}/registry/cache, so nothing was compared.", home.display()),
            "If the job compiles nothing, leave `cargo-home` out of --expect; otherwise `cargo fetch --locked` did not use this CARGO_HOME.",
        );
        return;
    }
    let (mut unknown, mut wrong, mut unreadable) = (Vec::new(), Vec::new(), Vec::new());
    for a in &archives {
        let stem = a.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
        match union.get(&stem) {
            None => unknown.push(stem),
            Some(sums) => match sha256_file(a) {
                Ok(h) if sums.contains(&h) => ctx.verdict.crates_matched += 1,
                Ok(h) => wrong.push(format!("{stem} (sha256 {h})")),
                Err(e) => unreadable.push(format!("{stem} ({e})")),
            },
        }
    }
    if !unknown.is_empty() {
        ctx.add(
            Probe::Crates,
            format!(
                "{} archive(s) have no entry in the tagged Cargo.lock files: {}.",
                unknown.len(),
                listed(&unknown)
            ),
            "Something wrote them that the build does not need: a shared CARGO_HOME, or a lock file missing from --cargo-workspace.",
        );
    }
    if !wrong.is_empty() {
        ctx.add(
            Probe::Crates,
            format!(
                "{} archive(s) do not match the checksum of Cargo.lock: {}.",
                wrong.len(),
                listed(&wrong)
            ),
            "A poisoned or stale cache: delete the CARGO_HOME of the job and run `cargo fetch --locked` again.",
        );
    }
    if !unreadable.is_empty() {
        ctx.add(
            Probe::Crates,
            format!(
                "check broken: {} archive(s) cannot be read: {}.",
                unreadable.len(),
                listed(&unreadable)
            ),
            "Check the permissions of CARGO_HOME.",
        );
    }
}

fn files_under(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => files_under(&p, out),
                Ok(_) => out.push(p),
                Err(_) => {}
            }
        }
    }
}

pub(crate) fn tauri_probe(ctx: &mut Ctx) {
    let Some(tools) = ctx.folder(Kind::TauriTools) else { return };
    let Some(pins) = ctx.opts.tauri_pins.clone() else {
        ctx.verdict
            .notices
            .push("tauri-tools: not measured (no --tauri-pins file; ADR 0085 WP-9 pins the files). The folder's identity was checked, its content was not.".to_string());
        return;
    };
    let text = match std::fs::read_to_string(&pins) {
        Ok(t) => t,
        Err(e) => {
            ctx.add(
                Probe::TauriTools,
                format!("check broken: the pin file {} cannot be read: {e}.", pins.display()),
                "Pass the pin file of WP-9.",
            );
            return;
        }
    };
    let mut pinned: BTreeMap<String, String> = BTreeMap::new();
    for line in text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')) {
        if let Some((sum, rel)) = line.split_once(char::is_whitespace) {
            pinned.insert(
                rel.trim().trim_start_matches('*').replace('\\', "/"),
                sum.trim().to_ascii_lowercase(),
            );
        }
    }
    let mut files = Vec::new();
    files_under(&tools, &mut files);
    if files.is_empty() {
        ctx.add(
            Probe::TauriTools,
            format!(
                "check broken: 0 files in {}, so the pins were compared with nothing.",
                tools.display()
            ),
            "WP-9 must fetch the pinned tools into <build folder>/.tauri before `tauri build`.",
        );
        return;
    }
    let mut bad = Vec::new();
    let mut seen = BTreeSet::new();
    for f in &files {
        let rel = f.strip_prefix(&tools).unwrap_or(f).to_string_lossy().replace('\\', "/");
        seen.insert(rel.clone());
        match (pinned.get(&rel), sha256_file(f)) {
            (None, _) => bad.push(format!("{rel} (no pin)")),
            (Some(want), Ok(got)) if *want == got => ctx.verdict.tauri_files_matched += 1,
            (Some(_), Ok(got)) => bad.push(format!("{rel} (sha256 {got})")),
            (Some(_), Err(e)) => bad.push(format!("{rel} ({e})")),
        }
    }
    for rel in pinned.keys().filter(|r| !seen.contains(*r)) {
        bad.push(format!("{rel} (pinned, but not in the folder)"));
    }
    if !bad.is_empty() {
        ctx.add(
            Probe::TauriTools,
            format!(
                "{} file(s) of {} differ from the pins: {}.",
                bad.len(),
                tools.display(),
                listed(&bad)
            ),
            "The bundler tools must come from the pinned URLs (WP-9), never from a cache or a download without a checksum.",
        );
    }
}
