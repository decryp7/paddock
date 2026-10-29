//! Finding a Chromium-based browser: installed (Edge, Chrome, Chromium; pure candidate lists, no registry) or
//! downloaded into the app's data folder by [`crate::install`].

use std::path::{Path, PathBuf};

/// Operating system family (a value, so candidate lists can be tested for every OS).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// Windows.
    Windows,
    /// macOS.
    Mac,
    /// Linux and other Unix.
    Linux,
}

impl Os {
    /// The OS this build runs on.
    pub const CURRENT: Os = if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::Mac
    } else {
        Os::Linux
    };
}

/// Which browser family an executable belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    /// Microsoft Edge.
    Edge,
    /// Google Chrome.
    Chrome,
    /// Chromium (or Chrome for Testing).
    Chromium,
}

impl Flavor {
    /// Display name.
    pub fn name(self) -> &'static str {
        match self {
            Flavor::Edge => "Microsoft Edge",
            Flavor::Chrome => "Google Chrome",
            Flavor::Chromium => "Chromium",
        }
    }
}

/// An executable that may be a Chromium browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Browser {
    /// Family.
    pub flavor: Flavor,
    /// Executable path.
    pub path: PathBuf,
}

/// Candidates in order of preference: Edge first on Windows (always present), Chrome first elsewhere.
/// `env` looks up an environment variable.
pub fn candidates(os: Os, env: &dyn Fn(&str) -> Option<String>) -> Vec<Browser> {
    let mut v = Vec::new();
    let mut add = |flavor, p: PathBuf| v.push(Browser { flavor, path: p });
    match os {
        Os::Windows => {
            let roots: Vec<PathBuf> = ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"]
                .iter()
                .filter_map(|k| env(k))
                .map(PathBuf::from)
                .collect();
            for r in &roots {
                add(Flavor::Edge, r.join(r"Microsoft\Edge\Application\msedge.exe"));
            }
            for r in &roots {
                add(Flavor::Chrome, r.join(r"Google\Chrome\Application\chrome.exe"));
            }
            for r in &roots {
                add(Flavor::Chromium, r.join(r"Chromium\Application\chrome.exe"));
            }
        }
        Os::Mac => {
            let mut roots = vec![PathBuf::from("/Applications")];
            if let Some(h) = env("HOME") {
                roots.push(PathBuf::from(h).join("Applications"));
            }
            for r in &roots {
                add(Flavor::Chrome, r.join("Google Chrome.app/Contents/MacOS/Google Chrome"));
            }
            for r in &roots {
                add(Flavor::Edge, r.join("Microsoft Edge.app/Contents/MacOS/Microsoft Edge"));
            }
            for r in &roots {
                add(Flavor::Chromium, r.join("Chromium.app/Contents/MacOS/Chromium"));
            }
        }
        Os::Linux => {
            for (f, n) in [
                (Flavor::Chrome, "google-chrome"),
                (Flavor::Chrome, "google-chrome-stable"),
                (Flavor::Edge, "microsoft-edge"),
                (Flavor::Edge, "microsoft-edge-stable"),
                (Flavor::Chromium, "chromium"),
                (Flavor::Chromium, "chromium-browser"),
            ] {
                for d in ["/usr/bin", "/usr/local/bin", "/snap/bin", "/opt/google/chrome"] {
                    add(f, Path::new(d).join(n));
                }
            }
        }
    }
    v
}

/// The first installed browser. `SPOTTER_CHROMIUM_PATH` (an explicit executable) wins.
pub fn find_installed() -> Option<Browser> {
    find_installed_with(Os::CURRENT, &|k| std::env::var(k).ok(), &|p| p.is_file())
}

/// [`find_installed`] with the OS, environment and file check injected.
pub fn find_installed_with(os: Os, env: &dyn Fn(&str) -> Option<String>, exists: &dyn Fn(&Path) -> bool) -> Option<Browser> {
    if let Some(p) = env("SPOTTER_CHROMIUM_PATH").map(PathBuf::from).filter(|p| exists(p)) {
        return Some(Browser {
            flavor: Flavor::Chromium,
            path: p,
        });
    }
    candidates(os, env).into_iter().find(|b| exists(&b.path))
}

/// Environment variable naming the one browser executable tests may launch.
pub const TEST_BROWSER_ENV: &str = "SPOTTER_TEST_BROWSER";

/// Environment variable overriding the folder that holds the test browser (default: see [`test_browser_dir`]).
pub const TEST_BROWSER_DIR_ENV: &str = "SPOTTER_TEST_BROWSER_DIR";

/// Set to `1` by CI where the test browser must be there: a missing one fails the test instead of skipping it.
pub const REQUIRE_TEST_BROWSER_ENV: &str = "SPOTTER_REQUIRE_TEST_BROWSER";

