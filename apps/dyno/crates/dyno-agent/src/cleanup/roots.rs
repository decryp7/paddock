//! Where each symbolic cleanup root lives on this OS. Pure: the folders come from an [`Env`] the agent fills from the
//! environment, so the mapping is testable on any host. A `*` segment matches every folder at that level (browser
//! profiles).

use dyno_core::cleanup::{Os, Root};
use std::path::PathBuf;

/// The folders the mapping is built from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Env {
    /// The OS.
    pub os: Option<Os>,
    /// The user's home.
    pub home: PathBuf,
    /// The user's temp folder.
    pub temp: PathBuf,
    /// %LOCALAPPDATA% (Windows).
    pub local_app_data: Option<PathBuf>,
    /// %WINDIR% (Windows).
    pub windir: Option<PathBuf>,
    /// CARGO_HOME, when set.
    pub cargo_home: Option<PathBuf>,
    /// RUSTUP_HOME, when set.
    pub rustup_home: Option<PathBuf>,
}

impl Env {
    /// The environment of this process.
    pub fn current() -> Env {
        let var = |k: &str| std::env::var_os(k).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty());
        let home = var("USERPROFILE").or_else(|| var("HOME")).unwrap_or_default();
        Env {
            os: Some(if cfg!(windows) { Os::Windows } else { Os::Macos }),
            temp: std::env::temp_dir(),
            local_app_data: var("LOCALAPPDATA"),
            windir: var("WINDIR").or_else(|| var("SystemRoot")),
            cargo_home: var("CARGO_HOME"),
            rustup_home: var("RUSTUP_HOME"),
            home,
        }
    }

    fn cargo(&self) -> PathBuf {
        self.cargo_home.clone().unwrap_or_else(|| self.home.join(".cargo"))
    }

    fn rustup(&self) -> PathBuf {
        self.rustup_home.clone().unwrap_or_else(|| self.home.join(".rustup"))
    }
}

/// The folders (or patterns with `*`) for a root; empty when it doesn't exist on this OS.
pub fn resolve(root: Root, env: &Env) -> Vec<PathBuf> {
    let h = &env.home;
    let windows = env.os == Some(Os::Windows);
    let lad = env.local_app_data.clone().unwrap_or_else(|| h.join("AppData").join("Local"));
    let lib = h.join("Library");
    let caches = lib.join("Caches");
    let both = |w: Vec<PathBuf>, m: Vec<PathBuf>| if windows { w } else { m };
    match root {
        Root::UserTemp => vec![env.temp.clone()],
        Root::SystemTemp => both(env.windir.iter().map(|w| w.join("Temp")).collect(), vec![]),
        Root::UpdateDownloads => both(
            env.windir.iter().map(|w| w.join("SoftwareDistribution").join("Download")).collect(),
            vec![],
        ),
        Root::CrashDumps => both(vec![lad.join("CrashDumps")], vec![lib.join("Logs").join("DiagnosticReports")]),
        Root::ErrorReports => both(
            vec![
                lad.join("Microsoft").join("Windows").join("WER").join("ReportArchive"),
                lad.join("Microsoft").join("Windows").join("WER").join("ReportQueue"),
            ],
            vec![],
        ),
        Root::ThumbnailCache => both(vec![lad.join("Microsoft").join("Windows").join("Explorer")], vec![]),
        Root::UserCaches => both(vec![], vec![caches.clone()]),
        Root::UserLogs => both(vec![], vec![lib.join("Logs")]),
        Root::ChromeCache => both(
            ["Cache", "Code Cache", "GPUCache"]
                .iter()
                .map(|c| lad.join("Google").join("Chrome").join("User Data").join("*").join(c))
                .collect(),
            vec![caches.join("Google").join("Chrome").join("*")],
        ),
        Root::EdgeCache => both(
            ["Cache", "Code Cache", "GPUCache"]
                .iter()
                .map(|c| lad.join("Microsoft").join("Edge").join("User Data").join("*").join(c))
                .collect(),
            vec![caches.join("Microsoft Edge").join("*")],
        ),
        Root::FirefoxCache => both(
            vec![lad.join("Mozilla").join("Firefox").join("Profiles").join("*").join("cache2")],
            vec![caches.join("Firefox").join("Profiles").join("*").join("cache2")],
        ),
        Root::SafariCache => both(vec![], vec![caches.join("com.apple.Safari")]),
        Root::Downloads => vec![h.join("Downloads")],
        Root::Projects => vec![h.clone()],
        Root::CargoCache => vec![
            env.cargo().join("registry").join("cache"),
            env.cargo().join("registry").join("src"),
            env.cargo().join("git").join("checkouts"),
        ],
        Root::NpmCache => both(vec![lad.join("npm-cache").join("_cacache")], vec![h.join(".npm").join("_cacache")]),
        Root::NugetCache => both(
            vec![lad.join("NuGet").join("v3-cache"), h.join(".nuget").join("packages")],
            vec![
                h.join(".local").join("share").join("NuGet").join("v3-cache"),
                h.join(".nuget").join("packages"),
            ],
        ),
        Root::PipCache => both(vec![lad.join("pip").join("Cache")], vec![caches.join("pip")]),
        Root::RustToolchains => vec![env.rustup().join("toolchains")],
        Root::DockerImages => vec![],
    }
}

