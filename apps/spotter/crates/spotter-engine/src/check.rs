//! One check of one watch, end to end: plan -> fetch -> select -> compare with the state -> soak -> events ->
//! notification decisions. Also "check now", what is due, and the digest.
use crate::engine::{lock, unless_cancelled, Change, Engine};
use crate::error::{EngineError, Result};
use crate::fetch::Validators;
use crate::notifier::{self, ActionRequest, Notification, NotificationKind};
use crate::pipeline::{Gathered, PageBaseline};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spotter_core::filter;
use spotter_core::job::{Job, Phase, Unit};
use spotter_core::journal::{Event, EventKind};
use spotter_core::limits::MAX_HISTORY;
use spotter_core::notify::{digest_due, ChannelKind, DigestMode};
use spotter_core::release::{select, Release};
use spotter_core::source::folder::Snapshot;
use spotter_core::source::SourceError;
use spotter_core::state::MAX_PRODUCT_HISTORY;
use spotter_core::state::{CheckContext, Effects, WatchState, WatchStatus};
use spotter_core::time::Timestamp;
use spotter_core::watch::{Watch, WatchId, WatchSpec};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// What a check did.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckReport {
    /// The watch.
    pub watch_id: String,
    /// Its status afterwards.
    pub status: WatchStatus,
    /// New releases found (empty on the first check, which sets the baseline).
    pub new_versions: Vec<String>,
    /// The check did not run because the watch is paused, was deleted or is already being checked.
    pub skipped: bool,
}

#[derive(Serialize, Deserialize)]
struct DigestItem {
    watch_id: String,
    watch_name: String,
    version: String,
    name: String,
    location: String,
}

