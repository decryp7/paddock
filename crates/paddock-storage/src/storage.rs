//! [`Storage`]: measure what the app keeps, then clear it safely. A scan is the preview (what each category holds, what clearing
//! would remove, the biggest items); a clear refuses to run without one, asks the user's confirmation per `data` category, and
//! re-checks every item against the live file system before deleting it ([`crate::guard`]). Both run with live progress and stop at
//! Cancel, leaving the state consistent: a scan changes nothing, a clear reports what it did.

use crate::category::{Category, How, Source};
use crate::error::StorageError;
use crate::guard::{Seen, recheck};
use crate::model::{
    CategoryClearResult, ClearRequest, ItemView, Kind, StorageCategory, StorageClearResult,
    StorageTree, StorageUsage,
};
use crate::run::Run;
use crate::walk::{Budget, MAX_ENTRIES, MAX_SCAN_TIME, Removal, remove, size_of};
use paddock_env::AppDirs;
use paddock_files::path::canonical;
use paddock_jobs::time::{Clock, SystemClock};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The biggest items listed per category in a preview.
pub const MAX_PREVIEW_ITEMS: usize = 8;
/// Entries the re-check of one item looks at at most.
pub const RECHECK_ENTRIES: u64 = 200_000;
/// How long the re-check of one item may take.
pub const RECHECK_TIME: std::time::Duration = std::time::Duration::from_secs(60);
/// Categories one clear request names at most.
pub const MAX_CATEGORIES: usize = 32;

type Snapshot = HashMap<String, HashMap<PathBuf, Seen>>;

/// The storage service of an app.
pub struct Storage {
    source: Arc<dyn Source>,
    boundaries: Vec<PathBuf>,
    clock: Arc<dyn Clock>,
    entries: u64,
    time: std::time::Duration,
    snapshot: Mutex<Option<Snapshot>>,
}

impl Storage {
    /// A service over the app's categories. Until [`Storage::within`] names the app's folders nothing can be cleared.
    pub fn new(source: Arc<dyn Source>) -> Storage {
        Storage {
            source,
            boundaries: Vec::new(),
            clock: Arc::new(SystemClock),
            entries: MAX_ENTRIES,
            time: MAX_SCAN_TIME,
            snapshot: Mutex::new(None),
        }
    }

    /// The app's own folders (data, config, cache, logs and runtime): a category whose folder is not inside one of them cannot be
    /// cleared, whatever the app declares.
    #[must_use]
    pub fn within(self, dirs: &AppDirs) -> Storage {
        self.also_within(dirs.data())
            .also_within(dirs.config())
            .also_within(dirs.cache())
            .also_within(dirs.logs())
            .also_within(dirs.runtime())
    }

    /// Another folder the app owns (the web view's profile, a folder in the system's temp folder the app creates).
    #[must_use]
    pub fn also_within(mut self, folder: &Path) -> Storage {
        self.boundaries.push(folder.to_path_buf());
        self
    }

    /// Replaces the clock the scan's time limit runs on (tests).
    #[must_use]
    pub fn clock(mut self, clock: Arc<dyn Clock>) -> Storage {
        self.clock = clock;
        self
    }

    /// Replaces the scan's limits (tests).
    #[must_use]
    pub fn limits(mut self, entries: u64, time: std::time::Duration) -> Storage {
        self.entries = entries;
        self.time = time;
        self
    }

    fn budget(&self) -> Budget {
        Budget::new(self.entries, self.time, self.clock.clone())
    }

    /// The limits of the re-check of one item right before it is deleted: its own, not the scan's.
    fn item_budget(&self) -> Budget {
        Budget::new(RECHECK_ENTRIES, RECHECK_TIME, self.clock.clone())
    }

    fn snapshot(&self) -> std::sync::MutexGuard<'_, Option<Snapshot>> {
        self.snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The category's folder, canonical, if it exists and is inside one of the app's folders. `Err` says why not.
    fn root_of(&self, c: &Category) -> Result<Option<PathBuf>, String> {
        if let Some(why) = &c.blocked {
            return Err(why.clone());
        }
        let Some(real) = c.root.as_ref().and_then(|root| canonical(root).ok()) else {
            return Ok(None);
        };
        let inside = self
            .boundaries
            .iter()
            .filter_map(|b| canonical(b).ok())
            .any(|b| real.starts_with(&b));
        if inside {
            Ok(Some(real))
        } else {
            Err("This folder is outside the folders the app owns, so it is never cleared".into())
        }
    }