/// Expands `*` segments against the file system (directories only, links skipped), returning existing folders.
pub fn expand(pattern: &std::path::Path) -> Vec<PathBuf> {
    let mut current = vec![PathBuf::new()];
    for comp in pattern.components() {
        let seg = comp.as_os_str();
        let mut next = Vec::new();
        for base in current {
            if seg == "*" {
                let Ok(rd) = std::fs::read_dir(&base) else { continue };
                for e in rd.flatten().take(256) {
                    if e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink()) {
                        next.push(e.path());
                    }
                }
            } else {
                next.push(base.join(seg));
            }
        }
        current = next;
    }
    current.into_iter().filter(|p| p.is_dir()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win() -> Env {
        Env {
            os: Some(Os::Windows),
            home: PathBuf::from("C:/Users/me"),
            temp: PathBuf::from("C:/Users/me/AppData/Local/Temp"),
            local_app_data: Some(PathBuf::from("C:/Users/me/AppData/Local")),
            windir: Some(PathBuf::from("C:/Windows")),
            cargo_home: None,
            rustup_home: Some(PathBuf::from("D:/rustup")),
        }
    }

    fn mac() -> Env {
        Env {
            os: Some(Os::Macos),
            home: PathBuf::from("/Users/me"),
            temp: PathBuf::from("/tmp/x"),
            ..Default::default()
        }
    }

    #[test]
    fn every_root_resolves_where_it_exists() {
        use dyno_core::cleanup::catalog;
        for spec in catalog() {
            for root in spec.roots {
                let w = resolve(*root, &win());
                let m = resolve(*root, &mac());
                if *root != Root::DockerImages {
                    assert!(!w.is_empty() || !m.is_empty(), "{root:?}");
                }
                if spec.os == Os::Windows {
                    assert!(m.is_empty() || *root == Root::UserTemp, "{root:?} has no macOS path");
                }
            }
        }
        assert_eq!(resolve(Root::SystemTemp, &win()), vec![PathBuf::from("C:/Windows/Temp")]);
        assert_eq!(resolve(Root::RustToolchains, &win()), vec![PathBuf::from("D:/rustup/toolchains")]);
        assert_eq!(
            resolve(Root::CargoCache, &mac())[0],
            PathBuf::from("/Users/me/.cargo/registry/cache")
        );
        assert!(resolve(Root::ChromeCache, &win())[0].to_string_lossy().contains("User Data"));
        assert_eq!(
            resolve(Root::SafariCache, &mac()),
            vec![PathBuf::from("/Users/me/Library/Caches/com.apple.Safari")]
        );
        let cur = Env::current();
        assert!(cur.os.is_some());
    }

    #[test]
    fn stars_expand_to_existing_folders() {
        let dir = tempfile::tempdir().unwrap();
        for p in ["Default/Cache", "Profile 1/Cache", "Profile 2"] {
            std::fs::create_dir_all(dir.path().join(p)).unwrap();
        }
        std::fs::write(dir.path().join("file"), b"x").unwrap();
        let mut got = expand(&dir.path().join("*").join("Cache"));
        got.sort();
        assert_eq!(got.len(), 2);
        assert!(got[0].ends_with("Default/Cache") || got[0].ends_with("Default\\Cache"));
        assert!(expand(&dir.path().join("missing").join("*")).is_empty());
    }
}
