//! [`Files`]: the app's user-file service, the pure core behind the Tauri commands. It joins the grants, the recent lists, the
//! picker and the launcher, so every rule (a path is used only when the user chose it, nothing that runs on open is opened, a
//! recent entry is granted again on use) is tested without a window.

use crate::grants::{Access, GrantSource, GrantedPath, PathGrants};
use crate::open::{self, Launcher, Os, ProcessLauncher};
use crate::path::{Result, canonical, parse};
use crate::pick::{PickRequest, Picker};
use crate::recent::{ListSpec, Recents};
use paddock_commands::CommandError;
use paddock_env::AppDirs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// One of the app's own folders, which the webview may ask to see by name (it never supplies the path).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AppFolder {
    /// The data folder.
    Data,
    /// The config folder (settings).
    Config,
    /// The cache folder.
    Cache,
    /// The log folder.
    Logs,
}

/// The user-file service of an app.
pub struct Files {
    grants: PathGrants,
    recents: Recents,
    picker: Option<Arc<dyn Picker>>,
    launcher: Arc<dyn Launcher>,
    os: Os,
    folders: Vec<(AppFolder, PathBuf)>,
}

impl Files {
    /// A service with these recent lists, no picker, the real launcher and no named folders.
    pub fn new(recents: Recents) -> Files {
        Files {
            grants: PathGrants::default(),
            recents,
            picker: None,
            launcher: Arc::new(ProcessLauncher::default()),
            os: Os::CURRENT,
            folders: Vec::new(),
        }
    }

    /// The family's usual setup: `recent.json` in the config folder with the two default lists, and the app's four folders.
    pub fn for_app(dirs: &AppDirs, lists: &[ListSpec]) -> Files {
        Files::new(Recents::open(Some(dirs.config()), lists))
            .folder(AppFolder::Data, dirs.data())
            .folder(AppFolder::Config, dirs.config())
            .folder(AppFolder::Cache, dirs.cache())
            .folder(AppFolder::Logs, dirs.logs())
    }

    /// Replaces how dialogs are shown (the native picker, or a fake in tests).
    #[must_use]
    pub fn picker(mut self, picker: Arc<dyn Picker>) -> Files {
        self.picker = Some(picker);
        self
    }

    /// Replaces how programs are started (a recording fake in tests).
    #[must_use]
    pub fn launcher(mut self, launcher: Arc<dyn Launcher>) -> Files {
        self.launcher = launcher;
        self
    }

    /// Builds command lines for another platform (tests).
    #[must_use]
    pub fn os(mut self, os: Os) -> Files {
        self.os = os;
        self
    }

    /// Names one of the app's folders.
    #[must_use]
    pub fn folder(mut self, which: AppFolder, path: &Path) -> Files {
        self.folders.retain(|(f, _)| *f != which);
        self.folders.push((which, path.to_path_buf()));
        self
    }

    /// Whether a native picker is available.
    pub fn can_pick(&self) -> bool {
        self.picker.is_some()
    }

    /// The grants (for the commands of the app that read or write the user's files).
    pub fn grants(&self) -> &PathGrants {
        &self.grants
    }

    /// The recent lists.
    pub fn recents(&self) -> &Recents {
        &self.recents
    }

    /// Shows the dialog for `window` (a grant key) and grants what the user chose to it; returns the granted paths (empty when
    /// the dialog was closed). Blocking: call it on a worker thread.
    pub fn pick(&self, window: &str, request: PickRequest) -> Result<Vec<GrantedPath>> {
        let request = request.validated()?;
        let picker = self.picker.as_ref().ok_or_else(|| {
            CommandError::unavailable("Choosing files is not available in this build")
        })?;
        let access = request.access();
        let mut granted = Vec::new();
        for path in picker.pick(window, &request) {
            match self
                .grants
                .grant(window, &path, access, GrantSource::Dialog)
            {
                Ok(g) => granted.push(g),
                Err(e) => log::warn!("a chosen path could not be granted: {}", e.message),
            }
        }
        Ok(granted)
    }

