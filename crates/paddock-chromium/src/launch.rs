//! Starting a browser: the command line, the profile (throwaway or kept) with an accent theme, and the supervised process
//! tree with its time and memory limits. The real user agent and the automation banner are kept on purpose: no stealth.

use crate::cdp::Connection;
use crate::discover::{Browser, Flavor};
use crate::page::Page;
use crate::{Error, Result};
use paddock_jobs::CancelToken;
use paddock_jobs::time::{Clock, Deadline, SystemClock};
use paddock_proc::{
    ExitInfo, Liveness, MemoryCap, Pace, Patience, ProcessId, Sampler, StartProbe, StartState,
    StartWatch, Supervised, Watched,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The page the browser opens with; always the last argument of its command line.
pub const START_PAGE: &str = "about:blank";
/// How long a browser with a throwaway profile gets to quit by itself after `Browser.close` before its tree is ended. A
/// loaded Mac took up to 11.7 s (p95 6.3 s) to exit after answering (docs/benchmarks.md, "paddock-chromium:
/// Browser.close exit time"); past the grace the tree is ended, which is safe for a throwaway profile (it is deleted).
pub const CLOSE_GRACE: Duration = Duration::from_secs(5);
/// The grace of a kept profile ([`Profile::Persistent`]): ending Chrome by force during its shutdown can lose cookies or
/// a sign-in it is still writing, so it gets longer than the slowest exit measured under load (11.7 s). Closing or
/// dropping a kept-profile [`Chromium`] can therefore block for up to 20 s: adopters close or drop it off the UI thread
/// (ER-17).
pub const CLOSE_GRACE_KEPT: Duration = Duration::from_secs(20);

/// The close grace for a profile kind: [`CLOSE_GRACE`] for a throwaway one, [`CLOSE_GRACE_KEPT`] for a kept one.
pub fn close_grace_for(profile: &Profile) -> Duration {
    match profile {
        Profile::Throwaway(_) => CLOSE_GRACE,
        Profile::Persistent(_) => CLOSE_GRACE_KEPT,
    }
}
/// How long [`Chromium::start_page`] waits for the page the browser opens with.
pub const START_PAGE_WAIT: Duration = Duration::from_secs(5);
/// How often the start-up and start-page waits look again (on the browser's clock).
const POLL: Duration = Duration::from_millis(50);
/// Set to `1` (by a CI job that runs as root in a container) to start the browser without its sandbox.
pub const NO_SANDBOX_ENV: &str = "PADDOCK_BROWSER_NO_SANDBOX";
/// The browser's error output is kept in this file inside the profile, so a failed start can say why.
const STDERR_FILE: &str = "browser-stderr.log";
/// At most this many bytes from the end of the browser's error output go into a message.
const STDERR_TAIL: usize = 600;
/// The prefix of every throwaway profile folder: `chromium-<pid>-<n>`.
const PROFILE_PREFIX: &str = "chromium-";

/// Where the browser window opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    /// A normal window, where the caller placed it (on a display, never off screen) or where the browser chooses.
    OnScreen(Option<WindowBounds>),
    /// A headful window moved off screen: behaves like a normal browser nobody looks at.
    OffScreen,
    /// New headless (`--headless=new`). Tests use it: no window on the desktop (rule 7).
    Headless,
}

/// A window's frame in the screen units the browser reads (points on macOS, device-independent pixels elsewhere).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// The browser's profile folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Profile {
    /// A fresh folder below this one (see [`profiles_dir`]), deleted when the browser ends.
    Throwaway(PathBuf),
    /// A per-purpose folder that keeps cookies between runs (signed-in research).
    Persistent(PathBuf),
}

/// Whether the browser runs inside its own sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sandbox {
    On,
    Off,
}

impl Sandbox {
    /// Chromium refuses to start as root ("Running as root without --no-sandbox is not supported"), which is how CI
    /// containers run. The sandbox is dropped only when the process is root AND [`NO_SANDBOX_ENV`] asks for it: a user's
    /// browser keeps its sandbox, and root without the opt-in still gets Chromium's own refusal.
    pub fn decide(env_value: Option<&str>, root: bool) -> Sandbox {
        if root && env_value.is_some_and(|v| !v.is_empty() && v != "0") {
            Sandbox::Off
        } else {
            Sandbox::On
        }
    }

    /// The decision for this process.
    pub fn from_environment() -> Sandbox {
        Sandbox::decide(
            std::env::var(NO_SANDBOX_ENV).ok().as_deref(),
            running_as_root(),
        )
    }
}

#[cfg(unix)]
fn running_as_root() -> bool {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(not(unix))]
fn running_as_root() -> bool {
    false
}

/// How the start judge reads the CPU time of the tree rooted at a process; `None`: the OS did not say.
pub type CpuSampler = Arc<dyn Fn(ProcessId) -> Option<Duration> + Send + Sync>;

