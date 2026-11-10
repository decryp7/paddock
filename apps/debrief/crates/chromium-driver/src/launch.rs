//! Starting a browser: the command line, the profile (throwaway or persistent) with an accent theme, and the
//! supervised process. The real user agent and the automation banner are kept on purpose: no stealth.

use crate::cdp::Connection;
use crate::discover::{Browser, Flavor};
use crate::page::Page;
use crate::process::Supervised;
use crate::{Error, Result};
use paddock_proc::{ExitInfo, Liveness, Pace, Patience, ProcessId, StallReport, StartProbe, StartState, StartWatch, Watched};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long [`Chromium::start_page`] waits for the page the browser opens with.
#[cfg(not(test))]
pub const START_PAGE_WAIT: Duration = Duration::from_secs(5);
#[cfg(test)]
pub const START_PAGE_WAIT: Duration = Duration::from_millis(300);

#[derive(Debug, Clone)]
pub enum Profile {
    /// A fresh folder below this one, deleted when the browser ends. The caller names the base, its own cache folder
    /// (Debrief: `browser-profiles` in the local data folder); the driver never falls back to the shared temp folder,
    /// where another local user could plant a name, and refuses an empty path at launch. A profile a crashed run left
    /// is not removed here: the caller sweeps its base at start-up (Debrief removes the ones older than 24 hours).
    Throwaway(PathBuf),
    /// A per-purpose directory that keeps cookies between runs.
    Persistent(PathBuf),
}

/// Where the browser window opens, in the screen units the browser reads its `--window-position`/`--window-size` in
/// (points on macOS, device-independent pixels elsewhere). The caller decides (the display of the window that asked).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct LaunchOptions {
    pub profile: Profile,
    /// Window frame colour as `0xRRGGBB` (best effort; see [`theme_preferences`]).
    pub accent: Option<u32>,
    /// The browser is ended after this long, whatever it does; also the last-resort cap of its start.
    pub max_lifetime: Duration,
    /// How the start is judged (ADR 0031): a start that is slow but alive is waited for; only a browser that exits,
    /// spins without becoming ready or does nothing at all fails it. Elapsed time alone never does.
    pub patience: Patience,
    /// Where its window opens; the browser's own choice when `None`.
    pub window: Option<WindowBounds>,
    /// A limit for the browser's disk cache (for a kept profile, so it does not grow without bound).
    pub disk_cache_bytes: Option<u64>,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        LaunchOptions {
            // No base yet: a launch refuses it, so the caller must name its own folder.
            profile: Profile::Throwaway(PathBuf::new()),
            accent: None,
            max_lifetime: Duration::from_secs(30 * 60),
            patience: Patience::default(),
            window: None,
            disk_cache_bytes: None,
        }
    }
}

/// The page the browser opens with; always the last argument of its command line.
pub const START_PAGE: &str = "about:blank";

/// The browser's switches (everything but the start page). `--enable-automation` keeps Chrome's "controlled by
/// automated test software" banner and `navigator.webdriver = true`; nothing hides either. The window opens on screen
/// like any other: no headless mode, and a position only where the caller placed it (on a display, never off screen).
fn switches(data_dir: &Path, opts: &LaunchOptions, sandbox: Sandbox) -> Vec<String> {
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
        Some(w) => {
            a.push(format!("--window-position={},{}", w.x, w.y));
            a.push(format!("--window-size={},{}", w.width.max(1), w.height.max(1)));
        }
        None => a.push("--window-size=1100,800".into()),
    }
    if let Some(bytes) = opts.disk_cache_bytes {
        a.push(format!("--disk-cache-size={bytes}"));
    }
    if sandbox == Sandbox::Off {
        a.push("--no-sandbox".into());
    }
    a
}

/// Keeps a Chromium build (Chrome for Testing) from applying the field-trial testing config it carries, as Puppeteer and
/// Playwright do; branded Chrome and Edge never apply it. That config turns on the network-service sandbox on Windows,
/// which could not start for a browser in a user folder on Paddock's Windows CI (all 12 Chrome logs of runs 7149 and
/// 7154), and real-browser loads ended in `net::ERR_ABORTED` in those runs (paddock-chromium's `NO_TESTING_FIELD_TRIALS`).
pub const NO_TESTING_FIELD_TRIALS: &str = "--disable-field-trial-config";

/// The whole command line `flavor` starts with: every switch, then [`START_PAGE`] last. This is the only place
/// arguments are assembled, so no switch can end up after the start page.
pub fn command_line(flavor: Flavor, data_dir: &Path, opts: &LaunchOptions, sandbox: Sandbox) -> Vec<String> {
    let mut a = switches(data_dir, opts, sandbox);
    if flavor == Flavor::Chromium {
        a.push(NO_TESTING_FIELD_TRIALS.into());
    }
    a.push(START_PAGE.into());
    a
}

/// Set to `1` (by a CI job that runs as root in a container) to start the browser without its sandbox. Honoured in a debug
/// build only (the CI job runs the debug tests); a release build ignores it and logs its name once, so the environment
/// cannot switch off the sandbox of a shipped Debrief (ADR 0073 DB1).
pub const NO_SANDBOX_ENV: &str = "DEBRIEF_BROWSER_NO_SANDBOX";

/// Whether the browser runs inside its own sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sandbox {
    On,
    Off,
}

impl Sandbox {
    /// Chromium refuses to start as root ("Running as root without --no-sandbox is not supported"), which is how
    /// every CI container runs. The sandbox is dropped only when the process is root AND the environment asks for it
    /// explicitly: a user's browser keeps its sandbox, and root without the opt-in still gets Chromium's own refusal.
    ///
    /// `debug` is `cfg!(debug_assertions)`: in a release build the variable never counts.
    pub fn decide(debug: bool, env_value: Option<&str>, root: bool) -> Sandbox {
        if debug && root && env_value.is_some_and(|v| !v.is_empty() && v != "0") {
            Sandbox::Off
        } else {
            Sandbox::On
        }
    }

