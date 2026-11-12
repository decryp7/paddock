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
    if !s.index.exists() {
        return vec![format!(
            "no size index at {} yet: run ./dev disk scan to see the largest folders",
            s.index.display()
        )];
    }
    let rows = Index::open(&s.index).and_then(|i| i.latest_sizes());
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
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::disk::index::{Index, SizeRow};

    const GIB: u64 = 1 << 30;

    fn settings() -> Settings {
        Settings::resolve(&CommonArgs::default(), &|_| None)
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
            "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk9s1 3906000000 3200000000 538000000 86% /Volumes/d8sst7\n";
        assert_eq!(parse_df(text).unwrap(), 538_000_000 * 1024);
        let spaced = "Filesystem 1024-blocks Used Available Capacity Mounted on\nmy disk 100 50 25 50% /Volumes/gog games\n";
        assert_eq!(parse_df(spaced).unwrap(), 25 * 1024);
        assert!(parse_df("Filesystem\n").is_err());
        assert!(parse_df("h\nno capacity here\n").is_err());
    }

    /// A fake `df` (ER-14: the real disk is never read): a script printing `avail_kib` KiB available.
    #[cfg(unix)]
    fn fake_df(dir: &Path, avail_gib: u64) -> String {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join("df");
        let kib = avail_gib * 1024 * 1024;
        std::fs::write(
            &p,
            format!("#!/bin/sh\necho 'Filesystem 1024-blocks Used Available Capacity Mounted on'\necho '/dev/x 9 9 {kib} 50% /x'\n"),
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p.to_string_lossy().to_string()
    }

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[test]
    fn plenty_of_space_prints_one_line_and_exits_0() {
        let d = tempfile::tempdir().unwrap();
        let (code, out, err) = run_with(&fake_df(d.path(), 500), &d.path().join("i.sqlite"));
        assert_eq!(code, 0);
        assert!(out.contains("500.0 GiB free"), "{out}");
        assert!(err.is_empty(), "{err}");
    }

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[test]
    fn below_the_refusal_level_one_error_with_the_next_step_and_exit_1() {
        let d = tempfile::tempdir().unwrap();
        let (code, _out, err) = run_with(&fake_df(d.path(), 10), &d.path().join("none.sqlite"));
        assert_eq!(code, 1);
        assert_eq!(err.matches("::error::").count(), 1, "{err}");
        assert!(err.contains("./dev disk report") && err.contains("10.0 GiB"), "{err}");
        assert!(err.contains("run ./dev disk scan"), "no index yet is said, not hidden: {err}");
    }

    #[cfg(unix)]
    #[test]
    fn a_df_that_cannot_be_read_is_not_measured_and_does_not_refuse() {
        let d = tempfile::tempdir().unwrap();
        let (code, _out, err) = run_with(d.path().join("missing-df").to_str().unwrap(), &d.path().join("i.sqlite"));
        assert_eq!(code, 0);
        assert!(err.contains("not measured"), "{err}");
    }
}
