//! Model update checks (ADR 0017): asks Hugging Face, through [`crate::egress`] only, whether the models you
//! downloaded have a newer revision or a better quantisation, and keeps the answers. It never downloads anything;
//! the rules are `pitwall_core::updates`, the schedule and the notice state `pitwall_core::update_state`.
//!
//! What a check sends: the file-list and info URLs of the repository ids you downloaded from (`GET
//! /api/models/<repo>/tree/main` and `/api/models/<repo>`), nothing else; no request body, no model names beyond the
//! repository id, no usage. It is bounded ([`MAX_REPOS`] repositories, a timeout per repository, a pause between
//! them, three failures in a row stop it), skipped entirely in offline mode, and a refusal from Hugging Face
//! (HTTP 429) ends it at once.

use crate::app::AppState;
use crate::egress::{DownloadError, Egress};
use crate::paths::now_ms;
use crate::paths::{self, Dirs};
use futures_util::future::BoxFuture;
use pitwall_core::events::{EventKind, Severity};
use pitwall_core::fit::Room;
use pitwall_core::hf::{self, RepoId};
use pitwall_core::mgmt::UpdateCheckStatus;
use pitwall_core::mgmt::{UpdateCheckOutcome, UpdateCheckResult};
use pitwall_core::privacy::EgressError;
use pitwall_core::time::Millis;
use pitwall_core::update_state::UpdateCheckSettings;
use pitwall_core::update_state::{DismissError, Finished, UpdateState};
use pitwall_core::updates::{self, Basis, Installed, RepoSnapshot, UpdateNotice};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

/// Most repositories one check asks about; the rest wait for the next check (see `pitwall_core::updates::pick_repos`).
pub const MAX_REPOS: usize = 40;
/// How long one repository may take (both of its requests).
pub const REPO_TIMEOUT: Duration = Duration::from_secs(30);
/// The pause between two repositories, so a long list does not hit Hugging Face in a burst.
pub const PACE: Duration = Duration::from_millis(500);
/// Failures in a row that end a check.
pub const MAX_FAILURES_IN_A_ROW: usize = 3;
const MAX_JSON: usize = 8 * 1024 * 1024;

/// Where repository listings come from: Hugging Face in production, a fake in tests. Implementations ask only about
/// the repository they are given and must honour offline mode (an `Egress(Offline)` error before any socket opens).
pub trait RepoSource: Send + Sync {
    /// The repository's GGUF files and current commit.
    fn snapshot<'a>(&'a self, repo: &'a RepoId) -> BoxFuture<'a, Result<RepoSnapshot, DownloadError>>;
}

/// Hugging Face through the egress client.
pub struct HfSource {
    egress: Egress,
}

impl HfSource {
    /// A source over the egress client.
    pub fn new(egress: Egress) -> HfSource {
        HfSource { egress }
    }
}

impl RepoSource for HfSource {
    fn snapshot<'a>(&'a self, repo: &'a RepoId) -> BoxFuture<'a, Result<RepoSnapshot, DownloadError>> {
        Box::pin(async move {
            let tree = self.egress.get_json(&hf::tree_url(repo)?, MAX_JSON).await?;
            let files = hf::parse_tree(&tree).map_err(DownloadError::Network)?;
            // The commit is for the notice text only: a failure to read it is not a failure of the check.
            let revision = match self.egress.get_json(&hf::info_url(repo)?, MAX_JSON).await {
                Ok(v) => updates::parse_revision(&v),
                Err(e @ DownloadError::Egress(_)) => return Err(e),
                Err(_) => None,
            };
            Ok(RepoSnapshot { revision, files })
        })
    }
}

/// What a check needs from the app.
pub struct Request<'a> {
    /// Offline mode is on.
    pub offline: bool,
    /// The installed models (those without a repository are ignored).
    pub installed: Vec<Installed>,
    /// The context models are judged at.
    pub context: u32,
    /// The machine's room.
    pub room: &'a Room,
    /// The clock.
    pub now: Millis,
}