    /// Grants every path that resolves, for reading (dropped files, the command line); returns the granted ones.
    pub fn grant_read(
        &self,
        window: &str,
        paths: &[PathBuf],
        source: GrantSource,
    ) -> Vec<GrantedPath> {
        self.grants.grant_all(window, paths, source)
    }

    /// The file or folder the app itself produced or chose, granted to `window` for reading.
    pub fn grant_output(&self, window: &str, path: &Path) -> Result<GrantedPath> {
        self.grants
            .grant(window, path, Access::Read, GrantSource::Output)
    }

    /// Records an opened or saved path in list `list` (the proof is the granted path); whether the lists changed.
    pub fn opened(&self, list: &str, granted: &GrantedPath, name: Option<&str>) -> bool {
        self.recents.record(list, granted, name)
    }

    /// Grants a path of list `list` to `window` again, because the user chose it earlier; the path is checked as a new one.
    pub fn regrant(&self, window: &str, list: &str, raw: &str) -> Result<GrantedPath> {
        if !self.recents.has(list, raw) {
            return Err(CommandError::not_found(
                "This file is no longer in the recent list; choose it again with Open",
            ));
        }
        self.grants
            .grant(window, parse(raw)?, Access::Read, GrantSource::Recent)
    }

    /// Shows a path the user chose in the file manager. Never runs anything.
    pub fn reveal(&self, window: &str, raw: &str) -> Result<()> {
        let granted = self.grants.check(window, raw, Access::Read, &[])?;
        self.start(&open::reveal(self.os, granted.path()))
    }

    /// Opens a path the user chose with the program the OS associates with it; a folder opens in the file manager. A file
    /// that would run (a script, a program, an installer) is refused.
    pub fn open(&self, window: &str, raw: &str) -> Result<()> {
        let granted = self.grants.check(window, raw, Access::Read, &[])?;
        if !granted.path().is_dir() && open::runs_on_open(granted.path()) {
            return Err(CommandError::denied(format!(
                "{} could run a program, so it is not opened from here",
                crate::path::file_name(granted.path())
            ))
            .hint("Show it in the file manager instead"));
        }
        self.start(&open::open_default(self.os, granted.path()))
    }

    /// Shows one of the app's own folders in the file manager.
    pub fn open_app_folder(&self, which: AppFolder) -> Result<()> {
        let path = self
            .folders
            .iter()
            .find(|(f, _)| *f == which)
            .map(|(_, p)| p)
            .ok_or_else(|| CommandError::not_found("This folder is not known to the app"))?;
        let path = canonical(path)?;
        self.start(&open::open_default(self.os, &path))
    }

    /// Opens a web address in the user's browser (http, https and mailto only).
    pub fn open_url(&self, raw: &str) -> Result<()> {
        let url = open::check_url(raw)?;
        self.start(&open::open_url_with(self.os, url))
    }

    fn start(&self, invocation: &open::Invocation) -> Result<()> {
        self.launcher.start(invocation).map_err(|e| {
            CommandError::io(format!(
                "cannot start {}: {e}",
                invocation.program.to_string_lossy()
            ))
            .hint("Open it from the file manager yourself")
        })
    }
}

#[cfg(test)]
pub(crate) mod fakes {
    use super::*;
    use crate::open::Invocation;
    use std::sync::Mutex;

    /// Records what would have been started.
    #[derive(Default)]
    pub struct Recording {
        pub started: Mutex<Vec<Invocation>>,
        pub fail: bool,
    }

    impl Launcher for Recording {
        fn start(&self, invocation: &Invocation) -> std::io::Result<()> {
            if self.fail {
                return Err(std::io::Error::other("no such program"));
            }
            self.started.lock().unwrap().push(invocation.clone());
            Ok(())
        }
    }

    /// Answers a dialog with fixed paths and remembers what it was asked.
    pub struct Choosing(pub Vec<PathBuf>, pub Mutex<Vec<(String, PickRequest)>>);

