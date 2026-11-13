//! The agent's side of the IP-list check and the map's data (ADR 0031 and `0031-amendments.md`): the user's lists, their
//! subscriptions, the allow list, the network switches, the matching index, the location databases and the user's own location.
//!
//! - **Storage.** User-made lists, the allow list and the switches are `intel-lists.json` in the data folder (versioned; a missing
//!   or corrupt file gives the defaults). A subscription's fetched text and the location databases are in the cache folder
//!   (`Dirs::intel_cache`); when each was last tried is `intel-fetch.json`, outside the cache.
//! - **Lookups** read an in-memory index swapped in whole (`Arc`): no database query per connection. The index is rebuilt at
//!   start and after an edit, as a job with progress and Cancel; a cancelled or failed build keeps the old index.
//! - **Network.** Every fetch goes through `paddock-net`'s `Egress` as a `paddock-download` job; the Egress objects are built
//!   only when a feature that needs one is on and used. There is no Offline mode; each feature has its own switch, on by default.
//! - **Logs** carry list ids, phases, counts, durations and failure kinds, never an address, entry, name or program.

use crate::geo_db::GeoReader;
use crate::geo_fetch::{self, GeoError, GeoFetcher, ASN, ATTRIBUTION, COUNTRY};
use crate::home_ip::HomeIp;
use crate::intel_fetch::{load_entries, FetchError, FetchState, FetchStore, Fetcher, Outcome};
use crate::paths::Dirs;
use dyno_core::geo::{summarise, GeoLookup, GeoRow, GeoSummary};
use dyno_core::intel::{AllowInput, IntelError, IntelIndex, ListInput};
use dyno_core::intel_lists::{
    apply_edit, parse_allow, EditError, EditOutcome, GeoStatus, IntelEdit, IntelStatus, ListSource, ListStatus, UserLists, MAX_TEXT_BYTES,
};
use dyno_core::intel_sub::Schedule;
use dyno_core::intel_text::parse_list;
use dyno_core::jobs::JobSpec;
use paddock_download::schedule::next_allowed;
use paddock_jobs::time::Clock;
use paddock_jobs::{JobContext, JobError, Phase};
use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Largest `intel-lists.json` read.
const MAX_FILE: u64 = (MAX_TEXT_BYTES as u64) * 4;
/// The job phase of the index build.
pub const INDEX_PHASE: &str = "index";

/// Reads the user's lists; a missing, oversized, corrupt or newer file gives the defaults.
pub fn load_lists(dirs: &Dirs) -> UserLists {
    let path = dirs.intel_lists();
    let Ok(meta) = std::fs::metadata(&path) else {
        return UserLists::default();
    };
    if meta.len() > MAX_FILE {
        log::warn!(target: "dyno::intel", "lists file ignored: too large");
        return UserLists::default();
    }
    match std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<UserLists>(&b).ok()) {
        Some(l) if l.version <= 1 => l,
        _ => {
            log::warn!(target: "dyno::intel", "lists file ignored: unreadable or from a newer version");
            UserLists::default()
        }
    }
}

fn save_lists(dirs: &Dirs, lists: &UserLists) -> std::io::Result<()> {
    let path = dirs.intel_lists();
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(lists).map_err(std::io::Error::other)?)?;
    crate::intel_fetch::replace(&tmp, &path)
}

/// What the home location says before any lookup ran.
const HOME_NOT_LOOKED_UP: &str = "not looked up yet";

/// The user's location from the OS region setting: the country of the locale. Nothing is sent anywhere.
fn region_home() -> HomeInfo {
    let env = |k: &str| std::env::var(k).ok();
    match crate::region::region_country(&dyno_platform::runner::SystemRunner, &env) {
        Some(c) => HomeInfo {
            country: Some(c),
            source: "Your location: from your region setting".into(),
        },
        None => HomeInfo {
            country: None,
            source: "Your location: unknown (no region setting found)".into(),
        },
    }
}

/// What the network objects are made of (tests pass fakes; the agent builds the real ones when first needed).
pub struct Net {
    /// List subscriptions.
    pub lists: Option<Arc<Fetcher>>,
    /// The location databases.
    pub geo: Option<Arc<GeoFetcher>>,
    /// The own-address lookup.
    pub home: Option<Arc<HomeIp>>,
}

/// The user's own location, as far as it is known.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HomeInfo {
    /// ISO country code.
    pub country: Option<String>,
    /// In words, for the page.
    pub source: String,
}