/// Launch parameters.
#[derive(Clone)]
pub struct LaunchOptions {
    pub profile: Profile,
    pub window: Window,
    /// Window frame colour as `0xRRGGBB` (best effort; see [`theme_preferences`]).
    pub accent: Option<u32>,
    /// The browser tree is ended after this long on [`LaunchOptions::clock`], whatever it does.
    pub max_lifetime: Duration,
    /// The browser tree is ended when its memory (PSS on Linux) exceeds this many bytes.
    pub memory_cap: Option<u64>,
    /// A limit for the browser's disk cache (a kept profile must not grow without bound).
    pub disk_cache_bytes: Option<u64>,
    /// [`QUIET_FLAGS`] and [`QUIET_DISABLED_FEATURES`]: no GPU process, no component downloads, no throttling of a window
    /// nobody sees. For a browser that reads pages; an interactive window the user browses in may turn them off.
    pub quiet: bool,
    /// More switches (before the start page).
    pub extra_args: Vec<String>,
    /// When a browser that has not opened its DevTools port is stuck rather than slow (ADR 0031): a slow or starved start
    /// is waited for, only a browser that spins, does nothing or cannot be observed is given up on. The wall-clock cap is
    /// [`LaunchOptions::max_lifetime`], which runs from the process start.
    pub patience: Patience,
    /// Reads the CPU time of the browser's tree for the start judge; `None` uses the OS ([`paddock_proc::tree_cpu_time`]).
    /// A test passes its own, since a fake browser's real CPU time says nothing.
    pub cpu_sampler: Option<CpuSampler>,
    /// The clock of the time limit (a fake clock in tests runs it out without waiting).
    pub clock: Arc<dyn Clock>,
    /// How the memory cap measures the tree; `None` uses PSS / resident size.
    pub memory_sampler: Option<Sampler>,
}

impl std::fmt::Debug for LaunchOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaunchOptions")
            .field("profile", &self.profile)
            .field("window", &self.window)
            .field("max_lifetime", &self.max_lifetime)
            .field("memory_cap", &self.memory_cap)
            .finish_non_exhaustive()
    }
}

impl LaunchOptions {
    /// Defaults: on screen where the browser chooses, quiet flags, 30 minutes, 4 GiB, no cache limit.
    pub fn new(profile: Profile) -> LaunchOptions {
        LaunchOptions {
            profile,
            window: Window::OnScreen(None),
            accent: None,
            max_lifetime: Duration::from_secs(30 * 60),
            memory_cap: Some(4 << 30),
            disk_cache_bytes: None,
            quiet: true,
            extra_args: Vec::new(),
            patience: Patience {
                tick: POLL,
                ..Patience::default()
            },
            cpu_sampler: None,
            clock: Arc::new(SystemClock),
            memory_sampler: None,
        }
    }
}

/// Switches that keep a browser quick and predictable on a cold, loaded machine (a CI runner, a VM without a GPU). None of
/// them changes what a page can detect about automation.
pub const QUIET_FLAGS: &[&str] = &[
    "--disable-gpu",
    "--disable-component-update",
    "--disable-default-apps",
    "--disable-domain-reliability",
    "--disable-breakpad",
    "--disable-hang-monitor",
    "--disable-background-networking",
    "--metrics-recording-only",
    "--no-service-autorun",
    "--disable-background-timer-throttling",
    "--disable-renderer-backgrounding",
    "--disable-backgrounding-occluded-windows",
];

/// Browser features the quiet browser starts without (in the one `--disable-features` switch, see [`command_line`]).
pub const QUIET_DISABLED_FEATURES: &[&str] = &[
    "Translate",
    "OptimizationHints",
    "MediaRouter",
    "AutofillServerCommunication",
    "msEdgeShopping",
    "msEdgeSync",
];

/// The switch that keeps a Chromium build (Chrome for Testing among them) from applying the field-trial testing config
/// it carries (`testing/variations/fieldtrial_testing_config.json`): experimental features that branded Chrome only gets
/// from the server for some users, and Puppeteer and Playwright turn off the same way. One of them is the network-service
/// sandbox on Windows (study `NetworkServiceSandboxWindows`), which could not start on Windows CI for a browser in the
/// runner's user folder: in all 12 Chrome logs of runs 7149 and 7154 the app-container access check failed ("Sandbox
/// cannot access executable", launch error 45, Windows error 5) and Chrome started the network service again,
/// unsandboxed. The lost loads of those runs started 3 to 4 s after the second start, when the profile's own requests
/// failed too (the sign-in request with `net::ERR_FAILED` instead of `net::ERR_NAME_NOT_RESOLVED`). With this switch,
/// CI run 7170 ran the real-browser tests five times on Windows (25 browsers) with none of these: no failed sandbox
/// start, no second start, no failed sign-in request, no `net::ERR_ABORTED`. Branded Chrome and Edge never apply the
/// testing config, so the switch is for [`Flavor::Chromium`] only. Read 2026-10-30 against Chrome for Testing 156.0.8078.47.
pub const NO_TESTING_FIELD_TRIALS: &str = "--disable-field-trial-config";

