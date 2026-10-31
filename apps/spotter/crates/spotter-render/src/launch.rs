//! Starting a browser: the command line, a throwaway profile and the supervised process with its time and memory
//! caps. The real user agent and the automation banner are kept on purpose: no stealth.

use crate::cdp::Connection;
use crate::discover::{Browser, Flavor};
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

/// Told, in plain words, that the browser was stopped by macOS or another app while it started and is being waited for
/// (ADR 0031: a stopped process is paused, not stuck, and the person is told).
#[derive(Clone)]
pub struct PauseNote(pub Arc<dyn Fn(PauseEvent) + Send + Sync>);

/// A change in whether the OS has the starting browser stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseEvent {
    /// Stopped; carries [`PAUSED_TEXT`].
    Paused,
    /// Running again (or the start ended): the "paused" line is over.
    Resumed,
}

impl std::fmt::Debug for PauseNote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PauseNote")
    }
}

/// Launch parameters.
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    /// Called with "The browser is paused by macOS or another app; Spotter waits" once per pause while the browser starts.
    pub on_paused: Option<PauseNote>,
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
    /// When a browser that has not opened its DevTools port is stuck rather than slow (ADR 0031): a slow or starved start is
    /// waited for. The wall-clock cap is [`LaunchOptions::max_lifetime`], which runs from the process start.
    pub patience: paddock_proc::Patience,
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
            patience: paddock_proc::Patience::default(),
            on_paused: None,
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

/// What the person is told while the browser is stopped by the OS or another app.
pub const PAUSED_TEXT: &str = "The browser is paused by macOS or another app; Spotter waits";

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

/// The start allowance inside the render's wall-clock cap ([`lifetime_for`]); a start is not failed at this time, it is
/// judged on the browser's run state (ADR 0031) and only the cap ends a slow one.
pub const START_ALLOWANCE: Duration = Duration::from_secs(20);

/// The wall-clock cap for a render whose page gets `page_budget`: the start allowance, the page and a margin.
pub fn lifetime_for(page_budget: Duration) -> Duration {
    START_ALLOWANCE + page_budget + Duration::from_secs(10)
}

/// How long a browser that failed to start gets to quit after a polite request before it is killed (the start failed, so
/// there is no run state to judge it by).
pub const CLOSE_GRACE: Duration = Duration::from_secs(5);

/// When a browser asked to quit (`Browser.close`) is stuck rather than slow (ADR 0031): a browser that is still using CPU is
/// flushing and is waited for; one that uses none and does not exit is given up on after `idle`. `spin` is CPU time summed
/// over cores and processes of the tree, so a multi-threaded flush reaches it faster than the wall clock.
pub const CLOSE_PATIENCE: paddock_proc::Patience = paddock_proc::Patience {
    spin: Duration::from_secs(60),
    idle: Duration::from_secs(5),
    hang_guard: Duration::from_secs(10),
    note_after: 0,
    tick: Duration::from_millis(50),
    cpu_every: 20,
};

/// When a self-test (`<browser> --version`) is stuck rather than slow: a first start that is scanned, paged in or loaded still uses
/// CPU or exits, and is waited for; one that uses none and does not exit for `idle` is hung (ADR 0031, no fixed time).
pub const SELF_TEST_PATIENCE: paddock_proc::Patience = paddock_proc::Patience {
    spin: Duration::from_secs(120),
    idle: Duration::from_secs(20),
    hang_guard: Duration::from_secs(300),
    note_after: 0,
    tick: Duration::from_millis(50),
    cpu_every: 20,
};

/// Waits until a short-lived program (a self-test) has exited, judged on its run state; `false` when it is stuck or the pace
/// ended the wait. `cpu` and `pace` are seams for tests.
pub(crate) fn wait_to_exit(
    process: &mut Supervised,
    cpu: impl FnMut(paddock_proc::ProcessId) -> Option<Duration>,
    pace: impl paddock_proc::Pace,
) -> bool {
    wait_to_quit(process, SELF_TEST_PATIENCE, cpu, pace) == Quit::Exited
}

