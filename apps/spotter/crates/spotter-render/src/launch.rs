//! Starting a browser: the command line, a throwaway profile and the supervised process with its time and memory
//! caps. The real user agent and the automation banner are kept on purpose: no stealth.

use crate::cdp::Connection;
use crate::discover::Browser;
use crate::page::Page;
use crate::process::{self, Supervised};
use crate::{Error, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How the browser window behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Headful but moved off screen (the default: behaves like a normal browser).
    OffScreen,
    /// New headless (`--headless=new`), where it is reliable.
    Headless,
    /// A normal window (to let the user solve a challenge).
    OnScreen,
}

/// Launch parameters.
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    /// The throwaway profile is created below this folder and deleted afterwards.
    pub profile_base: PathBuf,
    /// Window behaviour.
    pub mode: Mode,
    /// The browser tree is killed after this long, whatever it does.
    pub max_lifetime: Duration,
    /// The browser tree is killed when its resident memory exceeds this many bytes.
    pub memory_cap: Option<u64>,
}

impl LaunchOptions {
    /// Defaults: off screen, 2 minutes, 2 GiB.
    pub fn new(profile_base: impl Into<PathBuf>) -> LaunchOptions {
        LaunchOptions {
            profile_base: profile_base.into(),
            mode: Mode::OffScreen,
            max_lifetime: Duration::from_secs(120),
            memory_cap: Some(2 << 30),
        }
    }
}

/// The command-line arguments. `--enable-automation` keeps Chrome's "controlled by automated test software"
/// banner and `navigator.webdriver = true`; nothing hides either. No extensions, no sync.
pub fn args(data_dir: &Path, mode: Mode) -> Vec<String> {
    let mut a = vec![
        "--remote-debugging-port=0".to_string(),
        format!("--user-data-dir={}", data_dir.display()),
        "--enable-automation".into(),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--disable-sync".into(),
        "--disable-extensions".into(),
        "--disable-background-networking".into(),
        "--password-store=basic".into(),
        "--use-mock-keychain".into(),
        "--window-size=1100,800".into(),
    ];
    match mode {
        Mode::OffScreen => a.push("--window-position=-32000,-32000".into()),
        Mode::Headless => a.push("--headless=new".into()),
        Mode::OnScreen => {}
    }
    a.push("about:blank".into());
    a
}

const NONE: u8 = 0;
const TIMED_OUT: u8 = 1;
const OVER_MEMORY: u8 = 2;

/// Why the watchdog ended the browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndedBy {
    /// It is still running or ended by itself or by us.
    Nothing,
    /// The wall-clock limit.
    TimeLimit,
    /// The memory cap.
    MemoryCap,
}

