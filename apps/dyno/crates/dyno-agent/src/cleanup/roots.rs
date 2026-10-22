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
    /// XDG_CACHE_HOME, when set (Linux).
    pub xdg_cache: Option<PathBuf>,
    /// XDG_DATA_HOME, when set (Linux).
    pub xdg_data: Option<PathBuf>,
}

impl Env {
    /// The environment of this process.
    pub fn current() -> Env {
        let var = |k: &str| std::env::var_os(k).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty());
        let home = var("USERPROFILE").or_else(|| var("HOME")).unwrap_or_default();
        Env {
            os: Some(if cfg!(windows) {
                Os::Windows
            } else if cfg!(target_os = "macos") {
                Os::Macos
            } else {
                Os::Linux
            }),
            temp: std::env::temp_dir(),
            local_app_data: var("LOCALAPPDATA"),
            windir: var("WINDIR").or_else(|| var("SystemRoot")),
            cargo_home: var("CARGO_HOME"),
            rustup_home: var("RUSTUP_HOME"),
            xdg_cache: var("XDG_CACHE_HOME").filter(|p| p.is_absolute()),
            xdg_data: var("XDG_DATA_HOME").filter(|p| p.is_absolute()),
            home,
        }
    }

    /// The OS (macOS when unknown).
    pub fn os(&self) -> Os {
        self.os.unwrap_or(Os::Macos)
    }

    fn cargo(&self) -> PathBuf {
        self.cargo_home.clone().unwrap_or_else(|| self.home.join(".cargo"))
    }

    /// RUSTUP_HOME or `~/.rustup`.
    pub fn rustup(&self) -> PathBuf {
        self.rustup_home.clone().unwrap_or_else(|| self.home.join(".rustup"))
    }

    fn local_app_data(&self) -> PathBuf {
        self.local_app_data
            .clone()
            .unwrap_or_else(|| self.home.join("AppData").join("Local"))
    }

    /// The per-user cache folder (`{cache}`).
    pub fn cache_dir(&self) -> PathBuf {
        match self.os() {
            Os::Windows => self.local_app_data(),
            Os::Macos => self.home.join("Library").join("Caches"),
            Os::Linux => self.xdg_cache.clone().unwrap_or_else(|| self.home.join(".cache")),
        }
    }

    /// The per-user data folder (`{data}`).
    pub fn data_dir(&self) -> PathBuf {
        match self.os() {
            Os::Windows => self.local_app_data(),
            Os::Macos => self.home.join("Library").join("Application Support"),
            Os::Linux => self.xdg_data.clone().unwrap_or_else(|| self.home.join(".local").join("share")),
        }
    }
}

/// Expands a validated pack template (`{home}/.gradle/caches`) to a path (still possibly with `*` segments). `None`
/// when the variable does not exist on this OS or the template is not valid.
pub fn expand_template(template: &str, env: &Env) -> Option<PathBuf> {
    dyno_core::cleanup::pack::valid_template(template).ok()?;
    let (var, rest) = template.split_once('/')?;
    let base = match var {
        "{home}" => env.home.clone(),
        "{temp}" => env.temp.clone(),
        "{cache}" => env.cache_dir(),
        "{data}" => env.data_dir(),
        "{localAppData}" if env.os() == Os::Windows => env.local_app_data(),
        _ => return None,
    };
    Some(rest.split('/').fold(base, |p, seg| p.join(seg)))
}

