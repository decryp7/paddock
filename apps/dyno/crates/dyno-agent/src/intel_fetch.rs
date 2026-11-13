//! Fetching the lists the user subscribed to (ADR 0031 amendment 1). Dyno has no list of its own: every address here is one the
//! user typed. Each fetch is a `paddock-download` job over `paddock-net`'s [`Egress`] (https only, public addresses only, a size bound,
//! cancel in every phase), at most hourly per subscription; the text lands in the cache folder with the last good copy kept.
//!
//! - **The floor** counts from the last *attempt*, kept in `intel-fetch.json` in the data folder, not in the cache, so clearing
//!   the cache cannot cause an early fetch. "Update now" respects it too and answers the time it is next allowed.
//! - **A large drop** (below 10 % of the last good copy's entries) is not taken: the old copy stays in use, with a note.
//! - **Logs** carry the list id (one of Dyno's own slugs), phases, durations, byte and entry counts, HTTP status and the kind of
//!   failure. Never the address of the list, an entry, or the text (ER-27).

use crate::paths::Dirs;
use dyno_core::intel_sub::{SubscriptionStatus, MAX_BYTES, MIN_INTERVAL, MIN_KEEP_PERCENT};
use dyno_core::intel_text::{parse_list, Entry};
use paddock_download::schedule::{is_large_drop, next_allowed, staging_path, swap_in};
use paddock_download::{Config, DownloadError, Downloader, Integrity, Request};
use paddock_jobs::time::Clock;
use paddock_jobs::{JobContext, Phase};
use paddock_net::{Egress, Policy, Reach, SystemLookup};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// The text of the list in use.
const CURRENT: &str = "list.txt";
/// The copy before the current one.
const PREVIOUS: &str = "previous.txt";
/// Largest `intel-fetch.json` read.
const MAX_STATE_FILE: u64 = 4 << 20;

/// The job phase for reading the lines and comparing with the last good copy (the download's own phases are
/// `paddock-download`'s `connect`, `download` and `verify`).
pub const CHECK: &str = "check";
/// The job phase for replacing the current copy.
pub const SWAP: &str = "swap";

/// What an update did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A new list is in use.
    Updated {
        /// Entries after removing duplicates and covered entries.
        entries: u32,
        /// Bytes received.
        bytes: u64,
    },
    /// The new list looked broken (a large drop in entries): the previous copy stays in use.
    KeptPrevious {
        /// Entries in the rejected list.
        rejected_entries: u32,
    },
}

/// Why an update did not happen or did not finish. Every message says what to do next.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    /// The hourly floor.
    #[error("This list can be updated again at the time shown (at most once an hour).")]
    TooSoon {
        /// When, milliseconds since the Unix epoch.
        next_allowed_ms: i64,
    },
    /// The network layer refused or could not complete the request.
    #[error("{0}")]
    Net(String),
    /// The server answered with a status other than success.
    #[error("The server answered {0}. Check the address of the list.")]
    Http(u16),
    /// Larger than the limit.
    #[error("The list is larger than {} MB. Choose a smaller list.", MAX_BYTES >> 20)]
    TooBig,
    /// Not text, or no usable line.
    #[error("The file is not a list of addresses, ranges or host names (one per line). Check the address of the list.")]
    NotAList,
    /// Stopped by the user.
    #[error("Stopped.")]
    Cancelled,
    /// The cache folder could not be written.
    #[error("The list could not be saved in the cache folder ({0}). Check free space and permissions.")]
    Save(String),
}

impl FetchError {
    /// A short kind for logs (no text from the network).
    pub fn kind(&self) -> &'static str {
        match self {
            FetchError::TooSoon { .. } => "too-soon",
            FetchError::Net(_) => "network",
            FetchError::Http(_) => "http",
            FetchError::TooBig => "too-big",
            FetchError::NotAList => "not-a-list",
            FetchError::Cancelled => "cancelled",
            FetchError::Save(_) => "save",
        }
    }
}

