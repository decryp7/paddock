//! Where the agent keeps its files: one folder per user (`DYNO_DATA_DIR` overrides it, which is how tests stay out of
//! the real profile).

use std::path::{Path, PathBuf};

/// Makes the data folder `root` ready at the agent's start: created private (0700, whatever the umask), and, when an older Dyno made
/// it with the umask (0775 under umask 002) so only the account's own private group can write to it, that group write removed once.
/// A folder others can write to is not touched and is refused. Only the default data folder is ever tightened (`is_default`): a
/// folder the user named with `DYNO_DATA_DIR` is checked and refused as it is, never changed (ADR 0092 D2).
///
/// # Errors
/// The folder cannot be made, or it is open to others; the text ends with the next step.
pub fn prepare_root(root: &Path, is_default: bool) -> Result<(), String> {
    #[cfg(unix)]
    let others = |gid| paddock_env::group_has_other_members(gid, paddock_env::current_uid());
    #[cfg(not(unix))]
    let others = |_| true;
    prepare_root_with(root, is_default, others)
}

/// [`prepare_root`] with the group-membership lookup passed in (tests).
///
/// # Errors
/// See [`prepare_root`].
pub fn prepare_root_with(root: &Path, is_default: bool, group_has_others: impl FnOnce(u32) -> bool) -> Result<(), String> {
    #[cfg(unix)]
    if is_default && std::fs::symlink_metadata(root).is_ok() {
        if let Err(r) = paddock_env::tighten_group_write(root, group_has_others) {
            log::warn!(Why = r.reason(); "The data folder cannot be made private; run chmod go-w on it");
            return Err(format!(
                "the data folder is not safe to use ({}); run chmod go-w {}",
                r.reason(),
                root.display()
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = (is_default, group_has_others);
    open_private(root, "the data folder")
        .map(|_| ())
        .map_err(|e| format!("Dyno cannot use its data folder: {e}"))
}

/// The one way the agent opens or makes a folder that holds trusted files (ADR 0092 WP-6): an existing folder (a link to the
/// user's own folder is followed) is checked from its handle, a missing one is made owner-only from creation, and one that is open
/// to others, owned by another account or under a parent others can change is refused as it is: never tightened, because that
/// cannot evict what was planted while it was open. A refusal is logged with its reason (no path) and the error text names the
/// folder and the next step (ER-27). `what` names the folder in the log.
///
/// # Errors
/// [`paddock_env::private::PrivateError`]: refused, or an operation failed.
pub fn open_private(folder: &Path, what: &str) -> paddock_env::private::PrivateResult<paddock_env::private::PrivateDir> {
    use paddock_env::private::PrivateDir;
    let opened = if std::fs::symlink_metadata(folder).is_ok() {
        PrivateDir::open(folder, paddock_env::Links::Follow)
    } else {
        PrivateDir::create_all(folder)
    };
    if let Err(e) = &opened {
        log::warn!(Why = e.reason(); "Not using {what}: it is not safe or could not be made");
    }
    opened
}

/// [`open_private`] with the error as an `io::Error` for callers that return one (the text keeps the folder and the next step).
///
/// # Errors
/// `PermissionDenied` for a refusal, the OS error's kind otherwise.
pub fn open_private_io(folder: &Path, what: &str) -> std::io::Result<paddock_env::private::PrivateDir> {
    use paddock_env::private::PrivateError;
    open_private(folder, what).map_err(|e| match &e {
        PrivateError::Io { source, .. } => std::io::Error::new(source.kind(), e.to_string()),
        _ => std::io::Error::new(std::io::ErrorKind::PermissionDenied, e.to_string()),
    })
}

/// A per-run private folder that stands in for a data folder that was refused (ADR 0092, W8): everything the agent would read from or
/// save to its data folder goes here instead, so nothing is read from or written to the refused folder. It is made owner-only under
/// the temp folder with an unguessable name and removed when the agent ends.
#[derive(Debug)]
pub struct ScratchRoot(PathBuf);

impl ScratchRoot {
    /// The folder.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchRoot {
    fn drop(&mut self) {
        paddock_proc::cleanup::remove_dir_all(&self.0);
    }
}

/// Makes a [`ScratchRoot`] under `temp` (refused when `temp` is one others can change, like any private folder).
///
/// # Errors
/// The private folder could not be made; the text names the folder and the next step.
pub fn scratch_root_in(temp: &Path) -> Result<ScratchRoot, String> {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u32(std::process::id());
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    let path = temp.join(format!("Dyno-scratch-{:016x}", h.finish()));
    open_private(&path, "the scratch data folder")
        .map(|held| ScratchRoot(held.path().to_path_buf()))
        .map_err(|e| e.to_string())
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
pub(crate) mod tests {
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
        let tmp = tempfile::tempdir().unwrap();
        let d = Dirs::resolve(&no_env, tmp.path()).unwrap();
        assert_ne!(d.root(), tmp.path().join("Dyno"));
        assert!(d.root().starts_with(tmp.path()));
        assert_eq!(std::fs::metadata(d.root()).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(paddock_env::check_own_folder(d.root(), paddock_env::Links::Refuse), Ok(()));
    }

    #[cfg(unix)]
    #[test]
    fn a_planted_fallback_folder_is_refused_not_used() {
        let tmp = tempfile::tempdir().unwrap();
        let first = Dirs::resolve(&no_env, tmp.path()).unwrap();
        let name = first.root().file_name().unwrap().to_owned();
        std::fs::remove_dir(first.root()).unwrap();
        let attacker = tmp.path().join("attacker");
        std::fs::create_dir(&attacker).unwrap();
        std::os::unix::fs::symlink(&attacker, tmp.path().join(name)).unwrap();
        assert!(Dirs::resolve(&no_env, tmp.path()).is_err());
        assert_eq!(std::fs::read_dir(&attacker).unwrap().count(), 0, "nothing was written there");
    }

    #[cfg(unix)]
    #[test]
    fn a_data_folder_made_with_umask_002_still_lets_the_agent_write_and_read_agent_json() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("data");
        // What Dyno 1.1.0 left behind under umask 002: owned by the user, group write set, the user's own group.
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o775)).unwrap();
        let file = root.join("agent.json");
        let mine = crate::discovery::Discovery {
            endpoint: "e".into(),
            token: paddock_ipc::Token::from_bytes([1; 32]).to_hex(),
            pid: std::process::id(),
            started: 1,
            version: "0.1.0".into(),
            protocol: 1,
            build: String::new(),
        };
        prepare_root_with(&root, true, |_| false).unwrap();
        mine.write(&file).unwrap();
        assert_eq!(crate::discovery::Discovery::read(&file), Some(mine));
        assert_eq!(
            std::fs::metadata(&root).unwrap().permissions().mode() & 0o022,
            0,
            "no group or world write is left"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_data_folder_others_can_write_to_is_refused_untouched_with_the_chmod_next_step() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("data");
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
        let e = prepare_root_with(&root, true, |_| false).unwrap_err();
        assert!(e.contains("chmod go-w"), "{e}");
        assert_eq!(std::fs::metadata(&root).unwrap().permissions().mode() & 0o777, 0o777);
        // A shared group is refused too.
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o775)).unwrap();
        assert!(prepare_root_with(&root, true, |_| true).is_err());
    }

    /// A folder others can write to and replace children of (0777, no sticky bit) inside a private temp folder, and the temp folder.
    #[cfg(unix)]
    pub(crate) fn open_parent() -> (tempfile::TempDir, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let open = tmp.path().join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
        (tmp, open)
    }

    #[cfg(unix)]
    #[test]
    fn a_data_folder_under_a_parent_others_can_change_is_refused() {
        let (_tmp, open) = open_parent();
        let root = open.join("data");
        let e = prepare_root_with(&root, true, |_| false).unwrap_err();
        assert!(e.contains("chmod go-w"), "the refusal names the next step: {e}");
    }

    #[cfg(unix)]
    #[test]
    fn open_private_makes_a_missing_folder_owner_only_follows_a_link_to_an_own_folder_and_refuses_an_open_one() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let made = tmp.path().join("a").join("b");
        open_private(&made, "test").unwrap();
        assert_eq!(std::fs::metadata(&made).unwrap().permissions().mode() & 0o777, 0o700);
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&made, &link).unwrap();
        open_private(&link, "test").unwrap();
        let open = tmp.path().join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
        let e = open_private_io(&open, "test").unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(e.to_string().contains("chmod go-w"), "{e}");
        assert_eq!(
            std::fs::metadata(&open).unwrap().permissions().mode() & 0o777,
            0o777,
            "never tightened"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_new_data_folder_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("a").join("data");
        prepare_root_with(&root, true, |_| false).unwrap();
        assert_eq!(std::fs::metadata(&root).unwrap().permissions().mode() & 0o777, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn a_world_writable_temp_folder_without_the_sticky_bit_holds_no_fallback() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        let e = Dirs::resolve(&no_env, tmp.path()).unwrap_err();
        assert!(e.to_string().contains("use folder"), "{e}");
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0, "nothing was made there");
    }

    #[test]
    fn the_refusal_gives_the_same_next_step_for_the_agent_and_the_cli() {
        assert!(NEXT_STEP.contains("HOME") && NEXT_STEP.contains("DYNO_DATA_DIR"));
        assert_eq!(why(&paddock_env::EnvError::NoHome("x")), "no home folder is known");
    }

    #[test]
    fn a_known_home_wins_over_the_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let env = |k: &str| (k == "DYNO_DATA_DIR").then(|| home.clone());
        assert_eq!(Dirs::resolve(&env, tmp.path()).unwrap().root(), home);
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
    /// Windows: a folder in a private temp tree that Everyone may write to (an entry that new items inherit). Owed: runs on the
    /// Windows runner or VM.
    #[cfg(windows)]
    #[expect(
        clippy::disallowed_methods,
        reason = "a Windows-only test runs icacls on its own temporary folder to set and read its access list"
    )]
    pub(crate) fn everyone_folder() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let open = tmp.path().join("open");
        std::fs::create_dir(&open).unwrap();
        let out = std::process::Command::new("icacls")
            .arg(&open)
            .args(["/grant", "*S-1-1-0:(OI)(CI)F"])
            .output()
            .unwrap();
        assert!(out.status.success(), "icacls: {}", String::from_utf8_lossy(&out.stdout));
        (tmp, open)
    }

    #[cfg(windows)]
    #[test]
    fn windows_a_data_folder_everyone_can_write_is_refused_with_a_next_step_and_a_new_one_is_made() {
        let (tmp, open) = everyone_folder();
        let e = prepare_root_with(&open, true, |_| false).unwrap_err();
        assert!(e.contains("Security tab") || e.contains("entries"), "{e}");
        let e = open_private_io(&open, "test").unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
        // A folder made under an Everyone-writable parent is refused (the parent rule), and a fresh one elsewhere is made.
        assert!(open_private(&open.join("data"), "test").is_err());
        prepare_root_with(&tmp.path().join("own").join("data"), true, |_| false).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn a_folder_the_user_named_is_never_tightened_only_the_default_one_is() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("data");
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o775)).unwrap();
        // DYNO_DATA_DIR: checked and refused as it is, even though it is the account's own private group.
        let e = prepare_root_with(&root, false, |_| false).unwrap_err();
        assert!(e.contains("chmod go-w"), "{e}");
        assert_eq!(
            std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o775,
            "an override is left as it was"
        );
        // The default folder gets the one-time group-write removal.
        prepare_root_with(&root, true, |_| false).unwrap();
        assert_eq!(std::fs::metadata(&root).unwrap().permissions().mode() & 0o777, 0o755);
    }

    #[cfg(unix)]
    #[test]
    fn the_scratch_root_is_private_unguessable_and_removed_with_its_guard_and_refused_under_an_open_temp() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let a = scratch_root_in(tmp.path()).unwrap();
        let b = scratch_root_in(tmp.path()).unwrap();
        assert_ne!(a.path(), b.path());
        assert_eq!(std::fs::metadata(a.path()).unwrap().permissions().mode() & 0o777, 0o700);
        let gone = a.path().to_path_buf();
        drop(a);
        assert!(!gone.exists());
        let (_t, open) = open_parent();
        assert!(scratch_root_in(&open).is_err());
    }
}
