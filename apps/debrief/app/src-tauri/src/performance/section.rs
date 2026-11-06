//! The Performance section: its schema (the one validator of every value, from the Settings screen, an imported
//! settings file or a hand-edited `performance.json`), the relations between its fields, and the presets.

use super::live::Live;
use super::PerformanceSettings;
use crate::builtin_sections::{int, PERFORMANCE_SECTION};
use crate::settings::ExternalValues;
use debrief_core::settings::{Configurable, FieldError, FieldKind, SectionOwner, SettingField, SettingsSection, ValidValues};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Smallest and largest per-plugin cap, MB (the bounds every plugin gets, `debrief_plugin_api::sandbox`).
pub const MIN_PLUGIN_MB: u32 = 64;
pub const MAX_PLUGIN_MB: u32 = 4096;
/// Heap of each search index writer thread by default, MB (`debrief_store_duck::WRITER_HEAP_PER_THREAD`); and its range
/// (Tantivy needs at least 15 MB a thread).
pub const DEFAULT_INDEX_HEAP_PER_THREAD_MB: u32 = 64;
pub const MIN_INDEX_HEAP_PER_THREAD_MB: u32 = 16;
pub const MAX_INDEX_HEAP_PER_THREAD_MB: u32 = 1024;
/// Most search index writer threads.
pub const MAX_INDEX_WRITER_THREADS: u32 = 64;
/// The store worker's own use before DuckDB's limit (its code, the catalog, buffers), MB: the query budget minus
/// this is DuckDB's limit when none is chosen (300 → 192).
pub const WORKER_BASE_MB: u32 = 108;