    /// The decision for this process, from [`NO_SANDBOX_ENV`] and the effective user.
    fn from_environment() -> Sandbox {
        let value = std::env::var(NO_SANDBOX_ENV).ok();
        let debug = cfg!(debug_assertions);
        if !debug && value.as_deref().is_some_and(|v| !v.is_empty()) {
            crate::ignored_env::warn_once(NO_SANDBOX_ENV);
        }
        Sandbox::decide(debug, value.as_deref(), running_as_root())
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

/// Browser theme preferences that colour the window frame. Chrome and Edge read the user colour from
/// `browser.theme.user_color2` (ARGB integer) with a scheme and variant; the older `user_color` is written too.
/// Best effort: a version that ignores them still shows the other indicators.
pub fn theme_preferences(accent: u32) -> Value {
    let argb = (0xFF00_0000u32 | (accent & 0x00FF_FFFF)) as i32;
    json!({"browser": {"theme": {
        "user_color": argb, "user_color2": argb,
        "color_variant": 2, "color_variant2": 2,
        "color_scheme": 2, "color_scheme2": 2,
        "is_grayscale": false, "is_grayscale2": false
    }}})
}

/// Writes (merging into any existing file) the theme into `<data_dir>/Default/Preferences`.
pub fn write_theme(data_dir: &Path, accent: u32) -> std::io::Result<()> {
    let dir = data_dir.join("Default");
    std::fs::create_dir_all(&dir)?;
    let file = dir.join("Preferences");
    let mut root: Value = std::fs::read_to_string(&file)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
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

/// Why a launched browser can no longer be used. Each is final: the browser is ended and a new one started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gone {
    /// The process ended (the user quit the browser, or it crashed).
    Exited,
    /// The DevTools connection ended while the process still runs.
    Disconnected,
    /// The page Debrief drives was closed (the user closed its window or tab); on macOS the browser keeps running
    /// without a window.
    PageClosed,
}

/// A running browser. Dropping it ends the process and removes a throwaway profile.
pub struct Chromium {
    // Field order matters: the connection closes before the process is killed and the profile removed.
    conn: Arc<Connection>,
    process: Supervised,
    dir: PathBuf,
    throwaway: bool,
    alive: Arc<AtomicBool>,
    pub exe: Browser,
}

/// A fresh throwaway profile folder below `base` (the caller's own cache folder): created atomically with a random name,
/// removed by [`cleanup`] when the launch ends. Nothing is put in the shared temp folder, where another local user could
/// plant a name; an empty `base` is an error.
fn unique_dir(base: &Path) -> std::io::Result<PathBuf> {
    if base.as_os_str().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "no folder was given for throwaway profiles",
        ));
    }
    std::fs::create_dir_all(base)?;
    Ok(tempfile::Builder::new().prefix("chromium-driver-").tempdir_in(base)?.keep())
}

impl Chromium {
    pub fn launch(exe: &Browser, opts: &LaunchOptions) -> Result<Chromium> {
        Self::launch_with(exe, opts, Sandbox::from_environment())
    }

