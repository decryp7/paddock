//! A private local address the host listens on, and random tokens. Never
//! TCP. Unix: a socket inside a freshly created directory with mode 0700. Windows: a randomly named pipe
//! that only the current user can open (see `winpipe`).

use std::io;
use std::path::PathBuf;

/// `bytes` random bytes as lowercase hex.
pub fn random_hex(bytes: usize) -> io::Result<String> {
    let mut b = vec![0u8; bytes];
    getrandom::fill(&mut b).map_err(|e| io::Error::other(format!("no random numbers available: {e}")))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// A 256-bit random token.
pub fn new_token() -> io::Result<String> {
    random_hex(32)
}

/// Compares in constant time, so the token cannot be guessed byte by byte.
pub fn tokens_equal(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = (a.len() != b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(1);
    }
    diff == 0
}

/// Longest socket path (bytes) an endpoint may have: Unix socket paths are limited to about 100 bytes.
#[cfg(unix)]
pub const MAX_SOCKET_PATH: usize = 96;
/// Random bytes in an endpoint folder's name (twice that many hex digits).
#[cfg(unix)]
const NAME_BYTES: usize = 6;

/// Whether an endpoint named `<prefix>-<random>` can be made in `base` (on Unix, when its socket path is short enough;
/// `create_named` goes on to the next base otherwise). Always true elsewhere. `base` is taken as it is: callers pass the
/// resolved path, as `create_named` does.
pub fn fits(base: &std::path::Path, prefix: &str) -> bool {
    #[cfg(unix)]
    {
        base.join(format!("{prefix}-{}", "0".repeat(2 * NAME_BYTES)))
            .join("w.sock")
            .as_os_str()
            .len()
            <= MAX_SOCKET_PATH
    }
    #[cfg(not(unix))]
    {
        let _ = (base, prefix);
        true
    }
}

/// The names (prefixes) of the endpoint folders each part of Debrief makes, so a leak can be told from its folder name and
/// tests can check that every one fits (see [`fits`]).
pub mod prefix {
    /// The default of [`crate::process::SpawnSpec`] and of [`super::PrivateEndpoint::create`].
    pub const DEFAULT: &str = "debrief-ipc";
    /// A built-in plugin's process.
    pub const PLUGIN: &str = "debrief-plugin";
    /// A plugin under test (the plugin SDK's test host).
    pub const PLUGIN_TEST: &str = "debrief-plugin-test";
    /// The AI worker.
    pub const AI: &str = "debrief-ai";
    /// The store worker, made by the app process (its temp folder is the app's own).
    pub const STORE: &str = "debrief-store";
}

/// The private folders of the endpoints alive in this process (Unix), so a process that must end without running
/// destructors (a child whose lifeline closed calls `process::exit`) can still remove them: [`remove_live`].
#[cfg(unix)]
static LIVE: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

#[cfg(unix)]
fn live() -> std::sync::MutexGuard<'static, Vec<PathBuf>> {
    LIVE.lock().unwrap_or_else(|p| p.into_inner())
}

