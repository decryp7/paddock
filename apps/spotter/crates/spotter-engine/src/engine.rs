//! The engine facade. Its methods mirror the UI's `SpotterBackend` and return plain structs ([`crate::view`] and
//! core types); the shell maps them to its DTOs. Queries have no side effects; only the commands (save, delete,
//! pause, check, acknowledge, settings) change anything.
use crate::clock::{local_parts, Clock, Randomness, SystemClock, SystemRandom};
use crate::config_store;
use crate::db::Db;
use crate::error::{EngineError, Result};
use crate::fetch::{FetchConfig, PoliteFetcher};
use crate::notifier::{Notifier, NullNotifier};
use crate::pipeline::Checker;
use crate::secrets::SecretStore;
use crate::view::{DashboardView, EventView, ProductView, ReleaseView, SourcePreview, WatchDetail, WatchSummary, WhatsNewView};
use spotter_core::calendar::LocalZone;
use spotter_core::config::ConfigDoc;
use spotter_core::dashboard::{self, Range, WatchInput};
use spotter_core::import::{self, ImportFormat, Imported};
use spotter_core::limits::MAX_WATCHES;
use spotter_core::pack::{Pack, Preset};
use spotter_core::render::PageRenderer;
use spotter_core::schedule::{next_check, Schedule, SchedulePreview, Timing};
use spotter_core::schema::{FieldKind, SourceKindSpec};
use spotter_core::source::catalog;
use spotter_core::source::web_page::{discover_feeds, DiscoveredFeed};
use spotter_core::source::{Secrets, Source, SourceConfig};
use spotter_core::state::{WatchState, WatchStatus};
use spotter_core::time::Timestamp;
use spotter_core::version::Version;
use spotter_core::watch::{Settings, Watch, WatchId, WatchSpec};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

/// What changed, for the shell to push to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Watches, their state or a running check changed.
    Watches,
    /// The settings changed.
    Settings,
}

/// How the engine is set up.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Where `spotter.toml`, `spotter.db` and user packs live.
    pub data_dir: PathBuf,
    /// Spotter's version (for the `User-Agent`).
    pub app_version: String,
    /// HTTP behaviour; `None` for the defaults.
    pub fetch: Option<FetchConfig>,
    /// Checks that may run at once.
    pub max_concurrent: usize,
    /// How long a page may take to render.
    pub render_timeout: Duration,
}

impl EngineConfig {
    /// The defaults for a data folder.
    pub fn new(data_dir: impl Into<PathBuf>, app_version: &str) -> EngineConfig {
        EngineConfig {
            data_dir: data_dir.into(),
            app_version: app_version.to_string(),
            fetch: None,
            max_concurrent: 4,
            render_timeout: Duration::from_secs(45),
        }
    }
}

/// What the engine uses from its surroundings.
#[derive(Clone)]
pub struct Services {
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Randomness (ids, jitter).
    pub random: Arc<dyn Randomness>,
    /// The credential store.
    pub secrets: Arc<dyn SecretStore>,
    /// Notifications and actions.
    pub notifier: Arc<dyn Notifier>,
    /// The renderer for JavaScript pages, when one exists.
    pub renderer: Option<Arc<dyn PageRenderer>>,
}

impl Services {
    /// Real time and randomness, no notifier, no renderer.
    pub fn new(secrets: Arc<dyn SecretStore>) -> Services {
        Services {
            clock: Arc::new(SystemClock::new()),
            random: Arc::new(SystemRandom::default()),
            secrets,
            notifier: Arc::new(NullNotifier),
            renderer: None,
        }
    }
}

pub(crate) struct Inner {
    pub(crate) cfg: EngineConfig,
    pub(crate) svc: Services,
    pub(crate) db: Db,
    pub(crate) fetcher: Arc<PoliteFetcher>,
    pub(crate) checker: Checker,
    pub(crate) pack: Pack,
    pub(crate) doc: Mutex<ConfigDoc>,
    pub(crate) states: Mutex<BTreeMap<WatchId, WatchState>>,
    pub(crate) checking: Mutex<HashSet<WatchId>>,
    pub(crate) warnings: Mutex<Vec<String>>,
    pub(crate) migrated: bool,
    pub(crate) changes: broadcast::Sender<Change>,
}