    /// [`Chromium::launch`] with the sandbox decision made by the caller (tests run the root branch on any OS).
    #[expect(
        clippy::disallowed_methods,
        reason = "the browser, a GUI program: this crate cannot use Debrief's process helper"
    )]
    fn launch_with(exe: &Browser, opts: &LaunchOptions, sandbox: Sandbox) -> Result<Chromium> {
        let (dir, throwaway) = match &opts.profile {
            Profile::Throwaway(base) => (
                unique_dir(base).map_err(|e| Error::Launch(format!("the profile folder could not be created: {e}")))?,
                true,
            ),
            Profile::Persistent(p) => (p.clone(), false),
        };
        std::fs::create_dir_all(&dir).map_err(|e| Error::Launch(format!("the profile folder could not be created: {e}")))?;
        let _ = std::fs::remove_file(dir.join("DevToolsActivePort"));
        if let Some(c) = opts.accent {
            if let Err(e) = write_theme(&dir, c) {
                log::warn!("The browser frame colour could not be written: {e}");
            }
        }
        let mut cmd = std::process::Command::new(&exe.path);
        cmd.args(command_line(exe.flavor, &dir, opts, sandbox))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut process = match Supervised::spawn(cmd) {
            Ok(p) => p,
            Err(e) => {
                cleanup(&dir, throwaway);
                return Err(Error::Launch(e.to_string()));
            }
        };
        // The lifetime watchdog runs from the spawn, so it is also the last-resort cap of a start that never ends.
        let alive = Arc::new(AtomicBool::new(true));
        let timed_out = Arc::new(AtomicBool::new(false));
        // The browser's identity, taken right after the spawn: the watchdog and the CPU readings go by it (ADR 0033).
        let id = process.id().and_then(ProcessId::of);
        watchdog(alive.clone(), timed_out.clone(), id, opts.max_lifetime);
        match connect(&dir, &mut process, id, opts, &timed_out) {
            Ok(conn) => Ok(Chromium {
                conn: Arc::new(conn),
                process,
                dir,
                throwaway,
                alive,
                exe: exe.clone(),
            }),
            Err(e) => {
                alive.store(false, Ordering::Release);
                process.kill();
                cleanup(&dir, throwaway);
                Err(e)
            }
        }
    }

    pub fn pid(&self) -> Option<u32> {
        self.process.id()
    }

    pub fn is_running(&mut self) -> bool {
        self.process.is_running() && !self.conn.is_closed()
    }

    /// Why the browser and its `page` can no longer be used, if so. No round trip to the browser (see
    /// [`Page::exists`] for one).
    pub fn gone(&mut self, page: &Page) -> Option<Gone> {
        if !self.process.is_running() {
            Some(Gone::Exited)
        } else if self.conn.is_closed() {
            Some(Gone::Disconnected)
        } else {
            page.is_closed().then_some(Gone::PageClosed)
        }
    }

    pub fn profile_dir(&self) -> &Path {
        &self.dir
    }

    /// Opens another page (tab) to drive, beside the ones the browser has.
    pub fn new_page(&self) -> Result<Page> {
        Page::attach(self.conn.clone())
    }

    /// The page the browser opened with (the `about:blank` of its command line), to drive instead of opening a second
    /// one beside it: the window then has exactly one tab, and any further page is one the user opened. Waits up to
    /// [`START_PAGE_WAIT`] for it to appear; a browser that shows none by then gets a new page.
    pub fn start_page(&self) -> Result<Page> {
        let end = Instant::now() + START_PAGE_WAIT;
        loop {
            if let Some(target) = self.first_page_target()? {
                return Page::adopt(self.conn.clone(), target);
            }
            if Instant::now() >= end {
                log::warn!("The browser showed no start page within {START_PAGE_WAIT:?}; a new one is opened");
                return self.new_page();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn first_page_target(&self) -> Result<Option<String>> {
        let r = self.conn.call(None, "Target.getTargets", json!({}), Duration::from_secs(5))?;
        let pages: Vec<&Value> = r
            .get("targetInfos")
            .and_then(Value::as_array)
            .map(|t| t.iter().filter(|i| i.get("type").and_then(Value::as_str) == Some("page")).collect())
            .unwrap_or_default();
        let blank = pages.iter().find(|i| i.get("url").and_then(Value::as_str) == Some(START_PAGE));
        Ok(blank
            .or(pages.first())
            .and_then(|i| i.get("targetId").and_then(Value::as_str))
            .map(str::to_string))
    }

    /// The number of open pages (tabs and windows) of the browser; a page the user opened shows up here.
    pub fn page_count(&self) -> Result<usize> {
        let r = self.conn.call(None, "Target.getTargets", json!({}), Duration::from_secs(5))?;
        Ok(r.get("targetInfos").and_then(Value::as_array).map_or(0, |t| {
            t.iter().filter(|i| i.get("type").and_then(Value::as_str) == Some("page")).count()
        }))
    }

    /// Closes the browser the way a user would (`Browser.close`), waits up to `grace` for it to exit, ends it by force
    /// when it does not, and removes a throwaway profile. Returns true when it exited by itself.
    pub fn close_gracefully(&mut self, grace: Duration) -> bool {
        self.alive.store(false, Ordering::Release);
        let _ = self.conn.call(None, "Browser.close", json!({}), grace.min(Duration::from_secs(2)));
        let end = Instant::now() + grace;
        let mut exited = !self.process.is_running();
        while !exited && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(25));
            exited = !self.process.is_running();
        }
        // Whatever is left of the process tree (helpers, or the browser itself when it ignored the request) ends now.
        self.process.kill();
        cleanup(&self.dir, self.throwaway);
        exited
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
        cleanup(&self.dir, self.throwaway);
    }
}

/// A watchdog that ends the browser's tree by identity ([`paddock_proc::kill_tree`], ADR 0033) when `max` has passed
/// since its spawn, even if nobody calls it; it sets `timed_out` first, so a start it ended says why.
fn watchdog(alive: Arc<AtomicBool>, timed_out: Arc<AtomicBool>, id: Option<ProcessId>, max: Duration) {
    // cap: the browser's recorded lifetime (LaunchOptions::max_lifetime), the last resort that also bounds a start
    let end = Instant::now() + max;
    let _ = std::thread::Builder::new().name("chromium-watchdog".into()).spawn(move || {
        while alive.load(Ordering::Acquire) {
            if Instant::now() >= end {
                log::warn!("The browser reached its time limit ({max:?}) and is ended");
                timed_out.store(true, Ordering::Release);
                if let Some(id) = id {
                    paddock_proc::kill_tree(id);
                }
                return;
            }
            // pace: how often the watchdog looks; it bounds nothing
            std::thread::sleep(Duration::from_millis(250));
        }
    });
}

fn cleanup(dir: &Path, throwaway: bool) {
    if throwaway {
        // Chrome may still be flushing for a moment after it was killed.
        for _ in 0..5 {
            if std::fs::remove_dir_all(dir).is_ok() || !dir.exists() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// What the start judge looks at: the browser process, its `DevToolsActivePort` file and the endpoint it names.
struct BrowserProbe<'a> {
    dir: &'a Path,
    process: &'a mut Supervised,
    /// The browser's identity, taken right after the spawn, for its tree's CPU time (ADR 0033).
    id: Option<ProcessId>,
    conn: Option<Connection>,
    /// The last refused or half-made handshake (the file can appear a moment before the endpoint takes connections).
    refused: Option<Error>,
    /// How many handshakes were refused.
    refusals: u32,
    /// A failure that is not worth trying again.
    failed: Option<Error>,
}

impl StartProbe for BrowserProbe<'_> {
    fn liveness(&mut self) -> Liveness {
        if self.process.is_running() {
            Liveness::Running
        } else {
            // The browser's output goes nowhere (a GUI program), so there is no tail to show.
            Liveness::Exited(ExitInfo {
                status: None,
                stderr_tail: None,
            })
        }
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
            Ok(c) => {
                self.conn = Some(c);
                true
            }
            Err(e @ Error::Connection(_)) => {
                if self.refusals == 0 {
                    log::debug!("The browser's DevTools endpoint refused the first handshake ({e}); trying again");
                }
                self.refusals = self.refusals.saturating_add(1);
                self.refused = Some(e);
                false
            }
            Err(e) => {
                self.failed = Some(e);
                true
            }
        }
    }

    fn cpu_time(&mut self) -> Option<Duration> {
        paddock_proc::tree_cpu_time(self.id?)
    }
}

/// Waits one tick of the start watch on the real clock.
struct Ticks;

