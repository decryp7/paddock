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
use std::path::{Path, PathBuf};

/// How many findings of one probe are listed before the rest are counted.
const LIST_LIMIT: usize = 20;

/// The SHA-256 of a file as lower-case hex.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    std::io::copy(&mut File::open(path)?, &mut hasher)?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The registry packages of the tagged lock files: `(source, name, version)` to every checksum a lock file gives it.
#[derive(Debug, Default)]
pub struct Union {
    /// The checksums by package.
    pub sums: BTreeMap<(String, String, String), BTreeSet<String>>,
    /// Some lock file has a `git+` source (its checkouts under `CARGO_HOME/git` cannot be matched to a checksum).
    pub has_git: bool,
}

impl Union {
    /// The registry sources of the union.
    fn sources(&self) -> BTreeSet<&str> {
        self.sums.keys().map(|(s, _, _)| s.as_str()).collect()
    }
}

/// The folder-name prefixes cargo gives the cache folder of a registry source: `<host>-<hash>`. crates.io is
/// `index.crates.io-` (sparse) or `github.com-` (the old git index). Matching is by host: the hash is cargo's own and is not
/// recomputed here.
pub fn index_prefixes(source: &str) -> Vec<String> {
    let url = source.trim_start_matches("registry+").trim_start_matches("sparse+");
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split('/').next().unwrap_or_default();
    if host == "index.crates.io" || (host == "github.com" && rest.contains("crates.io-index")) {
        return vec!["index.crates.io-".into(), "github.com-".into()];
    }
    vec![format!("{host}-")]
}

