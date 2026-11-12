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
use crate::safe_remove;
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

/// Runs `clean`.
pub fn run(a: &CleanArgs, env: &dyn Fn(&str) -> Option<String>, note: &mut dyn FnMut(&str)) -> Result<Report, CliError> {
    run_with(a, env, note, &mut |_| {})
}

/// [`run`] with a hook called for each folder after it was judged removable and before the removal is tried (tests change the
/// folder there, to show that the judgement is made again).
pub fn run_with(
    a: &CleanArgs,
    env: &dyn Fn(&str) -> Option<String>,
    note: &mut dyn FnMut(&str),
    before_remove: &mut dyn FnMut(&Path),
) -> Result<Report, CliError> {
    let s = Settings::resolve(&a.common, env);
    if a.apply && context(&s, crate::worktree::note::now_secs()).process_folders.is_none() {
        return Err(
            CliError::refused("The processes' current folders cannot be read here, so whether a folder is in use is not known.").with_hint(
                "Run `./dev disk clean --apply` on a machine where they can be read (Linux, macOS); `--dry-run` works everywhere.",
            ),
        );
    }
    let mut cached = Index::open_read_only(&s.index).and_then(|i| i.latest_sizes()).unwrap_or_default();
    let mut text = String::new();
    let (mut removed, mut kept) = (Vec::new(), Vec::new());
    let mut total = 0u64;
    note(&format!(
        "disk clean: {} (state is worked out fresh; {} thread(s))",
        if a.apply { "APPLY" } else { "dry run, nothing is removed" },
        s.jobs
    ));
    for f in discover(&s) {
        // The clock and the processes are read again for every folder: a long clean must not judge by what it saw minutes ago.
        let ctx = context(&s, crate::worktree::note::now_secs());
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
        let (kind, owner) = f.record.as_ref().map_or(("", String::new()), |r| (r.kind.word(), r.owner.clone()));
        let result = if a.apply {
            before_remove(&f.path);
            // Again, right before acting: state, processes and the clock afresh, then the shared check inside `remove`.
            let ctx = context(&s, crate::worktree::note::now_secs());
            let again = inspect(&f.path, record::read(&f.path).ok().flatten().as_ref(), &ctx).state;
            if again.reclaimable() {
                // The size is read before the folder goes.
                let bytes = cached
                    .remove(&p)
                    .map(|r| r.bytes)
                    .unwrap_or_else(|| folder_size(&f.path, s.folder_timeout).bytes);
                safe_remove::remove(&f.path, &s.roots).map(|_| bytes)
            } else {
                Err(format!("it became {} while cleaning", again.word()))
            }
        } else {
            let bytes = cached
                .remove(&p)
                .map(|r| r.bytes)
                .unwrap_or_else(|| folder_size(&f.path, s.folder_timeout).bytes);
            safe_remove::check(&f.path, &s.roots).map(|_| bytes)
        };
        match result {
            Ok(bytes) if !a.apply => {
                text.push_str(&format!(
                    "would remove {p} ({}, {kind}, {}, owner {owner})\n",
                    gib(bytes),
                    state.word()
                ));
                removed.push(json!({"path": p, "bytes": bytes, "state": state.word(), "kind": kind, "owner": owner, "dry_run": true}));
                total += bytes;
            }
            Ok(bytes) => {
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
    if a.apply {
        let (index, _) = Index::open_or_recover(&s.index, crate::worktree::note::now_secs());
        if let Some(mut i) = index {
            let _ = scan(&s, Sizes::Skip, Some(&mut i), &mut |_| {});
        }
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

    /// A folder opened so its time can be set: Windows refuses that on a plain `File::open` (access denied) and needs write access
    /// and the backup-semantics flag to open a directory at all.
    fn open_dir_for_write(dir: &Path) -> std::fs::File {
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
            std::fs::OpenOptions::new()
                .write(true)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
                .open(dir)
                .unwrap()
        }
        #[cfg(not(windows))]
        {
            std::fs::File::open(dir).unwrap()
        }
    }

    fn old_record(dir: &Path, kind: Kind, link: &str) {
        let mut r = Record::new(kind, "implementer:t");
        r.created = 1_000_000;
        r.last_used = 1_000_000;
        r.link = link.to_string();
        record::write(dir, &r).unwrap();
        // The folder's own time counts as use too: age it.
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        open_dir_for_write(dir).set_modified(old).unwrap();
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

    // Windows cannot read other processes' folders, so it never judges a folder unused: see the test below.
    #[cfg(not(windows))]
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

    // Windows cannot read other processes' folders, so it never judges a folder unused: see the test below.
    #[cfg(not(windows))]
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

    // Windows cannot read other processes' folders, so it never judges a folder unused: see the test below.
    #[cfg(not(windows))]
    #[test]
    fn a_folder_used_after_it_was_judged_removable_is_judged_again_and_kept() {
        let (d, mut a) = setup();
        a.apply = true;
        let victim = d.path().join("dev-targets/stale-build");
        let mut logged = Vec::new();
        // Between the first judgement and the removal someone uses the folder: its record is touched.
        let r = run_with(&a, &|_| None, &mut |l| logged.push(l.to_string()), &mut |p| {
            if p == victim {
                let mut rec = record::read(p).unwrap().unwrap();
                rec.last_used = crate::worktree::note::now_secs();
                record::write(p, &rec).unwrap();
            }
        })
        .unwrap();
        assert!(victim.is_dir(), "it became live while cleaning and must stay");
        assert!(logged.iter().any(|l| l.contains("became live")), "{logged:?}");
        assert_eq!(r.data["removed"].as_array().unwrap().len(), 0);
    }

    /// Windows cannot read what other processes use, so a dry run would remove nothing and `--apply` is refused: no folder is ever
    /// removed on a guess (the cleaning tests above run where the process list can be read).
    #[cfg(windows)]
    #[test]
    fn on_windows_a_dry_run_would_remove_nothing_and_apply_is_refused() {
        let (d, mut a) = setup();
        let r = run(&a, &|_| None, &mut |_| {}).unwrap();
        assert!(r.text.contains("Would remove 0 folder(s)"), "{}", r.text);
        assert!(r.text.contains("not implemented on this OS"), "{}", r.text);
        a.apply = true;
        let e = run(&a, &|_| None, &mut |_| {}).unwrap_err();
        assert!(e.message.contains("cannot be read here"), "{}", e.message);
        for n in ["stale-build", "fresh-build", "stale-slot", "stale-orphan"] {
            assert!(d.path().join("dev-targets").join(n).is_dir(), "{n} must still be there");
        }
    }

    #[test]
    fn a_missing_root_is_not_measured_in_the_report() {
        let d = tempfile::tempdir().unwrap();
        let args = crate::disk::report::ReportArgs {
            common: CommonArgs {
                roots: vec![d.path().join("not-mounted")],
                index: Some(d.path().join("i.sqlite")),
                ..CommonArgs::default()
            },
            all: false,
            summary: false,
        };
        let r = crate::disk::report::run(&args, &|_| None, &mut |_| {}).unwrap();
        assert!(r.text.contains("not measured (the folder does not exist"), "{}", r.text);
    }
}