/// The lists, the index and the location data.
pub struct IntelCentre {
    dirs: Dirs,
    clock: Arc<dyn Clock>,
    lists: Mutex<UserLists>,
    fetch: Mutex<FetchStore>,
    index: RwLock<Arc<IntelIndex>>,
    index_error: Mutex<Option<String>>,
    geo: RwLock<Arc<GeoReader>>,
    home: Mutex<HomeInfo>,
    home_tried_ms: Mutex<Option<i64>>,
    net: Mutex<Net>,
}

impl IntelCentre {
    /// Reads the saved state; no network object exists yet and the index is empty until [`rebuild`](Self::rebuild) runs.
    pub fn new(dirs: Dirs, clock: Arc<dyn Clock>) -> IntelCentre {
        IntelCentre::with_net(
            dirs,
            clock,
            Net {
                lists: None,
                geo: None,
                home: None,
            },
        )
    }

    /// As [`new`](Self::new) with given network objects (tests).
    pub fn with_net(dirs: Dirs, clock: Arc<dyn Clock>, net: Net) -> IntelCentre {
        let now_secs = u64::try_from(clock.wall_ms() / 1000).unwrap_or(0);
        IntelCentre {
            lists: Mutex::new(load_lists(&dirs)),
            fetch: Mutex::new(FetchStore::load(&dirs)),
            geo: RwLock::new(Arc::new(geo_fetch::load_reader(&dirs, now_secs))),
            index: RwLock::new(Arc::new(IntelIndex::default())),
            index_error: Mutex::new(None),
            home: Mutex::new(HomeInfo {
                country: None,
                source: HOME_NOT_LOOKED_UP.into(),
            }),
            home_tried_ms: Mutex::new(None),
            net: Mutex::new(net),
            dirs,
            clock,
        }
    }

    fn real_lists(&self) -> Result<Arc<Fetcher>, FetchError> {
        let mut n = lock(&self.net);
        if let Some(f) = &n.lists {
            return Ok(f.clone());
        }
        let f = Arc::new(Fetcher::real(self.clock.clone())?);
        n.lists = Some(f.clone());
        Ok(f)
    }

    fn real_geo(&self) -> Result<Arc<GeoFetcher>, GeoError> {
        let mut n = lock(&self.net);
        if let Some(f) = &n.geo {
            return Ok(f.clone());
        }
        let f = Arc::new(GeoFetcher::real(self.clock.clone())?);
        n.geo = Some(f.clone());
        Ok(f)
    }