const DISABLE_FEATURES: &str = "--disable-features=";

/// The whole command line `flavor` starts with: every switch, then [`START_PAGE`] last. This is the only place the
/// arguments are assembled, so no switch can end up after the start page. `--enable-automation` keeps Chrome's
/// "controlled by automated test software" banner and `navigator.webdriver = true`; nothing hides either. Every feature to
/// turn off goes into one `--disable-features` switch, the caller's own (in [`LaunchOptions::extra_args`]) merged in, so
/// none is lost to a second switch.
pub fn command_line(
    flavor: Flavor,
    data_dir: &Path,
    opts: &LaunchOptions,
    sandbox: Sandbox,
) -> Vec<String> {
    let mut a = vec![
        "--remote-debugging-port=0".to_string(),
        format!("--user-data-dir={}", data_dir.display()),
        "--enable-automation".into(),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--disable-sync".into(),
        "--disable-extensions".into(),
        "--password-store=basic".into(),
        "--use-mock-keychain".into(),
    ];
    match opts.window {
        Window::OnScreen(Some(w)) => {
            a.push(format!("--window-position={},{}", w.x, w.y));
            a.push(format!(
                "--window-size={},{}",
                w.width.max(1),
                w.height.max(1)
            ));
        }
        Window::OnScreen(None) => a.push("--window-size=1100,800".into()),
        Window::OffScreen => {
            a.push("--window-size=1100,800".into());
            a.push("--window-position=-32000,-32000".into());
        }
        Window::Headless => {
            a.push("--window-size=1100,800".into());
            a.push("--headless=new".into());
        }
    }
    if opts.quiet {
        a.extend(QUIET_FLAGS.iter().map(|f| f.to_string()));
    }
    if let Some(bytes) = opts.disk_cache_bytes {
        a.push(format!("--disk-cache-size={bytes}"));
    }
    if sandbox == Sandbox::Off {
        a.push("--no-sandbox".into());
        // Containers have a small /dev/shm.
        a.push("--disable-dev-shm-usage".into());
    }
    let mut disabled: Vec<String> = Vec::new();
    let mut disable = |f: &str| {
        if !f.is_empty() && !disabled.iter().any(|d| d == f) {
            disabled.push(f.to_string());
        }
    };
    if opts.quiet {
        QUIET_DISABLED_FEATURES.iter().for_each(|f| disable(f));
    }
    for x in &opts.extra_args {
        if let Some(list) = x.strip_prefix(DISABLE_FEATURES) {
            list.split(',').map(str::trim).for_each(&mut disable);
        }
    }
    if !disabled.is_empty() {
        a.push(format!("{DISABLE_FEATURES}{}", disabled.join(",")));
    }
    if flavor == Flavor::Chromium {
        a.push(NO_TESTING_FIELD_TRIALS.into());
    }
    a.extend(
        opts.extra_args
            .iter()
            .filter(|x| x.as_str() != START_PAGE && !x.starts_with(DISABLE_FEATURES))
            .cloned(),
    );
    a.push(START_PAGE.into());
    a
}

/// Whether a window can be shown: a headful window needs an X server or a Wayland compositor on Linux. Other platforms
/// always have a window system. Pure: `env` reads one variable.
pub fn check_display(
    window: Window,
    linux: bool,
    env: impl Fn(&str) -> Option<String>,
) -> Result<()> {
    let set = |k: &str| env(k).is_some_and(|v| !v.trim().is_empty());
    let display = set("DISPLAY") || set("WAYLAND_DISPLAY");
    if !linux || window == Window::Headless || display {
        return Ok(());
    }
    Err(Error::Launch(
        "there is no display to open the browser on (neither DISPLAY nor WAYLAND_DISPLAY is set); run the app in a desktop \
         session, or on a server under a virtual display such as `xvfb-run`"
            .into(),
    ))
}

/// The last part of the browser's error output as one bounded, printable line (empty when there is none).
pub fn stderr_tail(bytes: &[u8]) -> String {
    let start = bytes.len().saturating_sub(STDERR_TAIL);
    let text = String::from_utf8_lossy(&bytes[start..]);
    let lines: Vec<String> = text
        .lines()
        .map(|l| {
            l.chars()
                .filter(|c| !c.is_control())
                .collect::<String>()
                .trim()
                .to_string()
        })
        .filter(|l| !l.is_empty())
        .collect();
    lines.join(" | ")
}

