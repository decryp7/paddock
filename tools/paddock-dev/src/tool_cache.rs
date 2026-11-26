//! The tool cache: `<PADDOCK_DEV_HOME or ~/.paddock-dev>/bin/` (and `bin-source/`), where `./dev` keeps paddock-dev
//! (`<hash>/paddock-dev-<host>`) and paddock-dev keeps the tools it builds (`throttle-<hash>/heavy`, [`crate::pre_push::throttle_tool`]).
//! One mechanism for every cached tool, so a long run never loses the binary it resolved (ER-21; CI run 7773's job-isolation, and
//! on 2026-11-14 six generators of one train-10 `gen check` found heavy gone):
//!  - [`hold`]: a run that starts a cached tool more than once holds its own name for it, a hard link (else a copy) under
//!    `<home>/held/`, and starts that. No prune, failed probe or publish by another run can take it away. `./dev`'s prune never
//!    removes an entry with a held file (a link count above one), so the entry stays as well.
//!  - [`touch`]: a cache hit marks its entry as used; the prune keeps an entry used within the last day.
//!  - [`hold_self`]: paddock-dev holds its own binary for the whole run when it runs from the cache, and [`self_exe`] names that
//!    hold, so the tool's own children (the planner, the pre-push gates) never start a path that is gone.
//!
//! The prune lives in `./dev` (it runs before this tool exists); a cached binary that fails its probe is never removed, only
//! replaced by an atomic rename.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The cache folder: `PADDOCK_DEV_HOME`, else `~/.paddock-dev` (what `./dev` uses).
pub fn home(env: &HashMap<String, String>) -> Result<PathBuf, String> {
    match (env.get("PADDOCK_DEV_HOME"), env.get("HOME")) {
        (Some(h), _) if !h.is_empty() => Ok(PathBuf::from(h)),
        (_, Some(h)) if !h.is_empty() => Ok(Path::new(h).join(".paddock-dev")),
        _ => Err("neither PADDOCK_DEV_HOME nor HOME is set, so there is no cache folder for the tools".into()),
    }
}

/// `name` with the platform's executable suffix.
pub fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// Marks a cache entry `dir` as used now (its modification time), so `./dev`'s prune keeps it for a day. Errors are ignored: it
/// is only a hint (a miss costs a rebuild).
pub fn touch(dir: &Path) {
    let _ = std::fs::File::open(dir).and_then(|f| f.set_modified(SystemTime::now()));
}

/// A run's own hold on a cached tool (see the module docs). Removed when dropped.
#[derive(Debug)]
pub struct Held {
    path: PathBuf,
}

impl Held {
    /// The held binary: start this, never the shared cache path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        // Our own unique name, made by `hold`: nothing else is removed (ER-14).
        let _ = std::fs::remove_file(&self.path);
    }
}

/// How old a hold must be before a later [`hold`] removes it as left behind by a run that was killed: no run lasts a week.
const STALE_HOLD: Duration = Duration::from_secs(7 * 24 * 3600);

/// Holds the cached binary `bin` of `tool` for this run (see [`Held`]) and says so on stderr. `Err` is the whole explanation.
pub fn hold(tool: &str, bin: &Path, env: &HashMap<String, String>) -> Result<Held, String> {
    let held = link(tool, bin, env)?;
    // ER-27: which binary this run starts, so a later "not found" names the hold, not the shared cache.
    eprintln!("{tool}: this run holds {} as {}", bin.display(), held.path.display());
    Ok(held)
}

/// [`hold`] without the message: paddock-dev's hold of itself happens on every run and would say it every time.
fn link(tool: &str, bin: &Path, env: &HashMap<String, String>) -> Result<Held, String> {
    let dir = home(env)?.join("held");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot hold {tool}: cannot make {}: {e}. Next: make it writable", dir.display()))?;
    sweep(&dir, SystemTime::now());
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let path = dir.join(exe(&format!("{tool}-{}-{nanos:x}", std::process::id())));
    // A hard link costs nothing and keeps the file alive whatever happens to the cache's name; a copy where links do not work.
    std::fs::hard_link(bin, &path)
        .or_else(|_| std::fs::copy(bin, &path).map(|_| ()))
        .map_err(|e| {
            format!(
                "cannot hold {tool}: {} could not be linked or copied to {}: {e}. Next: run the command again (another agent may have replaced the cache at that moment)",
                bin.display(),
                path.display()
            )
        })?;
    Ok(Held { path })
}

