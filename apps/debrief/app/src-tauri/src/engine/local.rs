//! Tests: the store in process (the DuckDB store on a temp folder; ADR 0014 "tests use the real adapter"),
//! with the importers of the given plugins (their processes, as in the store worker).

use super::{SharedStore, StoreHost};
use debrief_core::package::ProjectManifest;
use debrief_core::ports::StoreResult;
use debrief_core::{PluginRegistry, Profile};
use debrief_plugin_host::Builtins;
use debrief_store_duck::{DuckStore, StoreOptions};
use std::path::Path;
use std::sync::Arc;

pub(crate) struct LocalHost {
    plugins: Option<Arc<Builtins>>,
}

/// Built-in importers only.
pub(crate) fn host() -> Arc<dyn StoreHost> {
    Arc::new(LocalHost { plugins: None })
}

/// With the importers of `plugins` (none in safe mode).
pub(crate) fn with_plugins(plugins: Arc<Builtins>, safe_mode: bool) -> Arc<dyn StoreHost> {
    Arc::new(LocalHost {
        plugins: (!safe_mode).then_some(plugins),
    })
}

impl LocalHost {
    fn registry(&self) -> PluginRegistry {
        registry_of(self.plugins.clone())
    }
}

/// The registry a store gets: the importers of `plugins`, with no wall-clock budgets
/// ([`crate::test_support::patient_budgets`]).
pub(crate) fn registry_of(plugins: Option<Arc<Builtins>>) -> PluginRegistry {
    match plugins {
        Some(p) => {
            let registry = crate::plugin_config::install(p.plugins(), false, &[]).unwrap_or_else(|_| PluginRegistry::new());
            registry.set_budgets(crate::test_support::patient_budgets());
            registry
        }
        None => PluginRegistry::new(),
    }
}

/// The limits of a test's store: low memory mode (4 parse threads, 2 DuckDB threads, one index writer thread, a 128 MB
/// DuckDB limit). A test imports a few small files, and nextest runs one test process per core: with the defaults every
/// one of them started cores - 1 parse threads and its plugins at once, and the Mac runner's load reached 116 on 14 cores
/// (ER-20, rule 51; docs/performance.md).
pub(crate) fn test_store_options() -> StoreOptions {
    StoreOptions::low_memory()
}

impl StoreHost for LocalHost {
    fn create(&self, dir: &Path, profile: &Profile) -> StoreResult<SharedStore> {
        Ok(Arc::new(DuckStore::create_with(
            dir,
            profile.clone(),
            self.registry(),
            test_store_options(),
        )?))
    }

    fn open(&self, dir: &Path, profile: &Profile) -> StoreResult<SharedStore> {
        Ok(Arc::new(DuckStore::open_with(
            dir,
            profile.clone(),
            self.registry(),
            test_store_options(),
        )?))
    }

    fn open_project(&self, path: &Path, dest_root: &Path, profile: &Profile) -> StoreResult<(SharedStore, ProjectManifest)> {
        std::fs::create_dir_all(dest_root)?;
        let (store, manifest) = DuckStore::open_project(path, dest_root, profile.clone(), self.registry())?;
        Ok((Arc::new(store), manifest))
    }
}