/// The folders (or patterns with `*`) for a root; empty when it doesn't exist on this OS.
pub fn resolve(root: Root, env: &Env) -> Vec<PathBuf> {
    let h = &env.home;
    let os = env.os();
    let lad = env.local_app_data();
    let lib = h.join("Library");
    let caches = env.cache_dir();
    let per = |w: Vec<PathBuf>, m: Vec<PathBuf>, l: Vec<PathBuf>| match os {
        Os::Windows => w,
        Os::Macos => m,
        Os::Linux => l,
    };
    let browser = |win: PathBuf, mac: PathBuf, linux: PathBuf, leaves: &[&str]| -> Vec<PathBuf> {
        match os {
            Os::Windows => leaves.iter().map(|c| win.join(c)).collect(),
            Os::Macos => vec![mac],
            Os::Linux => leaves.iter().map(|c| linux.join(c)).collect(),
        }
    };
    let chromium = ["Cache", "Code Cache", "GPUCache"];
    match root {
        Root::UserTemp => vec![env.temp.clone()],
        Root::SystemTemp => per(env.windir.iter().map(|w| w.join("Temp")).collect(), vec![], vec![]),
        Root::UpdateDownloads => per(
            env.windir.iter().map(|w| w.join("SoftwareDistribution").join("Download")).collect(),
            vec![],
            vec![],
        ),
        Root::CrashDumps => per(
            vec![lad.join("CrashDumps")],
            vec![lib.join("Logs").join("DiagnosticReports")],
            vec![],
        ),
        Root::ErrorReports => per(
            vec![
                lad.join("Microsoft").join("Windows").join("WER").join("ReportArchive"),
                lad.join("Microsoft").join("Windows").join("WER").join("ReportQueue"),
            ],
            vec![],
            vec![],
        ),
        Root::ThumbnailCache => per(
            vec![lad.join("Microsoft").join("Windows").join("Explorer")],
            vec![],
            vec![caches.join("thumbnails")],
        ),
        Root::UserCaches => per(vec![], vec![caches.clone()], vec![caches.clone()]),
        Root::UserLogs => per(vec![], vec![lib.join("Logs")], vec![]),
        Root::ChromeCache => browser(
            lad.join("Google").join("Chrome").join("User Data").join("*"),
            caches.join("Google").join("Chrome").join("*"),
            caches.join("google-chrome").join("*"),
            &chromium,
        ),
        Root::EdgeCache => browser(
            lad.join("Microsoft").join("Edge").join("User Data").join("*"),
            caches.join("Microsoft Edge").join("*"),
            caches.join("microsoft-edge").join("*"),
            &chromium,
        ),
        Root::FirefoxCache => per(
            vec![lad.join("Mozilla").join("Firefox").join("Profiles").join("*").join("cache2")],
            vec![caches.join("Firefox").join("Profiles").join("*").join("cache2")],
            vec![caches.join("mozilla").join("firefox").join("*").join("cache2")],
        ),
        Root::SafariCache => per(vec![], vec![caches.join("com.apple.Safari")], vec![]),
        Root::Downloads => vec![h.join("Downloads")],
        Root::Projects => vec![h.clone()],
        Root::CargoCache => vec![
            env.cargo().join("registry").join("cache"),
            env.cargo().join("registry").join("src"),
            env.cargo().join("git").join("checkouts"),
        ],
        Root::NpmCache => per(
            vec![lad.join("npm-cache").join("_cacache")],
            vec![h.join(".npm").join("_cacache")],
            vec![h.join(".npm").join("_cacache")],
        ),
        Root::NugetCache => per(
            vec![lad.join("NuGet").join("v3-cache"), h.join(".nuget").join("packages")],
            vec![
                h.join(".local").join("share").join("NuGet").join("v3-cache"),
                h.join(".nuget").join("packages"),
            ],
            vec![env.data_dir().join("NuGet").join("v3-cache"), h.join(".nuget").join("packages")],
        ),
        Root::PipCache => per(
            vec![lad.join("pip").join("Cache")],
            vec![caches.join("pip")],
            vec![caches.join("pip")],
        ),
        Root::RustToolchains => vec![env.rustup().join("toolchains")],
        Root::DockerImages => vec![],
    }
}

/// Every pattern a category looks in on this OS (named roots and expanded templates).
pub fn patterns(spec: &dyno_core::cleanup::CategorySpec, env: &Env) -> Vec<PathBuf> {
    use dyno_core::cleanup::RootRef;
    spec.roots
        .iter()
        .flat_map(|r| match r {
            RootRef::Named(n) => resolve(*n, env),
            RootRef::Template(t) => expand_template(t, env).into_iter().collect(),
        })
        .collect()
}