/// The engine. Cheap to clone; clones share everything.
#[derive(Clone)]
pub struct Engine {
    pub(crate) inner: Arc<Inner>,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The keys of a source kind's secret fields.
fn secret_keys_of(kind: &str) -> Vec<String> {
    catalog::builtin(kind)
        .map(|k| {
            k.fields
                .into_iter()
                .filter(|f| matches!(f.kind, FieldKind::Secret))
                .map(|f| f.key)
                .collect()
        })
        .unwrap_or_default()
}

impl Engine {
    /// Opens the engine on a data folder: loads the configuration (migrating the old format), opens the database
    /// (working around damage) and reads user packs. Problems that were worked around are in [`Engine::warnings`].
    pub fn open(cfg: EngineConfig, svc: Services) -> Result<Engine> {
        std::fs::create_dir_all(&cfg.data_dir).map_err(|e| {
            EngineError::Internal(format!(
                "The data folder {} could not be created ({e}). Check that it is writable.",
                cfg.data_dir.display()
            ))
        })?;
        let loaded = config_store::load(&cfg.data_dir.join("spotter.toml"));
        let mut warnings = loaded.warnings;
        let (db, db_warnings) = Db::open(&cfg.data_dir.join("spotter.db"));
        warnings.extend(db_warnings);
        let (mut states, state_warnings) = db.load_states()?;
        warnings.extend(state_warnings);
        let ids: Vec<WatchId> = loaded.doc.watches.iter().filter_map(|w| WatchId::parse(&w.id).ok()).collect();
        states.retain(|id, _| ids.contains(id));
        let mut pack = Pack::builtin();
        warnings.extend(load_user_packs(&cfg.data_dir.join("packs"), &mut pack));
        let fetcher = Arc::new(
            PoliteFetcher::new(cfg.fetch.clone().unwrap_or_else(|| FetchConfig::new(&cfg.app_version))).map_err(EngineError::Internal)?,
        );
        let checker = Checker {
            fetcher: fetcher.clone(),
            renderer: svc.renderer.clone(),
            render_timeout: cfg.render_timeout,
        };
        let (changes, _) = broadcast::channel(64);
        let engine = Engine {
            inner: Arc::new(Inner {
                migrated: loaded.migrated,
                doc: Mutex::new(loaded.doc),
                states: Mutex::new(states),
                checking: Mutex::new(HashSet::new()),
                warnings: Mutex::new(warnings),
                cfg,
                svc,
                db,
                fetcher,
                checker,
                pack,
                changes,
            }),
        };
        if engine.inner.migrated {
            engine.save_config()?;
        }
        let _ = engine.inner.db.prune(engine.now(), &ids);
        Ok(engine)
    }

    pub(crate) fn now(&self) -> Timestamp {
        self.inner.svc.clock.now()
    }

    /// The machine's time zone as the shell reported it.
    pub(crate) fn local_zone(&self) -> LocalZone {
        LocalZone {
            name: self.inner.svc.clock.local_zone_name(),
            offset_minutes: self.inner.svc.clock.utc_offset_minutes(),
        }
    }

    /// When a watch is checked next if the check ran at `at`: its schedule, the source's minimum interval, backoff
    /// after failures and jitter. A schedule that cannot be evaluated falls back to the longest interval, never to a
    /// tight loop.
    pub(crate) fn next_check_after(&self, spec: &WatchSpec, at: Timestamp, failures: u32) -> Timestamp {
        let timing = Timing {
            min_interval_secs: spec.source.min_interval_secs(),
            failures,
            jitter: self.inner.svc.random.unit(),
        };
        next_check(&spec.schedule, &self.local_zone(), at, timing)
            .unwrap_or_else(|_| at.plus_secs(u64::from(spotter_core::limits::MAX_INTERVAL_SECS)))
    }

    pub(crate) fn emit(&self, c: Change) {
        // No subscriber is fine.
        let _ = self.inner.changes.send(c);
    }

    /// Subscribes to changes (the shell pushes the new state to the UI).
    pub fn subscribe(&self) -> broadcast::Receiver<Change> {
        self.inner.changes.subscribe()
    }

