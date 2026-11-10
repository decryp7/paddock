//! Finding a running Pitwall (Pitwall ADR 0009, Debrief ADR 0059): its `discovery.json` in Pitwall's data folder, else the
//! well-known port on 127.0.0.1. Pure decisions over an injected [`Locator`], so tests use a temporary folder and their own
//! port and never the user's real Pitwall.
//!
//! Folders come from `paddock-env` (`AppDirs` for `pitwall`): the same place as Pitwall's own on macOS and Linux, and on
//! Windows the Local folder first, then the old Roaming one for one minor version (decision D3, Pitwall 1.0.3 and older).
//! `PITWALL_HOME` names the data folder itself (Pitwall ADR 0009) and replaces the others.

use paddock_env::{AppDirs, AppId, Os, ProcessVars, Vars};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The discovery file's name in Pitwall's data folder.
pub const FILE_NAME: &str = "discovery.json";
/// Largest discovery file read.
pub const MAX_BYTES: u64 = 16 * 1024;
/// Pitwall's well-known port (next to Ollama's 11434).
pub const WELL_KNOWN_PORT: u16 = 11435;
/// Lowest port a discovery file may name.
const MIN_PORT: u16 = 1024;

/// The fields of `discovery.json` the client reads; unknown fields are ignored (the file grows additively).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    /// The file format.
    pub format: u32,
    /// Pitwall's version.
    pub version: String,
    /// The client contract version.
    pub api_version: u32,
    /// The server's process id.
    pub pid: u32,
    /// The loopback port.
    pub port: u16,
    /// `http://127.0.0.1:<port>`.
    pub base_url: String,
    /// A newer Pitwall leaves the file with `false` while it is installed but not serving (absent: serving).
    #[serde(default = "yes")]
    pub serving: bool,
}

fn yes() -> bool {
    true
}

impl Discovery {
    /// Parses the file: bounded, typed, and only a base URL on this machine is trusted.
    pub fn parse(text: &str) -> Result<Discovery, String> {
        if text.len() as u64 > MAX_BYTES {
            return Err("Pitwall's discovery file is too large".into());
        }
        let d: Discovery = serde_json::from_str(text)
            .map_err(|e| format!("Pitwall's discovery file is damaged ({e})"))?;
        if d.base_url != format!("http://127.0.0.1:{}", d.port) || d.port < MIN_PORT {
            return Err("Pitwall's discovery file does not point at this computer".into());
        }
        if d.version.len() > 64 {
            return Err("Pitwall's discovery file is damaged (version)".into());
        }
        Ok(d)
    }
}

/// The folders that may hold Pitwall's `discovery.json`, best first.
pub fn candidate_dirs(vars: &dyn Vars, os: Os) -> Vec<PathBuf> {
    if let Some(home) = vars.get("PITWALL_HOME").filter(|v| !v.is_empty()) {
        let home = PathBuf::from(home);
        return vec![home.clone(), home.join("data")];
    }
    // Constants that satisfy the id rules (a test covers it).
    let app = AppId::new("pitwall", "Pitwall").expect("the constant app id is valid");
    // The override variable was handled above; the portable flag and `<ID>_HOME` of the *calling* app do not apply to Pitwall.
    match AppDirs::resolve(&app, os, vars, None, false) {
        Ok(dirs) => {
            let mut v = vec![dirs.data().to_path_buf()];
            v.extend(dirs.legacy_roaming_data().map(Path::to_path_buf));
            v
        }
        Err(_) => vec![],
    }
}

/// Whether process `pid` may exist (any user); pid 0 never does. When the OS process table cannot be read the answer is
/// "yes": a discovery file is removed only on a proven "gone" (rule 53).
pub fn pid_alive(pid: u32) -> bool {
    pid_alive_by(pid, &paddock_proc::presence)
}