/// The most a quit may take, however it looks: a browser with a throwaway profile is safe to end after it (the profile is
/// deleted).
// cap: a bound on something that cannot report progress beyond CPU time; the judge ends a stuck quit much earlier.
const CLOSE_CAP: Duration = Duration::from_secs(20);

/// How long the browser gets to react to a polite termination after its quit was waited for (or given up on), before the
/// tree is killed. It is only reached by a browser that did not quit by itself.
const TERMINATE_GRACE: Duration = Duration::from_millis(500);

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
    #[expect(
        clippy::disallowed_methods,
        reason = "the browser, a GUI program: std Command, as the driver is copied between apps"
    )]
    pub fn launch(exe: &Browser, opts: &LaunchOptions) -> Result<Chromium> {
        // Test builds: no installed browser, and no real one outside a throwaway session (rule 7), however it was found
        // (an installed one through `find_installed`, a download, a hard-coded path).
        #[cfg(any(test, feature = "testing"))]
        {
            use paddock_testkit::live_browser::BrowserWindow;
            let window = match opts.mode {
                Mode::Headless => BrowserWindow::Headless,
                Mode::OffScreen | Mode::OnScreen => BrowserWindow::Shown,
            };
            crate::discover::check_test_launch(&exe.path, window).map_err(Error::Launch)?;
        }
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
        match connect(
            &dir,
            &mut process,
            opts.patience,
            &ended_by,
            opts.max_lifetime,
            paddock_proc::tree_cpu_time,
            SleepPace,
            &mut |m| {
                if let Some(n) = &opts.on_paused {
                    (n.0)(m);
                }
            },
        ) {
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
            _ if self.process.stopped_over_memory().is_some() => EndedBy::MemoryCap,
            _ => EndedBy::Nothing,
        }
    }

    /// Opens a page (tab) to drive.
    pub fn new_page(&self) -> Result<Page> {
        Page::attach(self.conn.clone())
    }

    /// Asks the browser to quit (`Browser.close` over DevTools) and waits for the process to end, judged on its run state
    /// ([`CLOSE_PATIENCE`]: a slow quit that still uses CPU is waited for, a stuck one is not), then ends whatever is
    /// left. A browser that is killed while it still starts up can leave a copy of itself behind on macOS (the
    /// code-sign clone), so every normal end goes through here. Returns whether the browser quit by itself, before any
    /// termination or kill, and was not ended by the watchdog or the memory cap. Idempotent.
    pub fn close(&mut self) -> bool {
        let mut by_itself = !self.process.is_running();
        if self.alive.load(Ordering::Acquire) && self.process.is_running() && !self.conn.is_closed() {
            let _ = self.conn.call(None, "Browser.close", serde_json::json!({}), Duration::from_secs(2));
            by_itself = wait_to_quit(
                &mut self.process,
                CLOSE_PATIENCE,
                paddock_proc::tree_cpu_time,
                CappedSleep::new(CLOSE_CAP),
            ) == Quit::Exited;
        }
        self.alive.store(false, Ordering::Release);
        self.process.terminate(TERMINATE_GRACE);
        by_itself && self.ended_by() == EndedBy::Nothing
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

/// What the quit judge looks at: the browser process, ready when it is gone.
struct QuitProbe<'a, F> {
    process: &'a mut Supervised,
    id: Option<paddock_proc::ProcessId>,
    cpu: F,
}

impl<F: FnMut(paddock_proc::ProcessId) -> Option<Duration>> paddock_proc::StartProbe for QuitProbe<'_, F> {
    fn liveness(&mut self) -> paddock_proc::Liveness {
        if self.process.is_running() {
            paddock_proc::Liveness::Running
        } else {
            paddock_proc::Liveness::Exited(paddock_proc::ExitInfo {
                status: None,
                stderr_tail: None,
            })
        }
    }

    fn pid(&self) -> Option<paddock_proc::ProcessId> {
        self.id
    }

    fn ready(&mut self) -> bool {
        !self.process.is_running()
    }

    fn cpu_time(&mut self) -> Option<Duration> {
        (self.cpu)(self.id?)
    }
}

