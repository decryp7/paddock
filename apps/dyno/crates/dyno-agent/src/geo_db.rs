//! The downloaded geo databases (DB-IP IP to Country Lite and IP to ASN Lite, MMDB format; ADR 0031 §3.1): opening and checking
//! one, and the [`GeoLookup`] over the two. The bytes come from the network, so a file is used only after
//! [`open_checked`] accepted it: `database_type`, `ip_version`, a recent build date, a node count within bounds, and the crate's
//! full structural check of the search tree and the data section ([`maxminddb::Reader::verify`]). The files are read into
//! memory (D10), never mapped. A failure keeps the previous month's file.

use dyno_core::geo::{GeoLookup, GeoTag};
use flate2::read::GzDecoder;
use maxminddb::{geoip2, Reader};
use std::io::Read;
use std::net::IpAddr;

/// A database older than this is refused (DB-IP publishes monthly), in seconds.
pub const MAX_AGE_SECS: u64 = 62 * 24 * 3600;
/// A build date this far in the future is refused (clock skew allowance), in seconds.
pub const MAX_FUTURE_SECS: u64 = 24 * 3600;
/// Largest search tree accepted (DB-IP's country file has under 2 million nodes).
pub const MAX_NODES: u32 = 20_000_000;

/// Which database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbKind {
    /// IP to Country Lite.
    Country,
    /// IP to ASN Lite.
    Asn,
}

impl DbKind {
    /// The `database_type` the file must declare.
    pub fn database_type(self) -> &'static str {
        match self {
            DbKind::Country => "DBIP-Country-Lite",
            DbKind::Asn => "DBIP-ASN-Lite",
        }
    }
}

/// Why a database was not used (a message that says what happens next; none contains file content).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DbError {
    /// Not a readable MMDB file.
    #[error("The downloaded file is not a valid location database ({0}); the previous one stays in use.")]
    Invalid(String),
    /// Another database than expected.
    #[error("The downloaded file is a different kind of database; the previous one stays in use.")]
    WrongType,
    /// Not an IPv6-capable tree.
    #[error("The downloaded database has an unexpected layout; the previous one stays in use.")]
    WrongLayout,
    /// Build date too old or in the future.
    #[error("The downloaded database has an implausible build date; the previous one stays in use.")]
    BadDate,
    /// Tree size out of bounds.
    #[error("The downloaded database has an implausible size; the previous one stays in use.")]
    BadSize,
    /// Larger than the cap once unpacked.
    #[error("The downloaded file is larger than allowed once unpacked; the previous database stays in use.")]
    TooBig,
    /// Not gzip.
    #[error("The downloaded file is not a valid archive; the previous database stays in use.")]
    NotGzip,
}

/// What an accepted database says about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DbInfo {
    /// Build time, seconds since the Unix epoch.
    pub build_epoch: u64,
    /// Nodes in the search tree.
    pub node_count: u32,
}

/// Unpacks gzip, refusing more than `cap` bytes of output (a decompression bomb).
pub fn inflate_capped(gz: &[u8], cap: u64) -> Result<Vec<u8>, DbError> {
    let mut out = Vec::new();
    GzDecoder::new(gz)
        .take(cap + 1)
        .read_to_end(&mut out)
        .map_err(|_| DbError::NotGzip)?;
    if out.len() as u64 > cap {
        return Err(DbError::TooBig);
    }
    Ok(out)
}

/// Opens `bytes` as a database of `kind` and checks it at `now` (seconds since the epoch). With `full`, also runs the crate's
/// complete structural check (slow on a real file: do it once, before a download is swapped in; the cheap check suffices for
/// opening a file this agent already verified).
pub fn open_checked(bytes: Vec<u8>, kind: DbKind, now: u64, full: bool) -> Result<(Reader<Vec<u8>>, DbInfo), DbError> {
    // `maxminddb` 0.27.3 does unchecked arithmetic on pointers in a damaged file (`reader.rs` `resolve_data_pointer`): a panic in
    // a debug build, a wrapped value that its next bounds check rejects in a release build. A panic must never reach the agent.
    guarded(|| open_unguarded(bytes, kind, now, full)).unwrap_or_else(|| Err(DbError::Invalid("damaged file".into())))
}