/// [`pid_alive`] over a look at the process (tests give one that cannot read the table).
fn pid_alive_by(pid: u32, look: &dyn Fn(u32) -> paddock_proc::Presence) -> bool {
    pid != 0 && look(pid).may_be_alive()
}

/// Whether a process id is alive.
pub type PidAlive = Arc<dyn Fn(u32) -> bool + Send + Sync>;

/// Where to look for Pitwall.
#[derive(Clone)]
pub struct Locator {
    /// Pitwall's data folders, best first.
    pub dirs: Vec<PathBuf>,
    /// The port tried when there is no discovery file (`None`: only the discovery file counts; tests).
    pub fallback_port: Option<u16>,
    /// Whether a process id is alive.
    pub pid_alive: PidAlive,
}

impl std::fmt::Debug for Locator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Locator")
            .field("dirs", &self.dirs)
            .field("fallback_port", &self.fallback_port)
            .finish()
    }
}

/// What the files say, before anything is asked over the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// No data folder: Pitwall was never run here.
    Absent,
    /// A discovery file of a live process.
    Discovered(Discovery),
    /// No usable discovery file; `port` may still answer (the well-known port), `installed` when a data folder exists.
    Guess {
        /// The port to try.
        port: Option<u16>,
        /// A data folder exists.
        installed: bool,
        /// Why the file was not used (damaged, too large); `None` for a missing file or a dead process.
        note: Option<String>,
    },
}

impl Locator {
    /// The real machine: Pitwall's folders for this user, the well-known port, the OS process table.
    pub fn system() -> Locator {
        Locator {
            dirs: candidate_dirs(&ProcessVars, Os::current()),
            fallback_port: Some(WELL_KNOWN_PORT),
            pid_alive: Arc::new(pid_alive),
        }
    }

    /// A locator over one folder (tests, and apps that were told where Pitwall lives).
    pub fn in_dir(dir: &Path, fallback_port: Option<u16>, pid_alive: PidAlive) -> Locator {
        Locator {
            dirs: vec![dir.to_path_buf()],
            fallback_port,
            pid_alive,
        }
    }

    /// Reads the discovery files (bounded) and decides where Pitwall may be.
    pub fn look(&self) -> Found {
        let installed = self.dirs.iter().any(|d| d.is_dir());
        let mut note = None;
        for dir in &self.dirs {
            let text = match read_bounded(&dir.join(FILE_NAME)) {
                Ok(Some(t)) => t,
                Ok(None) => continue,
                Err(e) => {
                    note.get_or_insert(e);
                    continue;
                }
            };
            match Discovery::parse(&text) {
                Ok(d) if (self.pid_alive)(d.pid) => return Found::Discovered(d),
                // A file left behind by a crashed Pitwall: the server may still answer on the well-known port.
                Ok(_) => {}
                Err(e) => {
                    note.get_or_insert(e);
                }
            }
        }
        if note.is_none() && !installed && self.fallback_port.is_none() {
            return Found::Absent;
        }
        Found::Guess {
            port: self.fallback_port,
            installed,
            note,
        }
    }
}

