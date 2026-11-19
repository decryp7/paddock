//! The push gates' kept build folders (`<worktree root>/pre-push/targets/<workspace>`) stay bounded.
//!
//! Every push builds in a throw-away worktree into these folders, shared by all agents' pushes. Cargo starts a new incremental
//! folder (`<profile>/incremental/<crate>-<hash>`) for every new unit hash and never removes the old ones, so on 2026-11-19 they
//! held 34.1 of 54.2 GiB, most of it last written days before (docs/research/2026-11-19-sccache.md, section 2). After a gate
//! run each kept folder that no build holds loses its incremental folders not written for [`STALE`], then goes through the CI
//! host runners' cap ([`crate::ci_host::prune::cap_folder`]: incremental caches first, then the folder) at [`CAP_GB`].
//!
//! A folder is pruned only while this process holds every cargo lock in it exclusively (`<profile>/.cargo-build-lock`, which a
//! stable cargo 1.100 build holds exclusively, and `.cargo-lock`, which it holds shared; cargo `src/compiler/layout.rs` at
//! 3acc79c0c): a build running in it, another agent's push, is never pulled from under it. A busy folder is skipped and said so.

use crate::ci_host::prune;
use crate::safe_remove;
use std::collections::HashMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// An incremental crate folder not written for this long is removed. The current unit hashes are rewritten by every push (Dyno's
/// kept folder: 147 of 1,129 written on 2026-11-18, 702 last written on 2026-11-13); a workspace nobody pushed for three days loses
/// only its incremental state, and its next gate compiles its own crates once without it (correct, a little slower).
pub const STALE: Duration = Duration::from_secs(3 * 24 * 3600);

/// The cap of one kept folder, in GiB. The largest part that is not incremental state measured on 2026-11-19 was 6.7 GiB (Dyno:
/// 17.8 GiB in all, 11.1 GiB incremental); 20 leaves about three times that, so the cap removes only runaway growth.
pub const CAP_GB: u64 = 20;

/// The cargo lock files cargo takes in a build folder.
const LOCK_FILES: [&str; 2] = [".cargo-build-lock", ".cargo-lock"];

/// Prunes every kept folder under `targets` (see the module). Messages go to `out`, one line each; nothing here fails a push.
pub fn prune_all(env: &HashMap<String, String>, targets: &Path, now: SystemTime, out: &mut dyn Write) {
    let Ok(entries) = std::fs::read_dir(targets) else { return };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect();
    folders.sort();
    for ws in folders {
        prune_one(env, &ws, now, CAP_GB, out);
    }
}

/// One kept folder with the given cap (GiB, or KiB with `PADDOCK_PRUNE_UNIT=kb` as in [`prune::cap_folder`]).
pub fn prune_one(env: &HashMap<String, String>, ws: &Path, now: SystemTime, cap: u64, out: &mut dyn Write) {
    let _held = match hold_locks(ws) {
        Ok(held) => held,
        Err(busy) => {
            let _ = writeln!(
                out,
                "kept build folder {} is in use ({}); not pruned this time",
                ws.display(),
                busy.display()
            );
            return;
        }
    };
    let home = env.get("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    let mut removed = 0usize;
    for inc in prune::incremental_folders(ws) {
        let Ok(crates) = std::fs::read_dir(&inc) else { continue };
        for c in crates.flatten() {
            let old = c
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| now.duration_since(m).ok())
                .is_some_and(|age| age > STALE);
            if !old {
                continue;
            }
            match safe_remove::remove_under(&inc, &c.path(), home.as_deref()) {
                Ok(true) => removed += 1,
                Ok(false) => {}
                Err(e) => {
                    let _ = writeln!(out, "::warning::kept build folder: could not remove {}: {e}", c.path().display());
                }
            }
        }
    }
    if removed > 0 {
        let _ = writeln!(
            out,
            "kept build folder {}: removed {removed} incremental folder(s) not written for {} days",
            ws.display(),
            STALE.as_secs() / 86_400
        );
    }
    if let Err(e) = prune::cap_folder(env, ws, cap, out) {
        let _ = writeln!(
            out,
            "::warning::kept build folder {}: the cap could not remove it: {e}",
            ws.display()
        );
    }
}

/// Takes every cargo lock in `ws` exclusively, without waiting; the first one another process holds is the error.
fn hold_locks(ws: &Path) -> Result<Vec<File>, PathBuf> {
    let mut held = Vec::new();
    for lock in lock_files(ws) {
        let Ok(f) = OpenOptions::new().read(true).write(true).open(&lock) else {
            // It went away (another prune) or cannot be opened: then it cannot be proved free.
            return Err(lock);
        };
        match f.try_lock() {
            Ok(()) => held.push(f),
            Err(TryLockError::WouldBlock | TryLockError::Error(_)) => return Err(lock),
        }
    }
    Ok(held)
}

