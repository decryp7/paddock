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

/// Extra flags the tests pass to the test browser: `--no-sandbox` and `--disable-dev-shm-usage` when
/// [`TEST_NO_SANDBOX_ENV`] is `1`, otherwise none.
pub fn test_browser_flags_with(env: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    if env(TEST_NO_SANDBOX_ENV).as_deref() == Some("1") {
        vec!["--no-sandbox".into(), "--disable-dev-shm-usage".into()]
    } else {
        Vec::new()
    }
}

/// [`test_browser_flags_with`] for the process environment.
pub fn test_browser_flags() -> Vec<String> {
    test_browser_flags_with(&|k| std::env::var(k).ok())
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
        assert_eq!(test_browser_flags_with(&with("1")), ["--no-sandbox", "--disable-dev-shm-usage"]);
        for v in ["", "0", "true", "yes"] {
            assert!(test_browser_flags_with(&with(v)).is_empty(), "{v}");
        }
        assert!(test_browser_flags_with(&|_| None).is_empty());
    }

    #[test]
    fn flavor_names() {
        assert_eq!(Flavor::Edge.name(), "Microsoft Edge");
        assert_eq!(Flavor::Chrome.name(), "Google Chrome");
        assert_eq!(Flavor::Chromium.name(), "Chromium");
    }
}
