//! The operating-system side of container engine hygiene and the Actions cache folder (ADR 0016): finding the engine,
//! running its commands with timeouts, measuring and trimming folders. What to remove and how to read the engine's
//! output are `pitcrew_core::hygiene`'s.
//!
//! Everything here is defensive: a missing engine or a stopped daemon is a report with a next step, not an error;
//! folders are measured without following links and within limits; a cache folder that is too broad is never touched.

use crate::process::{quiet_command, run_with_timeout};
use pitcrew_core::housekeeping::{failure_reason, MAX_ENTRIES};
use pitcrew_core::hygiene::{
    self, CacheEntry, CacheLimits, EngineKind, EngineReport, EngineUse, FolderUse, PruneKind, PruneResult, PruneSelection, MAX_EXAMPLES,
};
use pitcrew_core::protocol::{CacheInfo, CachePlan};
use pitcrew_core::runner_settings::RunnerSettings;
use pitcrew_core::time::Timestamp;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A quick engine command (a listing).
const LIST_TIMEOUT: Duration = Duration::from_secs(60);
/// A prune.
const PRUNE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Most files one folder measurement visits.
const MAX_FILES: usize = 300_000;
/// Longest one folder measurement runs.
const MEASURE_TIME: Duration = Duration::from_secs(20);
/// Most cache entries listed in the report.
const TOP: usize = 50;

/// Finds `name` (with `.exe` on Windows) on `path` (a `PATH`-style list).
pub fn find_program(name: &str, path: Option<&str>) -> Option<PathBuf> {
    let list = path.map(ToString::to_string).or_else(|| std::env::var("PATH").ok())?;
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&list).map(|d| d.join(&file)).find(|p| p.is_file())
}

/// The engine found on `path`: Docker first, then Podman.
pub fn find_engine(path: Option<&str>) -> Option<(EngineKind, PathBuf)> {
    [EngineKind::Docker, EngineKind::Podman]
        .into_iter()
        .find_map(|k| find_program(k.program(), path).map(|p| (k, p)))
}

fn run(program: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut cmd = quiet_command(program);
    cmd.args(args);
    let out = run_with_timeout(cmd, timeout).map_err(|e| e.to_string())?;
    if out.success {
        Ok(out.stdout)
    } else {
        let msg = out.stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("the command failed");
        Err(msg.chars().take(300).collect())
    }
}

fn engine_problem(kind: EngineKind, why: &str) -> String {
    format!(
        "{} did not answer ({why}). Start the engine (Docker Desktop, OrbStack, `podman machine start`) and try again.",
        kind.program()
    )
}

/// The folders whose size belongs in the report: act's action checkouts and the default cache server folder.
pub fn act_folders(home: Option<&Path>) -> Vec<(String, PathBuf)> {
    let Some(h) = home else { return vec![] };
    vec![
        ("Action checkouts (act)".to_string(), h.join(".cache").join("act")),
        ("Cache server (default)".to_string(), h.join(".cache").join("actcache")),
    ]
}

/// The user's home folder.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// The report: the engine's disk use and the act folders' sizes.
pub fn report(path: Option<&str>, home: Option<&Path>, now: Timestamp) -> EngineReport {
    let folders = act_folders(home)
        .into_iter()
        .map(|(label, p)| {
            let m = measure_dir(&p);
            FolderUse {
                label,
                path: p.display().to_string(),
                exists: m.exists,
                bytes: m.bytes,
                entries: m.entries,
                truncated: m.truncated,
            }
        })
        .collect();
    let Some((kind, program)) = find_engine(path) else {
        return EngineReport {
            engine: None,
            problem: Some(
                "No container engine (docker or podman) was found. Runners that use container jobs need one; Pitcrew only reports on it."
                    .into(),
            ),
            usage: vec![],
            folders,
            at: now,
        };
    };
    let df = run(
        &program,
        &[
            "system",
            "df",
            "--format",
            "{{.Type}}\t{{.TotalCount}}\t{{.Active}}\t{{.Size}}\t{{.Reclaimable}}",
        ],
        LIST_TIMEOUT,
    );
    let (usage, problem): (Vec<EngineUse>, Option<String>) = match df {
        Ok(out) => (hygiene::parse_df(&out), None),
        Err(e) => (vec![], Some(engine_problem(kind, &e))),
    };
    EngineReport {
        engine: Some(kind),
        problem,
        usage,
        folders,
        at: now,
    }
}

fn lines(program: &Path, args: &[&str]) -> Result<String, String> {
    run(program, args, LIST_TIMEOUT)
}