    /// Problems found while loading the configuration and state (worked around).
    pub fn warnings(&self) -> Vec<String> {
        lock(&self.inner.warnings).clone()
    }

    /// The old Release Monitor's configuration was migrated at this start.
    pub fn migrated_from_legacy(&self) -> bool {
        self.inner.migrated
    }

    /// Where the configuration file is.
    pub fn config_path(&self) -> PathBuf {
        self.inner.cfg.data_dir.join("spotter.toml")
    }

    pub(crate) fn save_config(&self) -> Result<()> {
        let doc = lock(&self.inner.doc).clone();
        config_store::save(&self.config_path(), &doc)
    }

    pub(crate) fn find_spec(&self, id: &str) -> Result<WatchSpec> {
        lock(&self.inner.doc)
            .watches
            .iter()
            .find(|w| w.id == id)
            .cloned()
            .ok_or_else(|| EngineError::NotFound(format!("There is no watch \"{id}\". It may have been deleted.")))
    }

    pub(crate) fn parse_spec(&self, spec: &WatchSpec) -> std::result::Result<Watch, Vec<String>> {
        Watch::parse(&self.inner.pack.resolve(spec))
    }

    pub(crate) fn watch_id(&self, id: &str) -> Result<WatchId> {
        WatchId::parse(id).map_err(EngineError::BadRequest)
    }

    pub(crate) fn secrets_for(&self, id: &str, source: &Source) -> Secrets {
        source
            .secret_keys()
            .into_iter()
            .filter_map(|k| self.inner.svc.secrets.get(id, &k).map(|v| (k, v)))
            .collect()
    }

    // ---- queries ----

    /// Every watch, in the user's order.
    pub fn list_watches(&self) -> Vec<WatchSummary> {
        let specs = lock(&self.inner.doc).watches.clone();
        specs.iter().map(|s| self.summary_of(s)).collect()
    }

    /// One watch with its history and check log.
    pub fn get_watch(&self, id: &str) -> Result<WatchDetail> {
        let spec = self.find_spec(id)?;
        let state = WatchId::parse(id)
            .ok()
            .and_then(|w| lock(&self.inner.states).get(&w).cloned())
            .unwrap_or_default();
        let secrets_set = secret_keys_of(&spec.source.kind)
            .into_iter()
            .filter(|k| self.inner.svc.secrets.get(id, k).is_some())
            .collect();
        Ok(WatchDetail {
            summary: self.summary_of(&spec),
            history: state.history.iter().map(ReleaseView::from).collect(),
            checks: state.checks.iter().cloned().collect(),
            secrets_set,
            products: product_views(&spec, &state),
            spec,
        })
    }

    pub(crate) fn summary_of(&self, spec: &WatchSpec) -> WatchSummary {
        let state = WatchId::parse(&spec.id)
            .ok()
            .and_then(|w| lock(&self.inner.states).get(&w).cloned())
            .unwrap_or_default();
        let checking = WatchId::parse(&spec.id).is_ok_and(|w| lock(&self.inner.checking).contains(&w));
        let kind = catalog::builtin(&spec.source.kind);
        let described = Source::parse(&spec.source)
            .map(|s| s.describe())
            .unwrap_or_else(|_| spec.source.kind.clone());
        WatchSummary {
            id: spec.id.clone(),
            name: spec.name.clone(),
            source_kind: spec.source.kind.clone(),
            source_label: kind.map_or_else(|| spec.source.kind.clone(), |k| k.label),
            source_description: described,
            paused: spec.paused,
            checking,
            latest: state.latest.as_ref().map(ReleaseView::from),
            acknowledged: state.acknowledged.as_ref().map(|v| v.text.clone()),
            installed: state.installed.as_ref().map(|v| v.text.clone()),
            unseen: state.has_unseen(),
            newer_than_installed: state.newer_than_installed(),
            last_check_ms: state.last_check.map(|t| t.0),
            next_check_ms: state.next_check.map(|t| t.0),
            failures: state.failures,
            schedule_text: spec.schedule.describe(),
            product_count: state.products.len() as u32,
            status: state.status,
        }
    }

