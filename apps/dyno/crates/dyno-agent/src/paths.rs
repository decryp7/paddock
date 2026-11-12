//! Where the agent keeps its files: one folder per user (`DYNO_DATA_DIR` overrides it, which is how tests stay out of
//! the real profile).

use std::path::{Path, PathBuf};

/// The one check of the data folder before anything is opened or written in it (ADR 0092, revised 2026-11-12): the agent's `run`,
/// `Running::start` and the app call it first, so the log, the instance lock, the crash and run markers and the database wait for it.
///
/// The default per-user folder is created owner-only (0700 on Unix, whatever the umask) when it is missing and otherwise used as it is:
/// the OS keeps other accounts out of the user's profile, and Chrome, VS Code and Electron apps make the same assumption (XDG: an
/// existing folder's permissions are not changed). A folder the user supplied (`DYNO_DATA_DIR`, a test's own folder) is where other
/// accounts can plausibly write (`C:\Dyno`, `/tmp`, a shared volume), so it is made owner-only if missing and refused when other
/// accounts can change it, like ssh's StrictModes and git's `safe.directory`. A refusal means Dyno does not start: with the cause and
/// the next step. There is no memory-only mode, which would lose the settings and Wake-on-LAN passwords without a word.
///
/// # Errors
/// The text to show: the cause, the next step, and that nothing was started or changed.
pub fn check_root(dirs: &Dirs) -> Result<(), String> {
    check_places(
        dirs.root(),
        !dirs.is_users_own(),
        &paddock_env::AppId::new(crate::logging::APP_ID, crate::logging::APP_NAME)
            .and_then(|id| paddock_env::AppDirs::for_app(&id))
            .ok(),
    )
}

/// Every place the agent or the app writes into at start: the data folder and, when the user supplied the app's folders (portable mode,
/// `<ID>_HOME`), their `data` and `logs` (the log, the crash marker and the run marker go there). All the checks run first and nothing
/// is made before they all pass, so a refused `<ID>_HOME` leaves a named `DYNO_DATA_DIR` uncreated too.
///
/// # Errors
/// As [`check_root`].
fn check_places(root: &Path, supplied: bool, app_dirs: &Option<paddock_env::AppDirs>) -> Result<(), String> {
    let app_folders: Vec<&Path> = app_dirs
        .as_ref()
        .filter(|a| a.source() != paddock_env::Source::Os)
        .map(|a| vec![a.data(), a.logs()])
        .unwrap_or_default();
    let named = supplied.then_some(root).into_iter().chain(app_folders.iter().copied());
    for folder in named {
        paddock_env::private::check_named_folder(folder).map_err(|e| refusal_text(&e.to_string()))?;
    }
    check_root_with(root, supplied)?;
    for folder in app_folders {
        check_root_with(folder, true)?;
    }
    Ok(())
}

/// [`check_root`] with the "supplied by the user" decision passed in (tests).
///
/// # Errors
/// See [`check_root`].
pub fn check_root_with(root: &Path, supplied: bool) -> Result<(), String> {
    let failed = |why: String| {
        log::error!(
            "The data folder is not usable ({}); Dyno does not start",
            if supplied {
                "a folder the user named"
            } else {
                "the default folder"
            }
        );
        refusal_text(&why)
    };
    if supplied {
        // Checked first, from what exists, so a refused folder (and a refused parent) is left as it is: nothing is made inside it.
        paddock_env::private::check_named_folder(root).map_err(|e| failed(e.to_string()))?;
        paddock_env::private::PrivateDir::create_all(root)
            .map(|_| ())
            .map_err(|e| failed(e.to_string()))
    } else {
        paddock_env::create_private_dir_all(root).map_err(|e| failed(format!("{} could not be made ({e})", root.display())))
    }
}

fn refusal_text(why: &str) -> String {
    format!("Dyno did not start: {why}. Nothing was started or changed. Fix the folder, or start Dyno with DYNO_DATA_DIR set to a folder of your own.")
}

/// What to do when no data folder can be used (the same words for the agent and the CLI).
pub const NEXT_STEP: &str = "Set HOME or DYNO_DATA_DIR to a folder you own";