/// Runs `f`, turning a panic into `None`.
fn guarded<T>(f: impl FnOnce() -> T) -> Option<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).ok()
}

fn open_unguarded(bytes: Vec<u8>, kind: DbKind, now: u64, full: bool) -> Result<(Reader<Vec<u8>>, DbInfo), DbError> {
    let info = {
        let reader = Reader::from_source(bytes.as_slice()).map_err(|e| DbError::Invalid(short(&e.to_string())))?;
        let m = &reader.metadata;
        if m.database_type != kind.database_type() {
            return Err(DbError::WrongType);
        }
        if m.ip_version != 6 || m.record_size != 24 && m.record_size != 28 && m.record_size != 32 {
            return Err(DbError::WrongLayout);
        }
        if m.build_epoch > now.saturating_add(MAX_FUTURE_SECS) || m.build_epoch.saturating_add(MAX_AGE_SECS) < now {
            return Err(DbError::BadDate);
        }
        if m.node_count == 0 || m.node_count > MAX_NODES {
            return Err(DbError::BadSize);
        }
        if full {
            // `Reader::verify` follows the tree without a cycle guard: a record that points back up (a damaged or hostile file)
            // makes it loop and grow without end. Prove the tree is acyclic first, with a bounded walk of our own.
            let tree_len = (m.node_count as usize).saturating_mul(m.record_size as usize) / 4;
            if tree_len > bytes.len() || tree_has_cycle(&bytes[..tree_len], m.node_count, m.record_size) {
                return Err(DbError::Invalid("the search tree is damaged".into()));
            }
            reader.verify().map_err(|e| DbError::Invalid(short(&e.to_string())))?;
        }
        DbInfo {
            build_epoch: m.build_epoch,
            node_count: m.node_count,
        }
    };
    let reader = Reader::from_source(bytes).map_err(|e| DbError::Invalid(short(&e.to_string())))?;
    Ok((reader, info))
}

/// The two records of node `n` in a search tree of `record_size` bits per record.
fn records(tree: &[u8], n: usize, record_size: u16) -> Option<[u32; 2]> {
    let b = |i: usize| tree.get(i).copied().map(u32::from);
    match record_size {
        24 => {
            let o = n.checked_mul(6)?;
            Some([
                b(o)? << 16 | b(o + 1)? << 8 | b(o + 2)?,
                b(o + 3)? << 16 | b(o + 4)? << 8 | b(o + 5)?,
            ])
        }
        28 => {
            let o = n.checked_mul(7)?;
            let mid = b(o + 3)?;
            Some([
                (mid & 0xF0) << 20 | b(o)? << 16 | b(o + 1)? << 8 | b(o + 2)?,
                (mid & 0x0F) << 24 | b(o + 4)? << 16 | b(o + 5)? << 8 | b(o + 6)?,
            ])
        }
        32 => {
            let o = n.checked_mul(8)?;
            let r = |i: usize| Some(b(i)? << 24 | b(i + 1)? << 16 | b(i + 2)? << 8 | b(i + 3)?);
            Some([r(o)?, r(o + 4)?])
        }
        _ => None,
    }
}

/// Whether the search tree has a cycle or is deeper than an IPv6 address (128 bits). A node reached by two paths is fine (an
/// IPv4 subtree may be aliased); a node reached again while still on the current path is not. One pass, `O(nodes)` time, one
/// byte per node.
fn tree_has_cycle(tree: &[u8], node_count: u32, record_size: u16) -> bool {
    const ACTIVE: u8 = 1;
    const DONE: u8 = 2;
    let mut state = vec![0u8; node_count as usize];
    // (node, next record to look at)
    let mut stack: Vec<(u32, u8)> = vec![(0, 0)];
    state[0] = ACTIVE;
    while let Some(&(node, next)) = stack.last() {
        if next == 2 {
            state[node as usize] = DONE;
            stack.pop();
            continue;
        }
        if let Some(top) = stack.last_mut() {
            top.1 += 1;
        }
        let Some(recs) = records(tree, node as usize, record_size) else {
            return true;
        };
        let child = recs[usize::from(next)];
        if child >= node_count {
            continue; // empty (== node_count) or a data pointer: verify() checks those
        }
        match state[child as usize] {
            ACTIVE => return true,
            DONE => {}
            _ => {
                if stack.len() > 128 {
                    return true;
                }
                state[child as usize] = ACTIVE;
                stack.push((child, 0));
            }
        }
    }
    false
}

