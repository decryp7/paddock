//! `disk check`: the job-start and pre-build check of free space (as GitHub's runner images check at job start).
//!
//! Below the warning level it prints one `::warning::` and the ten largest folders the index knows; below the refusal level
//! it prints one `::error::` with the next step and exits 1. A disk whose free space cannot be read is reported as not
//! measured, never as fine (ER-33), and does not refuse.

use super::config::{CommonArgs, Settings, gib};
use super::index::Index;
use clap::Args;
use paddock_proc::{Limits, command, run_bounded};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// `disk check`'s arguments.
#[derive(Args, Debug, Clone)]
pub struct CheckArgs {
    /// A path whose disk is checked (repeatable; default the current folder).
    #[arg(long = "path")]
    pub paths: Vec<PathBuf>,
    #[command(flatten)]
    /// The shared settings flags.
    pub common: CommonArgs,
}

/// The answer for one disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Enough free space.
    Ok,
    /// Below the warning level.
    Warn,
    /// Below the refusal level.
    Refuse,
}

/// The level for a free-space figure.
pub fn level(free: u64, s: &Settings) -> Level {
    if free < s.refuse_bytes {
        Level::Refuse
    } else if free < s.warn_bytes {
        Level::Warn
    } else {
        Level::Ok
    }
}

/// Reads the available bytes out of `df -Pk` output (POSIX format: the figure before the capacity column).
pub fn parse_df(text: &str) -> Result<u64, String> {
    let line = text.lines().nth(1).ok_or("df printed no data line")?;
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let cap = tokens
        .iter()
        .position(|t| t.len() > 1 && t.ends_with('%') && t[..t.len() - 1].bytes().all(|b| b.is_ascii_digit()))
        .filter(|i| *i >= 1)
        .ok_or("df's line has no capacity column")?;
    let kib: u64 = tokens[cap - 1]
        .parse()
        .map_err(|_| format!("df's available figure `{}` is not a number", tokens[cap - 1]))?;
    Ok(kib * 1024)
}

/// The free bytes on the disk holding `path`, asking `df` (`PADDOCK_DF` overrides the program, as the CI kit's script has it).
pub fn free_bytes(path: &Path, df: &str) -> Result<u64, String> {
    #[cfg(windows)]
    if df == "df" {
        return crate::ci_isolation::slotflow::windows_free_bytes(&path.to_string_lossy());
    }
    let mut cmd = command(df);
    cmd.arg("-Pk").arg(path);
    let out = run_bounded(cmd, Limits::new(Duration::from_secs(30)).max_output(1 << 16)).map_err(|e| format!("df did not run: {e}"))?;
    if !out.success() {
        return Err(format!("df failed: {}", out.stderr_text().trim()));
    }
    parse_df(&out.stdout_text())
}

fn largest(s: &Settings) -> Vec<String> {
    let Some(index) = s.index.as_deref() else {
        return vec![format!(
            "no size index: {} (docs/dev-setup.md)",
            s.unlocated.as_deref().unwrap_or("none was given")
        )];
    };
    if !index.exists() {
        return vec![format!(
            "no size index at {} yet: run ./dev disk scan to see the largest folders",
            index.display()
        )];
    }
    let rows = Index::open_read_only(index).and_then(|i| i.latest_sizes());
    match rows {
        Ok(sizes) if !sizes.is_empty() => {
            let mut v: Vec<_> = sizes.into_values().collect();
            v.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
            v.iter()
                .take(10)
                .map(|r| format!("{:>10}  {}{}", gib(r.bytes), r.path, if r.partial { " (at least)" } else { "" }))
                .collect()
        }
        Ok(_) => vec!["the size index is empty: run ./dev disk scan".to_string()],
        Err(e) => vec![format!("the size index could not be read ({e}): run ./dev disk scan")],
    }
}

