//! The prune steps: `dev ci prune-folder`, `prune-build-folder` and `prune-agent-builds` (the ports of
//! `tools/prune-build-folders.sh`, `tools/prune-ci-build-folder.sh` and `tools/prune-build-folders.ps1`).
//!
//! - **`prune-folder <folder> <max-gb>`**: a persistent CI build folder (host runners) stays within a cap so nothing piles up on the
//!   owner's machines. Over the cap the incremental-compilation caches go first (the bulk, and cheap to rebuild); still over, the
//!   folder is deleted. A missing folder, or a link, is fine and untouched (links are never followed). The size is the larger of
//!   the allocated size and the length of the files: a file system that allocates late (ZFS, btrfs, a compressed volume) reports
//!   next to nothing allocated for data written a moment ago, which once kept a full folder under the cap.
//!   `PADDOCK_PRUNE_UNIT=kb` makes the cap kilobytes instead of gigabytes (tests).
//! - **`prune-build-folder --max-gb <n> [--drop-bundles]`**: the CI form. Only the job's own kept build folder
//!   (`$PADDOCK_BUILD_FOLDER`), and only when `ci isolation owns <folder> target` says it is a `target-<name>-<slot>` folder of this
//!   job's slot directly in the CI home: a link, `$HOME`, `/`, a project folder, a path outside the CI home and another slot's folder
//!   (a concurrent job's) are refused with a notice. `--drop-bundles` first removes `<folder>/dist/bundle` (installers already
//!   uploaded). Always exits 0.
//! - **`prune-agent-builds --app <id> [--remove] [--keep <suffix>]...`**: lists the build folders agents left (`~/.<app>-target*` and
//!   `<repo>/target/*` except debug, release, rust-analyzer, doc, tmp and flycheck*) with their sizes; with `--remove` deletes those
//!   not kept (a `--keep 6c` keeps `~/.<app>-target-6c*` and `target/agent-6c*`). Ends with the free space of the repository's drive.
//!
//! Deletions go through [`safe_remove`](super::safe_remove): only under the folder being capped, never through a link.

use super::Step;
use super::safe_remove::{self, RemoveError};
use clap::Args as ClapArgs;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

/// `dev ci prune-folder`.
#[derive(ClapArgs, Debug)]
pub struct FolderArgs {
    /// The folder to cap.
    pub folder: PathBuf,
    /// The cap in whole gigabytes (kilobytes with `PADDOCK_PRUNE_UNIT=kb`).
    pub max_gb: String,
}

/// `dev ci prune-build-folder`.
#[derive(ClapArgs, Debug)]
pub struct CiArgs {
    /// The cap in whole gigabytes.
    #[arg(long, default_value = "20")]
    pub max_gb: String,
    /// First removes `<folder>/dist/bundle` (installers already uploaded as artifacts).
    #[arg(long)]
    pub drop_bundles: bool,
}

/// `dev ci prune-agent-builds`.
#[derive(ClapArgs, Debug)]
pub struct AgentArgs {
    /// The app id (lower-case letters, digits, dashes).
    #[arg(long)]
    pub app: String,
    /// The repository (default: the current folder).
    #[arg(long)]
    pub repo: Option<PathBuf>,
    /// Delete the folders not kept; without it only list them.
    #[arg(long)]
    pub remove: bool,
    /// Suffixes of build folders still in use (`6c` keeps `~/.<app>-target-6c*` and `target/agent-6c*`); repeat or comma-separate.
    #[arg(long, value_delimiter = ',')]
    pub keep: Vec<String>,
}

/// The size of a folder: the allocated bytes and the length of the files, both counted once per file (a hard link is one file).
/// Links are never followed.
pub fn sizes(dir: &Path) -> (u64, u64) {
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let (mut allocated, mut apparent) = (0u64, 0u64);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(meta) = std::fs::symlink_metadata(&d) else { continue };
        count(&meta, &mut seen, &mut allocated, &mut apparent);
        if meta.is_dir() && !meta.file_type().is_symlink() {
            let Ok(entries) = std::fs::read_dir(&d) else { continue };
            for e in entries.flatten() {
                stack.push(e.path());
            }
        }
    }
    (allocated, apparent)
}

