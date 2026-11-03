//! Fetching the lists the user subscribed to (ADR 0031 amendment 1). Dyno has no list of its own: every address here is one the
//! user typed. Each fetch goes through `paddock-net`'s [`Egress`] (https only, public addresses only, the Offline switch, a size
//! cap), at most hourly per subscription, and the text lands in the cache folder with the last good copy kept.
//!
//! - **The floor** counts from the last *attempt*, kept in `intel-fetch.json` in the data folder, not in the cache, so clearing
//!   the cache cannot cause an early fetch. "Update now" respects it too and answers the time it is next allowed.
//! - **A large drop** (below 10 % of the last good copy's entries) is not taken: the old copy stays in use, with a note.
//! - **Logs** carry the list id (one of Dyno's own slugs), phases, durations, byte and entry counts, HTTP status and the kind of
//!   failure. Never the address of the list, an entry, or the text (ER-27).

use crate::paths::Dirs;
use dyno_core::intel_sub::{is_large_drop, next_allowed, SubscriptionStatus, MAX_BYTES, MIN_INTERVAL};
use dyno_core::intel_text::{parse_list, Entry};
use paddock_jobs::time::Clock;
use paddock_jobs::CancelToken;
use paddock_net::{Egress, NetError, Policy, Reach, Request, SystemLookup};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// The text of the list in use.
const CURRENT: &str = "list.txt";
/// The copy before the current one.
const PREVIOUS: &str = "previous.txt";
/// Where a download is written before it replaces the current copy.
const STAGING: &str = "list.staging";
/// Largest `intel-fetch.json` read.
const MAX_STATE_FILE: u64 = 4 << 20;

/// The phases of one update, for the job that runs it (progress is labelled by phase, ER-18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Asking for the file and reading it.
    Download,
    /// Reading the lines and comparing with the last good copy.
    Check,
    /// Replacing the current copy.
    Swap,
}

impl Phase {
    /// The phase name used in logs and job labels.
    pub fn name(self) -> &'static str {
        match self {
            Phase::Download => "download",
            Phase::Check => "check",
            Phase::Swap => "swap",
        }
    }
}

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
    /// The publisher said nothing changed (`304`).
    NotModified,
    /// The new list looked broken (a large drop in entries): the previous copy stays in use.
    KeptPrevious {
        /// Entries in the rejected list.
        rejected_entries: u32,
    },
}

/// Why an update did not happen or did not finish. Every message says what to do next.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    /// Dyno is offline.
    #[error("Dyno is offline. Turn Offline mode off in Settings, then update the list.")]
    Offline,
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
            FetchError::Offline => "offline",
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
    /// The validator for the next conditional request.
    pub etag: Option<String>,
}

/// The states of every subscription, in `intel-fetch.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FetchStore {
    /// File format version.
    pub version: u32,
    /// By list id.
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

/// Fetches subscriptions through one egress.
pub struct Fetcher {
    egress: Arc<Egress>,
    clock: Arc<dyn Clock>,
}

impl Fetcher {
    /// A fetcher over a given door (tests pass a fake wire).
    pub fn new(egress: Arc<Egress>, clock: Arc<dyn Clock>) -> Fetcher {
        Fetcher { egress, clock }
    }

    /// The real door: the system resolver and the real wire with the system's proxy settings. Built only when the user has a
    /// subscription turned on, so with none, the agent constructs no `Egress` at all.
    pub fn real(clock: Arc<dyn Clock>) -> Result<Fetcher, FetchError> {
        let policy = policy();
        let wire = paddock_net::UreqWire::new(&policy, paddock_download::ProxyMode::System).map_err(|e| FetchError::Net(e.to_string()))?;
        let egress = Egress::new(policy, Arc::new(wire), Arc::new(SystemLookup), clock.clone());
        Ok(Fetcher::new(Arc::new(egress), clock))
    }

    /// Dyno's Offline switch: while on, every update fails at once with [`FetchError::Offline`].
    pub fn set_offline(&self, offline: bool) {
        self.egress.set_offline(offline);
    }

