//! Finding an installed Chromium-based browser. The candidate lists are pure functions of the platform and an
//! environment lookup; [`find`] only checks which candidate exists.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    Mac,
    Linux,
}

impl Os {
    pub const CURRENT: Os = if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::Mac
    } else {
        Os::Linux
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Edge,
    Chrome,
    Chromium,
}

/// An executable that may be a Chromium browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Browser {
    pub flavor: Flavor,
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

/// The first existing candidate. `CHROMIUM_PATH` (an explicit executable) wins.
pub fn find() -> Option<Browser> {
    find_with(Os::CURRENT, &|k| std::env::var(k).ok(), &|p| p.is_file())
}

/// Set by CI where the test browser must be there: a missing one fails the test instead of skipping it (rule T02).
#[cfg(any(test, feature = "test-support"))]
pub const REQUIRE_VAR: &str = "DEBRIEF_REQUIRE_TEST_BROWSER";

/// The OS name the shared gate takes (`std::env::consts::OS`).
#[cfg(any(test, feature = "test-support"))]
fn os_name(os: Os) -> &'static str {
    match os {
        Os::Windows => "windows",
        Os::Mac => "macos",
        Os::Linux => "linux",
    }
}

/// The browser tests may launch: only the explicit `CHROMIUM_TEST_PATH` (a pinned test build), never the browsers
/// installed for the user (their Chrome must not be started by a test run), and only where a real browser may start
/// (Linux in a throwaway CI session under a virtual display, `paddock_testkit::live_browser`; this driver has no headless
/// mode, so every test browser opens a window, rule 7). The gate's verdict and the reason a test skips.
#[cfg(any(test, feature = "test-support"))]
fn find_for_tests_with(
    os: Os,
    env: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&Path) -> bool,
) -> Result<paddock_testkit::live_browser::TestBrowser<Browser>, String> {
    use paddock_testkit::live_browser::{find_test_browser, BrowserWindow};
    let hint = "install the pin with conformance/ci/install-test-browser.mjs --rust-source app/src-tauri/src/browser/runtime.rs \
                and set CHROMIUM_TEST_PATH (installed browsers are never launched by tests)";
    find_test_browser(os_name(os), env, BrowserWindow::Shown, REQUIRE_VAR, hint, || {
        env("CHROMIUM_TEST_PATH")
            .map(PathBuf::from)
            .filter(|p| exists(p))
            .map(|path| Browser {
                flavor: Flavor::Chromium,
                path,
            })
    })
}

/// [`test_browser`] without a word: the pinned test browser where it may start, `None` otherwise (the app's test fallback,
/// asked by tests that do not need a browser).
#[cfg(any(test, feature = "test-support"))]
pub fn find_for_tests() -> Option<Browser> {
    use paddock_testkit::live_browser::{process_env, TestBrowser};
    match find_for_tests_with(Os::CURRENT, &process_env, &|p| p.is_file()) {
        Ok(TestBrowser::Found(b)) => Some(b),
        _ => None,
    }
}

/// The test browser for a live-browser test, or `None` with the reason printed (the test then returns). Panics on a job
/// set up wrongly and when [`REQUIRE_VAR`] is set but no browser was found.
#[cfg(any(test, feature = "test-support"))]
pub fn test_browser() -> Option<Browser> {
    use paddock_testkit::live_browser::{found_or_skip, process_env};
    found_or_skip(find_for_tests_with(Os::CURRENT, &process_env, &|p| p.is_file()))
}

