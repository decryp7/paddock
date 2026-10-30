//! Finding a Chromium-based browser: an explicit path the app names, the app's downloaded Chrome for Testing
//! ([`crate::runtime`]), or one the user installed (Edge, Chrome, Chromium; pure candidate lists, no registry). Tests get
//! only `CHROMIUM_TEST_PATH`, never a browser the user installed: killing a launched system Chrome leaves a code-sign clone
//! of the whole app behind on macOS, and a test must not touch the browser the user works in (rule 7).

use paddock_runtime::OsKind;
use std::path::{Path, PathBuf};

/// The environment variable naming the one browser executable tests may launch (a pinned Chrome for Testing).
pub const TEST_BROWSER_ENV: &str = "CHROMIUM_TEST_PATH";

/// Which browser family an executable belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    /// Microsoft Edge.
    Edge,
    /// Google Chrome.
    Chrome,
    /// Chromium or Chrome for Testing.
    Chromium,
}

impl Flavor {
    /// The display name.
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
    pub flavor: Flavor,
    pub path: PathBuf,
}

impl Browser {
    /// A Chromium (or Chrome for Testing) at `path`.
    pub fn chromium(path: impl Into<PathBuf>) -> Browser {
        Browser {
            flavor: Flavor::Chromium,
            path: path.into(),
        }
    }
}

/// The OS this build runs on.
pub fn this_os() -> OsKind {
    os_of(std::env::consts::OS)
}

/// The OS of a `std::env::consts::OS` value (Linux for every other Unix).
fn os_of(os: &str) -> OsKind {
    match os {
        "windows" => OsKind::Windows,
        "macos" => OsKind::Macos,
        _ => OsKind::Linux,
    }
}

/// Installed-browser candidates in order of preference: Edge first on Windows (always present), Chrome first elsewhere.
/// `env` looks up an environment variable.
pub fn candidates(os: OsKind, env: &dyn Fn(&str) -> Option<String>) -> Vec<Browser> {
    let mut v = Vec::new();
    let mut add = |flavor, p: PathBuf| v.push(Browser { flavor, path: p });
    match os {
        OsKind::Windows => {
            let roots: Vec<PathBuf> = ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"]
                .iter()
                .filter_map(|k| env(k))
                .map(PathBuf::from)
                .collect();
            for (flavor, rel) in [
                (Flavor::Edge, r"Microsoft\Edge\Application\msedge.exe"),
                (Flavor::Chrome, r"Google\Chrome\Application\chrome.exe"),
                (Flavor::Chromium, r"Chromium\Application\chrome.exe"),
            ] {
                for r in &roots {
                    add(flavor, r.join(rel));
                }
            }
        }
        OsKind::Macos => {
            let mut roots = vec![PathBuf::from("/Applications")];
            if let Some(h) = env("HOME") {
                roots.push(PathBuf::from(h).join("Applications"));
            }
            for (flavor, rel) in [
                (
                    Flavor::Chrome,
                    "Google Chrome.app/Contents/MacOS/Google Chrome",
                ),
                (
                    Flavor::Edge,
                    "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
                ),
                (Flavor::Chromium, "Chromium.app/Contents/MacOS/Chromium"),
            ] {
                for r in &roots {
                    add(flavor, r.join(rel));
                }
            }
        }
        OsKind::Linux => {
            for (f, n) in [
                (Flavor::Chrome, "google-chrome"),
                (Flavor::Chrome, "google-chrome-stable"),
                (Flavor::Edge, "microsoft-edge"),
                (Flavor::Edge, "microsoft-edge-stable"),
                (Flavor::Chromium, "chromium"),
                (Flavor::Chromium, "chromium-browser"),
            ] {
                for d in [
                    "/usr/bin",
                    "/usr/local/bin",
                    "/snap/bin",
                    "/opt/google/chrome",
                ] {
                    add(f, Path::new(d).join(n));
                }
            }
        }
    }
    v
}

/// The browser for the product: `override_var` (an explicit executable the app lets the user name, such as
/// `SPOTTER_CHROMIUM_PATH`) when it exists, then the app's downloaded Chrome for Testing (`downloaded`, from
/// [`crate::runtime::installed`]), then the first installed candidate.
pub fn find(override_var: Option<&str>, downloaded: Option<Browser>) -> Option<Browser> {
    find_with(
        this_os(),
        &|k| std::env::var(k).ok(),
        &|p| p.is_file(),
        override_var,
        downloaded,
    )
}

/// [`find`] with the OS, the environment and the file check injected.
pub fn find_with(
    os: OsKind,
    env: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&Path) -> bool,
    override_var: Option<&str>,
    downloaded: Option<Browser>,
) -> Option<Browser> {
    override_var
        .and_then(env)
        .filter(|p| !p.is_empty())
        .map(Browser::chromium)
        .filter(|b| exists(&b.path))
        .or(downloaded.filter(|b| exists(&b.path)))
        .or_else(|| candidates(os, env).into_iter().find(|b| exists(&b.path)))
}