/// The first 80 characters of a library message (it may quote file offsets, never addresses).
fn short(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(80).collect()
}

/// The country (and, when present, the ASN) database behind [`GeoLookup`].
#[derive(Default)]
pub struct GeoReader {
    country: Option<Reader<Vec<u8>>>,
    asn: Option<Reader<Vec<u8>>>,
    /// Build time of the country database, seconds since the epoch.
    pub built: Option<u64>,
}

impl GeoReader {
    /// A reader over the given databases (either may be missing).
    pub fn new(country: Option<(Reader<Vec<u8>>, DbInfo)>, asn: Option<(Reader<Vec<u8>>, DbInfo)>) -> GeoReader {
        GeoReader {
            built: country.as_ref().map(|(_, i)| i.build_epoch),
            country: country.map(|c| c.0),
            asn: asn.map(|a| a.0),
        }
    }

    /// Whether any database is loaded.
    pub fn is_on(&self) -> bool {
        self.country.is_some() || self.asn.is_some()
    }
}

impl GeoLookup for GeoReader {
    fn lookup(&self, ip: &IpAddr) -> Option<GeoTag> {
        // A file that passed the full check cannot panic here; one opened with the cheap check might be damaged on disk.
        guarded(|| self.lookup_unguarded(ip)).flatten()
    }
}

impl GeoReader {
    fn lookup_unguarded(&self, ip: &IpAddr) -> Option<GeoTag> {
        let (mut country, mut country_name) = (None, None);
        if let Some(db) = &self.country {
            if let Ok(res) = db.lookup(*ip) {
                if let Ok(Some(rec)) = res.decode::<geoip2::Country>() {
                    country = rec
                        .country
                        .iso_code
                        .filter(|c| c.len() == 2 && c.bytes().all(|b| b.is_ascii_alphabetic()))
                        .map(str::to_ascii_uppercase);
                    country_name = rec
                        .country
                        .names
                        .english
                        .map(|n| n.chars().filter(|c| !c.is_control()).take(80).collect::<String>());
                }
            }
        }
        let (mut asn, mut as_name) = (None, None);
        if let Some(db) = &self.asn {
            if let Ok(res) = db.lookup(*ip) {
                if let Ok(Some(rec)) = res.decode::<geoip2::Asn>() {
                    asn = rec.autonomous_system_number;
                    as_name = rec
                        .autonomous_system_organization
                        .map(|n| n.chars().filter(|c| !c.is_control()).take(120).collect::<String>());
                }
            }
        }
        // Only the ASN database on: the network is known, the country is not, so there is nothing to place on the map.
        country.map(|country| GeoTag {
            country,
            country_name,
            asn,
            as_name,
        })
    }
}

/// A writer of tiny MMDB files in the published format (<https://maxmind.github.io/MaxMind-DB/>), for tests and the fuzz seed.
#[cfg(any(test, feature = "testdb"))]
#[allow(clippy::unwrap_used)] // a test helper: it panics on a wrong fixture
pub mod testdb {
    use std::net::IpAddr;

    /// A value in the data section.
    #[derive(Debug, Clone)]
    pub enum V {
        /// UTF-8 string.
        S(String),
        /// Unsigned 16-bit.
        U16(u16),
        /// Unsigned 32-bit.
        U32(u32),
        /// Unsigned 64-bit.
        U64(u64),
        /// Map.
        M(Vec<(String, V)>),
        /// Array.
        A(Vec<V>),
    }