/// The reason an error gives, without the path (ER-27: no user paths in logs beyond what is already shown).
pub fn why(e: &paddock_env::EnvError) -> &'static str {
    match e {
        paddock_env::EnvError::NoHome(_) => "no home folder is known",
        paddock_env::EnvError::Io { op, .. } if *op == "use folder" => "the fallback folder is not safe to use",
        paddock_env::EnvError::Io { .. } => "the fallback folder could not be made",
        _ => "the data folder could not be chosen",
    }
}

/// The agent's files.
#[derive(Debug, Clone)]
pub struct Dirs {
    root: PathBuf,
    /// Where the dated logs are when that is not `<root>/logs` (the app's log folder in an install).
    log_folder: Option<PathBuf>,
    /// The OS cache folder for rebuildable files, when it is not `<root>/cache` (only the current user's real folders).
    cache_folder: Option<PathBuf>,
}

impl Dirs {
    /// The folder for the current user: `%LOCALAPPDATA%\Dyno`, `~/Library/Application Support/Dyno` or
    /// `$XDG_DATA_HOME/dyno` (`~/.local/share/dyno`); `DYNO_DATA_DIR` wins. When no home folder is known the folder is a private
    /// per-user one (see [`paddock_env::private_fallback_dir`]); a shared fixed path is never used.
    ///
    /// A program with no usable folder cannot work, so this stops it with a message that says what to fix (exit code 78).
    /// Use [`Dirs::try_current`] to handle the error instead.
    pub fn current() -> Dirs {
        Dirs::try_current().unwrap_or_else(|e| {
            eprintln!("dyno: {e}. {NEXT_STEP}");
            log::error!(Why = why(&e); "No usable data folder; stopping");
            std::process::exit(78)
        })
    }

