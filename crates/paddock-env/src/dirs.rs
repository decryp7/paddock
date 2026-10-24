use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::app::AppId;
use crate::error::{EnvError, Result, io};

/// The operating systems with a folder layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// Windows.
    Windows,
    /// macOS.
    MacOs,
    /// Linux and other Unix systems (XDG).
    Linux,
}

impl Os {
    /// The OS this build runs on.
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

/// Where environment variables come from: the process ([`ProcessVars`]) or a table ([`MapVars`], for tests), so every
/// layout can be checked on every OS without touching the real environment.
pub trait Vars {
    /// The variable's value, if set.
    fn get(&self, key: &str) -> Option<OsString>;
}

/// The process environment.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessVars;

impl Vars for ProcessVars {
    fn get(&self, key: &str) -> Option<OsString> {
        std::env::var_os(key)
    }
}

/// A fixed table of variables.
#[derive(Debug, Clone, Default)]
pub struct MapVars(BTreeMap<String, OsString>);

impl MapVars {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a variable.
    #[must_use]
    pub fn with(mut self, key: &str, value: impl Into<OsString>) -> Self {
        self.0.insert(key.to_owned(), value.into());
        self
    }
}

impl Vars for MapVars {
    fn get(&self, key: &str) -> Option<OsString> {
        self.0.get(key).cloned()
    }
}

/// Where the folders came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The OS's conventional places.
    Os,
    /// `<APP>_HOME` (debug builds and tests only).
    Override,
    /// A `portable.flag` file beside the executable: everything under `<id>-data` next to it.
    Portable,
}

/// The file whose presence beside the executable turns portable mode on.
pub const PORTABLE_FLAG: &str = "portable.flag";

/// The app's folders (decision D2/D3):
///
/// | | macOS | Windows | Linux |
/// |---|---|---|---|
/// | data | `~/Library/Application Support/<Name>` | `%LOCALAPPDATA%\<Name>` | `$XDG_DATA_HOME/<id>` |
/// | config | same as data | same as data | `$XDG_CONFIG_HOME/<id>` |
/// | cache | `~/Library/Caches/<Name>` | `%LOCALAPPDATA%\<Name>\cache` | `$XDG_CACHE_HOME/<id>` |
/// | logs | `~/Library/Logs/<Name>` | `%LOCALAPPDATA%\<Name>\logs` | `$XDG_STATE_HOME/<id>/logs` |
/// | runtime | `~/Library/Caches/<Name>/run` | `%LOCALAPPDATA%\<Name>\run` | `$XDG_RUNTIME_DIR/<id>`, else `<state>/<id>/run` |
///
/// Windows uses `%LOCALAPPDATA%` for everything (Local, never Roaming: models and caches must not roam); the old Roaming
/// folder is [`AppDirs::legacy_roaming_data`], for the one-time move. XDG variables count only when they hold an absolute
/// path. With `<ID>_HOME=<root>` (debug builds only) the folders are `<root>/{data,config,cache,logs,run}`; portable mode
/// uses the same shape under `<exe folder>/<id>-data`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDirs {
    app: AppId,
    source: Source,
    data: PathBuf,
    config: PathBuf,
    cache: PathBuf,
    logs: PathBuf,
    runtime: PathBuf,
    legacy_roaming: Option<PathBuf>,
}

impl AppDirs {
    /// The folders of the running app: the real environment and executable, `<ID>_HOME` honoured in debug builds only.
    pub fn for_app(app: &AppId) -> Result<Self> {
        let exe = std::env::current_exe().ok();
        Self::resolve(
            app,
            Os::current(),
            &ProcessVars,
            exe.as_deref().and_then(Path::parent),
            cfg!(debug_assertions),
        )
    }