    fn ctrl(out: &mut Vec<u8>, ty: u8, size: usize) {
        let (t, ext) = if ty > 7 { (0u8, Some(ty - 7)) } else { (ty, None) };
        assert!(size < 29 + 256, "the test writer keeps values small");
        let (small, more) = if size < 29 {
            (size as u8, None)
        } else {
            (29, Some((size - 29) as u8))
        };
        out.push((t << 5) | small);
        if let Some(e) = ext {
            out.push(e);
        }
        out.extend(more);
    }

    fn uint(out: &mut Vec<u8>, ty: u8, v: u64) {
        let bytes: Vec<u8> = v.to_be_bytes().iter().copied().skip_while(|b| *b == 0).collect();
        ctrl(out, ty, bytes.len());
        out.extend(bytes);
    }

    impl V {
        /// Encodes the value.
        pub fn encode(&self, out: &mut Vec<u8>) {
            match self {
                V::S(s) => {
                    ctrl(out, 2, s.len());
                    out.extend(s.as_bytes());
                }
                V::U16(v) => uint(out, 5, u64::from(*v)),
                V::U32(v) => uint(out, 6, u64::from(*v)),
                V::U64(v) => uint(out, 9, *v),
                V::M(m) => {
                    ctrl(out, 7, m.len());
                    for (k, v) in m {
                        V::S(k.clone()).encode(out);
                        v.encode(out);
                    }
                }
                V::A(a) => {
                    ctrl(out, 11, a.len());
                    for v in a {
                        v.encode(out);
                    }
                }
            }
        }
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Rec {
        Empty,
        Node(usize),
        Data(usize),
    }

    /// What to put in the metadata.
    #[derive(Debug, Clone)]
    pub struct Meta {
        /// `database_type`.
        pub database_type: String,
        /// `build_epoch`.
        pub build_epoch: u64,
        /// `ip_version`.
        pub ip_version: u16,
    }

    /// Builds a database: `entries` are `(address, prefix length, record)`; IPv4 prefixes live under `::/96` as the format says.
    pub fn build(meta: &Meta, entries: &[(IpAddr, u8, V)]) -> Vec<u8> {
        let mut nodes: Vec<[Rec; 2]> = vec![[Rec::Empty; 2]];
        let mut data = Vec::new();
        for (ip, prefix, record) in entries {
            let (bits, len): (u128, u32) = match ip {
                IpAddr::V4(a) => (u128::from(u32::from(*a)), 96 + u32::from(*prefix)),
                IpAddr::V6(a) => (u128::from(*a), u32::from(*prefix)),
            };
            let offset = data.len();
            record.encode(&mut data);
            let mut at = 0usize;
            for i in 0..len {
                let bit = ((bits >> (127 - i)) & 1) as usize;
                if i + 1 == len {
                    nodes[at][bit] = Rec::Data(offset);
                } else {
                    at = match nodes[at][bit] {
                        Rec::Node(n) => n,
                        _ => {
                            nodes.push([Rec::Empty; 2]);
                            let n = nodes.len() - 1;
                            nodes[at][bit] = Rec::Node(n);
                            n
                        }
                    };
                }
            }
        }
        let count = nodes.len() as u32;
        let mut out = Vec::new();
        for n in &nodes {
            for r in n {
                let v = match r {
                    Rec::Empty => count,
                    Rec::Node(i) => *i as u32,
                    Rec::Data(o) => count + 16 + *o as u32,
                };
                out.extend(&v.to_be_bytes()[1..]);
            }
        }
        out.extend([0u8; 16]);
        out.extend(data);
        out.extend(b"\xab\xcd\xefMaxMind.com");
        let md = V::M(vec![
            ("binary_format_major_version".into(), V::U16(2)),
            ("binary_format_minor_version".into(), V::U16(0)),
            ("build_epoch".into(), V::U64(meta.build_epoch)),
            ("database_type".into(), V::S(meta.database_type.clone())),
            ("description".into(), V::M(vec![("en".into(), V::S("Test".into()))])),
            ("ip_version".into(), V::U16(meta.ip_version)),
            ("languages".into(), V::A(vec![V::S("en".into())])),
            ("node_count".into(), V::U32(count)),
            ("record_size".into(), V::U16(24)),
        ]);
        md.encode(&mut out);
        out
    }

