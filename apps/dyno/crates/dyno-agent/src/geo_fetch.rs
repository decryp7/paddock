//! The download of the location databases: DB-IP IP to Country Lite and IP to ASN Lite (CC-BY-4.0, no account, monthly; ADR 0031
//! §3.1 and its amendments). On by default with its own Settings switch (the caller checks the switch; the first-run note says
//! what is fetched and from where) and the DB-IP attribution stays visible ([`ATTRIBUTION`]). Everything goes through
//! `paddock-net`'s [`Egress`]: https only, the one host `download.db-ip.com`, public addresses only, a body cap.
//!
//! - **Files** live in the cache folder of ADR 0029: `<cache>/intel/geo-country/current-<yyyy-mm>.mmdb`, the previous month as
//!   `previous.mmdb`, and `meta.json`. A cleared cache means "not downloaded yet".
//! - **Before a file is used:** gunzip with a cap, then [`open_checked`] (`database_type`, `ip_version`, build date, node count,
//!   `Reader::verify`). Any failure keeps the previous file in use.
//! - **Schedule:** a database of this month is never fetched again; otherwise from the 3rd of the month, then daily until the
//!   new file appears. The floor is 24 hours between attempts (DB-IP's "1 day"), counted from the last attempt kept outside the
//!   cache ([`FetchStore`]), also for "Update now".
//! - **Logs** carry the source id, phases, durations, bytes, node counts and the kind of failure; never an address.

use crate::geo_db::{inflate_capped, open_checked, DbError, DbKind, GeoReader};
use crate::intel_fetch::{download_to, policy as base_policy, replace, FetchError, FetchState, CHECK, SWAP};
use crate::paths::Dirs;
use paddock_download::schedule::next_allowed;
use paddock_download::{Config, Downloader};
use paddock_jobs::time::Clock;
use paddock_jobs::{JobContext, Phase};
use paddock_net::{Egress, Policy, Reach, SystemLookup};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// The only host the databases are fetched from.
pub const HOST: &str = "download.db-ip.com";
/// Least time between two attempts for one database ("Update now" included).
pub const MIN_INTERVAL: Duration = Duration::from_secs(24 * 3600);
/// Largest compressed download.
pub const MAX_GZ: u64 = 32 << 20;
/// Largest unpacked size (8 times the download cap).
pub const MAX_INFLATED: u64 = MAX_GZ * 8;
/// From this day of the month a new month's file is looked for.
pub const FIRST_CHECK_DAY: u32 = 3;
/// The credit DB-IP's CC-BY-4.0 licence asks for, shown under the map and in About.
pub const ATTRIBUTION: &str = "IP Geolocation by DB-IP (https://db-ip.com), licensed CC-BY-4.0.";

/// A database to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Source {
    /// The id in [`FetchStore`] and the folder name under `intel/`.
    pub id: &'static str,
    /// Which database.
    pub kind: DbKind,
    /// What the user calls it, for messages.
    pub name: &'static str,
    stem: &'static str,
}

/// IP to Country Lite.
pub const COUNTRY: Source = Source {
    id: "geo-country",
    kind: DbKind::Country,
    name: "IP to Country database",
    stem: "dbip-country-lite",
};
/// IP to ASN Lite.
pub const ASN: Source = Source {
    id: "geo-asn",
    kind: DbKind::Asn,
    name: "IP to ASN database",
    stem: "dbip-asn-lite",
};

impl Source {
    /// The file's address for a month (`https://download.db-ip.com/free/dbip-country-lite-2026-11.mmdb.gz`).
    pub fn url(&self, year: i32, month: u32) -> String {
        format!("https://{HOST}/free/{}-{year:04}-{month:02}.mmdb.gz", self.stem)
    }
}