fn count(meta: &std::fs::Metadata, seen: &mut HashSet<(u64, u64)>, allocated: &mut u64, apparent: &mut u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() > 1 && !meta.is_dir() && !seen.insert((meta.dev(), meta.ino())) {
            return;
        }
        *allocated += meta.blocks() * 512;
    }
    #[cfg(not(unix))]
    {
        let _ = seen;
        *allocated += meta.len();
    }
    *apparent += meta.len();
}

/// The folder's size in the cap's unit: the larger of the allocated size and the length of the files, in whole units (rounded
/// down, as the script's integer division was). `kb` counts kilobytes, else gigabytes of 1 048 576 KB.
pub fn size_in_units(dir: &Path, kb: bool) -> u64 {
    let (allocated, apparent) = sizes(dir);
    let kib = |b: u64| b.div_ceil(1024);
    let used = kib(allocated).max(kib(apparent));
    used / if kb { 1 } else { 1_048_576 }
}

fn cap_in_units(env: &HashMap<String, String>) -> bool {
    env.get("PADDOCK_PRUNE_UNIT").is_some_and(|u| u == "kb")
}

fn home_of(env: &HashMap<String, String>) -> Option<PathBuf> {
    env.get("HOME")
        .or_else(|| env.get("USERPROFILE"))
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// Every folder named `incremental` below `dir` (not looking inside one), never through a link.
fn incremental_folders(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let Ok(t) = e.file_type() else { continue };
            if !t.is_dir() {
                continue;
            }
            if e.file_name() == "incremental" {
                found.push(e.path());
            } else {
                stack.push(e.path());
            }
        }
    }
    found.sort();
    found
}

/// The cap itself: messages on `out`, `Err` only for a refusal of the guard.
pub fn cap_folder(env: &HashMap<String, String>, dir: &Path, max: u64, out: &mut dyn Write) -> Result<(), RemoveError> {
    let kb = cap_in_units(env);
    let home = home_of(env);
    let md = std::fs::symlink_metadata(dir);
    if !md.is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink()) {
        return Ok(());
    }
    let size = size_in_units(dir, kb);
    if size >= max {
        let _ = writeln!(out, "prune: {} is {size} (cap {max}); removing incremental caches", dir.display());
        for f in incremental_folders(dir) {
            if let Err(e) = safe_remove::remove_under(dir, &f, home.as_deref()) {
                let _ = writeln!(out, "::warning::prune: could not remove {}: {e}", f.display());
            }
        }
    }
    let size = size_in_units(dir, kb);
    if size >= max {
        let _ = writeln!(out, "prune: still {size}; removing {}", dir.display());
        let parent = dir.parent().unwrap_or(Path::new(""));
        safe_remove::remove_under(parent, dir, home.as_deref())?;
    }
    Ok(())
}

/// `dev ci prune-folder`.
pub fn run_folder(env: &HashMap<String, String>, a: &FolderArgs, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Ok(max) = a.max_gb.parse::<u64>().map_err(|_| ()).and_then(|n| {
        if a.max_gb.bytes().all(|b| b.is_ascii_digit()) {
            Ok(n)
        } else {
            Err(())
        }
    }) else {
        let _ = writeln!(err, "prune: the cap must be a whole number, got '{}'", a.max_gb);
        return 2;
    };
    match cap_folder(env, &a.folder, max, out) {
        Ok(()) => 0,
        Err(e) => {
            let _ = writeln!(out, "::warning::prune: {} was not removed: {e}", a.folder.display());
            0
        }
    }
}