    /// A country record.
    pub fn country(code: &str, name: &str) -> V {
        V::M(vec![(
            "country".into(),
            V::M(vec![
                ("iso_code".into(), V::S(code.into())),
                ("names".into(), V::M(vec![("en".into(), V::S(name.into()))])),
            ]),
        )])
    }

    /// An ASN record.
    pub fn asn(number: u32, org: &str) -> V {
        V::M(vec![
            ("autonomous_system_number".into(), V::U32(number)),
            ("autonomous_system_organization".into(), V::S(org.into())),
        ])
    }

    /// A small valid country database: documentation ranges mapped to invented countries.
    pub fn sample_country(build_epoch: u64) -> Vec<u8> {
        build(
            &Meta {
                database_type: "DBIP-Country-Lite".into(),
                build_epoch,
                ip_version: 6,
            },
            &[
                ("192.0.2.0".parse().unwrap(), 24, country("XA", "Testland")),
                ("198.51.100.0".parse().unwrap(), 24, country("XB", "Exampleland")),
                ("2001:db8::".parse().unwrap(), 32, country("XC", "Docland")),
            ],
        )
    }

    /// A small valid ASN database over the same ranges.
    pub fn sample_asn(build_epoch: u64) -> Vec<u8> {
        build(
            &Meta {
                database_type: "DBIP-ASN-Lite".into(),
                build_epoch,
                ip_version: 6,
            },
            &[("192.0.2.0".parse().unwrap(), 24, asn(64500, "Example Net"))],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::testdb::*;
    use super::*;
    use proptest::prelude::*;

    const NOW: u64 = 1_800_000_000;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn a_valid_file_is_accepted_and_answers_for_the_documentation_ranges() {
        let (reader, info) = open_checked(sample_country(NOW - 3600), DbKind::Country, NOW, true).unwrap();
        assert_eq!(info.build_epoch, NOW - 3600);
        let (asn, asn_info) = open_checked(sample_asn(NOW - 3600), DbKind::Asn, NOW, true).unwrap();
        let g = GeoReader::new(Some((reader, info)), Some((asn, asn_info)));
        assert!(g.is_on());
        let t = g.lookup(&ip("192.0.2.77")).unwrap();
        assert_eq!(
            (t.country.as_str(), t.country_name.as_deref(), t.asn, t.as_name.as_deref()),
            ("XA", Some("Testland"), Some(64500), Some("Example Net"))
        );
        assert_eq!(g.lookup(&ip("198.51.100.5")).unwrap().country, "XB");
        assert_eq!(g.lookup(&ip("2001:db8::1")).unwrap().country, "XC");
        assert_eq!(g.lookup(&ip("203.0.113.9")), None, "not in the file");
        assert_eq!(g.built, Some(NOW - 3600));
    }

    #[test]
    fn a_wrong_type_layout_date_or_size_is_refused() {
        let ok = |m: Meta| build(&m, &[("192.0.2.0".parse().unwrap(), 24, country("XA", "T"))]);
        let m = Meta {
            database_type: "DBIP-Country-Lite".into(),
            build_epoch: NOW,
            ip_version: 6,
        };
        let e = |bytes, kind| open_checked(bytes, kind, NOW, true).err().unwrap();
        assert_eq!(e(ok(m.clone()), DbKind::Asn), DbError::WrongType);
        assert_eq!(
            e(
                ok(Meta {
                    database_type: "GeoLite2-City".into(),
                    ..m.clone()
                }),
                DbKind::Country
            ),
            DbError::WrongType
        );
        assert_eq!(
            e(
                ok(Meta {
                    ip_version: 4,
                    ..m.clone()
                }),
                DbKind::Country
            ),
            DbError::WrongLayout
        );
        assert_eq!(
            e(
                ok(Meta {
                    build_epoch: NOW - MAX_AGE_SECS - 1,
                    ..m.clone()
                }),
                DbKind::Country
            ),
            DbError::BadDate
        );
        assert_eq!(
            e(
                ok(Meta {
                    build_epoch: NOW + MAX_FUTURE_SECS + 1,
                    ..m.clone()
                }),
                DbKind::Country
            ),
            DbError::BadDate
        );
        assert!(
            open_checked(
                ok(Meta {
                    build_epoch: NOW - MAX_AGE_SECS,
                    ..m
                }),
                DbKind::Country,
                NOW,
                true
            )
            .is_ok(),
            "62 days is the edge"
        );
    }

    #[test]
    fn garbage_a_truncated_file_and_a_broken_pointer_are_refused() {
        assert!(matches!(
            open_checked(b"not a database".to_vec(), DbKind::Country, NOW, true),
            Err(DbError::Invalid(_))
        ));
        assert!(matches!(
            open_checked(Vec::new(), DbKind::Country, NOW, true),
            Err(DbError::Invalid(_))
        ));
        let good = sample_country(NOW);
        // The tree is the first bytes; make the first record of the first node point far outside the file.
        let mut broken = good.clone();
        broken[0..3].copy_from_slice(&[0xFF, 0xFF, 0xFF]);
        // Opening may succeed (the cheap check) but the full check must not.
        assert!(open_checked(broken.clone(), DbKind::Country, NOW, true).is_err());
    }

    #[test]
    fn a_search_tree_that_points_back_up_is_refused_instead_of_looping() {
        // Byte 2 of the first node is the low byte of its left record: 0 makes the root its own child. `Reader::verify` alone
        // never returns on such a file (found by this scan; it grows without end).
        let mut cyclic = sample_country(NOW);
        cyclic[0..3].copy_from_slice(&[0, 0, 0]);
        let e = open_checked(cyclic, DbKind::Country, NOW, true).err().unwrap();
        assert_eq!(e, DbError::Invalid("the search tree is damaged".into()));
        // A longer cycle: node 1 pointing back to node 0.
        let mut cyclic = sample_country(NOW);
        cyclic[6..9].copy_from_slice(&[0, 0, 0]);
        assert!(open_checked(cyclic, DbKind::Country, NOW, true).is_err());
        // Aliased subtrees (a node reached twice) are legal.
        let tree = [0u8, 0, 1, 0, 0, 2, 0, 0, 2, 0, 0, 2, 0, 0, 9, 0, 0, 9];
        assert!(!tree_has_cycle(&tree, 3, 24));
        assert!(tree_has_cycle(&[0, 0, 0, 0, 0, 9, 0, 0, 9, 0, 0, 9], 2, 24));
    }

    #[test]
    fn a_gzip_bomb_is_refused_at_the_cap() {
        use flate2::{write::GzEncoder, Compression};
        use std::io::Write;
        let mut enc = GzEncoder::new(Vec::new(), Compression::best());
        enc.write_all(&vec![0u8; 5_000_000]).unwrap();
        let gz = enc.finish().unwrap();
        assert!(gz.len() < 20_000, "highly compressible");
        assert_eq!(inflate_capped(&gz, 1_000_000), Err(DbError::TooBig));
        assert_eq!(inflate_capped(&gz, 5_000_000).unwrap().len(), 5_000_000);
        assert_eq!(inflate_capped(b"plain text", 100), Err(DbError::NotGzip));
    }

    proptest! {
        /// The path the fuzz target runs: any bytes, and any single-byte damage to a valid file, never panic.
        #[test]
        fn damaged_and_arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..400), at in 0usize..2000, val in any::<u8>()) {
            let _ = open_checked(bytes.clone(), DbKind::Country, NOW, true);
            let _ = inflate_capped(&bytes, 1000);
            let mut file = sample_country(NOW);
            let n = file.len();
            file[at % n] = val;
            if let Ok((r, _)) = open_checked(file, DbKind::Country, NOW, true) {
                let g = GeoReader::new(Some((r, DbInfo { build_epoch: NOW, node_count: 1 })), None);
                let _ = g.lookup(&ip("192.0.2.1"));
                let _ = g.lookup(&ip("2001:db8::1"));
            }
        }
    }
}