/// What pruning `selection` would remove, from the engine's listings.
pub fn preview(path: Option<&str>, selection: &PruneSelection) -> Result<hygiene::PrunePreview, String> {
    selection.validate()?;
    let (kind, program) = find_engine(path).ok_or("No container engine (docker or podman) was found.")?;
    let until = selection.until();
    let fail = |e: String| engine_problem(kind, &e);
    let mut items = vec![];
    let images_fmt = "{{.ID}}\t{{.Repository}}:{{.Tag}}\t{{.Size}}";
    for k in &selection.kinds {
        match k {
            PruneKind::StoppedContainers => {
                let mut args = vec![
                    "ps",
                    "-a",
                    "--filter",
                    "status=exited",
                    "--filter",
                    "status=created",
                    "--format",
                    "{{.ID}}\t{{.Names}}\t{{.Image}}",
                ];
                let f;
                if let Some(u) = &until {
                    f = format!("until={u}");
                    args.extend(["--filter", &f]);
                }
                let out = lines(&program, &args).map_err(fail)?;
                let rows: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
                items.push(hygiene::PreviewItem {
                    kind: *k,
                    count: u32::try_from(rows.len()).unwrap_or(u32::MAX),
                    bytes: 0,
                    examples: rows.iter().take(MAX_EXAMPLES).map(|r| r.replace('\t', "  ")).collect(),
                });
            }
            PruneKind::DanglingImages => {
                let out = lines(&program, &["images", "--filter", "dangling=true", "--format", images_fmt]).map_err(fail)?;
                items.push(hygiene::item(*k, &hygiene::parse_images(&out)));
            }
            PruneKind::UnusedImages => {
                let mut args = vec!["images", "--format", images_fmt];
                let f;
                if let Some(u) = &until {
                    f = format!("until={u}");
                    args.extend(["--filter", &f]);
                }
                let out = lines(&program, &args).map_err(fail)?;
                let used = lines(&program, &["ps", "-a", "--format", "{{.Image}}"]).map_err(fail)?;
                let used: Vec<String> = used.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
                let unused: Vec<_> = hygiene::unused_images(&hygiene::parse_images(&out), &used)
                    .into_iter()
                    .filter(|i| !hygiene::is_dangling(i))
                    .collect();
                items.push(hygiene::item(*k, &unused));
            }
            PruneKind::BuildCache => {
                let out = lines(
                    &program,
                    &[
                        "system",
                        "df",
                        "--format",
                        "{{.Type}}\t{{.TotalCount}}\t{{.Active}}\t{{.Size}}\t{{.Reclaimable}}",
                    ],
                )
                .map_err(fail)?;
                let cache = hygiene::parse_df(&out)
                    .into_iter()
                    .find(|u| u.what.eq_ignore_ascii_case("Build Cache"));
                items.push(hygiene::PreviewItem {
                    kind: *k,
                    count: cache.as_ref().map_or(0, |c| c.total.saturating_sub(c.active)),
                    bytes: cache.map_or(0, |c| c.reclaimable_bytes),
                    examples: vec![],
                });
            }
            PruneKind::Volumes => {
                let out = lines(&program, &["volume", "ls", "--filter", "dangling=true", "--format", "{{.Name}}"]).map_err(fail)?;
                let names: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
                items.push(hygiene::PreviewItem {
                    kind: *k,
                    count: u32::try_from(names.len()).unwrap_or(u32::MAX),
                    bytes: 0,
                    examples: names.iter().take(MAX_EXAMPLES).map(ToString::to_string).collect(),
                });
            }
        }
    }
    Ok(hygiene::preview(items))
}

/// Prunes `selection`. The caller has checked that no job runs.
pub fn prune(path: Option<&str>, selection: &PruneSelection, now: Timestamp) -> Result<PruneResult, String> {
    selection.validate()?;
    let (_, program) = find_engine(path).ok_or("No container engine (docker or podman) was found.")?;
    let mut freed = vec![];
    let mut failures = vec![];
    for k in &selection.kinds {
        let args = hygiene::prune_args(*k, selection);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        match run(&program, &args, PRUNE_TIMEOUT) {
            Ok(out) => freed.push((*k, hygiene::parse_reclaimed(&out))),
            Err(e) => failures.push((*k, e)),
        }
    }
    Ok(PruneResult { freed, failures, at: now })
}

// ---- folders -----------------------------------------------------------------------------------------------------

/// What measuring a folder found.
pub struct Measured {
    exists: bool,
    bytes: u64,
    entries: u32,
    truncated: bool,
}

