//! Starting a browser: the command line, a throwaway profile and the supervised process with its time and memory
//! caps. The real user agent and the automation banner are kept on purpose: no stealth.

use crate::answer::{self, Life};
use crate::cdp::Connection;
use crate::discover::{Browser, Flavor};
use crate::page::Page;
use crate::process::{self, Supervised};
use crate::{Error, Result};
use paddock_proc::{ExitInfo, Liveness, Patience, ProcessId};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
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
    /// Extra command-line flags. Empty in the product; tests use it for the root-in-a-container switches
    /// (see `discover::test_browser_flags_with`).
    pub extra_args: Vec<String>,
    /// How the browser's answers to the commands that open a page are judged ([`crate::answer`]): while it is alive and
    /// working, never for a fixed time; `max_lifetime` is the overall bound.
    pub patience: Patience,
    /// The longest a browser may take to become ready (its DevTools endpoint). The wait ends earlier when the browser is
    /// ready or its process has exited; this is only the guard for a start that hangs. A product render keeps
    /// [`START_LIMIT`]; a test on a shared, starved host passes a long guard (ADR 0071).
    pub start_limit: Duration,
}

impl LaunchOptions {
    /// Defaults: off screen, 2 minutes, 2 GiB.
    pub fn new(profile_base: impl Into<PathBuf>) -> LaunchOptions {
        LaunchOptions {
            profile_base: profile_base.into(),
            mode: Mode::OffScreen,
            max_lifetime: Duration::from_secs(120),
            memory_cap: Some(2 << 30),
            extra_args: Vec::new(),
            patience: Patience::default(),
            start_limit: START_LIMIT,
        }
    }
}

/// Whether a window can be shown: headful modes need an X server or a Wayland compositor on Linux (a headless
/// server, an SSH session). Other platforms always have a window system. Pure: `env` reads one variable.
pub fn check_display(mode: Mode, linux: bool, env: impl Fn(&str) -> Option<String>) -> Result<()> {
    let set = |k: &str| env(k).is_some_and(|v| !v.trim().is_empty());
    if !linux || mode == Mode::Headless || set("DISPLAY") || set("WAYLAND_DISPLAY") {
        return Ok(());
    }
    Err(Error::Launch(
        "there is no display to open the browser on (neither DISPLAY nor WAYLAND_DISPLAY is set); run Spotter in a desktop session, \
         or on a server start it under a virtual display, for example `xvfb-run spotter`"
            .into(),
    ))
}

/// The browser's stderr is kept in this file inside the throwaway profile, so a failed start can say why.
const STDERR_FILE: &str = "browser-stderr.log";
/// At most this many bytes from the end of the browser's stderr go into an error message.
const STDERR_TAIL: usize = 600;

