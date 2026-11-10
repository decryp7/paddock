//! The link scan: no symlink or junction inside a `job` or `run` folder leads outside the job.
//!
//! A job folder can be fresh and still read a kept one through a link (`registry/src`, `node_modules` and `target/release`
//! pointed at a kept folder all pass an identity check of the folder itself). The walk reads each folder **without following
//! links** (`DirEntry::file_type` does not follow them) and resolves every link it meets; its target must lie inside the job
//! folder (an npm workspace link into the clone does) or inside the folder being walked. `<clone>/node_modules` is stricter:
//! absent, a real folder, or the ledger's `node-modules` folder.

use super::ledger::{Kind, Mode};
use super::verify::{Ctx, Probe};
use std::fs::DirEntry;
use std::path::{Path, PathBuf};

const LIST_LIMIT: usize = 20;

fn is_link(e: &DirEntry) -> bool {
    let Ok(t) = e.file_type() else { return false };
    if t.is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        // A junction is a directory with a reparse point.
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        return e
            .metadata()
            .map(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
            .unwrap_or(false);
    }
    #[cfg(not(windows))]
    false
}

/// What a walk found.
struct Walk {
    bad: Vec<String>,
    links_ok: usize,
}

fn walk(dir: &Path, allowed: &[PathBuf], out: &mut Walk) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if is_link(&e) {
            match p.canonicalize() {
                Ok(target) if allowed.iter().any(|a| target.starts_with(a)) => out.links_ok += 1,
                Ok(target) => out.bad.push(format!("{} -> {}", p.display(), target.display())),
                Err(_) => out.bad.push(format!("{} -> (a target that cannot be resolved)", p.display())),
            }
        } else if e.file_type().is_ok_and(|t| t.is_dir()) {
            walk(&p, allowed, out);
        }
    }
}

pub(crate) fn links_probe(ctx: &mut Ctx) {
    let root = ctx.ledger.job_root.canonicalize().unwrap_or_else(|_| ctx.ledger.job_root.clone());
    let mut roots: Vec<PathBuf> = ctx
        .ledger
        .entries
        .iter()
        .filter(|e| matches!(Mode::parse(&e.mode), Some(Mode::Job | Mode::Run)))
        .filter_map(|e| Path::new(&e.path).canonicalize().ok())
        .filter(|p| p.is_dir())
        .collect();
    roots.sort();
    roots.dedup();
    // A folder inside another one that is walked is seen by that walk.
    let mut top: Vec<PathBuf> = Vec::new();
    for r in roots {
        if !top.iter().any(|t| r.starts_with(t)) {
            top.push(r);
        }
    }
    let mut out = Walk {
        bad: Vec::new(),
        links_ok: 0,
    };
    for dir in &top {
        walk(dir, &[root.clone(), dir.clone()], &mut out);
    }
    if !out.bad.is_empty() {
        let shown: Vec<String> = out.bad.iter().take(LIST_LIMIT).cloned().collect();
        let more = out.bad.len().saturating_sub(LIST_LIMIT);
        ctx.add(
            Probe::Links,
            format!(
                "{} link(s) inside the job's folders lead outside the job: {}{}.",
                out.bad.len(),
                shown.join(", "),
                if more > 0 { format!(" and {more} more") } else { String::new() }
            ),
            "Release mode builds from folders this run made: remove the step that links a kept folder (registry, node_modules, target, caches).",
        );
    }
    // `<clone>/node_modules`: absent, a real folder, or the ledger's node-modules folder.
    if let Some(clone) = ctx.folder(Kind::Workspace) {
        let nm = clone.join("node_modules");
        if std::fs::symlink_metadata(&nm).is_ok_and(|m| m.file_type().is_symlink()) {
            let ledger_nm = ctx.folder(Kind::NodeModules);
            let ok = nm.canonicalize().ok().is_some_and(|t| Some(&t) == ledger_nm.as_ref());
            if !ok {
                ctx.add(
                    Probe::Links,
                    format!("{} is a link, but not to the node-modules folder `setup` made.", nm.display()),
                    "`npm ci` must write the job's own node_modules (PADDOCK_RELEASE_NODE_MODULES) or a real folder in the clone.",
                );
            }
        }
    }
}