/// Where the pinned Chrome for Testing build for tests is installed (`node scripts/install-test-browser.mjs`).
pub fn test_browser_dir(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if let Some(d) = env(TEST_BROWSER_DIR_ENV).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    let home = env("HOME").or_else(|| env("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".cache").join("spotter-test-browser"))
}

/// The browser tests may launch: `SPOTTER_TEST_BROWSER` or the pinned Chrome for Testing in the test cache folder.
/// Never an installed Edge, Chrome or Chromium: killing a launched system Chrome leaves a code-sign clone of the
/// whole app behind on macOS, and tests must not touch the browser the user works in.
pub fn find_for_tests() -> Option<Browser> {
    find_for_tests_with(Os::CURRENT, &|k| std::env::var(k).ok(), &|p| p.is_file())
}

/// [`find_for_tests`] with the OS, environment and file check injected.
pub fn find_for_tests_with(os: Os, env: &dyn Fn(&str) -> Option<String>, exists: &dyn Fn(&Path) -> bool) -> Option<Browser> {
    if let Some(p) = env(TEST_BROWSER_ENV).filter(|p| !p.is_empty()).map(PathBuf::from) {
        return exists(&p).then_some(Browser {
            flavor: Flavor::Chromium,
            path: p,
        });
    }
    let dir = test_browser_dir(env)?;
    crate::install::locate_executable(&dir, os)
}

/// Where [`crate::install`] puts a downloaded Chromium inside the app data folder.
pub fn downloaded_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("chromium")
}

/// Environment variable that makes the test launch paths start the test browser without its sandbox. Only for a CI
/// job that runs as root in a container (Chrome refuses to start as root otherwise). The product never reads it.
pub const TEST_NO_SANDBOX_ENV: &str = "SPOTTER_TEST_BROWSER_NO_SANDBOX";

/// The flags [`test_browser_flags_with`] always gives: the tests only talk to loopback servers by IP address, so the test
/// browser is kept off the network. `--no-proxy-server`: the runner's proxy settings (on Windows the system's, with WPAD
/// or PAC auto-detection) are never looked up; Chrome already sends no loopback request through a proxy (its implicit
/// bypass rules, M71 for PAC and M72 for manual proxies, net/docs/proxy.md, read 2026-10-28), but it resolves the
/// configuration first. The host rule: no host resolves but loopback (the rule applies to IP literals too), so the requests Chrome for Testing 156 makes on its own
/// even with `--disable-background-networking` (measured 2026-10-28 with `--log-net-log`: www.google.com,
/// accounts.google.com, update.googleapis.com, clients2.google.com) fail at once instead of reaching real hosts.
pub const TEST_ISOLATION_FLAGS: [&str; 2] = [
    "--no-proxy-server",
    "--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE localhost, EXCLUDE 127.0.0.1",
];

/// Extra flags the tests pass to the test browser: [`TEST_ISOLATION_FLAGS`], plus `--no-sandbox` and
/// `--disable-dev-shm-usage` when [`TEST_NO_SANDBOX_ENV`] is `1`.
pub fn test_browser_flags_with(env: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let mut flags: Vec<String> = TEST_ISOLATION_FLAGS.iter().map(|f| f.to_string()).collect();
    if env(TEST_NO_SANDBOX_ENV).as_deref() == Some("1") {
        flags.extend(["--no-sandbox".into(), "--disable-dev-shm-usage".into()]);
    }
    flags
}

/// [`test_browser_flags_with`] for the process environment.
pub fn test_browser_flags() -> Vec<String> {
    test_browser_flags_with(&|k| std::env::var(k).ok())
}

/// Set to `1` by a test job whose display is a virtual one (`xvfb-run` in the Linux CI container): only then may a test
/// open a headful browser window ([`crate::launch::Mode::OffScreen`] or `OnScreen`). The Mac and Windows runners run in a
/// user's session, where an off-screen window still takes a Dock or taskbar entry and the focus (Paddock rule 7: no
/// windows on the desktop). Headless tests run everywhere. Honoured on Linux only.
pub const LIVE_BROWSER_VAR: &str = "PADDOCK_LIVE_BROWSER";

/// Whether a test may open a browser window here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowedTests {
    /// Linux, a display, and the job said it is a virtual one.
    Allowed,
    /// Not here, and where such tests run instead (the test prints it, so a skip is never silent).
    NotHere(String),
}