    /// [`Dirs::current`] with the failure returned.
    ///
    /// # Errors
    /// No per-user folder is known and the private fallback cannot be made or is not safe to use.
    #[expect(
        clippy::disallowed_methods,
        reason = "the temp folder is only the base `paddock_env::private_fallback_dir` makes a private per-user folder under; a shared fixed path is never used"
    )]
    pub fn try_current() -> Result<Dirs, paddock_env::EnvError> {
        Dirs::resolve(
            &|k| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from),
            &std::env::temp_dir(),
        )
    }

    /// The same choice from an injected environment and temp folder (tests).
    ///
    /// # Errors
    /// See [`Dirs::try_current`].
    pub fn resolve(env: &dyn Fn(&str) -> Option<PathBuf>, temp: &Path) -> Result<Dirs, paddock_env::EnvError> {
        let override_root = env("DYNO_DATA_DIR");
        let cache_folder = if override_root.is_some() {
            None
        } else if cfg!(windows) {
            env("LOCALAPPDATA").map(|d| d.join("Dyno").join("Cache"))
        } else if cfg!(target_os = "macos") {
            env("HOME").map(|h| h.join("Library").join("Caches").join("Dyno"))
        } else {
            env("XDG_CACHE_HOME")
                .map(|d| d.join("dyno"))
                .or_else(|| env("HOME").map(|h| h.join(".cache").join("dyno")))
        };
        let root = match override_root {
            Some(r) => r,
            None => {
                let found = if cfg!(windows) {
                    env("LOCALAPPDATA").map(|d| d.join("Dyno"))
                } else if cfg!(target_os = "macos") {
                    env("HOME").map(|h| h.join("Library").join("Application Support").join("Dyno"))
                } else {
                    env("XDG_DATA_HOME")
                        .map(|d| d.join("dyno"))
                        .or_else(|| env("HOME").map(|h| h.join(".local").join("share").join("dyno")))
                };
                match found {
                    Some(r) => r,
                    None => {
                        let r = paddock_env::private_fallback_dir(temp, "Dyno");
                        match &r {
                            Ok(_) => log::warn!("No home folder is known; using the private per-user fallback data folder"),
                            Err(e) => {
                                log::error!(Why = e.to_string().as_str(); "No home folder is known and the fallback data folder is not safe to use")
                            }
                        }
                        r?
                    }
                }
            }
        };
        Ok(Dirs {
            root,
            log_folder: None,
            cache_folder,
        })
    }

    /// A folder chosen by the caller.
    pub fn at(root: impl Into<PathBuf>) -> Dirs {
        Dirs {
            root: root.into(),
            log_folder: None,
            cache_folder: None,
        }
    }

    /// The data folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where the macOS agent keeps Wake-on-LAN passwords: owner-only files, never the Keychain (see `dyno_platform::wol::os_store`).
    pub fn secrets_folder(&self) -> PathBuf {
        self.root.join("secrets")
    }

    /// The history database.
    pub fn database(&self) -> PathBuf {
        self.root.join("dyno.db")
    }

    /// The folder for rebuildable files: the OS cache folder (`~/Library/Caches/Dyno`, `$XDG_CACHE_HOME/dyno` or
    /// `~/.cache/dyno`, `%LOCALAPPDATA%\Dyno\Cache`), kept out of backups; `<data folder>/cache` when `DYNO_DATA_DIR` or
    /// [`Dirs::at`] names the data folder (tests, ER-14).
    pub fn cache_folder(&self) -> PathBuf {
        self.cache_folder.clone().unwrap_or_else(|| self.root.join("cache"))
    }

    /// The saved-views cache database (ADR 0029): rebuildable, so never in the data folder.
    pub fn cache_db(&self) -> PathBuf {
        self.cache_folder().join("cache.db")
    }

    /// Where the fetched text of one list subscription (or one geo database) is kept: `<cache folder>/intel/<id>`. The id is
    /// one of Dyno's own slugs (letters, digits and `-`), never user text; anything else is replaced.
    pub fn intel_cache(&self, id: &str) -> PathBuf {
        let slug: String = id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
            .take(64)
            .collect();
        self.cache_folder().join("intel").join(slug)
    }

    /// The user's own lists, subscriptions and allow entries (user data, so in the data folder, not in the cache).
    pub fn intel_lists(&self) -> PathBuf {
        self.root.join("intel-lists.json")
    }

    /// When each subscription and geo source was last tried: kept outside the cache, so clearing the cache cannot cause an early
    /// fetch.
    pub fn intel_fetch_state(&self) -> PathBuf {
        self.root.join("intel-fetch.json")
    }

    /// Where Dyno keeps tools it extracted itself (`tools/smartmontools-<version>/`): its own folder, no administrator rights.
    pub fn tools(&self) -> PathBuf {
        self.root.join("tools")
    }

    /// Where a tool download is staged before the helper gets it (rebuildable, so never user data); the job empties it.
    pub fn tool_staging(&self) -> PathBuf {
        self.cache_folder().join("tools")
    }

    /// The user's settings.
    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// The benchmark run history.
    pub fn benchmarks(&self) -> PathBuf {
        self.root.join("benchmarks.json")
    }

    /// The discovery file the app reads to find a running agent.
    pub fn discovery(&self) -> PathBuf {
        self.root.join("agent.json")
    }

    /// The lock only one agent per data folder can hold ([`crate::instance`]).
    pub fn instance_lock(&self) -> PathBuf {
        self.root.join("agent.lock")
    }

    /// The same folders with the dated logs read from `folder` (set once by `run` from the install's log places).
    pub fn with_log_folder(mut self, folder: PathBuf) -> Dirs {
        self.log_folder = Some(folder);
        self
    }

    /// The folder the agent's and the app's dated logs are in: the one `run` named, else this folder's own [`Dirs::logs`], so a
    /// harness on a temporary folder never reads the machine's real logs.
    pub fn log_folder(&self) -> PathBuf {
        self.log_folder.clone().unwrap_or_else(|| self.logs())
    }

    /// The agent's log folder.
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Where plugins live (`<id>/plugin.toml`).
    pub fn plugins(&self) -> PathBuf {
        self.root.join("plugins")
    }

    /// Whether this is the user's own data folder: the one this process would use, not moved by `DYNO_DATA_DIR`. Only an agent
    /// on it uses this computer's helper and runs the background checks; a test's or a development agent's folder is not it (ER-14).
    pub fn is_users_own(&self) -> bool {
        let overridden = std::env::var_os("DYNO_DATA_DIR").is_some_and(|v| !v.is_empty());
        users_own(&self.root, overridden, Dirs::current().root())
    }

    /// Which kind of folder this is, for the logs (never the user's path): every launch path of an installed agent
    /// (the app's child, the login item, the Windows `Run` entry) must log "user data folder".
    #[expect(
        clippy::disallowed_methods,
        reason = "compared with the data folder to tell the logs whether it is the last-resort scratch folder"
    )]
    pub fn kind(&self) -> &'static str {
        let overridden = std::env::var_os("DYNO_DATA_DIR").is_some_and(|v| !v.is_empty());
        // A label for the logs must never end the process: a folder that cannot be chosen is none of the known kinds.
        let current = Dirs::try_current().ok();
        folder_kind(
            &self.root,
            overridden,
            current.as_ref().map_or(Path::new(""), Dirs::root),
            &std::env::temp_dir(),
        )
    }
}

