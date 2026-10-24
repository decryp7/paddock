//! The demo's knowledge packs (plan X15/X23): one sample kind ("notes") with a built-in pack and the user's `packs/` folder
//! in the app's data folder, hot-reloaded by `paddock-packs`; the Knowledge packs page shows them.

use paddock_env::AppDirs;
use paddock_jobs::time::SystemClock;
use paddock_packs::watch::{self, NotifySource};
use paddock_packs::{
    BuiltinPack, DirSource, Limits, Origin, PackError, PackKind, PackLoader, PackStore, Reloader,
    user_packs_dir, view,
};
use serde::Deserialize;
use std::sync::Arc;

/// The sample pack format: a list of notes.
pub struct Notes;

/// One pack file as written.
#[derive(Deserialize)]
pub struct NotesFile {
    /// The notes.
    #[serde(default)]
    pub note: Vec<String>,
}

impl PackKind for Notes {
    type Raw = NotesFile;
    type Pack = Vec<String>;
    const FORMAT: u32 = 1;

    fn convert(raw: NotesFile, _origin: Origin) -> Result<Vec<String>, PackError> {
        Ok(raw.note)
    }
}

/// The built-in pack.
pub const BUILTIN: &str = "format = 1\nnote = [\"Packs are small files you can add to.\"]\n";

/// Loads the packs once, starts the folder watch (it lives as long as the app) and returns the setup for `Builder::packs`.
/// A watch that cannot start is only logged: the packs load, a restart picks up changes.
pub fn setup<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    dirs: &AppDirs,
) -> paddock_packs::tauri::Setup {
    use tauri::Manager as _;
    let dir = user_packs_dir(dirs);
    let loader = PackLoader::<Notes>::new(
        vec![BuiltinPack {
            name: "builtin.toml",
            text: BUILTIN,
        }],
        Box::new(DirSource::new(&dir)),
        Limits::default(),
    );
    let store = Arc::new(PackStore::new(loader));
    let reloader = Arc::new(Reloader::new(store.clone(), Arc::new(SystemClock)));
    reloader.reload_now();
    match watch::spawn(reloader, &NotifySource, &dir) {
        Ok(handle) => {
            app.manage(handle);
        }
        Err(why) => log::warn!("the packs folder is not watched: {why}"),
    }
    paddock_packs::tauri::Setup::new().kind(view::kind("notes", "Note packs", store))
}