/// `dev ci prune-build-folder`: the CI form; always 0.
pub fn run_ci(env: &HashMap<String, String>, windows: bool, a: &CiArgs, out: &mut dyn Write) -> i32 {
    let mut step = Step {
        env,
        out,
        windows,
        name: "prune",
    };
    let Some(dir) = step.var("PADDOCK_BUILD_FOLDER").map(str::to_string) else {
        step.say("prune: no build folder was set in this job; nothing to prune");
        return 0;
    };
    let max = match a.max_gb.parse::<u64>() {
        Ok(n) if a.max_gb.bytes().all(|b| b.is_ascii_digit()) => n,
        _ => {
            step.say(format!(
                "prune: the cap must be a whole number, got '{}'; nothing to prune",
                a.max_gb
            ));
            return 0;
        }
    };
    if step.isolation(&["owns", &dir, "target"]).is_err() {
        let slot = step.var("PADDOCK_CI_SLOT").unwrap_or("<slot>").to_string();
        step.say(format!(
            "prune: {dir} is not a kept build folder of this job's slot (target-<name>-{slot} in the CI home); leaving it alone"
        ));
        return 0;
    }
    let real = match paddock_env::winpath::real(Path::new(&dir)) {
        Ok(p) => p,
        Err(e) => {
            step.say(format!("prune: cannot read {dir} ({e}); nothing to prune"));
            return 0;
        }
    };
    let home = home_of(env);
    if a.drop_bundles
        && let Err(e) = safe_remove::remove_under(&real, &real.join("dist").join("bundle"), home.as_deref())
    {
        step.say(format!("::warning::prune: could not remove the bundles: {e}"));
    }
    if let Err(e) = cap_folder(env, &real, max, step.out) {
        step.say(format!("::warning::prune: {} was not removed: {e}", real.display()));
    }
    0
}

/// The match of a name against a prefix pattern, ignoring case (PowerShell's `-like 'prefix*'`).
fn like_prefix(name: &str, prefix: &str) -> bool {
    name.to_lowercase().starts_with(&prefix.to_lowercase())
}

/// The folders agents left, from the home folder and the repository's `target`.
pub fn agent_candidates(home: &Path, repo: &Path, app: &str) -> Vec<(PathBuf, PathBuf)> {
    const PROTECTED: [&str; 5] = ["debug", "release", "rust-analyzer", "doc", "tmp"];
    let mut found = Vec::new();
    let dirs = |root: &Path| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(root)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.path())
            .collect();
        v.sort();
        v
    };
    let prefix = format!(".{app}-target");
    for d in dirs(home) {
        if d.file_name().is_some_and(|n| like_prefix(&n.to_string_lossy(), &prefix)) {
            found.push((home.to_path_buf(), d));
        }
    }
    let target = repo.join("target");
    for d in dirs(&target) {
        let n = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if !PROTECTED.contains(&n.as_str()) && !like_prefix(&n, "flycheck") {
            found.push((target.clone(), d));
        }
    }
    found
}

/// Whether `name` belongs to a build still in use.
pub fn is_kept(name: &str, app: &str, keep: &[String]) -> bool {
    keep.iter()
        .any(|k| like_prefix(name, &format!(".{app}-target-{k}")) || like_prefix(name, &format!("agent-{k}")))
}

