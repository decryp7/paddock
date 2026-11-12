//! The settings of `./dev disk`: defaults, overridden by `PADDOCK_DISK_*` environment variables, overridden by flags.

use clap::Args;
use std::path::PathBuf;
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
/// The d7 and d8 roots of this machine (ER-24 layout: worktrees and builds on d7/d8). Folders directly under a root, and
/// those under its `wt` folder, are what the tooling tracks.
pub const DEFAULT_ROOTS: [&str; 2] = ["/Volumes/d7sst7/dev-targets", "/Volumes/d8sst7/dev-targets"];
/// Where the usage index lives by default.
pub const DEFAULT_INDEX: &str = "/Volumes/d8sst7/dev-targets/.paddock-disk.sqlite";
/// Folders under a root that hold other tracked folders (worktrees).
pub const DEFAULT_CONTAINERS: [&str; 1] = ["wt"];

/// The flags every `disk` command takes.
#[derive(Args, Debug, Clone, Default)]
pub struct CommonArgs {
    /// A root to look in (repeatable; default PADDOCK_DISK_ROOTS, else the d7 and d8 dev-targets folders).
    #[arg(long = "root")]
    pub roots: Vec<PathBuf>,
    /// The SQLite usage index (default PADDOCK_DISK_INDEX, else .paddock-disk.sqlite in the d8 dev-targets folder).
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
    /// Roots to look in.
    pub roots: Vec<PathBuf>,
    /// Folder names under a root that hold tracked folders.
    pub containers: Vec<String>,
    /// The index file.
    pub index: PathBuf,
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
    /// Flags over environment over defaults.
    pub fn resolve(args: &CommonArgs, env: &dyn Fn(&str) -> Option<String>) -> Settings {
        let num = |name: &str| env(name).and_then(|v| v.trim().parse::<u64>().ok());
        let roots = if !args.roots.is_empty() {
            args.roots.clone()
        } else if let Some(v) = env("PADDOCK_DISK_ROOTS").filter(|v| !v.trim().is_empty()) {
            std::env::split_paths(&v).collect()
        } else {
            DEFAULT_ROOTS.iter().map(PathBuf::from).collect()
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
            index: args
                .index
                .clone()
                .or_else(|| env("PADDOCK_DISK_INDEX").filter(|v| !v.is_empty()).map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from(DEFAULT_INDEX)),
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

    #[test]
    fn defaults_are_warn_200_refuse_50() {
        let s = Settings::resolve(&CommonArgs::default(), &env_of(&[]));
        assert_eq!((s.warn_bytes, s.refuse_bytes), (200 * GIB, 50 * GIB));
        assert_eq!(s.live_secs, 48 * 3600);
        assert_eq!(s.roots.len(), 2);
    }

    #[test]
    fn a_flag_beats_the_environment_which_beats_the_default() {
        let a = CommonArgs {
            warn_gib: Some(7),
            ..CommonArgs::default()
        };
        let s = Settings::resolve(&a, &env_of(&[("PADDOCK_DISK_WARN_GIB", "9"), ("PADDOCK_DISK_REFUSE_GIB", "3")]));
        assert_eq!((s.warn_bytes, s.refuse_bytes), (7 * GIB, 3 * GIB));
    }

    #[test]
    fn a_bad_number_in_the_environment_falls_back_to_the_default() {
        let s = Settings::resolve(&CommonArgs::default(), &env_of(&[("PADDOCK_DISK_WARN_GIB", "lots")]));
        assert_eq!(s.warn_bytes, 200 * GIB);
    }
}