/// The Performance section's schema. Every field applies when the process it is about starts next (see ADR 0031 §7).
pub fn schema() -> SettingsSection {
    let d = PerformanceSettings::default();
    let mb = |min: i64, max: i64| int(min, max, Some("MB"));
    SettingsSection::new(PERFORMANCE_SECTION, "Performance", SectionOwner::App)
        .describe(
            "How much memory and processor Debrief's background processes may use: the database worker that imports and \
             queries, the plugins and the AI. Suggest settings proposes values for this computer.",
        )
        // ---- memory ----
        .field(
            SettingField::new("storeCapMb", "Database worker memory cap", mb(512, 65_536), d.store_cap_mb)
                .describe(
                    "The most memory the database worker may use. Above it Debrief stops the worker and says so; it starts \
                     again on the next action. You are warned at 80%.",
                )
                .needs_restart(),
        )
        .field(
            SettingField::new("importBudgetMb", "Memory for imports", mb(128, 32_768), d.import_budget_mb)
                .describe(
                    "How much of the worker's memory an import plans to use. More lets Debrief read more files at once; it \
                     must fit in the cap.",
                )
                .needs_restart(),
        )
        .field(
            SettingField::new("queryBudgetMb", "Memory for queries", mb(200, 32_768), d.query_budget_mb)
                .describe(
                    "How much of the worker's memory searches, charts and analyses plan to use. More keeps more in memory \
                     instead of on disk; it must fit in the cap.",
                )
                .needs_restart(),
        )
        .field(
            SettingField::new("pluginMemoryMb", "Plugin memory cap", mb(i64::from(MIN_PLUGIN_MB), i64::from(MAX_PLUGIN_MB)), d.plugin_memory_mb)
                .describe(
                    "The cap of each plugin that does not say how much it needs. A plugin that asks for more keeps its \
                     own; a cap set for one plugin in the Plugins panel wins over both.",
                )
                .needs_restart(),
        )
        .field(
            SettingField::new("aiWorkerMemoryMb", "AI worker memory cap", mb(0, 16_384), d.ai_worker_memory_mb)
                .describe(
                    "The cap of the AI's helper process. The model itself runs in a separate process and is not counted. \
                     0 means no cap.",
                )
                .needs_restart(),
        )
        .field(
            SettingField::new("returnMemoryAtOnce", "Give freed memory back at once", FieldKind::Bool, d.return_memory_at_once)
                .describe(
                    "The database worker hands memory it no longer needs straight back to the system. Uses less memory \
                     over time; imports are a little slower. Part of the Low memory preset.",
                )
                .needs_restart(),
        )
        // ---- processor ----
        .field(
            SettingField::new("parseThreads", "Import threads", int(0, 64, None), d.parse_threads)
                .describe("How many files or parts of a file an import reads at the same time. 0 picks a number from the cores and the import memory.")
                .needs_restart(),
        )
        .field(
            SettingField::new("queryThreads", "Query threads", int(1, 64, None), d.query_threads)
                .describe("How many cores one query may use. More is faster on big projects and needs more of the query memory.")
                .needs_restart(),
        )
        .field(
            SettingField::new(
                "indexWriterThreads",
                "Search index threads",
                int(0, i64::from(MAX_INDEX_WRITER_THREADS), None),
                d.index_writer_threads,
            )
            .describe(
                "How many cores build the word-search index after an import, at a lowered priority. 0 is Auto: every core \
                 but the ones left free, at most 4, as many as fit their memory in the import memory.",
            )
            .needs_restart(),
        )
        .field(
            SettingField::new(
                "indexReadThreads",
                "Search index read threads",
                int(0, i64::from(MAX_INDEX_WRITER_THREADS), None),
                d.index_read_threads,
            )
            .describe(
                "How many cores read the stored events and prepare them for the word-search index, at a lowered priority. \
                 0 is Auto: every core but the ones left free, at most 8. The events waiting for the index take a fixed \
                 amount of memory whatever this is.",
            )
            .needs_restart(),
        )
        .field(
            SettingField::new(
                "indexHeapPerThreadMb",
                "Search index memory per thread",
                mb(i64::from(MIN_INDEX_HEAP_PER_THREAD_MB), i64::from(MAX_INDEX_HEAP_PER_THREAD_MB)),
                d.index_heap_per_thread_mb,
            )
            .describe(
                "The memory each search index thread collects words in before it writes them out. More writes fewer, \
                 bigger pieces; the threads' memory together must fit in the import memory.",
            )
            .needs_restart(),
        )
        .field(
            SettingField::new("pluginCpuPercent", "Plugin processor share", int(10, 100, Some("%")), d.plugin_cpu_percent)
                .describe(
                    "The share of the processor plugins may use. On Windows this is a hard limit. On Linux and macOS a \
                     value below 100% only gives plugins the lowest priority: they still use idle cores.",
                )
                .needs_restart(),
        )
        .field(
            SettingField::new("leaveCoresFree", "Cores left free", int(0, 32, None), d.leave_cores_free)
                .describe("Cores Debrief's thread pools leave for the rest of the computer, so it stays responsive during an import.")
                .needs_restart(),
        )
        // ---- DuckDB (advanced) ----
        .field(
            SettingField::new("duckMemoryLimitMb", "Database memory limit", mb(0, 65_536), d.duck_memory_limit_mb)
                .describe(
                    "The memory the database engine (DuckDB) keeps data in before it writes to disk. 0 takes it from the \
                     query memory. It must fit in the query memory.",
                )
                .needs_restart()
                .as_advanced(),
        )
        .field(
            SettingField::new("spillFolder", "Spill folder", FieldKind::Path { dir: true }, d.spill_folder.clone())
                .describe(
                    "Where the database writes what does not fit in its memory while a query runs. Empty uses each \
                     project's own folder. A fast disk with free space helps big sorts and analyses.",
                )
                .needs_restart()
                .as_advanced(),
        )
        .field(
            SettingField::new("maxSpillMb", "Spill size limit", mb(0, 1 << 20), d.max_spill_mb)
                .describe(
                    "The most the database may write to the spill folder; a query that needs more fails with a message \
                     instead of filling the disk. 0 lets it use up to 90% of the free space.",
                )
                .needs_restart()
                .as_advanced(),
        )
        .field(
            SettingField::new("allocatorFlushMb", "Kept freed memory", mb(1, 1024), d.allocator_flush_mb)
                .describe(
                    "How much freed memory the database keeps for reuse after a task instead of giving it back. Lower \
                     uses less memory between queries; higher can be a little faster.",
                )
                .needs_restart()
                .as_advanced(),
        )
        .field(
            SettingField::new("allocatorBackgroundThreads", "Free memory in the background", FieldKind::Bool, d.allocator_background_threads)
                .describe("The database gives freed memory back to the system from background threads (Linux and macOS; no effect on Windows).")
                .needs_restart()
                .as_advanced(),
        )
}

/// The cores this computer has (at least 1).
pub fn cores() -> u32 {
    std::thread::available_parallelism().map_or(1, |n| u32::try_from(n.get()).unwrap_or(u32::MAX))
}