    /// Updates one subscription: `url` is the user's address, `cache` the folder of this list ([`Dirs::intel_cache`]), `state` what
    /// was kept last time (changed in place: the caller saves it). `phase` is told when each phase starts.
    pub fn update(
        &self,
        id: &str,
        url: &str,
        state: &mut FetchState,
        cache: &Path,
        cancel: &CancelToken,
        phase: &mut dyn FnMut(Phase),
    ) -> Result<Outcome, FetchError> {
        let started = self.clock.monotonic();
        let result = self.run(url, state, cache, cancel, phase);
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

    fn run(
        &self,
        url: &str,
        state: &mut FetchState,
        cache: &Path,
        cancel: &CancelToken,
        phase: &mut dyn FnMut(Phase),
    ) -> Result<Outcome, FetchError> {
        if self.egress.is_offline() {
            return Err(FetchError::Offline);
        }
        let now_ms = self.clock.wall_ms();
        if let Some(at) = next_allowed(state.status.last_attempt_ms, now_ms, MIN_INTERVAL) {
            return Err(FetchError::TooSoon { next_allowed_ms: at });
        }
        // From here on this counts as an attempt, whatever happens.
        state.status.last_attempt_ms = Some(now_ms);
        let outcome = self.fetch(url, state, cache, cancel, phase);
        match &outcome {
            Ok(Outcome::Updated { .. } | Outcome::NotModified) => {
                state.status.error = None;
                state.status.last_fetched_ms = Some(now_ms);
                if matches!(outcome, Ok(Outcome::NotModified)) {
                    state.status.note = None;
                }
            }
            Ok(Outcome::KeptPrevious { .. }) => {}
            Err(e) => state.status.error = Some(e.to_string()),
        }
        outcome
    }

    fn fetch(
        &self,
        url: &str,
        state: &mut FetchState,
        cache: &Path,
        cancel: &CancelToken,
        phase: &mut dyn FnMut(Phase),
    ) -> Result<Outcome, FetchError> {
        phase(Phase::Download);
        let mut request = Request::get(url).without_lan();
        if let (Some(etag), true) = (state.etag.as_deref(), cache.join(CURRENT).is_file()) {
            request = request.if_none_match(etag).map_err(|e| FetchError::Net(e.to_string()))?;
        }
        let reply = self.egress.send(&request, cancel).map_err(map_net)?;
        if reply.not_modified() {
            return Ok(Outcome::NotModified);
        }
        if !reply.ok() {
            return Err(FetchError::Http(reply.status));
        }
        if cancel.is_cancelled() {
            return Err(FetchError::Cancelled);
        }
        phase(Phase::Check);
        let bytes = reply.body.len() as u64;
        let text = std::str::from_utf8(&reply.body).map_err(|_| FetchError::NotAList)?;
        if text.contains('\0') {
            return Err(FetchError::NotAList);
        }
        let parsed = parse_list(text, dyno_core::intel::MAX_TOTAL_ENTRIES);
        let entries = u32::try_from(parsed.entries.len()).unwrap_or(u32::MAX);
        if entries == 0 && (parsed.skipped_count > 0 || !text.trim().is_empty()) && state.status.entries == 0 {
            return Err(FetchError::NotAList);
        }
        if is_large_drop(state.status.entries, entries) {
            state.status.note = Some(format!(
                "The new list has {entries} entries instead of {}; kept the previous one.",
                state.status.entries
            ));
            return Ok(Outcome::KeptPrevious { rejected_entries: entries });
        }
        if cancel.is_cancelled() {
            return Err(FetchError::Cancelled);
        }
        phase(Phase::Swap);
        write_list(cache, &reply.body).map_err(|e| FetchError::Save(e.kind().to_string()))?;
        state.etag = reply.etag().map(str::to_string);
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
        Outcome::NotModified => "not-modified".into(),
        Outcome::KeptPrevious { rejected_entries } => format!("kept-previous rejected_entries={rejected_entries}"),
    }
}

fn map_net(e: NetError) -> FetchError {
    match e {
        NetError::Offline => FetchError::Offline,
        NetError::Cancelled => FetchError::Cancelled,
        NetError::TooLarge(_) => FetchError::TooBig,
        other => FetchError::Net(other.to_string()),
    }
}

/// Moves `from` over `to` (Windows cannot rename onto an existing file).
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    if cfg!(windows) && to.exists() {
        std::fs::remove_file(to)?;
    }
    std::fs::rename(from, to)
}