/// `dev ci prune-agent-builds`.
pub fn run_agent(env: &HashMap<String, String>, a: &AgentArgs, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let valid = a.app.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && a.app.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid {
        let _ = writeln!(
            err,
            "prune-agent-builds: --app must be lower-case letters, digits and dashes, starting with a letter, got '{}'",
            a.app
        );
        return 2;
    }
    let Some(home) = home_of(env) else {
        let _ = writeln!(err, "prune-agent-builds: HOME is not set");
        return 2;
    };
    let repo = a.repo.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let mut code = 0;
    for (root, dir) in agent_candidates(&home, &repo, &a.app) {
        let (_, apparent) = sizes(&dir);
        let gb = apparent as f64 / 1_073_741_824.0;
        let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let keep = is_kept(&name, &a.app, &a.keep);
        let action = if !a.remove {
            "found"
        } else if keep {
            "kept"
        } else {
            "removing"
        };
        let _ = writeln!(out, "{gb:>8.1} GB  {action:<9} {}", dir.display());
        if a.remove && !keep {
            // The candidates are direct children of the home folder named `.<app>-target*` or of the repository's target folder,
            // so the home check is off for this one command (its root is the home folder).
            if let Err(e) = safe_remove::remove_under(&root, &dir, None) {
                let _ = writeln!(err, "::warning::prune-agent-builds: could not remove {}: {e}", dir.display());
                code = 1;
            }
        }
    }
    match free_gb(&repo) {
        Some(g) => {
            let _ = writeln!(out, "Free on {}: {g:.1} GB", repo.display());
        }
        None => {
            let _ = writeln!(out, "Free on {}: unknown", repo.display());
        }
    }
    code
}

/// The free space of the drive `p` is on, in GB (1 073 741 824 bytes); `None` where it cannot be read.
#[cfg(unix)]
#[allow(unsafe_code)] // one FFI call with a zeroed struct of the documented type
pub fn free_gb(p: &Path) -> Option<f64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(p.as_os_str().as_bytes()).ok()?;
    // SAFETY: an all-zero statvfs is a valid value to pass for output; `c` is a live NUL-terminated path.
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are valid for the call.
    let ok = unsafe { libc::statvfs(c.as_ptr(), &mut s) };
    (ok == 0).then(|| s.f_bavail as f64 * s.f_frsize as f64 / 1_073_741_824.0)
}

/// The free space of the drive `p` is on, in GB; `None` where it cannot be read.
#[cfg(windows)]
#[allow(unsafe_code)] // one FFI call with documented out-pointers
pub fn free_gb(p: &Path) -> Option<f64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free: u64 = 0;
    // SAFETY: `wide` is a live NUL-terminated string and `free` a live u64; the other two out-pointers may be null.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then(|| free as f64 / 1_073_741_824.0)
}