/// What is wrong with `s` as a whole, one error per field (the ranges are the schema's; these are the relations).
pub fn check(s: &PerformanceSettings, cores: u32) -> Vec<FieldError> {
    let mut errors = Vec::new();
    let mut fail = |key: &str, message: String| errors.push(FieldError { key: key.into(), message });
    if s.import_budget_mb > s.store_cap_mb {
        fail(
            "importBudgetMb",
            format!(
                "The import memory ({} MB) must fit in the worker's cap ({} MB).",
                s.import_budget_mb, s.store_cap_mb
            ),
        );
    }
    if s.query_budget_mb > s.store_cap_mb {
        fail(
            "queryBudgetMb",
            format!(
                "The query memory ({} MB) must fit in the worker's cap ({} MB).",
                s.query_budget_mb, s.store_cap_mb
            ),
        );
    }
    if s.duck_memory_limit_mb > 0 && s.duck_memory_limit_mb > s.query_budget_mb {
        fail(
            "duckMemoryLimitMb",
            format!(
                "The database memory limit ({} MB) must fit in the query memory ({} MB).",
                s.duck_memory_limit_mb, s.query_budget_mb
            ),
        );
    }
    // Auto takes as many threads as fit, but at least one: one thread's heap must fit too.
    if s.index_heap_per_thread_mb > s.import_budget_mb {
        fail(
            "indexHeapPerThreadMb",
            format!(
                "A search index thread's memory ({} MB) must fit in the import memory ({} MB).",
                s.index_heap_per_thread_mb, s.import_budget_mb
            ),
        );
    }
    let index_mb = u64::from(s.index_writer_threads) * u64::from(s.index_heap_per_thread_mb);
    if s.index_writer_threads > 0 && index_mb > u64::from(s.import_budget_mb) && s.index_heap_per_thread_mb <= s.import_budget_mb {
        fail(
            "indexWriterThreads",
            format!(
                "{} search index threads of {} MB need {index_mb} MB; they must fit in the import memory ({} MB). Choose \
                 fewer threads, less memory per thread, or 0 (Auto).",
                s.index_writer_threads, s.index_heap_per_thread_mb, s.import_budget_mb
            ),
        );
    }
    if s.leave_cores_free > 0 && s.leave_cores_free >= cores {
        fail(
            "leaveCoresFree",
            format!("This computer has {cores} cores; leave at most {} free.", cores.saturating_sub(1)),
        );
    }
    let spill = s.spill_folder.trim();
    if !spill.is_empty() && !Path::new(spill).is_dir() {
        fail(
            "spillFolder",
            "The spill folder does not exist. Choose an existing folder, or leave it empty.".into(),
        );
    }
    errors
}

/// The values a preset puts in the section (the fields it does not name keep theirs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Preset {
    /// The tested budgets at full speed (the defaults).
    Standard,
    /// The old Low memory mode: fewer threads, smaller database memory, freed memory given back at once.
    LowMemory,
}

impl Preset {
    pub fn values(self) -> Value {
        match self {
            Preset::Standard => {
                let d = PerformanceSettings::default();
                json!({
                    "importBudgetMb": d.import_budget_mb,
                    "queryBudgetMb": d.query_budget_mb,
                    "returnMemoryAtOnce": d.return_memory_at_once,
                    "parseThreads": d.parse_threads,
                    "queryThreads": d.query_threads,
                    "indexWriterThreads": d.index_writer_threads,
                    "indexHeapPerThreadMb": d.index_heap_per_thread_mb,
                    "duckMemoryLimitMb": d.duck_memory_limit_mb,
                    "allocatorFlushMb": d.allocator_flush_mb,
                })
            }
            // DuckDB 128 MB (236 − 108), 4 parse threads, 2 query threads, 1 index thread of 16 MB (StoreOptions::low_memory).
            Preset::LowMemory => json!({
                "importBudgetMb": 384,
                "queryBudgetMb": 236,
                "returnMemoryAtOnce": true,
                "parseThreads": 4,
                "queryThreads": 2,
                "indexWriterThreads": 1,
                "indexHeapPerThreadMb": 16,
                "duckMemoryLimitMb": 0,
                "allocatorFlushMb": 8,
            }),
        }
    }
}