/// A running browser. Dropping it ends the process tree and removes the profile.
pub struct Chromium {
    // Field order matters: the connection closes before the process is killed.
    conn: Arc<Connection>,
    process: Supervised,
    dir: PathBuf,
    alive: Arc<AtomicBool>,
    ended_by: Arc<AtomicU8>,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_dir(base: &Path) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    base.join(format!(
        "spotter-render-{}-{n}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

impl Chromium {
    /// Starts `exe` with a fresh profile and connects to it.
    pub fn launch(exe: &Browser, opts: &LaunchOptions) -> Result<Chromium> {
        let dir = unique_dir(&opts.profile_base);
        std::fs::create_dir_all(&dir).map_err(|e| Error::Launch(format!("the profile folder could not be created: {e}")))?;
        let mut cmd = std::process::Command::new(&exe.path);
        cmd.args(args(&dir, opts.mode))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut process = match Supervised::spawn(cmd) {
            Ok(p) => p,
            Err(e) => {
                cleanup(&dir);
                return Err(Error::Launch(e.to_string()));
            }
        };
        let alive = Arc::new(AtomicBool::new(true));
        let ended_by = Arc::new(AtomicU8::new(NONE));
        // The caps run from the process start, also while it is still starting.
        watchdog(process.id(), alive.clone(), ended_by.clone(), opts.max_lifetime, opts.memory_cap);
        match connect(&dir, &mut process) {
            Ok(conn) => Ok(Chromium {
                conn: Arc::new(conn),
                process,
                dir,
                alive,
                ended_by,
            }),
            Err(e) => {
                alive.store(false, Ordering::Release);
                process.kill();
                cleanup(&dir);
                Err(e)
            }
        }
    }

    /// The process id (while running).
    pub fn pid(&self) -> Option<u32> {
        self.process.id()
    }

    /// Whether the browser runs and the connection is open.
    pub fn is_running(&mut self) -> bool {
        self.process.is_running() && !self.conn.is_closed()
    }

    /// The throwaway profile folder.
    pub fn profile_dir(&self) -> &Path {
        &self.dir
    }

    /// Who ended the browser, if the watchdog did.
    pub fn ended_by(&self) -> EndedBy {
        match self.ended_by.load(Ordering::Acquire) {
            TIMED_OUT => EndedBy::TimeLimit,
            OVER_MEMORY => EndedBy::MemoryCap,
            _ => EndedBy::Nothing,
        }
    }

    /// Opens a page (tab) to drive.
    pub fn new_page(&self) -> Result<Page> {
        Page::attach(self.conn.clone())
    }

    /// Ends the browser at once.
    pub fn kill(&mut self) {
        self.alive.store(false, Ordering::Release);
        self.process.kill();
    }
}

impl Drop for Chromium {
    fn drop(&mut self) {
        self.kill();
        cleanup(&self.dir);
    }
}

/// Ends the tree when time or memory is up, even if nobody calls anything. Stops when `alive` is cleared.
fn watchdog(pid: Option<u32>, alive: Arc<AtomicBool>, ended_by: Arc<AtomicU8>, max: Duration, cap: Option<u64>) {
    let Some(pid) = pid else { return };
    let end = Instant::now() + max;
    let _ = std::thread::Builder::new().name("render-watchdog".into()).spawn(move || {
        let mut tick = 0u32;
        while alive.load(Ordering::Acquire) {
            let reason = if Instant::now() >= end {
                Some(TIMED_OUT)
            } else if tick % 4 == 3 && cap.is_some_and(|c| process::current_tree_memory(pid) > c) {
                Some(OVER_MEMORY)
            } else {
                None
            };
            if let Some(r) = reason {
                // Re-check right before killing so a reused pid is never signalled after a normal end.
                if alive.swap(false, Ordering::AcqRel) {
                    ended_by.store(r, Ordering::Release);
                    process::kill_tree(pid);
                }
                return;
            }
            tick = tick.wrapping_add(1);
            std::thread::sleep(Duration::from_millis(250));
        }
    });
}

fn cleanup(dir: &Path) {
    // Chrome may still be flushing for a moment after it was killed.
    for _ in 0..20 {
        if std::fs::remove_dir_all(dir).is_ok() || !dir.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Waits for the `DevToolsActivePort` file Chrome writes into the profile and connects to the browser endpoint.
fn connect(dir: &Path, process: &mut Supervised) -> Result<Connection> {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(s) = std::fs::read_to_string(dir.join("DevToolsActivePort")) {
            let mut l = s.lines();
            if let (Some(port), Some(path)) = (l.next().and_then(|p| p.trim().parse::<u16>().ok()), l.next()) {
                return Connection::connect(port, path.trim());
            }
        }
        if !process.is_running() {
            return Err(Error::Launch("the browser exited right after it started".into()));
        }
        if Instant::now() > end {
            return Err(Error::Timeout("the browser start"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_automation_banner_and_hides_nothing() {
        let a = args(Path::new("/tmp/p"), Mode::OffScreen);
        assert!(a.contains(&"--enable-automation".to_string()));
        assert!(a
            .iter()
            .all(|x| !x.contains("AutomationControlled") && !x.contains("user-agent") && !x.contains("headless")));
        assert!(a.contains(&"--window-position=-32000,-32000".to_string()));
        assert!(a.contains(&"--user-data-dir=/tmp/p".to_string()));
        assert!(a.contains(&"--disable-extensions".to_string()));
    }

    #[test]
    fn modes() {
        assert!(!args(Path::new("/p"), Mode::OnScreen)
            .iter()
            .any(|x| x.starts_with("--window-position") || x.starts_with("--headless")));
        assert!(args(Path::new("/p"), Mode::Headless).contains(&"--headless=new".to_string()));
    }

    #[test]
    fn missing_executable_is_a_launch_error_and_leaves_no_profile() {
        let base = tempfile::tempdir().unwrap();
        let exe = Browser {
            flavor: crate::discover::Flavor::Chromium,
            path: "/nonexistent/chrome".into(),
        };
        let e = Chromium::launch(&exe, &LaunchOptions::new(base.path())).err().unwrap();
        assert!(matches!(e, Error::Launch(_)));
        assert_eq!(std::fs::read_dir(base.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_browser_that_exits_at_once_is_a_launch_error() {
        let base = tempfile::tempdir().unwrap();
        let exe = Browser {
            flavor: crate::discover::Flavor::Chromium,
            path: "/usr/bin/true".into(),
        };
        let e = Chromium::launch(&exe, &LaunchOptions::new(base.path())).err().unwrap();
        assert!(matches!(e, Error::Launch(_)), "{e}");
        assert_eq!(std::fs::read_dir(base.path()).unwrap().count(), 0);
    }

    #[test]
    fn profile_dirs_are_unique() {
        let b = Path::new("/b");
        assert_ne!(unique_dir(b), unique_dir(b));
    }
}
