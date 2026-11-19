//! `train landed <train>`: the disk clean the integrator runs after a train lands on main.
//!
//! It is `disk clean --apply` (the same safe logic: only recorded folders and worktrees that are merged or stale-pushed and not
//! live, never unsaved, locked, owned, CI slots, runners, sccache, VMs or Docker; a released review worktree whose commits are
//! all on origin is a stale-pushed one) with a time limit, and it never blocks the landing: an error or a timeout is a warning
//! and the exit code stays 0. What it removed and the space freed (the free bytes of the first root's disk, before and after) are
//! logged, so a user's log shows what went (ER-27).

use super::check::free_bytes;
use super::clean::{CleanArgs, run_core};
use super::config::{CommonArgs, Settings, gib};
use clap::Args;
use paddock_cli::{CliError, Report};
use serde_json::json;
use std::path::Path;

/// The default time limit of the clean at a landing.
pub const DEFAULT_MAX: &str = "10m";

/// `train landed`'s arguments.
#[derive(Args, Debug, Clone)]
pub struct LandedArgs {
    /// The train that landed, for example `train/2026-11-19-16` (named in the log).
    pub train: String,
    #[command(flatten)]
    /// The shared settings flags.
    pub common: CommonArgs,
    /// The longest the clean runs (default 10m); what it did not reach stays for the next landing.
    #[arg(long, value_name = "DURATION", default_value = DEFAULT_MAX)]
    pub max: String,
}