/// Reads the lock files of the union.
pub fn lock_union(lock_files: &[std::path::PathBuf]) -> Result<Union, String> {
    let mut union = Union::default();
    for path in lock_files {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{} cannot be read: {e}", path.display()))?;
        let doc: toml::Table = text.parse().map_err(|e| format!("{} is not valid TOML: {e}", path.display()))?;
        let packages = doc
            .get("package")
            .and_then(toml::Value::as_array)
            .ok_or_else(|| format!("{} has no [[package]] entries", path.display()))?;
        for p in packages {
            let source = p.get("source").and_then(toml::Value::as_str).unwrap_or_default();
            if source.starts_with("git+") {
                union.has_git = true;
            }
            let (Some(name), Some(version), Some(sum)) = (
                p.get("name").and_then(toml::Value::as_str),
                p.get("version").and_then(toml::Value::as_str),
                p.get("checksum").and_then(toml::Value::as_str),
            ) else {
                continue;
            };
            union
                .sums
                .entry((source.to_string(), name.to_string(), version.to_string()))
                .or_default()
                .insert(sum.to_string());
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
    // Archives are listed without following links: a link is a finding, never hashed.
    let sources = union.sources();
    let mut archives: Vec<(PathBuf, String)> = Vec::new(); // (file, index folder name)
    let (mut links, mut strays, mut orphan_indexes) = (Vec::new(), Vec::new(), Vec::new());
    let cache = home.join("registry").join("cache");
    if std::fs::symlink_metadata(&cache).is_ok_and(|m| m.file_type().is_symlink()) {
        links.push(cache.display().to_string());
    } else if let Ok(indexes) = std::fs::read_dir(&cache) {
        for index in indexes.flatten() {
            let name = index.file_name().to_string_lossy().into_owned();
            let Ok(t) = index.file_type() else { continue };
            if t.is_symlink() {
                links.push(index.path().display().to_string());
                continue;
            }
            if !t.is_dir() {
                continue;
            }
            if !sources
                .iter()
                .any(|s| index_prefixes(s).iter().any(|p| name.starts_with(p.as_str())))
            {
                orphan_indexes.push(name.clone());
            }
            if let Ok(files) = std::fs::read_dir(index.path()) {
                for f in files.flatten() {
                    let p = f.path();
                    let Ok(ft) = f.file_type() else { continue };
                    if ft.is_symlink() {
                        links.push(p.display().to_string());
                    } else if p.extension().is_some_and(|e| e == "crate") && ft.is_file() {
                        archives.push((p, name.clone()));
                    } else {
                        strays.push(p.display().to_string());
                    }
                }
            }
        }
    }
    archives.sort();
    if archives.is_empty() && links.is_empty() {
        ctx.add(
            Probe::Crates,
            format!(
                "check broken: 0 .crate archives under {}/registry/cache, so nothing was compared.",
                home.display()
            ),
            "If the job compiles nothing, leave `cargo-home` out of --expect; otherwise `cargo fetch --locked` did not use this CARGO_HOME.",
        );
        return;
    }
    if !links.is_empty() {
        ctx.add(
            Probe::Crates,
            format!("{} link(s) in the registry cache: {}.", links.len(), listed(&links)),
            "The cache must hold archives this run downloaded, never links to a kept registry.",
        );
    }
    if !orphan_indexes.is_empty() {
        ctx.add(
            Probe::Crates,
            format!(
                "registry index folder(s) {} match no registry source of the tagged Cargo.lock files.",
                listed(&orphan_indexes)
            ),
            "Something wrote another registry's cache here: a shared CARGO_HOME, or a lock file missing from --cargo-workspace.",
        );
    }
    if !strays.is_empty() {
        ctx.add(
            Probe::Crates,
            format!(
                "{} file(s) in the registry cache are not .crate archives: {}.",
                strays.len(),
                listed(&strays)
            ),
            "The cache must hold only the archives this run downloaded.",
        );
    }
    let git = home.join("git");
    if git.exists() || std::fs::symlink_metadata(&git).is_ok() {
        if union.has_git {
            ctx.verdict
                .notices
                .push("crates: git dependencies of the lock files are fetched under CARGO_HOME/git and are not matched to a checksum (not measured).".to_string());
        } else {
            ctx.add(
                Probe::Crates,
                format!("{} exists, but no tagged Cargo.lock has a git source.", git.display()),
                "Nothing in the tagged tree needs a git checkout: this is another build's leftover or a planted source.",
            );
        }
    }
    let (mut unknown, mut wrong, mut unreadable) = (Vec::new(), Vec::new(), Vec::new());
    for (a, index) in &archives {
        let stem = a.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
        // Every lock entry of this name-version whose source lives in this index folder.
        let mut candidates: BTreeSet<&String> = BTreeSet::new();
        let mut known = false;
        for ((source, name, version), sums) in &union.sums {
            if format!("{name}-{version}") == stem && index_prefixes(source).iter().any(|p| index.starts_with(p.as_str())) {
                known = true;
                candidates.extend(sums);
            }
        }
        if !known {
            unknown.push(format!("{index}/{stem}"));
            continue;
        }
        match sha256_file(a) {
            Ok(h) if candidates.contains(&h) => ctx.verdict.crates_matched += 1,
            Ok(h) => wrong.push(format!("{stem} (sha256 {h})")),
            Err(e) => unreadable.push(format!("{stem} ({e})")),
        }
    }
    if !unknown.is_empty() {
        ctx.add(
            Probe::Crates,
            format!(
                "{} archive(s) have no entry for their registry in the tagged Cargo.lock files: {}.",
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

/// Files under `dir`, links not followed; a link is reported in `links` and never read.
fn files_under(dir: &Path, out: &mut Vec<PathBuf>, links: &mut Vec<String>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_symlink() => links.push(p.display().to_string()),
                Ok(t) if t.is_dir() => files_under(&p, out, links),
                Ok(_) => out.push(p),
                Err(_) => {}
            }
        }
    }
}

pub(crate) fn tauri_probe(ctx: &mut Ctx) {
    let Some(tools) = ctx.folder(Kind::TauriTools) else { return };
    let on_tag = ctx.env.get("GITHUB_REF").is_some_and(|r| r.starts_with("refs/tags/"))
        || (ctx.env.get("GITHUB_ACTIONS") == Some("true") && ctx.env.get("PADDOCK_RELEASE_MODE") == Some("1"));
    if ctx.opts.allow_unpinned_tauri && on_tag {
        ctx.add(
            Probe::TauriTools,
            "check broken: --allow-unpinned-tauri is refused on a tag run and in release mode in CI (GITHUB_REF starts with refs/tags/, or GITHUB_ACTIONS=true with PADDOCK_RELEASE_MODE=1).",
            "Pass `--tauri-pins <WP-9 pin file>` and drop the flag.",
        );
        return;
    }
    let Some(pins) = ctx.opts.tauri_pins.clone() else {
        if !ctx.opts.allow_unpinned_tauri {
            ctx.add(
                Probe::TauriTools,
                "check broken: tauri-tools: not measured (check broken): no pin file was given, so the tool files were not compared with anything.",
                "Pass `--tauri-pins <WP-9 pin file>`; `--allow-unpinned-tauri` is for a local trial only.",
            );
        } else {
            ctx.verdict.notices.push(
                "tauri-tools: not measured (--allow-unpinned-tauri, local trial): the folder's identity was checked, its content was not."
                    .to_string(),
            );
        }
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
    let mut links = Vec::new();
    files_under(&tools, &mut files, &mut links);
    if !links.is_empty() {
        ctx.add(
            Probe::TauriTools,
            format!("{} link(s) in the Tauri tool folder: {}.", links.len(), listed(&links)),
            "The tools must be files fetched into <build folder>/.tauri, never links to a kept cache.",
        );
    }
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