impl Pace for Ticks {
    fn wait(&mut self, tick: Duration) -> bool {
        // pace: the poll interval of the start watch (Patience::tick); it bounds nothing
        std::thread::sleep(tick);
        true
    }
}

/// Why a judged start did not become ready.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StartFailure {
    /// The process ended before its handshake.
    Exited(ExitInfo),
    /// The judge called it stuck: spinning without becoming ready, idle, or unobservable past the hang guard.
    Stuck(StallReport),
    /// The pace was cancelled.
    Cancelled,
}

/// Drives the shared start judge (ADR 0031) over `probe` until the browser is ready, has exited or is stuck. A note
/// ("still starting") is not a reason to give up; only a stuck browser is. Returns the probe for what it holds (the
/// connection made at the handshake).
fn judge_start<P: StartProbe, C: Pace>(patience: Patience, probe: P, pace: C) -> (std::result::Result<(), StartFailure>, P) {
    let mut watch = StartWatch::new(patience, "the browser", probe, pace);
    let outcome = loop {
        match watch.wait() {
            Watched::State(StartState::Ready) => break Ok(()),
            Watched::State(StartState::Exited(info)) => break Err(StartFailure::Exited(info)),
            Watched::State(StartState::Stalled(r)) if r.cause.is_stuck() => break Err(StartFailure::Stuck(r)),
            Watched::State(StartState::Stalled(_) | StartState::Starting) => {}
            Watched::Cancelled => break Err(StartFailure::Cancelled),
        }
    };
    (outcome, watch.into_probe())
}

