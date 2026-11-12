//! `disk clean`: removes folders that are merged or stale-pushed and not live, and nothing else.
//!
//! Dry run is the default; only `--apply` removes. The state is worked out again right before each removal, never taken from
//! the index. Worktrees go through `git worktree remove` (no force: git itself refuses a locked or modified one); other
//! folders are removed only when they carry an owner record and lie under a configured root. Orphans, unsaved and live folders,
//! CI slot folders, user folders, runners, sccache, VMs and Docker (none of which carry a record) are never touched.

use super::config::{CommonArgs, Settings, gib};
use super::index::Index;
use super::measure::folder_size;
use super::record::{self, Kind};
use super::scan::{Found, Sizes, context, discover, scan};
use super::state::{State, inspect};
use crate::worktree::git::{Repo, key};
use crate::worktree::owners::OwnerStore;
use clap::Args;
use paddock_cli::{CliError, Report};
use serde_json::json;
use std::path::Path;

/// `disk clean`'s arguments.
#[derive(Args, Debug, Clone)]
pub struct CleanArgs {
    #[command(flatten)]
    /// The shared settings flags.
    pub common: CommonArgs,
    /// Remove what is reclaimable. Without it nothing is removed.
    #[arg(long, conflicts_with = "dry_run")]
    pub apply: bool,
    /// Say what would be removed and remove nothing (the default).
    #[arg(long)]
    pub dry_run: bool,
}

/// Why a folder may not be removed, `None` when it may. `kind` and `state` are the freshly worked-out ones.
pub fn refusal(f: &Found, state: State) -> Option<String> {
    let Some(rec) = &f.record else {
        return Some("no owner record".to_string());
    };
    if !state.reclaimable() {
        return Some(format!("it is {}", state.word()));
    }
    match rec.kind {
        Kind::Slot => Some("a CI slot folder: the CI kit sweeps those itself".to_string()),
        Kind::Worktree | Kind::Build | Kind::Scratch => None,
    }
}

fn safe_folder(path: &Path, s: &Settings) -> Result<(), String> {
    let meta = path.symlink_metadata().map_err(|e| format!("cannot read it: {e}"))?;
    if !meta.is_dir() {
        return Err("not a real folder (a symlink or a file)".into());
    }
    let real = path.canonicalize().map_err(|e| format!("cannot resolve it: {e}"))?;
    let under = s
        .roots
        .iter()
        .filter_map(|r| r.canonicalize().ok())
        .any(|r| real.starts_with(&r) && real != r);
    if !under {
        return Err("not below a configured root".into());
    }
    if record::read(path).ok().flatten().is_none() {
        return Err("its owner record is gone".into());
    }
    Ok(())
}

fn remove_worktree(path: &Path) -> Result<(), String> {
    let repo = Repo::discover(path).map_err(|e| e.to_string())?;
    let k = key(path);
    repo.git()
        .text(&["worktree", "remove", &k])
        .map_err(|e| format!("git would not remove it: {}", e.stderr.trim()))?;
    let _ = OwnerStore::open(&repo.common_dir).update(|m| m.remove(&k));
    Ok(())
}