/// The last part of the browser's stderr as one bounded, printable line (empty when there is none).
pub fn stderr_tail(bytes: &[u8]) -> String {
    let start = bytes.len().saturating_sub(STDERR_TAIL);
    let text = String::from_utf8_lossy(&bytes[start..]);
    let lines: Vec<String> = text
        .lines()
        .map(|l| l.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    lines.join(" | ")
}

/// The last [`STDERR_TAIL`] bytes of the browser's stderr file: it seeks there and reads no more, however large the file grew.
fn read_stderr_tail(dir: &Path) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(dir.join(STDERR_FILE)).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(STDERR_TAIL as u64))).ok()?;
    let mut bytes = Vec::with_capacity(STDERR_TAIL);
    f.take(STDERR_TAIL as u64).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn with_stderr(message: &str, dir: &Path) -> String {
    let tail = read_stderr_tail(dir).map(|b| stderr_tail(&b)).unwrap_or_default();
    if tail.is_empty() {
        message.to_string()
    } else {
        format!("{message} (browser output: {tail})")
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
];

/// Browser features every browser starts without, in the one `--disable-features` switch of [`args`].
pub const STABILITY_DISABLED_FEATURES: &[&str] = &[
    "Translate",
    "OptimizationHints",
    "MediaRouter",
    "AutofillServerCommunication",
    "msEdgeShopping",
    "msEdgeSync",
];

/// Keeps a Chromium build (Chrome for Testing) from applying the field-trial testing config it carries, as Puppeteer and
/// Playwright do; branded Chrome and Edge never apply it. That config turns on the network-service sandbox on Windows,
/// which could not start for a browser in a user folder on Paddock's Windows CI (all 12 Chrome logs of runs 7149 and
/// 7154), and real-browser loads ended in `net::ERR_ABORTED` in those runs (paddock-chromium's `NO_TESTING_FIELD_TRIALS`).
pub const NO_TESTING_FIELD_TRIALS: &str = "--disable-field-trial-config";

const DISABLE_FEATURES: &str = "--disable-features=";

/// The command-line arguments for `flavor`, the caller's `extra` included and `about:blank` last. `--enable-automation`
/// keeps Chrome's "controlled by automated test software" banner and `navigator.webdriver = true`; nothing hides either.
/// No extensions, no sync. Every feature to turn off is in one `--disable-features` switch, the caller's merged in.
pub fn args(data_dir: &Path, mode: Mode, flavor: Flavor, extra: &[String]) -> Vec<String> {
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
    let mut disabled: Vec<String> = Vec::new();
    let caller = extra
        .iter()
        .filter_map(|x| x.strip_prefix(DISABLE_FEATURES))
        .flat_map(|l| l.split(','));
    for f in STABILITY_DISABLED_FEATURES.iter().copied().chain(caller).map(str::trim) {
        if !f.is_empty() && !disabled.iter().any(|d| d == f) {
            disabled.push(f.to_string());
        }
    }
    a.push(format!("{DISABLE_FEATURES}{}", disabled.join(",")));
    if flavor == Flavor::Chromium {
        a.push(NO_TESTING_FIELD_TRIALS.into());
    }
    a.extend(
        extra
            .iter()
            .filter(|x| x.as_str() != "about:blank" && !x.starts_with(DISABLE_FEATURES))
            .cloned(),
    );
    a.push("about:blank".into());
    a
}

/// How long the browser may take to start (before the page's own time budget begins).
pub const START_LIMIT: Duration = Duration::from_secs(20);

/// The wall-clock cap for a render whose page gets `page_budget`: the start allowance, the page and a margin.
pub fn lifetime_for(page_budget: Duration) -> Duration {
    lifetime_with(START_LIMIT, page_budget)
}

/// [`lifetime_for`] with a start allowance other than [`START_LIMIT`] (a test's start guard).
pub fn lifetime_with(start_limit: Duration, page_budget: Duration) -> Duration {
    start_limit + page_budget + Duration::from_secs(10)
}

/// Runs `exe --version` to its end and returns how long it took: the first execution of a browser bundle on this machine,
/// made on its own so that a start that follows is not charged for it (ADR 0071; the same function as `paddock-chromium`'s,
/// both over [`paddock_proc::warm_up`]).
///
/// On macOS the first run of a freshly installed bundle is held before its first instruction while the OS assesses it
/// (XProtect, syspolicyd): measured 4.2 to 42.06 s for Chrome for Testing 156 on an idle Mac mini (docs/performance.md, 2026-11-11), with no CPU used by the
/// browser and no child process, against 0.4 to 0.7 s for every later start; a loaded CI host stretched it past a 120 s start
/// guard (CI runs 7918, 7929 and 7932, always the first test of the binary). A process in that state cannot be judged by
/// its CPU or its output, so this waits for the process to end, with no time limit of its own: the caller's own bound (a test
/// runner's slow-timeout) is the only guard. Failure says the exit status and the browser's output.
///
/// Public, with no product caller yet: it serves the tests until the product's first-start decision (Spotter warms the browser
/// up right after downloading it, in a "preparing the browser" step without a time limit; queued, not built).
///
/// macOS only: it returns `Ok(Duration::ZERO)` without starting anything elsewhere, because on Windows `chrome --version`
/// starts the browser (a window) and on Linux the first run is not held.
pub fn warm_up(exe: &Browser) -> Result<Duration> {
    if !cfg!(target_os = "macos") {
        return Ok(Duration::ZERO);
    }
    paddock_proc::warm_up(&exe.path, &["--version"]).map_err(Error::Launch)
}

/// How long a browser gets to quit by itself after `Browser.close` before it is killed.
pub const CLOSE_GRACE: Duration = Duration::from_secs(5);

const NONE: u8 = 0;
const TIMED_OUT: u8 = 1;
/// How often the supervisor looks at the browser's memory.
const MEMORY_LOOK: Duration = Duration::from_millis(500);

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
    /// Behind a lock so that a page can be opened through `&self` while the process is looked at ([`BrowserLife`]).
    process: Mutex<Supervised>,
    dir: PathBuf,
    alive: Arc<AtomicBool>,
    ended_by: Arc<AtomicU8>,
    patience: Patience,
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
    #[expect(
        clippy::disallowed_methods,
        reason = "the browser, a GUI program: std Command, as the driver is copied between apps"
    )]
    pub fn launch(exe: &Browser, opts: &LaunchOptions) -> Result<Chromium> {
        check_display(opts.mode, cfg!(target_os = "linux"), |k| std::env::var(k).ok())?;
        let dir = unique_dir(&opts.profile_base);
        std::fs::create_dir_all(&dir).map_err(|e| Error::Launch(format!("the profile folder could not be created: {e}")))?;
        let mut cmd = std::process::Command::new(&exe.path);
        let stderr = std::fs::File::create(dir.join(STDERR_FILE)).map_or_else(|_| std::process::Stdio::null(), std::process::Stdio::from);
        cmd.args(args(&dir, opts.mode, exe.flavor, &opts.extra_args))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(stderr);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(paddock_proc::CREATE_NO_WINDOW);
        }
        // The memory cap is watched by the shared supervisor from the process start, also while it is still starting: it
        // reads the pids of the tree only (on Windows the members of the child's job object), never the process table.
        let spawned = match opts.memory_cap {
            Some(limit) => {
                let mut cap = paddock_proc::MemoryCap::new(limit, "the browser");
                cap.every = MEMORY_LOOK;
                Supervised::spawn_capped(cmd, cap)
            }
            None => Supervised::spawn(cmd),
        };
        let mut process = match spawned {
            Ok(p) => p,
            Err(e) => {
                cleanup(&dir);
                return Err(Error::Launch(e.to_string()));
            }
        };
        let alive = Arc::new(AtomicBool::new(true));
        let ended_by = Arc::new(AtomicU8::new(NONE));
        // The caps run from the process start, also while it is still starting.
        watchdog(process.process(), alive.clone(), ended_by.clone(), opts.max_lifetime);
        match connect(&dir, &mut process, opts.start_limit) {
            Ok(conn) => Ok(Chromium {
                conn: Arc::new(conn),
                process: Mutex::new(process),
                dir,
                alive,
                ended_by,
                patience: opts.patience,
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
        lock(&self.process).id()
    }

    /// Whether the browser runs and the connection is open.
    pub fn is_running(&mut self) -> bool {
        lock(&self.process).is_running() && !self.conn.is_closed()
    }

    /// The throwaway profile folder.
    pub fn profile_dir(&self) -> &Path {
        &self.dir
    }

    /// Who ended the browser, if the watchdog did.
    pub fn ended_by(&self) -> EndedBy {
        match self.ended_by.load(Ordering::Acquire) {
            TIMED_OUT => EndedBy::TimeLimit,
            _ if lock(&self.process).stopped_over_memory().is_some() => EndedBy::MemoryCap,
            _ => EndedBy::Nothing,
        }
    }

    /// Opens a page (tab) to drive. Every command of the open (creating the tab, attaching to it, enabling its domains) is
    /// judged by the browser's liveness, never by a fixed deadline ([`crate::answer`], ADR 0071): it is waited for while the
    /// process runs and its tree uses CPU, and fails, naming the command, the time waited, the pid and the reason, once the
    /// browser ended or stopped working. [`LaunchOptions::max_lifetime`]'s watchdog stays the overall bound.
    pub fn new_page(&self) -> Result<Page> {
        let id = lock(&self.process).process();
        let life = RefCell::new(BrowserLife {
            process: &self.process,
            dir: &self.dir,
            id,
        });
        Page::attach(self.conn.clone(), &|session, method, params| {
            answer::ask(&self.conn, &life, self.patience, session, method, params)
        })
    }

    /// Asks the browser to quit (`Browser.close` over DevTools) and waits up to [`CLOSE_GRACE`] for the process to
    /// end, then ends whatever is left. A browser that is killed while it still starts up can leave a copy of
    /// itself behind on macOS (the code-sign clone), so every normal end goes through here. Idempotent.
    pub fn close(&mut self) {
        if self.alive.load(Ordering::Acquire) && lock(&self.process).is_running() && !self.conn.is_closed() {
            let _ = self.conn.call(None, "Browser.close", serde_json::json!({}), Duration::from_secs(2));
            let end = Instant::now() + CLOSE_GRACE;
            while lock(&self.process).is_running() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        self.alive.store(false, Ordering::Release);
        lock(&self.process).terminate(Duration::from_millis(500));
    }

    /// Arms a wall-clock cap that runs from now, in addition to [`LaunchOptions::max_lifetime`] (which runs from the spawn):
    /// for a caller that wants a limit on the browser's life after its start, however long the start took.
    #[doc(hidden)] // no product caller: the tests arm a cap after a started browser; hidden until one needs it
    pub fn limit_lifetime(&self, max: Duration) {
        watchdog(lock(&self.process).process(), self.alive.clone(), self.ended_by.clone(), max);
    }

    /// Ends the browser at once.
    pub fn kill(&mut self) {
        self.alive.store(false, Ordering::Release);
        lock(&self.process).kill();
    }
}

impl Drop for Chromium {
    fn drop(&mut self) {
        self.close();
        cleanup(&self.dir);
    }
}

/// The supervised process; a panic elsewhere while it was held does not stop the browser from being looked at or ended.
fn lock(process: &Mutex<Supervised>) -> MutexGuard<'_, Supervised> {
    process.lock().unwrap_or_else(|e| e.into_inner())
}

/// The running browser, as [`crate::answer`] looks at it while an answer is owed.
struct BrowserLife<'a> {
    process: &'a Mutex<Supervised>,
    dir: &'a Path,
    id: Option<ProcessId>,
}