/// When a hold was made, from its name (`<tool>-<pid>-<nanos, hex>[.exe]`). Never from the file's times: a hard link shares
/// them with the cached binary, which may have been built weeks ago.
fn made(name: &str) -> Option<SystemTime> {
    let stem = name.strip_suffix(".exe").unwrap_or(name);
    let nanos = u128::from_str_radix(stem.rsplit_once('-')?.1, 16).ok()?;
    UNIX_EPOCH.checked_add(Duration::from_nanos(u64::try_from(nanos).ok()?))
}

/// Removes holds made more than [`STALE_HOLD`] before `now` (a run that was killed before its hold was dropped). A name this
/// module did not make is left alone.
fn sweep(dir: &Path, now: SystemTime) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let stale = made(&e.file_name().to_string_lossy())
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > STALE_HOLD);
        if stale {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

static SELF_HOLD: OnceLock<Option<Held>> = OnceLock::new();

/// Holds this program's own binary for the whole run when it runs from the tool cache (`<home>/bin*/`), so a prune or a
/// republish by another run cannot remove the path its children start (see [`self_exe`]). Once per process; best effort: without
/// a hold [`self_exe`] is the program's own path. Returns the hold's path; `main` calls [`release_self`] before it exits.
///
/// Unix only. On Windows a hard link of a running program cannot be removed while it runs, so every run would leave its hold for
/// the week-long sweep; and Windows cannot remove the running program's own cache entry either, so the prune or a republish
/// fails on it and the entry stays for the run anyway.
#[cfg(unix)]
pub fn hold_self(env: &HashMap<String, String>) -> Option<&'static Path> {
    SELF_HOLD
        .get_or_init(|| {
            let me = std::env::current_exe().ok()?;
            let home = home(env).ok()?;
            let cached = ["bin", "bin-source"].iter().any(|b| me.starts_with(home.join(b)));
            if !cached {
                return None;
            }
            link("paddock-dev", &me, env).map_err(|why| eprintln!("::warning::{why}")).ok()
        })
        .as_ref()
        .map(Held::path)
}

/// Ends this process's own hold ([`hold_self`]): `std::process::exit` runs no destructors.
#[cfg(unix)]
pub fn release_self() {
    if let Some(Some(h)) = SELF_HOLD.get() {
        let _ = std::fs::remove_file(h.path());
    }
}

/// This program, to start again as a child: its own hold when it has one ([`hold_self`]), else its own path ([`self_exe_of`]).
/// The one way paddock-dev names itself as a child's program.
///
/// # Errors
///
/// When the path is unknown, or this process is a test program (see [`self_exe_of`]).
pub fn self_exe() -> std::io::Result<PathBuf> {
    match SELF_HOLD.get() {
        Some(Some(h)) => Ok(h.path().to_path_buf()),
        _ => self_exe_of(&std::env::current_exe()?),
    }
}