/// Not Unix or Windows: not known.
#[cfg(not(any(unix, windows)))]
pub fn free_gb(_: &Path) -> Option<f64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn big(root: &Path, path: &str, kb: usize) {
        let dir = root.join(path);
        std::fs::create_dir_all(&dir).unwrap();
        // Distinct bytes: a file system that compresses makes a run of equal bytes nearly free in the allocated size.
        let data: Vec<u8> = (0..kb * 1024).map(|i| (i.wrapping_mul(2_654_435_761usize) >> 7) as u8).collect();
        std::fs::write(dir.join("blob"), data).unwrap();
    }

    fn scratch() -> (tempfile::TempDir, PathBuf) {
        let d = crate::ci_host::canonical_tempdir();
        let root = d.path().join("ci").join("build");
        std::fs::create_dir_all(&root).unwrap();
        (d, root)
    }

    fn kb_env() -> HashMap<String, String> {
        [("PADDOCK_PRUNE_UNIT".to_string(), "kb".to_string())].into()
    }

    fn prune(dir: &Path, cap: &str) -> (i32, String, String) {
        let (mut o, mut e) = (Vec::new(), Vec::new());
        let code = run_folder(
            &kb_env(),
            &FolderArgs {
                folder: dir.to_path_buf(),
                max_gb: cap.into(),
            },
            &mut o,
            &mut e,
        );
        (code, String::from_utf8(o).unwrap(), String::from_utf8(e).unwrap())
    }

    #[test]
    fn under_the_cap_nothing_is_touched() {
        let (_d, dir) = scratch();
        big(&dir, "debug/incremental", 100);
        let (code, out, _) = prune(&dir, "100000");
        assert_eq!((code, out.as_str()), (0, ""));
        assert!(dir.join("debug/incremental/blob").exists());
    }

    #[test]
    fn over_the_cap_the_incremental_caches_go_first_and_the_rest_stays() {
        let (_d, dir) = scratch();
        big(&dir, "debug/incremental", 400);
        // conformance: layout-guess — prunes the `deps` folder of a build folder on purpose (a build artifact folder in both layouts); not a layout guess
        big(&dir, "debug/deps", 50);
        let (code, out, _) = prune(&dir, "300");
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("removing incremental caches"), "{out}");
        assert!(!dir.join("debug/incremental").exists());
        // conformance: layout-guess — prunes the `deps` folder of a build folder on purpose (a build artifact folder in both layouts); not a layout guess
        assert!(dir.join("debug/deps/blob").exists());
    }

    #[cfg(unix)]
    #[test]
    fn the_length_of_the_files_counts_not_only_the_blocks_the_file_system_has_allocated_so_far() {
        let (_d, dir) = scratch();
        std::fs::create_dir_all(dir.join("debug/incremental")).unwrap();
        // A sparse file: 400 KB long, nothing allocated (a file system that allocates late reports the same).
        let f = std::fs::File::create(dir.join("debug/incremental/blob")).unwrap();
        f.set_len(400 * 1024).unwrap();
        // conformance: layout-guess — prunes the `deps` folder of a build folder on purpose (a build artifact folder in both layouts); not a layout guess
        big(&dir, "debug/deps", 1);
        let (code, out, _) = prune(&dir, "300");
        assert_eq!(code, 0);
        assert!(out.contains("removing incremental caches"), "{out}");
        assert!(!dir.join("debug/incremental").exists());
    }

    #[test]
    fn still_over_after_that_the_folder_is_removed() {
        let (_d, dir) = scratch();
        big(&dir, "debug/incremental", 10);
        // conformance: layout-guess — prunes the `deps` folder of a build folder on purpose (a build artifact folder in both layouts); not a layout guess
        big(&dir, "debug/deps", 400);
        let (_, out, _) = prune(&dir, "300");
        assert!(out.contains("prune: still ") && out.contains("; removing "), "{out}");
        assert!(!dir.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_folder_is_fine_a_link_is_never_followed_and_a_bad_cap_is_refused() {
        let (d, dir) = scratch();
        assert_eq!(prune(&d.path().join("nope"), "1").0, 0);
        big(&dir, "incremental", 500);
        let link = d.path().join("ci/link");
        std::os::unix::fs::symlink(&dir, &link).unwrap();
        assert_eq!(prune(&link, "1").0, 0);
        assert!(dir.join("incremental/blob").exists(), "the target of a link is left alone");
        for bad in ["many", "-1", "+5", ""] {
            let (code, _, err) = prune(&dir, bad);
            assert_eq!(code, 2, "{bad}");
            assert!(err.contains("the cap must be a whole number"), "{err}");
        }
    }

    #[test]
    fn a_folder_directly_under_the_home_or_the_drive_root_is_refused_rather_than_removed() {
        let d = crate::ci_host::canonical_tempdir();
        let home = d.path().join("home");
        let dir = home.join("target-x");
        // conformance: layout-guess — prunes the `deps` folder of a build folder on purpose (a build artifact folder in both layouts); not a layout guess
        big(&dir, "debug/deps", 400);
        let mut env = kb_env();
        env.insert("HOME".into(), home.display().to_string());
        let (mut o, mut e) = (Vec::new(), Vec::new());
        let code = run_folder(
            &env,
            &FolderArgs {
                folder: dir.clone(),
                max_gb: "1".into(),
            },
            &mut o,
            &mut e,
        );
        assert_eq!(code, 0, "a prune never fails the job");
        assert!(String::from_utf8(o).unwrap().contains("::warning::prune:"), "the refusal is said");
        assert!(
            // conformance: layout-guess — prunes the `deps` folder of a build folder on purpose (a build artifact folder in both layouts); not a layout guess
            dir.join("debug/deps/blob").exists(),
            "the folder whose parent is the home folder stays"
        );
    }

    // ---- the CI form ----

    struct Ci {
        /// Keeps the scratch folder alive; read only by the Unix tests.
        #[allow(dead_code)]
        d: tempfile::TempDir,
        home: PathBuf,
    }

    fn ci() -> Ci {
        let d = crate::ci_host::canonical_tempdir();
        let home = paddock_env::winpath::real(d.path()).unwrap().join("home");
        std::fs::create_dir_all(home.join(".paddock-ci")).unwrap();
        Ci { d, home }
    }

    fn ci_prune(c: &Ci, dir: &str, slot: &str, drop: bool) -> (i32, String) {
        let mut env = kb_env();
        env.insert("HOME".into(), c.home.display().to_string());
        env.insert("RUNNER_OS".into(), "macOS".into());
        if !slot.is_empty() {
            env.insert("PADDOCK_CI_SLOT".into(), slot.into());
        }
        if !dir.is_empty() {
            env.insert("PADDOCK_BUILD_FOLDER".into(), dir.into());
        }
        let mut out = Vec::new();
        let code = run_ci(
            &env,
            false,
            &CiArgs {
                max_gb: "1".into(),
                drop_bundles: drop,
            },
            &mut out,
        );
        (code, String::from_utf8(out).unwrap())
    }

    #[test]
    fn a_target_folder_of_this_slot_in_the_ci_home_is_capped_and_its_bundles_go_with_drop_bundles() {
        let c = ci();
        let dir = c.home.join(".paddock-ci/target-app-app-s1");
        big(&dir, "debug/incremental", 3000);
        big(&dir, "dist/bundle", 10);
        let (code, out) = ci_prune(&c, &dir.display().to_string(), "s1", true);
        assert_eq!(code, 0, "{out}");
        assert!(!dir.join("debug/incremental").exists(), "{out}");
        assert!(!dir.join("dist/bundle").exists());
        // Another job's folder (another slot) is never pruned, even by name.
        let theirs = c.home.join(".paddock-ci/target-app-app-s2");
        big(&theirs, "debug/incremental", 3000);
        let (_, out) = ci_prune(&c, &theirs.display().to_string(), "s1", false);
        assert!(out.contains("leaving it alone"), "{out}");
        assert!(
            theirs.join("debug/incremental/blob").exists(),
            "the other slot's build folder stays"
        );
        assert!(
            ci_prune(&c, &dir.display().to_string(), "", false).1.contains("leaving it alone"),
            "no slot, nothing pruned"
        );
    }

    #[cfg(unix)]
    #[test]
    fn anything_else_is_left_alone_with_a_notice() {
        let c = ci();
        let outside = paddock_env::winpath::real(c.d.path()).unwrap().join("outside");
        big(&outside, "target-x/debug/incremental", 3000);
        let real = c.home.join(".paddock-ci/target-real-s1");
        big(&real, "debug/incremental", 3000);
        std::os::unix::fs::symlink(&real, c.home.join(".paddock-ci/target-link")).unwrap();
        big(&c.home, ".paddock-ci/notes/debug/incremental", 3000);
        big(&c.home, "Documents/debug/incremental", 3000);
        let dotted = format!("{}/.paddock-ci/target-real-s1/../target-real-s1/../../Documents", c.home.display());
        let cases: Vec<(&str, String, PathBuf)> = vec![
            ("unset", String::new(), real.join("debug/incremental/blob")),
            (
                "home",
                c.home.display().to_string(),
                c.home.join("Documents/debug/incremental/blob"),
            ),
            ("root", "/".into(), c.home.join("Documents/debug/incremental/blob")),
            (
                "outside",
                outside.join("target-x").display().to_string(),
                outside.join("target-x/debug/incremental/blob"),
            ),
            (
                "symlink",
                c.home.join(".paddock-ci/target-link").display().to_string(),
                real.join("debug/incremental/blob"),
            ),
            (
                "not target-*",
                c.home.join(".paddock-ci/notes").display().to_string(),
                c.home.join(".paddock-ci/notes/debug/incremental/blob"),
            ),
            (
                "the CI home",
                c.home.join(".paddock-ci").display().to_string(),
                real.join("debug/incremental/blob"),
            ),
            ("a dotted path out of it", dotted, c.home.join("Documents/debug/incremental/blob")),
        ];
        for (name, dir, canary) in cases {
            let (code, out) = ci_prune(&c, &dir, "s1", false);
            assert_eq!(code, 0, "{name}: {out}");
            assert!(canary.exists(), "{name} must not delete {}", canary.display());
            assert!(
                out.contains("nothing to prune") || out.contains("leaving it alone"),
                "{name}: {out}"
            );
        }
    }

    // ---- agent builds ----

    #[test]
    fn agents_build_folders_are_found_listed_and_removed_except_the_kept_and_protected_ones() {
        let d = crate::ci_host::canonical_tempdir();
        let home = d.path().join("u/home");
        let repo = d.path().join("repo");
        for p in [
            ".debrief-target-6c",
            ".debrief-target-6c2",
            ".debrief-target-sdk",
            ".other-target-1",
            "plain",
        ] {
            big(&home, p, 4);
        }
        for p in [
            "debug",
            "release",
            "rust-analyzer",
            "doc",
            "tmp",
            "flycheck0",
            "agent-6c1",
            "agent-7d",
        ] {
            big(&repo, &format!("target/{p}"), 4);
        }
        let env: HashMap<String, String> = [("HOME".to_string(), home.display().to_string())].into();
        let run = |remove: bool, keep: &[&str]| {
            let (mut o, mut e) = (Vec::new(), Vec::new());
            let a = AgentArgs {
                app: "debrief".into(),
                repo: Some(repo.clone()),
                remove,
                keep: keep.iter().map(|s| s.to_string()).collect(),
            };
            let code = run_agent(&env, &a, &mut o, &mut e);
            (code, String::from_utf8(o).unwrap(), String::from_utf8(e).unwrap())
        };
        let (code, out, _) = run(false, &[]);
        assert_eq!(code, 0);
        let listed: Vec<&str> = out.lines().filter(|l| l.contains(" GB  ")).collect();
        assert_eq!(listed.len(), 5, "{out}");
        assert!(listed.iter().all(|l| l.contains("found")), "{out}");
        assert!(out.lines().last().unwrap().starts_with("Free on "), "{out}");
        assert!(listed[0].starts_with("     0.0 GB  found     "), "{}", listed[0]);
        let (code, out, err) = run(true, &["6c", "sdk"]);
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("kept") && out.contains("removing"), "{out}");
        assert!(
            home.join(".debrief-target-6c").exists()
                && home.join(".debrief-target-6c2").exists()
                && home.join(".debrief-target-sdk").exists()
        );
        assert!(repo.join("target/agent-6c1").exists(), "kept by suffix");
        assert!(!repo.join("target/agent-7d").exists());
        for stays in [".other-target-1", "plain"] {
            assert!(home.join(stays).exists(), "{stays}");
        }
        for protected in ["debug", "release", "rust-analyzer", "doc", "tmp", "flycheck0"] {
            assert!(repo.join("target").join(protected).exists(), "{protected}");
        }
    }

    #[test]
    fn a_bad_app_id_is_refused() {
        let env: HashMap<String, String> = [("HOME".to_string(), "/x/y".to_string())].into();
        for bad in ["", "Debrief", "a b", "1x", "x_y"] {
            let (mut o, mut e) = (Vec::new(), Vec::new());
            let a = AgentArgs {
                app: bad.into(),
                repo: None,
                remove: false,
                keep: vec![],
            };
            assert_eq!(run_agent(&env, &a, &mut o, &mut e), 2, "{bad:?}");
        }
    }
}