/// What is kept about one subscription between runs (outside the cache folder).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FetchState {
    /// What the page shows.
    pub status: SubscriptionStatus,
}

/// The states of every subscription, in `intel-fetch.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FetchStore {
    /// File format version.
    pub version: u32,
    /// By list id (and `geo-country`, `geo-asn` for the location databases).
    pub lists: BTreeMap<String, FetchState>,
}

impl FetchStore {
    /// Reads the file; a missing, oversized or corrupt file gives the defaults (and a log line saying which).
    pub fn load(dirs: &Dirs) -> FetchStore {
        let path = dirs.intel_fetch_state();
        let Ok(meta) = std::fs::metadata(&path) else {
            return FetchStore::default();
        };
        if meta.len() > MAX_STATE_FILE {
            log::warn!(target: "dyno::intel", "fetch state ignored: file too large");
            return FetchStore::default();
        }
        match std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<FetchStore>(&b).ok())
        {
            Some(s) if s.version <= 1 => s,
            _ => {
                log::warn!(target: "dyno::intel", "fetch state ignored: unreadable or from a newer version");
                FetchStore::default()
            }
        }
    }

    /// Writes the file whole (a temporary file, then a rename).
    pub fn save(&self, dirs: &Dirs) -> std::io::Result<()> {
        let path = dirs.intel_fetch_state();
        let mut out = self.clone();
        out.version = 1;
        let bytes = serde_json::to_vec_pretty(&out).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes)?;
        replace(&tmp, &path)
    }
}

/// The policy of the subscription door: public addresses only, https only (the egress enforces both), any host (the user chose
/// the address), a body cap of [`MAX_BYTES`], three redirects, and an honest User-Agent.
pub fn policy() -> Policy {
    let mut p = Policy::default()
        .reach(Reach::Public)
        .max_redirects(3)
        .max_body(MAX_BYTES)
        .timeout(Duration::from_secs(120));
    p.user_agent = format!("Dyno/{}", env!("CARGO_PKG_VERSION"));
    p
}

/// Maps a failed download to the error the page shows.
pub(crate) fn map_download(e: DownloadError) -> FetchError {
    match e {
        DownloadError::Cancelled(_) => FetchError::Cancelled,
        DownloadError::Http { status, .. } => FetchError::Http(status),
        DownloadError::TooLarge { .. } => FetchError::TooBig,
        DownloadError::DiskFull | DownloadError::NotEnoughSpace { .. } => FetchError::Save("disk full".into()),
        DownloadError::Io(e) => FetchError::Save(e.kind().to_string()),
        // The door refused an address of this computer or a private network (a name that resolves to one, or a redirect to one).
        other if other.to_string().contains("points at this computer") || other.to_string().contains("private network") => FetchError::Net(
            "That address leads to this computer or a private network. Dyno fetches lists only from public addresses.".into(),
        ),
        other => FetchError::Net(other.to_string()),
    }
}

/// Downloads `url` to `dest` (a staging file in the cache folder) with `paddock-download`: job phases `connect`, `download` and
/// `verify` on `ctx`, cancel in every phase, the size bound, and the digest computed while writing. No checksum is known in
/// advance. Returns the bytes written.
pub(crate) fn download_to(downloader: &Downloader, ctx: &JobContext, url: &str, dest: &Path, max_bytes: u64) -> Result<u64, FetchError> {
    if let Some(dir) = dest.parent() {
        paddock_env::create_private_dir_all(dir).map_err(|e| {
            FetchError::Save(format!(
                "the list's cache folder could not be made ({}); next step: check that Dyno's data folder is writable",
                e.kind()
            ))
        })?;
    }
    let request = Request::new(url, dest, Integrity::Unpinned)
        .map_err(|e| FetchError::Net(e.to_string()))?
        .max_bytes(max_bytes);
    downloader.download(ctx, &request).map(|d| d.bytes).map_err(map_download)
}

/// Fetches subscriptions through one egress.
pub struct Fetcher {
    downloader: Downloader,
    clock: Arc<dyn Clock>,
}