/// `exe` as the program to start again, unless it is a test program: one in cargo's test layout (`<profile>/deps/`,
/// `<profile>/build/<package>/<hash>/out/`, [`paddock_env::cargo_layout`]). A unit or integration test runs this crate's code inside
/// the test harness, and a harness started with `gates check` takes the words as test-name filters and runs every matching test
/// again inside the test that started it: train 14's push-gate row `gates-register` did that (a 0.4 s test took 14 to 17 s on the
/// Mac and timed out at 60 s on the Windows runner, runs 8245 and 8248, while its nested runs slowed every test beside it). A
/// test names the real binary instead (`CARGO_BIN_EXE_paddock-dev`, or a stand-in).
///
/// # Errors
///
/// When `exe` is a test program.
pub fn self_exe_of(exe: &Path) -> std::io::Result<PathBuf> {
    if paddock_env::cargo_layout::profile_dir_of_exe(exe).is_some() {
        return Err(std::io::Error::other(format!(
            "{} is a test program (cargo's test layout), not paddock-dev: started again it would run its tests; next step: a test \
             names the real binary (CARGO_BIN_EXE_paddock-dev) or a stand-in",
            exe.display()
        )));
    }
    Ok(exe.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_tool(home: &Path, entry: &str, name: &str) -> PathBuf {
        let dir = home.join("bin").join(entry);
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join(exe(name));
        // One file with its script at its end: `hold` links it under another name, and it still answers.
        let program = paddock_testkit::StandIn::new(&dir, name).stdout("held").files().unwrap();
        paddock_testkit::exe::write_program(&bin, program.bundled()).unwrap();
        bin
    }

    fn env_for(home: &Path) -> HashMap<String, String> {
        HashMap::from([("PADDOCK_DEV_HOME".to_string(), home.display().to_string())])
    }

    #[test]
    fn a_held_tool_survives_another_run_removing_or_replacing_the_cache_and_the_run_completes() {
        for tool in ["throttle", "paddock-dev"] {
            let home = tempfile::tempdir().unwrap();
            let env = env_for(home.path());
            let cached = fake_tool(home.path(), &format!("{tool}-abc"), tool);
            let held = hold(tool, &cached, &env).unwrap();
            assert_ne!(held.path(), cached, "the run uses its own name, not the shared cache's");
            assert!(
                !held.path().starts_with(home.path().join("bin")),
                "outside the folder `./dev` prunes"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                assert!(
                    std::fs::metadata(&cached).unwrap().nlink() > 1,
                    "{tool}: `./dev`'s prune sees the entry is held"
                );
            }
            // Another agent's `./dev` prunes the entry (or a failed probe removes the binary) mid-run: the cached name is gone.
            std::fs::remove_file(&cached).unwrap();
            assert!(held.path().is_file(), "{tool}: the held binary is still there for the later steps");
            #[cfg(unix)]
            {
                let out = paddock_proc::command(held.path()).output().unwrap();
                assert_eq!(
                    String::from_utf8_lossy(&out.stdout).trim(),
                    "held",
                    "{tool}: the run's next start completes"
                );
            }
            let path = held.path().to_path_buf();
            drop(held);
            assert!(!path.exists(), "{tool}: the hold is removed when the run ends");
            assert!(home.path().join("held").is_dir());
        }
    }

    #[test]
    fn a_fresh_hold_of_a_binary_built_weeks_ago_survives_another_runs_sweep() {
        let home = tempfile::tempdir().unwrap();
        let env = env_for(home.path());
        let cached = fake_tool(home.path(), "throttle-abc", "throttle");
        // Built a month ago: a hard link shares the file's times, so the hold's file looks a month old too.
        let month = SystemTime::now() - Duration::from_secs(30 * 24 * 3600);
        std::fs::File::options()
            .write(true)
            .open(&cached)
            .unwrap()
            .set_modified(month)
            .unwrap();
        let first = hold("throttle", &cached, &env).unwrap();
        // Another run starts and holds the same binary; its sweep must not take the first run's hold away.
        let second = hold("throttle", &cached, &env).unwrap();
        assert!(
            first.path().is_file(),
            "a hold made a moment ago is not stale, whatever the binary's age"
        );
        drop(second);
        drop(first);
    }

    #[test]
    fn the_sweep_removes_only_holds_made_over_a_week_ago_and_names_it_did_not_make_stay() {
        let dir = tempfile::tempdir().unwrap();
        let now = SystemTime::now();
        let name = |age: Duration| {
            let t = (now - age).duration_since(UNIX_EPOCH).unwrap().as_nanos();
            exe(&format!("heavy-1-{t:x}"))
        };
        let old = name(Duration::from_secs(8 * 24 * 3600));
        let young = name(Duration::from_secs(6 * 24 * 3600));
        for n in [old.as_str(), young.as_str(), "notes.txt", "x-1-zz"] {
            std::fs::write(dir.path().join(n), "").unwrap();
        }
        sweep(dir.path(), now);
        assert!(!dir.path().join(&old).exists(), "a hold of a run that died a week ago goes");
        for n in [young.as_str(), "notes.txt", "x-1-zz"] {
            assert!(dir.path().join(n).exists(), "{n} stays");
        }
    }

    #[test]
    fn a_missing_cached_binary_cannot_be_held_and_says_why() {
        let home = tempfile::tempdir().unwrap();
        let why = hold("throttle", &home.path().join("bin/throttle-x/throttle"), &env_for(home.path()))
            .err()
            .unwrap();
        assert!(why.contains("cannot hold throttle") && why.contains("Next:"), "{why}");
    }

    #[cfg(unix)]
    #[test]
    fn a_program_outside_the_cache_holds_nothing_and_names_its_own_path() {
        // The test binary runs from cargo's target folder, never from a cache: no hold.
        let home = tempfile::tempdir().unwrap();
        assert_eq!(hold_self(&env_for(home.path())), None);
        assert!(!home.path().join("held").exists());
    }

    /// A paddock-dev outside the cache (`target/debug/paddock-dev`, a hook's copy, a cache entry) is started by its own path.
    #[test]
    fn a_paddock_dev_binary_is_started_again_by_its_own_path() {
        for exe in [
            "/w/target/debug/paddock-dev",
            "/w/.git/paddock-hooks/paddock-dev",
            "/h/.paddock-dev/bin/x/paddock-dev-host",
        ] {
            assert_eq!(self_exe_of(Path::new(exe)).unwrap(), PathBuf::from(exe), "{exe}");
        }
    }

    /// Every source file names this program through [`self_exe`], so no other place can start a test program as paddock-dev (the
    /// class of train 14's nested test runs). The files below read `current_exe()` for another reason, each named.
    #[test]
    fn no_source_file_reads_its_own_path_except_through_self_exe() {
        const OTHER_USES: &[(&str, &str)] = &[
            (
                "tool_cache.rs",
                "self_exe itself, and hold_self, which links the binary it runs from",
            ),
            ("cli.rs", "`--print-binary` prints the path, it starts nothing"),
            ("hook/install.rs", "copies the binary into the hook folder, it starts nothing"),
            (
                "test_sleeper.rs",
                "a test helper that starts its own test harness on purpose, with `--exact` and one test name",
            ),
            (
                "pre_push/exec.rs",
                "`harness_as_gate` compares a gate's program with the test binary to refuse it, it starts nothing",
            ),
            ("check/c120.rs", "C120's own samples and messages, text only"),
        ];
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let found = files_reading_own_path(&src);
        assert!(found.len() >= OTHER_USES.len(), "the scan found {found:?}: wrong folder?");
        let mut allowed: Vec<PathBuf> = OTHER_USES.iter().map(|(f, _)| PathBuf::from(f)).collect();
        allowed.sort();
        assert_eq!(
            found, allowed,
            "name this program with crate::tool_cache::self_exe() (a test program is never started as paddock-dev); a new other use \
             is added to OTHER_USES with its reason, and one that is gone is removed"
        );
    }

    /// The `.rs` files under `src` that read their own path (`current_exe()`), as paths relative to `src`, sorted.
    fn files_reading_own_path(src: &Path) -> Vec<PathBuf> {
        fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    rust_files(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let mut files = Vec::new();
        rust_files(src, &mut files);
        let needle = ["current", "_exe()"].concat();
        let mut found: Vec<PathBuf> = files
            .iter()
            .filter(|f| std::fs::read_to_string(f).unwrap().contains(&needle))
            .map(|f| f.strip_prefix(src).unwrap().to_path_buf())
            .collect();
        found.sort();
        found
    }

    /// Positive control of the scan: a sample tree with one use in a nested file and one file without.
    #[test]
    fn the_scan_finds_a_file_that_reads_its_own_path() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("a")).unwrap();
        std::fs::write(
            d.path().join("a/b.rs"),
            ["let me = std::env::current", "_exe().unwrap();\n"].concat(),
        )
        .unwrap();
        std::fs::write(d.path().join("c.rs"), "fn main() {}\n").unwrap();
        std::fs::write(d.path().join("d.txt"), ["current", "_exe()"].concat()).unwrap();
        assert_eq!(files_reading_own_path(d.path()), [Path::new("a").join("b.rs")]);
    }

    /// A test program is never "this program" to start again: started with `gates check` (the push gate's C100 row), the test
    /// harness takes the words as test-name filters and runs every matching test again, at full parallelism, inside the test that
    /// started it (train 14, 2026-11-18: `known_commit::a_delete_only_push...` 0.4 s on main, 14 to 17 s on the Mac and 56 s to a
    /// timeout on the Windows runner, runs 8245 and 8248, slowing every test beside it).
    #[test]
    fn a_test_program_is_never_started_again_as_this_program() {
        let e = self_exe().expect_err("this unit-test process is the test harness, not paddock-dev");
        assert!(e.to_string().contains("test program"), "{e}");
        // Both of cargo's test layouts.
        for exe in [
            // conformance: layout-guess — a sample of cargo's old test layout for the guard's own test, not a computed layout
            "/w/target/debug/deps/paddock_dev-0123456789abcdef",
            "/w/target/debug/build/paddock-dev/0123456789abcdef/out/paddock_dev-0123456789abcdef",
            "/w/target/debug/build/paddock-dev-0123/out/it-0123",
        ] {
            let e = self_exe_of(Path::new(exe)).expect_err(exe);
            assert!(e.to_string().contains(exe) && e.to_string().contains("next step"), "{e}");
        }
    }
}