/// Waits for one tick on the real clock, and says the wait is over (cancelled) once `cap` has passed.
struct CappedSleep {
    end: Instant,
}

impl CappedSleep {
    fn new(cap: Duration) -> CappedSleep {
        CappedSleep { end: Instant::now() + cap }
    }
}

impl paddock_proc::Pace for CappedSleep {
    fn wait(&mut self, tick: Duration) -> bool {
        std::thread::sleep(tick);
        Instant::now() < self.end
    }
}

/// How a wait for the browser to quit ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quit {
    /// The process exited.
    Exited,
    /// Judged stuck on its run state (no CPU and no exit, or spinning).
    Stuck,
    /// The pace's cap ended the wait.
    Capped,
}

/// Waits until the browser process has exited. It reuses the shared start judge with "ready" meaning "gone", so the quit is
/// judged on run state like a start (ADR 0031): a quit that still uses CPU is waited for. `cpu` and `pace` are seams for tests.
fn wait_to_quit(
    process: &mut Supervised,
    patience: paddock_proc::Patience,
    cpu: impl FnMut(paddock_proc::ProcessId) -> Option<Duration>,
    pace: impl paddock_proc::Pace,
) -> Quit {
    use paddock_proc::{StartState, StartWatch, Watched};
    let id = process.process();
    let mut watch = StartWatch::new(patience, "the browser", QuitProbe { process, id, cpu }, pace);
    loop {
        match watch.wait() {
            Watched::State(StartState::Stalled(r)) if !r.cause.is_stuck() => {}
            Watched::State(StartState::Ready | StartState::Exited(_)) => return Quit::Exited,
            Watched::State(_) => return Quit::Stuck,
            Watched::Cancelled => return Quit::Capped,
        }
    }
}

/// What the start judge looks at: the browser process, its `DevToolsActivePort` file and the connection it names.
struct BrowserProbe<'a, F> {
    dir: &'a Path,
    process: &'a mut Supervised,
    id: Option<paddock_proc::ProcessId>,
    cpu: F,
    conn: Option<Connection>,
    /// The last refused or half-made handshake (the file can appear a moment before the endpoint takes connections).
    refused: Option<Error>,
    /// A failure that is not worth trying again.
    failed: Option<Error>,
}

impl<F: FnMut(paddock_proc::ProcessId) -> Option<Duration>> paddock_proc::StartProbe for BrowserProbe<'_, F> {
    fn liveness(&mut self) -> paddock_proc::Liveness {
        if self.process.is_running() {
            return paddock_proc::Liveness::Running;
        }
        paddock_proc::Liveness::Exited(paddock_proc::ExitInfo {
            status: self.process.wait().and_then(|s| s.code()),
            stderr_tail: paddock_proc::tail_of(&self.dir.join(STDERR_FILE), STDERR_TAIL).map(|t| stderr_tail(t.as_bytes())),
        })
    }

    fn pid(&self) -> Option<paddock_proc::ProcessId> {
        self.id
    }

    fn ready(&mut self) -> bool {
        if self.conn.is_some() || self.failed.is_some() {
            return true;
        }
        let Ok(s) = std::fs::read_to_string(self.dir.join("DevToolsActivePort")) else {
            return false;
        };
        let mut l = s.lines();
        let (Some(port), Some(path)) = (l.next().and_then(|p| p.trim().parse::<u16>().ok()), l.next()) else {
            return false;
        };
        match Connection::connect(port, path.trim()) {
            Ok(c) => self.conn = Some(c),
            Err(e @ Error::Connection(_)) => {
                self.refused = Some(e);
                return false;
            }
            Err(e) => self.failed = Some(e),
        }
        true
    }

    fn cpu_time(&mut self) -> Option<Duration> {
        (self.cpu)(self.id?)
    }
}