    /// The built-in source kinds (the add-watch form is rendered from them).
    pub fn source_kinds(&self) -> Vec<SourceKindSpec> {
        catalog::builtin_kinds()
    }

    /// Presets from the built-in pack and the user's packs.
    pub fn presets(&self) -> Vec<Preset> {
        self.inner.pack.presets.clone()
    }

    /// Finds the feeds a page advertises.
    pub async fn discover_feeds(&self, url: &str) -> Result<Vec<DiscoveredFeed>> {
        let request = spotter_core::source::HttpRequest::get(url).header("Accept", "text/html, application/xhtml+xml");
        let parsed = url::Url::parse(url).map_err(|_| EngineError::BadRequest(format!("\"{url}\" is not a web address.")))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(EngineError::BadRequest("Enter an http:// or https:// address.".into()));
        }
        match self.inner.fetcher.get(&request, &Default::default()).await {
            Ok(crate::fetch::Fetched::Body { bytes, final_url, .. }) => Ok(discover_feeds(&String::from_utf8_lossy(&bytes), &final_url)),
            Ok(crate::fetch::Fetched::NotModified) => Ok(Vec::new()),
            Err(e) => Err(EngineError::Unavailable(e.message().to_string())),
        }
    }

    /// Builds drafts from a winget export, Brewfile or OPML text.
    pub fn import_list(&self, format: &str, text: &str) -> Result<Imported> {
        let format = ImportFormat::parse(format)
            .ok_or_else(|| EngineError::BadRequest(format!("\"{format}\" is not an import format (winget, brewfile, opml).")))?;
        import::import(format, text).map_err(|e| EngineError::Invalid(vec![e]))
    }

