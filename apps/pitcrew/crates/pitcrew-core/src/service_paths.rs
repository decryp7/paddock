//! Where the service keeps its files and its socket, per OS (ADR 0001, ADR 0009). Pure: the environment is a
//! function, so the service, the app and the tests agree on the same rules.
//!
//! - Windows: `%ProgramData%\Pitcrew` (machine-wide; the service is LocalSystem). The transport is a named pipe.
//! - macOS: `~/Library/Application Support/Pitcrew`; the socket is in the same folder.
//! - Linux: `$XDG_DATA_HOME/Pitcrew` (default `~/.local/share/Pitcrew`); the socket is in
//!   `$XDG_RUNTIME_DIR/pitcrew` when that is set (a per-user tmpfs, mode 0700, cleaned at log-out), else in the data folder.

use crate::platform::Os;
use std::path::{Path, PathBuf};

/// The access an interactive user's connection to the service's named pipe has and asks for (Windows): read and write data and
/// the attributes and `SYNCHRONIZE`/`READ_CONTROL` they need, **without `FILE_CREATE_PIPE_INSTANCE` (0x4)**, so a standard user
/// cannot host a pipe of this name and receive an elevated caller's connection. `GENERIC_READ | GENERIC_WRITE` includes that bit
/// (as `FILE_APPEND_DATA`), so both ends use this exact mask: the service's pipe ACL gives it to interactive users and the
/// app opens the pipe with it.
pub const PIPE_CLIENT_ACCESS: u32 = 0x0012_019b;

/// SYSTEM (`S-1-5-18`): the account the Windows service runs as.
pub const SID_SYSTEM: &str = "S-1-5-18";
/// BUILTIN\Administrators (`S-1-5-32-544`): the owner Windows gives what an administrator or a SYSTEM process creates.
pub const SID_ADMINISTRATORS: &str = "S-1-5-32-544";

/// Whether a client may talk to a pipe whose object is owned by `owner` (a SID string): the service's accounts (SYSTEM,
/// Administrators) always, and `own_user` (the SID of the account the client runs as) only for the pipe of the **per-user engine**,
/// which that user's own process hosts. For the system service's pipe pass `None`: a pipe owned by an ordinary user is then a
/// stranger's. Pure; the owner is read from the pipe object through the client's own handle (no pid is involved).
pub fn pipe_owner_ok(owner: &str, own_user: Option<&str>) -> bool {
    owner == SID_SYSTEM || owner == SID_ADMINISTRATORS || own_user.is_some_and(|u| u == owner)
}

/// How a client opens the service's pipe (Windows): with exactly [`PIPE_CLIENT_ACCESS`], and at **identification** impersonation
/// level (`SECURITY_IDENTIFICATION`), so whatever answers on the pipe can learn who the caller is but can never act as them: an
/// elevated caller's token is not handed to a program that merely hosts a pipe of this name.
#[cfg(windows)]
pub fn open_pipe_options() -> std::fs::OpenOptions {
    use std::os::windows::fs::OpenOptionsExt;
    /// `SECURITY_IDENTIFICATION` (winbase.h); the standard library adds `SECURITY_SQOS_PRESENT`.
    const SECURITY_IDENTIFICATION: u32 = 0x0001_0000;
    let mut options = std::fs::OpenOptions::new();
    options.access_mode(PIPE_CLIENT_ACCESS).security_qos_flags(SECURITY_IDENTIFICATION);
    options
}

/// The pipe of the engine whose data folder is `data_dir`, as a Windows pipe name: the installed service's folder (`installed`)
/// has `\\.\pipe\pitcrew`; any other folder (the per-user engine's, tests, development) has a name of its own derived from it, so
/// it never collides with the service installed on the same machine. Folders compare without case and trailing separators, as
/// Windows paths do. Pure.
pub fn pipe_name_for(data_dir: &Path, installed: Option<&Path>) -> String {
    const PIPE_NAME: &str = r"\\.\pipe\pitcrew";
    let norm = |p: &Path| p.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase();
    let dir = norm(data_dir);
    if installed.is_some_and(|i| norm(i) == dir) {
        return PIPE_NAME.to_string();
    }
    // FNV-1a: stable across builds and platforms, no dependency.
    let hash = dir.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("{PIPE_NAME}-{hash:016x}")
}