/// The reclaimable total the index knows: the merged and stale-pushed folders `clean --apply` may remove (CI slot folders are the
/// CI kit's, so they do not count), with how many have no size yet.
pub fn reclaimable_line(s: &Settings) -> String {
    let Some(index) = s.index.as_deref().filter(|i| i.exists()) else {
        return "reclaimable: not measured (no size index; run ./dev disk scan)".to_string();
    };
    let Ok(i) = Index::open_read_only(index) else {
        return "reclaimable: not measured (the size index could not be opened; run ./dev disk scan)".to_string();
    };
    let (Ok(folders), Ok(sizes)) = (i.folders(), i.latest_sizes()) else {
        return "reclaimable: not measured (the size index could not be read; run ./dev disk scan)".to_string();
    };
    let (mut bytes, mut count, mut unmeasured) = (0u64, 0usize, 0usize);
    for f in folders
        .iter()
        .filter(|f| f.kind != "slot" && super::state::State::parse(&f.state).is_some_and(super::state::State::reclaimable))
    {
        count += 1;
        match sizes.get(&f.path) {
            Some(r) => bytes += r.bytes,
            None => unmeasured += 1,
        }
    }
    format!(
        "reclaimable: {} in {count} merged or stale-pushed folder(s){} (./dev disk clean --apply)",
        gib(bytes),
        if unmeasured > 0 {
            format!(", {unmeasured} not measured")
        } else {
            String::new()
        }
    )
}

/// The check before a CI dispatch or a release sequence, on the runner volumes (the disks under the configured roots): `Err`
/// with the text for the `::error::` when one is below the refusal level, before any CI time is spent. Below the warning level it
/// prints the warning, the largest folders and the reclaimable total. A disk that cannot be read is "not measured" and passes.
pub fn before_dispatch(env: &dyn Fn(&str) -> Option<String>, log: &mut dyn Write) -> Result<(), String> {
    let s = Settings::resolve(&CommonArgs::default(), env);
    // Every configured root: each is a disk a run can fill. Everything goes to `log` (standard error in the commands), so a
    // `--json` command keeps standard output for its one object.
    let paths: Vec<PathBuf> = s.roots.iter().filter(|r| r.is_dir()).cloned().collect();
    if paths.is_empty() {
        let _ = writeln!(
            log,
            "::warning::disk: no runner volume found under the dev-targets roots; the disk check did not run"
        );
        return Ok(());
    }
    let a = CheckArgs {
        paths,
        common: CommonArgs::default(),
    };
    let mut out = Vec::new();
    let code = run(&a, env, &mut out, log);
    let _ = log.write_all(&out);
    match code {
        0 => Ok(()),
        _ => Err(
            "the disk is below the refusal level, so nothing is dispatched; free disk first (see the lines above), then dispatch again"
                .to_string(),
        ),
    }
}