/// [`Dirs::is_users_own`] with the environment passed in.
fn users_own(root: &Path, overridden: bool, current: &Path) -> bool {
    root == current && !overridden
}

/// [`Dirs::kind`] with the environment passed in.
fn folder_kind(root: &Path, overridden: bool, current: &Path, temp: &Path) -> &'static str {
    if root != current {
        "caller's folder"
    } else if overridden {
        "DYNO_DATA_DIR"
    } else if root.starts_with(temp) {
        "temp folder (no LOCALAPPDATA or HOME)"
    } else {
        "user data folder"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_live_under_the_root() {
        let d = Dirs::at("/x/dyno");
        assert_eq!(d.database(), Path::new("/x/dyno/dyno.db"));
        assert_eq!(d.discovery(), Path::new("/x/dyno/agent.json"));
        assert_eq!(d.root(), Path::new("/x/dyno"));
        for p in [d.settings(), d.logs(), d.plugins(), d.instance_lock()] {
            assert!(p.starts_with("/x/dyno"));
        }
    }

    #[cfg(unix)]
    fn no_env(_: &str) -> Option<PathBuf> {
        None
    }

    #[cfg(unix)]
    #[test]
    fn with_no_home_the_data_folder_is_a_private_per_user_folder_never_the_shared_fixed_path() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = paddock_testkit::Scratch::new(); // folders in the caller's own group, whatever TMPDIR's disk gives
        let tmp = scratch.work();
        let d = Dirs::resolve(&no_env, &tmp).unwrap();
        assert_ne!(d.root(), tmp.join("Dyno"));
        assert!(d.root().starts_with(&tmp));
        assert_eq!(std::fs::metadata(d.root()).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(paddock_env::check_own_folder(d.root(), paddock_env::Links::Refuse), Ok(()));
    }

    #[cfg(unix)]
    #[test]
    fn a_planted_fallback_folder_is_refused_not_used() {
        let scratch = paddock_testkit::Scratch::new(); // folders in the caller's own group, whatever TMPDIR's disk gives
        let tmp = scratch.work();
        let first = Dirs::resolve(&no_env, &tmp).unwrap();
        let name = first.root().file_name().unwrap().to_owned();
        std::fs::remove_dir(first.root()).unwrap();
        let attacker = tmp.join("attacker");
        std::fs::create_dir(&attacker).unwrap();
        std::os::unix::fs::symlink(&attacker, tmp.join(name)).unwrap();
        assert!(Dirs::resolve(&no_env, &tmp).is_err());
        assert_eq!(std::fs::read_dir(&attacker).unwrap().count(), 0, "nothing was written there");
    }

    #[cfg(unix)]
    #[test]
    fn the_default_folder_is_used_as_it_is_even_when_others_can_write_it_and_a_missing_one_is_made_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = paddock_testkit::Scratch::new(); // folders in the caller's own group, whatever TMPDIR's disk gives
        let tmp = scratch.work();
        let root = tmp.join("data");
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
        check_root_with(&root, false).unwrap();
        assert_eq!(
            std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o777,
            "an existing default folder is not changed"
        );
        let made = tmp.join("a").join("new");
        check_root_with(&made, false).unwrap();
        assert_eq!(std::fs::metadata(&made).unwrap().permissions().mode() & 0o777, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_the_user_named_is_refused_when_others_can_write_it_and_left_as_it_was() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("data");
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
        let e = check_root_with(&root, true).unwrap_err();
        assert!(
            e.contains("did not start") && e.contains("chmod go-w") && e.contains("DYNO_DATA_DIR"),
            "cause and next step: {e}"
        );
        assert_eq!(
            std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o777,
            "never tightened"
        );
        // A missing one is made owner-only; one under a parent others can change is refused.
        let made = tmp.path().join("new");
        check_root_with(&made, true).unwrap();
        assert_eq!(std::fs::metadata(&made).unwrap().permissions().mode() & 0o777, 0o700);
        assert!(check_root_with(&root.join("inner"), true).is_err());
    }

    /// Windows: a folder the user named under a place Everyone may write (the root of `C:\` is one) is refused; the default is not.
    /// Owed to the Windows runner or VM, with the manual check of an Administrators-owned `%LOCALAPPDATA%\Dyno` (used, not refused).
    #[cfg(windows)]
    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "a Windows-only test runs icacls on its own temporary folder to set its access list"
    )]
    fn windows_a_named_folder_everyone_can_write_is_refused_and_the_default_is_used() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("data");
        std::fs::create_dir(&root).unwrap();
        let out = std::process::Command::new("icacls")
            .arg(&root)
            .args(["/grant", "*S-1-1-0:(OI)(CI)F"])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(check_root_with(&root, true).unwrap_err().contains("did not start"));
        check_root_with(&root, false).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_app_folders_the_user_supplied_are_refused_like_a_named_data_folder_and_the_os_own_are_not() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("home");
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
        let id = paddock_env::AppId::new("dyno", "Dyno").unwrap();
        let vars = paddock_env::MapVars::new().with("DYNO_HOME", root.clone().into_os_string());
        let supplied = paddock_env::AppDirs::resolve(&id, paddock_env::Os::current(), &vars, None, true).ok();
        let e = check_places(&tmp.path().join("none"), false, &supplied).unwrap_err();
        assert!(
            e.contains("did not start") && e.contains("chmod go-w"),
            "logs, crash marker and run marker: {e}"
        );
        let os_own = paddock_env::AppDirs::resolve(
            &id,
            paddock_env::Os::current(),
            &paddock_env::MapVars::new().with("HOME", root.into_os_string()),
            None,
            false,
        )
        .ok();
        assert_eq!(
            check_places(&tmp.path().join("default"), false, &os_own),
            Ok(()),
            "the OS's own places are not checked"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_refused_named_folder_has_nothing_made_inside_it() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir(&home).unwrap();
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o777)).unwrap();
        // The folder itself is missing under one others can write: refused, and not made.
        assert!(check_root_with(&home.join("data"), true).is_err());
        assert!(!home.join("data").exists());
        // `<ID>_HOME` open to others: neither `data` nor `logs` is created before the refusal.
        let id = paddock_env::AppId::new("dyno", "Dyno").unwrap();
        let vars = paddock_env::MapVars::new().with("DYNO_HOME", home.clone().into_os_string());
        let supplied = paddock_env::AppDirs::resolve(&id, paddock_env::Os::current(), &vars, None, true).ok();
        assert!(check_places(&tmp.path().join("none"), false, &supplied).is_err());
        assert_eq!(
            std::fs::read_dir(&home).unwrap().count(),
            0,
            "nothing was made inside the refused folder"
        );
    }

    /// P2: a named data folder is not made when a supplied `<ID>_HOME` is refused: all the checks run before anything is created.
    #[cfg(unix)]
    #[test]
    fn a_named_data_folder_is_not_made_when_a_supplied_app_home_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir(&home).unwrap();
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o777)).unwrap();
        let id = paddock_env::AppId::new("dyno", "Dyno").unwrap();
        let vars = paddock_env::MapVars::new().with("DYNO_HOME", home.into_os_string());
        let supplied = paddock_env::AppDirs::resolve(&id, paddock_env::Os::current(), &vars, None, true).ok();
        let named = tmp.path().join("named");
        assert!(check_places(&named, true, &supplied).is_err());
        assert!(!named.exists(), "the named data folder was not made before the refusal");
    }

    #[cfg(unix)]
    #[test]
    fn a_world_writable_temp_folder_without_the_sticky_bit_holds_no_fallback() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = paddock_testkit::Scratch::new(); // folders in the caller's own group, whatever TMPDIR's disk gives
        let tmp = scratch.work();
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o777)).unwrap();
        let e = Dirs::resolve(&no_env, &tmp).unwrap_err();
        assert!(e.to_string().contains("use folder"), "{e}");
        assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0, "nothing was made there");
    }

    #[test]
    fn the_refusal_gives_the_same_next_step_for_the_agent_and_the_cli() {
        assert!(NEXT_STEP.contains("HOME") && NEXT_STEP.contains("DYNO_DATA_DIR"));
        assert_eq!(why(&paddock_env::EnvError::NoHome("x")), "no home folder is known");
    }

    #[test]
    fn a_known_home_wins_over_the_fallback() {
        let scratch = paddock_testkit::Scratch::new(); // folders in the caller's own group, whatever TMPDIR's disk gives
        let tmp = scratch.work();
        let home = tmp.join("home");
        let env = |k: &str| (k == "DYNO_DATA_DIR").then(|| home.clone());
        assert_eq!(Dirs::resolve(&env, &tmp).unwrap().root(), home);
    }

    #[test]
    fn only_the_folder_no_one_moved_is_the_users_own() {
        let cur = Path::new("/u/dyno");
        assert!(users_own(cur, false, cur));
        // Moved by DYNO_DATA_DIR (tests, development agents): this process's folder, but not the user's (ER-14).
        assert!(!users_own(cur, true, cur));
        assert!(!users_own(Path::new("/x"), false, cur));
    }

    #[test]
    fn the_folder_kind_names_no_path() {
        let (cur, tmp) = (Path::new("/u/dyno"), Path::new("/tmp"));
        assert_eq!(folder_kind(cur, false, cur, tmp), "user data folder");
        assert_eq!(folder_kind(cur, true, cur, tmp), "DYNO_DATA_DIR");
        assert_eq!(folder_kind(Path::new("/x"), false, cur, tmp), "caller's folder");
        assert_eq!(
            folder_kind(Path::new("/tmp/Dyno"), false, Path::new("/tmp/Dyno"), tmp),
            "temp folder (no LOCALAPPDATA or HOME)"
        );
        assert!(!Dirs::at("/x").kind().is_empty());
    }

    #[test]
    fn the_cache_database_is_beside_the_data_when_the_data_folder_is_named_and_in_the_cache_folder_otherwise() {
        assert_eq!(Dirs::at("/x/dyno").cache_db(), Path::new("/x/dyno/cache/cache.db"));
        let mut d = Dirs::at("/x/dyno");
        d.cache_folder = Some(PathBuf::from("/c/Dyno"));
        assert_eq!(d.cache_db(), Path::new("/c/Dyno/cache.db"));
        assert!(!d.cache_db().starts_with(d.root()), "never inside the data folder");
    }

    #[test]
    fn rebuildable_intel_files_are_beside_the_data_in_tests_and_in_the_cache_folder_otherwise() {
        let d = Dirs::at("/x/dyno");
        assert_eq!(d.intel_cache("list-1"), Path::new("/x/dyno/cache/intel/list-1"));
        assert_eq!(
            d.intel_cache("../etc"),
            Path::new("/x/dyno/cache/intel/___etc"),
            "an id never leaves the folder"
        );
        let mut d = Dirs::at("/x/dyno");
        d.cache_folder = Some(PathBuf::from("/c/Dyno"));
        assert_eq!(d.intel_cache("a"), Path::new("/c/Dyno/intel/a"));
        assert!(!d.intel_cache("a").starts_with(d.root()), "never inside the data folder");
        assert!(
            d.intel_fetch_state().starts_with(d.root()),
            "the last fetch time is not in the cache"
        );
        assert!(d.intel_lists().starts_with(d.root()));
    }

    #[test]
    fn the_current_user_gets_a_named_folder() {
        let d = Dirs::current();
        assert!(d.root().to_string_lossy().to_lowercase().contains("dyno"));
    }
}