/// Hash of everything that decides what a fetch returns; a changed source starts without validators.
fn fingerprint(spec: &WatchSpec) -> String {
    let text = format!("{:?}|{}|{}", spec.source, spec.allow_local, spec.version_filter.prefix);
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// What a notification is about: the watch, and for a product its name (`Acme Installer · PRO`).
pub(crate) fn subject_of(watch: &Watch, release: &Release) -> String {
    match release.product() {
        None => watch.name.clone(),
        Some(id) => {
            let shown = watch.tracking.products.display_name(id);
            if shown.starts_with(&watch.name) {
                shown.to_string()
            } else {
                format!("{} · {shown}", watch.name)
            }
        }
    }
}

struct Running {
    engine: Engine,
    id: WatchId,
}

impl Drop for Running {
    fn drop(&mut self) {
        lock(&self.engine.inner.checking).remove(&self.id);
        self.engine.emit(Change::Watches);
    }
}

impl Engine {
    /// Checks one watch now (a paused watch too, when asked for by id). Returns when the check is done.
    pub async fn check_one(&self, id: &str) -> Result<CheckReport> {
        self.check_one_job(id, &Job::none()).await
    }

    /// [`Engine::check_one`] reporting to `job` (a folder's growing count, the settle wait) and stopped by it:
    /// a cancelled check ends with [`EngineError::Cancelled`] and leaves the watch's status as it was.
    pub async fn check_one_job(&self, id: &str, job: &Job) -> Result<CheckReport> {
        let spec = self.find_spec(id)?;
        let wid = self.watch_id(id)?;
        let skipped = |status| CheckReport {
            watch_id: id.to_string(),
            status,
            new_versions: Vec::new(),
            skipped: true,
        };
        if !lock(&self.inner.checking).insert(wid.clone()) {
            return Ok(skipped(self.status_of(&wid)));
        }
        let _running = Running {
            engine: self.clone(),
            id: wid.clone(),
        };
        self.emit(Change::Watches);
        job.report(Phase::Checking, Unit::Count, 0, None);
        unless_cancelled(job, self.run_check(&spec, &wid, job))
            .await
            .unwrap_or(Err(EngineError::Cancelled))
    }

    /// Checks one watch, or every watch that is not paused when `id` is `None`.
    pub async fn check_now(&self, id: Option<&str>) -> Result<Vec<CheckReport>> {
        self.check_now_job(id, &Job::none()).await
    }

    /// [`Engine::check_now`] reporting to `job` and stopped by it. Checking everything reports
    /// [`Phase::Checking`] as `done` of `total` watches; a cancelled run lets the checks that are running stop and
    /// starts no more, and ends with [`EngineError::Cancelled`].
    pub async fn check_now_job(&self, id: Option<&str>, job: &Job) -> Result<Vec<CheckReport>> {
        match id {
            Some(id) => Ok(vec![self.check_one_job(id, job).await?]),
            None => {
                let ids: Vec<String> = lock(&self.inner.doc)
                    .watches
                    .iter()
                    .filter(|w| !w.paused)
                    .map(|w| w.id.clone())
                    .collect();
                let reports = self.check_many_job(ids, job).await;
                if job.is_cancelled() {
                    return Err(EngineError::Cancelled);
                }
                Ok(reports)
            }
        }
    }

    /// Runs the checks with at most `max_concurrent` at once.
    pub(crate) async fn check_many(&self, ids: Vec<String>) -> Vec<CheckReport> {
        self.check_many_job(ids, &Job::none()).await
    }

    /// [`Engine::check_many`] with progress (`done` of `total` watches) and cancel.
    pub(crate) async fn check_many_job(&self, ids: Vec<String>, job: &Job) -> Vec<CheckReport> {
        let total = ids.len() as u64;
        job.report_done(Phase::Checking, Unit::Count, 0, Some(total));
        // The checks inside report nowhere: their own counts would fight over the one "n of m".
        let inner = job.muted();
        let gate = Arc::new(Semaphore::new(self.inner.cfg.max_concurrent.max(1)));
        let mut set = JoinSet::new();
        for id in ids {
            let engine = self.clone();
            let gate = gate.clone();
            let inner = inner.clone();
            set.spawn(async move {
                let _permit = gate.acquire_owned().await.ok();
                if inner.is_cancelled() {
                    return Err(EngineError::Cancelled);
                }
                engine.check_one_job(&id, &inner).await
            });
        }
        let (mut out, mut finished) = (Vec::new(), 0u64);
        while let Some(done) = set.join_next().await {
            finished += 1;
            job.report(Phase::Checking, Unit::Count, finished, Some(total));
            // A check that panicked is contained here: the other checks and the engine carry on.
            if let Ok(Ok(report)) = done {
                out.push(report);
            }
        }
        job.report_done(Phase::Checking, Unit::Count, finished, Some(total));
        out
    }

    fn status_of(&self, id: &WatchId) -> WatchStatus {
        lock(&self.inner.states).get(id).map(|s| s.status.clone()).unwrap_or_default()
    }

    /// The watches whose check is due at `now`: not paused, not running, never checked or `next_check` reached.
    pub fn due_watches(&self, now: Timestamp) -> Vec<String> {
        let specs = lock(&self.inner.doc).watches.clone();
        let states = lock(&self.inner.states);
        let running = lock(&self.inner.checking);
        specs
            .iter()
            .filter(|s| !s.paused)
            .filter_map(|s| WatchId::parse(&s.id).ok())
            .filter(|id| !running.contains(id))
            .filter(|id| states.get(id).and_then(|st| st.next_check).is_none_or(|t| t <= now))
            .map(|id| id.to_string())
            .collect()
    }

    async fn run_check(&self, spec: &WatchSpec, id: &WatchId, job: &Job) -> Result<CheckReport> {
        let now = self.now();
        let watch = match self.parse_spec(spec) {
            Ok(w) => w,
            Err(problems) => return self.apply_invalid(spec, id, problems.join(" "), now),
        };
        let secrets = self.secrets_for(&spec.id, &watch.source);
        let fp = fingerprint(spec);
        let has_history = lock(&self.inner.states).get(id).is_some_and(|s| s.latest.is_some());
        let validators: Option<Validators> = if has_history {
            self.inner.db.cached_validators(id, &fp)?
        } else {
            None
        };
        let page_key = format!("page:{id}");
        let baseline: Option<PageBaseline> = self.inner.db.kv_get(&page_key)?.and_then(|t| serde_json::from_str(&t).ok());
        let folder_key = format!("folder:{id}");
        let snapshot_fp = crate::pipeline::snapshot_fingerprint(&watch);
        let snapshot: Option<Snapshot> = self
            .inner
            .db
            .kv_get(&folder_key)?
            .and_then(|t| Snapshot::parse(&t))
            .filter(|s| s.fingerprint == snapshot_fp);
        let started = Instant::now();
        let gathered = self
            .inner
            .checker
            .gather_job(&watch, &secrets, validators, baseline.as_ref(), snapshot.as_ref(), now, job)
            .await;
        if job.is_cancelled() {
            // Stopped by the user: not a failure of the watch, so nothing is recorded.
            return Err(EngineError::Cancelled);
        }
        let took_ms = started.elapsed().as_millis() as u64;
        if let Ok(Gathered::Fresh { folder: Some(found), .. }) = &gathered {
            let mut snap = found.0.clone();
            snap.fingerprint = snapshot_fp;
            self.inner.db.kv_set(&folder_key, &snap.render())?;
        }
        if let Ok(Gathered::Fresh { validators, baseline, .. }) = &gathered {
            self.inner.db.store_validators(id, &fp, validators)?;
            if let Some(b) = baseline {
                // A snapshot is plain data and always serialises; the Option keeps this path free of a dead branch.
                serde_json::to_string(b)
                    .ok()
                    .map(|json| self.inner.db.kv_set(&page_key, &json))
                    .transpose()?;
            }
        }
        let settings = self.settings();
        let cx = CheckContext {
            now,
            local_time: crate::clock::local_parts(now, self.inner.svc.clock.utc_offset_minutes()).1,
            notifications_active: settings.notifications_active(now.0) && !self.in_global_quiet(now),
            took_ms,
        };
        let (fx, snapshot, ok, error) = {
            let mut states = lock(&self.inner.states);
            let state = states.entry(id.clone()).or_default();
            let (fx, ok, error) = self.apply(&watch, state, gathered, cx);
            state.next_check = Some(self.next_check_after(spec, now, state.failures));
            (fx, state.clone(), ok, error)
        };
        self.inner.db.save_state(id, &snapshot)?;
        self.inner.db.count_check(id, now, ok)?;
        self.after_check(&watch, &snapshot, fx.clone(), settings.digest, now, error).await?;
        Ok(CheckReport {
            watch_id: spec.id.clone(),
            status: snapshot.status.clone(),
            new_versions: fx.new_releases.iter().map(|r| r.version.text.clone()).collect(),
            skipped: false,
        })
    }

    /// Applies a check's result to the state (pure apart from the soak bookkeeping).
    fn apply(
        &self,
        watch: &Watch,
        state: &mut WatchState,
        gathered: std::result::Result<Gathered, SourceError>,
        cx: CheckContext,
    ) -> (Effects, bool, Option<SourceError>) {
        if watch.scheme.has_id() {
            return self.apply_products(watch, state, gathered, cx);
        }
        let releases: Vec<Release> = match gathered {
            Err(e) => return (state.on_failure(&e, cx), false, Some(e)),
            Ok(Gathered::Unchanged) => state.history.iter().map(|k| k.release.clone()).collect(),
            Ok(Gathered::Fresh { candidates, .. }) => {
                let selected = select(candidates, &watch.scheme, &watch.filter, MAX_HISTORY).releases;
                watch.version_filter.apply_prefix(selected)
            }
        };
        let first_check = state.latest.is_none() && state.history.is_empty();
        let history = state.history.clone();
        let soaked = filter::soak(releases, &watch.version_filter, &history, first_check, &mut state.soaking, cx.now);
        (state.on_success(soaked.ready, &watch.notify, cx), true, None)
    }

    /// A watch whose pattern has an `id` group: each product is selected, soaked and compared on its own.
    fn apply_products(
        &self,
        watch: &Watch,
        state: &mut WatchState,
        gathered: std::result::Result<Gathered, SourceError>,
        cx: CheckContext,
    ) -> (Effects, bool, Option<SourceError>) {
        let by_product: BTreeMap<String, Vec<Release>> = match gathered {
            Err(e) => return (state.on_failure(&e, cx), false, Some(e)),
            Ok(Gathered::Unchanged) => state
                .products
                .iter()
                .map(|(id, p)| (id.clone(), p.history.iter().map(|k| k.release.clone()).collect()))
                .collect(),
            Ok(Gathered::Fresh { candidates, .. }) => {
                let selected = select(candidates, &watch.scheme, &watch.filter, MAX_PRODUCT_HISTORY).releases;
                let mut by: BTreeMap<String, Vec<Release>> = BTreeMap::new();
                for r in watch.version_filter.apply_prefix(selected) {
                    if let Some(p) = r.detail.product.clone() {
                        by.entry(p).or_default().push(r);
                    }
                }
                by
            }
        };
        let fx = state.on_success_products(by_product, &watch.notify, &watch.tracking.products, &watch.version_filter, cx);
        (fx, true, None)
    }

    fn apply_invalid(&self, spec: &WatchSpec, id: &WatchId, message: String, now: Timestamp) -> Result<CheckReport> {
        let (changed, snapshot) = {
            let mut states = lock(&self.inner.states);
            let state = states.entry(id.clone()).or_default();
            let changed = state.set_invalid(message.clone());
            state.last_check = Some(now);
            state.next_check = Some(self.next_check_after(spec, now, 0));
            (changed, state.clone())
        };
        self.inner.db.save_state(id, &snapshot)?;
        if changed {
            self.record(now, Some(id.clone()), EventKind::CheckFailed { message })?;
        }
        Ok(CheckReport {
            watch_id: spec.id.clone(),
            status: snapshot.status,
            new_versions: Vec::new(),
            skipped: false,
        })
    }

    pub(crate) fn record(&self, at: Timestamp, watch: Option<WatchId>, kind: EventKind) -> Result<()> {
        self.inner.db.record_event(&Event { at, watch, kind })
    }

    async fn after_check(
        &self,
        watch: &Watch,
        state: &WatchState,
        fx: Effects,
        digest: DigestMode,
        now: Timestamp,
        error: Option<SourceError>,
    ) -> Result<()> {
        for r in fx.new_releases.iter().rev() {
            self.record(
                now,
                Some(watch.id.clone()),
                EventKind::NewRelease {
                    version: r.version.text.clone(),
                    name: r.name.clone(),
                },
            )?;
        }
        if fx.status_changed {
            match (&state.status, &fx.previous_status) {
                (WatchStatus::NeedsYou(m), _) => self.record(now, Some(watch.id.clone()), EventKind::NeedsYou { message: m.clone() })?,
                (s, _) if s.is_failure() => {
                    let message = error.as_ref().map_or_else(String::new, |e| e.message().to_string());
                    self.record(now, Some(watch.id.clone()), EventKind::CheckFailed { message })?;
                }
                (_, Some(prev)) if prev.is_failure() => self.record(now, Some(watch.id.clone()), EventKind::Recovered)?,
                _ => {}
            }
        }
        let settings = self.settings();
        let active = settings.notifications_active(now.0) && !self.in_global_quiet(now);
        if let (WatchStatus::NeedsYou(m), true, true) = (&state.status, fx.status_changed, active) {
            let n = Notification {
                kind: NotificationKind::NeedsYou,
                watch_id: Some(watch.id.to_string()),
                title: format!("{} needs you", watch.name),
                body: m.clone(),
                release: None,
                channels: watch.channels.clone(),
            };
            self.dispatch(n, watch.allow_local, now).await?;
        }
        for release in &fx.notify {
            let label = subject_of(watch, release);
            if digest == DigestMode::Off {
                let new_product = release.product().is_some_and(|p| fx.new_products.iter().any(|n| n == p));
                let n = Notification {
                    kind: NotificationKind::NewRelease,
                    watch_id: Some(watch.id.to_string()),
                    title: format!("{label} {}", release.version.text),
                    body: format!(
                        "{} {}: {}.",
                        if new_product {
                            "A new product appeared"
                        } else {
                            "A new release is out"
                        },
                        label,
                        release.name
                    ),
                    release: Some(release.clone()),
                    channels: watch.channels.clone(),
                };
                self.dispatch(n, watch.allow_local, now).await?;
            } else {
                let item = DigestItem {
                    watch_id: watch.id.to_string(),
                    watch_name: label,
                    version: release.version.text.clone(),
                    name: release.name.clone(),
                    location: release.location.clone(),
                };
                serde_json::to_string(&item)
                    .ok()
                    .map(|json| self.inner.db.queue_digest(&watch.id, now, &json))
                    .transpose()?;
            }
        }
        if !watch.actions.is_empty() {
            for release in fx.run_actions {
                self.inner.svc.notifier.run_actions(&ActionRequest {
                    watch_id: watch.id.to_string(),
                    watch_name: subject_of(watch, &release),
                    release,
                    actions: watch.actions.clone(),
                });
            }
        }
        self.emit(Change::Watches);
        Ok(())
    }

    /// Hands a notification to the shell and delivers its webhook and ntfy channels; delivery failures are events.
    pub(crate) async fn dispatch(&self, n: Notification, allow_local: bool, now: Timestamp) -> Result<()> {
        self.inner.svc.notifier.notify(&n);
        if n.channels.is_empty() {
            return Ok(());
        }
        let watch = n.watch_id.as_deref().and_then(|w| WatchId::parse(w).ok());
        let mail = n
            .channels
            .iter()
            .any(|c| c.kind == ChannelKind::Email)
            .then(|| self.mail_account())
            .flatten();
        for failure in notifier::deliver(&self.inner.fetcher, mail.as_ref(), &n, allow_local).await {
            self.record(
                now,
                watch.clone(),
                EventKind::ActionFailed {
                    action: "notify".into(),
                    message: failure,
                },
            )?;
        }
        Ok(())
    }

    /// Sends the digest when it is due (see [`digest_due`]) and notifications are not waiting. Returns whether
    /// one went out.
    pub async fn flush_digest(&self) -> Result<bool> {
        let settings = self.settings();
        let now = self.now();
        let (clock, _) = crate::clock::local_parts(now, self.inner.svc.clock.utc_offset_minutes());
        let last = self.inner.db.kv_get("digest:last_day")?.and_then(|t| t.parse::<i64>().ok());
        if !digest_due(settings.digest, last, clock) || !settings.notifications_active(now.0) || self.in_global_quiet(now) {
            return Ok(false);
        }
        self.inner.db.kv_set("digest:last_day", &clock.day.to_string())?;
        let items: Vec<DigestItem> = self
            .inner
            .db
            .take_digest()?
            .iter()
            .filter_map(|j| serde_json::from_str(j).ok())
            .collect();
        if items.is_empty() {
            return Ok(false);
        }
        let lines: Vec<String> = items.iter().map(|i| format!("{} {}", i.watch_name, i.version)).collect();
        let n = Notification {
            kind: NotificationKind::Digest,
            watch_id: None,
            title: if items.len() == 1 {
                "1 new release".into()
            } else {
                format!("{} new releases", items.len())
            },
            body: lines.join("\n"),
            release: None,
            channels: Vec::new(),
        };
        self.dispatch(n, false, now).await?;
        Ok(true)
    }
}
