//! `disk clean`: removes folders that are merged or stale-pushed and not live, and nothing else.
//!
//! Dry run is the default; only `--apply` removes. The state is worked out again right before each removal, never taken from
//! the index. Worktrees go through `git worktree remove` (no force: git itself refuses a locked or modified one); other
//! folders are removed only when they carry an owner record and lie under a configured root. Orphans, unsaved and live folders,
//! CI slot folders, user folders, runners, sccache, VMs and Docker (none of which carry a record) are never touched.

use super::config::{CommonArgs, Settings, gib};
use super::index::Index;
use super::measure::{folder_size, par_map};
use super::record::{self, Kind};
use super::scan::{Found, Sizes, discover, scan};
use super::state::{Context, State, inspect};
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
    /// Stop judging and removing after this long (for example 10m); what was not reached stays for the next clean.
    #[arg(long, value_name = "DURATION")]
    pub max: Option<String>,
    /// Measure the folders that have no size in the index (slow on a large tree). By default the log uses the indexed sizes and
    /// says "size not measured" for the rest.
    #[arg(long)]
    pub measure: bool,
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
    run_core(a, env, note, before_remove, &|| crate::worktree::procs::process_folders().ok())
}

type Cached = std::collections::HashMap<String, super::index::SizeRow>;

/// The size shown for a folder: the indexed one, or measured when asked, else `None` (the decision never needs it, ER-20).
fn size_of(p: &str, path: &Path, cached: &mut Cached, s: &Settings, measure: bool) -> Option<u64> {
    cached
        .remove(p)
        .map(|r| r.bytes)
        .or_else(|| measure.then(|| folder_size(path, s.folder_timeout).bytes))
}

fn shown(bytes: Option<u64>) -> String {
    bytes.map_or_else(|| "size not measured".to_string(), gib)
}

/// How old the processes' listing of the removal pass may get before it is read again.
pub const RELIST_AFTER: std::time::Duration = std::time::Duration::from_secs(30);

/// Whether a listing of this age is read again.
pub fn needs_relist(age: std::time::Duration) -> bool {
    age > RELIST_AFTER
}

