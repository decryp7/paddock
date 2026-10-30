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
#[cfg(any(test, feature = "testing"))]
pub const TEST_BROWSER_ENV: &str = "SPOTTER_TEST_BROWSER";

/// Environment variable overriding the folder that holds the test browser (default: see [`test_browser_dir`]).
#[cfg(any(test, feature = "testing"))]
pub const TEST_BROWSER_DIR_ENV: &str = "SPOTTER_TEST_BROWSER_DIR";

/// Set to `1` by CI where the test browser must be there: a missing one fails the test instead of skipping it.
#[cfg(any(test, feature = "testing"))]
pub const REQUIRE_TEST_BROWSER_ENV: &str = "SPOTTER_REQUIRE_TEST_BROWSER";

/// Where the pinned Chrome for Testing build for tests is installed (`node scripts/install-test-browser.mjs`).
#[cfg(any(test, feature = "testing"))]
pub fn test_browser_dir(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if let Some(d) = env(TEST_BROWSER_DIR_ENV).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    let home = env("HOME").or_else(|| env("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".cache").join("spotter-test-browser"))
}

/// The browser a test may launch in `mode`: `SPOTTER_TEST_BROWSER` or the pinned Chrome for Testing in the test cache
/// folder, never an installed Edge, Chrome or Chromium (killing a launched system Chrome leaves a code-sign clone of the
/// whole app behind on macOS, and tests must not touch the browser the user works in), and only where a real browser may
/// start at all: Linux in a throwaway CI session, under a virtual display for a window (`paddock_testkit::live_browser`;
/// never in a user's session, headless or not, rule 7). `None` after printing why (the test then returns); panics on a
/// job set up wrongly or a missing browser where [`REQUIRE_TEST_BROWSER_ENV`] is set.
#[cfg(any(test, feature = "testing"))]
pub fn find_for_tests(mode: crate::launch::Mode) -> Option<Browser> {
    use paddock_testkit::live_browser::{found_or_skip, process_env};
    found_or_skip(find_for_tests_with(
        Os::CURRENT,
        paddock_testkit::live_browser::Host::current().test_vm,
        &process_env,
        &|p| p.is_file(),
        mode,
    ))
}

/// [`find_for_tests`] with the OS, environment and file check injected.
#[cfg(any(test, feature = "testing"))]
fn find_for_tests_with(
    os: Os,
    test_vm: bool,
    env: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&Path) -> bool,
    mode: crate::launch::Mode,
) -> Result<paddock_testkit::live_browser::TestBrowser<Browser>, String> {
    use paddock_testkit::live_browser::{find_test_browser, BrowserWindow};
    let window = match mode {
        crate::launch::Mode::Headless => BrowserWindow::Headless,
        crate::launch::Mode::OffScreen | crate::launch::Mode::OnScreen => BrowserWindow::Shown,
    };
    let os_name = match os {
        Os::Windows => "windows",
        Os::Mac => "macos",
        Os::Linux => "linux",
    };
    let hint = "run `node scripts/install-test-browser.mjs` (pinned Chrome for Testing) or set SPOTTER_TEST_BROWSER to its \
                executable; installed browsers are never used in tests";
    find_test_browser(
        paddock_testkit::live_browser::Host { os: os_name, test_vm },
        env,
        window,
        REQUIRE_TEST_BROWSER_ENV,
        hint,
        || {
            if let Some(p) = env(TEST_BROWSER_ENV).filter(|p| !p.is_empty()).map(PathBuf::from) {
                return exists(&p).then_some(Browser {
                    flavor: Flavor::Chromium,
                    path: p,
                });
            }
            let dir = test_browser_dir(env)?;
            crate::install::locate_executable(&dir, os)
        },
    )
}

/// Where [`crate::install`] puts a downloaded Chromium inside the app data folder.
pub fn downloaded_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("chromium")
}

/// Environment variable that makes the test launch paths start the test browser without its sandbox. Only for a CI
/// job that runs as root in a container (Chrome refuses to start as root otherwise). The product never reads it.
#[cfg(any(test, feature = "testing"))]
pub const TEST_NO_SANDBOX_ENV: &str = "SPOTTER_TEST_BROWSER_NO_SANDBOX";

/// The flags [`test_browser_flags_with`] always gives: the tests only talk to loopback servers by IP address, so the test
/// browser is kept off the network. `--no-proxy-server`: the runner's proxy settings (on Windows the system's, with WPAD
/// or PAC auto-detection) are never looked up; Chrome already sends no loopback request through a proxy (its implicit
/// bypass rules, M71 for PAC and M72 for manual proxies, net/docs/proxy.md, read 2026-10-28), but it resolves the
/// configuration first. The host rule: no host resolves but loopback (the rule applies to IP literals too), so the requests Chrome for Testing 156 makes on its own
/// even with `--disable-background-networking` (measured 2026-10-28 with `--log-net-log`: www.google.com,
/// accounts.google.com, update.googleapis.com, clients2.google.com) fail at once instead of reaching real hosts.
#[cfg(any(test, feature = "testing"))]
pub const TEST_ISOLATION_FLAGS: [&str; 2] = [
    "--no-proxy-server",
    "--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE localhost, EXCLUDE 127.0.0.1",
];

