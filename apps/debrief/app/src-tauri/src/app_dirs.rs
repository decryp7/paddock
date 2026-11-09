//! Debrief's own folders. Everything in the shell resolves them here, never through `app.path().app_*_dir()`
//! directly, so a debug build's `DEBRIEF_DATA_DIR` ([`crate::dev_overrides`]) relocates all of them at once:
//!
//! | folder        | normally (Tauri)               | with `DEBRIEF_DATA_DIR=<root>` |
//! |---------------|--------------------------------|----------------------------|
//! | data          | `app_data_dir`                 | `<root>/data`              |
//! | config        | `app_config_dir`               | `<root>/config`            |
//! | logs          | `app_log_dir`                  | `<root>/logs`              |
//! | webview data  | `app_local_data_dir` (Tauri's) | `<root>/local`             |
//!
//! Debrief keeps no cache of its own; the webview's caches live in its data folder.

use std::path::{Path, PathBuf};

use tauri::{Manager, Runtime};

/// One of Debrief's folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppDir {
    Data,
    Config,
    Log,
}

impl AppDir {
    fn sub(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Config => "config",
            Self::Log => "logs",
        }
    }
}

/// The folder under an override root.
pub fn under(root: &Path, dir: AppDir) -> PathBuf {
    root.join(dir.sub())
}

/// The webview's user data folder under an override root.
pub fn webview_data_dir(root: &Path) -> PathBuf {
    root.join("local")
}

/// Resolves a folder: the override root's when one is set, else Tauri's.
pub fn resolve<R: Runtime, M: Manager<R>>(app: &M, dir: AppDir) -> tauri::Result<PathBuf> {
    resolve_with(app, dir, crate::dev_overrides::current().data_dir.as_deref())
}

fn resolve_with<R: Runtime, M: Manager<R>>(app: &M, dir: AppDir, root: Option<&Path>) -> tauri::Result<PathBuf> {
    if let Some(root) = root {
        return Ok(under(root, dir));
    }
    let path = app.path();
    match dir {
        AppDir::Data => path.app_data_dir(),
        AppDir::Config => path.app_config_dir(),
        AppDir::Log => path.app_log_dir(),
    }
}

/// The webview's user data folder (its caches live inside): `<root>/local` under an override root, else
/// Tauri's `app_local_data_dir`, which Tauri gives WebView2 and WebKitGTK when no folder is set.
pub fn webview_data<R: Runtime, M: Manager<R>>(app: &M) -> tauri::Result<PathBuf> {
    webview_data_with(app, crate::dev_overrides::current().data_dir.as_deref())
}

fn webview_data_with<R: Runtime, M: Manager<R>>(app: &M, root: Option<&Path>) -> tauri::Result<PathBuf> {
    match root {
        Some(root) => Ok(webview_data_dir(root)),
        None => app.path().app_local_data_dir(),
    }
}

/// Debrief's data folder (sessions, the running marker, models, agent runs).
pub fn data<R: Runtime, M: Manager<R>>(app: &M) -> tauri::Result<PathBuf> {
    resolve(app, AppDir::Data)
}

/// The message of a feature that cannot work without the data folder: what is unavailable, why and the next step. There is
/// no fallback folder: a fixed or guessable folder in the shared temp folder could be planted by another local user.
/// A release build names the real cause (the user's profile folder could not be found); only a debug build also names
/// `DEBRIEF_DATA_DIR`, which release builds never read ([`crate::dev_overrides`]).
pub fn data_missing_message(feature: &str, cause: &dyn std::fmt::Display) -> String {
    data_missing_message_for(feature, cause, cfg!(debug_assertions))
}

/// [`data_missing_message`] for a debug (`true`) or a release build, so both texts are tested.
pub fn data_missing_message_for(feature: &str, cause: &dyn std::fmt::Display, debug_build: bool) -> String {
    let mut m = format!(
        "{feature} is unavailable: your user profile folder could not be found ({cause}). Check that your account has a home folder (HOME, or the Windows profile) and restart Debrief."
    );
    if debug_build {
        m.push_str(" In a debug build, DEBRIEF_DATA_DIR=<folder you own> also works.");
    }
    m
}

/// `data` as a folder, or the [`data_missing_message`] for `feature`: the one place that decides what a missing data folder
/// means (fail closed).
pub fn require_data(data: tauri::Result<PathBuf>, feature: &str) -> Result<PathBuf, String> {
    data.map_err(|e| data_missing_message(feature, &e))
}

/// The most entries one start-up sweep looks at (a bound, ER-27: the work is logged, never unbounded).
const MAX_SWEPT: usize = 1000;

/// Removes the entries of `root` that are older than `max_age` (their last change; those whose name does not start with
/// `prefix` are left alone). At most [`MAX_SWEPT`] entries are looked at. The removal never follows a link. Logs what
/// was found, removed and kept (`what` names it); returns how many were removed.
pub fn sweep_old(root: &Path, prefix: Option<&str>, max_age: std::time::Duration, what: &str) -> usize {
    sweep_old_at(root, prefix, max_age, std::time::SystemTime::now(), what)
}