    /// Checks a draft without saving it. `secrets` are values typed into secret fields (not stored); a missing key
    /// falls back to the stored secret of an existing watch.
    pub async fn test_source(&self, draft: &WatchSpec, secrets: &BTreeMap<String, String>) -> SourcePreview {
        let mut spec = draft.clone();
        if spec.id.is_empty() {
            spec.id = "preview".into();
        }
        let watch = match self.parse_spec(&spec) {
            Ok(w) => w,
            Err(problems) => {
                return SourcePreview {
                    problems,
                    ..SourcePreview::default()
                }
            }
        };
        let mut typed: Secrets = self.secrets_for(&spec.id, &watch.source);
        typed.extend(secrets.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k.clone(), v.clone())));
        let started = Instant::now();
        let now = self.now();
        let gathered = self.preview_candidates(&watch, &typed, now).await;
        let took_ms = started.elapsed().as_millis() as u64;
        match gathered {
            Err(e) => SourcePreview {
                error: Some(WatchStatus::from_error(&e)),
                took_ms,
                ..SourcePreview::default()
            },
            Ok((candidates, scan)) => {
                let sel = spotter_core::release::select(candidates.clone(), &watch.scheme, &watch.filter, 20);
                let releases = watch.version_filter.apply_prefix(sel.releases);
                SourcePreview {
                    problems: Vec::new(),
                    error: None,
                    releases: releases
                        .iter()
                        .map(|r| {
                            ReleaseView::from(&spotter_core::state::KnownRelease {
                                release: r.clone(),
                                first_seen: now,
                            })
                        })
                        .collect(),
                    without_version: sel.without_version,
                    prereleases_skipped: sel.prereleases_skipped,
                    names_skipped: sel.names_skipped,
                    took_ms,
                    extraction: Some(crate::preview::extraction(&watch, candidates, scan)),
                }
            }
        }
    }

    /// The candidates for the wizard: a folder is scanned in preview mode (no settle wait, a sample of the names it
    /// left out); anything else is read like a check.
    async fn preview_candidates(
        &self,
        watch: &Watch,
        secrets: &Secrets,
        now: Timestamp,
    ) -> std::result::Result<(Vec<spotter_core::release::Candidate>, Option<crate::preview::ScanFacts>), spotter_core::source::SourceError>
    {
        use spotter_core::source::Plan;
        match watch.source.plan(secrets) {
            Plan::Folder(f) => {
                let scan = spotter_sources::folder::scan(spotter_sources::folder::ScanRequest {
                    source: f,
                    readable: crate::pipeline::readable_for(watch),
                    previous: None,
                    sample_ignored: 200,
                    settle: false,
                })
                .await?;
                let facts = crate::preview::ScanFacts {
                    stats: scan.stats,
                    ignored: scan.ignored,
                    created_reported: scan.created_reported,
                };
                Ok((scan.candidates, Some(facts)))
            }
            _ => match self.inner.checker.gather(watch, secrets, None, None, None, now).await? {
                crate::pipeline::Gathered::Unchanged => Ok((Vec::new(), None)),
                crate::pipeline::Gathered::Fresh { candidates, .. } => Ok((candidates, None)),
            },
        }
    }

    /// Writes a regular expression from an example name and the parts the user selected (the wizard's helper).
    pub fn suggest_pattern(&self, example: &str, selections: &[spotter_core::extract::Selection]) -> Result<String> {
        crate::preview::suggest_pattern(example, selections).map_err(|m| EngineError::Invalid(vec![m]))
    }

    /// Tries a version scheme on one name (the wizard's Test field); nothing is read from any source.
    pub fn test_pattern(&self, versions: &str, pattern: Option<&str>, missing_older: bool, text: &str) -> crate::preview::PatternTest {
        let resolved = self.inner.pack.resolve(&WatchSpec {
            versions: versions.into(),
            version_pattern: pattern.map(str::to_string),
            ..blank_spec()
        });
        crate::preview::test_pattern(&resolved.versions, resolved.version_pattern.as_deref(), missing_older, text)
    }

    /// Shows a schedule in words with its next runs, for the editor; nothing is saved. `source_kind` sets the minimum
    /// interval (a folder may be scanned more often than a site is asked).
    pub fn preview_schedule(&self, schedule: &Schedule, source_kind: &str) -> Result<SchedulePreview> {
        let min = SourceConfig::new(source_kind, &[]).min_interval_secs();
        spotter_core::schedule::preview(schedule, &self.local_zone(), self.now(), min).map_err(EngineError::Invalid)
    }

    /// The patterns the wizard offers to start from (built-in and the user's packs).
    pub fn version_schemes(&self) -> Vec<spotter_core::pack::SchemeDef> {
        self.inner.pack.schemes.clone()
    }

    // ---- watch commands ----

    /// Saves a draft (a new watch when its id is empty). `secrets`: a value stores it in the credential store, an
    /// empty string deletes it, a missing key keeps it.
    pub fn save_watch(&self, mut draft: WatchSpec, secrets: &BTreeMap<String, String>) -> Result<WatchSummary> {
        let is_new_id = draft.id.trim().is_empty();
        if is_new_id {
            let taken: Vec<String> = lock(&self.inner.doc).watches.iter().map(|w| w.id.clone()).collect();
            let fresh = (0..32)
                .map(|_| WatchId::from_random(self.inner.svc.random.next_u64().to_le_bytes()))
                .find(|id| !taken.iter().any(|t| t == id.as_str()));
            draft.id = fresh
                .ok_or_else(|| EngineError::Internal("No free watch id could be made.".into()))?
                .to_string();
        }
        if let Err(problems) = self.parse_spec(&draft) {
            return Err(EngineError::Invalid(problems));
        }
        let allowed = secret_keys_of(&draft.source.kind);
        if let Some(bad) = secrets.keys().find(|k| !allowed.contains(k)) {
            return Err(EngineError::Invalid(vec![format!(
                "\"{bad}\" is not a secret field of this source."
            )]));
        }
        let id = self.watch_id(&draft.id)?;
        let source_changed;
        let mut schedule_changed = false;
        {
            let mut doc = lock(&self.inner.doc);
            match doc.watches.iter_mut().find(|w| w.id == draft.id) {
                Some(existing) => {
                    schedule_changed = existing.schedule != draft.schedule;
                    source_changed = existing.source != draft.source
                        || existing.versions != draft.versions
                        || existing.version_pattern != draft.version_pattern;
                    *existing = draft.clone();
                }
                None => {
                    if doc.watches.len() >= MAX_WATCHES {
                        return Err(EngineError::Invalid(vec![format!("Spotter keeps at most {MAX_WATCHES} watches.")]));
                    }
                    source_changed = false;
                    doc.watches.push(draft.clone());
                }
            }
        }
        for (key, value) in secrets {
            let result = if value.is_empty() {
                self.inner.svc.secrets.delete(&draft.id, key)
            } else {
                self.inner.svc.secrets.set(&draft.id, key, value)
            };
            result.map_err(|m| EngineError::Unavailable(format!("The access token could not be stored in your credential store: {m}")))?;
        }
        if source_changed {
            // A different source is a different history: start again with a fresh baseline.
            lock(&self.inner.states).remove(&id);
            self.inner.db.delete_watch(&id)?;
        }
        if schedule_changed && !source_changed {
            // The new schedule counts from the last check, so "Next check" is right at once (and a check that is
            // overdue under the new schedule runs once, not repeatedly).
            let last = lock(&self.inner.states).get(&id).and_then(|s| s.last_check);
            let next = self.next_check_after(&draft, last.unwrap_or_else(|| self.now()), 0);
            if let Some(state) = lock(&self.inner.states).get_mut(&id) {
                state.next_check = Some(next);
                let snapshot = state.clone();
                self.inner.db.save_state(&id, &snapshot)?;
            }
        }
        self.save_config()?;
        self.emit(Change::Watches);
        Ok(self.summary_of(&draft))
    }

    /// Deletes a watch with its history and secrets.
    pub fn delete_watch(&self, id: &str) -> Result<()> {
        let spec = self.find_spec(id)?;
        let wid = self.watch_id(id)?;
        lock(&self.inner.doc).watches.retain(|w| w.id != id);
        lock(&self.inner.states).remove(&wid);
        self.inner.db.delete_watch(&wid)?;
        for key in secret_keys_of(&spec.source.kind) {
            // A secret that cannot be removed is not worth failing the delete for.
            let _ = self.inner.svc.secrets.delete(id, &key);
        }
        self.save_config()?;
        self.emit(Change::Watches);
        Ok(())
    }

    /// Pauses or resumes a watch.
    pub fn set_paused(&self, id: &str, paused: bool) -> Result<()> {
        self.find_spec(id)?;
        if let Some(w) = lock(&self.inner.doc).watches.iter_mut().find(|w| w.id == id) {
            w.paused = paused;
        }
        self.save_config()?;
        self.emit(Change::Watches);
        Ok(())
    }

    fn version_in(&self, id: &str, text: &str) -> Result<Version> {
        let spec = self.find_spec(id)?;
        let watch = self.parse_spec(&spec).map_err(EngineError::Invalid)?;
        watch.scheme.extract(text).ok_or_else(|| {
            EngineError::BadRequest(format!(
                "\"{text}\" is not a version in this watch's scheme ({}).",
                watch.scheme.id()
            ))
        })
    }

    fn change_state(&self, id: &str, f: impl FnOnce(&mut WatchState)) -> Result<()> {
        self.find_spec(id)?;
        let wid = self.watch_id(id)?;
        let snapshot = {
            let mut states = lock(&self.inner.states);
            let s = states.entry(wid.clone()).or_default();
            f(s);
            s.clone()
        };
        self.inner.db.save_state(&wid, &snapshot)?;
        self.emit(Change::Watches);
        Ok(())
    }

    /// Marks a version (the latest when `None`) as seen.
    pub fn acknowledge(&self, id: &str, version: Option<&str>) -> Result<()> {
        let v = version.map(|t| self.version_in(id, t)).transpose()?;
        self.change_state(id, |s| s.acknowledge(v))
    }

    /// Marks the latest release of one product as seen.
    pub fn acknowledge_product(&self, id: &str, product: &str) -> Result<()> {
        self.change_state(id, |s| s.acknowledge_product(product))
    }

    /// Forgets the acknowledgement: the latest release counts as new again.
    pub fn reset_acknowledgement(&self, id: &str) -> Result<()> {
        self.change_state(id, WatchState::reset_acknowledgement)
    }

    /// Sets (or clears) the installed version.
    pub fn set_installed(&self, id: &str, version: Option<&str>) -> Result<()> {
        let v = version.map(|t| self.version_in(id, t)).transpose()?;
        self.change_state(id, |s| s.set_installed(v))
    }

    /// Records a finished (or failed) download of a release in the journal; the shell does the downloading.
    pub fn record_download(&self, id: &str, version: &str, outcome: std::result::Result<(String, u64, bool), String>) -> Result<()> {
        use spotter_core::journal::{DownloadRecord, Event, EventKind};
        let wid = self.watch_id(id)?;
        let at = self.now();
        let (ok, size, kind) = match outcome {
            Ok((path, size, verified)) => (
                true,
                size,
                EventKind::DownloadFinished {
                    version: version.into(),
                    path,
                    size,
                    verified,
                },
            ),
            Err(message) => (
                false,
                0,
                EventKind::ActionFailed {
                    action: "download".into(),
                    message,
                },
            ),
        };
        self.inner.db.record_event(&Event {
            at,
            watch: Some(wid.clone()),
            kind,
        })?;
        self.inner.db.record_download(&DownloadRecord {
            at,
            watch: wid,
            version: version.into(),
            size,
            ok,
        })
    }

    /// A release the watch knows (the latest or one in its history), by its version text. The shell downloads or opens
    /// it; the engine does not.
    pub fn release_of(&self, id: &str, version: &str) -> Result<spotter_core::release::Release> {
        self.release_of_product(id, None, version)
    }

    /// Like [`Engine::release_of`], for a product of a watch with an `id` group (versions can repeat across products).
    pub fn release_of_product(&self, id: &str, product: Option<&str>, version: &str) -> Result<spotter_core::release::Release> {
        self.find_spec(id)?;
        let wid = self.watch_id(id)?;
        let states = lock(&self.inner.states);
        let state = states.get(&wid);
        let state = match product {
            Some(p) => state.and_then(|s| s.products.get(p)),
            None => state,
        };
        state
            .into_iter()
            .flat_map(|s| s.latest.iter().chain(s.history.iter()))
            .find(|k| k.release.version.text == version)
            .map(|k| k.release.clone())
            .ok_or_else(|| {
                EngineError::NotFound(format!(
                    "The watch has no release \"{version}\" (it may have aged out of the history)."
                ))
            })
    }

    /// Records that an action on a release failed (a command that could not run, a folder that could not be opened);
    /// the shell runs the actions, the journal shows the failure.
    pub fn record_action_failure(&self, id: &str, action: &str, message: &str) -> Result<()> {
        use spotter_core::journal::EventKind;
        let wid = self.watch_id(id)?;
        self.record(
            self.now(),
            Some(wid),
            EventKind::ActionFailed {
                action: action.into(),
                message: message.into(),
            },
        )
    }

    // ---- dashboard and events ----

    /// The dashboard for a range (`30d`, `90d`, `1y`).
    pub fn dashboard(&self, range: &str) -> Result<DashboardView> {
        let r = Range::parse(range).ok_or_else(|| EngineError::BadRequest(format!("\"{range}\" is not a range (30d, 90d, 1y).")))?;
        let started = Instant::now();
        let journal = self.inner.db.load_journal()?;
        let specs = lock(&self.inner.doc).watches.clone();
        let states = lock(&self.inner.states).clone();
        let ids: Vec<WatchId> = specs.iter().filter_map(|s| WatchId::parse(&s.id).ok()).collect();
        let empty = WatchState::default();
        let inputs: Vec<WatchInput<'_>> = ids
            .iter()
            .map(|id| WatchInput {
                id,
                state: states.get(id).unwrap_or(&empty),
            })
            .collect();
        let d = dashboard::compute(&inputs, &journal, r, self.now(), 12);
        let info = |id: &WatchId| {
            specs
                .iter()
                .find(|s| s.id == id.as_str())
                .map(|s| (s.name.clone(), s.source.kind.clone()))
        };
        let whats_new = d
            .whats_new
            .iter()
            .map(|n| {
                let (watch_name, source_kind) = info(&n.watch).unwrap_or_default();
                WhatsNewView {
                    watch_id: n.watch.to_string(),
                    watch_name,
                    source_kind,
                    release: ReleaseView::from(&spotter_core::state::KnownRelease {
                        release: n.release.clone(),
                        first_seen: n.released_at,
                    }),
                    released_at_ms: n.released_at.0,
                    newer_than_installed: n.newer_than_installed,
                    unseen: n.unseen,
                }
            })
            .collect();
        Ok(DashboardView {
            range: range.to_string(),
            buckets_ms: d.buckets.iter().map(|t| t.0).collect(),
            whats_new,
            watches: d.watches,
            downloads: d.downloads,
            compute_micros: started.elapsed().as_micros() as u64,
        })
    }

    /// The events feed, newest first; `before_ms` pages back.
    pub fn events(&self, limit: usize, before_ms: Option<i64>) -> Result<Vec<EventView>> {
        let specs = lock(&self.inner.doc).watches.clone();
        let events = self.inner.db.events(limit.clamp(1, 500), before_ms.map(Timestamp))?;
        Ok(events
            .iter()
            .map(|e| EventView::of(e, |id| specs.iter().find(|s| s.id == id).map(|s| s.name.clone())))
            .collect())
    }

    // ---- settings ----

    /// The settings.
    pub fn settings(&self) -> Settings {
        lock(&self.inner.doc).settings.clone()
    }

    /// Saves the settings.
    pub fn save_settings(&self, mut settings: Settings) -> Result<Settings> {
        settings.default_interval_secs = spotter_core::schedule::clamp_interval(settings.default_interval_secs);
        if let Some(f) = &settings.downloads_folder {
            if f.trim().is_empty() || f.contains('\0') {
                settings.downloads_folder = None;
            }
        }
        lock(&self.inner.doc).settings = settings.clone();
        self.save_config()?;
        self.emit(Change::Settings);
        Ok(settings)
    }

    /// Pauses notifications for `minutes` (`None` resumes).
    pub fn pause_notifications(&self, minutes: Option<u32>) -> Result<Settings> {
        let until = minutes.map(|m| self.now().plus_secs(u64::from(m.min(60 * 24 * 30)) * 60).0);
        lock(&self.inner.doc).settings.notifications_paused_until = until;
        self.save_config()?;
        self.emit(Change::Settings);
        Ok(self.settings())
    }

    /// Whether notifications wait right now (global quiet hours).
    pub(crate) fn in_global_quiet(&self, now: Timestamp) -> bool {
        let (_, local) = local_parts(now, self.inner.svc.clock.utc_offset_minutes());
        self.settings().quiet_hours.is_some_and(|q| q.contains(local))
    }

    /// The source config of a watch (used by tests and the shell for "Test").
    pub fn source_of(&self, id: &str) -> Result<SourceConfig> {
        Ok(self.find_spec(id)?.source)
    }
}