/// [`WindowedTests`] for `os` and the environment `env`. `Err` is a job set up wrongly, which fails the test: the
/// variable without a display (no `xvfb-run`), or `require_var` set on Linux without the variable (the windowed tests
/// would skip). A set variable is a non-blank value other than `0`.
pub fn windowed_tests_with(os: Os, env: &dyn Fn(&str) -> Option<String>, require_var: &str) -> Result<WindowedTests, String> {
    let set = |k: &str| env(k).is_some_and(|v| !v.trim().is_empty() && v.trim() != "0");
    if os != Os::Linux {
        return Ok(WindowedTests::NotHere(format!(
            "this test opens a browser window, so it runs only on Linux under a virtual display (Xvfb, the Linux CI leg with \
             {LIVE_BROWSER_VAR}=1), never on a {os:?} desktop (Paddock rule 7)"
        )));
    }
    match (set(LIVE_BROWSER_VAR), set("DISPLAY") || set("WAYLAND_DISPLAY")) {
        (true, true) => Ok(WindowedTests::Allowed),
        (true, false) => Err(format!(
            "{LIVE_BROWSER_VAR} is set but there is no display: run the tests under xvfb-run (a virtual display)"
        )),
        (false, _) if set(require_var) => Err(format!(
            "{require_var} is set on Linux but {LIVE_BROWSER_VAR} is not: run the tests under xvfb-run with \
             {LIVE_BROWSER_VAR}=1, or the windowed browser tests skip"
        )),
        (false, _) => Ok(WindowedTests::NotHere(format!(
            "this test opens a browser window: run it under xvfb-run with {LIVE_BROWSER_VAR}=1 (Linux only), never on a \
             desktop (Paddock rule 7)"
        ))),
    }
}

/// For a test that opens a headful browser window: `true` where it may (see [`windowed_tests_with`]); otherwise prints
/// why and returns `false` (the test then returns). Panics on a job set up wrongly.
pub fn windowed_tests_here() -> bool {
    match windowed_tests_with(Os::CURRENT, &|k| std::env::var(k).ok(), REQUIRE_TEST_BROWSER_ENV) {
        Ok(WindowedTests::Allowed) => true,
        Ok(WindowedTests::NotHere(why)) => {
            eprintln!("not run here: {why}");
            false
        }
        Err(why) => panic!("{why}"),
    }
}