/// Why a database was not updated.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GeoError {
    /// The floor, the network, the server, or saving.
    #[error(transparent)]
    Fetch(#[from] FetchError),
    /// The file did not pass the checks; the previous one stays in use.
    #[error(transparent)]
    Db(#[from] DbError),
    /// Neither this month's nor last month's file exists on the server.
    #[error("The new database is not published yet. Dyno will look again tomorrow.")]
    NotPublished,
}

impl GeoError {
    /// A short kind for logs.
    pub fn kind(&self) -> &'static str {
        match self {
            GeoError::Fetch(e) => e.kind(),
            GeoError::Db(e) => e.kind(),
            GeoError::NotPublished => "not-published",
        }
    }
}

/// What an update did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeoOutcome {
    /// A database of the current month is already in use; nothing was requested.
    UpToDate,
    /// A new database is in use.
    Updated {
        /// Its month.
        year: i32,
        /// Its month.
        month: u32,
        /// Bytes downloaded.
        bytes: u64,
    },
}

/// The calendar year, month and day of a Unix time (UTC), by the days-from-civil algorithm.
pub fn civil(unix_secs: u64) -> (i32, u32, u32) {
    let days = (unix_secs / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = (yoe + era * 400) as i32 + i32::from(m <= 2);
    (y, m, d)
}

fn previous_month(y: i32, m: u32) -> (i32, u32) {
    if m == 1 {
        (y - 1, 12)
    } else {
        (y, m - 1)
    }
}

fn file_name(y: i32, m: u32) -> String {
    format!("current-{y:04}-{m:02}.mmdb")
}

/// The month of the newest `current-<yyyy-mm>.mmdb` in `cache`.
pub fn newest_month(cache: &Path) -> Option<(i32, u32)> {
    let mut best = None;
    for e in std::fs::read_dir(cache).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(rest) = name.strip_prefix("current-").and_then(|r| r.strip_suffix(".mmdb")) else {
            continue;
        };
        let Some((y, m)) = rest.split_once('-') else { continue };
        if let (Ok(y), Ok(m)) = (y.parse::<i32>(), m.parse::<u32>()) {
            if (1..=12).contains(&m) && best.is_none_or(|b| (y, m) > b) {
                best = Some((y, m));
            }
        }
    }
    best
}

/// Whether an automatic check is due: nothing is in use yet, or the month moved on, it is the 3rd or later, and the floor has
/// passed since the last attempt.
pub fn automatic_due(now_secs: u64, last_attempt_ms: Option<i64>, have: Option<(i32, u32)>) -> bool {
    let now_ms = i64::try_from(now_secs.saturating_mul(1000)).unwrap_or(i64::MAX);
    if next_allowed(last_attempt_ms, now_ms, MIN_INTERVAL).is_some() {
        return false;
    }
    let (y, m, d) = civil(now_secs);
    match have {
        None => true,
        Some(h) => h < (y, m) && d >= FIRST_CHECK_DAY,
    }
}

/// The policy of the database door: the one host, public addresses only.
pub fn policy() -> Policy {
    let mut p: Policy = base_policy()
        .reach(Reach::Public)
        .max_redirects(2)
        .max_body(MAX_GZ)
        .timeout(Duration::from_secs(300));
    p = p.allow_hosts(&[HOST]);
    p
}

/// Downloads and checks the databases.
pub struct GeoFetcher {
    downloader: Downloader,
    clock: Arc<dyn Clock>,
}

impl GeoFetcher {
    /// Over a given door (tests pass a fake wire).
    pub fn new(egress: Arc<Egress>, clock: Arc<dyn Clock>, config: Config) -> GeoFetcher {
        GeoFetcher {
            downloader: Downloader::new(Arc::new(egress.download_transport())).with_config(config),
            clock,
        }
    }

