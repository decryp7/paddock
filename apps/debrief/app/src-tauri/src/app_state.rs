//! App-wide state shared by every window (configuration folders, safe mode, which plugins are enabled, the
//! plugin processes, the store worker) and the factory that gives each new window its own store and host.

use crate::engine::{Engine, Host, StoreHost};
use crate::profile::{load_profile, ProfileDoc};
use crate::windows::{WindowFactory, WindowState};
use debrief_plugin_host::Builtins;
use std::path::PathBuf;
use std::sync::Arc;

/// Managed as Tauri state.
#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) profile_dir: Option<PathBuf>,
    /// The app's configuration folder (plugin and app settings).
    pub(crate) config_dir: Option<PathBuf>,
    pub(crate) safe_mode: bool,
    /// Plugins the user turned off (read once: changes need a restart).
    disabled: Arc<Vec<String>>,
    /// The built-in plugins, each in its own process (ADR 0017), shared by every window: a running process
    /// monitor is the same in every window. None in safe mode. Installed third-party plugins are among them.
    pub(crate) plugins: Arc<Builtins>,
    /// Installed third-party plugins (ADR 0027): the install folder, the trust settings and what was loaded.
    pub(crate) installed: Arc<crate::plugin_packages::Installed>,
    /// Where every window's store is made and opened (the store worker).
    pub(crate) stores: Arc<dyn StoreHost>,
    /// The folder of scratch stores (`<data>/stores`).
    pub(crate) store_root: PathBuf,
    /// Keeps a private scratch folder alive when there is no data folder (see [`store_root`]).
    _store_scratch: Option<Arc<tempfile::TempDir>>,
    /// The parser library (ADR 0026), shared by every window.
    pub(crate) parsers: Arc<crate::parsers::ParserLibrary>,
    /// Settings ▸ Performance as the processes get it, and what their memory watchers report (ADR 0031).
    pub(crate) performance: Arc<crate::performance::live::Live>,
}

/// The folder of scratch stores: `<data>/stores`, or, without a data folder, a fresh private folder (random name, owner
/// only, created atomically) that is removed when the returned guard is dropped. Never a fixed name in the shared temp folder.
pub(crate) fn store_root(data: Option<&std::path::Path>) -> anyhow::Result<(PathBuf, Option<Arc<tempfile::TempDir>>)> {
    match data {
        Some(d) => Ok((d.join("stores"), None)),
        None => {
            let dir = debrief_ipc::scratch::private_tempdir("debrief-stores-").map_err(|e| {
                anyhow::anyhow!("cannot make a private folder for the scratch stores: {e}. Check the free disk space and the rights on the temp folder, then restart Debrief")
            })?;
            Ok((dir.path().to_path_buf(), Some(Arc::new(dir))))
        }
    }
}

impl AppState {
    /// The app's state: plugin processes (with the third-party plugins installed below `plugins_dir`, loaded unless
    /// in safe mode) and the store worker (both start on first use).
    pub fn new(
        profile_dir: Option<PathBuf>,
        data_dir: Option<PathBuf>,
        lib_dir: Option<PathBuf>,
        plugins_dir: Option<PathBuf>,
        safe_mode: bool,
    ) -> anyhow::Result<AppState> {
        let mut plugins = if safe_mode {
            Builtins::none()
        } else {
            Builtins::load(&debrief_plugin_host::builtins::default_binary())
        };
        let installed = crate::plugin_packages::Installed::load(plugins_dir, safe_mode, &mut plugins);
        let config_dir = profile_dir.as_ref().and_then(|d| d.parent()).map(PathBuf::from);
        let disabled = crate::plugin_config::load_plugin_settings(config_dir.as_deref()).disabled;
        let performance = crate::performance::live::Live::start(config_dir.clone(), data_dir.as_deref());
        let stores = crate::engine::WorkerHost::new(
            disabled,
            safe_mode,
            lib_dir,
            installed.for_store_worker(),
            performance.worker_launch(),
            Some(performance.hub.listener()),
        )?;
        performance.attach_worker(stores.worker());
        let plugins = Arc::new(plugins);
        performance.attach_plugins(plugins.clone());
        let (root, scratch) = store_root(data_dir.as_deref())?;
        let mut state = AppState::with(profile_dir, safe_mode, plugins, Arc::new(stores), root);
        state._store_scratch = scratch;
        state.installed = Arc::new(installed);
        state.performance = performance;
        Ok(state)
    }