/// The data folder of the **per-user engine** on Windows (`%LOCALAPPDATA%\Pitcrew\engine`): the same engine as the service
/// (`pitcrew-service run`) started in the user's own session at log-on, with a folder only that user can write, for the runners that
/// live in folders the user can change (ADR 0004, second amendment). `None` when the environment names no such folder. Pure.
pub fn user_engine_dir(env: &dyn Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    env("LOCALAPPDATA").map(|d| join(Os::Windows, &d, &["Pitcrew", "engine"]))
}

/// The Unix socket's file name.
pub const SOCKET_FILE: &str = "pitcrew.sock";

// Paths are built and checked with `os`'s rules, not the host's (`Path::join` and `is_absolute` follow the host), so the
// answer for an OS is the same wherever the code runs.

/// `base` with `parts` appended, separated as on `os`.
fn join(os: Os, base: &Path, parts: &[&str]) -> PathBuf {
    let sep = if os == Os::Windows { '\\' } else { '/' };
    let mut s = base.to_string_lossy().trim_end_matches(sep).to_string();
    for p in parts {
        s.push(sep);
        s.push_str(p);
    }
    PathBuf::from(s)
}

/// Whether `p` is absolute on `os`.
fn is_absolute_on(os: Os, p: &Path) -> bool {
    let s = p.to_string_lossy();
    let b = s.as_bytes();
    match os {
        Os::Windows => {
            s.starts_with("\\\\") || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/'))
        }
        Os::Macos | Os::Linux => s.starts_with('/'),
    }
}

/// An absolute path from an environment variable; XDG says to ignore relative values.
fn absolute(os: Os, env: &dyn Fn(&str) -> Option<PathBuf>, key: &str) -> Option<PathBuf> {
    env(key).filter(|p| is_absolute_on(os, p))
}

/// The service's data folder on `os`, or `None` when the environment names no home.
pub fn data_dir(os: Os, env: &dyn Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    match os {
        Os::Windows => env("ProgramData").map(|p| join(os, &p, &["Pitcrew"])),
        Os::Macos => env("HOME").map(|h| join(os, &h, &["Library", "Application Support", "Pitcrew"])),
        Os::Linux => absolute(os, env, "XDG_DATA_HOME")
            .map(|d| join(os, &d, &["Pitcrew"]))
            .or_else(|| env("HOME").map(|h| join(os, &h, &[".local", "share", "Pitcrew"]))),
    }
}

/// The folder holding the service's Unix socket (not used on Windows).
pub fn socket_dir(os: Os, env: &dyn Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    match os {
        Os::Linux => absolute(os, env, "XDG_RUNTIME_DIR")
            .map(|r| join(os, &r, &["pitcrew"]))
            .or_else(|| data_dir(os, env)),
        _ => data_dir(os, env),
    }
}

/// The socket's full path.
pub fn socket_path(os: Os, env: &dyn Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    socket_dir(os, env).map(|d| join(os, &d, &[SOCKET_FILE]))
}