fn read_bounded(path: &Path) -> Result<Option<String>, String> {
    use std::io::Read;
    let f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Pitwall's discovery file cannot be read ({e})")),
    };
    let mut text = String::new();
    f.take(MAX_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| "Pitwall's discovery file is damaged".to_string())?;
    Ok(Some(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_env::MapVars;

    fn file(port: u16, pid: u32) -> String {
        format!(
            r#"{{"format":1,"version":"1.0.4","apiVersion":1,"pid":{pid},"port":{port},"baseUrl":"http://127.0.0.1:{port}","startedAt":7,"future":true}}"#
        )
    }

    fn locator(dir: &Path, fallback: Option<u16>) -> Locator {
        Locator::in_dir(dir, fallback, Arc::new(|pid| pid == 42))
    }

    #[test]
    fn parsing_is_strict_about_the_address_and_lenient_about_new_fields() {
        let d = Discovery::parse(&file(23456, 42)).unwrap();
        assert_eq!(
            (
                d.api_version,
                d.port,
                d.version.as_str(),
                d.format,
                d.serving
            ),
            (1, 23456, "1.0.4", 1, true)
        );
        let paused =
            Discovery::parse(&file(23456, 42).replace("\"future\":true", "\"serving\":false"))
                .unwrap();
        assert!(!paused.serving);
        for bad in [
            file(23456, 42).replace("http://127.0.0.1:23456", "http://evil.example:23456"),
            file(23456, 42).replace("http://127.0.0.1:23456", "http://127.0.0.1:23456/"),
            file(80, 42),
            file(23456, 42).replace("1.0.4", &"9".repeat(65)),
        ] {
            assert!(Discovery::parse(&bad).is_err(), "{bad}");
        }
        assert!(
            Discovery::parse(&file(80, 42))
                .unwrap_err()
                .contains("this computer")
        );
        assert!(Discovery::parse("{").unwrap_err().contains("damaged"));
        assert!(
            Discovery::parse(&" ".repeat(MAX_BYTES as usize + 1))
                .unwrap_err()
                .contains("too large")
        );
    }

    #[test]
    fn the_folders_match_pitwalls_and_windows_keeps_the_old_one_for_a_while() {
        let mac = MapVars::new().with("HOME", "/h");
        assert_eq!(
            candidate_dirs(&mac, Os::MacOs),
            vec![PathBuf::from("/h/Library/Application Support/Pitwall")]
        );
        let linux = MapVars::new()
            .with("HOME", "/h")
            .with("XDG_DATA_HOME", "/x");
        assert_eq!(
            candidate_dirs(&linux, Os::Linux),
            vec![PathBuf::from("/x/pitwall")]
        );
        let relative = MapVars::new()
            .with("HOME", "/h")
            .with("XDG_DATA_HOME", "rel");
        assert_eq!(
            candidate_dirs(&relative, Os::Linux),
            vec![PathBuf::from("/h/.local/share/pitwall")]
        );
        let win = MapVars::new()
            .with("USERPROFILE", "C:\\U")
            .with("LOCALAPPDATA", "C:\\U\\L")
            .with("APPDATA", "C:\\U\\R");
        let dirs = candidate_dirs(&win, Os::Windows);
        assert_eq!(dirs.len(), 2);
        assert!(
            dirs[0].ends_with("Pitwall") && dirs[0].to_string_lossy().contains('L'),
            "Local first: {dirs:?}"
        );
        assert!(
            dirs[1].to_string_lossy().contains('R'),
            "then Roaming: {dirs:?}"
        );
        // PITWALL_HOME names the folder itself (and its `data` child) and replaces the rest.
        let own = mac.clone().with("PITWALL_HOME", "/p");
        assert_eq!(
            candidate_dirs(&own, Os::MacOs),
            vec![PathBuf::from("/p"), PathBuf::from("/p/data")]
        );
        // The calling app's own override variable does not move Pitwall's folder.
        let other = mac.clone().with("DEBRIEF_HOME", "/d");
        assert_eq!(candidate_dirs(&other, Os::MacOs).len(), 1);
        // No home folder: nothing to look in, the fallback port still works.
        assert!(candidate_dirs(&MapVars::new(), Os::MacOs).is_empty());
        let _ = Locator::system();
    }

    #[test]
    fn look_reads_the_file_of_a_live_process_and_guesses_otherwise() {
        let d = tempfile::tempdir().unwrap();
        let missing = d.path().join("none");
        assert_eq!(locator(&missing, None).look(), Found::Absent);
        assert_eq!(
            locator(&missing, Some(9999)).look(),
            Found::Guess {
                port: Some(9999),
                installed: false,
                note: None
            }
        );
        assert_eq!(
            locator(d.path(), None).look(),
            Found::Guess {
                port: None,
                installed: true,
                note: None
            }
        );
        std::fs::write(d.path().join(FILE_NAME), file(23456, 42)).unwrap();
        assert!(matches!(locator(d.path(), None).look(), Found::Discovered(x) if x.port == 23456));
        // A stale file (dead process) is a guess.
        std::fs::write(d.path().join(FILE_NAME), file(23456, 7)).unwrap();
        assert!(matches!(
            locator(d.path(), Some(1)).look(),
            Found::Guess {
                note: None,
                installed: true,
                ..
            }
        ));
        std::fs::write(d.path().join(FILE_NAME), "garbage").unwrap();
        assert!(
            matches!(locator(d.path(), None).look(), Found::Guess { note: Some(n), .. } if n.contains("damaged"))
        );
        std::fs::write(
            d.path().join(FILE_NAME),
            vec![b' '; MAX_BYTES as usize + 10],
        )
        .unwrap();
        assert!(
            matches!(locator(d.path(), None).look(), Found::Guess { note: Some(n), .. } if n.contains("too large"))
        );
        assert!(format!("{:?}", locator(d.path(), None)).contains("Locator"));
        // A file that cannot be read as text is damaged, not a crash.
        std::fs::write(d.path().join(FILE_NAME), [0xff, 0xfe, 0xfd]).unwrap();
        assert!(matches!(
            locator(d.path(), None).look(),
            Found::Guess { note: Some(_), .. }
        ));
        // A file the user cannot read is reported, not a crash.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(d.path().join(FILE_NAME), file(23456, 42)).unwrap();
            std::fs::set_permissions(
                d.path().join(FILE_NAME),
                std::fs::Permissions::from_mode(0o000),
            )
            .unwrap();
            // (Running as root reads it anyway; both outcomes are valid answers.)
            let found = locator(d.path(), None).look();
            assert!(
                matches!(
                    found,
                    Found::Discovered(_) | Found::Guess { note: Some(_), .. }
                ),
                "{found:?}"
            );
            std::fs::set_permissions(
                d.path().join(FILE_NAME),
                std::fs::Permissions::from_mode(0o600),
            )
            .unwrap();
            std::fs::remove_file(d.path().join(FILE_NAME)).unwrap();
            std::fs::write(d.path().join(FILE_NAME), "").unwrap();
        }
        // A directory where the file should be is unreadable.
        std::fs::remove_file(d.path().join(FILE_NAME)).unwrap();
        std::fs::create_dir(d.path().join(FILE_NAME)).unwrap();
        assert!(matches!(
            locator(d.path(), None).look(),
            Found::Guess { note: Some(_), .. }
        ));
    }

    #[test]
    fn the_first_live_file_among_several_folders_wins() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        std::fs::write(old.path().join(FILE_NAME), file(30001, 42)).unwrap();
        let l = Locator {
            dirs: vec![new.path().to_path_buf(), old.path().to_path_buf()],
            fallback_port: None,
            pid_alive: Arc::new(|p| p == 42),
        };
        assert!(
            matches!(l.look(), Found::Discovered(x) if x.port == 30001),
            "the Roaming file is found when Local has none"
        );
        std::fs::write(new.path().join(FILE_NAME), file(30002, 42)).unwrap();
        assert!(
            matches!(l.look(), Found::Discovered(x) if x.port == 30002),
            "Local wins"
        );
    }

    #[test]
    fn this_process_is_alive_and_pid_zero_is_not() {
        assert!(pid_alive(std::process::id()));
        assert!(!pid_alive(0));
    }

    #[test]
    fn a_discovery_file_is_stale_only_when_the_process_is_proven_gone() {
        use paddock_proc::Presence::{Gone, Running, Unknown};
        assert!(pid_alive_by(7, &|_| Running));
        assert!(
            pid_alive_by(7, &|_| Unknown),
            "an unreadable process table never makes the file stale (rule 53)"
        );
        assert!(!pid_alive_by(7, &|_| Gone));
        assert!(!pid_alive_by(0, &|_| Running), "pid 0 never exists");
    }
}