/// A finished check.
#[derive(Debug, Clone)]
pub struct Report {
    /// What the user is told.
    pub result: UpdateCheckResult,
    /// Notices not shown before this check (new findings, or a changed offer).
    pub fresh: Vec<UpdateNotice>,
}

fn quick(outcome: UpdateCheckOutcome, message: impl Into<String>) -> Report {
    Report {
        result: UpdateCheckResult {
            outcome,
            message: message.into(),
            repos_checked: 0,
            notices: 0,
            retry_in_secs: None,
        },
        fresh: vec![],
    }
}

struct Running<'a>(&'a AtomicBool);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// The checker and its saved state.
pub struct Updates {
    state: Mutex<UpdateState>,
    running: AtomicBool,
    dirs: Dirs,
    pace: Duration,
    timeout: Duration,
}

impl Updates {
    /// Opens the saved state in `dirs`. The flag says a file was there but unreadable (it starts empty).
    pub fn open(dirs: &Dirs) -> (Updates, bool) {
        let (p, b) = paths::read_with_backup(&dirs.model_updates(), pitwall_core::update_state::MAX_BYTES);
        let (state, damaged) = UpdateState::load(p.as_deref(), b.as_deref());
        let u = Updates {
            state: Mutex::new(state),
            running: AtomicBool::new(false),
            dirs: dirs.clone(),
            pace: PACE,
            timeout: REPO_TIMEOUT,
        };
        (u, damaged)
    }

    /// The pause between repositories (tests use none).
    pub fn with_pace(mut self, pace: Duration) -> Updates {
        self.pace = pace;
        self
    }

    /// How long one repository may take (tests use a short one).
    pub fn with_timeout(mut self, timeout: Duration) -> Updates {
        self.timeout = timeout;
        self
    }

    fn lock(&self) -> MutexGuard<'_, UpdateState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A check is running.
    pub fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Reads the state.
    pub fn read<R>(&self, f: impl FnOnce(&UpdateState) -> R) -> R {
        f(&self.lock())
    }

    /// Saves the state (atomic, with a backup). A failure is ignored: the next check rebuilds the notices.
    fn save(&self, st: &UpdateState) {
        let _ = paths::write_atomic(&self.dirs.model_updates(), st.to_json().as_bytes());
    }

    /// Hides a notice (see `UpdateState::dismiss`) and saves.
    pub fn dismiss(&self, id: &str, snooze_days: Option<u32>, now: Millis) -> Result<(), DismissError> {
        let mut st = self.lock();
        st.dismiss(id, snooze_days, now)?;
        self.save(&st);
        Ok(())
    }

    /// A download of `file` from `repo` finished: forget the notices that offered it.
    pub fn downloaded(&self, repo: &str, file: &str) {
        let mut st = self.lock();
        if st.downloaded(repo, file) > 0 {
            self.save(&st);
        }
    }

    /// Runs one check. Offline mode, a rate limit or a running check answer at once without a single request.
    pub async fn check(&self, source: &dyn RepoSource, req: Request<'_>) -> Report {
        if req.offline {
            return quick(
                UpdateCheckOutcome::Offline,
                "Offline mode is on, so Pitwall did not ask Hugging Face about your models. Turn offline mode off in Settings > Network to check.",
            );
        }
        if let Err(secs) = self.lock().manual_allowed(req.now) {
            let mut r = quick(
                UpdateCheckOutcome::RateLimited,
                format!("A check ran moments ago. Try again in {secs} seconds."),
            );
            r.result.retry_in_secs = Some(secs);
            return r;
        }
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return quick(UpdateCheckOutcome::Busy, "A check is already running.");
        }
        let _running = Running(&self.running);
        self.lock().attempted(req.now, None);