/// Removes the private folder of every endpoint still alive in this process. Call it right before ending the process
/// without unwinding (`std::process::exit`): a host that is stopped while it starts a child (the socket exists, the
/// child has not connected yet) would otherwise leave `<prefix>-<random>` behind in the temp folder, for good.
pub fn remove_live() {
    #[cfg(unix)]
    for dir in std::mem::take(&mut *live()) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// A listening address owned by the supervisor; the private directory is removed on drop.
pub struct PrivateEndpoint {
    pub address: String,
    dir: Option<PathBuf>,
}

impl PrivateEndpoint {
    /// A private address named `debrief-ipc-<random>` ([`prefix::DEFAULT`]).
    pub fn create() -> io::Result<PrivateEndpoint> {
        Self::create_named(prefix::DEFAULT)
    }

    /// A private address whose directory (Unix) or pipe (Windows) is named `<prefix>-<random>`, so a user
    /// looking at open handles can tell which part of Debrief it belongs to.
    #[cfg(unix)]
    pub fn create_named(prefix: &str) -> io::Result<PrivateEndpoint> {
        let harden: fn(&std::path::Path) -> io::Result<()> = harden_dir;
        let mut bases: Vec<PathBuf> = Vec::new();
        if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|d| d.is_dir()) {
            bases.push(d);
        }
        bases.push(std::env::temp_dir());
        bases.push(PathBuf::from("/tmp"));
        let mut last = io::Error::other("no place for a private socket");
        for base in bases {
            // Named by its resolved path (macOS' temp folder `/var/folders/…` is `/private/var/folders/…`): a contained
            // child's sandbox profile allows exactly this socket, and the sandbox matches resolved paths.
            let base = base.canonicalize().unwrap_or(base);
            match Self::create_in(&base, prefix, harden) {
                Ok(Some(e)) => return Ok(e),
                Ok(None) => {}
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// One try in `base`: `Ok(None)` when the socket path would be too long there. The folder is owned by the returned
    /// endpoint from the moment it exists, so a failing later step (`harden`) removes it again instead of leaving it.
    #[cfg(unix)]
    fn create_in(
        base: &std::path::Path,
        prefix: &str,
        harden: fn(&std::path::Path) -> io::Result<()>,
    ) -> io::Result<Option<PrivateEndpoint>> {
        use std::os::unix::fs::DirBuilderExt;
        let dir = base.join(format!("{prefix}-{}", random_hex(NAME_BYTES)?));
        let sock = dir.join("w.sock");
        if sock.as_os_str().len() > MAX_SOCKET_PATH {
            return Ok(None);
        }
        std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
        live().push(dir.clone());
        let endpoint = PrivateEndpoint {
            address: sock.to_string_lossy().into_owned(),
            dir: Some(dir.clone()),
        };
        harden(&dir)?;
        Ok(Some(endpoint))
    }

    #[cfg(windows)]
    pub fn create_named(prefix: &str) -> io::Result<PrivateEndpoint> {
        Ok(PrivateEndpoint {
            address: format!(r"\\.\pipe\{prefix}-{}", random_hex(12)?),
            dir: None,
        })
    }
}

impl PrivateEndpoint {
    /// The private directory holding the socket (Unix); `None` for a named pipe.
    pub fn dir(&self) -> Option<&std::path::Path> {
        self.dir.as_deref()
    }
}

/// Defends against a permissive umask or an odd file system: the folder is owner-only whatever `mkdir` made of it.
#[cfg(unix)]
fn harden_dir(dir: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

impl Drop for PrivateEndpoint {
    fn drop(&mut self) {
        if let Some(d) = self.dir.take() {
            #[cfg(unix)]
            live().retain(|l| *l != d);
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_and_compared_exactly() {
        let a = new_token().unwrap();
        assert_eq!(a.len(), 64);
        assert_ne!(a, new_token().unwrap());
        assert!(tokens_equal(&a, &a.clone()));
        assert!(!tokens_equal(&a, &a[..63]) && !tokens_equal(&a, "") && !tokens_equal("", &a));
        assert!(tokens_equal("", ""));
        assert!(!tokens_equal("abc", "abd"));
    }

    #[cfg(unix)]
    #[test]
    fn the_socket_directory_is_private_and_removed_on_drop() {
        use std::os::unix::fs::PermissionsExt;
        let e = PrivateEndpoint::create().unwrap();
        let dir = std::path::Path::new(&e.address).parent().unwrap().to_path_buf();
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        assert!(e.address.ends_with("w.sock") && e.address.len() < 100);
        assert!(dir.file_name().unwrap().to_string_lossy().starts_with("debrief-ipc-"));
        let other = PrivateEndpoint::create_named("debrief-x").unwrap();
        assert_ne!(e.address, other.address);
        drop(e);
        assert!(!dir.exists());
    }

    /// Regression (macOS CI temp-leak check, `debrief-plugin-*`): a worker stopped through its lifeline while it started a
    /// plugin ended with `process::exit`, which runs no destructors, so the plugin's endpoint folder stayed.
    #[cfg(unix)]
    #[test]
    fn remove_live_removes_the_folders_of_endpoints_still_alive() {
        // Made where the product makes it (`create_named` takes the first base whose socket path fits): CI's per-job temp
        // folder is too long for a socket, so an endpoint made directly in a temp dir there does not fit.
        let e = PrivateEndpoint::create_named("debrief-live").unwrap();
        let dir = e.dir().unwrap().to_path_buf();
        assert!(dir.is_dir());
        remove_live();
        assert!(!dir.exists(), "the folder is gone without the endpoint being dropped");
        drop(e); // dropping afterwards is harmless
    }

    /// Regression: a step after `mkdir` that fails (the permission hardening) left the new folder behind, unowned.
    #[cfg(unix)]
    #[test]
    fn a_failing_step_after_making_the_folder_removes_it_again() {
        let base = tempfile::tempdir().unwrap();
        let failing: fn(&std::path::Path) -> io::Result<()> = |_| Err(io::Error::other("cannot harden"));
        assert!(PrivateEndpoint::create_in(base.path(), "debrief-x", failing).is_err());
        assert_eq!(std::fs::read_dir(base.path()).unwrap().count(), 0, "no folder is left behind");
        let e = PrivateEndpoint::create_in(base.path(), "debrief-x", harden_dir).unwrap().unwrap();
        assert!(e.dir().unwrap().is_dir());
    }
}
