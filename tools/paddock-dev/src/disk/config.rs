//! The settings of `./dev disk`: defaults, overridden by `PADDOCK_DISK_*` environment variables, overridden by flags.

use clap::Args;
use paddock_cli::CliError;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Warn below this much free space (GiB).
pub const DEFAULT_WARN_GIB: u64 = 200;
/// Refuse below this much free space (GiB).
pub const DEFAULT_REFUSE_GIB: u64 = 50;
/// A folder used within this many hours is live.
pub const DEFAULT_LIVE_HOURS: u64 = 48;
/// The longest one folder's size is measured.
pub const DEFAULT_FOLDER_TIMEOUT_SECS: u64 = 60;
/// Sizes measured less than this many hours ago are not measured again by `scan`.
pub const DEFAULT_SIZE_MAX_AGE_HOURS: u64 = 6;
/// The usage index's file name in the dev-targets folder. The default root is the dev-targets folder itself, derived
/// ([`crate::places::dev_targets`]: the parent of `paddock.worktreeRoot`), never a disk named here (C125). Folders directly
/// under a root, and those under its `wt` folder, are what the tooling tracks.
pub const INDEX_FILE: &str = ".paddock-disk.sqlite";
/// Folders under a root that hold other tracked folders (worktrees).
pub const DEFAULT_CONTAINERS: [&str; 1] = ["wt"];

/// The flags every `disk` command takes.
#[derive(Args, Debug, Clone, Default)]
pub struct CommonArgs {
    /// A root to look in (repeatable; default PADDOCK_DISK_ROOTS, else the dev-targets folder: PADDOCK_DEV_TARGETS, else the
    /// parent of `git config paddock.worktreeRoot`).
    #[arg(long = "root")]
    pub roots: Vec<PathBuf>,
    /// The SQLite usage index (default PADDOCK_DISK_INDEX, else .paddock-disk.sqlite in the dev-targets folder).
    #[arg(long)]
    pub index: Option<PathBuf>,
    /// Warn below this many GiB free (default PADDOCK_DISK_WARN_GIB, else 200).
    #[arg(long)]
    pub warn_gib: Option<u64>,
    /// Refuse below this many GiB free (default PADDOCK_DISK_REFUSE_GIB, else 50).
    #[arg(long)]
    pub refuse_gib: Option<u64>,
    /// A folder used in the last N hours is live (default PADDOCK_DISK_LIVE_HOURS, else 48).
    #[arg(long)]
    pub live_hours: Option<u64>,
    /// Threads for sizes and git questions (default PADDOCK_DISK_JOBS, else Auto: up to 4, an SSD's worth).
    #[arg(long)]
    pub jobs: Option<usize>,
}

/// Resolved settings.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Warn below this many bytes free.
    pub warn_bytes: u64,
    /// Refuse below this many bytes free.
    pub refuse_bytes: u64,
    /// Roots to look in; empty when none is given and the dev-targets folder cannot be derived (see `unlocated`).
    pub roots: Vec<PathBuf>,
    /// Folder names under a root that hold tracked folders.
    pub containers: Vec<String>,
    /// The index file; `None` when none is given and the dev-targets folder cannot be derived (see `unlocated`).
    pub index: Option<PathBuf>,
    /// Why the dev-targets folder could not be derived, when a default needed it.
    pub unlocated: Option<String>,
    /// Live window in seconds.
    pub live_secs: u64,
    /// Worker threads.
    pub jobs: usize,
    /// Per-folder size time limit.
    pub folder_timeout: Duration,
    /// Cached sizes younger than this are reused by `scan`, in seconds.
    pub size_max_age_secs: u64,
}

const GIB: u64 = 1 << 30;

impl Settings {
    /// Flags over environment over defaults; the defaults of the roots and the index come from the dev-targets folder of the
    /// repository the command runs in.
    pub fn resolve(args: &CommonArgs, env: &dyn Fn(&str) -> Option<String>) -> Settings {
        let derive = || {
            let cwd = std::env::current_dir().map_err(|e| format!("the current folder cannot be read: {e}"))?;
            crate::places::dev_targets(&cwd, env).map_err(|e| format!("{e:?}"))
        };
        Settings::resolve_with(args, env, &derive)
    }