    /// Measures every category and keeps what it saw as the preview a later clear is checked against. Cancel changes nothing.
    pub fn scan(&self, run: &dyn Run) -> Result<StorageUsage, StorageError> {
        let categories = self.source.categories();
        let mut budget = self.budget();
        let mut snapshot = Snapshot::new();
        let mut rows = Vec::with_capacity(categories.len());
        for c in &categories {
            if run.cancelled() {
                return Err(StorageError::Cancelled);
            }
            run.phase(
                &c.id,
                &format!("Measuring {}", c.label),
                Some(c.items.len() as u64),
            );
            let (row, seen) = self.measure(c, &mut budget, run);
            if run.cancelled() {
                return Err(StorageError::Cancelled);
            }
            snapshot.insert(c.id.clone(), seen);
            rows.push(row);
        }
        let usage = StorageUsage {
            total_bytes: rows.iter().map(|r| r.bytes).sum(),
            capped: rows.iter().any(|r| r.capped),
            categories: rows,
        };
        *self.snapshot() = Some(snapshot);
        Ok(usage)
    }

    fn measure(
        &self,
        c: &Category,
        budget: &mut Budget,
        run: &dyn Run,
    ) -> (StorageCategory, HashMap<PathBuf, Seen>) {
        let verdict = self.root_of(c);
        let mut row = StorageCategory {
            id: c.id.clone(),
            label: c.label.clone(),
            kind: c.kind,
            path: c.shown.as_ref().map(|p| p.to_string_lossy().into_owned()),
            exists: c
                .shown
                .as_ref()
                .is_some_and(|p| std::fs::symlink_metadata(p).is_ok()),
            bytes: 0,
            files: 0,
            clearable_bytes: 0,
            clearable_items: 0,
            capped: false,
            clearable: verdict.is_ok(),
            note: match &verdict {
                Err(why) => Some(why.clone()),
                Ok(_) => c.note.clone(),
            },
            items: Vec::new(),
        };
        let mut seen = HashMap::new();
        let mut views = Vec::new();
        for item in &c.items {
            if run.cancelled() {
                break;
            }
            let size = size_of(&item.path, budget, run);
            run.add(1);
            row.bytes += size.bytes;
            row.files += size.files;
            row.capped |= size.capped;
            if verdict.is_ok() && item.protect.is_none() {
                row.clearable_bytes += size.bytes;
                row.clearable_items += 1;
            }
            seen.insert(
                item.path.clone(),
                Seen {
                    folder: std::fs::symlink_metadata(&item.path).is_ok_and(|m| m.is_dir()),
                    newest: size.newest,
                },
            );
            views.push(ItemView {
                name: item
                    .path
                    .file_name()
                    .unwrap_or(item.path.as_os_str())
                    .to_string_lossy()
                    .into_owned(),
                bytes: size.bytes,
                protected: item.protect.clone(),
            });
        }
        views.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
        views.truncate(MAX_PREVIEW_ITEMS);
        row.items = views;
        (row, seen)
    }