/// [`run_with`] with the processes' folders read through `procs` (tests count and fake the reads). Listing every process is the
/// slow step (lsof), so it is read once to judge all folders (the judging runs in parallel) and once more, fresh, before the
/// first removal; each removal then re-reads its own folder's git state and record.
pub fn run_core(
    a: &CleanArgs,
    env: &dyn Fn(&str) -> Option<String>,
    note: &mut dyn FnMut(&str),
    before_remove: &mut dyn FnMut(&Path),
    procs: &(dyn Fn() -> Option<Vec<std::path::PathBuf>> + Sync),
) -> Result<Report, CliError> {
    let started = std::time::Instant::now();
    let limit = match a.max.as_deref() {
        Some(m) => Some(crate::wait::parse_duration(m).map_err(|e| CliError::usage(e).with_hint("Write --max as 90s, 10m or 1h."))?),
        None => None,
    };
    let out_of_time = || limit.is_some_and(|l| started.elapsed() >= l);
    let s = Settings::resolve(&a.common, env);
    s.located_roots()?;
    let index_path = s.index_file()?.to_path_buf();
    let ctx_now = || Context {
        now: crate::worktree::note::now_secs(),
        live_secs: s.live_secs,
        process_folders: procs(),
    };
    let first = ctx_now();
    if a.apply && first.process_folders.is_none() {
        return Err(
            CliError::refused("The processes' current folders cannot be read here, so whether a folder is in use is not known.").with_hint(
                "Run `./dev disk clean --apply` on a machine where they can be read (Linux, macOS); `--dry-run` works everywhere.",
            ),
        );
    }
    let mut cached = Index::open_read_only(&index_path)
        .and_then(|i| i.latest_sizes())
        .unwrap_or_default();
    let mut text = String::new();
    let (mut removed, mut kept) = (Vec::new(), Vec::new());
    let mut total = 0u64;
    let mut unmeasured = 0usize;
    note(&format!(
        "disk clean: {} (state is worked out fresh; {} thread(s){})",
        if a.apply { "APPLY" } else { "dry run, nothing is removed" },
        s.jobs,
        limit.map_or(String::new(), |l| format!(", at most {}", crate::wait::fmt_duration(l)))
    ));
    let found = discover(&s);
    note(&format!(
        "disk clean: {} folder(s) found in {:.1}s",
        found.len(),
        started.elapsed().as_secs_f64()
    ));
    // Judging is git and file reads per folder: in parallel, bounded by the jobs setting, against the one process listing.
    let judged: Vec<Option<State>> = par_map(&found, s.jobs, |f| {
        if out_of_time() {
            return None;
        }
        Some(inspect(&f.path, f.record.as_ref(), &first).state)
    });
    note(&format!("disk clean: judged in {:.1}s", started.elapsed().as_secs_f64()));
    let mut skipped = 0usize;
    // The processes' listing for the removal pass: read once, and again when it is older than `RELIST_AFTER` (a long pass must not
    // act on a minutes-old listing).
    let mut fresh: Option<(std::time::Instant, Context)> = None;
    for (f, state) in found.iter().zip(judged) {
        let Some(state) = state else {
            skipped += 1;
            continue;
        };
        if let Some(why) = refusal(f, state) {
            // Orphans and non-reclaimable states are the bulk: only the reclaimable-but-refused ones are worth a line.
            if state.reclaimable() {
                kept.push(json!({"path": f.path.display().to_string(), "reason": why}));
                text.push_str(&format!("keep {} ({why})\n", f.path.display()));
            }
            continue;
        }
        if out_of_time() {
            skipped += 1;
            continue;
        }
        let p = f.path.display().to_string();
        let (kind, owner) = f.record.as_ref().map_or(("", String::new()), |r| (r.kind.word(), r.owner.clone()));
        let result = if a.apply {
            before_remove(&f.path);
            // Again, right before acting: the processes read afresh once for the whole removal pass, the clock and the folder's
            // own state for each folder, then the shared check inside `remove`.
            if fresh.as_ref().is_some_and(|(at, _)| needs_relist(at.elapsed())) {
                fresh = None;
            }
            let (_, ctx) = fresh.get_or_insert_with(|| (std::time::Instant::now(), ctx_now()));
            ctx.now = crate::worktree::note::now_secs();
            let again = inspect(&f.path, record::read(&f.path).ok().flatten().as_ref(), ctx).state;
            if again.reclaimable() {
                // The size is read before the folder goes.
                let bytes = size_of(&p, &f.path, &mut cached, &s, a.measure);
                safe_remove::remove(&f.path, &s.roots).map(|_| bytes)
            } else {
                Err(format!("it became {} while cleaning", again.word()))
            }
        } else {
            let bytes = size_of(&p, &f.path, &mut cached, &s, a.measure);
            safe_remove::check(&f.path, &s.roots).map(|_| bytes)
        };
        match result {
            Ok(bytes) => {
                let verb = if a.apply { "removed" } else { "would remove" };
                let line = format!("{p} ({}, {kind}, {}, owner {owner})", shown(bytes), state.word());
                if a.apply {
                    note(&format!("disk clean: removed {line}"));
                }
                text.push_str(&format!("{verb} {line}\n"));
                let mut row = json!({"path": p, "bytes": bytes, "state": state.word(), "kind": kind, "owner": owner});
                if !a.apply {
                    row["dry_run"] = json!(true);
                }
                removed.push(row);
                total += bytes.unwrap_or(0);
                unmeasured += usize::from(bytes.is_none());
            }
            Err(why) => {
                note(&format!("disk clean: kept {p} ({why})"));
                text.push_str(&format!("keep {p} ({why})\n"));
                kept.push(json!({"path": p, "reason": why}));
            }
        }
    }
    if skipped > 0 {
        let line =
            format!("disk clean: stopped at the time limit; {skipped} folder(s) were not judged or removed and stay for the next clean");
        note(&line);
        text.push_str(&format!("{line}\n"));
    }
    text.push_str(&format!(
        "{} {} folder(s), {}{}\n",
        if a.apply { "Removed" } else { "Would remove" },
        removed.len(),
        gib(total),
        if unmeasured > 0 {
            format!(" and {unmeasured} folder(s) of size not measured")
        } else {
            String::new()
        }
    ));
    if a.apply && !out_of_time() {
        let (index, _) = Index::open_or_recover(&index_path, crate::worktree::note::now_secs());
        if let Some(mut i) = index {
            let _ = scan(&s, Sizes::Skip, Some(&mut i), &mut |_| {});
        }
    }
    Ok(Report::new(
        json!({"apply": a.apply, "removed": removed, "kept": kept, "bytes": total, "unmeasured": unmeasured, "skipped": skipped,
            "stoppedAtLimit": skipped > 0, "elapsedMs": started.elapsed().as_millis() as u64}),
        text,
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_process_listing_older_than_30_seconds_is_read_again() {
        use std::time::Duration;
        assert!(!needs_relist(Duration::ZERO));
        assert!(!needs_relist(Duration::from_secs(30)));
        assert!(needs_relist(Duration::from_secs(31)));
    }

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
            max: None,
            measure: false,
        };
        (d, a)
    }

    // Reading other processes' folders is not implemented on Windows (ADR 0083 §1), so it never judges a folder unused: see the test below.
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

    // Reading other processes' folders is not implemented on Windows (ADR 0083 §1), so it never judges a folder unused: see the test below.
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
                .any(|l| l.contains("removed") && l.contains("stale-build") && l.contains("size not measured")),
            "{logged:?}"
        );
        assert_eq!(r.data["removed"].as_array().unwrap().len(), 1);
    }

    // Reading other processes' folders is not implemented on Windows (ADR 0083 §1), so it never judges a folder unused: see the test below.
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

    /// Reading what other processes use is not implemented on Windows (ADR 0083 §1), so a dry run would remove nothing and `--apply` is refused: no folder is ever
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

    /// The listing of every process is the slow step (lsof): one read judges every folder, one more precedes the first removal.
    #[cfg(not(windows))]
    #[test]
    fn the_processes_are_listed_once_to_judge_and_once_before_removing_not_once_per_folder() {
        let (_d, mut a) = setup();
        let reads = std::sync::atomic::AtomicUsize::new(0);
        let count = || {
            reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some(Vec::new())
        };
        let r = run_core(&a, &|_| None, &mut |_| {}, &mut |_| {}, &count).unwrap();
        assert_eq!(r.data["removed"].as_array().unwrap().len(), 1);
        assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 1, "a dry run reads them once");
        reads.store(0, std::sync::atomic::Ordering::SeqCst);
        a.apply = true;
        run_core(&a, &|_| None, &mut |_| {}, &mut |_| {}, &count).unwrap();
        assert_eq!(
            reads.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "apply reads them again, once, before removing"
        );
    }

    /// Positive control: the one listing really keeps a folder a process is in (the seam is not a way to skip the check).
    #[cfg(not(windows))]
    #[test]
    fn a_folder_a_process_is_in_is_kept_by_the_one_listing() {
        let (d, mut a) = setup();
        a.apply = true;
        let inside = d.path().join("dev-targets/stale-build/src");
        let procs = move || Some(vec![inside.clone()]);
        let r = run_core(&a, &|_| None, &mut |_| {}, &mut |_| {}, &procs).unwrap();
        assert!(d.path().join("dev-targets/stale-build").is_dir());
        assert_eq!(r.data["removed"].as_array().unwrap().len(), 0, "{}", r.text);
    }

    #[cfg(not(windows))]
    #[test]
    fn a_clean_past_its_time_limit_removes_nothing_more_and_says_what_it_left() {
        let (d, mut a) = setup();
        a.apply = true;
        a.max = Some("0s".to_string());
        let mut logged = Vec::new();
        let r = run_core(
            &a,
            &|_| None,
            &mut |l| logged.push(l.to_string()),
            &mut |_| {},
            &|| Some(Vec::new()),
        )
        .unwrap();
        assert!(d.path().join("dev-targets/stale-build").is_dir(), "out of time: nothing is removed");
        assert_eq!(r.data["stoppedAtLimit"], true, "{}", r.text);
        assert!(logged.iter().any(|l| l.contains("stopped at the time limit")), "{logged:?}");
        // Control: with room the same tree is cleaned.
        a.max = Some("10m".to_string());
        run_core(&a, &|_| None, &mut |_| {}, &mut |_| {}, &|| Some(Vec::new())).unwrap();
        assert!(!d.path().join("dev-targets/stale-build").exists());
    }

    #[cfg(not(windows))]
    #[test]
    fn sizes_are_measured_only_when_asked_and_say_so_when_not() {
        let (_d, mut a) = setup();
        let plain = run(&a, &|_| None, &mut |_| {}).unwrap();
        assert!(plain.text.contains("size not measured"), "{}", plain.text);
        assert_eq!(plain.data["unmeasured"], 1);
        a.measure = true;
        let sized = run(&a, &|_| None, &mut |_| {}).unwrap();
        assert!(
            !sized.text.contains("size not measured") && sized.text.contains("GiB"),
            "{}",
            sized.text
        );
        assert!(sized.data["bytes"].as_u64().unwrap() >= 10_000);
    }

    #[test]
    fn a_bad_max_is_a_usage_error() {
        let (_d, mut a) = setup();
        a.max = Some("soon".to_string());
        assert!(run(&a, &|_| None, &mut |_| {}).is_err());
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