fn load_user_packs(dir: &std::path::Path, pack: &mut Pack) -> Vec<String> {
    let mut warnings = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return warnings };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    files.sort();
    for path in files.into_iter().take(20) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| Pack::parse(&t))
        {
            Ok((p, w)) => {
                warnings.extend(w.into_iter().map(|m| format!("Pack {name}: {m}")));
                pack.merge(p);
            }
            Err(e) => warnings.push(format!("Pack {name} was skipped: {e}")),
        }
    }
    warnings
}

fn blank_spec() -> WatchSpec {
    WatchSpec {
        id: String::new(),
        name: String::new(),
        source: SourceConfig::new("", &[]),
        versions: String::new(),
        version_pattern: None,
        include_prereleases: false,
        name_filter: String::new(),
        schedule: Default::default(),
        notify: Default::default(),
        actions: Vec::new(),
        channels: Vec::new(),
        version_filter: Default::default(),
        allow_local: false,
        paused: false,
        tracking: Default::default(),
    }
}

/// The products of a watch as the detail page shows them.
fn product_views(spec: &WatchSpec, state: &WatchState) -> Vec<ProductView> {
    let settings = &spec.tracking.products;
    state
        .products
        .iter()
        .map(|(id, p)| ProductView {
            id: id.clone(),
            name: settings.display_name(id).to_string(),
            muted: settings.is_muted(id),
            latest: p.latest.as_ref().map(ReleaseView::from),
            unseen: p.has_unseen(),
            history: p.history.iter().map(ReleaseView::from).collect(),
        })
        .collect()
}