    /// [`Settings::resolve`] with the dev-targets folder's derivation passed in (asked only when a default needs it).
    pub fn resolve_with(
        args: &CommonArgs,
        env: &dyn Fn(&str) -> Option<String>,
        dev_targets: &dyn Fn() -> Result<PathBuf, String>,
    ) -> Settings {
        let num = |name: &str| env(name).and_then(|v| v.trim().parse::<u64>().ok());
        let given_roots: Option<Vec<PathBuf>> = if !args.roots.is_empty() {
            Some(args.roots.clone())
        } else {
            env("PADDOCK_DISK_ROOTS")
                .filter(|v| !v.trim().is_empty())
                .map(|v| std::env::split_paths(&v).collect())
        };
        let given_index = args
            .index
            .clone()
            .or_else(|| env("PADDOCK_DISK_INDEX").filter(|v| !v.is_empty()).map(PathBuf::from));
        let derived = if given_roots.is_some() && given_index.is_some() {
            None
        } else {
            Some(dev_targets())
        };
        let (roots, index, unlocated) = match derived {
            None => (given_roots.unwrap_or_default(), given_index, None),
            Some(Ok(dt)) => (
                given_roots.unwrap_or_else(|| vec![dt.clone()]),
                given_index.or_else(|| Some(dt.join(INDEX_FILE))),
                None,
            ),
            Some(Err(why)) => (given_roots.unwrap_or_default(), given_index, Some(why)),
        };
        let jobs = args
            .jobs
            .or_else(|| num("PADDOCK_DISK_JOBS").and_then(|n| usize::try_from(n).ok()))
            .filter(|n| *n > 0)
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(1).clamp(1, 4)));
        Settings {
            warn_bytes: args.warn_gib.or_else(|| num("PADDOCK_DISK_WARN_GIB")).unwrap_or(DEFAULT_WARN_GIB) * GIB,
            refuse_bytes: args
                .refuse_gib
                .or_else(|| num("PADDOCK_DISK_REFUSE_GIB"))
                .unwrap_or(DEFAULT_REFUSE_GIB)
                * GIB,
            roots,
            containers: DEFAULT_CONTAINERS.iter().map(|s| (*s).to_string()).collect(),
            index,
            unlocated,
            live_secs: args
                .live_hours
                .or_else(|| num("PADDOCK_DISK_LIVE_HOURS"))
                .unwrap_or(DEFAULT_LIVE_HOURS)
                * 3600,
            jobs,
            folder_timeout: Duration::from_secs(num("PADDOCK_DISK_FOLDER_TIMEOUT_SECS").unwrap_or(DEFAULT_FOLDER_TIMEOUT_SECS)),
            size_max_age_secs: DEFAULT_SIZE_MAX_AGE_HOURS * 3600,
        }
    }

    /// The roots, or the refusal (fail closed) when none is given and the dev-targets folder cannot be derived.
    pub fn located_roots(&self) -> Result<&[PathBuf], CliError> {
        if self.roots.is_empty() {
            return Err(self.unlocated_error("no folder to look in"));
        }
        Ok(&self.roots)
    }

    /// The index file, or the refusal (fail closed) when none is given and the dev-targets folder cannot be derived.
    pub fn index_file(&self) -> Result<&Path, CliError> {
        self.index.as_deref().ok_or_else(|| self.unlocated_error("no usage index"))
    }

    fn unlocated_error(&self, what: &str) -> CliError {
        CliError::usage(format!(
            "disk: {what}: {}",
            self.unlocated.as_deref().unwrap_or("none was given")
        ))
        .with_hint("Pass --root and --index, set PADDOCK_DISK_ROOTS and PADDOCK_DISK_INDEX, or derive them: `git config paddock.worktreeRoot <dev-targets>/wt` once, or PADDOCK_DEV_TARGETS (docs/dev-setup.md).")
    }

    /// The root a path is under, if any.
    pub fn root_of(&self, path: &std::path::Path) -> Option<&PathBuf> {
        self.roots.iter().find(|r| path.starts_with(r))
    }
}

/// `12.3 GiB` for a byte count.
pub fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / GIB as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| (*v).to_string())
    }

    fn derived() -> Result<PathBuf, String> {
        Ok(PathBuf::from("/srv/disk-b/dev-targets"))
    }

    #[test]
    fn defaults_are_warn_200_refuse_50() {
        let s = Settings::resolve_with(&CommonArgs::default(), &env_of(&[]), &derived);
        assert_eq!((s.warn_bytes, s.refuse_bytes), (200 * GIB, 50 * GIB));
        assert_eq!(s.live_secs, 48 * 3600);
    }

    #[test]
    fn the_default_root_and_index_are_the_derived_dev_targets_folder_not_a_named_disk() {
        let s = Settings::resolve_with(&CommonArgs::default(), &env_of(&[]), &derived);
        assert_eq!(s.roots, vec![PathBuf::from("/srv/disk-b/dev-targets")]);
        assert_eq!(s.index, Some(PathBuf::from("/srv/disk-b/dev-targets/.paddock-disk.sqlite")));
        assert!(s.located_roots().is_ok() && s.index_file().is_ok());
    }

    #[test]
    fn without_a_derivable_folder_the_defaults_fail_closed_and_given_ones_still_work() {
        let gone = || -> Result<PathBuf, String> { Err("no worktree root is set".into()) };
        let s = Settings::resolve_with(&CommonArgs::default(), &env_of(&[]), &gone);
        assert!(s.roots.is_empty() && s.index.is_none());
        let e = format!("{:?}", s.located_roots().unwrap_err());
        assert!(e.contains("no worktree root is set") && e.contains("paddock.worktreeRoot"), "{e}");
        assert!(s.index_file().is_err());
        // The thresholds do not need the folder.
        assert_eq!(s.warn_bytes, 200 * GIB);
        // Roots and index given: the derivation is never asked.
        let never = || -> Result<PathBuf, String> { panic!("asked") };
        let s = Settings::resolve_with(
            &CommonArgs::default(),
            &env_of(&[("PADDOCK_DISK_ROOTS", "/srv/r"), ("PADDOCK_DISK_INDEX", "/srv/i.sqlite")]),
            &never,
        );
        assert_eq!(
            (s.roots, s.index),
            (vec![PathBuf::from("/srv/r")], Some(PathBuf::from("/srv/i.sqlite")))
        );
    }

    #[test]
    fn a_flag_beats_the_environment_which_beats_the_default() {
        let a = CommonArgs {
            warn_gib: Some(7),
            ..CommonArgs::default()
        };
        let s = Settings::resolve_with(
            &a,
            &env_of(&[("PADDOCK_DISK_WARN_GIB", "9"), ("PADDOCK_DISK_REFUSE_GIB", "3")]),
            &derived,
        );
        assert_eq!((s.warn_bytes, s.refuse_bytes), (7 * GIB, 3 * GIB));
    }

    #[test]
    fn a_bad_number_in_the_environment_falls_back_to_the_default() {
        let s = Settings::resolve_with(&CommonArgs::default(), &env_of(&[("PADDOCK_DISK_WARN_GIB", "lots")]), &derived);
        assert_eq!(s.warn_bytes, 200 * GIB);
    }
}