/// Writes the staging file, keeps the current copy as the previous one, and moves the new one into place.
fn write_list(cache: &Path, body: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(cache)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(cache, std::fs::Permissions::from_mode(0o700))?;
    }
    let staging = cache.join(STAGING);
    {
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&staging)?;
        f.write_all(body)?;
        f.sync_all()?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o600))?;
    }
    let current = cache.join(CURRENT);
    if current.is_file() {
        replace(&current, &cache.join(PREVIOUS))?;
    }
    replace(&staging, &current)
}

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
mod tests {
    use super::*;
    use paddock_jobs::time::FakeClock;
    use paddock_net::{FakeWire, Reply, StaticLookup};

    const URL: &str = "https://lists.example/mine.txt";
    const HOUR_MS: i64 = 3_600_000;

    struct Rig {
        fetcher: Fetcher,
        wire: Arc<FakeWire>,
        clock: FakeClock,
        dir: tempfile::TempDir,
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
            fetcher: Fetcher::new(Arc::new(egress), Arc::new(clock.clone())),
            wire,
            clock,
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn run(r: &Rig, state: &mut FetchState) -> Result<Outcome, FetchError> {
        r.fetcher
            .update("list-1", URL, state, &r.dir.path().join("c"), &CancelToken::new(), &mut |_| {})
    }

    fn list_of(n: usize) -> Vec<u8> {
        (1..=n)
            .map(|i| format!("8.8.{}.{}\n", i / 250, i % 250 + 1))
            .collect::<String>()
            .into_bytes()
    }

    #[test]
    fn a_good_list_is_taken_counted_and_kept_in_the_cache_with_its_validator() {
        let r = rig(vec![
            Reply::ok(b"# mine\n8.8.8.8 # dns\n8.8.0.0/16\nevil.example\n").header("etag", "\"v1\"")
        ]);
        let mut st = FetchState::default();
        let mut phases = Vec::new();
        let o = r
            .fetcher
            .update("list-1", URL, &mut st, &r.dir.path().join("c"), &CancelToken::new(), &mut |p| {
                phases.push(p)
            });
        assert_eq!(o, Ok(Outcome::Updated { entries: 2, bytes: 45 }));
        assert_eq!(phases, [Phase::Download, Phase::Check, Phase::Swap]);
        assert_eq!((st.status.entries, st.status.covered, st.status.duplicates), (2, 1, 0));
        assert_eq!(st.status.last_fetched_ms, Some(1_800_000_000_000));
        assert_eq!(st.etag.as_deref(), Some("\"v1\""));
        assert_eq!(st.status.error, None);
        assert_eq!(load_entries(&r.dir.path().join("c")).len(), 2);
        assert_eq!(r.wire.sends(), 1);
        assert!(policy().user_agent.starts_with("Dyno/"), "an honest User-Agent");
    }

    #[test]
    fn offline_refuses_before_any_request_and_does_not_count_as_an_attempt() {
        let r = rig(vec![Reply::ok(b"8.8.8.8\n")]);
        r.fetcher.set_offline(true);
        let mut st = FetchState::default();
        assert_eq!(run(&r, &mut st), Err(FetchError::Offline));
        assert_eq!(r.wire.sends(), 0);
        assert_eq!(st.status.last_attempt_ms, None);
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
        let r = rig(vec![Reply::status(503)]);
        let mut st = FetchState::default();
        assert_eq!(run(&r, &mut st), Err(FetchError::Http(503)));
        assert!(st.status.error.as_deref().unwrap().contains("503"));
        assert!(matches!(run(&r, &mut st), Err(FetchError::TooSoon { .. })));
    }