/// Extra flags the tests pass to the test browser: [`TEST_ISOLATION_FLAGS`], plus `--no-sandbox` and
/// `--disable-dev-shm-usage` when [`TEST_NO_SANDBOX_ENV`] is `1`.
#[cfg(any(test, feature = "testing"))]
pub fn test_browser_flags_with(env: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let mut flags: Vec<String> = TEST_ISOLATION_FLAGS.iter().map(|f| f.to_string()).collect();
    if env(TEST_NO_SANDBOX_ENV).as_deref() == Some("1") {
        flags.extend(["--no-sandbox".into(), "--disable-dev-shm-usage".into()]);
    }
    flags
}

/// [`test_browser_flags_with`] for the process environment.
#[cfg(any(test, feature = "testing"))]
pub fn test_browser_flags() -> Vec<String> {
    test_browser_flags_with(&|k| std::env::var(k).ok())
}

/// The executable of a downloaded Chromium, if one was installed completely.
pub fn find_downloaded(data_dir: &Path) -> Option<Browser> {
    crate::install::locate_executable(&downloaded_dir(data_dir), Os::CURRENT)
}

/// The test-build gate of every program this driver starts (a launch, a self-test): never an installed browser, and no
/// real program outside a throwaway session (rule 7; `paddock_testkit::live_browser::check_launch`), however it was found.
#[cfg(any(test, feature = "testing"))]
pub(crate) fn check_test_launch(exe: &Path, window: paddock_testkit::live_browser::BrowserWindow) -> Result<(), String> {
    let installed = candidates(Os::CURRENT, &|k| std::env::var(k).ok()).into_iter().map(|b| b.path);
    paddock_testkit::live_browser::check_launch(exe, installed, window)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::Mode;

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

    /// The finder's search alone, as on a Linux CI job that may start the browser.
    fn found(os: Os, env: &dyn Fn(&str) -> Option<String>, exists: &dyn Fn(&Path) -> bool) -> Option<Browser> {
        use paddock_testkit::live_browser::{TestBrowser, LIVE_BROWSER_VAR};
        let allowed = |k: &str| if k == LIVE_BROWSER_VAR { Some("1".to_string()) } else { env(k) };
        match find_for_tests_with(os, false, &allowed, exists, Mode::Headless) {
            Ok(TestBrowser::Found(b)) => Some(b),
            _ => None,
        }
    }

    #[test]
    fn test_discovery_never_returns_a_system_install() {
        // Everything a user could have installed exists, and SPOTTER_CHROMIUM_PATH is set: tests still get nothing.
        let all = |k: &str| match k {
            "HOME" => Some("/nonexistent-home".to_string()),
            "SPOTTER_CHROMIUM_PATH" => Some("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".to_string()),
            _ => None,
        };
        assert!(found(Os::Linux, &all, &|_| true).is_none());
        // The explicit variable is honoured only when the file exists.
        let explicit = |k: &str| (k == TEST_BROWSER_ENV).then(|| "/opt/cft/chrome".to_string());
        assert_eq!(
            found(Os::Linux, &explicit, &|p| p == Path::new("/opt/cft/chrome")).unwrap().path,
            PathBuf::from("/opt/cft/chrome")
        );
        assert!(found(Os::Linux, &explicit, &|_| false).is_none());
    }

    /// Regression (rule 7; Debrief CI 7141, 7146, the same class here): the Mac and Windows runners run in a user's
    /// session, so a test browser there, headless or not, is a process of that session (and a headful one a window, a Dock
    /// or taskbar entry and the focus). A test browser is handed out only on Linux in a throwaway CI session.
    #[test]
    fn tests_get_no_browser_in_a_desktop_session() {
        use paddock_testkit::live_browser::{TestBrowser, LIVE_BROWSER_VAR};
        let ci = |k: &str| match k {
            TEST_BROWSER_ENV => Some("/opt/cft/chrome".to_string()),
            LIVE_BROWSER_VAR | REQUIRE_TEST_BROWSER_ENV => Some("1".to_string()),
            "DISPLAY" => Some(":99".to_string()),
            _ => None,
        };
        for os in [Os::Mac, Os::Windows] {
            for mode in [Mode::Headless, Mode::OffScreen, Mode::OnScreen] {
                assert!(
                    matches!(find_for_tests_with(os, false, &ci, &|_| true, mode), Ok(TestBrowser::Skip(_))),
                    "{os:?} {mode:?}"
                );
            }
        }
        for mode in [Mode::Headless, Mode::OffScreen, Mode::OnScreen] {
            assert!(
                matches!(
                    find_for_tests_with(Os::Linux, false, &ci, &|_| true, mode),
                    Ok(TestBrowser::Found(_))
                ),
                "{mode:?}"
            );
        }
        // A window needs the virtual display; headless does not.
        let no_display = |k: &str| if k == "DISPLAY" { None } else { ci(k) };
        assert!(find_for_tests_with(Os::Linux, false, &no_display, &|_| true, Mode::OffScreen).is_err());
        assert!(find_for_tests_with(Os::Linux, false, &no_display, &|_| true, Mode::Headless).is_ok());
        // A macOS or Windows test VM with `PADDOCK_LIVE_BROWSER=vm` may (the release's VM check); a desktop with `vm` fails.
        let vm = |k: &str| match k {
            TEST_BROWSER_ENV => Some("/opt/cft/chrome".to_string()),
            LIVE_BROWSER_VAR => Some("vm".to_string()),
            _ => None,
        };
        for os in [Os::Mac, Os::Windows] {
            assert!(matches!(
                find_for_tests_with(os, true, &vm, &|_| true, Mode::OffScreen),
                Ok(TestBrowser::Found(_))
            ));
            assert!(find_for_tests_with(os, false, &vm, &|_| true, Mode::Headless).is_err());
        }
        let _ = find_for_tests(Mode::Headless);
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