    /// The folders for `os` from `vars`, checking `exe_dir` for the portable flag. `allow_override` says whether
    /// `<ID>_HOME` counts (the app's debug builds and tests); in a release build it is ignored.
    pub fn resolve(
        app: &AppId,
        os: Os,
        vars: &dyn Vars,
        exe_dir: Option<&Path>,
        allow_override: bool,
    ) -> Result<Self> {
        if allow_override && let Some(root) = vars.get(&app.home_var()).filter(|v| !v.is_empty()) {
            return Ok(Self::rooted(app, Source::Override, PathBuf::from(root)));
        }
        if let Some(exe) = exe_dir.filter(|d| d.join(PORTABLE_FLAG).is_file()) {
            return Ok(Self::rooted(
                app,
                Source::Portable,
                exe.join(format!("{}-data", app.id())),
            ));
        }
        match os {
            Os::MacOs => Self::mac(app, vars),
            Os::Windows => Self::windows(app, vars),
            Os::Linux => Self::linux(app, vars),
        }
    }

    fn rooted(app: &AppId, source: Source, root: PathBuf) -> Self {
        Self {
            app: app.clone(),
            source,
            data: root.join("data"),
            config: root.join("config"),
            cache: root.join("cache"),
            logs: root.join("logs"),
            runtime: root.join("run"),
            legacy_roaming: None,
        }
    }

    fn mac(app: &AppId, vars: &dyn Vars) -> Result<Self> {
        let lib = home(Os::MacOs, vars)?.join("Library");
        let data = lib.join("Application Support").join(app.name());
        let cache = lib.join("Caches").join(app.name());
        Ok(Self {
            app: app.clone(),
            source: Source::Os,
            config: data.clone(),
            data,
            runtime: cache.join("run"),
            cache,
            logs: lib.join("Logs").join(app.name()),
            legacy_roaming: None,
        })
    }

    fn windows(app: &AppId, vars: &dyn Vars) -> Result<Self> {
        let user = home(Os::Windows, vars)?;
        let local = absolute(Os::Windows, vars, "LOCALAPPDATA")
            .unwrap_or_else(|| user.join("AppData").join("Local"));
        let roaming = absolute(Os::Windows, vars, "APPDATA")
            .unwrap_or_else(|| user.join("AppData").join("Roaming"));
        let data = local.join(app.name());
        Ok(Self {
            app: app.clone(),
            source: Source::Os,
            config: data.clone(),
            cache: data.join("cache"),
            logs: data.join("logs"),
            runtime: data.join("run"),
            data,
            legacy_roaming: Some(roaming.join(app.name())),
        })
    }

    fn linux(app: &AppId, vars: &dyn Vars) -> Result<Self> {
        let home = home(Os::Linux, vars)?;
        let xdg = |var: &str, fallback: &[&str]| {
            absolute(Os::Linux, vars, var)
                .unwrap_or_else(|| fallback.iter().fold(home.clone(), |p, s| p.join(s)))
                .join(app.id())
        };
        let state = xdg("XDG_STATE_HOME", &[".local", "state"]);
        Ok(Self {
            app: app.clone(),
            source: Source::Os,
            data: xdg("XDG_DATA_HOME", &[".local", "share"]),
            config: xdg("XDG_CONFIG_HOME", &[".config"]),
            cache: xdg("XDG_CACHE_HOME", &[".cache"]),
            logs: state.join("logs"),
            runtime: absolute(Os::Linux, vars, "XDG_RUNTIME_DIR")
                .map_or_else(|| state.join("run"), |r| r.join(app.id())),
            legacy_roaming: None,
        })
    }

    /// The app these folders belong to.
    pub fn app(&self) -> &AppId {
        &self.app
    }

    /// Where the folders came from.
    pub fn source(&self) -> Source {
        self.source
    }

    /// Durable app data (stores, models, discovery files).
    pub fn data(&self) -> &Path {
        &self.data
    }

    /// Settings and other user configuration.
    pub fn config(&self) -> &Path {
        &self.config
    }

    /// Anything that can be deleted and rebuilt.
    pub fn cache(&self) -> &Path {
        &self.cache
    }

    /// Log files.
    pub fn logs(&self) -> &Path {
        &self.logs
    }

    /// Sockets, pid and marker files that must not survive a reboot where the OS cleans them.
    pub fn runtime(&self) -> &Path {
        &self.runtime
    }