    /// The real door; built only while the map's data switch is on.
    pub fn real(clock: Arc<dyn Clock>) -> Result<GeoFetcher, GeoError> {
        let p = policy();
        let wire = paddock_net::UreqWire::new(&p, paddock_download::ProxyMode::System).map_err(|e| FetchError::Net(e.to_string()))?;
        let egress = Egress::new(p, Arc::new(wire), Arc::new(SystemLookup), clock.clone());
        Ok(GeoFetcher::new(Arc::new(egress), clock, Config::default()))
    }

    /// Updates one database. `state` is changed in place (the caller saves it); `cache` is [`Dirs::intel_cache`] of the source.
    pub fn update(&self, source: Source, state: &mut FetchState, cache: &Path, ctx: &JobContext) -> Result<GeoOutcome, GeoError> {
        let started = self.clock.monotonic();
        let result = self.run(source, state, cache, ctx);
        let ms = self.clock.monotonic().saturating_sub(started).as_millis();
        match &result {
            Ok(GeoOutcome::UpToDate) => log::info!(target: "dyno::geo", "database check: id={} outcome=up-to-date", source.id),
            Ok(GeoOutcome::Updated { bytes, .. }) => {
                log::info!(target: "dyno::geo", "database update: id={} outcome=updated bytes={bytes} duration_ms={ms}", source.id)
            }
            Err(e) => {
                let declared = match e {
                    GeoError::Db(d) => d
                        .declared_type()
                        .map(|t| format!(" declared_type={t:?} expected={:?}", source.kind.database_type())),
                    _ => None,
                };
                log::warn!(target: "dyno::geo", "database update failed: id={} kind={} duration_ms={ms}{}", source.id, e.kind(), declared.unwrap_or_default())
            }
        }
        if let Err(e) = &result {
            state.status.error = Some(format!("{}: {e}", source.name));
        }
        result
    }

    fn run(&self, source: Source, state: &mut FetchState, cache: &Path, ctx: &JobContext) -> Result<GeoOutcome, GeoError> {
        let now_ms = self.clock.wall_ms();
        let now_secs = u64::try_from(now_ms / 1000).unwrap_or(0);
        let (y, m, _) = civil(now_secs);
        if cache.join(file_name(y, m)).is_file() {
            return Ok(GeoOutcome::UpToDate);
        }
        if let Some(at) = next_allowed(state.status.last_attempt_ms, now_ms, MIN_INTERVAL) {
            return Err(FetchError::TooSoon { next_allowed_ms: at }.into());
        }
        state.status.last_attempt_ms = Some(now_ms);

        let staging = cache.join("download.gz");
        let mut found = None;
        for (cy, cm) in [(y, m), previous_month(y, m)] {
            if cache.join(file_name(cy, cm)).is_file() {
                // Last month's file is in use and this month's is not published yet.
                return Err(GeoError::NotPublished);
            }
            match download_to(&self.downloader, ctx, &source.url(cy, cm), &staging, MAX_GZ) {
                Ok(bytes) => {
                    found = Some((cy, cm, bytes));
                    break;
                }
                Err(FetchError::Http(404)) => continue,
                Err(e) => {
                    let _ = std::fs::remove_file(&staging);
                    return Err(e.into());
                }
            }
        }
        let Some((fy, fm, bytes)) = found else {
            return Err(GeoError::NotPublished);
        };

        ctx.phase(Phase::items(CHECK, "Checking the database", 1));
        let checked = (|| {
            let gz = std::fs::read(&staging).map_err(|e| FetchError::Save(e.kind().to_string()))?;
            let digest = hex(&Sha256::digest(&gz));
            let raw = inflate_capped(&gz, MAX_INFLATED)?;
            let (_reader, info) = open_checked(raw.clone(), source.kind, now_secs, true)?;
            Ok::<_, GeoError>((digest, raw, info))
        })();
        let _ = std::fs::remove_file(&staging);
        let (digest, raw, info) = checked?;
        if ctx.cancel_token().is_cancelled() {
            return Err(FetchError::Cancelled.into());
        }

        ctx.phase(Phase::items(SWAP, "Replacing the database", 1));
        swap_in(cache, fy, fm, &raw).map_err(|e| FetchError::Save(e.kind().to_string()))?;
        let meta = format!(
            "{{\"source\":\"{}\",\"fetchedMs\":{now_ms},\"month\":\"{fy:04}-{fm:02}\",\"sha256\":\"{digest}\",\"buildEpoch\":{},\"nodeCount\":{},\"format\":1}}\n",
            source.id, info.build_epoch, info.node_count
        );
        let _ = std::fs::write(cache.join("meta.json"), meta);
        state.status.last_fetched_ms = Some(now_ms);
        state.status.error = None;
        state.status.note = Some(format!("Database of {fy:04}-{fm:02}"));
        Ok(GeoOutcome::Updated {
            year: fy,
            month: fm,
            bytes,
        })
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Writes the staging file, moves the older `current-*` files aside (the newest becomes `previous.mmdb`, the rest go) and puts the
/// new file in place.
fn swap_in(cache: &Path, y: i32, m: u32, raw: &[u8]) -> std::io::Result<()> {
    paddock_env::create_private_dir_all(cache)?;
    let staging = cache.join(".staging");
    // A unique temporary file created exclusive and owner-only (0600 from creation, no chmod), then renamed over `.staging`.
    paddock_env::write_atomic(&staging, raw, true).map_err(std::io::Error::other)?;
    let mut older: Vec<PathBuf> = std::fs::read_dir(cache)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("current-") && n.ends_with(".mmdb") && n != file_name(y, m))
        })
        .collect();
    older.sort();
    if let Some(newest) = older.pop() {
        replace(&newest, &cache.join("previous.mmdb"))?;
    }
    for p in older {
        let _ = std::fs::remove_file(p);
    }
    replace(&staging, &cache.join(file_name(y, m)))
}