/// Flags for the browser tests launch (with `LaunchOptions::extra_args`): the tests only talk to loopback servers by IP
/// address, so the test browser is kept off the network. `--no-proxy-server`: the runner's proxy settings (on Windows
/// the system's, with WPAD or PAC auto-detection) are never looked up; Chrome already sends no loopback request through
/// a proxy (its implicit bypass rules, M71 for PAC and M72 for manual proxies, net/docs/proxy.md, read 2026-10-28), but
/// it resolves the configuration first. The host rule: no host resolves but loopback (the rule applies to IP literals
/// too), so the requests Chrome for Testing 156 makes on its own even with `--disable-background-networking` (measured
/// 2026-10-28 with `--log-net-log`: www.google.com, accounts.google.com, update.googleapis.com, clients2.google.com)
/// fail at once instead of reaching real hosts.
pub const TEST_ISOLATION_FLAGS: [&str; 2] = [
    "--no-proxy-server",
    "--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE localhost, EXCLUDE 127.0.0.1",
];

/// Set to `1` by CI where the test browser must be there: a missing one fails the test instead of skipping it.
#[cfg(any(test, feature = "test-support"))]
pub const REQUIRE_TEST_BROWSER_ENV: &str = "PADDOCK_REQUIRE_TEST_BROWSER";

/// The browser a test may launch with `window`: only `CHROMIUM_TEST_PATH` when it exists and is not one of the browsers
/// the user installed, and only where a real browser may start at all (Linux in a throwaway CI session,
/// `paddock_testkit::live_browser`: never in a user's session, headless or not, rule 7). `None` after printing why (the
/// test then returns); panics on a job set up wrongly or a missing browser where [`REQUIRE_TEST_BROWSER_ENV`] is set.
#[cfg(any(test, feature = "test-support"))]
pub fn find_for_tests(window: crate::Window) -> Option<Browser> {
    use paddock_testkit::live_browser::{found_or_skip, process_env};
    found_or_skip(find_for_tests_with(
        paddock_testkit::live_browser::Host::current(),
        &process_env,
        &|p| p.is_file(),
        window,
    ))
}