/// Whether `dir` is one the service may use as a data folder: absolute.
pub fn is_usable(dir: &Path) -> bool {
    dir.is_absolute()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<PathBuf> + 'a {
        move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| PathBuf::from(v))
    }

    #[test]
    fn each_os_has_its_data_folder() {
        let e = env(&[("ProgramData", "C:\\ProgramData"), ("HOME", "/home/ci")]);
        assert_eq!(data_dir(Os::Windows, &e), Some(PathBuf::from("C:\\ProgramData\\Pitcrew")));
        assert_eq!(
            data_dir(Os::Macos, &e),
            Some(PathBuf::from("/home/ci/Library/Application Support/Pitcrew"))
        );
        assert_eq!(data_dir(Os::Linux, &e), Some(PathBuf::from("/home/ci/.local/share/Pitcrew")));
        assert_eq!(data_dir(Os::Linux, &env(&[])), None);
        assert_eq!(data_dir(Os::Windows, &env(&[("HOME", "/h")])), None);
        assert_eq!(data_dir(Os::Macos, &env(&[])), None);
    }

    #[test]
    fn the_per_user_engine_has_a_folder_of_its_own_and_a_pipe_of_its_own() {
        let e = env(&[
            ("LOCALAPPDATA", "C:\\Users\\me\\AppData\\Local"),
            ("ProgramData", "C:\\ProgramData"),
        ]);
        let user = user_engine_dir(&e).unwrap();
        assert_eq!(user, PathBuf::from("C:\\Users\\me\\AppData\\Local\\Pitcrew\\engine"));
        assert_eq!(user_engine_dir(&env(&[])), None);
        let installed = data_dir(Os::Windows, &e);
        // The service keeps `\\.\pipe\pitcrew`; the per-user engine's pipe is another name, stable for the same folder.
        assert_eq!(
            pipe_name_for(installed.as_deref().unwrap(), installed.as_deref()),
            r"\\.\pipe\pitcrew"
        );
        let pipe = pipe_name_for(&user, installed.as_deref());
        assert!(pipe.starts_with(r"\\.\pipe\pitcrew-") && pipe != r"\\.\pipe\pitcrew", "{pipe}");
        assert_eq!(
            pipe,
            pipe_name_for(Path::new("c:\\users\\me\\appdata\\local\\pitcrew\\engine\\"), installed.as_deref())
        );
    }

    #[test]
    fn a_pipe_is_the_services_when_its_owner_is_system_or_administrators_and_a_users_only_for_their_own_engine() {
        assert!(pipe_owner_ok(SID_SYSTEM, None) && pipe_owner_ok(SID_ADMINISTRATORS, None));
        let me = "S-1-5-21-1-2-3-1001";
        assert!(
            !pipe_owner_ok(me, None),
            "a user's pipe is a stranger's to the system service's client"
        );
        assert!(pipe_owner_ok(me, Some(me)), "the per-user engine's own pipe");
        assert!(!pipe_owner_ok("S-1-5-21-1-2-3-1002", Some(me)), "another user's pipe is not mine");
        assert!(!pipe_owner_ok("S-1-5-32-545", Some(me)), "Users");
        assert!(!pipe_owner_ok("", None));
    }

    #[test]
    fn paths_follow_the_target_os_not_the_host() {
        assert!(is_absolute_on(Os::Linux, Path::new("/data")));
        assert!(!is_absolute_on(Os::Linux, Path::new("data")));
        assert!(!is_absolute_on(Os::Linux, Path::new("C:\\data")));
        assert!(is_absolute_on(Os::Windows, Path::new("C:\\data")));
        assert!(is_absolute_on(Os::Windows, Path::new("D:/data")));
        assert!(is_absolute_on(Os::Windows, Path::new("\\\\server\\share")));
        assert!(!is_absolute_on(Os::Windows, Path::new("/data")));
        assert_eq!(join(Os::Linux, Path::new("/a/"), &["b", "c"]), PathBuf::from("/a/b/c"));
        assert_eq!(join(Os::Windows, Path::new("C:\\a\\"), &["b"]), PathBuf::from("C:\\a\\b"));
    }

    #[test]
    fn linux_follows_xdg_but_ignores_relative_values() {
        let e = env(&[("HOME", "/home/ci"), ("XDG_DATA_HOME", "/data")]);
        assert_eq!(data_dir(Os::Linux, &e), Some(PathBuf::from("/data/Pitcrew")));
        let rel = env(&[("HOME", "/home/ci"), ("XDG_DATA_HOME", "data"), ("XDG_RUNTIME_DIR", "run")]);
        assert_eq!(data_dir(Os::Linux, &rel), Some(PathBuf::from("/home/ci/.local/share/Pitcrew")));
        assert_eq!(socket_dir(Os::Linux, &rel), Some(PathBuf::from("/home/ci/.local/share/Pitcrew")));
    }

    #[test]
    fn the_linux_socket_prefers_the_runtime_dir() {
        let e = env(&[("HOME", "/home/ci"), ("XDG_RUNTIME_DIR", "/run/user/1000")]);
        assert_eq!(socket_dir(Os::Linux, &e), Some(PathBuf::from("/run/user/1000/pitcrew")));
        assert_eq!(
            socket_path(Os::Linux, &e),
            Some(PathBuf::from("/run/user/1000/pitcrew/pitcrew.sock"))
        );
        let no_runtime = env(&[("HOME", "/home/ci")]);
        assert_eq!(
            socket_path(Os::Linux, &no_runtime),
            Some(PathBuf::from("/home/ci/.local/share/Pitcrew/pitcrew.sock"))
        );
        assert_eq!(
            socket_path(Os::Macos, &no_runtime),
            Some(PathBuf::from("/home/ci/Library/Application Support/Pitcrew/pitcrew.sock"))
        );
        assert_eq!(socket_path(Os::Linux, &env(&[])), None);
        assert!(is_usable(Path::new(if cfg!(windows) { "C:\\x" } else { "/x" })));
        assert!(!is_usable(Path::new("x")));
    }
}