/// Runs the check; returns the exit code (0, or 1 when a disk is below the refusal level).
pub fn run(a: &CheckArgs, env: &dyn Fn(&str) -> Option<String>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let s = Settings::resolve(&a.common, env);
    let df = env("PADDOCK_DF").filter(|v| !v.is_empty()).unwrap_or_else(|| "df".to_string());
    let paths = if a.paths.is_empty() {
        vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))]
    } else {
        a.paths.clone()
    };
    let mut code = 0;
    let mut warned = false;
    for p in &paths {
        match free_bytes(p, &df) {
            Err(why) => {
                let _ = writeln!(
                    err,
                    "::warning::disk: free space on {} not measured ({why}); the check did not run",
                    p.display()
                );
            }
            Ok(free) => match level(free, &s) {
                Level::Ok => {
                    let _ = writeln!(
                        out,
                        "disk: {} free on {} (warns under {}, refuses under {})",
                        gib(free),
                        p.display(),
                        gib(s.warn_bytes),
                        gib(s.refuse_bytes)
                    );
                }
                Level::Warn => {
                    warned = true;
                    let _ = writeln!(
                        err,
                        "::warning::disk: {} free on {}, below the warning level of {} (PADDOCK_DISK_WARN_GIB). Next: ./dev disk report, then ./dev disk clean --dry-run.",
                        gib(free),
                        p.display(),
                        gib(s.warn_bytes)
                    );
                }
                Level::Refuse => {
                    code = 1;
                    let _ = writeln!(
                        err,
                        "::error::disk: only {} free on {}, below the refusal level of {} (PADDOCK_DISK_REFUSE_GIB). Next: free disk first (./dev disk report, then ./dev disk clean --dry-run and --apply), then run again.",
                        gib(free),
                        p.display(),
                        gib(s.refuse_bytes)
                    );
                    warned = true;
                }
            },
        }
    }
    if warned {
        let _ = writeln!(err, "disk: the largest folders the index knows:");
        for l in largest(&s) {
            let _ = writeln!(err, "  {l}");
        }
        let _ = writeln!(err, "disk: {}", reclaimable_line(&s));
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::index::{Index, SizeRow};
    use paddock_testkit::StandIn;

    const GIB: u64 = 1 << 30;

    fn settings() -> Settings {
        Settings::resolve_with(&CommonArgs::default(), &|_| None, &|| {
            Err("tests never derive the dev-targets folder from the real git config".to_string())
        })
    }

    #[test]
    fn the_levels_split_at_200_and_50_gib() {
        let s = settings();
        assert_eq!(level(300 * GIB, &s), Level::Ok);
        assert_eq!(level(200 * GIB, &s), Level::Ok);
        assert_eq!(level(199 * GIB, &s), Level::Warn);
        assert_eq!(level(50 * GIB, &s), Level::Warn);
        assert_eq!(level(49 * GIB, &s), Level::Refuse);
    }

    #[test]
    fn df_output_in_posix_format_is_read_even_when_the_names_have_spaces() {
        let text =
            "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk9s1 3906000000 3200000000 538000000 86% /srv/disk-b\n";
        assert_eq!(parse_df(text).unwrap(), 538_000_000 * 1024);
        let spaced = "Filesystem 1024-blocks Used Available Capacity Mounted on\nmy disk 100 50 25 50% /Volumes/gog games\n";
        assert_eq!(parse_df(spaced).unwrap(), 25 * 1024);
        assert!(parse_df("Filesystem\n").is_err());
        assert!(parse_df("h\nno capacity here\n").is_err());
    }

    /// A fake `df` (ER-14: the real disk is never read): the testkit's stand-in printing `avail_gib` GiB available in `df -Pk`'s
    /// shape, on every OS.
    fn fake_df(dir: &Path, avail_gib: u64) -> String {
        let kib = avail_gib * 1024 * 1024;
        StandIn::new(dir, "df")
            .stdout("Filesystem 1024-blocks Used Available Capacity Mounted on")
            .stdout(&format!("/dev/x 9 9 {kib} 50% /x"))
            .install()
            .unwrap()
            .program()
            .to_string_lossy()
            .to_string()
    }

    fn run_with(df: &str, index: &Path) -> (i32, String, String) {
        let a = CheckArgs {
            paths: vec![PathBuf::from("/work")],
            common: CommonArgs {
                index: Some(index.to_path_buf()),
                ..CommonArgs::default()
            },
        };
        let df = df.to_string();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&a, &move |k| (k == "PADDOCK_DF").then(|| df.clone()), &mut out, &mut err);
        (
            code,
            String::from_utf8_lossy(&out).to_string(),
            String::from_utf8_lossy(&err).to_string(),
        )
    }

    #[test]
    fn plenty_of_space_prints_one_line_and_exits_0() {
        let d = tempfile::tempdir().unwrap();
        let (code, out, err) = run_with(&fake_df(d.path(), 500), &d.path().join("i.sqlite"));
        assert_eq!(code, 0);
        assert!(out.contains("500.0 GiB free"), "{out}");
        assert!(err.is_empty(), "{err}");
    }

    #[test]
    fn below_the_warning_level_one_warning_and_the_largest_folders_and_exit_0() {
        let d = tempfile::tempdir().unwrap();
        let idx = d.path().join("i.sqlite");
        let mut i = Index::open(&idx).unwrap();
        let rows: Vec<SizeRow> = (0..12)
            .map(|n| SizeRow {
                path: format!("/f{n:02}"),
                bytes: (n + 1) * GIB,
                partial: false,
                measured_at: 5,
            })
            .collect();
        i.record_sizes(&rows, 5).unwrap();
        drop(i);
        let (code, _out, err) = run_with(&fake_df(d.path(), 120), &idx);
        assert_eq!(code, 0);
        assert_eq!(err.matches("::warning::").count(), 1, "{err}");
        assert!(!err.contains("::error::"));
        let listed = err
            .lines()
            .filter(|l| l.trim_start().starts_with(|c: char| c.is_ascii_digit()) && l.contains("/f"))
            .count();
        assert_eq!(listed, 10, "{err}");
        assert!(
            err.contains("/f11") && !err.contains("/f00"),
            "the biggest ten, not the smallest: {err}"
        );
    }

    #[test]
    fn below_the_refusal_level_one_error_with_the_next_step_and_exit_1() {
        let d = tempfile::tempdir().unwrap();
        let (code, _out, err) = run_with(&fake_df(d.path(), 10), &d.path().join("none.sqlite"));
        assert_eq!(code, 1);
        assert_eq!(err.matches("::error::").count(), 1, "{err}");
        assert!(err.contains("./dev disk report") && err.contains("10.0 GiB"), "{err}");
        assert!(err.contains("run ./dev disk scan"), "no index yet is said, not hidden: {err}");
    }

    #[test]
    fn a_df_that_cannot_be_read_is_not_measured_and_does_not_refuse() {
        let d = tempfile::tempdir().unwrap();
        let (code, _out, err) = run_with(d.path().join("missing-df").to_str().unwrap(), &d.path().join("i.sqlite"));
        assert_eq!(code, 0);
        assert!(err.contains("not measured"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn the_warning_names_the_reclaimable_total_from_the_index() {
        use crate::disk::index::FolderRow;
        let d = tempfile::tempdir().unwrap();
        let idx = d.path().join("i.sqlite");
        let mut i = Index::open(&idx).unwrap();
        let row = |path: &str, kind: &str, state: &str| FolderRow {
            path: path.into(),
            kind: kind.into(),
            owner: "a:b".into(),
            state: state.into(),
            ..FolderRow::default()
        };
        i.sync_folders(&[
            row("/m", "worktree", "merged"),
            row("/s", "slot", "stale-pushed"),
            row("/l", "build", "live"),
        ])
        .unwrap();
        let size = |p: &str, g: u64| SizeRow {
            path: p.into(),
            bytes: g * GIB,
            partial: false,
            measured_at: 5,
        };
        i.record_sizes(&[size("/m", 7), size("/s", 50), size("/l", 90)], 5).unwrap();
        drop(i);
        let (code, _out, err) = run_with(&fake_df(d.path(), 120), &idx);
        assert_eq!(code, 0);
        assert!(
            err.contains("reclaimable: 7.0 GiB in 1 merged or stale-pushed folder(s)"),
            "slots and live folders do not count: {err}"
        );
    }

    #[cfg(unix)]
    fn before(avail_gib: u64) -> (Result<(), String>, String, String, tempfile::TempDir) {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("dev-targets");
        std::fs::create_dir_all(&root).unwrap();
        let df = fake_df(d.path(), avail_gib);
        let root_s = root.to_string_lossy().to_string();
        let env = move |k: &str| match k {
            "PADDOCK_DF" => Some(df.clone()),
            "PADDOCK_DEV_TARGETS" => Some(root_s.clone()),
            _ => None,
        };
        let mut log = Vec::new();
        let r = before_dispatch(&env, &mut log);
        (r, String::new(), String::from_utf8_lossy(&log).to_string(), d)
    }

    #[cfg(unix)]
    #[test]
    fn a_dispatch_is_refused_below_the_refusal_level_before_any_ci_time_is_spent() {
        let (r, _out, err, _d) = before(10);
        assert!(r.is_err(), "{err}");
        assert_eq!(err.matches("::error::").count(), 1, "{err}");
        assert!(err.contains("./dev disk clean"), "the next step is named: {err}");
    }

    #[cfg(unix)]
    #[test]
    fn a_dispatch_warns_with_the_reclaimable_total_under_the_warning_level_and_goes_on() {
        let (r, _out, err, _d) = before(120);
        assert!(r.is_ok(), "{err}");
        assert_eq!(err.matches("::warning::").count(), 1, "{err}");
        assert!(err.contains("reclaimable:"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn a_dispatch_with_room_is_silent_and_goes_on() {
        let (r, _out, err, _d) = before(500);
        assert!(r.is_ok() && !err.contains("::"), "{err}");
        assert!(
            err.contains("500.0 GiB free"),
            "everything is on the log stream, none on standard output: {err}"
        );
    }

    /// Every configured root is checked, not the first only: two roots on a full disk are two refusals.
    #[cfg(unix)]
    #[test]
    fn every_configured_root_is_checked() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let df = fake_df(d.path(), 10);
        let roots = std::env::join_paths([&a, &b]).unwrap().to_string_lossy().to_string();
        let env = move |k: &str| match k {
            "PADDOCK_DF" => Some(df.clone()),
            "PADDOCK_DISK_ROOTS" => Some(roots.clone()),
            _ => None,
        };
        let mut log = Vec::new();
        assert!(before_dispatch(&env, &mut log).is_err());
        let text = String::from_utf8_lossy(&log);
        assert_eq!(text.matches("::error::").count(), 2, "{text}");
    }
}