    /// The state with the given plugins and store host (tests).
    pub fn with(
        profile_dir: Option<PathBuf>,
        safe_mode: bool,
        plugins: Arc<Builtins>,
        stores: Arc<dyn StoreHost>,
        store_root: PathBuf,
    ) -> AppState {
        let config_dir = profile_dir.as_ref().and_then(|d| d.parent()).map(PathBuf::from);
        let disabled = crate::plugin_config::load_plugin_settings(config_dir.as_deref()).disabled;
        let parsers = crate::parsers::ParserLibrary::load(crate::parsers::parser_dir(config_dir.as_deref()).as_deref());
        // Tests' states: the saved settings, no first-run suggestion.
        let performance = crate::performance::live::Live::with(
            config_dir.clone(),
            crate::performance::load(config_dir.as_deref()),
            crate::performance::section::cores(),
        );
        AppState {
            profile_dir,
            config_dir,
            safe_mode,
            disabled: Arc::new(disabled),
            plugins,
            installed: Arc::new(crate::plugin_packages::Installed::none()),
            stores,
            store_root,
            _store_scratch: None,
            parsers: Arc::new(parsers),
            performance,
        }
    }

    /// A new window: its own plugin registry (crash health is per window; the plugin processes are shared) with
    /// the saved profile (the default in safe mode, where no plugin is installed), and its store slot (the store
    /// is made on first use).
    pub fn new_window(&self, label: &str) -> anyhow::Result<WindowState> {
        let registry = crate::plugin_config::install(self.plugins.plugins(), self.safe_mode, &self.disabled)?;
        let (profile, doc) = if self.safe_mode {
            ProfileDoc::builtin()
        } else {
            load_profile(self.profile_dir.as_ref(), &registry)
        };
        let budget = self.performance.documents_budget();
        let profiling = crate::profiling::ProfilingSlot::from_registry_within(&registry, Some(budget.clone()));
        let dumps = crate::dumps::DumpsSlot::from_registry_within(&registry, Some(budget));
        let host = Host::new(registry, profile.clone())?;
        let engine = Engine::new(self.stores.clone(), self.store_root.clone(), label, profile);
        engine.set_parsers(self.parsers.setup());
        let mut state = WindowState::new(label, engine, host, doc);
        state.profiling = profiling;
        state.dumps = dumps;
        state.names = crate::names::WindowNames::open(self.config_dir.as_ref().map(|d| d.join(crate::names::BOOK_FILE)));
        Ok(state)
    }

    /// The factory for [`crate::windows::Windows`].
    pub fn factory(&self) -> WindowFactory {
        let shared = self.clone();
        Box::new(move |label| shared.new_window(label))
    }
}

#[cfg(test)]
mod store_root_tests {
    use super::*;

    #[test]
    fn the_scratch_stores_live_under_the_data_folder() {
        let data = tempfile::tempdir().unwrap();
        let (root, guard) = store_root(Some(data.path())).unwrap();
        assert_eq!(root, data.path().join("stores"));
        assert!(guard.is_none());
    }

    /// Without a data folder the stores go to a fresh private folder, never a fixed name in the shared temp folder that
    /// another local user could have planted.
    #[test]
    fn without_a_data_folder_the_scratch_stores_get_a_private_folder_that_goes_with_the_state() {
        let (a, ga) = store_root(None).unwrap();
        let (b, _gb) = store_root(None).unwrap();
        assert_ne!(a, b, "no predictable name");
        assert!(a.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&a).unwrap().permissions().mode() & 0o777, 0o700);
        }
        drop(ga);
        assert!(!a.exists(), "removed when the state is dropped");
    }
}