    /// Windows only: the old `%APPDATA%\<Name>` (Roaming) folder some apps used for data before decision D3, for the
    /// one-time move and the discovery-file lookup. `None` on other systems, and with an override or portable mode.
    pub fn legacy_roaming_data(&self) -> Option<&Path> {
        self.legacy_roaming.as_deref()
    }

    /// Creates data, config, cache, logs and runtime (owner-only on Unix). Idempotent.
    pub fn ensure(&self) -> Result<()> {
        for dir in [
            &self.data,
            &self.config,
            &self.cache,
            &self.logs,
            &self.runtime,
        ] {
            create_private_dir(dir)?;
        }
        Ok(())
    }
}

fn create_private_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(io("create folder", dir))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Best effort: a folder the user shares on purpose keeps its mode when it cannot be changed.
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

/// Whether `path` is absolute in `os`'s syntax (checked by shape, so Windows paths can be tested on any host).
fn looks_absolute(os: Os, path: &str) -> bool {
    match os {
        Os::Windows => {
            let b = path.as_bytes();
            path.starts_with("\\\\")
                || (b.len() >= 3
                    && b[0].is_ascii_alphabetic()
                    && b[1] == b':'
                    && (b[2] == b'\\' || b[2] == b'/'))
        }
        _ => path.starts_with('/'),
    }
}

fn absolute(os: Os, vars: &dyn Vars, key: &str) -> Option<PathBuf> {
    let v = vars.get(key)?;
    looks_absolute(os, &v.to_string_lossy()).then(|| PathBuf::from(v))
}

