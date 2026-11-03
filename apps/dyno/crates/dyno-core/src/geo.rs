//! Where connections go (ADR 0031 §3, the part that stays): a per-country summary of the remote addresses, computed here from
//! the socket rows **before** the page's row cap, so the map counts every connection the socket read returned. The database
//! lookup is a port ([`GeoLookup`]) the agent implements over the downloaded DB-IP files; this module never touches a file.
//!
//! - Only [`AddressClass::Public`] addresses are located (after IPv4-mapped and NAT64 unwrapping). Private, loopback, link-local,
//!   CGNAT, multicast, reserved and special-purpose addresses go to the "on this computer and your network" groups.
//! - A public address the database does not know is `unknown`.
//! - Locations are where an address is *registered*, not where a server is; the page says so (see [`LOCATION_NOTE`]).

use crate::ip_lookup::{classify, unwrapped, AddressClass};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::IpAddr;

/// Shown with the map: what a location means.
pub const LOCATION_NOTE: &str = "Locations are where an address is registered. Content networks (for example Cloudflare, Akamai) \
     serve from near you but are listed under one country.";
/// Most countries in a summary (there are fewer than 260 ISO codes).
pub const MAX_COUNTRIES: usize = 300;

/// What the database knows about one public address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoTag {
    /// ISO 3166-1 alpha-2 code, upper case.
    pub country: String,
    /// The country's English name, when the database has it.
    pub country_name: Option<String>,
    /// Autonomous system number, when the ASN database is on.
    pub asn: Option<u32>,
    /// The network's name ("Cloudflare, Inc."), when known.
    pub as_name: Option<String>,
}

/// The port the agent implements over the downloaded databases.
pub trait GeoLookup {
    /// The tag of a **public** address (never called for any other); `None` when the database does not know it.
    fn lookup(&self, ip: &IpAddr) -> Option<GeoTag>;
}

/// One connection, as the map needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeoRow {
    /// The remote address.
    pub remote: IpAddr,
    /// The owning program's name.
    pub program: String,
    /// Whether a bad-IP list flagged it (the verdict is decided elsewhere).
    pub flagged: bool,
}

/// One country on the map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CountryCount {
    /// ISO code.
    pub code: String,
    /// English name, or the code when unknown.
    pub name: String,
    /// Connections to it.
    pub connections: u32,
    /// Distinct programs.
    pub programs: u32,
    /// Distinct remote addresses.
    pub hosts: u32,
    /// Connections flagged by a list.
    pub flagged: u32,
    /// Whether some of the addresses belong to a network in the CDN or cloud group (reserved for the page's honesty note).
    pub includes_content_networks: bool,
}

/// A group of non-public remote addresses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalGroup {
    /// The class.
    pub class: AddressClass,
    /// Connections to addresses of the class.
    pub connections: u32,
    /// Distinct programs.
    pub programs: u32,
}

/// The map's data.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoSummary {
    /// Countries, most connections first (ties by code).
    pub countries: Vec<CountryCount>,
    /// Non-public destinations by class.
    pub local: Vec<LocalGroup>,
    /// Connections to public addresses the database does not know (or when no database is on).
    pub unknown: u32,
    /// The database's build date (`yyyy-mm-dd`), when one is in use.
    pub db_date: Option<String>,
    /// Connections left out because the socket read itself was capped; 0 when the summary covers everything.
    pub truncated: u32,
    /// Whether any database is in use.
    pub database_on: bool,
    /// The country of the user's own location (where the arcs start), when known.
    pub home_country: Option<String>,
    /// Where that comes from, in words: "Your location: from your public IP (via Cloudflare)", "... from your region setting"
    /// or why it is not known.
    pub home_source: String,
}

#[derive(Default)]
struct Acc {
    name: Option<String>,
    connections: u32,
    flagged: u32,
    programs: HashSet<String>,
    hosts: HashSet<IpAddr>,
    content: bool,
}

/// Networks (by AS number) shown as content networks; data, not code, so the page can extend it (the agent passes the list).
pub type ContentAsns<'a> = &'a [u32];