    impl Picker for Choosing {
        fn pick(&self, window: &str, request: &PickRequest) -> Vec<PathBuf> {
            self.1
                .lock()
                .unwrap()
                .push((window.into(), request.clone()));
            self.0.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fakes::*;
    use super::*;
    use crate::recent::{FILES, PROJECTS};
    use paddock_commands::ErrorCode;
    use std::sync::Mutex;

    fn files(rec: &Arc<Recording>) -> Files {
        Files::new(Recents::open(None, &[FILES, PROJECTS]))
            .launcher(rec.clone())
            .os(Os::Mac)
    }

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn a_dialog_grants_what_was_chosen_and_nothing_else() {
        let t = tempfile::tempdir().unwrap();
        let chosen = t.path().join("in.log");
        std::fs::write(&chosen, "x").unwrap();
        let missing = t.path().join("gone.log");
        let rec = Arc::new(Recording::default());
        let picker = Arc::new(Choosing(vec![chosen.clone(), missing], Mutex::default()));
        let f = files(&rec).picker(picker.clone());
        assert!(f.can_pick());
        let got = f
            .pick("main", PickRequest::open_files("Open logs"))
            .unwrap();
        assert_eq!(got.len(), 1, "a path that cannot be granted is dropped");
        assert!(
            f.grants()
                .check("main", &s(&chosen), Access::Read, &[])
                .is_ok()
        );
        assert!(
            f.grants()
                .check("w-2", &s(&chosen), Access::Read, &[])
                .is_err()
        );
        assert_eq!(picker.1.lock().unwrap()[0].0, "main");
        // A save dialog grants a write.
        let out = t.path().join("out.zip");
        let save = Files::new(Recents::open(None, &[FILES]))
            .picker(Arc::new(Choosing(vec![out.clone()], Mutex::default())));
        save.pick("main", PickRequest::save("Save", "out.zip"))
            .unwrap();
        assert!(
            save.grants()
                .check("main", &s(&out), Access::Write, &[])
                .is_ok()
        );
        // A bad request never reaches the dialog.
        let bad = PickRequest::save("Save", "../out.zip");
        assert_eq!(f.pick("main", bad).unwrap_err().code, ErrorCode::Invalid);
        assert_eq!(picker.1.lock().unwrap().len(), 1);
    }

    #[test]
    fn without_a_picker_the_error_says_so() {
        let f = files(&Arc::default());
        assert!(!f.can_pick());
        let e = f.pick("main", PickRequest::open_file("x")).unwrap_err();
        assert_eq!(e.code, ErrorCode::Unavailable);
    }

    #[test]
    fn recent_entries_are_recorded_after_a_grant_and_granted_again_on_use() {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("a.proj");
        std::fs::write(&file, "x").unwrap();
        let f = files(&Arc::default());
        let granted = f
            .grants()
            .grant("main", &file, Access::Read, GrantSource::Dialog)
            .unwrap();
        assert!(f.opened("projects", &granted, Some("Alpha")));
        // A new run: no grants, but the list remembers.
        f.grants().forget("main");
        assert!(
            f.grants()
                .check("main", &s(&file), Access::Read, &[])
                .is_err()
        );
        let again = f.regrant("main", "projects", &granted.text()).unwrap();
        assert_eq!(again.path(), granted.path());
        assert!(
            f.grants()
                .check("main", &s(&file), Access::Read, &[])
                .is_ok()
        );
        // Not in the list: refused, not granted.
        let other = t.path().join("other.proj");
        std::fs::write(&other, "x").unwrap();
        let e = f.regrant("main", "projects", &s(&other)).unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound);
        assert!(
            f.grants()
                .check("main", &s(&other), Access::Read, &[])
                .is_err()
        );
        assert!(
            f.regrant("main", "files", &granted.text()).is_err(),
            "another list"
        );
        // An entry whose file is gone cannot be granted.
        std::fs::remove_file(&file).unwrap();
        assert_eq!(
            f.regrant("main", "projects", &granted.text())
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn reveal_and_open_need_a_grant_and_never_run_programs() {
        let t = tempfile::tempdir().unwrap();
        let log = t.path().join("a.log");
        let script = t.path().join("run.sh");
        std::fs::write(&log, "x").unwrap();
        std::fs::write(&script, "x").unwrap();
        let rec = Arc::new(Recording::default());
        let f = files(&rec);
        assert_eq!(
            f.reveal("main", &s(&log)).unwrap_err().code,
            ErrorCode::NotGranted
        );
        assert_eq!(
            f.open("main", &s(&log)).unwrap_err().code,
            ErrorCode::NotGranted
        );
        assert!(
            rec.started.lock().unwrap().is_empty(),
            "nothing started without a grant"
        );
        f.grant_output("main", t.path()).unwrap();
        f.reveal("main", &s(&log)).unwrap();
        f.open("main", &s(&log)).unwrap();
        f.open("main", &s(t.path())).unwrap();
        let e = f.open("main", &s(&script)).unwrap_err();
        assert_eq!(e.code, ErrorCode::Denied);
        assert!(e.message.contains("run.sh"));
        f.reveal("main", &s(&script)).unwrap();
        let started = rec.started.lock().unwrap();
        assert_eq!(started.len(), 4);
        assert_eq!(started[0].args[0], "-R");
        assert_eq!(started[1].program, "open");
    }

    #[test]
    fn a_launcher_that_fails_gives_an_error_with_a_next_step() {
        let t = tempfile::tempdir().unwrap();
        let rec = Arc::new(Recording {
            fail: true,
            ..Recording::default()
        });
        let f = files(&rec).folder(AppFolder::Logs, t.path());
        let e = f.open_app_folder(AppFolder::Logs).unwrap_err();
        assert_eq!(e.code, ErrorCode::Io);
        assert!(e.hint.is_some());
        assert!(f.open_url("https://example.org").is_err());
    }

    #[test]
    fn app_folders_open_by_name_only() {
        let t = tempfile::tempdir().unwrap();
        let rec = Arc::new(Recording::default());
        let f = files(&rec)
            .folder(AppFolder::Logs, t.path())
            .folder(AppFolder::Logs, t.path());
        f.open_app_folder(AppFolder::Logs).unwrap();
        assert_eq!(rec.started.lock().unwrap().len(), 1);
        assert_eq!(
            f.open_app_folder(AppFolder::Cache).unwrap_err().code,
            ErrorCode::NotFound
        );
        let gone = files(&rec).folder(AppFolder::Data, &t.path().join("gone"));
        assert_eq!(
            gone.open_app_folder(AppFolder::Data).unwrap_err().code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn web_addresses_are_checked_before_anything_starts() {
        let rec = Arc::new(Recording::default());
        let f = files(&rec).os(Os::Linux);
        f.open_url("https://example.org/docs").unwrap();
        assert_eq!(rec.started.lock().unwrap()[0].program, "xdg-open");
        for bad in ["file:///etc/passwd", "javascript:1", "--help", ""] {
            assert!(f.open_url(bad).is_err(), "{bad:?}");
        }
        assert_eq!(rec.started.lock().unwrap().len(), 1);
    }

    #[test]
    fn for_app_names_the_four_folders_and_keeps_recent_in_config() {
        use paddock_env::{AppId, MapVars, Os as EnvOs};
        let t = tempfile::tempdir().unwrap();
        let id = AppId::new("filesdemo", "Files Demo").unwrap();
        let vars = MapVars::new().with("FILESDEMO_HOME", t.path().as_os_str());
        let dirs = AppDirs::resolve(&id, EnvOs::Linux, &vars, None, true).unwrap();
        let rec = Arc::new(Recording::default());
        let f = Files::for_app(&dirs, &[FILES])
            .launcher(rec.clone())
            .os(Os::Mac);
        std::fs::create_dir_all(dirs.logs()).unwrap();
        f.open_app_folder(AppFolder::Logs).unwrap();
        let file = t.path().join("x.log");
        std::fs::write(&file, "x").unwrap();
        let g = f
            .grants()
            .grant("main", &file, Access::Read, GrantSource::Dialog)
            .unwrap();
        f.opened("files", &g, None);
        assert!(dirs.config().join(crate::recent::FILE).exists());
    }
}