fn modified_ms(m: &std::fs::Metadata) -> Timestamp {
    let t = m.modified().unwrap_or(UNIX_EPOCH);
    Timestamp(u64::try_from(t.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()).unwrap_or(0))
}

/// Size and newest change of a path, without following links, within the limits.
fn tree(path: &Path, files: &mut usize, until: Instant, truncated: &mut bool) -> (u64, Timestamp) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return (0, Timestamp(0));
    };
    if !meta.is_dir() {
        return (meta.len(), modified_ms(&meta));
    }
    let (mut total, mut newest) = (0, modified_ms(&meta));
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(list) = std::fs::read_dir(&dir) else { continue };
        for e in list.flatten() {
            *files += 1;
            if *files > MAX_FILES || Instant::now() > until {
                *truncated = true;
                return (total, newest);
            }
            let Ok(m) = e.metadata() else { continue };
            newest = newest.max(modified_ms(&m));
            if m.is_dir() && !m.file_type().is_symlink() {
                stack.push(e.path());
            } else {
                total += m.len();
            }
        }
    }
    (total, newest)
}

/// Measures a folder: size and the number of entries directly inside.
pub fn measure_dir(path: &Path) -> Measured {
    if !path.is_dir() {
        return Measured {
            exists: false,
            bytes: 0,
            entries: 0,
            truncated: false,
        };
    }
    let (mut files, mut truncated) = (0, false);
    let (bytes, _) = tree(path, &mut files, Instant::now() + MEASURE_TIME, &mut truncated);
    let entries = std::fs::read_dir(path).map_or(0, |l| u32::try_from(l.flatten().count()).unwrap_or(u32::MAX));
    Measured {
        exists: true,
        bytes,
        entries,
        truncated,
    }
}

/// The entries directly inside `dir` with their sizes (bounded).
pub fn cache_entries(dir: &Path) -> (Vec<CacheEntry>, bool) {
    let (mut files, mut truncated) = (0, false);
    let until = Instant::now() + MEASURE_TIME;
    let mut out = vec![];
    if let Ok(list) = std::fs::read_dir(dir) {
        for e in list.flatten().take(MAX_ENTRIES) {
            let (bytes, modified) = tree(&e.path(), &mut files, until, &mut truncated);
            out.push(CacheEntry {
                name: e.file_name().to_string_lossy().into_owned(),
                bytes,
                modified,
            });
        }
    }
    (out, truncated)
}

/// The cache folder a runner uses and whether it is the default: `cache.dir` of its configuration (relative paths are
/// relative to the runner's folder), else `$HOME/.cache/actcache`.
pub fn cache_dir_of(settings: &RunnerSettings, runner_dir: &Path, home: Option<&Path>) -> Option<(PathBuf, bool)> {
    match settings.cache_dir.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => {
            let p = PathBuf::from(d);
            Some((if p.is_absolute() { p } else { runner_dir.join(p) }, false))
        }
        None => home.map(|h| (h.join(".cache").join("actcache"), true)),
    }
}

/// `10GB`, `512MB`, `1TiB` as MB (0: no limit).
pub fn size_limit_mb(text: &str) -> Option<u64> {
    hygiene::parse_size(text).map(|b| b / (1024 * 1024))
}

/// The cache report of a runner.
pub fn cache_report(
    settings: &RunnerSettings,
    runner_dir: &Path,
    home: Option<&Path>,
    free_bytes: Option<u64>,
) -> Result<CacheInfo, String> {
    let (dir, default_dir) =
        cache_dir_of(settings, runner_dir, home).ok_or("the cache folder is not known: this account has no home folder")?;
    let problem = hygiene::safe_cache_dir(&dir, home).err();
    let exists = dir.is_dir();
    let (mut entries, truncated) = if exists { cache_entries(&dir) } else { (vec![], false) };
    let bytes: u64 = entries.iter().map(|e| e.bytes).sum();
    let count = u32::try_from(entries.len()).unwrap_or(u32::MAX);
    entries.sort_by_key(|e| std::cmp::Reverse(e.bytes));
    entries.truncate(TOP);
    let limit = settings.cache_size_limit.as_deref().and_then(size_limit_mb).filter(|l| *l > 0);
    Ok(CacheInfo {
        enabled: settings.cache_enabled,
        dir: dir.display().to_string(),
        default_dir,
        exists,
        bytes,
        entries: count,
        top: entries,
        size_limit_mb: limit,
        over_limit: limit.is_some_and(|l| bytes > l * 1024 * 1024),
        free_bytes,
        truncated,
        problem,
    })
}