pub fn find_with(os: Os, env: &dyn Fn(&str) -> Option<String>, exists: &dyn Fn(&Path) -> bool) -> Option<Browser> {
    if let Some(p) = env("CHROMIUM_PATH").map(PathBuf::from).filter(|p| exists(p)) {
        return Some(Browser {
            flavor: Flavor::Chromium,
            path: p,
        });
    }
    candidates(os, env).into_iter().find(|b| exists(&b.path))
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

    use paddock_testkit::live_browser::{TestBrowser, LIVE_BROWSER_VAR};

    /// `CHROMIUM_TEST_PATH` plus the given variables.
    fn pinned_with(extra: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |k: &str| {
            (k == "CHROMIUM_TEST_PATH")
                .then(|| "/opt/test/chrome".to_string())
                .or_else(|| extra.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string()))
        }
    }

    const XVFB: &[(&str, &str)] = &[(LIVE_BROWSER_VAR, "1"), ("DISPLAY", ":99")];

    fn found(os: Os, env: &dyn Fn(&str) -> Option<String>, exists: &dyn Fn(&Path) -> bool) -> Option<Browser> {
        match find_for_tests_with(os, env, exists) {
            Ok(TestBrowser::Found(b)) => Some(b),
            _ => None,
        }
    }

    #[test]
    fn tests_find_only_the_pinned_test_browser_never_the_installed_ones() {
        let installed = |p: &Path| p.starts_with("/Applications") || p.starts_with("/usr/bin");
        let xvfb_only = |k: &str| XVFB.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string()).or_else(|| env(k));
        assert!(
            found(Os::Linux, &xvfb_only, &installed).is_none(),
            "an installed Chrome is not a test browser"
        );
        let pinned = pinned_with(XVFB);
        assert!(
            found(Os::Linux, &pinned, &|_| false).is_none(),
            "a path that does not exist is skipped"
        );
        let b = found(Os::Linux, &pinned, &|p| p == Path::new("/opt/test/chrome")).unwrap();
        assert_eq!(b.path, PathBuf::from("/opt/test/chrome"));
    }

    /// Regression (CI 7141, 7146): the Mac runner sets `CHROMIUM_TEST_PATH` and runs in the user's session, so every test
    /// browser (this driver has no headless mode) opened a window on the user's desktop. A test browser is handed out only
    /// on Linux, where the job says it is a throwaway session with a virtual display (rule 7).
    #[test]
    fn a_test_browser_opens_a_window_only_on_a_virtual_display() {
        let exists = |p: &Path| p == Path::new("/opt/test/chrome");
        for os in [Os::Mac, Os::Windows] {
            let Ok(TestBrowser::Skip(why)) = find_for_tests_with(os, &pinned_with(XVFB), &exists) else {
                panic!("{os:?} got a browser")
            };
            assert!(why.contains("Linux") && why.contains(LIVE_BROWSER_VAR), "{why}");
        }
        // Linux with a display the job did not vouch for (a developer's desktop): not there either.
        assert!(found(Os::Linux, &pinned_with(&[("DISPLAY", ":0")]), &exists).is_none());
        assert_eq!(
            found(Os::Linux, &pinned_with(XVFB), &exists).unwrap().path,
            PathBuf::from("/opt/test/chrome")
        );
        // Every window needs the virtual display; a job set up wrongly fails.
        assert!(find_for_tests_with(Os::Linux, &pinned_with(&[(LIVE_BROWSER_VAR, "1")]), &exists).is_err());
        assert!(find_for_tests_with(Os::Linux, &pinned_with(&[(REQUIRE_VAR, "1")]), &exists).is_err());
        let _ = (find_for_tests(), os_name(Os::Mac), os_name(Os::Windows));
    }

    #[test]
    fn finds_first_existing_and_honours_override() {
        let only = PathBuf::from("/usr/bin/chromium");
        let b = find_with(Os::Linux, &env, &|p| p == only).unwrap();
        assert_eq!(b.flavor, Flavor::Chromium);
        let over = |k: &str| (k == "CHROMIUM_PATH").then(|| "/x/chrome".to_string());
        assert_eq!(
            find_with(Os::Linux, &over, &|p| p == Path::new("/x/chrome")).unwrap().path,
            PathBuf::from("/x/chrome")
        );
        assert!(find_with(Os::Linux, &env, &|_| false).is_none());
    }
}
