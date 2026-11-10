//! `setup`: makes every folder the build uses, new and empty, and records its identity (ADR 0085 §4 item 1).
//!
//! Each folder is created with a name that did not exist (`create_dir` fails on an existing path, so an existing folder
//! can never be adopted), owner-only on Unix, directly under the job's folder (`$RUNNER_TEMP`). Nothing is read from a
//! kept location. The caller appends the printed `KEY=value` lines to `$GITHUB_ENV`.

use super::ledger::{self, Kind, Mode};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};

/// What `setup` made: the exported variables and the folders by kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Made {
    /// The ledger file.
    pub ledger: PathBuf,
    /// The folder of each kind, in the order they were made.
    pub folders: Vec<(Kind, PathBuf)>,
    /// `KEY`, `value` pairs for the job's environment.
    pub env: Vec<(String, String)>,
}

/// The folders `setup` makes. `TauriTools` is made inside `Target`.
pub const MADE: [Kind; 6] = [
    Kind::Workspace,
    Kind::Target,
    Kind::CargoHome,
    Kind::Npm,
    Kind::NodeModules,
    Kind::TauriTools,
];

/// Makes a folder `<parent>/<label>-<random>` that did not exist, owner-only on Unix.
pub fn unique_dir(parent: &Path, label: &str) -> io::Result<PathBuf> {
    let mut last = None;
    for attempt in 0..64u32 {
        let mut h = RandomState::new().build_hasher();
        h.write_u32(attempt);
        h.write_u32(std::process::id());
        if let Ok(d) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            h.write_u128(d.as_nanos());
        }
        let candidate = parent.join(format!("{label}-{:012x}", h.finish() & 0xffff_ffff_ffff));
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut b = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
        match b.create(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("no free name")))
}

/// Nanoseconds since the epoch now.
pub fn now_ns() -> i128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i128::try_from(d.as_nanos()).unwrap_or(i128::MAX))
        .unwrap_or(0)
}

/// Makes the job's folders under `temp` and starts the ledger at `ledger_path`.
pub fn setup(temp: &Path, ledger_path: &Path) -> Result<Made, String> {
    let job_root = temp.canonicalize().map_err(|e| {
        format!(
            "the job folder {} is not usable: {e}. Pass --temp or set RUNNER_TEMP.",
            temp.display()
        )
    })?;
    if let Some(dir) = ledger_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("the ledger folder {} cannot be made: {e}", dir.display()))?;
    }
    ledger::create(ledger_path, &job_root, now_ns())?;
    let mut folders: Vec<(Kind, PathBuf)> = Vec::new();
    for kind in MADE {
        let made = match kind {
            Kind::TauriTools => {
                let target = folders
                    .iter()
                    .find(|(k, _)| *k == Kind::Target)
                    .map(|(_, p)| p.clone())
                    .unwrap_or_default();
                let p = target.join(".tauri");
                std::fs::create_dir(&p).map(|()| p)
            }
            _ => unique_dir(&job_root, &format!("ri-{kind}")),
        }
        .map_err(|e| format!("the {kind} folder cannot be made under {}: {e}", job_root.display()))?;
        ledger::append(ledger_path, kind, Mode::Job, Some(&made))?;
        folders.push((kind, made));
    }
    let path_of = |k: Kind| {
        folders
            .iter()
            .find(|(kk, _)| *kk == k)
            .map(|(_, p)| p.display().to_string())
            .unwrap_or_default()
    };
    let env = vec![
        ("PADDOCK_RELEASE_MODE".to_string(), "1".to_string()),
        ("PADDOCK_RELEASE_LEDGER".to_string(), ledger_path.display().to_string()),
        ("PADDOCK_RELEASE_CLONE".to_string(), path_of(Kind::Workspace)),
        ("PADDOCK_RELEASE_TARGET".to_string(), path_of(Kind::Target)),
        ("CARGO_TARGET_DIR".to_string(), path_of(Kind::Target)),
        ("CARGO_HOME".to_string(), path_of(Kind::CargoHome)),
        ("npm_config_cache".to_string(), path_of(Kind::Npm)),
        ("PADDOCK_RELEASE_NODE_MODULES".to_string(), path_of(Kind::NodeModules)),
        ("PADDOCK_RELEASE_TAURI_TOOLS".to_string(), path_of(Kind::TauriTools)),
        ("RUSTC_WRAPPER".to_string(), String::new()),
        ("CARGO_BUILD_RUSTC_WRAPPER".to_string(), String::new()),
        ("RUSTC_WORKSPACE_WRAPPER".to_string(), String::new()),
    ];
    Ok(Made {
        ledger: ledger_path.to_path_buf(),
        folders,
        env,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_makes_six_fresh_folders_and_records_each() {
        let temp = tempfile::tempdir().unwrap();
        let ledger = temp.path().join("release-isolation/ledger.tsv");
        let made = setup(temp.path(), &ledger).unwrap();
        assert_eq!(made.folders.len(), 6);
        let read = ledger::Ledger::read(&ledger).unwrap();
        assert_eq!(read.entries.len(), 6);
        for (kind, path) in &made.folders {
            assert!(path.is_dir(), "{kind}");
            assert!(read.get(*kind).unwrap().id.is_some());
        }
        let tauri = &made.folders[5].1;
        assert_eq!(tauri.file_name().unwrap(), ".tauri");
        assert!(tauri.starts_with(&made.folders[1].1), "inside the build folder");
        assert!(made.env.iter().any(|(k, v)| k == "RUSTC_WRAPPER" && v.is_empty()));
        assert!(setup(temp.path(), &ledger).is_err(), "a second setup is refused");
    }

    #[test]
    fn two_setups_in_two_jobs_never_share_a_folder() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let ma = setup(a.path(), &a.path().join("l.tsv")).unwrap();
        let mb = setup(b.path(), &b.path().join("l.tsv")).unwrap();
        assert!(ma.folders.iter().all(|(_, p)| !mb.folders.iter().any(|(_, q)| p == q)));
    }

    #[test]
    fn an_existing_name_is_never_adopted() {
        let temp = tempfile::tempdir().unwrap();
        let a = unique_dir(temp.path(), "x").unwrap();
        let b = unique_dir(temp.path(), "x").unwrap();
        assert_ne!(a, b);
    }
}