impl Life for BrowserLife<'_> {
    fn liveness(&mut self) -> Liveness {
        if lock(self.process).is_running() {
            return Liveness::Running;
        }
        let stderr_tail = read_stderr_tail(self.dir).map(|b| stderr_tail(&b)).filter(|t| !t.is_empty());
        Liveness::Exited(ExitInfo { status: None, stderr_tail })
    }

    fn pid(&self) -> Option<ProcessId> {
        self.id
    }

    fn cpu_time(&mut self) -> Option<Duration> {
        paddock_proc::tree_cpu_time(self.id?)
    }
}

/// Ends the tree when the time is up, even if nobody calls anything. Stops when `alive` is cleared.
fn watchdog(root: Option<paddock_proc::ProcessId>, alive: Arc<AtomicBool>, ended_by: Arc<AtomicU8>, max: Duration) {
    let Some(root) = root else { return };
    let end = Instant::now() + max;
    let _ = std::thread::Builder::new().name("render-watchdog".into()).spawn(move || {
        while alive.load(Ordering::Acquire) {
            let reason = if Instant::now() >= end { Some(TIMED_OUT) } else { None };
            if let Some(r) = reason {
                // By identity (ADR 0033): a pid handed on after the browser ended is left alone.
                if alive.swap(false, Ordering::AcqRel) {
                    ended_by.store(r, Ordering::Release);
                    process::kill_tree(root);
                }
                return;
            }
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

/// Waits for the `DevToolsActivePort` file Chrome writes into the profile and connects to the browser endpoint. The wait
/// ends when the browser is ready or its process has exited (with its exit status and the end of its error output); a
/// browser that is alive but silent is waited for up to `limit`, the guard for a hung start (ADR 0071).
fn connect(dir: &Path, process: &mut Supervised, limit: Duration) -> Result<Connection> {
    let began = Instant::now();
    let end = began + limit;
    loop {
        if let Ok(s) = std::fs::read_to_string(dir.join("DevToolsActivePort")) {
            let mut l = s.lines();
            if let (Some(port), Some(path)) = (l.next().and_then(|p| p.trim().parse::<u16>().ok()), l.next()) {
                return Connection::connect(port, path.trim());
            }
        }
        if !process.is_running() {
            // Not `wait`: that forgets the tree, and helpers an exited browser left behind are ended by the caller's kill.
            let status = process
                .exit_status()
                .map_or_else(|| "exit status unknown".to_string(), |s| s.to_string());
            return Err(Error::Launch(with_stderr(
                &format!("the browser exited right after it started ({status})"),
                dir,
            )));
        }
        if Instant::now() > end {
            let pid = process.id().map_or_else(|| "unknown".to_string(), |p| p.to_string());
            return Err(Error::Launch(with_stderr(
                &format!(
                    "the browser (pid {pid}) was still running but not ready after {:.0} s (the start guard); it is ended now",
                    began.elapsed().as_secs_f64()
                ),
                dir,
            )));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unit tests start fake browsers: no window system is needed for them.
    fn headless(base: &Path) -> LaunchOptions {
        let mut o = LaunchOptions::new(base);
        o.mode = Mode::Headless;
        o
    }

    #[test]
    fn keeps_the_automation_banner_and_hides_nothing() {
        let a = args(Path::new("/tmp/p"), Mode::OffScreen, Flavor::Chromium, &[]);
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
            let a = args(Path::new("/p"), mode, Flavor::Chromium, &[]);
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
    fn stderr_tail_is_bounded_single_line_and_printable() {
        assert_eq!(stderr_tail(b""), "");
        assert_eq!(stderr_tail(b"one\n\n  two \x07\n"), "one | two");
        let long = "x".repeat(5000);
        assert!(stderr_tail(long.as_bytes()).len() <= STDERR_TAIL);
        assert_eq!(stderr_tail(&[0xff, b'a']), "\u{fffd}a");
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_start_reports_the_browsers_stderr() {
        use std::os::unix::fs::PermissionsExt;
        let base = tempfile::tempdir().unwrap();
        let exe_path = base.path().join("fake-chrome.sh");
        std::fs::write(
            &exe_path,
            "#!/bin/sh\necho 'Running as root without --no-sandbox is not supported' >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&exe_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let exe = Browser {
            flavor: crate::discover::Flavor::Chromium,
            path: exe_path,
        };
        let e = Chromium::launch(&exe, &headless(base.path())).err().unwrap().to_string();
        assert!(
            e.contains("exited right after it started") && e.contains("without --no-sandbox"),
            "{e}"
        );
    }

    /// A Chromium build starts without its testing field trials, branded Chrome and Edge get no switch; the caller's
    /// arguments come before `about:blank` and their features join the one `--disable-features` switch.
    #[test]
    fn chromium_builds_start_without_testing_field_trials_and_extras_merge() {
        let extra = vec![
            "--disable-features=Foo,Translate".to_string(),
            "--lang=de".to_string(),
            "about:blank".to_string(),
        ];
        for flavor in [Flavor::Chromium, Flavor::Chrome, Flavor::Edge] {
            let a = args(Path::new("/p"), Mode::Headless, flavor, &extra);
            assert_eq!(
                a.contains(&NO_TESTING_FIELD_TRIALS.to_string()),
                flavor == Flavor::Chromium,
                "{a:?}"
            );
            assert_eq!(a.last().map(String::as_str), Some("about:blank"), "{a:?}");
            assert_eq!(a.iter().filter(|x| *x == "about:blank").count(), 1, "{a:?}");
            assert!(a.contains(&"--lang=de".to_string()));
            let features: Vec<&String> = a.iter().filter(|x| x.starts_with("--disable-features=")).collect();
            assert_eq!(
                features,
                [
                    &"--disable-features=Translate,OptimizationHints,MediaRouter,AutofillServerCommunication,msEdgeShopping,msEdgeSync,Foo"
                        .to_string()
                ],
                "{a:?}"
            );
        }
    }

    #[test]
    fn extra_arguments_are_not_part_of_the_default_command_line() {
        assert!(LaunchOptions::new("/p").extra_args.is_empty());
        assert!(!args(Path::new("/p"), Mode::Headless, Flavor::Chromium, &[])
            .iter()
            .any(|a| a.contains("no-sandbox")));
    }

    #[test]
    fn modes() {
        assert!(!args(Path::new("/p"), Mode::OnScreen, Flavor::Chromium, &[])
            .iter()
            .any(|x| x.starts_with("--window-position") || x.starts_with("--headless")));
        assert!(args(Path::new("/p"), Mode::Headless, Flavor::Chromium, &[]).contains(&"--headless=new".to_string()));
    }

    #[test]
    fn missing_executable_is_a_launch_error_and_leaves_no_profile() {
        let base = tempfile::tempdir().unwrap();
        let exe = Browser {
            flavor: crate::discover::Flavor::Chromium,
            path: "/nonexistent/chrome".into(),
        };
        let e = Chromium::launch(&exe, &headless(base.path())).err().unwrap();
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
        let e = Chromium::launch(&exe, &headless(base.path())).err().unwrap();
        assert!(matches!(e, Error::Launch(_)), "{e}");
        assert_eq!(std::fs::read_dir(base.path()).unwrap().count(), 0);
    }

    #[test]
    fn headful_modes_need_a_display_on_linux_and_say_what_to_do() {
        let none = |_: &str| None;
        let only = |name: &'static str, v: &'static str| move |k: &str| (k == name).then(|| v.to_string());
        for mode in [Mode::OffScreen, Mode::OnScreen] {
            let e = check_display(mode, true, none).unwrap_err().to_string();
            assert!(e.contains("no display") && e.contains("xvfb-run"), "{e}");
            assert!(check_display(mode, true, only("DISPLAY", ":0")).is_ok());
            assert!(check_display(mode, true, only("WAYLAND_DISPLAY", "wayland-0")).is_ok());
            assert!(check_display(mode, true, only("DISPLAY", "  ")).is_err(), "a blank DISPLAY is none");
            assert!(
                check_display(mode, false, none).is_ok(),
                "macOS and Windows always have a window system"
            );
        }
        assert!(check_display(Mode::Headless, true, none).is_ok());
    }

    #[test]
    fn profile_dirs_are_unique() {
        let b = Path::new("/b");
        assert_ne!(unique_dir(b), unique_dir(b));
    }

    /// A stand-in browser that is alive but never writes `DevToolsActivePort`: the start ends at the guard, says the
    /// browser was still running with its pid and output, and leaves no process and no profile behind.
    #[cfg(unix)]
    #[test]
    fn a_browser_that_never_becomes_ready_fails_at_the_guard_and_is_ended() {
        use std::os::unix::fs::PermissionsExt;
        let base = tempfile::tempdir().unwrap();
        let exe_path = base.path().join("silent-chrome.sh");
        std::fs::write(
            &exe_path,
            "#!/bin/sh\necho 'still warming up' >&2\necho $$ > \"$(dirname \"$0\")/pid\"\nexec sleep 300\n",
        )
        .unwrap();
        std::fs::set_permissions(&exe_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let exe = Browser {
            flavor: crate::discover::Flavor::Chromium,
            path: exe_path,
        };
        let mut o = headless(base.path());
        o.start_limit = Duration::from_secs(3);
        let e = Chromium::launch(&exe, &o).err().unwrap().to_string();
        assert!(
            e.contains("still running but not ready") && e.contains("pid ") && e.contains("still warming up"),
            "{e}"
        );
        // `exec sleep` drops the profile folder from the command line, so the process is found by the pid the script wrote.
        let pid: u32 = std::fs::read_to_string(base.path().join("pid"))
            .expect("the stand-in wrote no pid before the start guard fired")
            .trim()
            .parse()
            .unwrap();
        assert!(
            paddock_proc::ProcessId::of(pid).is_none_or(|id| !id.is_alive()),
            "the silent browser (pid {pid}) was left running"
        );
        assert_eq!(
            std::fs::read_dir(base.path())
                .unwrap()
                .filter(|d| d.as_ref().unwrap().path().is_dir())
                .count(),
            0
        );
    }

    /// A stand-in that exits at once ends the wait at once, however long the guard, with its exit status and output.
    #[cfg(unix)]
    #[test]
    fn a_browser_that_exits_ends_the_wait_with_its_status_not_at_the_guard() {
        use std::os::unix::fs::PermissionsExt;
        let base = tempfile::tempdir().unwrap();
        let exe_path = base.path().join("dying-chrome.sh");
        std::fs::write(&exe_path, "#!/bin/sh\necho 'out of memory' >&2\nexit 3\n").unwrap();
        std::fs::set_permissions(&exe_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let exe = Browser {
            flavor: crate::discover::Flavor::Chromium,
            path: exe_path,
        };
        let mut o = headless(base.path());
        o.start_limit = Duration::from_secs(120);
        let t = Instant::now();
        let e = Chromium::launch(&exe, &o).err().unwrap().to_string();
        assert!(t.elapsed() < Duration::from_secs(30), "waited {:?}", t.elapsed());
        assert!(e.contains("exited right after it started (exit status: 3)"), "{e}");
        assert!(e.contains("out of memory"), "{e}");
    }

    /// The browser exited with a helper still running (Chrome's crashpad, GPU or renderer processes outlive a browser that
    /// died): the failed start ends the whole tree, not only the leader.
    #[cfg(unix)]
    #[test]
    fn a_browser_that_exits_leaves_no_helper_behind() {
        use std::os::unix::fs::PermissionsExt;
        let base = tempfile::tempdir().unwrap();
        let exe_path = base.path().join("leaving-chrome.sh");
        std::fs::write(
            &exe_path,
            "#!/bin/sh\nsleep 300 >/dev/null 2>&1 &\necho $! > \"$(dirname \"$0\")/helper\"\nexit 3\n",
        )
        .unwrap();
        std::fs::set_permissions(&exe_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let exe = Browser {
            flavor: crate::discover::Flavor::Chromium,
            path: exe_path,
        };
        let e = Chromium::launch(&exe, &headless(base.path())).err().unwrap().to_string();
        assert!(e.contains("exit status: 3"), "{e}");
        let pid: u32 = std::fs::read_to_string(base.path().join("helper")).unwrap().trim().parse().unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        while paddock_proc::ProcessId::of(pid).is_some_and(|id| id.is_alive()) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(50)); // conformance: hang-guard — polling for the helper to be gone, bound 10 s
        }
        let helper = paddock_proc::ProcessId::of(pid);
        if let Some(id) = helper.filter(|id| id.is_alive()) {
            paddock_proc::kill_tree(id);
            panic!("the helper (pid {pid}) of an exited browser was left running");
        }
    }

    /// Only the end of a large stderr file is read.
    #[test]
    fn only_the_end_of_a_large_stderr_file_is_read() {
        let d = tempfile::tempdir().unwrap();
        let mut big = vec![b'x'; 3 << 20];
        big.extend_from_slice(b"\nthe real cause\n");
        std::fs::write(d.path().join(STDERR_FILE), &big).unwrap();
        let tail = read_stderr_tail(d.path()).unwrap();
        assert!(tail.len() <= STDERR_TAIL);
        assert!(with_stderr("failed", d.path()).ends_with("the real cause)"));
        assert!(read_stderr_tail(&d.path().join("none")).is_none());
    }

    /// A stand-in that takes its time before answering (the OS holding a first run): `warm_up` waits for it however long it
    /// takes, with no budget of its own, and reports how long it waited.
    #[cfg(target_os = "macos")]
    #[test]
    fn warm_up_waits_for_a_slow_first_run_without_a_budget() {
        use std::os::unix::fs::PermissionsExt;
        let base = tempfile::tempdir().unwrap();
        let exe_path = base.path().join("slow-chrome.sh");
        // conformance: hang-guard — the stand-in's own delay is 2 s; nothing in `warm_up` is a clock
        std::fs::write(&exe_path, "#!/bin/sh\nsleep 2\necho 'Fake Chrome 1.0'\n").unwrap();
        std::fs::set_permissions(&exe_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let exe = Browser {
            flavor: crate::discover::Flavor::Chromium,
            path: exe_path,
        };
        let took = warm_up(&exe).unwrap();
        assert!(took >= Duration::from_secs(2), "{took:?}");
    }

    /// A first run that fails says its exit status and output; one that cannot be run says why.
    #[cfg(target_os = "macos")]
    #[test]
    fn warm_up_failures_name_the_status_and_the_output() {
        use std::os::unix::fs::PermissionsExt;
        let base = tempfile::tempdir().unwrap();
        let exe_path = base.path().join("broken-chrome.sh");
        std::fs::write(&exe_path, "#!/bin/sh\necho 'bundle damaged' >&2\nexit 5\n").unwrap();
        std::fs::set_permissions(&exe_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let broken = Browser {
            flavor: crate::discover::Flavor::Chromium,
            path: exe_path,
        };
        let e = warm_up(&broken).unwrap_err().to_string();
        assert!(e.contains("exit status: 5") && e.contains("bundle damaged"), "{e}");
        let missing = Browser {
            flavor: crate::discover::Flavor::Chromium,
            path: base.path().join("nope"),
        };
        assert!(matches!(warm_up(&missing), Err(Error::Launch(_))));
    }

    #[test]
    fn the_lifetime_follows_the_start_guard() {
        assert_eq!(
            lifetime_with(START_LIMIT, Duration::from_secs(40)),
            lifetime_for(Duration::from_secs(40))
        );
        assert_eq!(lifetime_with(Duration::from_secs(120), Duration::ZERO), Duration::from_secs(130));
    }
}