/// [`find_for_tests`] with the machine, the environment and the file check injected.
#[cfg(any(test, feature = "test-support"))]
fn find_for_tests_with(
    host: paddock_testkit::live_browser::Host,
    env: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&Path) -> bool,
    window: crate::Window,
) -> Result<paddock_testkit::live_browser::TestBrowser<Browser>, String> {
    use paddock_testkit::live_browser::{BrowserWindow, find_test_browser};
    let shows = match window {
        crate::Window::Headless => BrowserWindow::Headless,
        crate::Window::OffScreen | crate::Window::OnScreen(_) => BrowserWindow::Shown,
    };
    let hint = format!(
        "set {TEST_BROWSER_ENV} to the pinned Chrome for Testing (installed browsers are never used by tests)"
    );
    find_test_browser(host, env, shows, REQUIRE_TEST_BROWSER_ENV, &hint, || {
        let path = PathBuf::from(env(TEST_BROWSER_ENV).filter(|p| !p.is_empty())?);
        let installed = candidates(os_of(host.os), env)
            .iter()
            .any(|c| c.path == path);
        if installed {
            log::warn!(
                "{TEST_BROWSER_ENV} names an installed browser; tests use only a Chrome for Testing build"
            );
            return None;
        }
        exists(&path).then(|| Browser::chromium(path))
    })
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
    fn candidates_per_os() {
        let w = candidates(OsKind::Windows, &env);
        assert_eq!(w[0].flavor, Flavor::Edge);
        assert!(w[0].path.to_string_lossy().contains("msedge.exe"));
        assert!(w.iter().any(|b| b.flavor == Flavor::Chrome));
        let m = candidates(OsKind::Macos, &env);
        assert_eq!(m[0].flavor, Flavor::Chrome);
        assert!(
            m.iter()
                .any(|b| b.path.starts_with("/Users/u/Applications"))
        );
        assert!(
            candidates(OsKind::Linux, &env)
                .iter()
                .any(|b| b.path.ends_with("chromium-browser"))
        );
        assert_eq!(os_of("windows"), OsKind::Windows);
        assert_eq!(os_of("macos"), OsKind::Macos);
        assert_eq!(os_of("freebsd"), OsKind::Linux);
        assert_eq!(this_os(), os_of(std::env::consts::OS));
    }

    #[test]
    fn the_product_prefers_the_override_then_the_download_then_an_installed_browser() {
        let only = |p: &'static str| move |q: &Path| q == Path::new(p);
        let over = |k: &str| (k == "APP_CHROMIUM_PATH").then(|| "/x/chrome".to_string());
        let got = find_with(
            OsKind::Linux,
            &over,
            &only("/x/chrome"),
            Some("APP_CHROMIUM_PATH"),
            None,
        )
        .unwrap();
        assert_eq!(got.path, PathBuf::from("/x/chrome"));
        let dl = Browser::chromium("/data/runtimes/chromium/1/chrome");
        let got = find_with(
            OsKind::Linux,
            &env,
            &|p| p == dl.path || p == Path::new("/usr/bin/chromium"),
            Some("APP_CHROMIUM_PATH"),
            Some(dl.clone()),
        );
        assert_eq!(got, Some(dl.clone()));
        let got = find_with(
            OsKind::Linux,
            &env,
            &only("/usr/bin/chromium"),
            None,
            Some(dl),
        )
        .unwrap();
        assert_eq!(got.flavor, Flavor::Chromium);
        assert!(find_with(OsKind::Linux, &env, &|_| false, None, None).is_none());
        let empty = |k: &str| (k == "V").then(String::new);
        assert!(
            find_with(OsKind::Windows, &empty, &|_| true, Some("V"), None).is_none(),
            "no roots on Windows"
        );
        let _ = find(None, None);
    }

    #[test]
    fn tests_never_get_an_installed_browser() {
        use paddock_testkit::live_browser::{LIVE_BROWSER_VAR, TestBrowser};
        let named = |v: &'static str| {
            move |k: &str| match k {
                TEST_BROWSER_ENV => Some(v.to_string()),
                LIVE_BROWSER_VAR => Some("1".into()),
                "HOME" => Some("/h".into()),
                _ => None,
            }
        };
        let found = |os, env: &dyn Fn(&str) -> Option<String>, exists: &dyn Fn(&Path) -> bool| {
            match find_for_tests_with(
                paddock_testkit::live_browser::Host::plain(os),
                env,
                exists,
                crate::Window::Headless,
            ) {
                Ok(TestBrowser::Found(b)) => Some(b),
                _ => None,
            }
        };
        assert!(
            found("linux", &named("/usr/bin/google-chrome"), &|_| true).is_none(),
            "the user's Chrome is refused"
        );
        assert!(
            found(
                "linux",
                &|k| (k == LIVE_BROWSER_VAR).then(|| "1".into()),
                &|_| true
            )
            .is_none(),
            "nothing named"
        );
        assert!(found("linux", &named(""), &|_| true).is_none());
        let cft = "/cache/cft/chrome";
        assert_eq!(
            found("linux", &named(cft), &|p| p == Path::new(cft)),
            Some(Browser::chromium(cft))
        );
        assert!(found("linux", &named(cft), &|_| false).is_none());
        assert!(named(cft)("OTHER").is_none());
    }

    /// Regression (rule 7; Debrief CI 7141, 7146): a test browser, headless or not, never starts in a desktop session
    /// (the Mac and Windows runners run in a user's session).
    #[test]
    fn tests_get_no_browser_in_a_desktop_session() {
        use paddock_testkit::live_browser::{LIVE_BROWSER_VAR, TestBrowser};
        let cft = "/cache/cft/chrome";
        let env = |k: &str| match k {
            TEST_BROWSER_ENV => Some(cft.to_string()),
            LIVE_BROWSER_VAR | REQUIRE_TEST_BROWSER_ENV => Some("1".into()),
            "DISPLAY" => Some(":99".into()),
            _ => None,
        };
        for os in ["macos", "windows"] {
            for w in [
                crate::Window::Headless,
                crate::Window::OffScreen,
                crate::Window::OnScreen(None),
            ] {
                assert!(
                    matches!(
                        find_for_tests_with(
                            paddock_testkit::live_browser::Host::plain(os),
                            &env,
                            &|_| true,
                            w
                        ),
                        Ok(TestBrowser::Skip(_))
                    ),
                    "{os} {w:?}"
                );
            }
        }
        assert!(matches!(
            find_for_tests_with(
                paddock_testkit::live_browser::Host::plain("linux"),
                &env,
                &|_| true,
                crate::Window::OffScreen
            ),
            Ok(TestBrowser::Found(_))
        ));
        // A window needs the virtual display.
        let no_display = |k: &str| if k == "DISPLAY" { None } else { env(k) };
        assert!(
            find_for_tests_with(
                paddock_testkit::live_browser::Host::plain("linux"),
                &no_display,
                &|_| true,
                crate::Window::OnScreen(None)
            )
            .is_err()
        );
        assert!(
            find_for_tests_with(
                paddock_testkit::live_browser::Host::plain("linux"),
                &no_display,
                &|_| true,
                crate::Window::Headless
            )
            .is_ok()
        );
        let _ = find_for_tests(crate::Window::Headless);
    }

    #[test]
    fn flavor_names() {
        assert_eq!(Flavor::Edge.name(), "Microsoft Edge");
        assert_eq!(Flavor::Chrome.name(), "Google Chrome");
        assert_eq!(Flavor::Chromium.name(), "Chromium");
    }
}