        let installed: Vec<&Installed> = req.installed.iter().collect();
        let all_ids: Vec<String> = installed.iter().map(|i| i.model_id.clone()).collect();
        if installed.is_empty() {
            let mut st = self.lock();
            let msg = "No model in your library came from a Hugging Face repository, so there is nothing to check.";
            st.finish(
                Finished {
                    at: req.now,
                    checked: &all_ids,
                    installed: &all_ids,
                    found: vec![],
                },
                msg,
            );
            self.save(&st);
            return quick(UpdateCheckOutcome::NothingToCheck, msg);
        }

        let (repos, left) = updates::pick_repos(
            installed.iter().map(|i| i.repo.as_str().to_string()).collect(),
            req.now / pitwall_core::time::DAY,
            MAX_REPOS,
        );
        let mut snapshots = std::collections::BTreeMap::new();
        let (mut failures, mut in_a_row, mut stopped): (usize, usize, Option<String>) = (0, 0, None);
        for (i, name) in repos.iter().enumerate() {
            if i > 0 && !self.pace.is_zero() {
                tokio::time::sleep(self.pace).await;
            }
            let Ok(id) = RepoId::parse(name) else { continue };
            match tokio::time::timeout(self.timeout, source.snapshot(&id)).await {
                Ok(Ok(s)) => {
                    in_a_row = 0;
                    snapshots.insert(name.clone(), s);
                }
                Ok(Err(DownloadError::Egress(EgressError::Offline))) => {
                    return quick(UpdateCheckOutcome::Offline, "Offline mode was turned on; the check stopped.");
                }
                Ok(Err(DownloadError::Status(429, _))) => {
                    stopped =
                        Some("Hugging Face asked Pitwall to slow down (HTTP 429); the check stopped and will be retried later.".into());
                    break;
                }
                Ok(Err(_)) | Err(_) => {
                    failures += 1;
                    in_a_row += 1;
                    if in_a_row >= MAX_FAILURES_IN_A_ROW {
                        stopped =
                            Some("Hugging Face did not answer several times in a row; the check stopped. Check the connection.".into());
                        break;
                    }
                }
            }
        }
        if snapshots.is_empty() {
            let msg = stopped.unwrap_or_else(|| "Hugging Face could not be reached. Check the connection and try again.".into());
            let mut st = self.lock();
            st.attempted(req.now, Some(&msg));
            self.save(&st);
            return quick(UpdateCheckOutcome::Failed, msg);
        }

        let basis = Basis {
            context: req.context,
            room: req.room,
            now: req.now,
        };
        let asked: Vec<Installed> = req
            .installed
            .iter()
            .filter(|i| snapshots.contains_key(i.repo.as_str()))
            .cloned()
            .collect();
        let found = updates::evaluate(&asked, &snapshots, &basis);
        let checked: Vec<String> = asked.iter().map(|i| i.model_id.clone()).collect();

        let mut st = self.lock();
        let before = st.visible(req.now);
        let mut msg = format!(
            "Asked Hugging Face about {} {}.",
            snapshots.len(),
            plural(snapshots.len(), "repository", "repositories")
        );
        if failures > 0 {
            msg.push_str(&format!(" {failures} could not be read."));
        }
        if left > 0 {
            msg.push_str(&format!(" {left} more will be asked at the next check."));
        }
        if let Some(s) = &stopped {
            msg.push(' ');
            msg.push_str(s);
        }
        st.finish(
            Finished {
                at: req.now,
                checked: &checked,
                installed: &all_ids,
                found,
            },
            &msg,
        );
        self.save(&st);
        let now_visible = st.visible(req.now);
        let fresh: Vec<UpdateNotice> = now_visible
            .iter()
            .filter(|n| !before.iter().any(|b| b.id == n.id && b.fingerprint() == n.fingerprint()))
            .cloned()
            .collect();
        Report {
            result: UpdateCheckResult {
                outcome: UpdateCheckOutcome::Checked,
                message: msg,
                repos_checked: snapshots.len() as u32,
                notices: now_visible.len() as u32,
                retry_in_secs: None,
            },
            fresh,
        }
    }
}