    /// Clears what the user chose. Needs a scan first (the preview); a `data` category needs its own confirmation; every item is
    /// re-checked right before it is deleted. A category that cannot be cleared, or items left in place, are results, not errors.
    pub fn clear(
        &self,
        request: &ClearRequest,
        run: &dyn Run,
    ) -> Result<StorageClearResult, StorageError> {
        self.validate(request)?;
        let declared = self.source.categories();
        let chosen: Vec<(&Category, bool)> = request
            .categories
            .iter()
            .filter_map(|choice| {
                declared
                    .iter()
                    .find(|c| c.id == choice.id)
                    .map(|c| (c, choice.confirmed))
            })
            .collect();
        let snapshot = self.snapshot().clone().ok_or(StorageError::NoPreview)?;
        // The preview is used up by this clear: what is left is measured again before the next one.
        *self.snapshot() = None;

        let mut results = Vec::new();
        let mut work: Vec<(&Category, PathBuf)> = Vec::new();
        for (c, confirmed) in &chosen {
            if c.kind == Kind::Data && !confirmed {
                results.push(CategoryClearResult::failed(
                    &c.id,
                    "It holds your own data: confirm to clear it.",
                ));
                continue;
            }
            match self.root_of(c) {
                Err(why) => results.push(CategoryClearResult::failed(&c.id, why)),
                Ok(None) => results.push(CategoryClearResult::new(&c.id)),
                Ok(Some(root)) => {
                    results.push(CategoryClearResult::new(&c.id));
                    work.push((c, root));
                }
            }
        }
        let total: u64 = work
            .iter()
            .map(|(c, _)| c.items.iter().filter(|i| i.protect.is_none()).count() as u64)
            .sum();
        run.phase("clear", "Removing", Some(total));
        let mut cancelled = false;
        'all: for (c, root) in &work {
            let seen = snapshot.get(&c.id);
            let mut removal = Removal::default();
            for item in c.items.iter().filter(|i| i.protect.is_none()) {
                if run.cancelled() {
                    cancelled = true;
                    store(&mut results, &c.id, removal);
                    break 'all;
                }
                match recheck(
                    &item.path,
                    root,
                    seen.and_then(|s| s.get(&item.path)),
                    &mut self.item_budget(),
                ) {
                    Ok(None) => {}
                    Err(why) => removal.skip(&item.path, why),
                    Ok(Some((real, live))) => match &item.how {
                        How::Path => {
                            if !remove(&real, run, &mut removal) && run.cancelled() {
                                cancelled = true;
                                store(&mut results, &c.id, removal);
                                break 'all;
                            }
                        }
                        How::Custom(f) => match f(&real) {
                            Ok(()) => {
                                removal.freed_bytes += live.size.bytes;
                                removal.removed_files += live.size.files;
                            }
                            Err(why) => removal.skip(&item.path, why),
                        },
                    },
                }
                run.add(1);
            }
            store(&mut results, &c.id, removal);
        }
        Ok(StorageClearResult {
            freed_bytes: results.iter().map(|r| r.freed_bytes).sum(),
            categories: results,
            cancelled,
        })
    }

    /// Checks a request without doing anything: not too many categories, each one the app keeps and named once, and a preview to
    /// clear against. A command calls this before it starts the job, so a refusal reaches the user at once.
    pub fn validate(&self, request: &ClearRequest) -> Result<(), StorageError> {
        if request.categories.len() > MAX_CATEGORIES {
            return Err(StorageError::TooMany);
        }
        let declared = self.source.categories();
        for (i, choice) in request.categories.iter().enumerate() {
            if request.categories[..i].iter().any(|c| c.id == choice.id) {
                return Err(StorageError::Duplicate(choice.id.clone()));
            }
            if !declared.iter().any(|c| c.id == choice.id) {
                return Err(StorageError::UnknownCategory(choice.id.clone()));
            }
        }
        if self.snapshot().is_none() {
            return Err(StorageError::NoPreview);
        }
        Ok(())
    }

    /// The folder of category `id` that a tree may walk: canonical and inside the app's folders.
    fn tree_root(&self, id: &str) -> Result<(Category, PathBuf), StorageError> {
        let c = self
            .source
            .categories()
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| StorageError::UnknownCategory(id.to_string()))?;
        match self.root_of(&c) {
            Ok(Some(root)) => Ok((c, root)),
            Ok(None) => Err(StorageError::Unavailable(
                "This folder does not exist yet".into(),
            )),
            Err(why) => Err(StorageError::Unavailable(why)),
        }
    }

    /// Checks that category `id` can be walked, without walking it (a command calls this before it starts the job).
    pub fn validate_tree(&self, id: &str) -> Result<(), StorageError> {
        self.tree_root(id).map(drop)
    }

    /// The bounded size tree of category `id`'s folder (depth, items per folder, entries and time limited; no links; one volume).
    /// A cancelled walk returns what it saw, marked `capped`.
    pub fn tree(&self, id: &str, run: &dyn Run) -> Result<StorageTree, StorageError> {
        let (c, root) = self.tree_root(id)?;
        run.phase("tree", &format!("Measuring {}", c.label), None);
        let (node, capped) = crate::tree::size_tree(&root, &c.label, &mut self.budget(), run);
        Ok(StorageTree {
            id: c.id,
            root: node,
            capped,
        })
    }

    /// The folder to show in the file manager for category `id` (it must exist).
    pub fn folder(&self, id: &str) -> Result<PathBuf, StorageError> {
        let c = self
            .source
            .categories()
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| StorageError::UnknownCategory(id.to_string()))?;
        c.shown
            .filter(|p| p.exists())
            .ok_or_else(|| StorageError::UnknownCategory(id.to_string()))
    }
}