/// Browser theme preferences that colour the window frame. Chrome and Edge read the user colour from
/// `browser.theme.user_color2` (ARGB integer) with a scheme and variant; the older `user_color` is written too.
pub fn theme_preferences(accent: u32) -> Value {
    let argb = (0xFF00_0000u32 | (accent & 0x00FF_FFFF)) as i32;
    json!({"browser": {"theme": {"user_color2": argb, "user_color": argb, "color_scheme2": 0, "color_variant2": 1}}})
}

/// Merges [`theme_preferences`] into `<data_dir>/Default/Preferences`, keeping everything else in it.
pub fn write_theme(data_dir: &Path, accent: u32) -> std::io::Result<()> {
    let dir = data_dir.join("Default");
    std::fs::create_dir_all(&dir)?;
    let file = dir.join("Preferences");
    let mut root: Value = std::fs::read_to_string(&file)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    merge(&mut root, theme_preferences(accent));
    std::fs::write(file, root.to_string())
}

fn merge(into: &mut Value, from: Value) {
    match (into, from) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                merge(a.entry(k).or_insert(Value::Null), v);
            }
        }
        (slot, v) => *slot = v,
    }
}

/// The folder for throwaway profiles in the app's cache folder.
pub fn profiles_dir(dirs: &paddock_env::AppDirs) -> PathBuf {
    dirs.cache().join("browser-profiles")
}

/// A profile folder no other launch uses: the process id and a counter (the clock alone is not enough: two launches at
/// once got the same folder on macOS).
fn unique_dir(base: &Path) -> PathBuf {
    static LAUNCHES: AtomicU64 = AtomicU64::new(0);
    let k = LAUNCHES.fetch_add(1, Ordering::Relaxed);
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    base.join(format!("{PROFILE_PREFIX}{}-{n}-{k}", std::process::id()))
}

/// Removes throwaway profiles in `base` left by processes that are gone (a crash, a kill). Call at start-up. Returns how
/// many were removed.
pub fn sweep_profiles(base: &Path) -> usize {
    sweep_profiles_with(base, &paddock_proc::is_alive)
}

fn sweep_profiles_with(base: &Path, alive: &dyn Fn(u32) -> bool) -> usize {
    let Ok(rd) = std::fs::read_dir(base) else {
        return 0;
    };
    let me = std::process::id();
    rd.flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let pid = name
                .strip_prefix(PROFILE_PREFIX)
                .and_then(|r| r.split('-').next())
                .and_then(|p| p.parse::<u32>().ok());
            pid.is_some_and(|p| p != me && !alive(p)) && std::fs::remove_dir_all(e.path()).is_ok()
        })
        .count()
}

/// Why a launched browser can no longer be used. Each is final: the browser is ended and a new one started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gone {
    /// The process ended (the user quit the browser, it crashed, or a limit ended it: see [`Chromium::ended_by`]).
    Exited,
    /// The DevTools connection ended while the process still runs.
    Disconnected,
    /// The page was closed (the user closed its window or tab); on macOS the browser keeps running without a window.
    PageClosed,
}

/// How a browser's helper processes started ([`Chromium::helper_starts`]), from its histograms (names as in Chrome for
/// Testing 156.0.8078.47: `ChildProcess.Launched.UtilityProcessHash`, `ChildProcess.LaunchFailed.UtilityProcessErrorCode`,
/// `Chrome.SystemNetworkContextManager.NetworkSandboxLaunchFailed.ErrorCode` and `...NetworkSandboxState`).
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HelperStarts {
    /// Helper processes that started.
    pub started: u64,
    /// Tries that failed (each is followed by another).
    pub failed: u64,
    /// Tries of the sandboxed network service that failed.
    pub network_sandbox_failed: u64,
    /// Times the browser decided to run the network service unsandboxed because an earlier sandboxed start had failed
    /// (`NetworkSandboxState` bucket 4, `kDisabledBecauseOfFailedLaunch`).
    pub network_sandbox_given_up: u64,
}

impl HelperStarts {
    /// Pure: the counts from `Browser.getHistograms` lists.
    pub fn from_histograms(launch: &[Value], sandbox: &[Value]) -> HelperStarts {
        fn named<'a>(list: &'a [Value], name: &'a str) -> impl Iterator<Item = &'a Value> {
            list.iter()
                .filter(move |h| h.get("name").and_then(Value::as_str) == Some(name))
        }
        let count = |list: &[Value], name: &str| -> u64 {
            named(list, name)
                .filter_map(|h| h.get("count").and_then(Value::as_u64))
                .sum()
        };
        let given_up = named(
            sandbox,
            "Chrome.SystemNetworkContextManager.NetworkSandboxState",
        )
        .filter_map(|h| h.get("buckets").and_then(Value::as_array))
        .flatten()
        .filter(|b| b.get("low").and_then(Value::as_i64) == Some(4))
        .filter_map(|b| b.get("count").and_then(Value::as_u64))
        .sum();
        HelperStarts {
            started: count(launch, "ChildProcess.Launched.UtilityProcessHash"),
            failed: count(launch, "ChildProcess.LaunchFailed.UtilityProcessErrorCode"),
            network_sandbox_failed: count(
                sandbox,
                "Chrome.SystemNetworkContextManager.NetworkSandboxLaunchFailed.ErrorCode",
            ),
            network_sandbox_given_up: given_up,
        }
    }
}

