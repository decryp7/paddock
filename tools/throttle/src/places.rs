//! Where throttle keeps its lock folder, and where an outside caller can say otherwise (user decision 2026-11-26: throttle decides
//! its own folder, so no caller has to pass one).
//!
//! Precedence: (1) the environment, `THROTTLE_DIR` then the aliases `PLOCK_DIR` and `PLOCK_ROOT` (and `HEAVY_DIR`, [`crate::config::aliased`]);
//! (2) the `dir` key of the user's `config.toml`; (3) the per-user STATE folder of the platform. A process with no `HOME` (a launchd daemon
//! with `UserName`, a service) finds the home through the user database. The core here is pure: the machine is the [`Host`].

use paddock_env::Os;
use std::path::{Path, PathBuf};

/// The folder the shell plock used on the agents' Mac: an old lock folder whose history is copied over once.
pub const LEGACY_ROOT: &str = "/private/tmp/claude-501";

/// The user's settings file, in [`config_file`]'s folder.
pub const CONFIG_FILE: &str = "config.toml";

/// What the resolver needs from the machine: the OS, the user database, files and the dev-targets folder. Tests build their own.
pub struct Host<'a> {
    /// The platform whose conventions apply.
    pub os: Os,
    /// The home folder from the user database (asked only when the environment gives none).
    pub user_home: &'a dyn Fn() -> Option<PathBuf>,
    /// A small text file's contents, if it can be read.
    pub read_file: &'a dyn Fn(&Path) -> Option<String>,
    /// Whether the lock folder holds a run-time history file ([`crate::history::FILE`]).
    pub has_history: &'a dyn Fn(&Path) -> bool,
    /// The dev-targets folder (its `throttle` folder is an old lock folder), when the program derived one.
    pub dev_targets: Option<PathBuf>,
}

impl Host<'static> {
    /// A machine with no home, no files and no old folders: what a test starts from.
    pub fn none() -> Host<'static> {
        Host {
            os: Os::Linux,
            user_home: &|| None,
            read_file: &|_| None,
            has_history: &|_| false,
            dev_targets: None,
        }
    }

    /// The real machine.
    pub fn real(dev_targets: Option<PathBuf>) -> Host<'static> {
        Host {
            os: Os::current(),
            user_home: &crate::userdb::home_of_current_user,
            read_file: &|p| std::fs::read_to_string(p).ok().filter(|t| t.len() <= 1 << 16),
            has_history: &|d| d.join(crate::history::FILE).is_file(),
            dev_targets,
        }
    }
}

fn looks_absolute(os: Os, p: &str) -> bool {
    if os == Os::Windows {
        let b = p.as_bytes();
        b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
    } else {
        p.starts_with('/')
    }
}

fn absolute(get: &dyn Fn(&str) -> Option<String>, os: Os, key: &str) -> Option<PathBuf> {
    get(key).filter(|v| looks_absolute(os, v)).map(PathBuf::from)
}

/// The home folder: `HOME` (`USERPROFILE` on Windows) when it is absolute, else the user database's.
pub fn home(get: &dyn Fn(&str) -> Option<String>, host: &Host<'_>) -> Option<PathBuf> {
    let key = if host.os == Os::Windows { "USERPROFILE" } else { "HOME" };
    absolute(get, host.os, key).or_else(|| (host.user_home)())
}

/// The per-user state folder: macOS `~/Library/Application Support/throttle`, Linux `$XDG_STATE_HOME/throttle` else `~/.local/state/throttle`,
/// Windows `%LOCALAPPDATA%\throttle`. `None` when no home can be found.
pub fn state_dir(get: &dyn Fn(&str) -> Option<String>, host: &Host<'_>) -> Option<PathBuf> {
    let base = match host.os {
        Os::MacOs => home(get, host)?.join("Library").join("Application Support"),
        Os::Linux => absolute(get, host.os, "XDG_STATE_HOME").or_else(|| home(get, host).map(|h| h.join(".local").join("state")))?,
        Os::Windows => absolute(get, host.os, "LOCALAPPDATA").or_else(|| home(get, host).map(|h| h.join("AppData").join("Local")))?,
    };
    Some(base.join("throttle"))
}