/// How often the schedule is looked at (a check itself runs at most weekly or daily).
const TICK: Duration = Duration::from_secs(5 * 60);
/// The wait after start-up before the first look, so a start never begins with network traffic.
const FIRST_TICK: Duration = Duration::from_secs(60);
/// Most events raised for one check (the rest are summarised in one).
const MAX_EVENTS: usize = 3;

impl AppState {
    /// Replaces where checks ask (tests use a fake Hugging Face).
    pub fn set_update_source(&self, source: Arc<dyn RepoSource>) {
        if let Ok(mut g) = self.update_source.write() {
            *g = source;
        }
    }

    fn update_source(&self) -> Arc<dyn RepoSource> {
        self.update_source
            .read()
            .map(|g| g.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    /// Runs a check now and raises one event per new finding. Never downloads anything.
    pub async fn run_update_check(self: &Arc<Self>) -> Report {
        let cfg = self.cfg();
        let offline = self.offline.load(Ordering::SeqCst);
        // Offline mode is answered before the library is even scanned.
        let installed = if offline {
            vec![]
        } else {
            let lib = self.library.clone();
            tokio::task::spawn_blocking(move || lib.installed()).await.unwrap_or_default()
        };
        let room = crate::system::room_from(&self.serving.memory_now(), &self.hardware, self.serving.hot_ram().0);
        let report = self
            .updates
            .check(
                self.update_source().as_ref(),
                Request {
                    offline,
                    installed,
                    context: cfg.models.default_context,
                    room: &room,
                    now: now_ms(),
                },
            )
            .await;
        for n in report.fresh.iter().take(MAX_EVENTS) {
            self.event(EventKind::ModelUpdate, Severity::Info, format!("{}: {}", n.model_id, n.summary));
        }
        if report.fresh.len() > MAX_EVENTS {
            self.event(
                EventKind::ModelUpdate,
                Severity::Info,
                format!("{} more model updates are available.", report.fresh.len() - MAX_EVENTS),
            );
        }
        report
    }

    /// One look at the schedule: runs a check when one is due and allowed. Returns it when it ran.
    pub async fn scheduled_update_tick(self: &Arc<Self>) -> Option<Report> {
        let settings = self.cfg().models.update_check;
        let due = !self.offline.load(Ordering::SeqCst) && self.updates.read(|s| s.due(&settings, now_ms()));
        if due {
            Some(self.run_update_check().await)
        } else {
            None
        }
    }

    /// Starts the schedule loop. The first look is a minute after start-up; nothing is asked unless the user turned
    /// the check on.
    pub fn start_update_schedule(self: &Arc<Self>) {
        let me = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(FIRST_TICK).await;
            loop {
                let _ = me.scheduled_update_tick().await;
                tokio::time::sleep(TICK).await;
            }
        });
    }

    /// The state of the checks for the UI.
    pub fn update_status(&self) -> UpdateCheckStatus {
        let settings: UpdateCheckSettings = self.cfg().models.update_check;
        let offline = self.offline.load(Ordering::SeqCst);
        let now = now_ms();
        self.updates.read(|s| UpdateCheckStatus {
            settings,
            last_checked_at: s.last_checked_at,
            last_attempt_at: s.last_attempt_at,
            last_message: s.last_message.clone(),
            next_check_at: if offline { None } else { s.next_due(&settings, now) },
            offline,
            running: self.updates.running(),
            notices: s.visible(now).len() as u32,
            host: "huggingface.co".into(),
            max_repos: MAX_REPOS as u32,
        })
    }

    /// The notices to show now.
    pub fn update_notices(&self) -> Vec<UpdateNotice> {
        let now = now_ms();
        self.updates.read(|s| s.visible(now))
    }
}