fn store(results: &mut [CategoryClearResult], id: &str, removal: Removal) {
    if let Some(r) = results.iter_mut().find(|r| r.id == id) {
        r.freed_bytes = removal.freed_bytes;
        r.removed_files = removal.removed_files;
        r.skipped = removal.skipped;
        r.skipped_count = removal.skipped_count;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::category::{Item, Source};
    use crate::run::Recorder;
    use crate::walk::tests::write;
    use paddock_jobs::time::FakeClock;
    use std::time::Duration;

    struct Fx {
        _d: tempfile::TempDir,
        root: PathBuf,
    }

    fn fx() -> Fx {
        let d = tempfile::tempdir().unwrap();
        let root = canonical(d.path()).unwrap();
        Fx { _d: d, root }
    }

    fn storage(fx: &Fx, categories: Vec<Category>) -> Storage {
        Storage::new(Arc::new(categories)).also_within(&fx.root)
    }

    fn choice(id: &str, confirmed: bool) -> crate::model::ClearChoice {
        crate::model::ClearChoice {
            id: id.into(),
            confirmed,
        }
    }

    fn request(choices: Vec<crate::model::ClearChoice>) -> ClearRequest {
        ClearRequest {
            categories: choices,
        }
    }

    /// A cache of three items and logs where today's file stays.
    fn standard(fx: &Fx) -> Vec<Category> {
        write(&fx.root.join("cache/a.bin"), 100);
        write(&fx.root.join("cache/dir/b.bin"), 50);
        write(&fx.root.join("cache/dir/c.bin"), 25);
        write(&fx.root.join("logs/old.log"), 10);
        write(&fx.root.join("logs/today.log"), 5);
        vec![
            Category::new("cache", "Cache", Kind::Cache)
                .folder(fx.root.join("cache"))
                .entries(|_| None),
            Category::new("logs", "Logs", Kind::Data)
                .folder(fx.root.join("logs"))
                .entries(|p| {
                    p.ends_with("today.log")
                        .then(|| "being written".to_string())
                })
                .note("Today's log is kept."),
        ]
    }

    #[test]
    fn a_scan_measures_every_category_and_previews_what_clearing_removes() {
        let fx = fx();
        let s = storage(&fx, standard(&fx));
        let rec = Recorder::new();
        let usage = s.scan(&rec).unwrap();
        assert_eq!(usage.total_bytes, 190);
        assert!(!usage.capped);
        let cache = &usage.categories[0];
        assert_eq!(
            (
                cache.bytes,
                cache.files,
                cache.clearable_bytes,
                cache.clearable_items
            ),
            (175, 3, 175, 2)
        );
        assert!(cache.exists && cache.clearable && cache.kind == Kind::Cache);
        assert_eq!(
            cache
                .items
                .iter()
                .map(|i| (i.name.as_str(), i.bytes))
                .collect::<Vec<_>>(),
            [("a.bin", 100), ("dir", 75)]
        );
        let logs = &usage.categories[1];
        assert_eq!(
            (logs.bytes, logs.clearable_bytes, logs.clearable_items),
            (15, 10, 1)
        );
        assert_eq!(logs.note.as_deref(), Some("Today's log is kept."));
        assert_eq!(
            logs.items
                .iter()
                .find(|i| i.name == "today.log")
                .unwrap()
                .protected
                .as_deref(),
            Some("being written")
        );
        // Live progress from the first moment, each category a phase with its own count.
        assert_eq!(
            rec.phases(),
            [
                ("Measuring Cache".to_string(), Some(2)),
                ("Measuring Logs".to_string(), Some(2))
            ]
        );
        assert_eq!(rec.done(), 4);
    }

    #[test]
    fn the_preview_lists_only_the_biggest_items_biggest_first() {
        let fx = fx();
        for i in 0..MAX_PREVIEW_ITEMS + 4 {
            write(&fx.root.join(format!("c/f{i:02}")), i + 1);
        }
        let s = storage(
            &fx,
            vec![
                Category::new("c", "C", Kind::Cache)
                    .folder(fx.root.join("c"))
                    .entries(|_| None),
            ],
        );
        let items = &s.scan(&()).unwrap().categories[0].items;
        assert_eq!(items.len(), MAX_PREVIEW_ITEMS);
        assert_eq!(items[0].bytes, (MAX_PREVIEW_ITEMS + 4) as u64);
        assert!(items.windows(2).all(|w| w[0].bytes >= w[1].bytes));
    }

    #[test]
    fn a_stopped_walk_says_so_and_a_cancelled_scan_changes_nothing() {
        let fx = fx();
        let clock = FakeClock::new();
        let s = storage(&fx, standard(&fx))
            .clock(Arc::new(clock.clone()))
            .limits(1_000, Duration::from_secs(10));
        /// Eleven seconds pass after every item: the scan's time limit is on the injected clock.
        struct Ticking(FakeClock);
        impl Run for Ticking {
            fn phase(&self, _: &str, _: &str, _: Option<u64>) {}
            fn add(&self, _: u64) {
                self.0.advance(Duration::from_secs(11));
            }
            fn cancelled(&self) -> bool {
                false
            }
        }
        let usage = s.scan(&Ticking(clock)).unwrap();
        assert!(usage.capped, "the time limit ended the walk");
        let few = storage(&fx, standard(&fx)).limits(2, Duration::from_secs(60));
        assert!(few.scan(&()).unwrap().capped);
        // A cancelled scan returns Cancelled and does not replace the preview of an earlier scan.
        let s = storage(&fx, standard(&fx));
        s.scan(&()).unwrap();
        let cancelled = Recorder::new();
        cancelled.cancel();
        assert_eq!(s.scan(&cancelled), Err(StorageError::Cancelled));
        assert!(
            s.clear(&request(vec![choice("cache", false)]), &()).is_ok(),
            "the earlier preview still stands"
        );
        // Cancel in the middle (after the first category's items) is the same.
        let s = storage(&fx, standard(&fx));
        assert_eq!(
            s.scan(&Recorder::new().cancel_after(1)),
            Err(StorageError::Cancelled)
        );
    }

    #[test]
    fn nothing_is_cleared_without_a_preview_and_the_preview_is_used_up_by_a_clear() {
        let fx = fx();
        let s = storage(&fx, standard(&fx));
        let ask = request(vec![choice("cache", false)]);
        assert_eq!(s.clear(&ask, &()), Err(StorageError::NoPreview));
        assert!(fx.root.join("cache/a.bin").exists());
        s.scan(&()).unwrap();
        s.clear(&ask, &()).unwrap();
        assert_eq!(
            s.clear(&ask, &()),
            Err(StorageError::NoPreview),
            "measure again before the next clear"
        );
    }

    #[test]
    fn clearing_removes_what_the_preview_showed_and_keeps_what_must_stay() {
        let fx = fx();
        let s = storage(&fx, standard(&fx));
        s.scan(&()).unwrap();
        let rec = Recorder::new();
        let result = s
            .clear(
                &request(vec![choice("cache", false), choice("logs", true)]),
                &rec,
            )
            .unwrap();
        assert_eq!(result.freed_bytes, 185);
        assert!(!result.cancelled);
        assert_eq!(
            (
                result.categories[0].freed_bytes,
                result.categories[0].removed_files
            ),
            (175, 3)
        );
        assert_eq!(
            (
                result.categories[1].freed_bytes,
                result.categories[1].removed_files
            ),
            (10, 1)
        );
        assert!(
            !fx.root.join("cache/a.bin").exists()
                && !fx.root.join("cache/dir").exists()
                && !fx.root.join("logs/old.log").exists()
        );
        assert!(
            fx.root.join("logs/today.log").exists(),
            "the protected item stays"
        );
        assert!(
            fx.root.join("cache").is_dir(),
            "the category's folder itself is never removed"
        );
        assert_eq!(rec.phases(), [("Removing".to_string(), Some(3))]);
        assert_eq!(rec.done(), 3);
    }

    #[test]
    fn the_users_data_needs_its_own_confirmation_and_the_rest_goes_on() {
        let fx = fx();
        let s = storage(&fx, standard(&fx));
        s.scan(&()).unwrap();
        let result = s
            .clear(
                &request(vec![choice("logs", false), choice("cache", false)]),
                &(),
            )
            .unwrap();
        assert!(
            result.categories[0]
                .error
                .as_deref()
                .unwrap()
                .contains("confirm")
        );
        assert!(
            fx.root.join("logs/old.log").exists(),
            "unconfirmed data is untouched"
        );
        assert!(
            !fx.root.join("cache/a.bin").exists(),
            "the cache was cleared"
        );
        assert_eq!(result.freed_bytes, 175);
    }

    #[test]
    fn a_request_is_refused_whole_when_it_names_something_the_app_does_not_keep() {
        let fx = fx();
        let s = storage(&fx, standard(&fx));
        s.scan(&()).unwrap();
        for id in ["../etc", "nope", ""] {
            assert!(
                matches!(
                    s.clear(
                        &request(vec![choice("cache", false), choice(id, true)]),
                        &()
                    ),
                    Err(StorageError::UnknownCategory(_))
                ),
                "{id:?}"
            );
        }
        assert!(fx.root.join("cache/a.bin").exists(), "nothing was removed");
        assert!(matches!(
            s.clear(
                &request(vec![choice("cache", false), choice("cache", false)]),
                &()
            ),
            Err(StorageError::Duplicate(_))
        ));
        let many = (0..=MAX_CATEGORIES)
            .map(|i| choice(&format!("c{i}"), false))
            .collect();
        assert_eq!(s.clear(&request(many), &()), Err(StorageError::TooMany));
        assert!(
            s.clear(&request(vec![choice("cache", false)]), &()).is_ok(),
            "refusals do not use up the preview"
        );
    }

    #[test]
    fn an_item_that_changed_or_appeared_since_the_preview_is_left_in_place_with_a_reason() {
        let fx = fx();
        struct Fresh(PathBuf);
        impl Source for Fresh {
            fn categories(&self) -> Vec<Category> {
                // The list is read again at every call, as an app does.
                vec![
                    Category::new("cache", "Cache", Kind::Cache)
                        .folder(self.0.join("cache"))
                        .entries(|_| None),
                ]
            }
        }
        standard(&fx);
        let s = Storage::new(Arc::new(Fresh(fx.root.clone()))).also_within(&fx.root);
        s.scan(&()).unwrap();
        // A newer file inside a listed folder, a listed file replaced by a folder, and a new item.
        let later = std::fs::metadata(fx.root.join("cache/dir"))
            .unwrap()
            .modified()
            .unwrap()
            + Duration::from_secs(120);
        write(&fx.root.join("cache/dir/new.bin"), 1);
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(fx.root.join("cache/dir/new.bin"))
            .unwrap();
        f.set_modified(later).unwrap();
        std::fs::remove_file(fx.root.join("cache/a.bin")).unwrap();
        std::fs::create_dir(fx.root.join("cache/a.bin")).unwrap();
        write(&fx.root.join("cache/appeared.bin"), 1);
        let result = s
            .clear(&request(vec![choice("cache", false)]), &())
            .unwrap();
        let reasons: Vec<String> = result.categories[0]
            .skipped
            .iter()
            .map(|s| s.reason.clone())
            .collect();
        assert_eq!(result.categories[0].skipped_count, 3, "{reasons:?}");
        assert!(
            reasons
                .iter()
                .any(|r| r.contains("changed after the preview"))
        );
        assert!(reasons.iter().any(|r| r.contains("file to a folder")));
        assert!(
            reasons
                .iter()
                .any(|r| r.contains("appeared after the preview"))
        );
        assert!(
            fx.root.join("cache/dir/b.bin").exists() && fx.root.join("cache/appeared.bin").exists()
        );
    }

    #[test]
    fn an_item_that_is_gone_is_not_an_error() {
        let fx = fx();
        let s = storage(&fx, standard(&fx));
        s.scan(&()).unwrap();
        std::fs::remove_file(fx.root.join("cache/a.bin")).unwrap();
        let result = s
            .clear(&request(vec![choice("cache", false)]), &())
            .unwrap();
        assert_eq!(result.categories[0].skipped_count, 0);
        assert_eq!(result.categories[0].freed_bytes, 75);
    }

    #[test]
    fn cancel_stops_between_items_and_what_was_done_stays_done() {
        let fx = fx();
        let s = storage(&fx, standard(&fx));
        s.scan(&()).unwrap();
        let result = s
            .clear(
                &request(vec![choice("cache", false), choice("logs", true)]),
                &Recorder::new().cancel_after(1),
            )
            .unwrap();
        assert!(result.cancelled);
        assert!(
            !fx.root.join("cache/a.bin").exists(),
            "the first item was removed"
        );
        assert!(
            fx.root.join("cache/dir/b.bin").exists(),
            "the second was not"
        );
        assert!(
            fx.root.join("logs/old.log").exists(),
            "the next category was not started"
        );
        assert_eq!(result.categories[0].removed_files, 1);
        // Cancel while an item is being removed leaves it in place (a folder keeps what was not removed).
        let s = storage(&fx, standard(&fx));
        s.scan(&()).unwrap();
        let rec = Recorder::new();
        rec.cancel();
        let result = s
            .clear(&request(vec![choice("cache", false)]), &rec)
            .unwrap();
        assert!(result.cancelled && result.freed_bytes == 0);
    }

    #[test]
    fn cancel_while_an_item_is_being_removed_reports_a_partial_result() {
        let fx = fx();
        let s = storage(&fx, standard(&fx));
        s.scan(&()).unwrap();
        let result = s
            .clear(
                &request(vec![choice("cache", false)]),
                &Recorder::new().cancel_on_check(1),
            )
            .unwrap();
        assert!(result.cancelled);
        assert!(
            fx.root.join("cache/a.bin").exists(),
            "the item being removed when Cancel came stays"
        );
        assert_eq!(result.freed_bytes, 0);
    }

    #[test]
    fn the_app_can_remove_an_item_itself_and_its_refusal_is_listed() {
        let fx = fx();
        write(&fx.root.join("models/m1.gguf"), 40);
        write(&fx.root.join("models/m2.gguf"), 60);
        let ok = Arc::new(Mutex::new(Vec::new()));
        let ok2 = ok.clone();
        let c = Category::new("models", "Models", Kind::Data)
            .folder(fx.root.join("models"))
            .item(
                Item::path(fx.root.join("models/m1.gguf")).removed_by(move |p| {
                    ok2.lock().unwrap().push(p.to_path_buf());
                    std::fs::remove_file(p).map_err(|e| e.to_string())
                }),
            )
            .item(
                Item::path(fx.root.join("models/m2.gguf"))
                    .removed_by(|_| Err("the engine is using it".into())),
            );
        let s = storage(&fx, vec![c]);
        s.scan(&()).unwrap();
        let result = s
            .clear(&request(vec![choice("models", true)]), &())
            .unwrap();
        assert_eq!(ok.lock().unwrap().len(), 1);
        assert!(
            !fx.root.join("models/m1.gguf").exists() && fx.root.join("models/m2.gguf").exists()
        );
        assert_eq!(result.categories[0].freed_bytes, 40);
        assert_eq!(
            result.categories[0].skipped[0].reason,
            "the engine is using it"
        );
    }

    #[test]
    fn nothing_outside_the_apps_folders_is_ever_removed() {
        let fx = fx();
        let outside = tempfile::tempdir().unwrap();
        write(&outside.path().join("precious.txt"), 9);
        let cats = vec![
            Category::new("outside", "Outside", Kind::Cache)
                .folder(outside.path())
                .entries(|_| None),
            Category::new("unset", "Unset", Kind::Cache),
            Category::new("missing", "Missing", Kind::Cache)
                .folder(fx.root.join("missing"))
                .entries(|_| None),
        ];
        let s = storage(&fx, cats);
        let usage = s.scan(&()).unwrap();
        assert!(!usage.categories[0].clearable);
        assert!(
            usage.categories[0]
                .note
                .as_deref()
                .unwrap()
                .contains("outside the folders the app owns")
        );
        assert_eq!(usage.categories[0].bytes, 9, "it can still be shown");
        assert!(
            !usage.categories[1].clearable
                && usage.categories[1].path.is_none()
                && !usage.categories[1].exists
        );
        assert!(usage.categories[2].clearable && !usage.categories[2].exists);
        let result = s
            .clear(
                &request(vec![
                    choice("outside", true),
                    choice("unset", true),
                    choice("missing", true),
                ]),
                &(),
            )
            .unwrap();
        assert!(
            result.categories[0]
                .error
                .as_deref()
                .unwrap()
                .contains("outside")
        );
        assert!(result.categories[1].error.is_some());
        assert_eq!(
            result.categories[2],
            CategoryClearResult::new("missing"),
            "a missing folder has nothing to clear"
        );
        assert!(outside.path().join("precious.txt").exists());
        // Without any boundary nothing is cleared.
        let bare = Storage::new(Arc::new(standard(&fx)));
        bare.scan(&()).unwrap();
        let r = bare
            .clear(&request(vec![choice("cache", false)]), &())
            .unwrap();
        assert!(r.categories[0].error.is_some() && fx.root.join("cache/a.bin").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_is_a_link_to_somewhere_else_is_judged_by_where_it_leads() {
        let fx = fx();
        let outside = tempfile::tempdir().unwrap();
        write(&outside.path().join("precious.txt"), 9);
        assert!(
            crate::walk::tests::link_dir(outside.path(), &fx.root.join("link")),
            "links are available here"
        );
        let s = storage(
            &fx,
            vec![
                Category::new("l", "L", Kind::Cache)
                    .folder(fx.root.join("link"))
                    .entries(|_| None),
            ],
        );
        let usage = s.scan(&()).unwrap();
        assert!(!usage.categories[0].clearable, "{:?}", usage.categories[0]);
        s.clear(&request(vec![choice("l", true)]), &()).unwrap();
        assert!(outside.path().join("precious.txt").exists());
    }

    #[test]
    fn the_apps_folders_are_the_boundary() {
        use paddock_env::{AppId, MapVars, Os};
        let fx = fx();
        let id = AppId::new("storagedemo", "Storage Demo").unwrap();
        let dirs = AppDirs::resolve(
            &id,
            Os::Linux,
            &MapVars::new().with("STORAGEDEMO_HOME", fx.root.as_os_str()),
            None,
            true,
        )
        .unwrap();
        dirs.ensure().unwrap();
        write(&dirs.cache().join("c.bin"), 7);
        let c = vec![
            Category::new("cache", "Cache", Kind::Cache)
                .folder(dirs.cache())
                .entries(|_| None),
        ];
        let s = Storage::new(Arc::new(c)).within(&dirs);
        s.scan(&()).unwrap();
        assert_eq!(
            s.clear(&request(vec![choice("cache", false)]), &())
                .unwrap()
                .freed_bytes,
            7
        );
    }

    #[test]
    fn a_tree_walks_a_categorys_folder_inside_the_boundary_only() {
        let fx = fx();
        let outside = tempfile::tempdir().unwrap();
        write(&outside.path().join("x"), 3);
        let mut cats = standard(&fx);
        cats.push(Category::new("out", "Out", Kind::Cache).folder(outside.path()));
        cats.push(Category::new("none", "None", Kind::Cache).folder(fx.root.join("none")));
        let s = storage(&fx, cats);
        let rec = Recorder::new();
        let tree = s.tree("cache", &rec).unwrap();
        assert_eq!(
            (
                tree.id.as_str(),
                tree.root.name.as_str(),
                tree.root.bytes,
                tree.capped
            ),
            ("cache", "Cache", 175, false)
        );
        assert_eq!(rec.phases(), [("Measuring Cache".to_string(), None)]);
        assert!(matches!(
            s.tree("nope", &()),
            Err(StorageError::UnknownCategory(_))
        ));
        assert!(
            matches!(s.tree("out", &()), Err(StorageError::Unavailable(w)) if w.contains("outside"))
        );
        assert!(
            matches!(s.tree("none", &()), Err(StorageError::Unavailable(w)) if w.contains("does not exist"))
        );
        assert!(s.validate_tree("cache").is_ok() && s.validate_tree("out").is_err());
    }

    #[test]
    fn a_categorys_folder_can_be_shown_only_when_it_exists() {
        let fx = fx();
        let s = storage(&fx, standard(&fx));
        assert_eq!(s.folder("cache").unwrap(), fx.root.join("cache"));
        assert!(matches!(
            s.folder("nope"),
            Err(StorageError::UnknownCategory(_))
        ));
        let gone = storage(
            &fx,
            vec![Category::new("g", "G", Kind::Cache).folder(fx.root.join("g"))],
        );
        assert!(gone.folder("g").is_err());
        let unset = storage(&fx, vec![Category::new("u", "U", Kind::Cache)]);
        assert!(unset.folder("u").is_err());
    }

    #[test]
    fn a_poisoned_preview_lock_recovers() {
        let fx = fx();
        let s = Arc::new(storage(&fx, standard(&fx)));
        let s2 = s.clone();
        let _ = std::thread::spawn(move || {
            let _hold = s2.snapshot();
            panic!("poison");
        })
        .join();
        assert!(s.scan(&()).is_ok());
    }
}