/// The user's settings file: macOS `~/Library/Application Support/throttle/config.toml`, Linux `$XDG_CONFIG_HOME/throttle/config.toml` else
/// `~/.config/throttle/config.toml`, Windows `%APPDATA%\throttle\config.toml`.
pub fn config_file(get: &dyn Fn(&str) -> Option<String>, host: &Host<'_>) -> Option<PathBuf> {
    let base = match host.os {
        Os::MacOs => home(get, host)?.join("Library").join("Application Support"),
        Os::Linux => absolute(get, host.os, "XDG_CONFIG_HOME").or_else(|| home(get, host).map(|h| h.join(".config")))?,
        Os::Windows => absolute(get, host.os, "APPDATA").or_else(|| home(get, host).map(|h| h.join("AppData").join("Roaming")))?,
    };
    Some(base.join("throttle").join(CONFIG_FILE))
}

/// The `dir` key of a `config.toml`: the one key throttle reads, a top-level string (`dir = "/x"` or `dir = '/x'`); comments, blank lines
/// and other keys are skipped, and the first table header ends the top level. `Err` says what is wrong with the value.
pub fn dir_from_config(text: &str, os: Os) -> Result<Option<PathBuf>, String> {
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            break;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        if key.trim() != "dir" {
            continue;
        }
        let v = value.trim();
        let parsed = if let Some(rest) = v.strip_prefix('\'') {
            rest.split_once('\'').map(|(s, _)| s.to_string())
        } else if let Some(rest) = v.strip_prefix('"') {
            let mut out = String::new();
            let mut chars = rest.chars();
            let mut closed = false;
            while let Some(c) = chars.next() {
                match c {
                    '"' => {
                        closed = true;
                        break;
                    }
                    '\\' => match chars.next() {
                        Some('\\') => out.push('\\'),
                        Some('"') => out.push('"'),
                        _ => {
                            return Err(format!(
                                "dir = {v}: only \\\\ and \\\" are escapes (use a 'literal string' for a Windows path)"
                            ));
                        }
                    },
                    c => out.push(c),
                }
            }
            closed.then_some(out)
        } else {
            None
        };
        let Some(dir) = parsed.filter(|d| !d.is_empty()) else {
            return Err(format!("dir = {v} is not a quoted, non-empty string"));
        };
        return if looks_absolute(os, &dir) {
            Ok(Some(PathBuf::from(dir)))
        } else {
            Err(format!("dir = {dir:?} is not an absolute folder"))
        };
    }
    Ok(None)
}

/// Which step of the precedence gave the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The environment.
    Env,
    /// The user's `config.toml`.
    ConfigFile(PathBuf),
    /// The platform's per-user state folder.
    Default,
    /// Nothing above worked (no home): `$TMPDIR/throttle-lock`.
    TempFallback,
}

/// The lock folder, how it was found, and what to tell the user about a settings file that could not be used.
pub fn resolve_root(get: &dyn Fn(&str) -> Option<String>, host: &Host<'_>, warnings: &mut Vec<String>) -> (PathBuf, Source) {
    let first = |names: &[&str]| names.iter().find_map(|n| get(n).filter(|v| !v.is_empty()));
    if let Some(v) = first(&["THROTTLE_DIR", "PLOCK_DIR", "PLOCK_ROOT"]) {
        return (PathBuf::from(v), Source::Env);
    }
    if let Some(file) = config_file(get, host)
        && let Some(text) = (host.read_file)(&file)
    {
        match dir_from_config(&text, host.os) {
            Ok(Some(dir)) => return (dir, Source::ConfigFile(file)),
            Ok(None) => {}
            Err(e) => warnings.push(format!("{}: {e}; using the default folder", file.display())),
        }
    }
    if let Some(dir) = state_dir(get, host) {
        return (dir, Source::Default);
    }
    let base = first(&["TMPDIR", "TEMP"]).unwrap_or_else(|| "/tmp".into());
    (PathBuf::from(base).join("throttle-lock"), Source::TempFallback)
}

/// The old lock folder whose history moves to `root`: `THROTTLE_MOVED_FROM`, else `<dev-targets>/throttle`, else [`LEGACY_ROOT`], the first
/// that is another folder than `root` and holds a history. The reader copies it once ([`crate::history::History::moved`]). The machine's
/// old folders are read only when the folder is the default or the config file's: a caller that names the folder in the environment (a
/// test, a CI job) never reads the machine's old one (ER-14).
pub fn moved_from(get: &dyn Fn(&str) -> Option<String>, host: &Host<'_>, root: &Path, source: &Source) -> Option<PathBuf> {
    if let Some(v) = get("THROTTLE_MOVED_FROM").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(v));
    }
    if matches!(source, Source::Env | Source::TempFallback) {
        return None;
    }
    let candidates = host
        .dev_targets
        .iter()
        .map(|d| d.join("throttle"))
        .chain(std::iter::once(PathBuf::from(LEGACY_ROOT)));
    candidates.filter(|c| c != root).find(|c| (host.has_history)(c))
}