    /// The current index (cheap to clone).
    pub fn index(&self) -> Arc<IntelIndex> {
        self.index.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// The current location databases.
    pub fn geo(&self) -> Arc<GeoReader> {
        self.geo.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Applies a user's edit, saves, and removes the cached text of a removed list. The caller then starts a rebuild job when
    /// [`needs_rebuild`] says so.
    pub fn edit(&self, edit: IntelEdit) -> Result<EditOutcome, EditError> {
        let mut lists = lock(&self.lists);
        let mut next = lists.clone();
        let outcome = apply_edit(&mut next, edit)?;
        if let EditOutcome::Removed { id } = &outcome {
            let _ = std::fs::remove_dir_all(self.dirs.intel_cache(id));
            lock(&self.fetch).lists.remove(id);
            let _ = lock(&self.fetch).save(&self.dirs);
        }
        *lists = next;
        if let Err(e) = save_lists(&self.dirs, &lists) {
            log::warn!(target: "dyno::intel", "lists not saved: {}", e.kind());
        }
        Ok(outcome)
    }

    /// The text of what a subscription's fetch tells the page.
    fn state_of(&self, id: &str) -> FetchState {
        lock(&self.fetch).lists.get(id).cloned().unwrap_or_default()
    }

    /// Everything the lists page and the map's settings show.
    pub fn status(&self) -> IntelStatus {
        let lists = lock(&self.lists).clone();
        let now = self.clock.wall_ms();
        let index = self.index();
        let rows: Vec<ListStatus> = lists
            .lists
            .iter()
            .map(|l| {
                let (counts, next) = match &l.source {
                    ListSource::Url { .. } => {
                        let s = self.state_of(&l.id).status;
                        let next = next_allowed(s.last_attempt_ms, now, dyno_core::intel_sub::MIN_INTERVAL);
                        (s, next)
                    }
                    _ => {
                        let p = parse_list(&l.text, dyno_core::intel::MAX_TOTAL_ENTRIES);
                        (
                            // A typed or imported list shows its entries as typed: duplicates are only counted once the user
                            // chose "Remove duplicates" (the preview says what would go).
                            dyno_core::intel_sub::SubscriptionStatus {
                                entries: u32::try_from(p.entries.len() + p.duplicates as usize + p.covered as usize).unwrap_or(u32::MAX),
                                skipped: p.skipped_count,
                                ..Default::default()
                            },
                            None,
                        )
                    }
                };
                ListStatus {
                    id: l.id.clone(),
                    name: l.name.clone(),
                    enabled: l.enabled,
                    source: l.source.clone(),
                    counts,
                    next_allowed_ms: next,
                    text: (!matches!(l.source, ListSource::Url { .. }) && l.text.len() <= dyno_core::intel_lists::EDITABLE_TEXT_BYTES)
                        .then(|| l.text.clone()),
                }
            })
            .collect();
        let geo = self.geo();
        let store = lock(&self.fetch);
        let (note, terms) = IntelStatus::texts();
        IntelStatus {
            lists: rows,
            allow: lists.allow.clone(),
            index_segments: u32::try_from(index.segment_count()).unwrap_or(u32::MAX),
            index_ranges: u32::try_from(index.stored_ranges()).unwrap_or(u32::MAX),
            index_bytes: index.approx_bytes() as u64,
            index_error: lock(&self.index_error).clone(),
            geo: GeoStatus {
                switches: lists.switches,
                database_on: geo.is_on(),
                db_date: geo.built.map(|b| {
                    let (y, m, d) = geo_fetch::civil(b);
                    format!("{y:04}-{m:02}-{d:02}")
                }),
                country: store.lists.get(COUNTRY.id).map(|s| s.status.clone()).unwrap_or_default(),
                asn: store.lists.get(ASN.id).map(|s| s.status.clone()).unwrap_or_default(),
                home_source: self.home().source,
                attribution: ATTRIBUTION.to_string(),
            },
            first_run_note: note,
            terms_notice: terms,
        }
    }

    /// Builds the index from the enabled lists as the job `ctx`: phase `index` counts the entries taken. Cancel stops it and keeps
    /// the old index; a limit reached is reported in the status and keeps the old index too.
    pub fn rebuild(&self, ctx: &JobContext) -> Result<(), JobError> {
        let started = self.clock.monotonic();
        let lists = lock(&self.lists).clone();
        let mut inputs = Vec::with_capacity(lists.lists.len());
        for l in &lists.lists {
            let text_entries = match &l.source {
                ListSource::Url { .. } => load_entries(&self.dirs.intel_cache(&l.id)),
                _ => parse_list(&l.text, dyno_core::intel::MAX_TOTAL_ENTRIES).entries,
            };
            inputs.push(ListInput {
                id: l.id.clone(),
                name: l.name.clone(),
                enabled: l.enabled,
                entries: text_entries,
            });
        }
        let (addresses, hosts) = parse_allow(&lists.allow);
        let total: u64 = inputs.iter().map(|i| i.entries.len() as u64).sum();
        ctx.phase(Phase::items(INDEX_PHASE, "Building the match index", total.max(1)));
        let cancel = ctx.cancel_token().clone();
        let mut last = 0u64;
        let built = IntelIndex::build_with_progress(&inputs, &AllowInput { addresses, hosts }, &mut |_, n| {
            ctx.add(n.saturating_sub(last));
            last = n;
            !cancel.is_cancelled()
        });
        let ms = self.clock.monotonic().saturating_sub(started).as_millis();
        match built {
            Ok(ix) => {
                log::info!(target: "dyno::intel", "index built: lists={} entries={total} ranges={} duration_ms={ms}", inputs.len(), ix.stored_ranges());
                *self.index.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(ix);
                *lock(&self.index_error) = None;
                Ok(())
            }
            Err(IntelError::Cancelled) => Err(JobError::Cancelled(paddock_jobs::CancelReason::User)),
            Err(e) => {
                log::warn!(target: "dyno::intel", "index not built: {e}");
                *lock(&self.index_error) = Some(e.to_string());
                Err(JobError::Failed(e.to_string()))
            }
        }
    }

    /// Updates one subscription as the job `ctx` (download, check, swap), then rebuilds the index.
    pub fn update_list(&self, ctx: &JobContext, id: &str) -> Result<(), JobError> {
        let url = match lock(&self.lists).lists.iter().find(|l| l.id == id).map(|l| l.source.clone()) {
            Some(ListSource::Url { url, .. }) => url,
            _ => return Err(JobError::Failed("That is not a subscription.".into())),
        };
        let fetcher = self.real_lists().map_err(|e| JobError::Failed(e.to_string()))?;
        let mut state = self.state_of(id);
        let result = fetcher.update(id, &url, &mut state, &self.dirs.intel_cache(id), ctx);
        {
            let mut store = lock(&self.fetch);
            store.lists.insert(id.to_string(), state);
            let _ = store.save(&self.dirs);
        }
        match result {
            Ok(Outcome::Updated { .. }) => self.rebuild(ctx),
            Ok(Outcome::KeptPrevious { .. }) => Ok(()),
            Err(FetchError::Cancelled) => Err(JobError::Cancelled(paddock_jobs::CancelReason::User)),
            Err(e) => Err(JobError::Failed(e.to_string())),
        }
    }

    /// Updates the location databases (country, then ASN) as the job `ctx`, then loads them. A database that is already of this
    /// month, or not published yet, is not an error.
    pub fn update_geo(&self, ctx: &JobContext) -> Result<(), JobError> {
        let fetcher = self.real_geo().map_err(|e| JobError::Failed(e.to_string()))?;
        // The country database is the one the map needs; the ASN database only adds a network name. A failure of the optional ASN
        // database is logged and shown on the databases' own status line (naming it), and does not fail the job.
        let mut failure = None;
        for source in [COUNTRY, ASN] {
            let mut state = self.state_of(source.id);
            let r = fetcher.update(source, &mut state, &self.dirs.intel_cache(source.id), ctx);
            {
                let mut store = lock(&self.fetch);
                store.lists.insert(source.id.to_string(), state);
                let _ = store.save(&self.dirs);
            }
            match r {
                Ok(_) | Err(GeoError::NotPublished) => {}
                Err(GeoError::Fetch(FetchError::Cancelled)) => return Err(JobError::Cancelled(paddock_jobs::CancelReason::User)),
                Err(GeoError::Fetch(FetchError::TooSoon { .. })) => {}
                Err(e) if source.id == ASN.id => {
                    log::warn!(target: "dyno::geo", "optional database failed, job continues: id={} kind={}", source.id, e.kind())
                }
                Err(e) => failure = Some(format!("{}: {e}", source.name)),
            }
        }
        let now_secs = u64::try_from(self.clock.wall_ms() / 1000).unwrap_or(0);
        *self.geo.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(geo_fetch::load_reader(&self.dirs, now_secs));
        failure.map_or(Ok(()), |m| Err(JobError::Failed(m)))
    }

    /// Finds the user's own location: the public address (when the switch is on), located in the local database. Blocks; call it
    /// from a job. The address itself is not kept beyond this call.
    pub fn find_home(&self, ctx: &JobContext) {
        let on = lock(&self.lists).switches.own_location;
        let geo = self.geo();
        let info = if !on {
            // Switched off: the region setting is read locally and sent nowhere.
            let mut h = region_home();
            if h.country.is_none() {
                h.source = "Your location: not asked for (switched off in Settings), and no region setting found".into();
            }
            h
        } else {
            let home = {
                let mut n = lock(&self.net);
                if n.home.is_none() {
                    n.home = HomeIp::real(self.clock.clone()).ok().map(Arc::new);
                }
                n.home.clone()
            };
            match home.and_then(|h| h.find(ctx.cancel_token())) {
                Some(found) => match geo.lookup(&found.ip) {
                    Some(tag) => HomeInfo {
                        country: Some(tag.country),
                        source: format!("Your location: from your public IP (via {})", found.via.name()),
                    },
                    None => HomeInfo {
                        country: None,
                        source: "Your location: your public IP is not in the location database yet".into(),
                    },
                },
                None => region_home(),
            }
        };
        *lock(&self.home) = info;
    }

    /// The user's own location as last found. Until a lookup has run, the OS region setting is read (it needs no network).
    pub fn home(&self) -> HomeInfo {
        let mut h = lock(&self.home);
        if h.country.is_none() && h.source == HOME_NOT_LOOKED_UP {
            *h = region_home();
        }
        h.clone()
    }

    /// The jobs that are due now: location databases (when their switch is on and a month is missing), subscriptions on a schedule,
    /// and a first index build.
    pub fn due(&self) -> Vec<JobSpec> {
        let now_ms = self.clock.wall_ms();
        let now_secs = u64::try_from(now_ms / 1000).unwrap_or(0);
        let lists = lock(&self.lists).clone();
        let store = lock(&self.fetch).clone();
        let mut out = Vec::new();
        if lists.switches.location_databases {
            // The first download happens when the map is first opened ([`wants_geo`](Self::wants_geo)); this is the monthly one.
            let have = geo_fetch::newest_month(&self.dirs.intel_cache(COUNTRY.id));
            let last = store.lists.get(COUNTRY.id).and_then(|s| s.status.last_attempt_ms);
            if have.is_some() && geo_fetch::automatic_due(now_secs, last, have) {
                out.push(JobSpec::UpdateGeo);
            }
        }
        for l in &lists.lists {
            if let (
                true,
                ListSource::Url {
                    schedule: Schedule::Automatic { every },
                    ..
                },
            ) = (l.enabled, &l.source)
            {
                let last = store.lists.get(&l.id).and_then(|s| s.status.last_attempt_ms);
                if paddock_download::schedule::automatic_due(last, now_ms, every.duration(), dyno_core::intel_sub::MIN_INTERVAL) {
                    out.push(JobSpec::UpdateList { id: l.id.clone() });
                }
            }
        }
        out
    }

    /// Whether opening the map should start the location job now: the databases are switched on and none is loaded yet, or the
    /// own location was not looked for in the last hour (and its switch is on).
    pub fn wants_geo(&self) -> bool {
        let lists = lock(&self.lists);
        let mut tried = lock(&self.home_tried_ms);
        let now = self.clock.wall_ms();
        let hour = 3_600_000;
        let stale = tried.is_none_or(|t| now.saturating_sub(t) >= hour || t > now);
        let first_db = lists.switches.location_databases && !self.geo().is_on();
        if stale && (first_db || lists.switches.own_location) {
            *tried = Some(now);
            return true;
        }
        false
    }

    /// The map's data for the current connections: aggregated from every row, before any page cap. `flagged` is decided by the
    /// index; no name is looked up.
    pub fn geo_summary(&self, rows: &[(IpAddr, String)], left_out: u32) -> GeoSummary {
        let ix = self.index();
        let geo = self.geo();
        let rows: Vec<GeoRow> = rows
            .iter()
            .map(|(ip, program)| GeoRow {
                remote: *ip,
                program: program.clone(),
                flagged: matches!(ix.check(ip, None), dyno_core::intel::Verdict::Flagged { .. }),
            })
            .collect();
        let date = geo.built.map(|b| {
            let (y, m, d) = geo_fetch::civil(b);
            format!("{y:04}-{m:02}-{d:02}")
        });
        summarise(&rows, geo.is_on().then_some(&*geo as &dyn GeoLookup), date, left_out, &[])
    }

    /// The names of the lists that flag `ip` (empty when none does or the address is allowed); no name is looked up.
    pub fn flagged_by(&self, ip: &IpAddr) -> Vec<String> {
        match self.index().check(ip, None) {
            dyno_core::intel::Verdict::Flagged { matches } => {
                let mut names: Vec<String> = matches.into_iter().map(|m| m.list_name).collect();
                names.dedup();
                names
            }
            _ => Vec::new(),
        }
    }

    /// The folder of a list's cached text.
    pub fn cache_of(&self, id: &str) -> std::path::PathBuf {
        self.dirs.intel_cache(id)
    }
}

/// Whether an edit changes what the index matches (so a rebuild job should follow).
pub fn needs_rebuild(edit: &IntelEdit, outcome: &EditOutcome) -> bool {
    matches!(
        edit,
        IntelEdit::AddList { .. }
            | IntelEdit::RemoveList { .. }
            | IntelEdit::SetEnabled { .. }
            | IntelEdit::SetText { .. }
            | IntelEdit::DedupeApply { .. }
            | IntelEdit::SetAllow { .. }
    ) && !matches!(outcome, EditOutcome::Preview(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo_db::testdb::{sample_asn, sample_country};
    use crate::intel_fetch::tests::{config, ctx};
    use dyno_core::intel::Verdict;
    use dyno_core::intel_lists::NewSource;
    use flate2::{write::GzEncoder, Compression};
    use paddock_jobs::time::FakeClock;
    use paddock_net::{Egress, FakeWire, Reply, StaticLookup};
    use std::io::Write;
    use std::time::Duration;

    // 2026-11-03 12:00 UTC.
    const NOW_MS: i64 = 1_793_707_200_000;

    struct Rig {
        centre: IntelCentre,
        clock: FakeClock,
        wire: Arc<FakeWire>,
        dirs: Dirs,
        _tmp: paddock_testkit::PrivateTemp,
    }

    fn gz(bytes: &[u8]) -> Vec<u8> {
        let mut e = GzEncoder::new(Vec::new(), Compression::fast());
        e.write_all(bytes).unwrap();
        e.finish().unwrap()
    }

    fn rig(replies: Vec<Reply>) -> Rig {
        let tmp = paddock_testkit::private_tempdir().unwrap();
        let dirs = Dirs::at(tmp.path());
        let clock = FakeClock::at_wall_ms(NOW_MS);
        let mut wire = FakeWire::new();
        for r in replies {
            wire = wire.then(r);
        }
        let wire = Arc::new(wire);
        let lookup = Arc::new(
            StaticLookup::new()
                .with("lists.example", &["93.184.216.34"])
                .with(crate::geo_fetch::HOST, &["93.184.216.35"]),
        );
        let list_egress = Egress::new(crate::intel_fetch::policy(), wire.clone(), lookup.clone(), Arc::new(clock.clone()));
        let geo_egress = Egress::new(crate::geo_fetch::policy(), wire.clone(), lookup, Arc::new(clock.clone()));
        let net = Net {
            lists: Some(Arc::new(Fetcher::new(Arc::new(list_egress), Arc::new(clock.clone()), config()))),
            geo: Some(Arc::new(GeoFetcher::new(Arc::new(geo_egress), Arc::new(clock.clone()), config()))),
            home: None,
        };
        Rig {
            centre: IntelCentre::with_net(dirs.clone(), Arc::new(clock.clone()), net),
            clock,
            wire,
            dirs,
            _tmp: tmp,
        }
    }

    fn add_manual(c: &IntelCentre, name: &str, text: &str) -> String {
        let EditOutcome::Added { id } = c
            .edit(IntelEdit::AddList {
                name: name.into(),
                source: NewSource::Manual { text: text.into() },
            })
            .unwrap()
        else {
            panic!()
        };
        id
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn edits_are_saved_and_come_back_after_a_restart() {
        let r = rig(vec![]);
        let id = add_manual(&r.centre, "Work", "8.8.8.8 # dns\n8.8.0.0/16");
        r.centre.edit(IntelEdit::SetAllow { text: "9.9.9.9".into() }).unwrap();
        let again = IntelCentre::new(r.dirs.clone(), Arc::new(r.clock.clone()));
        let st = again.status();
        assert_eq!(st.lists.len(), 1);
        assert_eq!((st.lists[0].id.as_str(), st.lists[0].name.as_str()), (id.as_str(), "Work"));
        assert_eq!(
            (st.lists[0].counts.entries, st.lists[0].counts.covered),
            (2, 0),
            "as typed, until duplicates are removed"
        );
        assert_eq!(st.allow, "9.9.9.9");
        assert!(st.geo.switches.location_databases && st.geo.switches.own_location, "on by default");
        assert!(!st.first_run_note.is_empty() && st.geo.attribution.contains("DB-IP"));
    }

    #[test]
    fn a_corrupt_or_newer_lists_file_gives_the_defaults() {
        let tmp = paddock_testkit::private_tempdir().unwrap();
        let dirs = Dirs::at(tmp.path());
        std::fs::write(dirs.intel_lists(), b"{ nope").unwrap();
        assert_eq!(load_lists(&dirs), UserLists::default());
        std::fs::write(dirs.intel_lists(), br#"{"version": 7}"#).unwrap();
        assert_eq!(load_lists(&dirs), UserLists::default());
    }

    #[test]
    fn the_index_is_built_by_a_job_and_matches_lists_allows_and_names() {
        let r = rig(vec![]);
        add_manual(&r.centre, "A", "8.8.8.0/24 # office scanner\nevil.example");
        add_manual(&r.centre, "B", "8.8.8.0/24");
        r.centre
            .edit(IntelEdit::SetAllow {
                text: "8.8.8.9\nupdate.example".into(),
            })
            .unwrap();
        assert_eq!(
            r.centre.index().check(&ip("8.8.8.8"), None),
            Verdict::Clean,
            "nothing before the build"
        );
        r.centre.rebuild(&ctx()).unwrap();
        let Verdict::Flagged { matches } = r.centre.index().check(&ip("8.8.8.8"), None) else {
            panic!()
        };
        assert_eq!(matches.len(), 2, "both lists are named");
        assert_eq!(matches[0].note.as_deref(), Some("office scanner"));
        assert!(matches!(r.centre.index().check(&ip("8.8.8.9"), None), Verdict::Allowed { .. }));
        assert!(matches!(
            r.centre.index().check(&ip("1.2.3.4"), Some("x.evil.example")),
            Verdict::Flagged { .. }
        ));
        let st = r.centre.status();
        assert_eq!((st.index_ranges, st.index_error), (1, None));
    }

    #[test]
    fn a_cancelled_build_keeps_the_old_index_and_a_limit_is_reported() {
        let r = rig(vec![]);
        add_manual(&r.centre, "A", "8.8.8.8");
        r.centre.rebuild(&ctx()).unwrap();
        let big: String = (0..40_000)
            .map(|i| format!("{}.{}.{}.1\n", 20 + i / 65_536, (i / 256) % 256, i % 256))
            .collect();
        add_manual(&r.centre, "Big", &big);
        let job = ctx();
        job.cancel_token().cancel();
        assert!(matches!(r.centre.rebuild(&job), Err(JobError::Cancelled(_))));
        assert!(
            matches!(r.centre.index().check(&ip("8.8.8.8"), None), Verdict::Flagged { .. }),
            "the old index is still in use"
        );
        assert_eq!(r.centre.index().stored_ranges(), 1);
    }

    #[test]
    fn a_subscription_is_fetched_as_a_job_then_the_index_includes_it_and_removing_it_deletes_its_text() {
        let r = rig(vec![Reply::ok(b"# list\n1.2.3.0/24 # c2 range\n")]);
        let EditOutcome::Added { id } = r
            .centre
            .edit(IntelEdit::AddList {
                name: "Feed".into(),
                source: NewSource::Url {
                    url: "https://lists.example/feed.txt".into(),
                    every_hours: Some(24),
                },
            })
            .unwrap()
        else {
            panic!()
        };
        r.centre.update_list(&ctx(), &id).unwrap();
        assert!(matches!(r.centre.index().check(&ip("1.2.3.4"), None), Verdict::Flagged { .. }));
        let st = r.centre.status();
        assert_eq!(st.lists[0].counts.entries, 1);
        assert_eq!(st.lists[0].counts.last_fetched_ms, Some(NOW_MS));
        assert!(st.lists[0].next_allowed_ms.is_some(), "the hourly floor is shown");
        assert!(r.dirs.intel_cache(&id).join("list.txt").is_file());
        // The fetch state survives a restart (outside the cache).
        assert!(FetchStore::load(&r.dirs).lists.contains_key(&id));
        r.centre.edit(IntelEdit::RemoveList { id: id.clone() }).unwrap();
        assert!(!r.dirs.intel_cache(&id).exists());
        assert!(!FetchStore::load(&r.dirs).lists.contains_key(&id));
        assert_eq!(r.wire.sends(), 1);
    }

    #[test]
    fn a_failed_subscription_update_fails_the_job_with_the_reason_and_keeps_it_on_the_page() {
        let r = rig(vec![Reply::status(404)]);
        let EditOutcome::Added { id } = r
            .centre
            .edit(IntelEdit::AddList {
                name: "Feed".into(),
                source: NewSource::Url {
                    url: "https://lists.example/gone.txt".into(),
                    every_hours: None,
                },
            })
            .unwrap()
        else {
            panic!()
        };
        let e = r.centre.update_list(&ctx(), &id).unwrap_err();
        assert!(matches!(&e, JobError::Failed(m) if m.contains("404")), "{e:?}");
        assert!(r.centre.status().lists[0].counts.error.as_deref().unwrap().contains("404"));
    }

    #[test]
    fn what_is_due_follows_the_schedules_and_the_switches() {
        let r = rig(vec![]);
        let EditOutcome::Added { id } = r
            .centre
            .edit(IntelEdit::AddList {
                name: "Feed".into(),
                source: NewSource::Url {
                    url: "https://lists.example/feed.txt".into(),
                    every_hours: Some(24),
                },
            })
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(
            r.centre.due(),
            vec![JobSpec::UpdateList { id: id.clone() }],
            "never fetched: due; no database yet: the map's first open starts that"
        );
        r.centre
            .edit(IntelEdit::SetSchedule {
                id: id.clone(),
                every_hours: None,
            })
            .unwrap();
        assert!(r.centre.due().is_empty(), "manual subscriptions never start alone");
        r.centre.edit(IntelEdit::SetSchedule { id, every_hours: Some(24) }).unwrap();
        r.centre
            .edit(IntelEdit::SetEnabled {
                id: "list-1".into(),
                enabled: false,
            })
            .unwrap();
        assert!(r.centre.due().is_empty(), "an off list is not fetched");
    }

    #[test]
    fn opening_the_map_starts_the_location_job_once_an_hour_and_respects_the_switches() {
        let r = rig(vec![]);
        assert!(r.centre.wants_geo(), "no database yet");
        assert!(!r.centre.wants_geo(), "asked just now");
        r.clock.advance(Duration::from_secs(3601));
        assert!(r.centre.wants_geo(), "the own location is looked up again after an hour");
        let off = dyno_core::intel_lists::NetSwitches {
            location_databases: false,
            own_location: false,
            first_run_note_seen: true,
        };
        r.centre.edit(IntelEdit::SetSwitches { switches: off }).unwrap();
        r.clock.advance(Duration::from_secs(7200));
        assert!(!r.centre.wants_geo(), "both switches off: nothing goes online");
        assert!(r.centre.due().is_empty());
    }

    #[test]
    fn an_asn_database_that_fails_does_not_fail_the_job_but_names_itself_on_its_status_line() {
        let built = u64::try_from(NOW_MS / 1000).unwrap() - 3600;
        let r = rig(vec![Reply::ok(&gz(&sample_country(built))), Reply::ok(&gz(&sample_country(built)))]);
        r.centre.update_geo(&ctx()).expect("country is fine");
        let st = r.centre.status();
        assert!(st.geo.database_on);
        assert!(st.geo.country.error.is_none());
        let msg = st.geo.asn.error.expect("the ASN failure is shown");
        assert!(msg.starts_with("IP to ASN database:"), "{msg}");
        // The country database failing is the job's failure, and says which one.
        let r = rig(vec![Reply::ok(&gz(&sample_asn(built))), Reply::ok(&gz(&sample_asn(built)))]);
        let e = r.centre.update_geo(&ctx()).unwrap_err().to_string();
        assert!(e.starts_with("IP to Country database:"), "{e}");
    }

    #[test]
    fn the_location_databases_download_as_a_job_and_the_summary_counts_flagged_connections() {
        let built = u64::try_from(NOW_MS / 1000).unwrap() - 3600;
        let r = rig(vec![Reply::ok(&gz(&sample_country(built))), Reply::ok(&gz(&sample_asn(built)))]);
        add_manual(&r.centre, "A", "8.8.8.0/24");
        r.centre.rebuild(&ctx()).unwrap();
        r.centre.update_geo(&ctx()).unwrap();
        assert!(r.centre.geo().is_on());
        let st = r.centre.status();
        assert!(st.geo.database_on && st.geo.db_date.is_some());
        assert_eq!(st.geo.country.last_fetched_ms, Some(NOW_MS));
        // The sample databases cover documentation ranges, which are never located: everything public is "unknown", the rest local.
        let s = r.centre.geo_summary(
            &[
                (ip("8.8.8.8"), "Firefox".into()),
                (ip("192.0.2.5"), "Mail".into()),
                (ip("10.0.0.2"), "Mail".into()),
            ],
            7,
        );
        assert_eq!((s.unknown, s.truncated, s.database_on), (1, 7, true));
        assert_eq!(s.local.iter().map(|g| g.connections).sum::<u32>(), 2);
        assert_eq!(r.wire.sends(), 2);
    }

    #[test]
    fn without_any_database_the_summary_says_so_and_nothing_is_located() {
        let r = rig(vec![]);
        let s = r.centre.geo_summary(&[(ip("8.8.8.8"), "A".into())], 0);
        assert_eq!((s.database_on, s.unknown, s.countries.len()), (false, 1, 0));
    }

    #[test]
    fn the_own_location_is_not_asked_for_when_its_switch_is_off() {
        let r = rig(vec![]);
        r.centre
            .edit(IntelEdit::SetSwitches {
                switches: dyno_core::intel_lists::NetSwitches {
                    own_location: false,
                    ..Default::default()
                },
            })
            .unwrap();
        r.centre.find_home(&ctx());
        let h = r.centre.home();
        assert!(
            h.source.contains("region setting") || h.source.contains("switched off"),
            "either the region setting named a country or it says why not: {}",
            h.source
        );
        assert!(!h.source.contains("public IP"), "the address lookup was not made");
        assert_eq!(r.wire.sends(), 0);
    }

    #[test]
    fn before_any_lookup_the_home_is_the_region_setting_not_a_blank() {
        let r = rig(vec![]);
        let h = r.centre.home();
        assert!(
            h.source == "Your location: from your region setting" || h.source == "Your location: unknown (no region setting found)",
            "{}",
            h.source
        );
        assert_eq!(h.country.is_some(), h.source.contains("from your region setting"));
        assert_eq!(r.centre.status().geo.home_source, h.source, "the page says the same");
        assert_eq!(r.wire.sends(), 0, "reading the region setting sends nothing");
    }
}