/// Why the supervisor ended the browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndedBy {
    /// It still runs, or it ended by itself or on request.
    Nothing,
    /// [`LaunchOptions::max_lifetime`] passed.
    TimeLimit,
    /// [`LaunchOptions::memory_cap`] was exceeded.
    MemoryCap,
}

/// A running browser. Dropping it closes it (as a user would, then ends what is left) and removes a throwaway profile.
pub struct Chromium {
    // Field order matters: the connection closes before the process is ended and the profile removed.
    conn: Arc<Connection>,
    /// Shared with the time-limit watchdog, which ends the tree through it (on Windows that closes the kill-on-close job).
    process: Arc<Mutex<Supervised>>,
    dir: PathBuf,
    throwaway: bool,
    /// Cancelled when the browser ends on purpose: stops the time-limit watchdog.
    stop: CancelToken,
    timed_out: Arc<AtomicBool>,
    exe: Browser,
    /// `LaunchOptions::clock`: the time limit, the start-page wait and the pages' waits run on it.
    clock: Arc<dyn Clock>,
    /// Whether the last `Browser.close` was answered (`None` before a close sent one); see [`Chromium::close_answered`].
    close_answered: Option<bool>,
}

impl Chromium {
    /// Starts `exe` and connects to it. The sandbox follows [`Sandbox::from_environment`].
    pub fn launch(exe: &Browser, opts: &LaunchOptions) -> Result<Chromium> {
        Self::launch_with(exe, opts, Sandbox::from_environment())
    }

    /// [`Chromium::launch`] with the sandbox decided by the caller (tests run the root branch on any OS).
    pub fn launch_with(exe: &Browser, opts: &LaunchOptions, sandbox: Sandbox) -> Result<Chromium> {
        // Test builds: no installed browser, and no real one outside a throwaway session (rule 7), however it was found.
        #[cfg(any(test, feature = "test-support"))]
        {
            use paddock_testkit::live_browser::BrowserWindow;
            let window = match opts.window {
                Window::Headless => BrowserWindow::Headless,
                Window::OffScreen | Window::OnScreen(_) => BrowserWindow::Shown,
            };
            crate::discover::check_test_launch(&exe.path, window).map_err(Error::Launch)?;
        }
        check_display(opts.window, cfg!(target_os = "linux"), |k| {
            std::env::var(k).ok()
        })?;
        let (dir, throwaway) = match &opts.profile {
            Profile::Throwaway(base) => (unique_dir(base), true),
            Profile::Persistent(p) => (p.clone(), false),
        };
        std::fs::create_dir_all(&dir)
            .map_err(|e| Error::Launch(format!("the profile folder could not be created: {e}")))?;
        let _ = std::fs::remove_file(dir.join("DevToolsActivePort"));
        if let Some(c) = opts.accent
            && let Err(e) = write_theme(&dir, c)
        {
            log::warn!("The browser frame colour could not be written: {e}");
        }
        let stderr = std::fs::File::create(dir.join(STDERR_FILE))
            .map_or_else(|_| std::process::Stdio::null(), std::process::Stdio::from);
        let mut cmd = paddock_proc::command(&exe.path);
        cmd.args(command_line(exe.flavor, &dir, opts, sandbox))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(stderr);
        let spawned = match opts.memory_cap {
            Some(limit) => {
                let mut cap = MemoryCap::new(limit, "the browser");
                cap.sampler = opts.memory_sampler.clone();
                Supervised::spawn_capped(cmd, cap)
            }
            None => Supervised::spawn(cmd),
        };
        let process = match spawned {
            Ok(p) => Arc::new(Mutex::new(p)),
            Err(e) => {
                cleanup(&dir, throwaway);
                return Err(Error::Launch(e.to_string()));
            }
        };
        let stop = CancelToken::new();
        let timed_out = Arc::new(AtomicBool::new(false));
        // The limit runs from the process start, also while it starts.
        watchdog(
            process.clone(),
            opts.clock.clone(),
            opts.max_lifetime,
            stop.clone(),
            timed_out.clone(),
        );
        let started = connect(&dir, &process, opts, &timed_out).and_then(|conn| {
            // The memory watcher looks every 200 ms, so a browser that is quick to start can be connected before its first
            // look: one look now, with the same cap and sampler, so "over the cap while it starts" does not depend on timing.
            over_cap_at_start(opts, lock(&process).id()).map_or(Ok(conn), Err)
        });
        match started {
            Ok(conn) => Ok(Chromium {
                conn: Arc::new(conn),
                process,
                dir,
                throwaway,
                stop,
                timed_out,
                exe: exe.clone(),
                clock: opts.clock.clone(),
                close_answered: None,
            }),
            Err(e) => {
                stop.cancel();
                lock(&process).kill();
                cleanup(&dir, throwaway);
                Err(e)
            }
        }
    }