/// The existing folders a category scans: its patterns with `*` expanded.
pub fn folders(spec: &dyno_core::cleanup::CategorySpec, env: &Env) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for p in patterns(spec, env) {
        for f in expand(&p) {
            if !out.contains(&f) {
                out.push(f);
            }
        }
    }
    out
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
            rustup_home: Some(PathBuf::from("D:/rustup")),
            ..Default::default()
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

    fn linux() -> Env {
        Env {
            os: Some(Os::Linux),
            home: PathBuf::from("/home/me"),
            temp: PathBuf::from("/tmp"),
            ..Default::default()
        }
    }

    #[test]
    fn every_builtin_category_resolves_somewhere_on_its_systems() {
        for spec in dyno_core::cleanup::catalog() {
            for (os, env) in [(Os::Windows, win()), (Os::Macos, mac()), (Os::Linux, linux())] {
                let found = patterns(&spec, &env);
                if spec.applies_to(os) && spec.id != "dockerImages" {
                    assert!(!found.is_empty(), "{} has no folder on {os:?}", spec.id);
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
        assert!(Env::current().os.is_some());
    }

    #[test]
    fn linux_uses_the_xdg_folders() {
        let l = linux();
        assert_eq!(resolve(Root::UserCaches, &l), vec![PathBuf::from("/home/me/.cache")]);
        assert_eq!(resolve(Root::ThumbnailCache, &l), vec![PathBuf::from("/home/me/.cache/thumbnails")]);
        assert_eq!(resolve(Root::PipCache, &l), vec![PathBuf::from("/home/me/.cache/pip")]);
        assert_eq!(
            resolve(Root::FirefoxCache, &l),
            vec![PathBuf::from("/home/me/.cache/mozilla/firefox/*/cache2")]
        );
        assert_eq!(resolve(Root::ChromeCache, &l).len(), 3);
        assert!(resolve(Root::SafariCache, &l).is_empty() && resolve(Root::UserLogs, &l).is_empty());
        let custom = Env {
            xdg_cache: Some(PathBuf::from("/c")),
            xdg_data: Some(PathBuf::from("/d")),
            ..linux()
        };
        assert_eq!(resolve(Root::UserCaches, &custom), vec![PathBuf::from("/c")]);
        assert_eq!(resolve(Root::NugetCache, &custom)[0], PathBuf::from("/d/NuGet/v3-cache"));
        assert_eq!(expand_template("{data}/app/x", &custom), Some(PathBuf::from("/d/app/x")));
        assert_eq!(expand_template("{cache}/app", &custom), Some(PathBuf::from("/c/app")));
    }

    #[test]
    fn templates_expand_per_os_and_refuse_nonsense() {
        assert_eq!(
            expand_template("{home}/.gradle/caches", &linux()),
            Some(PathBuf::from("/home/me/.gradle/caches"))
        );
        assert_eq!(expand_template("{temp}/x", &mac()), Some(PathBuf::from("/tmp/x/x")));
        assert_eq!(
            expand_template("{localAppData}/App", &win()),
            Some(PathBuf::from("C:/Users/me/AppData/Local/App"))
        );
        assert_eq!(expand_template("{localAppData}/App", &linux()), None, "no such folder off Windows");
        assert_eq!(expand_template("{home}/../etc", &linux()), None);
        assert_eq!(expand_template("/etc/passwd", &linux()), None);
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

    #[test]
    fn folders_lists_existing_folders_once() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a").join("b")).unwrap();
        let env = Env {
            os: Some(Os::Linux),
            home: dir.path().to_path_buf(),
            temp: dir.path().join("nothing"),
            ..Default::default()
        };
        let pack = "format = 1\n[[category]]\nid = \"p\"\nname = \"P\"\ndescription = \"d\"\ngroup = \"developer\"\nrisk = \"review\"\npaths = [\"{home}/a/b\", \"{home}/a/*\", \"{home}/missing\"]\n";
        let spec = dyno_core::cleanup::pack::parse(pack, dyno_core::cleanup::pack::Trust::User)
            .unwrap()
            .remove(0);
        assert_eq!(folders(&spec, &env), vec![dir.path().join("a").join("b")]);
    }
}