/// Runs `clean`.
pub fn run(a: &CleanArgs, env: &dyn Fn(&str) -> Option<String>, note: &mut dyn FnMut(&str)) -> Result<Report, CliError> {
    let s = Settings::resolve(&a.common, env);
    let now = crate::worktree::note::now_secs();
    let ctx = context(&s, now);
    let mut cached = Index::open(&s.index).ok().and_then(|i| i.latest_sizes().ok()).unwrap_or_default();
    let mut text = String::new();
    let (mut removed, mut kept) = (Vec::new(), Vec::new());
    let mut total = 0u64;
    note(&format!(
        "disk clean: {} (state is worked out fresh; {} thread(s))",
        if a.apply { "APPLY" } else { "dry run, nothing is removed" },
        s.jobs
    ));
    for f in discover(&s) {
        let state = inspect(&f.path, f.record.as_ref(), &ctx).state;
        if let Some(why) = refusal(&f, state) {
            // Orphans and non-reclaimable states are the bulk: only the reclaimable-but-refused ones are worth a line.
            if state.reclaimable() {
                kept.push(json!({"path": f.path.display().to_string(), "reason": why}));
                text.push_str(&format!("keep {} ({why})\n", f.path.display()));
            }
            continue;
        }
        let p = f.path.display().to_string();
        let bytes = cached
            .remove(&p)
            .map(|r| r.bytes)
            .unwrap_or_else(|| folder_size(&f.path, s.folder_timeout).bytes);
        let (kind, owner) = f.record.as_ref().map_or(("", String::new()), |r| (r.kind.word(), r.owner.clone()));
        if !a.apply {
            text.push_str(&format!(
                "would remove {p} ({}, {kind}, {}, owner {owner})\n",
                gib(bytes),
                state.word()
            ));
            removed.push(json!({"path": p, "bytes": bytes, "state": state.word(), "kind": kind, "owner": owner, "dry_run": true}));
            total += bytes;
            continue;
        }
        // Again, right before acting: the folder may have been used or changed since the first look.
        let again = inspect(&f.path, record::read(&f.path).ok().flatten().as_ref(), &ctx).state;
        let result = if !again.reclaimable() {
            Err(format!("it became {} while cleaning", again.word()))
        } else {
            safe_folder(&f.path, &s).and_then(|()| match f.record.as_ref().map(|r| r.kind) {
                Some(Kind::Worktree) => remove_worktree(&f.path),
                _ => std::fs::remove_dir_all(&f.path).map_err(|e| format!("could not remove it: {e}")),
            })
        };
        match result {
            Ok(()) => {
                note(&format!(
                    "disk clean: removed {p} ({}, {kind}, {}, owner {owner})",
                    gib(bytes),
                    state.word()
                ));
                text.push_str(&format!("removed {p} ({}, {kind}, {}, owner {owner})\n", gib(bytes), state.word()));
                removed.push(json!({"path": p, "bytes": bytes, "state": state.word(), "kind": kind, "owner": owner}));
                total += bytes;
            }
            Err(why) => {
                note(&format!("disk clean: kept {p} ({why})"));
                text.push_str(&format!("keep {p} ({why})\n"));
                kept.push(json!({"path": p, "reason": why}));
            }
        }
    }
    text.push_str(&format!(
        "{} {} folder(s), {}\n",
        if a.apply { "Removed" } else { "Would remove" },
        removed.len(),
        gib(total)
    ));
    if a.apply
        && let Ok(mut i) = Index::open(&s.index)
    {
        let _ = scan(&s, Sizes::Skip, Some(&mut i), &mut |_| {});
    }
    Ok(Report::new(
        json!({"apply": a.apply, "removed": removed, "kept": kept, "bytes": total}),
        text,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::record::Record;

    fn found(kind: Option<Kind>) -> Found {
        Found {
            path: "/r/x".into(),
            record: kind.map(|k| Record::new(k, "a:b")),
            record_error: None,
        }
    }

    #[test]
    fn only_merged_and_stale_pushed_folders_with_a_record_may_go() {
        assert!(refusal(&found(Some(Kind::Worktree)), State::Merged).is_none());
        assert!(refusal(&found(Some(Kind::Build)), State::StalePushed).is_none());
        for state in [State::Live, State::Unsaved, State::Orphan] {
            assert!(refusal(&found(Some(Kind::Build)), state).is_some(), "{state:?}");
        }
        assert!(refusal(&found(None), State::Merged).unwrap().contains("no owner record"));
        assert!(refusal(&found(Some(Kind::Slot)), State::Merged).unwrap().contains("CI kit"));
    }

    fn old_record(dir: &Path, kind: Kind, link: &str) {
        let mut r = Record::new(kind, "implementer:t");
        r.created = 1_000_000;
        r.last_used = 1_000_000;
        r.link = link.to_string();
        record::write(dir, &r).unwrap();
        // The folder's own time counts as use too: age it.
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        std::fs::File::open(dir).unwrap().set_modified(old).unwrap();
    }

    fn setup() -> (tempfile::TempDir, CleanArgs) {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("dev-targets");
        for n in ["stale-build", "fresh-build", "stale-slot", "stale-orphan"] {
            std::fs::create_dir_all(root.join(n)).unwrap();
            std::fs::write(root.join(n).join("data"), vec![0u8; 10_000]).unwrap();
        }
        old_record(&root.join("stale-build"), Kind::Build, "");
        record::write(&root.join("fresh-build"), &Record::new(Kind::Build, "implementer:t")).unwrap();
        old_record(&root.join("stale-slot"), Kind::Slot, "");
        // stale-orphan has no record at all.
        let a = CleanArgs {
            common: CommonArgs {
                roots: vec![root],
                index: Some(d.path().join("i.sqlite")),
                jobs: Some(2),
                ..CommonArgs::default()
            },
            apply: false,
            dry_run: false,
        };
        (d, a)
    }

    #[test]
    fn the_default_is_a_dry_run_that_removes_nothing_and_names_what_it_would() {
        let (d, a) = setup();
        let r = run(&a, &|_| None, &mut |_| {}).unwrap();
        assert!(r.text.contains("would remove") && r.text.contains("stale-build"), "{}", r.text);
        assert!(r.text.contains("Would remove 1 folder(s)"), "{}", r.text);
        for n in ["stale-build", "fresh-build", "stale-slot", "stale-orphan"] {
            assert!(d.path().join("dev-targets").join(n).is_dir(), "{n} must still be there");
        }
    }

    #[test]
    fn apply_removes_only_the_stale_recorded_build_folder_and_logs_its_path_and_size() {
        let (d, mut a) = setup();
        a.apply = true;
        let mut logged = Vec::new();
        let r = run(&a, &|_| None, &mut |l| logged.push(l.to_string())).unwrap();
        let root = d.path().join("dev-targets");
        assert!(!root.join("stale-build").exists());
        for keep in ["fresh-build", "stale-slot", "stale-orphan"] {
            assert!(root.join(keep).is_dir(), "{keep} must be kept");
        }
        assert!(
            logged
                .iter()
                .any(|l| l.contains("removed") && l.contains("stale-build") && l.contains("GiB")),
            "{logged:?}"
        );
        assert_eq!(r.data["removed"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_folder_whose_record_vanishes_before_removal_is_kept() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("r");
        std::fs::create_dir_all(root.join("x")).unwrap();
        let s = Settings::resolve(
            &CommonArgs {
                roots: vec![root.clone()],
                ..CommonArgs::default()
            },
            &|_| None,
        );
        assert!(safe_folder(&root.join("x"), &s).unwrap_err().contains("record is gone"));
        old_record(&root.join("x"), Kind::Build, "");
        assert!(safe_folder(&root.join("x"), &s).is_ok());
        assert!(safe_folder(&root, &s).is_err(), "a root itself is never removed");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("x"), root.join("link")).unwrap();
            assert!(safe_folder(&root.join("link"), &s).unwrap_err().contains("symlink"));
        }
    }
}