/// Opens the databases on disk for lookups: the newest `current-*` file of each source, else `previous.mmdb`, using the cheap
/// check (the full one ran before the swap). A missing or unusable file means that database is off.
pub fn load_reader(dirs: &Dirs, now_secs: u64) -> GeoReader {
    let open = |source: Source| {
        let cache = dirs.intel_cache(source.id);
        let mut names: Vec<PathBuf> = newest_month(&cache).map(|(y, m)| cache.join(file_name(y, m))).into_iter().collect();
        names.push(cache.join("previous.mmdb"));
        names.into_iter().find_map(|p| {
            let bytes = std::fs::read(&p).ok().filter(|b| b.len() as u64 <= MAX_INFLATED)?;
            open_checked(bytes, source.kind, now_secs, false).ok()
        })
    };
    GeoReader::new(open(COUNTRY), open(ASN))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo_db::testdb::{sample_asn, sample_country};
    use dyno_core::geo::GeoLookup;
    use flate2::{write::GzEncoder, Compression};
    use paddock_jobs::time::FakeClock;
    use paddock_net::{FakeWire, Reply, StaticLookup};
    use std::io::Write;

    // 2026-11-03 12:00 UTC.
    const NOW_MS: i64 = 1_793_707_200_000;
    const NOW: u64 = (NOW_MS / 1000) as u64;
    const DAY: Duration = Duration::from_secs(86_400);

    fn gz(bytes: &[u8]) -> Vec<u8> {
        let mut e = GzEncoder::new(Vec::new(), Compression::fast());
        e.write_all(bytes).unwrap();
        e.finish().unwrap()
    }

    struct Rig {
        fetcher: GeoFetcher,
        wire: Arc<FakeWire>,
        clock: FakeClock,
        dir: tempfile::TempDir,
    }

    fn rig(replies: Vec<Reply>) -> Rig {
        let clock = FakeClock::at_wall_ms(NOW_MS);
        let mut wire = FakeWire::new();
        for r in replies {
            wire = wire.then(r);
        }
        let wire = Arc::new(wire);
        let lookup = Arc::new(StaticLookup::new().with(HOST, &["93.184.216.34"]));
        let egress = Egress::new(policy(), wire.clone(), lookup, Arc::new(clock.clone()));
        Rig {
            fetcher: GeoFetcher::new(Arc::new(egress), Arc::new(clock.clone()), crate::intel_fetch::tests::config()),
            wire,
            clock,
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn go(r: &Rig, src: Source, st: &mut FetchState) -> Result<GeoOutcome, GeoError> {
        r.fetcher
            .update(src, st, &r.dir.path().join("c"), &crate::intel_fetch::tests::ctx())
    }

    #[test]
    fn civil_dates_and_urls() {
        assert_eq!(civil(NOW), (2026, 11, 3));
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(951_782_400), (2000, 2, 29));
        assert_eq!(
            COUNTRY.url(2026, 11),
            "https://download.db-ip.com/free/dbip-country-lite-2026-11.mmdb.gz"
        );
        assert_eq!(ASN.url(2026, 1), "https://download.db-ip.com/free/dbip-asn-lite-2026-01.mmdb.gz");
        assert_eq!(previous_month(2026, 1), (2025, 12));
    }

    #[test]
    fn a_good_download_is_checked_swapped_in_and_answers_lookups() {
        let r = rig(vec![Reply::ok(&gz(&sample_country(NOW - 3600)))]);
        let mut st = FetchState::default();
        let o = go(&r, COUNTRY, &mut st);
        assert!(matches!(o, Ok(GeoOutcome::Updated { year: 2026, month: 11, .. })), "{o:?}");
        assert!(!r.dir.path().join("c/download.gz").exists(), "no staging file is left");
        assert_eq!(r.wire.seen()[0].url, COUNTRY.url(2026, 11));
        assert!(r.dir.path().join("c/current-2026-11.mmdb").is_file());
        assert!(std::fs::read_to_string(r.dir.path().join("c/meta.json"))
            .unwrap()
            .contains("\"month\":\"2026-11\""));
        assert_eq!(st.status.last_fetched_ms, Some(NOW_MS));
        // The files are read back through the dirs, as the agent does at start.
        let dirs = Dirs::at(r.dir.path().join("d"));
        let cache = dirs.intel_cache(COUNTRY.id);
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::copy(r.dir.path().join("c/current-2026-11.mmdb"), cache.join("current-2026-11.mmdb")).unwrap();
        let g = load_reader(&dirs, NOW);
        assert_eq!(g.lookup(&"192.0.2.9".parse().unwrap()).unwrap().country, "XA");
    }

    #[test]
    fn this_months_database_is_not_requested_again() {
        let r = rig(vec![Reply::ok(&gz(&sample_country(NOW))), Reply::ok(&gz(&sample_country(NOW)))]);
        let mut st = FetchState::default();
        go(&r, COUNTRY, &mut st).unwrap();
        r.clock.advance(DAY * 2);
        assert_eq!(go(&r, COUNTRY, &mut st), Ok(GeoOutcome::UpToDate));
        assert_eq!(r.wire.sends(), 1);
    }

    #[test]
    fn update_now_inside_the_daily_floor_sends_nothing_and_a_cleared_cache_does_not_reset_it() {
        let r = rig(vec![Reply::status(404), Reply::status(404)]);
        let mut st = FetchState::default();
        assert_eq!(go(&r, COUNTRY, &mut st), Err(GeoError::NotPublished));
        assert_eq!(r.wire.sends(), 2, "this month, then last month");
        r.clock.advance(Duration::from_secs(3600));
        let e = go(&r, COUNTRY, &mut st).unwrap_err();
        assert_eq!(
            e,
            GeoError::Fetch(FetchError::TooSoon {
                next_allowed_ms: NOW_MS + 24 * 3_600_000
            })
        );
        assert_eq!(r.wire.sends(), 2);
    }

    #[test]
    fn last_months_file_is_tried_when_this_months_is_not_published_yet() {
        let r = rig(vec![Reply::status(404), Reply::ok(&gz(&sample_country(NOW - 5 * 86_400)))]);
        let mut st = FetchState::default();
        let o = go(&r, COUNTRY, &mut st).unwrap();
        assert!(matches!(o, GeoOutcome::Updated { year: 2026, month: 10, .. }));
        assert_eq!(r.wire.seen()[1].url, COUNTRY.url(2026, 10));
    }

    #[test]
    fn a_bad_file_keeps_the_previous_one_and_swaps_nothing() {
        let r = rig(vec![
            Reply::ok(&gz(&sample_asn(NOW))),
            Reply::ok(b"plain text"),
            Reply::ok(&gz(&[0u8; 100])),
        ]);
        let c = r.dir.path().join("c");
        std::fs::create_dir_all(&c).unwrap();
        std::fs::write(c.join("current-2026-10.mmdb"), sample_country(NOW - 10 * 86_400)).unwrap();
        let mut st = FetchState::default();
        assert!(matches!(go(&r, COUNTRY, &mut st), Err(GeoError::Db(DbError::WrongType(_)))));
        assert!(st.status.error.is_some());
        r.clock.advance(DAY);
        assert_eq!(go(&r, COUNTRY, &mut st), Err(GeoError::Db(DbError::NotGzip)));
        r.clock.advance(DAY);
        assert!(matches!(go(&r, COUNTRY, &mut st), Err(GeoError::Db(DbError::Invalid(_)))));
        assert!(c.join("current-2026-10.mmdb").is_file(), "last month's database stays in use");
        assert!(!c.join("current-2026-11.mmdb").exists());
        assert!(!c.join(".staging").exists());
    }

    #[test]
    fn a_swap_keeps_the_previous_month_aside() {
        let r = rig(vec![Reply::ok(&gz(&sample_country(NOW)))]);
        let c = r.dir.path().join("c");
        std::fs::create_dir_all(&c).unwrap();
        std::fs::write(c.join("current-2026-10.mmdb"), b"old october").unwrap();
        std::fs::write(c.join("current-2026-09.mmdb"), b"old september").unwrap();
        let mut st = FetchState::default();
        go(&r, COUNTRY, &mut st).unwrap();
        assert_eq!(std::fs::read(c.join("previous.mmdb")).unwrap(), b"old october");
        assert!(!c.join("current-2026-09.mmdb").exists() && !c.join("current-2026-10.mmdb").exists());
        assert_eq!(newest_month(&c), Some((2026, 11)));
    }

    #[test]
    fn a_redirect_to_another_host_is_refused() {
        let r = rig(vec![Reply::redirect(302, "https://evil.example/x.mmdb.gz")]);
        let mut st = FetchState::default();
        assert!(matches!(go(&r, COUNTRY, &mut st), Err(GeoError::Fetch(FetchError::Net(_)))));
        assert_eq!(r.wire.sends(), 1);
    }

    #[test]
    fn the_automatic_check_waits_for_the_third_and_for_the_floor() {
        let day = |d: u64| NOW - 2 * 86_400 + d * 86_400; // d = 0 is 2026-11-01 12:00
        assert!(automatic_due(day(0), None, None), "nothing in use: fetch at once");
        assert!(
            !automatic_due(day(0), None, Some((2026, 10))),
            "the 1st: the new file is not looked for yet"
        );
        assert!(automatic_due(day(2), None, Some((2026, 10))), "the 3rd");
        assert!(!automatic_due(day(2), None, Some((2026, 11))), "this month's is in use");
        let last = Some(i64::try_from(day(2) * 1000).unwrap());
        assert!(!automatic_due(day(2) + 3600, last, Some((2026, 10))), "inside the floor");
        assert!(automatic_due(day(3), last, Some((2026, 10))), "daily until the file appears");
    }
}