impl Fetcher {
    /// A fetcher over a given door (tests pass a fake wire) and download settings.
    pub fn new(egress: Arc<Egress>, clock: Arc<dyn Clock>, config: Config) -> Fetcher {
        Fetcher {
            downloader: Downloader::new(Arc::new(egress.download_transport())).with_config(config),
            clock,
        }
    }

    /// The real door: the system resolver and the real wire with the system's proxy settings. Built only when the user has a
    /// subscription turned on, so with none, the agent constructs no `Egress` at all.
    pub fn real(clock: Arc<dyn Clock>) -> Result<Fetcher, FetchError> {
        let policy = policy();
        let wire = paddock_net::UreqWire::new(&policy, paddock_download::ProxyMode::System).map_err(|e| FetchError::Net(e.to_string()))?;
        let egress = Egress::new(policy, Arc::new(wire), Arc::new(SystemLookup), clock.clone());
        Ok(Fetcher::new(Arc::new(egress), clock, Config::default()))
    }

    /// Updates one subscription as the job `ctx`: `url` is the user's address, `cache` the folder of this list
    /// ([`Dirs::intel_cache`]), `state` what was kept last time (changed in place: the caller saves it).
    pub fn update(&self, id: &str, url: &str, state: &mut FetchState, cache: &Path, ctx: &JobContext) -> Result<Outcome, FetchError> {
        let started = self.clock.monotonic();
        let result = self.run(url, state, cache, ctx);
        let ms = (self.clock.monotonic().saturating_sub(started)).as_millis();
        match &result {
            Ok(o) => {
                log::info!(target: "dyno::intel", "list update: id={id} outcome={} duration_ms={ms}", outcome_kind(o));
            }
            Err(e) => {
                log::warn!(target: "dyno::intel", "list update failed: id={id} kind={} duration_ms={ms}", e.kind());
            }
        }
        result
    }

    fn run(&self, url: &str, state: &mut FetchState, cache: &Path, ctx: &JobContext) -> Result<Outcome, FetchError> {
        let now_ms = self.clock.wall_ms();
        if let Some(at) = next_allowed(state.status.last_attempt_ms, now_ms, MIN_INTERVAL) {
            return Err(FetchError::TooSoon { next_allowed_ms: at });
        }
        // From here on this counts as an attempt, whatever happens.
        state.status.last_attempt_ms = Some(now_ms);
        let staging = staging_path(&cache.join(CURRENT));
        let outcome = self.fetch(url, state, cache, &staging, ctx);
        let _ = std::fs::remove_file(&staging);
        match &outcome {
            Ok(Outcome::Updated { .. }) => {
                state.status.error = None;
                state.status.last_fetched_ms = Some(now_ms);
            }
            Ok(Outcome::KeptPrevious { .. }) => {}
            Err(e) => state.status.error = Some(e.to_string()),
        }
        outcome
    }

    fn fetch(&self, url: &str, state: &mut FetchState, cache: &Path, staging: &Path, ctx: &JobContext) -> Result<Outcome, FetchError> {
        let bytes = download_to(&self.downloader, ctx, url, staging, MAX_BYTES)?;
        ctx.phase(Phase::items(CHECK, "Checking the list", 1));
        let body = std::fs::read(staging).map_err(|e| FetchError::Save(e.kind().to_string()))?;
        let text = std::str::from_utf8(&body).map_err(|_| FetchError::NotAList)?;
        if text.contains('\0') {
            return Err(FetchError::NotAList);
        }
        let parsed = parse_list(text, dyno_core::intel::MAX_TOTAL_ENTRIES);
        let entries = u32::try_from(parsed.entries.len()).unwrap_or(u32::MAX);
        if entries == 0 && (parsed.skipped_count > 0 || !text.trim().is_empty()) && state.status.entries == 0 {
            return Err(FetchError::NotAList);
        }
        if is_large_drop(u64::from(state.status.entries), u64::from(entries), MIN_KEEP_PERCENT, 10) {
            state.status.note = Some(format!(
                "The new list has {entries} entries instead of {}; kept the previous one.",
                state.status.entries
            ));
            return Ok(Outcome::KeptPrevious { rejected_entries: entries });
        }
        if ctx.cancel_token().is_cancelled() {
            return Err(FetchError::Cancelled);
        }
        ctx.phase(Phase::items(SWAP, "Replacing the list", 1));
        swap_in(staging, &cache.join(CURRENT), &cache.join(PREVIOUS)).map_err(|e| FetchError::Save(e.kind().to_string()))?;
        state.status.entries = entries;
        state.status.duplicates = parsed.duplicates;
        state.status.covered = parsed.covered;
        state.status.skipped = parsed.skipped_count;
        state.status.note = None;
        Ok(Outcome::Updated { entries, bytes })
    }
}