/// The executable of a downloaded Chromium, if one was installed completely.
pub fn find_downloaded(data_dir: &Path) -> Option<Browser> {
    crate::install::locate_executable(&downloaded_dir(data_dir), Os::CURRENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(k: &str) -> Option<String> {
        match k {
            "ProgramFiles" => Some(r"C:\Program Files".into()),
            "HOME" => Some("/Users/u".into()),
            _ => None,
        }
    }

    #[test]
    fn windows_prefers_edge() {
        let c = candidates(Os::Windows, &env);
        assert_eq!(c[0].flavor, Flavor::Edge);
        assert!(c[0].path.to_string_lossy().contains("msedge.exe"));
        assert!(c.iter().any(|b| b.flavor == Flavor::Chrome));
    }

    #[test]
    fn mac_checks_user_applications_too() {
        let c = candidates(Os::Mac, &env);
        assert!(c.iter().any(|b| b.path.starts_with("/Users/u/Applications")));
        assert_eq!(c[0].flavor, Flavor::Chrome);
    }

    #[test]
    fn linux_lists_common_names() {
        let c = candidates(Os::Linux, &env);
        assert!(c.iter().any(|b| b.path.ends_with("chromium-browser")));
    }

    #[test]
    fn finds_first_existing_and_honours_override() {
        let only = PathBuf::from("/usr/bin/chromium");
        assert_eq!(
            find_installed_with(Os::Linux, &env, &|p| p == only).unwrap().flavor,
            Flavor::Chromium
        );
        let over = |k: &str| (k == "SPOTTER_CHROMIUM_PATH").then(|| "/x/chrome".to_string());
        assert_eq!(
            find_installed_with(Os::Linux, &over, &|p| p == Path::new("/x/chrome"))
                .unwrap()
                .path,
            PathBuf::from("/x/chrome")
        );
        assert!(find_installed_with(Os::Linux, &env, &|_| false).is_none());
        assert!(find_installed_with(Os::Windows, &|_| None, &|_| true).is_none());
    }

    #[test]
    fn test_discovery_never_returns_a_system_install() {
        // Everything a user could have installed exists, and SPOTTER_CHROMIUM_PATH is set: tests still get nothing.
        let all = |k: &str| match k {
            "HOME" => Some("/nonexistent-home".to_string()),
            "SPOTTER_CHROMIUM_PATH" => Some("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".to_string()),
            _ => None,
        };
        for os in [Os::Windows, Os::Mac, Os::Linux] {
            assert!(find_for_tests_with(os, &all, &|_| true).is_none(), "{os:?}");
        }
        // The explicit variable is honoured only when the file exists.
        let explicit = |k: &str| (k == TEST_BROWSER_ENV).then(|| "/opt/cft/chrome".to_string());
        assert_eq!(
            find_for_tests_with(Os::Linux, &explicit, &|p| p == Path::new("/opt/cft/chrome"))
                .unwrap()
                .path,
            PathBuf::from("/opt/cft/chrome")
        );
        assert!(find_for_tests_with(Os::Linux, &explicit, &|_| false).is_none());
    }

    /// Regression (Debrief CI 7141, 7146; the same class here): the Mac and Windows runners run in the user's session, so a
    /// headful test browser (off screen or not) put a window, a Dock or taskbar entry and a focus change on the user's
    /// desktop. A headful test runs only on Linux, where the job says its display is a virtual one (rule 7).
    #[test]
    fn a_windowed_test_runs_only_on_a_virtual_display() {
        let env =
            |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
        const XVFB: &[(&str, &str)] = &[(LIVE_BROWSER_VAR, "1"), ("DISPLAY", ":99"), (REQUIRE_TEST_BROWSER_ENV, "1")];
        for os in [Os::Mac, Os::Windows] {
            let Ok(WindowedTests::NotHere(why)) = windowed_tests_with(os, &env(XVFB), REQUIRE_TEST_BROWSER_ENV) else {
                panic!("{os:?} allowed a window")
            };
            assert!(why.contains("Linux") && why.contains(LIVE_BROWSER_VAR), "{why}");
        }
        assert_eq!(
            windowed_tests_with(Os::Linux, &env(XVFB), REQUIRE_TEST_BROWSER_ENV),
            Ok(WindowedTests::Allowed)
        );
        assert_eq!(
            windowed_tests_with(
                Os::Linux,
                &env(&[(LIVE_BROWSER_VAR, "1"), ("WAYLAND_DISPLAY", "w")]),
                REQUIRE_TEST_BROWSER_ENV
            ),
            Ok(WindowedTests::Allowed)
        );
        // A developer's Linux desktop: not there either.
        assert!(matches!(
            windowed_tests_with(Os::Linux, &env(&[("DISPLAY", ":0")]), REQUIRE_TEST_BROWSER_ENV),
            Ok(WindowedTests::NotHere(_))
        ));
        assert!(matches!(
            windowed_tests_with(
                Os::Linux,
                &env(&[(LIVE_BROWSER_VAR, "0"), ("DISPLAY", ":99")]),
                REQUIRE_TEST_BROWSER_ENV
            ),
            Ok(WindowedTests::NotHere(_))
        ));
        // A job set up wrongly fails: the variable without a display, or the browser required without the variable.
        let e = windowed_tests_with(
            Os::Linux,
            &env(&[(LIVE_BROWSER_VAR, "1"), ("DISPLAY", " ")]),
            REQUIRE_TEST_BROWSER_ENV,
        )
        .unwrap_err();
        assert!(e.contains("xvfb-run"), "{e}");
        let e = windowed_tests_with(Os::Linux, &env(&[(REQUIRE_TEST_BROWSER_ENV, "1")]), REQUIRE_TEST_BROWSER_ENV).unwrap_err();
        assert!(e.contains(LIVE_BROWSER_VAR), "{e}");
    }

    #[test]
    fn test_browser_dir_defaults_below_the_cache_and_can_be_overridden() {
        assert_eq!(test_browser_dir(&env), Some(PathBuf::from("/Users/u/.cache/spotter-test-browser")));
        let over = |k: &str| (k == TEST_BROWSER_DIR_ENV).then(|| "/t".to_string());
        assert_eq!(test_browser_dir(&over), Some(PathBuf::from("/t")));
        assert_eq!(test_browser_dir(&|_| None), None);
    }

    #[test]
    fn the_no_sandbox_switch_is_only_honoured_when_exactly_one() {
        let with = |v: &'static str| move |k: &str| (k == TEST_NO_SANDBOX_ENV).then(|| v.to_string());
        let isolated: Vec<String> = TEST_ISOLATION_FLAGS.iter().map(|f| f.to_string()).collect();
        let mut sandboxless = isolated.clone();
        sandboxless.extend(["--no-sandbox".into(), "--disable-dev-shm-usage".into()]);
        assert_eq!(test_browser_flags_with(&with("1")), sandboxless);
        for v in ["", "0", "true", "yes"] {
            assert_eq!(test_browser_flags_with(&with(v)), isolated, "{v}");
        }
        assert_eq!(test_browser_flags_with(&|_| None), isolated);
    }

    #[test]
    fn flavor_names() {
        assert_eq!(Flavor::Edge.name(), "Microsoft Edge");
        assert_eq!(Flavor::Chrome.name(), "Google Chrome");
        assert_eq!(Flavor::Chromium.name(), "Chromium");
    }
}