/// [`sweep_old`] with the clock passed in.
pub fn sweep_old_at(root: &Path, prefix: Option<&str>, max_age: std::time::Duration, now: std::time::SystemTime, what: &str) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    let (mut looked, mut removed, mut failed) = (0usize, 0usize, 0usize);
    for entry in entries.flatten().take(MAX_SWEPT) {
        if prefix.is_some_and(|p| !entry.file_name().to_string_lossy().starts_with(p)) {
            continue;
        }
        looked += 1;
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        let old = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age >= max_age);
        if !old {
            continue;
        }
        let done = if meta.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        match done {
            Ok(()) => removed += 1,
            Err(e) => {
                failed += 1;
                log::warn!(
                    "Could not remove {what} entry {}: {e}",
                    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                );
            }
        }
    }
    log::info!("Start-up sweep of {what}: {looked} looked at, {removed} removed, {failed} could not be removed");
    removed
}

/// Debrief's config folder (settings, profiles, window state).
pub fn config<R: Runtime, M: Manager<R>>(app: &M) -> tauri::Result<PathBuf> {
    resolve(app, AppDir::Config)
}

/// Debrief's log folder (CLEF logs, error snapshots).
pub fn log<R: Runtime, M: Manager<R>>(app: &M) -> tauri::Result<PathBuf> {
    resolve(app, AppDir::Log)
}

/// The window-state plugin's file: absolute under an override root (the plugin joins it onto Tauri's config
/// folder, and joining an absolute path replaces that), else the plugin's default name.
pub fn window_state_file(root: Option<&Path>) -> Option<PathBuf> {
    root.map(|r| under(r, AppDir::Config).join(".window-state.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No data folder: the feature is unavailable with the next step, and no folder in the shared temp folder is offered.
    #[test]
    fn a_missing_data_folder_is_reported_with_the_next_step_never_replaced() {
        let m = require_data(Err(tauri::Error::WindowNotFound), "Video support").unwrap_err();
        assert!(m.starts_with("Video support is unavailable"), "{m}");
        assert!(m.contains("DEBRIEF_DATA_DIR"), "{m}");
        assert_eq!(require_data(Ok(PathBuf::from("/data")), "x"), Ok(PathBuf::from("/data")));
    }

    /// A release build never reads `DEBRIEF_DATA_DIR`, so its message names the real cause and the remedy that works.
    #[test]
    fn the_release_message_names_the_profile_folder_and_never_the_debug_variable() {
        let release = data_missing_message_for("Opening bundles", &"x", false);
        assert!(release.contains("user profile folder could not be found"), "{release}");
        assert!(release.contains("restart Debrief"), "{release}");
        assert!(!release.contains("DEBRIEF_DATA_DIR"), "{release}");
        let debug = data_missing_message_for("Opening bundles", &"x", true);
        assert!(debug.contains("DEBRIEF_DATA_DIR"), "{debug}");
        assert_eq!(data_missing_message("a", &"x").contains("DEBRIEF_DATA_DIR"), cfg!(debug_assertions));
    }

    #[test]
    fn the_start_up_sweep_removes_old_entries_with_the_prefix_and_keeps_the_rest() {
        let root = tempfile::tempdir().unwrap();
        let now = std::time::SystemTime::now();
        let old = now - std::time::Duration::from_secs(10 * 24 * 3600);
        let make = |name: &str, when: std::time::SystemTime| {
            let p = root.path().join(name);
            std::fs::create_dir(&p).unwrap();
            std::fs::write(p.join("f"), b"x").unwrap();
            std::fs::File::open(&p).unwrap().set_modified(when).unwrap();
        };
        make("x-old", old);
        make("x-new", now);
        make("other-old", old);
        let day = std::time::Duration::from_secs(24 * 3600);
        assert_eq!(sweep_old_at(root.path(), Some("x-"), 7 * day, now, "test entries"), 1);
        assert!(!root.path().join("x-old").exists());
        assert!(root.path().join("x-new").exists(), "recent: kept");
        assert!(root.path().join("other-old").exists(), "another name: untouched");
        assert_eq!(sweep_old_at(&root.path().join("missing"), None, day, now, "none"), 0);
    }

    #[test]
    fn an_override_root_holds_every_folder() {
        let app = crate::test_support::test_app_with_commands(None);
        let root_scratch = tempfile::tempdir().unwrap();
        let root = root_scratch.path().join("debrief-app-dirs-test");
        for (dir, sub) in [(AppDir::Data, "data"), (AppDir::Config, "config"), (AppDir::Log, "logs")] {
            assert_eq!(resolve_with(&app, dir, Some(&root)).unwrap(), root.join(sub));
        }
        assert_eq!(webview_data_dir(&root), root.join("local"));
        assert_eq!(webview_data_with(&app, Some(&root)).unwrap(), root.join("local"));
        assert_eq!(window_state_file(Some(&root)), Some(root.join("config").join(".window-state.json")));
        assert!(window_state_file(Some(&root)).unwrap().is_absolute());
        assert_eq!(window_state_file(None), None);
    }

    #[test]
    fn without_an_override_tauri_resolves_them() {
        let app = crate::test_support::test_app_with_commands(None);
        let p = app.path();
        assert_eq!(resolve_with(&app, AppDir::Data, None).ok(), p.app_data_dir().ok());
        assert_eq!(resolve_with(&app, AppDir::Config, None).ok(), p.app_config_dir().ok());
        assert_eq!(resolve_with(&app, AppDir::Log, None).ok(), p.app_log_dir().ok());
        // Unit tests never call `dev_overrides::init`.
        assert_eq!(data(&app).ok(), p.app_data_dir().ok());
        assert_eq!(config(&app).ok(), p.app_config_dir().ok());
        assert_eq!(log(&app).ok(), p.app_log_dir().ok());
        assert_eq!(webview_data(&app).ok(), p.app_local_data_dir().ok());
    }
}