/// The section, served by the app (its values live in `performance.json`).
pub struct PerformanceSection {
    pub dir: Option<PathBuf>,
    /// The running values to update after a change (none in tests of the section alone).
    pub live: Option<Arc<Live>>,
}

impl PerformanceSection {
    fn settings_with(&self, values: &ValidValues) -> PerformanceSettings {
        super::load(self.dir.as_deref()).with_values(&values.to_json())
    }
}

impl Configurable for PerformanceSection {
    fn settings_section(&self) -> SettingsSection {
        schema()
    }

    fn apply_settings(&self, values: &ValidValues) -> anyhow::Result<()> {
        let dir = self
            .dir
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("there is no configuration folder"))?;
        let s = self.settings_with(values);
        if let Some(e) = check(&s, cores()).first() {
            anyhow::bail!("{}", e.message);
        }
        super::save(dir, &s)?;
        if let Some(live) = &self.live {
            live.apply(&s);
        }
        Ok(())
    }
}

impl ExternalValues for PerformanceSection {
    fn load(&self) -> Value {
        super::load(self.dir.as_deref()).to_values()
    }

    fn check(&self, values: &ValidValues) -> Vec<FieldError> {
        check(&self.settings_with(values), cores())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::performance::tests::temp;

    #[test]
    fn the_schema_is_valid_and_its_defaults_are_the_settings_defaults() {
        let s = schema();
        s.check().unwrap();
        assert_eq!(s.id, PERFORMANCE_SECTION);
        assert_eq!(s.defaults().to_json(), PerformanceSettings::default().to_values());
        assert!(s.fields.iter().all(|f| f.restart_required), "everything applies at the next start");
        assert!(s.fields.iter().all(|f| !f.description.is_empty()), "every field explains itself");
        let advanced: Vec<&str> = s.fields.iter().filter(|f| f.advanced).map(|f| f.key.as_str()).collect();
        assert_eq!(
            advanced,
            [
                "duckMemoryLimitMb",
                "spillFolder",
                "maxSpillMb",
                "allocatorFlushMb",
                "allocatorBackgroundThreads"
            ]
        );
        let cpu = s.find("pluginCpuPercent").unwrap();
        assert!(cpu.description.contains("Windows") && cpu.description.contains("lowest priority"));
    }

    #[test]
    fn the_index_read_threads_are_a_setting_with_auto_checked_in_rust() {
        let s = schema();
        let f = s.find("indexReadThreads").expect("the read threads are a field");
        assert!(f.restart_required && f.description.contains("Auto"));
        assert_eq!(s.defaults().to_json()["indexReadThreads"], 0, "Auto by default");
        for (value, ok) in [(-1, false), (0, true), (8, true), (64, true), (65, false)] {
            let r = debrief_core::settings::validate(&s, &json!({ "indexReadThreads": value }));
            assert_eq!(r.is_ok(), ok, "{value} threads");
        }
    }

    #[test]
    fn the_index_writer_threads_and_heap_per_thread_are_settings_checked_in_rust() {
        let s = schema();
        assert!(s.find("indexThreads").is_none(), "replaced by indexWriterThreads (0 is Auto)");
        let threads = s.find("indexWriterThreads").expect("the writer's threads are a field");
        let heap = s.find("indexHeapPerThreadMb").expect("the heap of each writer thread is a field");
        assert!(threads.restart_required && heap.restart_required && !threads.advanced && !heap.advanced);
        assert!(threads.description.contains("Auto") && heap.description.contains("each"));
        let d = s.defaults().to_json();
        assert_eq!(d["indexWriterThreads"], 0, "Auto by default");
        assert_eq!(d["indexHeapPerThreadMb"], DEFAULT_INDEX_HEAP_PER_THREAD_MB);
        for (value, ok) in [(-1, false), (0, true), (1, true), (13, true), (64, true), (65, false)] {
            let r = debrief_core::settings::validate(&s, &json!({ "indexWriterThreads": value }));
            assert_eq!(r.is_ok(), ok, "{value} threads");
        }
        // Tantivy needs at least 15 MB a thread.
        for (value, ok) in [(15, false), (16, true), (64, true), (256, true), (1024, true), (1025, false)] {
            let r = debrief_core::settings::validate(&s, &json!({ "indexHeapPerThreadMb": value }));
            assert_eq!(r.is_ok(), ok, "{value} MB");
        }
    }

    #[test]
    fn explicit_index_threads_must_fit_their_heaps_in_the_import_memory() {
        let fits = PerformanceSettings {
            index_writer_threads: 12,
            index_heap_per_thread_mb: 64,
            ..PerformanceSettings::default()
        };
        assert!(check(&fits, 16).is_empty(), "12 x 64 MB = 768 MB, the import memory");
        let over = PerformanceSettings {
            index_writer_threads: 13,
            ..fits.clone()
        };
        let e = check(&over, 16);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].key, "indexWriterThreads");
        assert!(
            e[0].message.contains("832 MB") && e[0].message.contains("768 MB"),
            "{}",
            e[0].message
        );
        // Auto takes as many threads as fit, at least one, so one thread's heap must fit as well.
        let auto = PerformanceSettings {
            index_heap_per_thread_mb: 1024,
            ..PerformanceSettings::default()
        };
        let e = check(&auto, 16);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].key, "indexHeapPerThreadMb");
        assert!(
            e[0].message.contains("1024 MB") && e[0].message.contains("768 MB"),
            "{}",
            e[0].message
        );
        let fits = PerformanceSettings {
            index_heap_per_thread_mb: 768,
            ..PerformanceSettings::default()
        };
        assert!(check(&fits, 16).is_empty(), "one thread of 768 MB fits");
    }

    #[test]
    fn relations_between_fields_are_checked() {
        assert!(check(&PerformanceSettings::default(), 8).is_empty());
        let s = PerformanceSettings {
            store_cap_mb: 1024,
            import_budget_mb: 2048,
            query_budget_mb: 1500,
            duck_memory_limit_mb: 2000,
            leave_cores_free: 4,
            spill_folder: "/definitely/not/a/folder/debrief".into(),
            ..PerformanceSettings::default()
        };
        let keys: Vec<String> = check(&s, 4).into_iter().map(|e| e.key).collect();
        assert_eq!(
            keys,
            [
                "importBudgetMb",
                "queryBudgetMb",
                "duckMemoryLimitMb",
                "leaveCoresFree",
                "spillFolder"
            ]
        );
        let ok = PerformanceSettings {
            spill_folder: std::env::temp_dir().to_string_lossy().into_owned(),
            duck_memory_limit_mb: 200,
            ..PerformanceSettings::default()
        };
        assert!(check(&ok, 2).is_empty(), "{:?}", check(&ok, 2));
    }

    #[test]
    fn presets_are_values_the_schema_accepts() {
        let s = schema();
        for p in [Preset::Standard, Preset::LowMemory] {
            let (_, errors) = debrief_core::settings::sanitize(&s, &p.values());
            assert!(errors.is_empty(), "{p:?}: {errors:?}");
            let applied = PerformanceSettings::default().with_values(&p.values());
            assert!(check(&applied, 8).is_empty(), "{p:?}");
        }
        assert_eq!(
            PerformanceSettings::default().with_values(&Preset::Standard.values()),
            PerformanceSettings::default()
        );
        let low = PerformanceSettings::default().with_values(&Preset::LowMemory.values());
        assert!(low.return_memory_at_once);
        assert_eq!(
            low.query_budget_mb - WORKER_BASE_MB,
            128,
            "DuckDB's limit of the old Low memory mode"
        );
    }

    #[test]
    fn the_section_saves_what_the_processes_read_and_refuses_what_does_not_fit() {
        let d = temp("section");
        let section = PerformanceSection {
            dir: Some(d.clone()),
            live: None,
        };
        let schema = section.settings_section();
        let (valid, _) = debrief_core::settings::sanitize(&schema, &json!({"storeCapMb": 4096, "importBudgetMb": 1024}));
        assert!(section.check(&valid).is_empty());
        section.apply_settings(&valid).unwrap();
        assert_eq!(super::super::load(Some(&d)).store_cap_mb, 4096);
        assert_eq!(section.load()["importBudgetMb"], 1024);
        let (too_big, _) = debrief_core::settings::sanitize(&schema, &json!({"storeCapMb": 512, "importBudgetMb": 1024}));
        assert_eq!(section.check(&too_big)[0].key, "importBudgetMb");
        assert!(section.apply_settings(&too_big).is_err(), "applying re-checks");
        assert_eq!(super::super::load(Some(&d)).store_cap_mb, 4096, "nothing saved");
        let none = PerformanceSection { dir: None, live: None };
        assert!(none.apply_settings(&valid).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