/// The plan for `limits` over the folder's entries.
pub fn cache_plan(dir: &Path, limits: &CacheLimits, home: Option<&Path>, now: Timestamp) -> Result<CachePlan, String> {
    limits.validate()?;
    hygiene::safe_cache_dir(dir, home)?;
    let (entries, _) = cache_entries(dir);
    let entries = hygiene::plan_cache(&entries, limits, now);
    let bytes = entries.iter().map(|(e, _)| e.bytes).sum();
    Ok(CachePlan { entries, bytes })
}

/// Deletes the planned entries (names are re-checked: a single path component inside `dir`, never a link target).
pub fn cache_clean(dir: &Path, plan: &CachePlan) -> Vec<(String, String)> {
    let mut failures = vec![];
    for (e, _) in &plan.entries {
        let name = Path::new(&e.name);
        if name.components().count() != 1 || e.name == ".." || e.name == "." {
            failures.push((e.name.clone(), "the name is not a plain entry of the cache folder".into()));
            continue;
        }
        let path = dir.join(name);
        let result = match std::fs::symlink_metadata(&path) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => std::fs::remove_dir_all(&path),
            Ok(_) => std::fs::remove_file(&path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        };
        if let Err(err) = result {
            failures.push((e.name.clone(), failure_reason(err.raw_os_error(), cfg!(windows), &err.to_string())));
        }
    }
    failures
}