    fn proc(&self) -> MutexGuard<'_, Supervised> {
        lock(&self.process)
    }

    /// The executable that runs.
    pub fn exe(&self) -> &Browser {
        &self.exe
    }

    /// The process id (while running).
    pub fn pid(&self) -> Option<u32> {
        self.proc().id()
    }

    /// Whether the browser runs and the connection is open.
    pub fn is_running(&mut self) -> bool {
        self.proc().is_running() && !self.conn.is_closed()
    }

    /// The profile folder.
    pub fn profile_dir(&self) -> &Path {
        &self.dir
    }

    /// Whether the time limit or the memory cap ended the browser.
    pub fn ended_by(&self) -> EndedBy {
        if self.proc().stopped_over_memory().is_some() {
            EndedBy::MemoryCap
        } else if self.timed_out.load(Ordering::Acquire) {
            EndedBy::TimeLimit
        } else {
            EndedBy::Nothing
        }
    }

    /// Why the browser and its `page` can no longer be used, if so. No round trip to the browser (see [`Page::exists`]).
    pub fn gone(&mut self, page: &Page) -> Option<Gone> {
        if !self.proc().is_running() {
            Some(Gone::Exited)
        } else if self.conn.is_closed() {
            Some(Gone::Disconnected)
        } else {
            page.is_closed().then_some(Gone::PageClosed)
        }
    }

    /// Opens another page (tab) to drive, beside the ones the browser has.
    pub fn new_page(&self) -> Result<Page> {
        Page::attach(self.conn.clone(), self.clock.clone())
    }

    /// The page the browser opened with (its [`START_PAGE`]), to drive instead of opening a second one beside it: the
    /// window then has exactly one tab, and any further page is one the user opened. Waits up to [`START_PAGE_WAIT`] for
    /// it to appear; a browser that shows none by then gets a new page.
    pub fn start_page(&self) -> Result<Page> {
        let end = Deadline::after(self.clock.clone(), START_PAGE_WAIT);
        loop {
            if let Some(target) = self.first_page_target()? {
                return Page::adopt(self.conn.clone(), target, self.clock.clone());
            }
            if end.passed() {
                log::warn!(
                    "The browser showed no start page within {START_PAGE_WAIT:?}; a new one is opened"
                );
                return self.new_page();
            }
            let _ = self.clock.wait(POLL, &CancelToken::new());
        }
    }

    /// How the browser's helper processes started so far, as the browser counts them in its own histograms
    /// ([`HelperStarts`]). `None` when the browser does not say: it does not know `Browser.getHistograms` (an experimental
    /// DevTools method) or answers without histograms. A helper that could not start is started again, and requests in
    /// flight on the first try are lost (see [`NO_TESTING_FIELD_TRIALS`]). For tests and diagnostics.
    #[doc(hidden)]
    pub fn helper_starts(&self) -> Result<Option<HelperStarts>> {
        let Some(launch) = self.histograms("ChildProcess.Launch")? else {
            return Ok(None);
        };
        let Some(sandbox) = self.histograms("Chrome.SystemNetworkContextManager.NetworkSandbox")?
        else {
            return Ok(None);
        };
        Ok(Some(HelperStarts::from_histograms(&launch, &sandbox)))
    }

    /// The histograms whose names contain `query`; `None` when the browser does not answer with a list.
    fn histograms(&self, query: &str) -> Result<Option<Vec<Value>>> {
        match self.conn.call(
            None,
            "Browser.getHistograms",
            json!({"query": query}),
            Duration::from_secs(5),
        ) {
            Ok(r) => Ok(r.get("histograms").and_then(Value::as_array).cloned()),
            Err(Error::Protocol(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn pages(&self) -> Result<Vec<Value>> {
        let r = self
            .conn
            .call(None, "Target.getTargets", json!({}), Duration::from_secs(5))?;
        Ok(r.get("targetInfos")
            .and_then(Value::as_array)
            .map(|t| {
                t.iter()
                    .filter(|i| i.get("type").and_then(Value::as_str) == Some("page"))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }

    fn first_page_target(&self) -> Result<Option<String>> {
        let pages = self.pages()?;
        let blank = pages
            .iter()
            .find(|i| i.get("url").and_then(Value::as_str) == Some(START_PAGE));
        Ok(blank
            .or(pages.first())
            .and_then(|i| i.get("targetId").and_then(Value::as_str))
            .map(str::to_string))
    }

    /// The number of open pages (tabs and windows); a page the user opened shows up here.
    pub fn page_count(&self) -> Result<usize> {
        Ok(self.pages()?.len())
    }

    /// Closes the browser the way a user would (`Browser.close`), waits up to `grace` for it to exit, ends what is left of
    /// the tree, and removes a throwaway profile. Ending a browser that is still starting by force can leave a copy of it
    /// behind on macOS (the code-sign clone), so every normal end comes through here. Returns true when it exited by itself.
    /// Idempotent.
    pub fn close_gracefully(&mut self, grace: Duration) -> bool {
        self.stop.cancel();
        let mut exited = !self.proc().is_running();
        if !exited && !self.conn.is_closed() {
            let answer = self.conn.call(
                None,
                "Browser.close",
                json!({}),
                grace.min(Duration::from_secs(2)),
            );
            self.close_answered = Some(answer.is_ok());
            if let Err(e) = answer {
                // Not an error to the caller (a closing browser may drop the connection before it answers), but the
                // reason a close was slow or forced.
                log::debug!("Browser.close was not answered: {e}");
            }
            let end = Instant::now() + grace;
            while !exited && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(25));
                exited = !self.proc().is_running();
            }
        }
        self.proc().kill();
        if self.throwaway {
            end_leftovers(&self.dir);
        }
        cleanup(&self.dir, self.throwaway);
        exited
    }

    /// Whether the browser answered the last `Browser.close` that [`close_gracefully`](Self::close_gracefully) sent
    /// (`None` when none was sent: the browser or its connection had ended already). For a slow or forced close it tells
    /// "the browser never took the request" from "it took it and was slow to exit".
    pub fn close_answered(&self) -> Option<bool> {
        self.close_answered
    }

    /// The grace [`close`](Self::close) gives this browser: [`CLOSE_GRACE`], or [`CLOSE_GRACE_KEPT`] for a kept profile.
    pub fn close_grace(&self) -> Duration {
        if self.throwaway {
            CLOSE_GRACE
        } else {
            CLOSE_GRACE_KEPT
        }
    }

    /// [`close_gracefully`](Self::close_gracefully) with [`close_grace`](Self::close_grace).
    pub fn close(&mut self) -> bool {
        self.close_gracefully(self.close_grace())
    }

    /// Ends the browser tree at once.
    pub fn kill(&mut self) {
        self.stop.cancel();
        self.proc().kill();
    }
}

impl Drop for Chromium {
    fn drop(&mut self) {
        self.close();
    }
}

/// The launch error when the browser `pid` uses more than the cap right after it connected.
fn over_cap_at_start(opts: &LaunchOptions, pid: Option<u32>) -> Option<Error> {
    let (limit, pid) = (opts.memory_cap?, pid?);
    let used = opts
        .memory_sampler
        .as_ref()
        .map_or_else(|| paddock_proc::tree_memory(pid), |sample| sample(pid));
    (used > limit)
        .then(|| Error::Launch("the browser used more memory than allowed while it started".into()))
}

/// The supervised browser; a panic elsewhere while it was held does not stop it from being ended.
fn lock(p: &Mutex<Supervised>) -> MutexGuard<'_, Supervised> {
    p.lock().unwrap_or_else(|e| e.into_inner())
}

/// Ends the tree when `max` passed on `clock`, even if nobody calls anything. Stops when `stop` is cancelled.
fn watchdog(
    process: Arc<Mutex<Supervised>>,
    clock: Arc<dyn Clock>,
    max: Duration,
    stop: CancelToken,
    timed_out: Arc<AtomicBool>,
) {
    let _ = std::thread::Builder::new()
        .name("chromium-watchdog".into())
        .spawn(move || {
            if clock.wait(max, &stop).is_ok() && !stop.is_cancelled() {
                log::warn!("The browser reached its time limit ({max:?}) and is ended");
                timed_out.store(true, Ordering::Release);
                // Through the Supervised: the tree and, on Windows, its kill-on-close job.
                lock(&process).kill();
            }
        });
}

/// Ends processes that still name the throwaway profile after the tree was ended: a helper that left the browser's process
/// group (a crash handler that daemonised) would otherwise outlive it. The folder name is unique to this launch, so only
/// this browser's processes match. Returns how many were ended.
fn end_leftovers(dir: &Path) -> usize {
    let left = paddock_proc::processes_mentioning(&dir.to_string_lossy());
    for &id in &left {
        log::warn!("A browser helper ({id}) outlived the browser and is ended");
        // By identity: a helper that ended since the look is not mistaken for whatever got its pid (ADR 0033).
        paddock_proc::end(id);
    }
    left.len()
}

fn cleanup(dir: &Path, throwaway: bool) {
    if throwaway {
        cleanup_with(dir, &|d| std::fs::remove_dir_all(d));
    }
}

/// Removes `dir`, retrying for a moment: Chrome may still be flushing after it ended. Gives up quietly (the next
/// [`sweep_profiles`] removes it).
fn cleanup_with(dir: &Path, remove: &dyn Fn(&Path) -> std::io::Result<()>) -> bool {
    for _ in 0..10 {
        if remove(dir).is_ok() || !dir.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let shown = dir.display();
    log::warn!("The browser profile {shown} could not be removed (removed at the next start)");
    false
}

/// What the start judge looks at: the browser process, its `DevToolsActivePort` file and the connection it names.
struct BrowserProbe<'a> {
    dir: &'a Path,
    process: &'a Mutex<Supervised>,
    id: Option<ProcessId>,
    cpu: Option<CpuSampler>,
    conn: Option<Connection>,
    /// The last refused or half-made handshake (the file can appear a moment before the endpoint takes connections).
    refused: Option<Error>,
    /// A failure that is not worth trying again.
    failed: Option<Error>,
}

impl StartProbe for BrowserProbe<'_> {
    fn liveness(&mut self) -> Liveness {
        if lock(self.process).is_running() {
            return Liveness::Running;
        }
        let stderr_tail = std::fs::read(self.dir.join(STDERR_FILE))
            .map(|b| stderr_tail(&b))
            .ok()
            .filter(|t| !t.is_empty());
        Liveness::Exited(ExitInfo {
            status: None,
            stderr_tail,
        })
    }

    fn ready(&mut self) -> bool {
        if self.conn.is_some() || self.failed.is_some() {
            return true;
        }
        let Ok(s) = std::fs::read_to_string(self.dir.join("DevToolsActivePort")) else {
            return false;
        };
        let mut l = s.lines();
        let (Some(port), Some(path)) = (
            l.next().and_then(|p| p.trim().parse::<u16>().ok()),
            l.next(),
        ) else {
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
        let id = self.id?;
        match &self.cpu {
            Some(sample) => sample(id),
            None => paddock_proc::tree_cpu_time(id),
        }
    }
}

/// Waits on the browser's clock.
struct ClockPace(Arc<dyn Clock>);

impl Pace for ClockPace {
    fn wait(&mut self, tick: Duration) -> bool {
        self.0.wait(tick, &CancelToken::new()).is_ok()
    }
}

/// Waits for the `DevToolsActivePort` file the browser writes into the profile and connects to its endpoint, judged on the
/// browser's run state (ADR 0031): a slow start is waited for; the wall-clock cap (`opts.max_lifetime`, the watchdog) ends
/// it when it takes the whole allowance, and then the failure says so. Runs on the browser's clock, so a test drives it
/// with a fake one.
fn connect(
    dir: &Path,
    process: &Mutex<Supervised>,
    opts: &LaunchOptions,
    timed_out: &AtomicBool,
) -> Result<Connection> {
    let id = lock(process).process();
    let probe = BrowserProbe {
        dir,
        process,
        id,
        cpu: opts.cpu_sampler.clone(),
        conn: None,
        refused: None,
        failed: None,
    };
    let mut watch = StartWatch::new(
        opts.patience,
        "the browser",
        probe,
        ClockPace(opts.clock.clone()),
    );
    // A note ("still starting") is not a reason to give up; only a stuck browser is.
    let seen = loop {
        match watch.wait() {
            Watched::State(StartState::Stalled(r)) if !r.cause.is_stuck() => {}
            seen => break seen,
        }
    };
    let probe = watch.into_probe();
    match seen {
        Watched::Cancelled => Err(Error::Closed),
        Watched::State(StartState::Ready) => match (probe.conn, probe.failed) {
            (Some(conn), _) => Ok(conn),
            (None, Some(e)) => Err(e),
            (None, None) => Err(Error::Closed),
        },
        Watched::State(StartState::Exited(info)) => {
            if timed_out.load(Ordering::Acquire) {
                return Err(Error::Launch(format!(
                    "the browser did not start within {} s (its time limit)",
                    opts.max_lifetime.as_secs()
                )));
            }
            if lock(process).stopped_over_memory().is_some() {
                return Err(Error::Launch(
                    "the browser used more memory than allowed while it started".into(),
                ));
            }
            let message = "the browser exited right after it started";
            Err(Error::Launch(match info.stderr_tail {
                Some(tail) => format!("{message} (browser output: {tail})"),
                None => message.into(),
            }))
        }
        // Stuck (`Starting` is never answered by a watch): the last refused handshake says more than the time did.
        Watched::State(StartState::Stalled(report)) => {
            Err(probe.refused.unwrap_or(Error::Launch(report.message)))
        }
        Watched::State(StartState::Starting) => Err(Error::Closed),
    }
}

#[cfg(test)]
mod pure_tests;
#[cfg(all(test, unix))]
mod tests;