fn outcome_kind(o: &Outcome) -> String {
    match o {
        Outcome::Updated { entries, bytes } => format!("updated entries={entries} bytes={bytes}"),
        Outcome::KeptPrevious { rejected_entries } => format!("kept-previous rejected_entries={rejected_entries}"),
    }
}

/// Moves `from` over `to` (Windows cannot rename onto an existing file).
pub(crate) use paddock_download::schedule::replace;

/// The entries of the list in use for subscription `cache` (empty when nothing was fetched yet or the cache was cleared). The
/// text is read again from the cache folder; it is bounded by [`MAX_BYTES`].
pub fn load_entries(cache: &Path) -> Vec<Entry> {
    let path = cache.join(CURRENT);
    let Ok(meta) = std::fs::metadata(&path) else {
        return Vec::new();
    };
    if meta.len() > MAX_BYTES {
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    parse_list(&text, dyno_core::intel::MAX_TOTAL_ENTRIES).entries
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use paddock_jobs::time::FakeClock;
    use paddock_jobs::{EventSink, JobRegistry, JobSnapshot};
    use paddock_net::{FakeWire, Reply, StaticLookup};
    use std::time::Duration;

    const URL: &str = "https://lists.example/mine.txt";
    const HOUR_MS: i64 = 3_600_000;

    struct Null;
    impl EventSink for Null {
        fn emit(&self, _: &JobSnapshot) {}
    }

    pub(crate) fn ctx() -> JobContext {
        JobRegistry::new(Arc::new(Null)).register("test", "test", None)
    }

    /// One attempt, no retry sleeps.
    pub(crate) fn config() -> Config {
        Config {
            attempts: 1,
            max_throttles: 1,
            backoff: Duration::from_millis(1),
            max_wait: Duration::from_millis(5),
            ..Config::default()
        }
    }

    struct Rig {
        fetcher: Fetcher,
        wire: Arc<FakeWire>,
        clock: FakeClock,
        dir: paddock_testkit::PrivateTemp,
    }

    fn rig(replies: Vec<Reply>) -> Rig {
        let clock = FakeClock::at_wall_ms(1_800_000_000_000);
        let mut wire = FakeWire::new();
        for r in replies {
            wire = wire.then(r);
        }
        let wire = Arc::new(wire);
        let lookup = Arc::new(StaticLookup::new().with("lists.example", &["93.184.216.34"]));
        let egress = Egress::new(policy(), wire.clone(), lookup, Arc::new(clock.clone()));
        Rig {
            fetcher: Fetcher::new(Arc::new(egress), Arc::new(clock.clone()), config()),
            wire,
            clock,
            dir: paddock_testkit::private_tempdir().unwrap(),
        }
    }

    fn run(r: &Rig, state: &mut FetchState) -> Result<Outcome, FetchError> {
        r.fetcher.update("list-1", URL, state, &r.dir.path().join("c"), &ctx())
    }

    fn list_of(n: usize) -> Vec<u8> {
        (1..=n)
            .map(|i| format!("8.8.{}.{}\n", i / 250, i % 250 + 1))
            .collect::<String>()
            .into_bytes()
    }

    #[test]
    fn a_good_list_is_taken_counted_and_kept_in_the_cache() {
        let r = rig(vec![Reply::ok(b"# mine\n8.8.8.8 # dns\n8.8.0.0/16\nevil.example\n")]);
        let mut st = FetchState::default();
        let o = run(&r, &mut st);
        assert_eq!(o, Ok(Outcome::Updated { entries: 2, bytes: 45 }));
        assert_eq!((st.status.entries, st.status.covered, st.status.duplicates), (2, 1, 0));
        assert_eq!(st.status.last_fetched_ms, Some(1_800_000_000_000));
        assert_eq!(st.status.error, None);
        assert_eq!(load_entries(&r.dir.path().join("c")).len(), 2);
        assert_eq!(r.wire.sends(), 1);
        assert!(policy().user_agent.starts_with("Dyno/"), "an honest User-Agent");
        let c = r.dir.path().join("c");
        assert!(
            !staging_path(&c.join(CURRENT)).exists() && !c.join("list.txt.staging.part").exists(),
            "no staging file is left"
        );
    }

    #[test]
    fn update_now_inside_the_hourly_floor_sends_nothing_and_says_when() {
        let r = rig(vec![Reply::ok(b"8.8.8.8\n"), Reply::ok(b"8.8.8.9\n")]);
        let mut st = FetchState::default();
        run(&r, &mut st).unwrap();
        r.clock.advance(Duration::from_secs(30 * 60));
        let e = run(&r, &mut st).unwrap_err();
        assert_eq!(
            e,
            FetchError::TooSoon {
                next_allowed_ms: 1_800_000_000_000 + HOUR_MS
            }
        );
        assert_eq!(r.wire.sends(), 1);
        r.clock.advance(Duration::from_secs(30 * 60));
        run(&r, &mut st).unwrap();
        assert_eq!(r.wire.sends(), 2);
    }

    #[test]
    fn a_cleared_cache_does_not_reset_the_floor() {
        let r = rig(vec![Reply::ok(b"8.8.8.8\n"), Reply::ok(b"8.8.8.8\n")]);
        let mut st = FetchState::default();
        run(&r, &mut st).unwrap();
        std::fs::remove_dir_all(r.dir.path().join("c")).unwrap();
        assert!(matches!(run(&r, &mut st), Err(FetchError::TooSoon { .. })));
        assert_eq!(r.wire.sends(), 1);
    }

    #[test]
    fn a_failed_attempt_counts_for_the_floor_and_the_error_is_kept() {
        let r = rig(vec![Reply::status(404)]);
        let mut st = FetchState::default();
        assert_eq!(run(&r, &mut st), Err(FetchError::Http(404)));
        assert!(st.status.error.as_deref().unwrap().contains("404"));
        assert!(matches!(run(&r, &mut st), Err(FetchError::TooSoon { .. })));
    }

    #[test]
    fn a_large_drop_keeps_the_last_good_copy_and_says_so() {
        let r = rig(vec![Reply::ok(&list_of(100)), Reply::ok(b"8.8.8.8\n")]);
        let mut st = FetchState::default();
        run(&r, &mut st).unwrap();
        assert_eq!(st.status.entries, 100);
        r.clock.advance(Duration::from_secs(3600));
        assert_eq!(run(&r, &mut st), Ok(Outcome::KeptPrevious { rejected_entries: 1 }));
        assert_eq!(st.status.entries, 100);
        assert!(st.status.note.as_deref().unwrap().contains("kept the previous one"));
        assert_eq!(load_entries(&r.dir.path().join("c")).len(), 100);
    }

    #[test]
    fn the_previous_copy_is_kept_beside_the_current_one() {
        let r = rig(vec![Reply::ok(b"8.8.8.8\n"), Reply::ok(b"9.9.9.9\n")]);
        let mut st = FetchState::default();
        run(&r, &mut st).unwrap();
        r.clock.advance(Duration::from_secs(3600));
        run(&r, &mut st).unwrap();
        let c = r.dir.path().join("c");
        assert_eq!(std::fs::read_to_string(c.join(PREVIOUS)).unwrap(), "8.8.8.8\n");
        assert_eq!(std::fs::read_to_string(c.join(CURRENT)).unwrap(), "9.9.9.9\n");
    }

    #[test]
    fn binary_and_non_list_answers_are_refused_and_nothing_is_kept() {
        for body in [
            &b"\x00\x01\x02"[..],
            &[0xFF, 0xFE, 0x41][..],
            b"<html><body>Not found now</body></html>\n",
        ] {
            let r = rig(vec![Reply::ok(body)]);
            let mut st = FetchState::default();
            assert_eq!(run(&r, &mut st), Err(FetchError::NotAList));
            assert!(!r.dir.path().join("c").join(CURRENT).exists());
            assert!(!staging_path(&r.dir.path().join("c").join(CURRENT)).exists());
        }
    }

    #[test]
    fn a_redirect_to_plain_http_and_an_unresolvable_name_are_refused() {
        let r = rig(vec![Reply::redirect(302, "http://lists.example/x")]);
        let mut st = FetchState::default();
        assert!(matches!(run(&r, &mut st), Err(FetchError::Net(_))));
        let r = rig(vec![]);
        let mut st = FetchState::default();
        let e = r
            .fetcher
            .update("l", "https://intranet.example/x", &mut st, &r.dir.path().join("c"), &ctx());
        assert!(
            matches!(e, Err(FetchError::Net(_))),
            "a name the resolver does not know is not reachable"
        );
    }

    #[test]
    fn a_stop_before_the_swap_leaves_the_old_list() {
        let r = rig(vec![Reply::ok(b"8.8.8.8\n"), Reply::ok(b"9.9.9.9\n")]);
        let mut st = FetchState::default();
        run(&r, &mut st).unwrap();
        r.clock.advance(Duration::from_secs(3600));
        let job = ctx();
        job.cancel_token().cancel();
        let e = r.fetcher.update("l", URL, &mut st, &r.dir.path().join("c"), &job);
        assert_eq!(e, Err(FetchError::Cancelled));
        assert_eq!(std::fs::read_to_string(r.dir.path().join("c").join(CURRENT)).unwrap(), "8.8.8.8\n");
    }

    #[test]
    fn the_fetch_state_round_trips_and_a_corrupt_file_gives_the_defaults() {
        let dir = paddock_testkit::private_tempdir().unwrap();
        let dirs = Dirs::at(dir.path());
        let mut s = FetchStore::default();
        let mut st = FetchState::default();
        st.status.entries = 7;
        s.lists.insert("a".into(), st);
        s.save(&dirs).unwrap();
        assert_eq!(FetchStore::load(&dirs).lists["a"].status.entries, 7);
        std::fs::write(dirs.intel_fetch_state(), b"{ not json").unwrap();
        assert_eq!(FetchStore::load(&dirs), FetchStore::default());
        std::fs::write(dirs.intel_fetch_state(), br#"{"version": 9}"#).unwrap();
        assert_eq!(FetchStore::load(&dirs), FetchStore::default(), "a newer version is not guessed at");
    }

    #[test]
    fn download_errors_map_to_messages_that_say_what_to_do() {
        use paddock_jobs::CancelReason;
        assert_eq!(map_download(DownloadError::Cancelled(CancelReason::User)), FetchError::Cancelled);
        assert_eq!(map_download(DownloadError::TooLarge { size: 2, limit: 1 }), FetchError::TooBig);
        assert!(matches!(map_download(DownloadError::DiskFull), FetchError::Save(_)));
        assert!(matches!(
            map_download(DownloadError::Io(std::io::ErrorKind::NotFound.into())),
            FetchError::Save(_)
        ));
        assert!(matches!(
            map_download(DownloadError::Throttled { host: "h".into() }),
            FetchError::Net(_)
        ));
    }

    #[test]
    fn with_no_subscription_no_egress_exists() {
        // The agent builds a Fetcher only when a subscription is on; the policy alone constructs nothing.
        assert_eq!(policy().max_body, MAX_BYTES);
    }
}