/// Runs the clean for a landed train. Only a bad argument is an error; everything else is a warning in the report.
pub fn landed(
    a: &LandedArgs,
    env: &dyn Fn(&str) -> Option<String>,
    note: &mut dyn FnMut(&str),
    procs: &(dyn Fn() -> Option<Vec<std::path::PathBuf>> + Sync),
) -> Result<Report, CliError> {
    if a.train.trim().is_empty() {
        return Err(CliError::usage("name the train that landed.").with_hint("`./dev train landed train/2026-11-19-16`."));
    }
    crate::wait::parse_duration(&a.max).map_err(|e| CliError::usage(e).with_hint("Write --max as 90s, 10m or 1h."))?;
    let s = Settings::resolve(&a.common, env);
    let df = env("PADDOCK_DF").filter(|v| !v.is_empty()).unwrap_or_else(|| "df".to_string());
    let disk: Option<&Path> = s.roots.iter().map(|r| r.as_path()).find(|r| r.is_dir());
    let free = |when: &str, note: &mut dyn FnMut(&str)| -> Option<u64> {
        let d = disk?;
        match free_bytes(d, &df) {
            Ok(b) => Some(b),
            Err(why) => {
                note(&format!("::warning::disk: free space {when} the clean was not measured ({why})"));
                None
            }
        }
    };
    note(&format!(
        "train landed: {} is on main; cleaning merged and stale-pushed folders (at most {})",
        a.train, a.max
    ));
    let before = free("before", note);
    let clean = CleanArgs {
        common: a.common.clone(),
        apply: true,
        dry_run: false,
        max: Some(a.max.clone()),
        measure: false,
    };
    let result = run_core(&clean, env, note, &mut |_| {}, procs);
    let after = free("after", note);
    let freed = match (before, after) {
        (Some(b), Some(f)) => Some(f.saturating_sub(b)),
        _ => None,
    };
    match result {
        Ok(r) => {
            let removed = r.data["removed"].as_array().map_or(0, Vec::len);
            let line = format!(
                "train landed: {} removed {removed} folder(s); freed {}; {} free now{}",
                a.train,
                freed.map_or("not measured".to_string(), gib),
                after.map_or("not measured".to_string(), gib),
                if r.data["stoppedAtLimit"] == true {
                    "; stopped at the time limit"
                } else {
                    ""
                }
            );
            note(&line);
            Ok(Report::new(
                json!({"train": a.train, "ran": true, "removed": removed, "freedBytes": freed, "freeBytes": after,
                    "stoppedAtLimit": r.data["stoppedAtLimit"], "clean": r.data}),
                format!("{}{line}\n", r.text),
            ))
        }
        Err(e) => {
            // The landing is done: a clean that cannot run is a warning with its next step, never a failure.
            let line = format!(
                "::warning::disk: the clean after {} did not run ({}). The landing is not affected. Next: run ./dev disk clean --dry-run, then --apply.",
                a.train, e.message
            );
            note(&line);
            Ok(Report::new(
                json!({"train": a.train, "ran": false, "error": e.message}),
                format!("{line}\n"),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::disk::record::{self, Kind, Record};

    #[cfg(unix)]
    fn old_build(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("data"), vec![0u8; 10_000]).unwrap();
        let mut r = Record::new(Kind::Build, "implementer:t");
        r.created = 1_000_000;
        r.last_used = 1_000_000;
        record::write(dir, &r).unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        std::fs::File::open(dir).unwrap().set_modified(old).unwrap();
    }

    fn args(root: &Path, idx: &Path, max: &str) -> LandedArgs {
        LandedArgs {
            train: "train/2026-11-19-16".into(),
            common: CommonArgs {
                roots: vec![root.to_path_buf()],
                index: Some(idx.to_path_buf()),
                jobs: Some(2),
                ..CommonArgs::default()
            },
            max: max.into(),
        }
    }

    /// A fake `df` (ER-14): its free space is its own counter file, so the folders going shows up as more free space.
    #[cfg(unix)]
    fn fake_df(dir: &Path) -> String {
        let p = dir.join("df");
        let n = dir.join("calls");
        paddock_testkit::exe::write_program(
            &p,
            format!(
                "#!/bin/sh\necho x >> '{n}'\nc=$(wc -l < '{n}' | tr -d ' ')\nk=$((c * 1048576 * 10))\necho 'Filesystem 1024-blocks Used Available Capacity Mounted on'\necho \"/dev/x 9 9 $k 50% /x\"\n",
                n = n.display()
            ),
        )
        .unwrap();
        p.to_string_lossy().to_string()
    }

    #[cfg(unix)]
    #[test]
    fn a_landing_removes_the_stale_build_folder_and_logs_what_went_and_the_space_freed() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("dev-targets");
        old_build(&root.join("stale-build"));
        let fresh = root.join("fresh-build");
        std::fs::create_dir_all(&fresh).unwrap();
        record::write(&fresh, &Record::new(Kind::Build, "implementer:t")).unwrap();
        let df = fake_df(d.path());
        let mut logged = Vec::new();
        let r = landed(
            &args(&root, &d.path().join("i.sqlite"), "5m"),
            &move |k| (k == "PADDOCK_DF").then(|| df.clone()),
            &mut |l| logged.push(l.to_string()),
            &|| Some(Vec::new()),
        )
        .unwrap();
        assert!(!root.join("stale-build").exists());
        assert!(fresh.is_dir(), "a folder used just now is live");
        assert_eq!(r.data["removed"], 1, "{}", r.text);
        assert_eq!(r.data["freedBytes"], 10u64 << 30, "{}", r.text);
        assert!(
            logged
                .iter()
                .any(|l| l.contains("train/2026-11-19-16") && l.contains("freed 10.0 GiB")),
            "{logged:?}"
        );
    }

    /// Never blocks: a clean that cannot run (the processes cannot be read) is a warning and a report, not an error.
    #[cfg(unix)]
    #[test]
    fn a_clean_that_cannot_run_is_a_warning_and_never_an_error() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("dev-targets");
        old_build(&root.join("stale-build"));
        let mut logged = Vec::new();
        let r = landed(
            &args(&root, &d.path().join("i.sqlite"), "5m"),
            &|_| None,
            &mut |l| logged.push(l.to_string()),
            &|| None,
        )
        .unwrap();
        assert_eq!(r.data["ran"], false);
        assert!(root.join("stale-build").is_dir(), "nothing is removed on a guess");
        assert!(
            logged.iter().any(|l| l.starts_with("::warning::") && l.contains("Next:")),
            "{logged:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_landing_past_its_time_limit_warns_and_leaves_the_rest() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("dev-targets");
        old_build(&root.join("stale-build"));
        let r = landed(&args(&root, &d.path().join("i.sqlite"), "0s"), &|_| None, &mut |_| {}, &|| {
            Some(Vec::new())
        })
        .unwrap();
        assert_eq!(r.data["stoppedAtLimit"], true, "{}", r.text);
        assert!(root.join("stale-build").is_dir());
    }

    #[test]
    fn an_empty_train_name_or_a_bad_limit_is_a_usage_error() {
        let d = tempfile::tempdir().unwrap();
        let mut a = args(d.path(), &d.path().join("i.sqlite"), "5m");
        a.train = " ".into();
        assert!(landed(&a, &|_| None, &mut |_| {}, &|| None).is_err());
        let b = args(d.path(), &d.path().join("i.sqlite"), "soon");
        assert!(landed(&b, &|_| None, &mut |_| {}, &|| None).is_err());
    }
}