/// Waits for one tick on the real clock.
pub(crate) struct SleepPace;

impl paddock_proc::Pace for SleepPace {
    fn wait(&mut self, tick: Duration) -> bool {
        std::thread::sleep(tick);
        true
    }
}

/// Waits for the `DevToolsActivePort` file Chrome writes into the profile and connects to the browser endpoint, judged on the
/// browser's run state (ADR 0031) by the shared start judge: a slow start is waited for; the wall-clock cap (the watchdog)
/// ends one that takes the whole allowance, and then the failure says so. `cpu` and `pace` are seams for tests.
#[expect(
    clippy::too_many_arguments,
    reason = "the test seams (cpu, pace, the pause note) are arguments, not fields"
)]
fn connect(
    dir: &Path,
    process: &mut Supervised,
    patience: paddock_proc::Patience,
    ended_by: &AtomicU8,
    max_lifetime: Duration,
    cpu: impl FnMut(paddock_proc::ProcessId) -> Option<Duration>,
    pace: impl paddock_proc::Pace,
    on_paused: &mut dyn FnMut(PauseEvent),
) -> Result<Connection> {
    use paddock_proc::{StartState, StartWatch, Watched};
    let id = process.process();
    let probe = BrowserProbe {
        dir,
        process,
        id,
        cpu,
        conn: None,
        refused: None,
        failed: None,
    };
    let mut watch = StartWatch::new(patience, "the browser", probe, pace);
    // A note ("still starting") is not a reason to give up; only a stuck browser is.
    let mut told = false;
    let seen = loop {
        match watch.wait() {
            Watched::State(StartState::Stalled(r)) if !r.cause.is_stuck() => {
                if r.cause == paddock_proc::StallCause::Paused {
                    if !told {
                        on_paused(PauseEvent::Paused);
                    }
                    told = true;
                } else if told {
                    on_paused(PauseEvent::Resumed);
                    told = false;
                }
            }
            seen => break seen,
        }
    };
    if told {
        on_paused(PauseEvent::Resumed);
    }
    let probe = watch.into_probe();
    match seen {
        Watched::Cancelled => Err(Error::Closed),
        Watched::State(StartState::Ready) => match (probe.conn, probe.failed) {
            (Some(conn), _) => Ok(conn),
            (None, Some(e)) => Err(e),
            (None, None) => Err(Error::Closed),
        },
        Watched::State(StartState::Exited(info)) => {
            if ended_by.load(Ordering::Acquire) == TIMED_OUT {
                return Err(Error::Launch(format!(
                    "the browser did not start within {} s (its time limit)",
                    max_lifetime.as_secs()
                )));
            }
            if probe.process.stopped_over_memory().is_some() {
                return Err(Error::Launch("the browser used more memory than allowed while it started".into()));
            }
            let status = info.status.map_or_else(String::new, |c| format!(" (exit code {c})"));
            let message = format!("the browser exited right after it started{status}");
            Err(Error::Launch(match info.stderr_tail {
                Some(tail) => format!("{message} (browser output: {tail})"),
                None => message,
            }))
        }
        // Stuck (`Starting` is never answered by a watch): the last refused handshake says more than the time did.
        Watched::State(StartState::Stalled(report)) => Err(probe.refused.unwrap_or_else(|| {
            let tail = paddock_proc::tail_of(&dir.join(STDERR_FILE), STDERR_TAIL)
                .map(|t| stderr_tail(t.as_bytes()))
                .filter(|t| !t.is_empty());
            Error::Launch(match tail {
                Some(tail) => format!("{} (browser output: {tail})", report.message),
                None => report.message,
            })
        })),
        Watched::State(StartState::Starting) => Err(Error::Closed),
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
        assert!(lifetime_for(Duration::ZERO) > START_ALLOWANCE);
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
            "#!/bin/sh\n# paddock-fake-browser\necho 'Running as root without --no-sandbox is not supported' >&2\nexit 1\n",
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

    /// A fast clock for the judge's ticks (1 ms each instead of 50 ms): the process state is real, the verdicts count ticks.
    /// Past `TICK_GUARD` ticks it says the wait is cancelled, so a broken judge fails in seconds, not minutes.
    #[cfg(unix)]
    struct Yield(u32);

    #[cfg(unix)]
    const TICK_GUARD: u32 = 20_000;

    #[cfg(unix)]
    impl paddock_proc::Pace for &mut Yield {
        fn wait(&mut self, _tick: Duration) -> bool {
            // A tick shortened to 1 ms: the judge counts ticks, so the verdicts are the same; the process is real and needs the time.
            std::thread::sleep(Duration::from_millis(1));
            self.0 += 1;
            self.0 < TICK_GUARD
        }
    }

    /// Starts `script` as a fake browser whose stderr goes to the profile folder `dir`, and waits for its DevTools port.
    #[cfg(unix)]
    #[expect(clippy::disallowed_methods, reason = "a fake browser script, like the real browser: std Command")]
    fn start_fake(
        script: &str,
        dir: &Path,
        ended_by: u8,
        memory_cap: Option<u64>,
        cpu: impl FnMut(paddock_proc::ProcessId) -> Option<Duration>,
    ) -> Result<Connection> {
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.args(["-c", script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::fs::File::create(dir.join(STDERR_FILE)).unwrap());
        let mut process = match memory_cap {
            Some(limit) => {
                let mut cap = paddock_proc::MemoryCap::new(limit, "the fake browser");
                cap.every = Duration::from_millis(20);
                Supervised::spawn_capped(cmd, cap)
            }
            None => Supervised::spawn(cmd),
        }
        .unwrap();
        let r = connect(
            dir,
            &mut process,
            paddock_proc::Patience::default(),
            &AtomicU8::new(ended_by),
            Duration::from_secs(7),
            cpu,
            &mut Yield(0),
            &mut |_| {},
        );
        process.kill();
        r
    }

    /// A browser stopped by the OS or App Tamer at start-up is paused, not stuck: it is not ended, and the wait goes on until it
    /// is resumed (the caller's cap still bounds it). Idle takes 1200 ticks (60 s at 50 ms); the resume comes at tick 1500.
    #[cfg(unix)]
    #[test]
    #[expect(clippy::disallowed_methods, reason = "a fake browser script and kill(1): std Command")]
    fn a_browser_stopped_at_start_is_paused_not_ended() {
        struct ResumeAt {
            ticks: u32,
            pid: u32,
        }
        impl paddock_proc::Pace for &mut ResumeAt {
            fn wait(&mut self, _tick: Duration) -> bool {
                std::thread::sleep(Duration::from_millis(1));
                self.ticks += 1;
                if self.ticks == 1500 {
                    std::process::Command::new("kill")
                        .args(["-CONT", &self.pid.to_string()])
                        .status()
                        .unwrap();
                }
                self.ticks < TICK_GUARD
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.args(["-c", "kill -STOP $$; exec sleep 300"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::fs::File::create(dir.path().join(STDERR_FILE)).unwrap());
        let mut process = Supervised::spawn(cmd).unwrap();
        let mut told: Vec<PauseEvent> = Vec::new();
        let mut pace = ResumeAt {
            ticks: 0,
            pid: process.process().unwrap().pid(),
        };
        let e = connect(
            dir.path(),
            &mut process,
            paddock_proc::Patience::default(),
            &AtomicU8::new(NONE),
            Duration::from_secs(7),
            |_| Some(Duration::from_secs(1)),
            &mut pace,
            &mut |e| told.push(e),
        )
        .err()
        .unwrap();
        process.kill();
        // Judged stuck only after the resume, never while it was stopped.
        assert!(
            pace.ticks > 1500,
            "given up on after {} ticks, while the browser was still stopped",
            pace.ticks
        );
        assert!(!e.to_string().is_empty());
        assert_eq!(
            told,
            [PauseEvent::Paused, PauseEvent::Resumed],
            "the person is told once that the browser is paused, and once that it is not"
        );
    }

    /// A browser that takes longer to quit than the old fixed 5 s grace, but works while it does, is waited for, not killed.
    #[cfg(unix)]
    #[test]
    #[expect(clippy::disallowed_methods, reason = "a fake browser script: std Command")]
    fn a_slow_quit_that_still_uses_cpu_is_waited_for() {
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.args(["-c", "sleep 6"]);
        let mut process = Supervised::spawn(cmd).unwrap();
        // 6 s at 1 ms ticks is about 6000 ticks, sixty times the idle limit of 100 ticks: only the CPU use keeps the wait going.
        let quit = wait_to_quit(&mut process, CLOSE_PATIENCE, working(), &mut Yield(0));
        assert_eq!(quit, Quit::Exited);
    }

    /// A browser that neither exits nor uses CPU is judged stuck after the idle ticks (about 100), long before any guard or cap.
    #[cfg(unix)]
    #[test]
    #[expect(clippy::disallowed_methods, reason = "a fake browser script: std Command")]
    fn a_stuck_quit_is_given_up_on_by_run_state() {
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.args(["-c", "exec sleep 300"]);
        let mut process = Supervised::spawn(cmd).unwrap();
        let mut pace = Yield(0);
        let quit = wait_to_quit(&mut process, CLOSE_PATIENCE, |_| Some(Duration::from_secs(1)), &mut pace);
        assert_eq!(quit, Quit::Stuck);
        assert!(pace.0 < 1_000, "judged after {} ticks", pace.0);
        process.kill();
    }

    /// The memory cap ending the fake during its start is reported as such, not as a crash.
    #[cfg(unix)]
    #[test]
    fn a_start_ended_by_the_memory_cap_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let script = "x=$(head -c 60000000 /dev/zero | tr '\\0' a); sleep 300";
        let e = start_fake(script, dir.path(), NONE, Some(16 << 20), working())
            .err()
            .unwrap()
            .to_string();
        assert!(e.contains("more memory than allowed"), "{e}");
    }

    /// A CPU reading that grows a little each time (a browser that is working), so only its exit ends the wait.
    #[cfg(unix)]
    fn working() -> impl FnMut(paddock_proc::ProcessId) -> Option<Duration> {
        let mut n = 0u64;
        move |_| {
            n += 1;
            Some(Duration::from_micros(n))
        }
    }

    /// A browser that stays alive, uses no CPU and never opens its port is stuck (judged on ticks, not on a fixed time), and
    /// the error carries the tail of what it printed.
    #[cfg(unix)]
    #[test]
    fn a_hung_start_is_reported_with_the_browsers_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let printed = dir.path().join(STDERR_FILE);
        // Flat CPU is reported only once the fake has printed its line: wait on the condition, not on a time.
        let flat = move |_| {
            std::fs::metadata(&printed)
                .is_ok_and(|m| m.len() > 0)
                .then_some(Duration::from_secs(1))
        };
        let e = start_fake(
            "echo 'GPU process is waiting for a lock' >&2; exec sleep 300",
            dir.path(),
            NONE,
            None,
            flat,
        )
        .err()
        .unwrap()
        .to_string();
        assert!(e.contains("done nothing") && e.contains("GPU process is waiting"), "{e}");
    }

    /// A start that is slow but progressing (the browser keeps using CPU) is waited for far past the old fixed 20 s and past the
    /// idle limit (60 s of ticks), on the fake clock: it is ended only when the process itself ends, never judged by elapsed time.
    #[cfg(unix)]
    #[test]
    #[expect(clippy::disallowed_methods, reason = "a fake browser script and kill(1): std Command")]
    fn a_slow_start_that_keeps_working_is_waited_for_not_cut_off_at_a_fixed_time() {
        struct EndAt {
            ticks: u32,
            pid: u32,
        }
        impl paddock_proc::Pace for &mut EndAt {
            fn wait(&mut self, _tick: Duration) -> bool {
                std::thread::sleep(Duration::from_millis(1));
                self.ticks += 1;
                if self.ticks == 3000 {
                    std::process::Command::new("kill")
                        .args(["-9", &self.pid.to_string()])
                        .status()
                        .unwrap();
                }
                self.ticks < TICK_GUARD
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.args(["-c", "exec sleep 300"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::fs::File::create(dir.path().join(STDERR_FILE)).unwrap());
        let mut process = Supervised::spawn(cmd).unwrap();
        let mut pace = EndAt {
            ticks: 0,
            pid: process.process().unwrap().pid(),
        };
        let e = connect(
            dir.path(),
            &mut process,
            paddock_proc::Patience::default(),
            &AtomicU8::new(NONE),
            Duration::from_secs(7),
            working(),
            &mut pace,
            &mut |_| {},
        )
        .err()
        .unwrap()
        .to_string();
        process.kill();
        // 3000 ticks of 50 ms are 150 s: waited for while it worked; ended only when the process ended.
        assert!(
            pace.ticks >= 3000,
            "given up on after {} ticks while the browser was still working: {e}",
            pace.ticks
        );
        assert!(e.contains("exited"), "{e}");
    }

    /// The exit status and the stderr tail of a browser that ended during the start are named.
    #[cfg(unix)]
    #[test]
    fn a_browser_that_exits_during_the_start_names_its_exit_code_and_output() {
        let dir = tempfile::tempdir().unwrap();
        let e = start_fake("echo 'bad flag' >&2; exit 3", dir.path(), NONE, None, |_| None)
            .err()
            .unwrap()
            .to_string();
        assert!(
            e.contains("exited right after it started") && e.contains("exit code 3") && e.contains("bad flag"),
            "{e}"
        );
    }

    /// A browser the time cap ended during its start was not a crash: the message says it did not start within the cap.
    #[cfg(unix)]
    #[test]
    fn a_start_ended_by_the_time_cap_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let e = start_fake("exit 1", dir.path(), TIMED_OUT, None, working())
            .err()
            .unwrap()
            .to_string();
        assert!(e.contains("did not start within 7 s"), "{e}");
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

    /// Regression (review P1-1): a renderer without `with_browser` falls through to `find_installed`, and a test launched
    /// the user's Chrome. In test builds the launch itself refuses an installed browser.
    #[test]
    fn a_test_build_never_launches_an_installed_browser() {
        let base = tempfile::tempdir().unwrap();
        let env = |k: &str| std::env::var(k).ok();
        // conformance: no-launch — the gate under test refuses each candidate before anything starts
        for exe in crate::discover::candidates(crate::discover::Os::CURRENT, &env) {
            let e = Chromium::launch(&exe, &headless(base.path())).err().unwrap();
            assert!(
                matches!(&e, Error::Launch(m) if m.contains("installed")),
                "{}: {e}",
                exe.path.display()
            );
        }
        assert_eq!(std::fs::read_dir(base.path()).unwrap().count(), 0, "refused before any profile");
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
        // A fake that exits at once (a script: a test never starts a real program outside a throwaway session).
        let bin = tempfile::tempdir().unwrap();
        let stub = bin.path().join("exits.sh");
        std::fs::write(&stub, "#!/bin/sh\n# paddock-fake-browser\nexit 0\n").unwrap();
        std::fs::set_permissions(&stub, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let exe = Browser {
            flavor: crate::discover::Flavor::Chromium,
            path: stub,
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
}