/// Aggregates `rows` by country. `left_out` is the number of connections the socket read dropped because of its own cap (0
/// when none). Runs over every row it is given, whatever the page later shows.
pub fn summarise(
    rows: &[GeoRow],
    geo: Option<&dyn GeoLookup>,
    db_date: Option<String>,
    left_out: u32,
    content_asns: ContentAsns<'_>,
) -> GeoSummary {
    let mut countries: HashMap<String, Acc> = HashMap::new();
    let mut local: BTreeMap<AddressClass, (u32, HashSet<&str>)> = BTreeMap::new();
    let mut seen: HashMap<IpAddr, Option<GeoTag>> = HashMap::new();
    let mut unknown = 0u32;
    for row in rows {
        let class = classify(&row.remote);
        if class != AddressClass::Public {
            let e = local.entry(class).or_default();
            e.0 = e.0.saturating_add(1);
            e.1.insert(row.program.as_str());
            continue;
        }
        let ip = unwrapped(&row.remote);
        let tag = match geo {
            None => None,
            Some(g) => seen.entry(ip).or_insert_with(|| g.lookup(&ip)).clone(),
        };
        let Some(tag) = tag else {
            unknown = unknown.saturating_add(1);
            continue;
        };
        if countries.len() >= MAX_COUNTRIES && !countries.contains_key(&tag.country) {
            unknown = unknown.saturating_add(1);
            continue;
        }
        let a = countries.entry(tag.country.clone()).or_default();
        a.name = a.name.take().or(tag.country_name);
        a.connections = a.connections.saturating_add(1);
        a.flagged = a.flagged.saturating_add(u32::from(row.flagged));
        a.programs.insert(row.program.clone());
        a.hosts.insert(ip);
        a.content |= tag.asn.is_some_and(|n| content_asns.contains(&n));
    }
    let mut countries: Vec<CountryCount> = countries
        .into_iter()
        .map(|(code, a)| CountryCount {
            name: a.name.unwrap_or_else(|| code.clone()),
            code,
            connections: a.connections,
            programs: u32::try_from(a.programs.len()).unwrap_or(u32::MAX),
            hosts: u32::try_from(a.hosts.len()).unwrap_or(u32::MAX),
            flagged: a.flagged,
            includes_content_networks: a.content,
        })
        .collect();
    countries.sort_by(|a, b| b.connections.cmp(&a.connections).then_with(|| a.code.cmp(&b.code)));
    GeoSummary {
        countries,
        local: local
            .into_iter()
            .map(|(class, (connections, programs))| LocalGroup {
                class,
                connections,
                programs: u32::try_from(programs.len()).unwrap_or(u32::MAX),
            })
            .collect(),
        unknown,
        db_date,
        truncated: left_out,
        database_on: geo.is_some(),
        home_country: None,
        home_source: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lookup over invented ranges: documentation-free public blocks mapped to invented countries.
    struct Fake;
    impl GeoLookup for Fake {
        fn lookup(&self, ip: &IpAddr) -> Option<GeoTag> {
            let tag = |c: &str, n: &str, asn| GeoTag {
                country: c.into(),
                country_name: Some(n.into()),
                asn,
                as_name: None,
            };
            match ip {
                IpAddr::V4(a) if a.octets()[0] == 8 => Some(tag("US", "United States", Some(13335))),
                IpAddr::V4(a) if a.octets()[0] == 9 => Some(tag("DE", "Germany", None)),
                IpAddr::V6(a) if a.segments()[0] == 0x2606 => Some(tag("US", "United States", None)),
                _ => None,
            }
        }
    }

    fn row(ip: &str, program: &str, flagged: bool) -> GeoRow {
        GeoRow {
            remote: ip.parse().unwrap(),
            program: program.into(),
            flagged,
        }
    }

    #[test]
    fn connections_are_counted_per_country_with_programs_hosts_and_flags() {
        let rows = [
            row("8.8.8.8", "Firefox", false),
            row("8.8.4.4", "Firefox", true),
            row("8.8.8.8", "Slack", false),
            row("2606:4700::1", "Firefox", false),
            row("9.9.9.9", "Mail", false),
            row("77.1.1.1", "Mail", false),
        ];
        let s = summarise(&rows, Some(&Fake), Some("2026-11-01".into()), 0, &[13335]);
        assert_eq!(s.countries.len(), 2);
        let us = &s.countries[0];
        assert_eq!((us.code.as_str(), us.name.as_str()), ("US", "United States"));
        assert_eq!((us.connections, us.programs, us.hosts, us.flagged), (4, 2, 3, 1));
        assert!(us.includes_content_networks);
        assert_eq!(s.countries[1].code, "DE");
        assert!(!s.countries[1].includes_content_networks);
        assert_eq!(s.unknown, 1);
        assert_eq!((s.db_date.as_deref(), s.truncated, s.database_on), (Some("2026-11-01"), 0, true));
    }

    #[test]
    fn private_and_special_addresses_are_grouped_and_never_located() {
        struct Panics;
        impl GeoLookup for Panics {
            fn lookup(&self, ip: &IpAddr) -> Option<GeoTag> {
                assert!(crate::ip_lookup::is_public(ip), "asked about {ip}");
                None
            }
        }
        let rows = [
            row("127.0.0.1", "A", false),
            row("::1", "A", false),
            row("192.168.1.5", "B", false),
            row("10.0.0.2", "B", false),
            row("100.64.0.9", "C", false),
            row("198.18.0.7", "C", false),
            row("2002::1", "C", false),
            row("224.0.0.251", "D", false),
            row("::ffff:10.1.1.1", "E", false),
            row("64:ff9b::a00:1", "E", false),
        ];
        let s = summarise(&rows, Some(&Panics), None, 0, &[]);
        assert!(s.countries.is_empty());
        assert_eq!(s.unknown, 0);
        let by = |c: AddressClass| s.local.iter().find(|g| g.class == c).map(|g| (g.connections, g.programs));
        assert_eq!(by(AddressClass::Loopback), Some((2, 1)));
        assert_eq!(by(AddressClass::Private), Some((4, 2)));
        assert_eq!(by(AddressClass::Cgnat), Some((1, 1)));
        assert_eq!(by(AddressClass::Special), Some((2, 1)));
        assert_eq!(by(AddressClass::Multicast), Some((1, 1)));
    }

    #[test]
    fn mapped_and_nat64_addresses_are_located_as_the_address_inside() {
        let rows = [row("::ffff:8.8.8.8", "A", false), row("64:ff9b::909:909", "A", false)];
        let s = summarise(&rows, Some(&Fake), None, 0, &[]);
        let codes: Vec<&str> = s.countries.iter().map(|c| c.code.as_str()).collect();
        assert_eq!(codes, ["DE", "US"]);
    }

    #[test]
    fn without_a_database_every_public_connection_is_unknown_and_the_summary_says_so() {
        let s = summarise(&[row("8.8.8.8", "A", false), row("10.0.0.1", "A", false)], None, None, 0, &[]);
        assert_eq!((s.unknown, s.database_on, s.countries.len(), s.local.len()), (1, false, 0, 1));
    }

    #[test]
    fn the_summary_counts_every_row_it_is_given_and_reports_what_the_socket_read_left_out() {
        let rows: Vec<GeoRow> = (0..5000)
            .map(|i| row(&format!("8.8.{}.{}", i / 250, i % 250 + 1), "A", false))
            .collect();
        let s = summarise(&rows, Some(&Fake), None, 37, &[]);
        assert_eq!(s.countries[0].connections, 5000, "beyond the 2,000 rows a page shows");
        assert_eq!(s.truncated, 37);
    }

    #[test]
    fn the_number_of_countries_is_bounded() {
        struct Many;
        impl GeoLookup for Many {
            fn lookup(&self, ip: &IpAddr) -> Option<GeoTag> {
                let IpAddr::V4(a) = ip else { return None };
                let o = a.octets();
                Some(GeoTag {
                    country: format!("C{}{}", o[1], o[2]),
                    country_name: None,
                    asn: None,
                    as_name: None,
                })
            }
        }
        let rows: Vec<GeoRow> = (0..2000u32)
            .map(|i| row(&format!("8.{}.{}.1", i / 200, i % 200), "A", false))
            .collect();
        let s = summarise(&rows, Some(&Many), None, 0, &[]);
        assert_eq!(s.countries.len(), MAX_COUNTRIES);
        assert_eq!(
            s.unknown as usize,
            2000 - rows.len().min(2000) + (2000 - MAX_COUNTRIES),
            "the rest are 'unknown'"
        );
    }
}
