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

/// Flags that keep a throwaway browser quick and predictable on a cold, loaded machine (a CI runner, a fresh VM):
/// no GPU process (a virtual machine without a GPU spends many seconds on failed GPU start-ups), no component or
/// field-trial downloads, no crash reporter, and no throttling of a window nobody can see (an off-screen window is
/// "occluded", which delays timers and the load of its page). None of them changes what a page can detect about
/// automation.
pub const STABILITY_FLAGS: &[&str] = &[
    "--disable-gpu",
    "--disable-component-update",
    "--disable-default-apps",
    "--disable-domain-reliability",
    "--disable-breakpad",
    "--disable-hang-monitor",
    "--metrics-recording-only",
    "--no-service-autorun",
    "--disable-background-timer-throttling",
    "--disable-renderer-backgrounding",
    "--disable-backgrounding-occluded-windows",
    "--disable-features=Translate,OptimizationHints,MediaRouter,AutofillServerCommunication,msEdgeShopping,msEdgeSync",
];

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
    a.extend(STABILITY_FLAGS.iter().map(|f| f.to_string()));
    match mode {
        Mode::OffScreen => a.push("--window-position=-32000,-32000".into()),
        Mode::Headless => a.push("--headless=new".into()),
        Mode::OnScreen => {}
    }
    a.push("about:blank".into());
    a
}

/// How long the browser may take to start (before the page's own time budget begins).
pub const START_LIMIT: Duration = Duration::from_secs(20);

/// The wall-clock cap for a render whose page gets `page_budget`: the start allowance, the page and a margin.
pub fn lifetime_for(page_budget: Duration) -> Duration {
    START_LIMIT + page_budget + Duration::from_secs(10)
}

/// How long a browser gets to quit by itself after `Browser.close` before it is killed.
pub const CLOSE_GRACE: Duration = Duration::from_secs(5);

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
                process.terminate(CLOSE_GRACE);
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

    /// Asks the browser to quit (`Browser.close` over DevTools) and waits up to [`CLOSE_GRACE`] for the process to
    /// end, then ends whatever is left. A browser that is killed while it still starts up can leave a copy of
    /// itself behind on macOS (the code-sign clone), so every normal end goes through here. Idempotent.
    pub fn close(&mut self) {
        if self.alive.load(Ordering::Acquire) && self.process.is_running() && !self.conn.is_closed() {
            let _ = self.conn.call(None, "Browser.close", serde_json::json!({}), Duration::from_secs(2));
            let end = Instant::now() + CLOSE_GRACE;
            while self.process.is_running() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        self.alive.store(false, Ordering::Release);
        self.process.terminate(Duration::from_millis(500));
    }

    /// Ends the browser at once.
    pub fn kill(&mut self) {
        self.alive.store(false, Ordering::Release);
        self.process.kill();
    }
}

impl Drop for Chromium {
    fn drop(&mut self) {
        self.close();
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
    let end = Instant::now() + START_LIMIT;
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
    fn carries_the_stability_flags_in_every_mode() {
        for mode in [Mode::OffScreen, Mode::Headless, Mode::OnScreen] {
            let a = args(Path::new("/p"), mode);
            for f in [
                "--disable-gpu",
                "--disable-component-update",
                "--disable-backgrounding-occluded-windows",
            ] {
                assert!(a.contains(&f.to_string()), "{mode:?} lacks {f}");
            }
            assert!(
                a.iter().filter(|x| x.starts_with("--disable-features=")).count() == 1,
                "one --disable-features only"
            );
            assert_eq!(a.last().map(String::as_str), Some("about:blank"));
        }
    }

    #[test]
    fn the_wall_clock_cap_covers_start_and_page() {
        assert_eq!(lifetime_for(Duration::from_secs(40)), Duration::from_secs(70));
        assert!(lifetime_for(Duration::ZERO) > START_LIMIT);
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