fn home(os: Os, vars: &dyn Vars) -> Result<PathBuf> {
    let (key, what) = if os == Os::Windows {
        ("USERPROFILE", "profile folder (USERPROFILE)")
    } else {
        ("HOME", "home folder (HOME)")
    };
    absolute(os, vars, key).ok_or(EnvError::NoHome(what))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> AppId {
        AppId::new("pitwall", "Pitwall").unwrap()
    }

    fn p(parts: &[&str]) -> PathBuf {
        parts.iter().fold(PathBuf::new(), |a, s| {
            if a.as_os_str().is_empty() {
                PathBuf::from(s)
            } else {
                a.join(s)
            }
        })
    }

    fn resolve(os: Os, vars: &MapVars) -> Result<AppDirs> {
        AppDirs::resolve(&app(), os, vars, None, false)
    }

    #[test]
    fn macos_layout() {
        let d = resolve(Os::MacOs, &MapVars::new().with("HOME", "/Users/u")).unwrap();
        assert_eq!(
            d.data(),
            Path::new("/Users/u/Library/Application Support/Pitwall")
        );
        assert_eq!(d.config(), d.data());
        assert_eq!(d.cache(), Path::new("/Users/u/Library/Caches/Pitwall"));
        assert_eq!(d.logs(), Path::new("/Users/u/Library/Logs/Pitwall"));
        assert_eq!(
            d.runtime(),
            Path::new("/Users/u/Library/Caches/Pitwall/run")
        );
        assert_eq!((d.source(), d.legacy_roaming_data()), (Source::Os, None));
    }

    #[test]
    fn windows_is_local_for_everything_and_names_the_roaming_folder() {
        let vars = MapVars::new()
            .with("USERPROFILE", "C:\\Users\\u")
            .with("LOCALAPPDATA", "C:\\Users\\u\\AppData\\Local")
            .with("APPDATA", "C:\\Users\\u\\AppData\\Roaming");
        let d = resolve(Os::Windows, &vars).unwrap();
        let data = p(&["C:\\Users\\u\\AppData\\Local", "Pitwall"]);
        assert_eq!(d.data(), data);
        assert_eq!(d.config(), data);
        assert_eq!(d.cache(), data.join("cache"));
        assert_eq!(d.logs(), data.join("logs"));
        assert_eq!(d.runtime(), data.join("run"));
        assert_eq!(
            d.legacy_roaming_data().unwrap(),
            p(&["C:\\Users\\u\\AppData\\Roaming", "Pitwall"])
        );
    }

    #[test]
    fn windows_without_appdata_variables_falls_back_under_the_profile() {
        let d = resolve(
            Os::Windows,
            &MapVars::new()
                .with("USERPROFILE", "C:\\Users\\u")
                .with("LOCALAPPDATA", "relative"),
        )
        .unwrap();
        assert_eq!(
            d.data(),
            p(&["C:\\Users\\u", "AppData", "Local", "Pitwall"])
        );
        assert_eq!(
            d.legacy_roaming_data().unwrap(),
            p(&["C:\\Users\\u", "AppData", "Roaming", "Pitwall"])
        );
    }

    #[test]
    fn linux_defaults_and_xdg() {
        let d = resolve(Os::Linux, &MapVars::new().with("HOME", "/home/u")).unwrap();
        assert_eq!(d.data(), Path::new("/home/u/.local/share/pitwall"));
        assert_eq!(d.config(), Path::new("/home/u/.config/pitwall"));
        assert_eq!(d.cache(), Path::new("/home/u/.cache/pitwall"));
        assert_eq!(d.logs(), Path::new("/home/u/.local/state/pitwall/logs"));
        assert_eq!(d.runtime(), Path::new("/home/u/.local/state/pitwall/run"));

        let vars = MapVars::new()
            .with("HOME", "/home/u")
            .with("XDG_DATA_HOME", "/x/data")
            .with("XDG_CONFIG_HOME", "/x/config")
            .with("XDG_CACHE_HOME", "/x/cache")
            .with("XDG_STATE_HOME", "/x/state")
            .with("XDG_RUNTIME_DIR", "/run/user/1000");
        let d = resolve(Os::Linux, &vars).unwrap();
        assert_eq!(d.data(), Path::new("/x/data/pitwall"));
        assert_eq!(d.config(), Path::new("/x/config/pitwall"));
        assert_eq!(d.cache(), Path::new("/x/cache/pitwall"));
        assert_eq!(d.logs(), Path::new("/x/state/pitwall/logs"));
        assert_eq!(d.runtime(), Path::new("/run/user/1000/pitwall"));
    }

    #[test]
    fn relative_xdg_values_are_ignored() {
        let vars = MapVars::new()
            .with("HOME", "/home/u")
            .with("XDG_DATA_HOME", "data")
            .with("XDG_RUNTIME_DIR", "");
        let d = resolve(Os::Linux, &vars).unwrap();
        assert_eq!(d.data(), Path::new("/home/u/.local/share/pitwall"));
        assert_eq!(d.runtime(), Path::new("/home/u/.local/state/pitwall/run"));
    }

    #[test]
    fn a_missing_or_relative_home_is_an_error() {
        for os in [Os::MacOs, Os::Linux, Os::Windows] {
            assert!(
                matches!(resolve(os, &MapVars::new()), Err(EnvError::NoHome(_))),
                "{os:?}"
            );
        }
        assert!(matches!(
            resolve(Os::Linux, &MapVars::new().with("HOME", "rel")),
            Err(EnvError::NoHome(_))
        ));
        assert!(
            resolve(Os::Linux, &MapVars::new())
                .unwrap_err()
                .to_string()
                .contains("home folder")
        );
    }

    #[test]
    fn the_override_roots_every_folder_in_debug_only() {
        let vars = MapVars::new()
            .with("HOME", "/home/u")
            .with("PITWALL_HOME", "/tmp/pw");
        let d = AppDirs::resolve(&app(), Os::Linux, &vars, None, true).unwrap();
        assert_eq!(d.source(), Source::Override);
        assert_eq!(d.data(), Path::new("/tmp/pw/data"));
        assert_eq!(d.config(), Path::new("/tmp/pw/config"));
        assert_eq!(d.cache(), Path::new("/tmp/pw/cache"));
        assert_eq!(d.logs(), Path::new("/tmp/pw/logs"));
        assert_eq!(d.runtime(), Path::new("/tmp/pw/run"));
        // A release build ignores it.
        let d = AppDirs::resolve(&app(), Os::Linux, &vars, None, false).unwrap();
        assert_eq!(d.source(), Source::Os);
        assert_eq!(d.data(), Path::new("/home/u/.local/share/pitwall"));
        // An empty value is not an override, and the override needs no home folder.
        let d = AppDirs::resolve(
            &app(),
            Os::Linux,
            &MapVars::new().with("PITWALL_HOME", ""),
            None,
            true,
        );
        assert!(d.is_err());
        let d = AppDirs::resolve(
            &app(),
            Os::Windows,
            &MapVars::new().with("PITWALL_HOME", "/t"),
            None,
            true,
        )
        .unwrap();
        assert!(d.legacy_roaming_data().is_none());
    }

    #[test]
    fn a_release_build_ignores_the_override() {
        // `for_app` passes `cfg!(debug_assertions)`; the release rule is the `allow_override = false` branch above.
        let vars = MapVars::new()
            .with("HOME", "/h")
            .with("PITWALL_HOME", "/tmp/pw");
        assert_eq!(
            AppDirs::resolve(&app(), Os::MacOs, &vars, None, false)
                .unwrap()
                .source(),
            Source::Os
        );
    }

    #[test]
    fn portable_mode_follows_the_flag_beside_the_executable() {
        let t = tempfile::tempdir().unwrap();
        let vars = MapVars::new().with("HOME", "/home/u");
        let without = AppDirs::resolve(&app(), Os::Linux, &vars, Some(t.path()), false).unwrap();
        assert_eq!(without.source(), Source::Os);
        std::fs::write(t.path().join(PORTABLE_FLAG), "").unwrap();
        let d = AppDirs::resolve(&app(), Os::Linux, &vars, Some(t.path()), false).unwrap();
        assert_eq!(d.source(), Source::Portable);
        assert_eq!(d.data(), t.path().join("pitwall-data").join("data"));
        assert_eq!(d.logs(), t.path().join("pitwall-data").join("logs"));
        // Portable mode needs no home folder; the override wins over it.
        assert!(
            AppDirs::resolve(&app(), Os::Linux, &MapVars::new(), Some(t.path()), false).is_ok()
        );
        let both = vars.with("PITWALL_HOME", "/o");
        assert_eq!(
            AppDirs::resolve(&app(), Os::Linux, &both, Some(t.path()), true)
                .unwrap()
                .source(),
            Source::Override
        );
    }

    #[test]
    fn ensure_creates_every_folder_once_more_without_error() {
        let t = tempfile::tempdir().unwrap();
        let vars = MapVars::new().with("PITWALL_HOME", t.path().join("home"));
        let d = AppDirs::resolve(&app(), Os::current(), &vars, None, true).unwrap();
        d.ensure().unwrap();
        d.ensure().unwrap();
        for dir in [d.data(), d.config(), d.cache(), d.logs(), d.runtime()] {
            assert!(dir.is_dir(), "{}", dir.display());
        }
        assert_eq!(d.app().id(), "pitwall");
    }

    #[test]
    fn ensure_reports_a_file_in_the_way() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("home"), "x").unwrap();
        let vars = MapVars::new().with("PITWALL_HOME", t.path().join("home"));
        let d = AppDirs::resolve(&app(), Os::current(), &vars, None, true).unwrap();
        let e = d.ensure().unwrap_err();
        assert!(matches!(
            e,
            EnvError::Io {
                op: "create folder",
                ..
            }
        ));
        assert!(std::error::Error::source(&e).is_some());
    }

    #[test]
    fn process_vars_read_the_environment_and_for_app_resolves() {
        assert!(
            ProcessVars
                .get("PADDOCK_ENV_SURELY_UNSET_VARIABLE")
                .is_none()
        );
        assert!(ProcessVars.get("PATH").is_some());
        let _ = Os::current();
        // The real call must not fail on a normal machine; it creates nothing.
        assert!(
            AppDirs::for_app(&AppId::new("paddock-env-test", "Paddock Env Test").unwrap()).is_ok()
        );
    }
}