    #[test]
    fn not_modified_keeps_the_list_and_sends_the_validator() {
        let r = rig(vec![Reply::ok(b"8.8.8.8\n").header("etag", "\"v1\""), Reply::status(304)]);
        let mut st = FetchState::default();
        run(&r, &mut st).unwrap();
        r.clock.advance(Duration::from_secs(3600));
        assert_eq!(run(&r, &mut st), Ok(Outcome::NotModified));
        let sent = r.wire.seen();
        assert!(sent[1]
            .headers
            .iter()
            .any(|(n, v)| n.eq_ignore_ascii_case("if-none-match") && v == "\"v1\""));
        assert_eq!(load_entries(&r.dir.path().join("c")).len(), 1);
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
        assert!(!c.join(STAGING).exists());
    }

    #[test]
    fn binary_and_non_list_answers_are_refused_and_nothing_is_written() {
        for body in [
            &b"\x00\x01\x02"[..],
            &[0xFF, 0xFE, 0x41][..],
            b"<html><body>Not found now</body></html>\n",
        ] {
            let r = rig(vec![Reply::ok(body)]);
            let mut st = FetchState::default();
            assert_eq!(run(&r, &mut st), Err(FetchError::NotAList));
            assert!(!r.dir.path().join("c").exists());
        }
    }

    #[test]
    fn a_redirect_to_plain_http_and_an_unresolvable_name_are_refused() {
        let r = rig(vec![Reply::redirect(302, "http://lists.example/x")]);
        let mut st = FetchState::default();
        assert!(matches!(run(&r, &mut st), Err(FetchError::Net(_))));
        let r = rig(vec![]);
        let mut st = FetchState::default();
        let e = r.fetcher.update(
            "l",
            "https://intranet.example/x",
            &mut st,
            &r.dir.path().join("c"),
            &CancelToken::new(),
            &mut |_| {},
        );
        assert!(
            matches!(e, Err(FetchError::Net(_))),
            "a name the resolver does not know is not reachable"
        );
    }

    #[test]
    fn cancel_before_the_swap_leaves_the_old_list() {
        let r = rig(vec![Reply::ok(b"8.8.8.8\n"), Reply::ok(b"9.9.9.9\n")]);
        let mut st = FetchState::default();
        run(&r, &mut st).unwrap();
        r.clock.advance(Duration::from_secs(3600));
        let cancel = CancelToken::new();
        let c2 = cancel.clone();
        let e = r.fetcher.update("l", URL, &mut st, &r.dir.path().join("c"), &cancel, &mut |p| {
            if p == Phase::Check {
                c2.cancel();
            }
        });
        assert_eq!(e, Err(FetchError::Cancelled));
        assert_eq!(std::fs::read_to_string(r.dir.path().join("c").join(CURRENT)).unwrap(), "8.8.8.8\n");
    }

    #[test]
    fn the_fetch_state_round_trips_and_a_corrupt_file_gives_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(dir.path());
        let mut s = FetchStore::default();
        s.lists.insert(
            "a".into(),
            FetchState {
                etag: Some("x".into()),
                ..FetchState::default()
            },
        );
        s.save(&dirs).unwrap();
        assert_eq!(FetchStore::load(&dirs).lists["a"].etag.as_deref(), Some("x"));
        std::fs::write(dirs.intel_fetch_state(), b"{ not json").unwrap();
        assert_eq!(FetchStore::load(&dirs), FetchStore::default());
        std::fs::write(dirs.intel_fetch_state(), br#"{"version": 9}"#).unwrap();
        assert_eq!(FetchStore::load(&dirs), FetchStore::default(), "a newer version is not guessed at");
    }

    #[test]
    fn with_no_subscription_no_egress_exists() {
        // The agent builds a Fetcher only when a subscription is on; the policy alone constructs nothing.
        assert_eq!(policy().max_body, MAX_BYTES);
    }
}