/// The age of `t` for tests and messages.
pub fn now_ts() -> Timestamp {
    Timestamp(u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitcrew_core::hygiene::CacheReason;

    fn temp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("pitcrew-it-hyg-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn programs_are_found_on_the_given_path_only() {
        let d = temp("find");
        let file = d.join(format!("docker{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&file, "x").unwrap();
        let path = std::env::join_paths([d.clone()]).unwrap().to_string_lossy().into_owned();
        assert_eq!(find_engine(Some(&path)), Some((EngineKind::Docker, file)));
        let empty = temp("find-none");
        assert!(find_engine(Some(&empty.display().to_string())).is_none());
        let _ = std::fs::remove_dir_all(d);
        let _ = std::fs::remove_dir_all(empty);
    }

    #[test]
    fn a_missing_engine_is_a_report_with_a_next_step_and_the_act_folders_are_measured() {
        let home = temp("home");
        std::fs::create_dir_all(home.join(".cache/act/checkout")).unwrap();
        std::fs::write(home.join(".cache/act/checkout/a.txt"), vec![0u8; 2048]).unwrap();
        let empty = temp("nopath");
        let r = report(Some(&empty.display().to_string()), Some(&home), Timestamp(5));
        assert!(r.engine.is_none() && r.problem.as_deref().unwrap().contains("No container engine"));
        let act = &r.folders[0];
        assert!(act.exists && act.bytes == 2048 && act.entries == 1, "{act:?}");
        assert!(!r.folders[1].exists);
        assert!(preview(
            Some(&empty.display().to_string()),
            &PruneSelection {
                kinds: vec![PruneKind::Volumes],
                older_than_days: 0
            }
        )
        .is_err());
        assert!(prune(
            Some(&empty.display().to_string()),
            &PruneSelection {
                kinds: vec![],
                older_than_days: 0
            },
            Timestamp(1)
        )
        .is_err());
        let _ = std::fs::remove_dir_all(home);
        let _ = std::fs::remove_dir_all(empty);
    }

    #[cfg(unix)]
    #[test]
    fn a_fake_engine_is_asked_for_a_preview_and_a_prune() {
        use std::os::unix::fs::PermissionsExt;
        let d = temp("fake");
        let script = d.join("docker");
        std::fs::write(
            &script,
            "#!/bin/sh\ncase \"$1 $2\" in\n\
             'system df') printf 'Images\\t3\\t1\\t1GB\\t600MB (60%%)\\nBuild Cache\\t4\\t0\\t2GB\\t2GB\\n';;\n\
             'images --filter') printf 'abc\\t<none>:<none>\\t100MB\\n';;\n\
             'images --format') printf 'abc\\t<none>:<none>\\t100MB\\nddd\\tnode:20\\t1GB\\neee\\told:1\\t50MB\\n';;\n\
             'ps -a') printf 'old:1\\n';;\n\
             'image prune') echo 'Total reclaimed space: 100MB';;\n\
             'volume prune') echo 'boom' >&2; exit 1;;\n\
             *) exit 0;;\nesac\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = d.display().to_string();
        let r = report(Some(&path), None, Timestamp(1));
        assert_eq!(r.engine, Some(EngineKind::Docker));
        assert_eq!(r.usage.len(), 2);
        let sel = PruneSelection {
            kinds: vec![PruneKind::DanglingImages, PruneKind::UnusedImages, PruneKind::BuildCache],
            older_than_days: 0,
        };
        let p = preview(Some(&path), &sel).unwrap();
        assert_eq!(p.items[0].count, 1);
        assert_eq!(
            p.items[1].examples,
            vec!["node:20 (ddd)".to_string()],
            "the dangling image and the one a container uses are not listed"
        );
        assert_eq!(p.items[2].bytes, 2_000_000_000);
        let res = prune(
            Some(&path),
            &PruneSelection {
                kinds: vec![PruneKind::DanglingImages, PruneKind::Volumes],
                older_than_days: 0,
            },
            Timestamp(2),
        )
        .unwrap();
        assert_eq!(res.freed, vec![(PruneKind::DanglingImages, 100_000_000)]);
        assert_eq!(res.failures[0].0, PruneKind::Volumes);
        assert!(res.failures[0].1.contains("boom"));
        let _ = std::fs::remove_dir_all(d);
    }

    // A folder's own time cannot be set through a file handle on Windows; the logic is the same everywhere.
    #[cfg(unix)]
    #[test]
    fn the_cache_folder_is_reported_planned_and_trimmed_safely() {
        let home = temp("cachehome");
        let dir = home.join(".cache/actcache");
        std::fs::create_dir_all(dir.join("old")).unwrap();
        std::fs::write(dir.join("old/blob"), vec![0u8; 4096]).unwrap();
        std::fs::write(dir.join("new.bin"), vec![0u8; 1024]).unwrap();
        let s = RunnerSettings {
            cache_size_limit: Some("1KiB".into()),
            ..RunnerSettings::default()
        };
        let info = cache_report(&s, &home.join("runner"), Some(&home), Some(9)).unwrap();
        assert!(
            info.default_dir && info.exists && info.entries == 2 && info.bytes == 5120,
            "{info:?}"
        );
        assert_eq!(info.size_limit_mb, None, "1 KiB rounds down to 0 MB: no limit");
        assert!(info.problem.is_none());
        // Size limit: 0 MB means none, so use an age limit through the plan; set the mtime of "old" far back.
        // An entry is as old as the newest thing inside it.
        let back = SystemTime::now() - Duration::from_secs(40 * 86_400);
        std::fs::File::options()
            .write(true)
            .open(dir.join("old/blob"))
            .unwrap()
            .set_modified(back)
            .unwrap();
        std::fs::File::open(dir.join("old")).unwrap().set_modified(back).unwrap();
        let limits = CacheLimits {
            max_mb: 0,
            max_age_days: 30,
        };
        let plan = cache_plan(&dir, &limits, Some(&home), now_ts()).unwrap();
        assert_eq!(plan.entries.len(), 1);
        assert_eq!((plan.entries[0].0.name.as_str(), plan.entries[0].1), ("old", CacheReason::Old));
        assert_eq!(plan.bytes, 4096);
        assert!(cache_clean(&dir, &plan).is_empty());
        assert!(!dir.join("old").exists() && dir.join("new.bin").exists());
        // A name that tries to leave the folder is refused, a too-broad folder is not planned.
        let evil = CachePlan {
            entries: vec![(
                CacheEntry {
                    name: "../x".into(),
                    bytes: 1,
                    modified: Timestamp(0),
                },
                CacheReason::Old,
            )],
            bytes: 1,
        };
        assert_eq!(cache_clean(&dir, &evil).len(), 1);
        assert!(cache_plan(&home, &limits, Some(&home), now_ts()).is_err());
        assert!(cache_plan(
            &dir,
            &CacheLimits {
                max_mb: 0,
                max_age_days: 0
            },
            Some(&home),
            now_ts()
        )
        .is_err());
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn the_cache_folder_comes_from_the_runner_configuration() {
        let s = RunnerSettings {
            cache_dir: Some("caches".into()),
            ..RunnerSettings::default()
        };
        let (p, def) = cache_dir_of(&s, Path::new("/r"), Some(Path::new("/h"))).unwrap();
        assert_eq!((p, def), (Path::new("/r").join("caches"), false));
        let (p, def) = cache_dir_of(&RunnerSettings::default(), Path::new("/r"), Some(Path::new("/h"))).unwrap();
        assert_eq!((p, def), (Path::new("/h").join(".cache").join("actcache"), true));
        assert!(cache_dir_of(&RunnerSettings::default(), Path::new("/r"), None).is_none());
        assert_eq!(size_limit_mb("10GB"), Some(9536));
    }
}