/// Waits for the `DevToolsActivePort` file Chrome writes into the profile and connects to the browser endpoint, judged on
/// the browser's run state (ADR 0031) rather than on a fixed time: a start that is slow because the machine is busy is
/// waited for; one that exits fails at once; one that is stuck fails after the patience. The lifetime watchdog
/// (`opts.max_lifetime`) is the last-resort cap, and a start it ended says so.
fn connect(
    dir: &Path,
    process: &mut Supervised,
    id: Option<ProcessId>,
    opts: &LaunchOptions,
    timed_out: &AtomicBool,
) -> Result<Connection> {
    let probe = BrowserProbe {
        dir,
        process,
        id,
        conn: None,
        refused: None,
        refusals: 0,
        failed: None,
    };
    let (outcome, probe) = judge_start(opts.patience, probe, Ticks);
    match outcome {
        Ok(()) => match (probe.conn, probe.failed) {
            (Some(conn), _) => Ok(conn),
            (None, Some(e)) => Err(e),
            (None, None) => Err(Error::Closed),
        },
        Err(StartFailure::Exited(_)) if timed_out.load(Ordering::Acquire) => Err(Error::Launch(format!(
            "the browser did not start within {} s (its time limit)",
            opts.max_lifetime.as_secs()
        ))),
        Err(StartFailure::Exited(_)) => Err(Error::Launch("the browser exited right after it started".into())),
        // The last refused handshake says more than the stall did.
        Err(StartFailure::Stuck(report)) => Err(match probe.refused {
            Some(Error::Connection(m)) => Error::Connection(format!(
                "{m} (the browser's endpoint refused {} handshakes; {})",
                probe.refusals, report.message
            )),
            _ => Error::Launch(report.message),
        }),
        Err(StartFailure::Cancelled) => Err(Error::Closed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default options with throwaway profiles below `base`.
    #[cfg(unix)]
    fn opts_in(base: &Path) -> LaunchOptions {
        LaunchOptions {
            profile: Profile::Throwaway(base.join("profiles")),
            ..LaunchOptions::default()
        }
    }

    /// A throwaway profile lives below the folder the caller names (its own cache folder), never in the shared temp folder.
    #[test]
    fn a_throwaway_profile_is_made_below_the_callers_folder_and_never_in_the_shared_temp_folder() {
        let base = tempfile::tempdir().unwrap();
        let d = unique_dir(&base.path().join("browser-profiles")).unwrap();
        assert!(d.starts_with(base.path()) && d.is_dir(), "{d:?}");
        assert!(d.file_name().unwrap().to_string_lossy().starts_with("chromium-driver-"));
    }

    /// The end of a launch removes a throwaway profile (it lives below the caller's cache folder, so a leftover would stay
    /// for good) and never a kept one.
    #[test]
    fn cleanup_removes_a_throwaway_profile_and_never_a_kept_one() {
        let base = tempfile::tempdir().unwrap();
        let throwaway = unique_dir(base.path()).unwrap();
        let kept = base.path().join("kept");
        std::fs::create_dir(&kept).unwrap();
        cleanup(&throwaway, true);
        cleanup(&kept, false);
        assert!(!throwaway.exists());
        assert!(kept.exists());
    }

    #[test]
    fn a_launch_without_a_folder_for_throwaway_profiles_is_refused() {
        assert!(unique_dir(Path::new("")).is_err());
        let browser = Browser {
            flavor: crate::Flavor::Chromium,
            path: PathBuf::from("never-started"),
        };
        let e = Chromium::launch(&browser, &LaunchOptions::default())
            .err()
            .expect("no folder, no launch");
        assert!(matches!(&e, Error::Launch(m) if m.contains("no folder was given")), "{e:?}");
    }

    #[test]
    fn every_launch_gets_its_own_profile_folder() {
        let base = tempfile::tempdir().unwrap();
        let dirs: std::collections::HashSet<PathBuf> = (0..100).map(|_| unique_dir(base.path()).unwrap()).collect();
        assert_eq!(dirs.len(), 100);
    }

    #[test]
    fn keeps_the_automation_banner_and_hides_nothing() {
        let a = command_line(Flavor::Chromium, Path::new("/tmp/p"), &LaunchOptions::default(), Sandbox::On);
        assert!(a.contains(&"--enable-automation".to_string()));
        assert!(a
            .iter()
            .all(|x| !x.contains("AutomationControlled") && !x.contains("user-agent") && !x.contains("headless")));
        assert!(a.contains(&"--user-data-dir=/tmp/p".to_string()));
    }

    #[test]
    fn the_sandbox_is_dropped_only_for_root_that_asked_for_it() {
        assert_eq!(Sandbox::decide(true, Some("1"), true), Sandbox::Off);
        assert_eq!(
            Sandbox::decide(true, Some("1"), false),
            Sandbox::On,
            "a user's browser keeps its sandbox"
        );
        assert_eq!(
            Sandbox::decide(true, None, true),
            Sandbox::On,
            "root without the opt-in is not changed"
        );
        assert_eq!(Sandbox::decide(true, Some(""), true), Sandbox::On);
        assert_eq!(Sandbox::decide(true, Some("0"), true), Sandbox::On);
    }

    #[test]
    fn a_release_build_keeps_the_sandbox_whatever_the_environment_says() {
        for value in [Some("1"), Some("true"), None] {
            assert_eq!(Sandbox::decide(false, value, true), Sandbox::On, "{value:?}");
        }
    }

    #[test]
    fn the_start_page_is_last_for_every_combination_of_options() {
        let window = Some(WindowBounds {
            x: 10,
            y: 20,
            width: 300,
            height: 200,
        });
        for sandbox in [Sandbox::On, Sandbox::Off] {
            for window in [None, window] {
                for disk_cache_bytes in [None, Some(99)] {
                    let opts = LaunchOptions {
                        window,
                        disk_cache_bytes,
                        ..LaunchOptions::default()
                    };
                    let a = command_line(Flavor::Chromium, Path::new("/p"), &opts, sandbox);
                    assert_eq!(a.last().map(String::as_str), Some(START_PAGE), "{a:?}");
                    assert_eq!(a.iter().filter(|x| *x == START_PAGE).count(), 1, "{a:?}");
                    assert_eq!(a.contains(&"--no-sandbox".to_string()), sandbox == Sandbox::Off, "{a:?}");
                    assert_eq!(a.contains(&"--disk-cache-size=99".to_string()), disk_cache_bytes.is_some(), "{a:?}");
                }
            }
        }
    }

    /// A Chromium build starts without its testing field trials; branded Chrome and Edge get no switch.
    #[test]
    fn chromium_builds_start_without_testing_field_trials() {
        for flavor in [Flavor::Chromium, Flavor::Chrome, Flavor::Edge] {
            let a = command_line(flavor, Path::new("/p"), &LaunchOptions::default(), Sandbox::On);
            assert_eq!(
                a.contains(&NO_TESTING_FIELD_TRIALS.to_string()),
                flavor == Flavor::Chromium,
                "{a:?}"
            );
            assert_eq!(a.last().map(String::as_str), Some(START_PAGE), "{a:?}");
        }
    }

    #[test]
    fn the_window_opens_on_screen() {
        assert!(
            !command_line(Flavor::Chromium, Path::new("/p"), &LaunchOptions::default(), Sandbox::On)
                .iter()
                .any(|x| x.starts_with("--window-position") || x.contains("headless") || x.contains("offscreen"))
        );
        let placed = command_line(
            Flavor::Chromium,
            Path::new("/p"),
            &LaunchOptions {
                window: Some(WindowBounds {
                    x: -1800,
                    y: 40,
                    width: 1100,
                    height: 800,
                }),
                ..LaunchOptions::default()
            },
            Sandbox::On,
        );
        assert!(placed.contains(&"--window-position=-1800,40".to_string()), "{placed:?}");
        assert!(placed.contains(&"--window-size=1100,800".to_string()));
        assert_eq!(placed.iter().filter(|a| a.starts_with("--window-size")).count(), 1);
    }

    #[test]
    fn theme_is_written_and_merged() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("Default")).unwrap();
        std::fs::write(d.path().join("Default/Preferences"), r#"{"browser":{"other":1},"x":2}"#).unwrap();
        write_theme(d.path(), 0x336699).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(d.path().join("Default/Preferences")).unwrap()).unwrap();
        assert_eq!(v["browser"]["other"], 1);
        assert_eq!(v["x"], 2);
        assert_eq!(v["browser"]["theme"]["user_color2"], 0xFF336699u32 as i32);
        assert_eq!(v["browser"]["theme"]["user_color"], v["browser"]["theme"]["user_color2"]);
    }

    /// A browser start played tick by tick: it runs (using a little CPU each tick) for `ready_after` ticks and then
    /// answers its handshake, or exits after `exit_after` ticks; `cpu_step` is the CPU it uses per tick (0: idle).
    struct ScriptedStart {
        tick: u32,
        ready_after: Option<u32>,
        exit_after: Option<u32>,
        cpu_step: Duration,
    }

    impl StartProbe for ScriptedStart {
        fn liveness(&mut self) -> Liveness {
            match self.exit_after {
                Some(n) if self.tick >= n => Liveness::Exited(ExitInfo {
                    status: Some(1),
                    stderr_tail: None,
                }),
                _ => Liveness::Running,
            }
        }

        fn ready(&mut self) -> bool {
            self.ready_after.is_some_and(|n| self.tick >= n)
        }

        fn cpu_time(&mut self) -> Option<Duration> {
            Some(self.cpu_step * self.tick)
        }
    }

    /// The injected clock: every wait advances the probe's time by one tick at once (no real time passes) and counts it.
    struct FakeClock<'a> {
        probe_tick: &'a std::cell::Cell<u32>,
        waited: Duration,
    }

    impl Pace for &mut FakeClock<'_> {
        fn wait(&mut self, tick: Duration) -> bool {
            self.probe_tick.set(self.probe_tick.get() + 1);
            self.waited += tick;
            true
        }
    }

    /// Runs [`judge_start`] over a scripted start on the fake clock; answers the outcome and the time the browser was
    /// given (in the clock's time, not the wall's).
    fn judge(ready_after: Option<u32>, exit_after: Option<u32>, cpu_step: Duration) -> (std::result::Result<(), StartFailure>, Duration) {
        // The probe reads its time from a cell the clock advances, so the two stay in step.
        struct Shared<'a> {
            script: ScriptedStart,
            time: &'a std::cell::Cell<u32>,
        }
        impl StartProbe for Shared<'_> {
            fn liveness(&mut self) -> Liveness {
                self.script.tick = self.time.get();
                self.script.liveness()
            }
            fn ready(&mut self) -> bool {
                self.script.tick = self.time.get();
                self.script.ready()
            }
            fn cpu_time(&mut self) -> Option<Duration> {
                self.script.tick = self.time.get();
                self.script.cpu_time()
            }
        }
        let time = std::cell::Cell::new(0);
        let mut clock = FakeClock {
            probe_tick: &time,
            waited: Duration::ZERO,
        };
        let probe = Shared {
            script: ScriptedStart {
                tick: 0,
                ready_after,
                exit_after,
                cpu_step,
            },
            time: &time,
        };
        let (outcome, _) = judge_start(Patience::default(), probe, &mut clock);
        (outcome, clock.waited)
    }

    /// The flake of CI runs 7213, 7250 and the one after: on a loaded runner a real browser took longer than a fixed
    /// 20 s to write its `DevToolsActivePort` and the launch failed with `Timeout("the browser start")` while the
    /// browser was working. A start that keeps using CPU is waited for however long it takes (ADR 0031).
    #[test]
    fn a_slow_start_that_makes_progress_is_not_failed() {
        let tick = Patience::default().tick;
        // 45 s on the clock (three times the old limit's worth of margin), using CPU all along, then ready.
        let ticks = u32::try_from(Duration::from_secs(45).as_millis() / tick.as_millis()).unwrap();
        let (outcome, waited) = judge(Some(ticks), None, Duration::from_millis(5));
        assert_eq!(outcome, Ok(()), "a working start is not failed after {waited:?}");
        assert!(waited >= Duration::from_secs(45), "{waited:?}");
    }

    #[test]
    fn a_start_that_dies_fails_at_once() {
        let (outcome, waited) = judge(None, Some(3), Duration::from_millis(5));
        assert!(matches!(outcome, Err(StartFailure::Exited(_))), "{outcome:?}");
        assert!(waited <= Patience::default().tick * 3, "no waiting after the exit: {waited:?}");
    }

    #[test]
    fn a_start_that_does_nothing_is_stuck_after_the_idle_patience_not_a_fixed_limit() {
        let p = Patience::default();
        let (outcome, waited) = judge(None, None, Duration::ZERO);
        match outcome {
            Err(StartFailure::Stuck(r)) => assert!(matches!(r.cause, paddock_proc::StallCause::Idle(_)), "{r:?}"),
            other => panic!("expected an idle stall, got {other:?}"),
        }
        assert!(waited >= p.idle, "{waited:?}");
        assert!(
            waited < p.idle + p.tick * (p.cpu_every * 2),
            "bounded by the idle patience: {waited:?}"
        );
    }

    /// The real launch path, with a "browser" that exits at once: it fails fast and says so.
    #[cfg(unix)]
    #[test]
    fn a_browser_that_exits_at_launch_fails_fast_with_a_launch_error() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("dies.sh");
        std::fs::write(&exe, "#!/bin/sh\nexit 3\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let browser = Browser {
            flavor: crate::Flavor::Chromium,
            path: exe,
        };
        let base = tempfile::tempdir().unwrap();
        let e = Chromium::launch(&browser, &opts_in(base.path()))
            .err()
            .expect("a dead browser fails");
        assert!(matches!(&e, Error::Launch(m) if m.contains("exited")), "{e:?}");
    }

    /// A process that stays alive doing nothing: a browser that never answers.
    #[cfg(unix)]
    #[allow(clippy::disallowed_methods, reason = "the test starts its own `sleep` child")]
    fn idle_child() -> Supervised {
        let mut cmd = std::process::Command::new("sleep");
        cmd.arg("3600");
        Supervised::spawn(cmd).unwrap()
    }

    #[cfg(unix)]
    fn probe<'a>(dir: &'a Path, process: &'a mut Supervised) -> BrowserProbe<'a> {
        let id = process.id().and_then(ProcessId::of);
        BrowserProbe {
            dir,
            process,
            id,
            conn: None,
            refused: None,
            refusals: 0,
            failed: None,
        }
    }

    /// A `DevToolsActivePort` that names a port nobody listens on is a refused handshake: not ready, tried again, and named
    /// (with the count) when the start is given up as stuck.
    #[cfg(unix)]
    #[test]
    fn a_refused_endpoint_is_retried_and_named_when_the_start_is_given_up() {
        let d = tempfile::tempdir().unwrap();
        // conformance: freed-port — refusal is the case under test; if the port were taken, the endpoint would answer and the assert fails
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        std::fs::write(d.path().join("DevToolsActivePort"), format!("{closed}\n/devtools/browser/x\n")).unwrap();
        let mut process = idle_child();
        {
            let mut p = probe(d.path(), &mut process);
            assert!(!p.ready(), "a refused handshake is not ready");
            assert!(matches!(p.refused, Some(Error::Connection(_))), "{:?}", p.refused);
            assert!(p.failed.is_none(), "a refusal is tried again, not a failure");
            assert_eq!(p.refusals, 1);
        }
        let id = process.id().and_then(ProcessId::of);
        let opts = LaunchOptions {
            patience: Patience {
                idle: Duration::from_millis(300),
                tick: Duration::from_millis(10),
                cpu_every: 2,
                ..Patience::default()
            },
            ..LaunchOptions::default()
        };
        let e = connect(d.path(), &mut process, id, &opts, &AtomicBool::new(false))
            .err()
            .expect("an endpoint that keeps refusing is given up");
        assert!(
            matches!(&e, Error::Connection(m) if m.contains("refused") && !m.contains("refused 0 ")),
            "the refused handshake is named: {e:?}"
        );
        process.kill();
    }

    /// The lifetime watchdog runs from the spawn, so it also ends a start that never becomes ready, and the error says so.
    #[cfg(unix)]
    #[test]
    fn a_start_that_outlasts_the_lifetime_is_ended_and_names_the_time_limit() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("never-ready.sh");
        std::fs::write(&exe, "#!/bin/sh\nexec sleep 3600\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let browser = Browser {
            flavor: crate::Flavor::Chromium,
            path: exe,
        };
        let base = tempfile::tempdir().unwrap();
        let opts = LaunchOptions {
            max_lifetime: Duration::from_millis(300),
            ..opts_in(base.path())
        };
        let e = Chromium::launch(&browser, &opts).err().expect("the watchdog ends the start");
        assert!(matches!(&e, Error::Launch(m) if m.contains("time limit")), "{e:?}");
    }

    /// The probe reads the CPU time of the browser's tree by its identity, and it grows while the browser works.
    #[cfg(unix)]
    #[test]
    #[allow(clippy::disallowed_methods, reason = "the test starts its own busy `sh` child")]
    fn the_probe_reads_a_working_browser_s_cpu_time_and_it_grows() {
        let d = tempfile::tempdir().unwrap();
        let mut cmd = std::process::Command::new("sh");
        cmd.args(["-c", "while :; do :; done"]);
        let mut process = Supervised::spawn(cmd).unwrap();
        {
            let mut p = probe(d.path(), &mut process);
            let first = p.cpu_time().expect("the OS reads the tree's CPU time");
            // conformance: hang-guard — waits on the condition; only a child that never runs reaches it
            let end = Instant::now() + Duration::from_secs(30);
            loop {
                let now = p.cpu_time().expect("the OS reads the tree's CPU time");
                if now > first {
                    break;
                }
                assert!(Instant::now() < end, "the CPU time of a busy child did not grow");
                std::thread::yield_now();
            }
        }
        process.kill();
    }

    /// Not a test: the fake browser process re-runs this binary on this function (see [`crate::fake`]).
    #[test]
    fn fake_browser_process() {
        crate::fake::serve_if_requested();
    }

    #[cfg(unix)]
    fn launch_fake(mode: crate::fake::Mode, tabs: usize) -> (tempfile::TempDir, crate::fake::FakeBrowser, Chromium) {
        let d = tempfile::tempdir().unwrap();
        let fake = crate::fake::FakeBrowser::install(d.path(), "launch::tests::fake_browser_process", mode, tabs).unwrap();
        let exe = Browser {
            flavor: crate::Flavor::Chromium,
            path: fake.exe.clone(),
        };
        let b = Chromium::launch(&exe, &opts_in(d.path())).expect("the fake starts");
        (d, fake, b)
    }

    #[cfg(unix)]
    #[test]
    fn the_launch_command_line_has_no_offscreen_or_headless_argument() {
        let (_d, fake, b) = launch_fake(crate::fake::Mode::Obey, 1);
        let args = fake.args();
        assert!(args.iter().any(|a| a.starts_with("--user-data-dir=")));
        assert!(args.iter().any(|a| a == "--enable-automation"));
        assert!(!args
            .iter()
            .any(|a| a.contains("window-position") || a.contains("headless") || a.contains("-32000")));
        drop(b);
    }

    #[cfg(unix)]
    #[test]
    fn close_gracefully_lets_the_browser_exit_and_removes_the_profile() {
        let (_d, _fake, mut b) = launch_fake(crate::fake::Mode::Obey, 1);
        let (dir, pid) = (b.profile_dir().to_path_buf(), b.pid().unwrap());
        assert!(dir.exists());
        // conformance: hang-guard — the exit is waited for on the condition; a loaded runner needs more than the grace
        assert!(b.close_gracefully(Duration::from_secs(40)), "it exited by itself");
        assert!(!dir.exists(), "the throwaway profile is gone");
        assert!(!crate::process::process_exists(pid) || !b.is_running());
    }

    #[cfg(unix)]
    #[test]
    fn a_browser_that_ignores_close_is_killed_after_the_timeout() {
        let (_d, _fake, mut b) = launch_fake(crate::fake::Mode::IgnoreClose, 1);
        let dir = b.profile_dir().to_path_buf();
        let start = Instant::now();
        assert!(!b.close_gracefully(Duration::from_millis(400)), "it had to be ended by force");
        assert!(start.elapsed() < Duration::from_secs(5), "bounded");
        assert!(!b.is_running());
        assert!(!dir.exists());
    }

    #[cfg(unix)]
    #[test]
    fn page_count_sees_the_pages_the_browser_has() {
        let (_d, _fake, b) = launch_fake(crate::fake::Mode::Obey, 3);
        assert_eq!(b.page_count().unwrap(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn without_the_sandbox_the_start_page_is_still_the_last_argument() {
        let d = tempfile::tempdir().unwrap();
        let fake = crate::fake::FakeBrowser::install(d.path(), "launch::tests::fake_browser_process", crate::fake::Mode::Obey, 1).unwrap();
        let exe = Browser {
            flavor: crate::Flavor::Chromium,
            path: fake.exe.clone(),
        };
        let opts = LaunchOptions {
            disk_cache_bytes: Some(1234),
            ..opts_in(d.path())
        };
        let mut b = Chromium::launch_with(&exe, &opts, Sandbox::Off).expect("the fake starts");
        let args = fake.args();
        assert!(args.contains(&"--no-sandbox".to_string()), "{args:?}");
        assert!(args.contains(&"--disk-cache-size=1234".to_string()), "{args:?}");
        assert_eq!(
            args.last().map(String::as_str),
            Some("about:blank"),
            "the start page stays last: {args:?}"
        );
        assert_eq!(args.iter().filter(|a| *a == "about:blank").count(), 1, "{args:?}");
        b.kill();
    }

    #[cfg(unix)]
    #[test]
    fn a_kept_profile_is_used_with_a_capped_cache_and_survives_the_close() {
        let d = tempfile::tempdir().unwrap();
        let fake = crate::fake::FakeBrowser::install(d.path(), "launch::tests::fake_browser_process", crate::fake::Mode::Obey, 1).unwrap();
        let exe = Browser {
            flavor: crate::Flavor::Chromium,
            path: fake.exe.clone(),
        };
        let kept = d.path().join("kept-profile");
        let opts = LaunchOptions {
            profile: Profile::Persistent(kept.clone()),
            disk_cache_bytes: Some(1234),
            ..LaunchOptions::default()
        };
        let mut b = Chromium::launch(&exe, &opts).expect("the fake starts");
        let args = fake.args();
        assert!(args.contains(&format!("--user-data-dir={}", kept.display())), "{args:?}");
        assert!(args.contains(&"--disk-cache-size=1234".to_string()), "{args:?}");
        assert_eq!(args.last().map(String::as_str), Some("about:blank"), "the start page stays last");
        // conformance: hang-guard — the exit is waited for on the condition; a loaded runner needs more than the grace
        assert!(b.close_gracefully(Duration::from_secs(40)));
        assert!(kept.is_dir(), "a kept profile is not removed");
    }

    #[cfg(unix)]
    #[test]
    fn the_start_page_is_driven_so_the_window_has_one_tab() {
        let (_d, fake, b) = launch_fake(crate::fake::Mode::Obey, 1);
        let p = b.start_page().unwrap();
        assert_eq!(b.page_count().unwrap(), 1, "no second, empty tab beside it");
        assert!(p.exists().unwrap());
        assert!(!fake.calls().iter().any(|c| c == "Target.createTarget"), "{:?}", fake.calls());
        // A page opened besides it counts (the user opening a tab looks the same).
        b.new_page().unwrap();
        assert_eq!(b.page_count().unwrap(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn a_browser_without_a_start_page_gets_a_new_one_after_a_bounded_wait() {
        let (_d, fake, b) = launch_fake(crate::fake::Mode::Obey, 0);
        let started = Instant::now();
        let p = b.start_page().unwrap();
        assert!(started.elapsed() < START_PAGE_WAIT + Duration::from_secs(3), "bounded");
        assert!(fake.calls().iter().any(|c| c == "Target.createTarget"));
        assert!(p.exists().unwrap());
        assert_eq!(b.page_count().unwrap(), 1);
    }

    #[cfg(unix)]
    fn wait_gone(b: &mut Chromium, p: &Page) -> Gone {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(g) = b.gone(p) {
                return g;
            }
            assert!(Instant::now() < end, "the end of the browser was not seen");
            std::thread::yield_now();
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_user_closing_the_window_ends_the_page_while_the_process_runs() {
        let (_d, fake, mut b) = launch_fake(crate::fake::Mode::Obey, 1);
        let p = b.new_page().unwrap();
        assert_eq!(b.gone(&p), None);
        assert!(p.exists().unwrap());
        fake.act(crate::fake::UserAction::CloseWindow).unwrap();
        assert_eq!(wait_gone(&mut b, &p), Gone::PageClosed);
        assert!(b.is_running(), "the browser itself still runs (macOS keeps it without a window)");
        assert!(!p.exists().unwrap());
        assert!(matches!(p.evaluate("1"), Err(Error::Closed)), "every call says the page was closed");
        assert!(matches!(p.info(), Err(Error::Closed)));
        assert!(matches!(
            p.navigate("http://127.0.0.1:9/", Duration::from_secs(5)),
            Err(Error::Closed)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_load_in_progress_ends_at_once_when_the_window_is_closed() {
        let (_d, fake, b) = launch_fake(crate::fake::Mode::Obey, 1);
        let p = b.new_page().unwrap();
        fake.act(crate::fake::UserAction::StallLoads).unwrap();
        let started = Instant::now();
        let load = std::thread::spawn(move || p.navigate("http://127.0.0.1:9/", Duration::from_secs(20)));
        fake.act(crate::fake::UserAction::CloseWindow).unwrap();
        let r = load.join().unwrap();
        assert!(matches!(r, Err(Error::Closed)), "{r:?}");
        assert!(started.elapsed() < Duration::from_secs(5), "no wait for the load timeout");
    }

    #[cfg(unix)]
    #[test]
    fn quitting_the_browser_is_seen_as_exited() {
        let (_d, fake, mut b) = launch_fake(crate::fake::Mode::Obey, 1);
        let p = b.new_page().unwrap();
        fake.act(crate::fake::UserAction::Quit).unwrap();
        assert!(matches!(wait_gone(&mut b, &p), Gone::Exited | Gone::Disconnected));
        let end = Instant::now() + Duration::from_secs(5);
        while b.gone(&p) != Some(Gone::Exited) {
            assert!(Instant::now() < end, "the process end is seen");
            std::thread::yield_now();
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_lost_connection_is_seen_while_the_process_runs() {
        let (_d, fake, mut b) = launch_fake(crate::fake::Mode::Obey, 1);
        let p = b.new_page().unwrap();
        fake.act(crate::fake::UserAction::DropConnection).unwrap();
        assert_eq!(wait_gone(&mut b, &p), Gone::Disconnected);
        assert!(matches!(p.evaluate("1"), Err(Error::Closed)));
        b.kill();
    }

    #[test]
    fn missing_executable_is_a_launch_error() {
        let exe = Browser {
            flavor: crate::Flavor::Chromium,
            path: "/nonexistent/chrome".into(),
        };
        let e = Chromium::launch(&exe, &LaunchOptions::default()).err().unwrap();
        assert!(matches!(e, Error::Launch(_)));
    }
}
