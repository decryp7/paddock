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

/// Set to `1` by a test job whose display is a virtual one (`xvfb-run` in the Linux CI container): only then may a test
/// open a browser window. This driver has no headless mode, so every test browser opens a window, and a window on the
/// user's desktop is what Paddock rule 7 forbids (the Mac runner runs in the user's session). Honoured on Linux only.
pub const LIVE_BROWSER_VAR: &str = "PADDOCK_LIVE_BROWSER";

/// Set by CI where the test browser must be there: a missing one fails the test instead of skipping it (rule T02).
pub const REQUIRE_VAR: &str = "DEBRIEF_REQUIRE_TEST_BROWSER";

/// Whether a test may open a browser window here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowedTests {
    /// Linux, a display, and the job said it is a virtual one.
    Allowed,
    /// Not here, and where they run instead (the test prints it, so a skip is never silent).
    NotHere(String),
}

/// [`WindowedTests`] for `os` and the environment `env`. `Err` is a job set up wrongly, which fails the test: the
/// variable without a display (no `xvfb-run`), or `require_var` set on Linux without the variable (the browser tests
/// would all skip). A set variable is a non-blank value other than `0`.
pub fn windowed_tests_with(os: Os, env: &dyn Fn(&str) -> Option<String>, require_var: &str) -> Result<WindowedTests, String> {
    let set = |k: &str| env(k).is_some_and(|v| !v.trim().is_empty() && v.trim() != "0");
    if os != Os::Linux {
        return Ok(WindowedTests::NotHere(format!(
            "this test opens a browser window, so it runs only on Linux under a virtual display (Xvfb, the Linux CI job with \
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
             {LIVE_BROWSER_VAR}=1, or every browser test skips"
        )),
        (false, _) => Ok(WindowedTests::NotHere(format!(
            "this test opens a browser window: run it under xvfb-run with {LIVE_BROWSER_VAR}=1 (Linux only), never on a \
             desktop (Paddock rule 7)"
        ))),
    }
}

/// The browser tests may launch: only the explicit `CHROMIUM_TEST_PATH` (a pinned test build), never the browsers
/// installed for the user (their Chrome must not be started by a test run), and only where its window lands on a
/// virtual display ([`windowed_tests_with`]). `None` otherwise; quiet (a live test uses [`test_browser`]).
pub fn find_for_tests() -> Option<Browser> {
    find_for_tests_with(Os::CURRENT, &|k| std::env::var(k).ok(), &|p| p.is_file())
}

/// [`find_for_tests`] with the platform, environment and file check injected.
pub fn find_for_tests_with(os: Os, env: &dyn Fn(&str) -> Option<String>, exists: &dyn Fn(&Path) -> bool) -> Option<Browser> {
    if windowed_tests_with(os, env, REQUIRE_VAR) != Ok(WindowedTests::Allowed) {
        return None;
    }
    env("CHROMIUM_TEST_PATH")
        .map(PathBuf::from)
        .filter(|p| exists(p))
        .map(|path| Browser {
            flavor: Flavor::Chromium,
            path,
        })
}

/// The test browser for a live-browser test, or `None` with the reason printed (the test then returns). Panics on a job
/// set up wrongly ([`windowed_tests_with`]) and when [`REQUIRE_VAR`] is set but no browser was found.
pub fn test_browser() -> Option<Browser> {
    let env = |k: &str| std::env::var(k).ok();
    match windowed_tests_with(Os::CURRENT, &env, REQUIRE_VAR) {
        Err(why) => panic!("{why}"),
        Ok(WindowedTests::NotHere(why)) => {
            eprintln!("not run here: {why}");
            None
        }
        Ok(WindowedTests::Allowed) => {
            let found = find_for_tests();
            if found.is_none() {
                let required = env(REQUIRE_VAR).is_some_and(|v| !v.is_empty() && v != "0");
                assert!(
                    !required,
                    "{REQUIRE_VAR} is set but no pinned test browser was found: install the pin with \
                     conformance/ci/install-test-browser.mjs --rust-source app/src-tauri/src/browser/runtime.rs and set CHROMIUM_TEST_PATH"
                );
                eprintln!("skipped: set CHROMIUM_TEST_PATH to a pinned test Chromium (installed browsers are never launched by tests)");
            }
            found
        }
    }
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

    /// `CHROMIUM_TEST_PATH` plus the given variables.
    fn pinned_with(extra: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |k: &str| {
            (k == "CHROMIUM_TEST_PATH")
                .then(|| "/opt/test/chrome".to_string())
                .or_else(|| extra.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string()))
        }
    }

    fn only(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
    }

    const XVFB: &[(&str, &str)] = &[(LIVE_BROWSER_VAR, "1"), ("DISPLAY", ":99")];

    #[test]
    fn tests_find_only_the_pinned_test_browser_never_the_installed_ones() {
        let installed = |p: &Path| p.starts_with("/Applications") || p.starts_with("/usr/bin");
        let xvfb_only = |k: &str| only(XVFB)(k).or_else(|| env(k));
        assert!(
            find_for_tests_with(Os::Linux, &xvfb_only, &installed).is_none(),
            "an installed Chrome is not a test browser"
        );
        let pinned = pinned_with(XVFB);
        assert!(
            find_for_tests_with(Os::Linux, &pinned, &|_| false).is_none(),
            "a path that does not exist is skipped"
        );
        let b = find_for_tests_with(Os::Linux, &pinned, &|p| p == Path::new("/opt/test/chrome")).unwrap();
        assert_eq!(b.path, PathBuf::from("/opt/test/chrome"));
    }

    /// Regression (CI 7141, 7146): the Mac runner sets `CHROMIUM_TEST_PATH` and runs in the user's session, so every test
    /// browser (this driver has no headless mode) opened a window on the user's desktop. A test browser is handed out only
    /// on Linux, where the job says its display is a virtual one (rule 7: no windows on the desktop).
    #[test]
    fn a_test_browser_opens_a_window_only_on_a_virtual_display() {
        let exists = |p: &Path| p == Path::new("/opt/test/chrome");
        // A desktop session: never, whatever the job sets.
        for os in [Os::Mac, Os::Windows] {
            assert!(find_for_tests_with(os, &pinned_with(XVFB), &exists).is_none(), "{os:?}");
            let Ok(WindowedTests::NotHere(why)) = windowed_tests_with(os, &pinned_with(XVFB), REQUIRE_VAR) else {
                panic!("{os:?} allowed a window")
            };
            assert!(why.contains("Linux") && why.contains(LIVE_BROWSER_VAR), "{why}");
        }
        // Linux with a display the job did not vouch for (a developer's desktop): not there either.
        assert!(find_for_tests_with(Os::Linux, &pinned_with(&[("DISPLAY", ":0")]), &exists).is_none());
        // Linux under Xvfb with the job's variable: the pinned browser.
        let b = find_for_tests_with(Os::Linux, &pinned_with(XVFB), &exists).unwrap();
        assert_eq!(b.path, PathBuf::from("/opt/test/chrome"));
        assert_eq!(
            windowed_tests_with(Os::Linux, &pinned_with(XVFB), REQUIRE_VAR),
            Ok(WindowedTests::Allowed)
        );
    }

    #[test]
    fn a_misconfigured_job_fails_instead_of_skipping_the_window_tests() {
        // The variable without a display: the job forgot xvfb-run.
        let e = windowed_tests_with(Os::Linux, &only(&[(LIVE_BROWSER_VAR, "1")]), REQUIRE_VAR).unwrap_err();
        assert!(e.contains("xvfb-run"), "{e}");
        // The browser is required on Linux, but the job did not say its display is virtual: the tests would skip silently.
        let e = windowed_tests_with(Os::Linux, &only(&[(REQUIRE_VAR, "1")]), REQUIRE_VAR).unwrap_err();
        assert!(e.contains(LIVE_BROWSER_VAR), "{e}");
        // Required on a Mac: not run there all the same (the Linux job runs them), and never a window.
        assert!(matches!(
            windowed_tests_with(Os::Mac, &only(&[(REQUIRE_VAR, "1")]), REQUIRE_VAR),
            Ok(WindowedTests::NotHere(_))
        ));
        // Wayland counts as a display; "0" and blanks are not "set".
        assert_eq!(
            windowed_tests_with(Os::Linux, &only(&[(LIVE_BROWSER_VAR, "1"), ("WAYLAND_DISPLAY", "w")]), REQUIRE_VAR),
            Ok(WindowedTests::Allowed)
        );
        assert!(matches!(
            windowed_tests_with(Os::Linux, &only(&[(LIVE_BROWSER_VAR, "0"), ("DISPLAY", ":99")]), REQUIRE_VAR),
            Ok(WindowedTests::NotHere(_))
        ));
        assert!(windowed_tests_with(Os::Linux, &only(&[(LIVE_BROWSER_VAR, "1"), ("DISPLAY", " ")]), REQUIRE_VAR).is_err());
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