/// The cargo lock files below `ws` (a profile folder is at most four levels down: `xwin/<target>/debug`), never through a link.
fn lock_files(ws: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![(ws.to_path_buf(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let Ok(t) = e.file_type() else { continue };
            let name = e.file_name();
            if t.is_file() && LOCK_FILES.iter().any(|l| name == *l) {
                found.push(e.path());
            } else if t.is_dir() && depth < 4 && name != "incremental" && name != "build" && name != "deps" {
                stack.push((e.path(), depth + 1));
            }
        }
    }
    found.sort();
    found
}

// Unix only: a folder's time is set through a handle to it, which Windows opens only with backup semantics (the lock and
// prune code itself is the same on every OS).
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn kb_env() -> HashMap<String, String> {
        [("PADDOCK_PRUNE_UNIT".to_string(), "kb".to_string())].into()
    }

    /// A kept folder `<tmp>/ci/targets/apps-x` with a profile folder holding cargo's two lock files.
    fn kept() -> (tempfile::TempDir, PathBuf) {
        let d = crate::ci_host::canonical_tempdir();
        let ws = d.path().join("ci").join("targets").join("apps-x");
        std::fs::create_dir_all(ws.join("debug/incremental")).unwrap();
        for l in LOCK_FILES {
            File::create(ws.join("debug").join(l)).unwrap();
        }
        (d, ws)
    }

    /// A crate folder last written `age` ago.
    fn crate_folder(ws: &Path, name: &str, age: Duration) -> PathBuf {
        let p = ws.join("debug/incremental").join(name);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("s-blob"), b"x").unwrap();
        let t = SystemTime::now() - age;
        File::open(&p).unwrap().set_modified(t).unwrap();
        p
    }

    fn run(ws: &Path, cap: u64) -> String {
        let mut out = Vec::new();
        prune_one(&kb_env(), ws, SystemTime::now(), cap, &mut out);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn incremental_folders_not_written_for_three_days_go_and_current_ones_stay() {
        let (_d, ws) = kept();
        let old = crate_folder(&ws, "dyno_app-1111", STALE + Duration::from_secs(3600));
        let fresh = crate_folder(&ws, "dyno_app-2222", Duration::from_secs(3600));
        let out = run(&ws, 1_000_000);
        assert!(!old.exists(), "{out}");
        assert!(fresh.exists(), "{out}");
        assert!(out.contains("removed 1 incremental folder(s)"), "{out}");
    }

    #[test]
    fn a_folder_a_build_holds_is_never_touched() {
        let (_d, ws) = kept();
        let old = crate_folder(&ws, "dyno_app-1111", STALE + Duration::from_secs(3600));
        // What a running stable cargo holds: `.cargo-build-lock` exclusively (another process; a second open file
        // description in this one conflicts the same way under flock and LockFileEx).
        let build = OpenOptions::new()
            .read(true)
            .write(true)
            .open(ws.join("debug/.cargo-build-lock"))
            .unwrap();
        build.lock().unwrap();
        let out = run(&ws, 0);
        assert!(old.exists(), "{out}");
        assert!(out.contains("is in use") && out.contains(".cargo-build-lock"), "{out}");
        build.unlock().unwrap();
    }

    #[test]
    fn a_shared_hold_of_the_artifact_lock_also_counts_as_in_use() {
        let (_d, ws) = kept();
        let old = crate_folder(&ws, "dyno_app-1111", STALE + Duration::from_secs(3600));
        let artifact = OpenOptions::new().read(true).open(ws.join("debug/.cargo-lock")).unwrap();
        artifact.lock_shared().unwrap();
        let out = run(&ws, 0);
        assert!(old.exists(), "{out}");
        assert!(out.contains("is in use"), "{out}");
        artifact.unlock().unwrap();
    }

    #[test]
    fn over_the_cap_the_shared_cap_runs_once_the_folder_is_free() {
        let (_d, ws) = kept();
        let fresh = crate_folder(&ws, "dyno_app-2222", Duration::from_secs(60));
        std::fs::write(fresh.join("big"), vec![7u8; 64 * 1024]).unwrap();
        let out = run(&ws, 16);
        assert!(out.contains("removing incremental caches"), "{out}");
        assert!(!fresh.exists(), "{out}");
    }

    #[test]
    fn prune_all_visits_every_kept_folder_and_ignores_files() {
        let (d, ws) = kept();
        let old = crate_folder(&ws, "a-1", STALE + Duration::from_secs(3600));
        let other = d.path().join("ci/targets/crates");
        std::fs::create_dir_all(other.join("debug/incremental/b-1")).unwrap();
        File::open(other.join("debug/incremental/b-1"))
            .unwrap()
            .set_modified(SystemTime::now() - STALE - Duration::from_secs(3600))
            .unwrap();
        std::fs::write(d.path().join("ci/targets/a-file"), b"x").unwrap();
        let mut out = Vec::new();
        // GiB units: the 20 GiB cap is far away, so only the age rule acts.
        prune_all(&HashMap::new(), &d.path().join("ci/targets"), SystemTime::now(), &mut out);
        let out = String::from_utf8(out).unwrap();
        assert!(!old.exists() && !other.join("debug/incremental/b-1").exists(), "{out}");
    }
}