// The cases are Unix path text (`/h`, `/Users/u`, XDG values): `Path::is_absolute` reads them as absolute only on Unix, where throttle's
// tests run (C171: a Unix-only case sits behind `cfg(unix)`).
#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect()
    }

    fn host(os: Os, user_home: &'static dyn Fn() -> Option<PathBuf>, read: &'static dyn Fn(&Path) -> Option<String>) -> Host<'static> {
        Host {
            os,
            user_home,
            read_file: read,
            has_history: &|_| false,
            dev_targets: None,
        }
    }

    fn root_of(m: &HashMap<String, String>, h: &Host<'_>) -> (PathBuf, Source) {
        resolve_root(&|k| m.get(k).cloned(), h, &mut Vec::new())
    }

    #[test]
    fn the_default_is_the_platforms_per_user_state_folder() {
        let m = env(&[("HOME", "/Users/u")]);
        let none = &|| None;
        let nofile = &|_: &Path| None;
        let (d, s) = root_of(&m, &host(Os::MacOs, none, nofile));
        assert_eq!(
            (d, s),
            (PathBuf::from("/Users/u/Library/Application Support/throttle"), Source::Default)
        );
        assert_eq!(
            root_of(&m, &host(Os::Linux, none, nofile)).0,
            PathBuf::from("/Users/u/.local/state/throttle")
        );
        let m2 = env(&[("HOME", "/h"), ("XDG_STATE_HOME", "/xs")]);
        assert_eq!(root_of(&m2, &host(Os::Linux, none, nofile)).0, PathBuf::from("/xs/throttle"));
        let relative = env(&[("HOME", "/h"), ("XDG_STATE_HOME", "rel")]);
        assert_eq!(
            root_of(&relative, &host(Os::Linux, none, nofile)).0,
            PathBuf::from("/h/.local/state/throttle"),
            "a relative XDG value is ignored"
        );
        let win = env(&[("LOCALAPPDATA", "C:\\Users\\u\\AppData\\Local")]);
        assert_eq!(
            root_of(&win, &host(Os::Windows, none, nofile)).0,
            PathBuf::from("C:\\Users\\u\\AppData\\Local").join("throttle")
        );
    }

    #[test]
    fn precedence_is_the_environment_then_the_config_file_then_the_default() {
        let none = &|| None;
        let file = &|p: &Path| p.ends_with("throttle/config.toml").then(|| "dir = \"/from/config\"\n".to_string());
        let h = host(Os::Linux, none, file);
        let m = env(&[("HOME", "/h")]);
        assert_eq!(
            root_of(&m, &h),
            (
                PathBuf::from("/from/config"),
                Source::ConfigFile(PathBuf::from("/h/.config/throttle/config.toml"))
            )
        );
        // The environment wins over the file, and its aliases keep working (PLOCK_DIR, PLOCK_ROOT; HEAVY_DIR through `aliased`).
        for (k, expected) in [("THROTTLE_DIR", "/e1"), ("PLOCK_DIR", "/e2"), ("PLOCK_ROOT", "/e3")] {
            let m = env(&[("HOME", "/h"), (k, expected)]);
            assert_eq!(root_of(&m, &h), (PathBuf::from(expected), Source::Env), "{k}");
        }
        let m = env(&[("HOME", "/h"), ("THROTTLE_DIR", "/e1"), ("PLOCK_DIR", "/e2")]);
        assert_eq!(root_of(&m, &h).0, PathBuf::from("/e1"));
        // Without the file the default is used.
        let nofile = &|_: &Path| None;
        assert_eq!(
            root_of(&env(&[("HOME", "/h")]), &host(Os::Linux, none, nofile)).0,
            PathBuf::from("/h/.local/state/throttle")
        );
    }

    #[test]
    fn a_daemon_without_home_finds_it_through_the_user_database() {
        let db = &|| Some(PathBuf::from("/Users/runner"));
        let nofile = &|_: &Path| None;
        let (d, s) = root_of(&HashMap::new(), &host(Os::MacOs, db, nofile));
        assert_eq!(
            (d, s),
            (PathBuf::from("/Users/runner/Library/Application Support/throttle"), Source::Default)
        );
        // The config file is looked up under the same home.
        let file = &|p: &Path| {
            (p == Path::new("/Users/runner/Library/Application Support/throttle/config.toml")).then(|| "dir = '/cfg'".to_string())
        };
        assert_eq!(root_of(&HashMap::new(), &host(Os::MacOs, db, file)).0, PathBuf::from("/cfg"));
        // An empty or relative HOME is no home either.
        for bad in ["", "rel"] {
            assert_eq!(
                root_of(&env(&[("HOME", bad)]), &host(Os::MacOs, db, nofile)).0,
                PathBuf::from("/Users/runner/Library/Application Support/throttle")
            );
        }
        // No home anywhere: the temp fallback, never a panic.
        let none = &|| None;
        let m = env(&[("TMPDIR", "/t")]);
        assert_eq!(
            root_of(&m, &host(Os::MacOs, none, nofile)),
            (PathBuf::from("/t/throttle-lock"), Source::TempFallback)
        );
    }

    #[test]
    fn the_config_files_dir_key() {
        let l = Os::Linux;
        assert_eq!(
            dir_from_config("# c\nother = 1\ndir = \"/a b\"  # x\n", l),
            Ok(Some(PathBuf::from("/a b")))
        );
        assert_eq!(dir_from_config("dir = '/lit'", l), Ok(Some(PathBuf::from("/lit"))));
        assert_eq!(
            dir_from_config("dir = 'C:\\x\\t'", Os::Windows),
            Ok(Some(PathBuf::from("C:\\x\\t")))
        );
        assert_eq!(dir_from_config("dir = \"C:\\\\x\"", Os::Windows), Ok(Some(PathBuf::from("C:\\x"))));
        assert_eq!(dir_from_config("", l), Ok(None));
        assert_eq!(
            dir_from_config("[t]\ndir = \"/in-table\"", l),
            Ok(None),
            "only the top level counts"
        );
        for bad in ["dir = /nope", "dir = \"\"", "dir = \"rel/dir\"", "dir = \"/open", "dir = \"C:\\x\""] {
            assert!(dir_from_config(bad, l).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_bad_config_file_is_reported_and_the_default_used() {
        let file = &|_: &Path| Some("dir = \"rel\"".to_string());
        let none = &|| None;
        let h = host(Os::Linux, none, file);
        let m = env(&[("HOME", "/h")]);
        let mut w = Vec::new();
        let (d, s) = resolve_root(&|k| m.get(k).cloned(), &h, &mut w);
        assert_eq!((d, s), (PathBuf::from("/h/.local/state/throttle"), Source::Default));
        assert!(w.len() == 1 && w[0].contains("config.toml") && w[0].contains("absolute"), "{w:?}");
    }

    #[test]
    fn the_old_folder_is_named_only_when_it_has_a_history_and_is_another_folder() {
        let has = &|d: &Path| d == Path::new("/dt/throttle") || d == Path::new(LEGACY_ROOT);
        let mut h = Host::none();
        h.has_history = has;
        h.dev_targets = Some(PathBuf::from("/dt"));
        let get = &|_: &str| None;
        let d = &Source::Default;
        assert_eq!(moved_from(get, &h, Path::new("/new"), d), Some(PathBuf::from("/dt/throttle")));
        assert_eq!(
            moved_from(get, &h, Path::new("/dt/throttle"), d),
            Some(PathBuf::from(LEGACY_ROOT)),
            "the folder itself did not move"
        );
        h.dev_targets = None;
        assert_eq!(moved_from(get, &h, Path::new("/new"), d), Some(PathBuf::from(LEGACY_ROOT)));
        assert_eq!(moved_from(get, &h, Path::new(LEGACY_ROOT), d), None);
        let cf = &Source::ConfigFile(PathBuf::from("/c"));
        assert_eq!(moved_from(get, &h, Path::new("/new"), cf), Some(PathBuf::from(LEGACY_ROOT)));
        assert_eq!(
            moved_from(get, &h, Path::new("/new"), &Source::Env),
            None,
            "a folder named in the environment reads no old folder"
        );
        let h2 = Host::none();
        assert_eq!(
            moved_from(get, &h2, Path::new("/new"), d),
            None,
            "no history anywhere: nothing is read"
        );
        let named = &|k: &str| (k == "THROTTLE_MOVED_FROM").then(|| "/named".to_string());
        assert_eq!(
            moved_from(named, &h2, Path::new("/new"), &Source::Env),
            Some(PathBuf::from("/named"))
        );
    }
}