fn plural<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 {
        one
    } else {
        many
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! A fake Hugging Face: canned answers per repository, every request counted.
    use super::*;
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::atomic::AtomicUsize;

    pub enum Answer {
        Repo(RepoSnapshot),
        Fail(fn() -> DownloadError),
    }

    #[derive(Default)]
    pub struct FakeHf {
        pub repos: Mutex<BTreeMap<String, Answer>>,
        pub calls: AtomicUsize,
        pub asked: Mutex<VecDeque<String>>,
    }

    impl FakeHf {
        pub fn with(repo: &str, snap: RepoSnapshot) -> FakeHf {
            let f = FakeHf::default();
            f.repos.lock().unwrap().insert(repo.into(), Answer::Repo(snap));
            f
        }
        pub fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl RepoSource for FakeHf {
        fn snapshot<'a>(&'a self, repo: &'a RepoId) -> BoxFuture<'a, Result<RepoSnapshot, DownloadError>> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.asked.lock().unwrap().push_back(repo.as_str().to_string());
                match self.repos.lock().unwrap().get(repo.as_str()) {
                    Some(Answer::Repo(s)) => Ok(s.clone()),
                    Some(Answer::Fail(f)) => Err(f()),
                    None => Err(DownloadError::Status(404, ": the file was not found")),
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;
    use pitwall_core::hf::{RepoFiles, Variant};
    use pitwall_core::GIB;

    fn room() -> Room {
        Room {
            ram_total: 64 * GIB,
            ram_available: 48 * GIB,
            ram_releasable: 0,
            vram_free: None,
            unified: false,
        }
    }

    fn var(file: &str, size: u64, sha: char) -> Variant {
        Variant {
            file: file.into(),
            parts: vec![file.into()],
            quant: pitwall_core::quant::label_from_file_name(file),
            size,
            sha256: vec![sha.to_string().repeat(64)],
        }
    }

    fn snap(vs: Vec<Variant>) -> RepoSnapshot {
        RepoSnapshot {
            revision: None,
            files: RepoFiles {
                variants: vs,
                projectors: vec![],
            },
        }
    }

    fn have(repo: &str, file: &str, size: u64, sha: Option<char>) -> Installed {
        Installed {
            model_id: format!("{}-{}", repo.replace('/', "-"), file.trim_end_matches(".gguf")).to_ascii_lowercase(),
            repo: RepoId::parse(repo).unwrap(),
            file: file.into(),
            quant: pitwall_core::quant::label_from_file_name(file),
            size,
            sha256: sha.map(|c| vec![c.to_string().repeat(64)]).unwrap_or_default(),
            revision: None,
        }
    }

    fn updates(dir: &std::path::Path) -> Updates {
        Updates::open(&Dirs::at(dir)).0.with_pace(Duration::ZERO)
    }

    fn req<'a>(room: &'a Room, installed: Vec<Installed>, now: Millis, offline: bool) -> Request<'a> {
        Request {
            offline,
            installed,
            context: 8192,
            room,
            now,
        }
    }

    #[tokio::test]
    async fn the_real_source_refuses_in_offline_mode_before_any_socket() {
        let e = Egress::new(std::sync::Arc::new(AtomicBool::new(true)));
        let r = HfSource::new(e).snapshot(&RepoId::parse("a/b").unwrap()).await;
        assert!(matches!(r, Err(DownloadError::Egress(EgressError::Offline))));
    }

    #[tokio::test]
    async fn offline_mode_makes_no_request_at_all() {
        let d = tempfile::tempdir().unwrap();
        let u = updates(d.path());
        let fake = FakeHf::with("acme/Tiny-GGUF", snap(vec![]));
        let r = room();
        let rep = u
            .check(
                &fake,
                req(&r, vec![have("acme/Tiny-GGUF", "t-Q4_K_M.gguf", 1, None)], 1_000_000, true),
            )
            .await;
        assert_eq!(rep.result.outcome, UpdateCheckOutcome::Offline);
        assert!(rep.result.message.contains("Offline mode is on"));
        assert_eq!(fake.calls(), 0, "zero requests while offline");
        assert!(
            u.read(|s| s.last_attempt_at.is_none() && s.last_checked_at.is_none()),
            "an offline refusal is not an attempt"
        );
        assert!(!d.path().join("model-updates.json").exists());
    }

    #[tokio::test]
    async fn a_check_finds_a_newer_file_and_a_better_quantisation_and_saves_them() {
        let d = tempfile::tempdir().unwrap();
        let u = updates(d.path());
        let r = room();
        let fake = FakeHf::with(
            "acme/Tiny-GGUF",
            snap(vec![var("Tiny-Q4_K_M.gguf", 4 * GIB, 'b'), var("Tiny-Q5_K_M.gguf", 5 * GIB, 'c')]),
        );
        let rep = u
            .check(
                &fake,
                req(
                    &r,
                    vec![have("acme/Tiny-GGUF", "Tiny-Q4_K_M.gguf", 4 * GIB, Some('a'))],
                    5_000_000,
                    false,
                ),
            )
            .await;
        assert_eq!(
            (rep.result.outcome, rep.result.repos_checked, rep.result.notices),
            (UpdateCheckOutcome::Checked, 1, 2)
        );
        assert_eq!(rep.fresh.len(), 2);
        assert_eq!(fake.calls(), 1, "one request per repository");
        assert_eq!(
            fake.asked.lock().unwrap().iter().collect::<Vec<_>>(),
            vec!["acme/Tiny-GGUF"],
            "only the repository id is asked"
        );
        // Saved: a restart sees the same notices and the check times.
        let (again, damaged) = Updates::open(&Dirs::at(d.path()));
        assert!(!damaged);
        assert_eq!(again.read(|s| (s.all().len(), s.last_checked_at)), (2, Some(5_000_000)));
        // The same findings are not "fresh" the next time, and dismissals stick.
        let id = rep.fresh[0].id.clone();
        u.dismiss(&id, None, 5_100_000).unwrap();
        let rep2 = u
            .check(
                &fake,
                req(
                    &r,
                    vec![have("acme/Tiny-GGUF", "Tiny-Q4_K_M.gguf", 4 * GIB, Some('a'))],
                    6_000_000,
                    false,
                ),
            )
            .await;
        assert!(rep2.fresh.is_empty());
        assert_eq!(rep2.result.notices, 1, "one is dismissed");
    }

    #[tokio::test]
    async fn checks_are_rate_limited_and_never_run_twice_at_once() {
        let d = tempfile::tempdir().unwrap();
        let u = updates(d.path());
        let r = room();
        let fake = FakeHf::with("acme/Tiny-GGUF", snap(vec![]));
        let m = || vec![have("acme/Tiny-GGUF", "t-Q4_K_M.gguf", 1, None)];
        assert_eq!(
            u.check(&fake, req(&r, m(), 1_000_000, false)).await.result.outcome,
            UpdateCheckOutcome::Checked
        );
        let again = u.check(&fake, req(&r, m(), 1_020_000, false)).await;
        assert_eq!(
            (again.result.outcome, again.result.retry_in_secs),
            (UpdateCheckOutcome::RateLimited, Some(40))
        );
        assert_eq!(fake.calls(), 1, "the refused check asked nothing");
        // A check that is still running turns the next one away.
        u.running.store(true, Ordering::SeqCst);
        let busy = u.check(&fake, req(&r, m(), 9_000_000, false)).await;
        assert_eq!(busy.result.outcome, UpdateCheckOutcome::Busy);
        u.running.store(false, Ordering::SeqCst);
        assert!(!u.running());
        assert_eq!(
            u.check(&fake, req(&r, m(), 9_000_000, false)).await.result.outcome,
            UpdateCheckOutcome::Checked
        );
    }

    #[tokio::test]
    async fn nothing_to_check_clears_old_notices() {
        let d = tempfile::tempdir().unwrap();
        let u = updates(d.path());
        let r = room();
        let fake = FakeHf::default();
        let rep = u.check(&fake, req(&r, vec![], 1_000_000, false)).await;
        assert_eq!(rep.result.outcome, UpdateCheckOutcome::NothingToCheck);
        assert_eq!(fake.calls(), 0);
        assert!(u.read(|s| s.last_checked_at.is_some()));
    }

    #[tokio::test]
    async fn failures_are_contained_and_reported() {
        let d = tempfile::tempdir().unwrap();
        let u = updates(d.path());
        let r = room();
        let fake = FakeHf::with("good/Repo", snap(vec![var("g-Q4_K_M.gguf", 100, 'b')]));
        fake.repos
            .lock()
            .unwrap()
            .insert("bad/Repo".into(), Answer::Fail(|| DownloadError::Network("down".into())));
        let installed = vec![
            have("good/Repo", "g-Q4_K_M.gguf", 100, Some('a')),
            have("bad/Repo", "b-Q4_K_M.gguf", 100, Some('a')),
        ];
        let rep = u.check(&fake, req(&r, installed.clone(), 1_000_000, false)).await;
        assert_eq!(
            (rep.result.outcome, rep.result.repos_checked, rep.result.notices),
            (UpdateCheckOutcome::Checked, 1, 1)
        );
        assert!(rep.result.message.contains("1 could not be read"), "{}", rep.result.message);
        // Everything failing is a failed attempt, remembered, and the old notices stay.
        let all_bad = FakeHf::default();
        let rep = u.check(&all_bad, req(&r, installed, 2_000_000, false)).await;
        assert_eq!(rep.result.outcome, UpdateCheckOutcome::Failed);
        assert!(u.read(|s| s.last_message.clone().unwrap().contains("could not be reached")));
        assert_eq!(
            u.read(|s| (s.all().len(), s.last_checked_at, s.last_attempt_at)),
            (1, Some(1_000_000), Some(2_000_000))
        );
    }

    #[tokio::test]
    async fn hugging_face_saying_slow_down_stops_the_check() {
        let d = tempfile::tempdir().unwrap();
        let u = updates(d.path());
        let r = room();
        let fake = FakeHf::default();
        fake.repos.lock().unwrap().insert("a/One".into(), Answer::Repo(snap(vec![])));
        fake.repos.lock().unwrap().insert(
            "b/Two".into(),
            Answer::Fail(|| DownloadError::Status(429, ": rate limited, retry later")),
        );
        fake.repos.lock().unwrap().insert("c/Three".into(), Answer::Repo(snap(vec![])));
        let installed: Vec<Installed> = ["a/One", "b/Two", "c/Three"]
            .iter()
            .map(|r| have(r, "m-Q4_K_M.gguf", 1, None))
            .collect();
        let rep = u.check(&fake, req(&r, installed, 1_000_000, false)).await;
        assert_eq!(fake.calls(), 2, "nothing is asked after the 429");
        assert_eq!((rep.result.outcome, rep.result.repos_checked), (UpdateCheckOutcome::Checked, 1));
        assert!(rep.result.message.contains("slow down"));
    }

    #[tokio::test]
    async fn three_failures_in_a_row_end_the_check_and_the_window_is_bounded() {
        let d = tempfile::tempdir().unwrap();
        let u = updates(d.path());
        let r = room();
        let fake = FakeHf::default(); // every repository answers 404
        let installed: Vec<Installed> = (0..MAX_REPOS + 5)
            .map(|i| have(&format!("o/R{i:02}"), "m-Q4_K_M.gguf", 1, None))
            .collect();
        let rep = u.check(&fake, req(&r, installed, 1_000_000, false)).await;
        assert_eq!(rep.result.outcome, UpdateCheckOutcome::Failed);
        assert_eq!(fake.calls(), MAX_FAILURES_IN_A_ROW);
        // With answers, at most MAX_REPOS repositories are asked and the message says how many are left.
        let ok = FakeHf::default();
        let installed: Vec<Installed> = (0..MAX_REPOS + 5)
            .map(|i| have(&format!("o/R{i:02}"), "m-Q4_K_M.gguf", 1, None))
            .collect();
        for i in &installed {
            ok.repos.lock().unwrap().insert(i.repo.as_str().into(), Answer::Repo(snap(vec![])));
        }
        let rep = u.check(&ok, req(&r, installed, 9_000_000, false)).await;
        assert_eq!(ok.calls(), MAX_REPOS);
        assert!(
            rep.result.message.contains("5 more will be asked at the next check"),
            "{}",
            rep.result.message
        );
    }

    #[tokio::test]
    async fn a_check_that_finds_offline_mode_mid_way_stops_quietly() {
        let d = tempfile::tempdir().unwrap();
        let u = updates(d.path());
        let r = room();
        let fake = FakeHf::default();
        fake.repos
            .lock()
            .unwrap()
            .insert("a/One".into(), Answer::Fail(|| DownloadError::Egress(EgressError::Offline)));
        let rep = u
            .check(&fake, req(&r, vec![have("a/One", "m-Q4_K_M.gguf", 1, None)], 1_000_000, false))
            .await;
        assert_eq!(rep.result.outcome, UpdateCheckOutcome::Offline);
        assert!(u.read(|s| s.last_checked_at.is_none()));
    }

    #[tokio::test]
    async fn a_slow_repository_times_out_as_a_failure() {
        struct Hang;
        impl RepoSource for Hang {
            fn snapshot<'a>(&'a self, _: &'a RepoId) -> BoxFuture<'a, Result<RepoSnapshot, DownloadError>> {
                Box::pin(std::future::pending())
            }
        }
        let d = tempfile::tempdir().unwrap();
        let u = updates(d.path()).with_timeout(Duration::from_millis(30));
        let r = room();
        let rep = u
            .check(&Hang, req(&r, vec![have("a/One", "m-Q4_K_M.gguf", 1, None)], 1_000_000, false))
            .await;
        assert_eq!(rep.result.outcome, UpdateCheckOutcome::Failed);
    }

    #[tokio::test]
    async fn a_download_clears_its_notice_and_a_damaged_state_file_starts_empty() {
        let d = tempfile::tempdir().unwrap();
        let u = updates(d.path());
        let r = room();
        let fake = FakeHf::with("acme/Tiny-GGUF", snap(vec![var("Tiny-Q4_K_M.gguf", 4 * GIB, 'b')]));
        u.check(
            &fake,
            req(
                &r,
                vec![have("acme/Tiny-GGUF", "Tiny-Q4_K_M.gguf", 4 * GIB, Some('a'))],
                1_000_000,
                false,
            ),
        )
        .await;
        assert_eq!(u.read(|s| s.all().len()), 1);
        u.downloaded("acme/Tiny-GGUF", "Tiny-Q4_K_M.gguf");
        assert_eq!(u.read(|s| s.all().len()), 0);
        u.downloaded("acme/Tiny-GGUF", "Tiny-Q4_K_M.gguf");
        assert!(u.dismiss("nope", None, 1).is_err());
        // Damage: both files unreadable, then only the primary.
        std::fs::write(d.path().join("model-updates.json"), b"{{{").unwrap();
        std::fs::write(d.path().join("model-updates.json.bak"), b"also bad").unwrap();
        let (fresh, damaged) = Updates::open(&Dirs::at(d.path()));
        assert!(damaged && fresh.read(|s| s.all().is_empty()));
    }
}
