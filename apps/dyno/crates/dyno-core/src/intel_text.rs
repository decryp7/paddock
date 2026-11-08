//! The text of a user's list: one IP address, CIDR range or hostname per line, `#` starts a comment (a whole line, or after the
//! entry, where it becomes the entry's note). Parse, don't validate: a line becomes a typed [`Target`] once, here, and nothing
//! downstream sees text again. Bounded: a longest line, a longest note, a most entries (see the limits in [`crate::intel`]).

use crate::ip_lookup::{classify, unwrapped, AddressClass};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Longest line read, in bytes; a longer line is skipped.
pub const MAX_LINE: usize = 400;
/// Longest note kept, in characters.
pub const MAX_NOTE: usize = 200;
/// Longest host name, as in DNS.
pub const MAX_HOST: usize = 253;
/// Most skipped lines remembered for the "N lines skipped" message.
pub const MAX_SKIPPED_KEPT: usize = 20;

/// A range of addresses: `start..=end` in one 128-bit space where an IPv4 address `a.b.c.d` is `::ffff:a.b.c.d` (so one
/// ordering serves both families; IPv4-mapped and NAT64 addresses are unwrapped before they get here).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IpRange {
    /// First address.
    pub start: u128,
    /// Last address.
    pub end: u128,
    /// The prefix length as written (IPv4 prefixes are 0 to 32, IPv6 0 to 128), for display.
    pub prefix: u8,
    /// Whether the range is IPv4.
    pub v4: bool,
}

const V4_BASE: u128 = 0xFFFF_0000_0000;

/// An address as a point in the 128-bit space (unwrapping IPv4-mapped and NAT64 forms).
pub fn point(ip: &IpAddr) -> u128 {
    match unwrapped(ip) {
        IpAddr::V4(a) => V4_BASE + u128::from(u32::from(a)),
        IpAddr::V6(a) => u128::from(a),
    }
}

impl IpRange {
    /// A single address.
    pub fn address(ip: &IpAddr) -> IpRange {
        let ip = unwrapped(ip);
        let p = point(&ip);
        IpRange {
            start: p,
            end: p,
            prefix: if ip.is_ipv4() { 32 } else { 128 },
            v4: ip.is_ipv4(),
        }
    }

    /// The range `ip/prefix` (host bits cleared). `None` when the prefix is longer than the family allows.
    pub fn cidr(ip: &IpAddr, prefix: u8) -> Option<IpRange> {
        // `::ffff:a.b.c.d/p` with p >= 96 is the IPv4 range a.b.c.d/(p-96); a NAT64 prefix stays the IPv6 range it is.
        let ip = match ip {
            IpAddr::V6(a) if a.to_ipv4_mapped().is_some() && prefix >= 96 => {
                return IpRange::cidr(&IpAddr::V4(a.to_ipv4_mapped()?), prefix - 96);
            }
            IpAddr::V6(_) => *ip,
            v4 => *v4,
        };
        match ip {
            IpAddr::V4(a) => {
                if prefix > 32 {
                    return None;
                }
                let host = u32::MAX.checked_shr(u32::from(prefix)).unwrap_or(0);
                let start = u32::from(a) & !host;
                Some(IpRange {
                    start: V4_BASE + u128::from(start),
                    end: V4_BASE + u128::from(start | host),
                    prefix,
                    v4: true,
                })
            }
            IpAddr::V6(a) => {
                if prefix > 128 {
                    return None;
                }
                let host = u128::MAX.checked_shr(u32::from(prefix)).unwrap_or(0);
                let start = u128::from(a) & !host;
                Some(IpRange {
                    start,
                    end: start | host,
                    prefix,
                    v4: false,
                })
            }
        }
    }

    /// The IPv4 block of the 128-bit space, `(first, last)`.
    pub const V4_BLOCK: (u128, u128) = (V4_BASE, V4_BASE + 0xFFFF_FFFF);

    /// Whether `p` lies inside.
    pub fn contains(&self, p: u128) -> bool {
        self.start <= p && p <= self.end
    }

    /// The first address of the range, for the class check.
    fn first(&self) -> IpAddr {
        if self.v4 {
            IpAddr::V4(Ipv4Addr::from(u32::try_from(self.start - V4_BASE).unwrap_or(0)))
        } else {
            IpAddr::V6(Ipv6Addr::from(self.start))
        }
    }
}

impl fmt::Display for IpRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let max = if self.v4 { 32 } else { 128 };
        if self.prefix == max {
            write!(f, "{}", self.first())
        } else {
            write!(f, "{}/{}", self.first(), self.prefix)
        }
    }
}

/// What one line of a list names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Target {
    /// An address or a CIDR range.
    Range(IpRange),
    /// A host name, normalised ([`normalise_host`]); matches itself and every name below it.
    Host(String),
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Range(r) => r.fmt(f),
            Target::Host(h) => f.write_str(h),
        }
    }
}

/// One line of a list.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Entry {
    /// What it names.
    pub target: Target,
    /// The user's reason, shown as "why flagged".
    pub note: Option<String>,
}

/// Why a line was not taken (the text says what to change).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkipReason {
    /// Longer than [`MAX_LINE`].
    TooLong,
    /// Not an address, a range or a host name.
    NotAnEntry,
    /// A private, loopback, link-local or other address that is never matched.
    NeverMatched,
    /// A host name with a character outside letters, digits, `-`, `_` and `.` (international names: write the `xn--` form).
    BadHost,
}

impl SkipReason {
    /// A sentence for the user.
    pub fn message(self) -> &'static str {
        match self {
            SkipReason::TooLong => "line is too long",
            SkipReason::NotAnEntry => "not an IP address, range (like 203.0.113.0/24) or host name",
            SkipReason::NeverMatched => "private and special-purpose addresses are never matched",
            SkipReason::BadHost => "host names use letters, digits, '-' and '_' (write international names in the xn-- form)",
        }
    }
}

/// A skipped line: its number (from 1) and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    /// Line number from 1.
    pub line: u32,
    /// Why.
    pub reason: SkipReason,
}

/// The outcome of reading a list's text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    /// The entries taken, in order, duplicates removed.
    pub entries: Vec<Entry>,
    /// The first skipped lines (at most [`MAX_SKIPPED_KEPT`]).
    pub skipped: Vec<Skipped>,
    /// How many lines were skipped in all.
    pub skipped_count: u32,
    /// Whether reading stopped at the entry limit.
    pub truncated: bool,
    /// Exact duplicates dropped (same normalised address, range or host name).
    pub duplicates: u32,
    /// Entries dropped because a wider range, or a parent host name, of the same list already covers them.
    pub covered: u32,
}

/// An entry [`dedupe`] removed, and what made it redundant (for the preview).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    /// The removed entry.
    pub entry: Entry,
    /// The entry that stays and covers it (the same entry again for an exact duplicate).
    pub by: Target,
}

/// The outcome of [`dedupe`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Deduped {
    /// What stays, in the original order.
    pub entries: Vec<Entry>,
    /// Exact duplicates removed.
    pub duplicates: u32,
    /// Covered entries removed.
    pub covered: u32,
    /// The first removed entries, at most [`MAX_SKIPPED_KEPT`] (the preview of "Remove duplicates").
    pub examples: Vec<Removed>,
}

/// Removes exact duplicates and entries covered by a broader range (or a parent host name) in the same list. The first of
/// equal entries keeps its note. Pure: apply the result only after the user saw the preview.
pub fn dedupe(entries: Vec<Entry>) -> Deduped {
    let n = entries.len();
    let mut removed: Vec<Option<(bool, usize)>> = vec![None; n]; // (is_duplicate, kept index)
                                                                 // Exact repeats first (the same normalised target as an earlier entry), so each removal is counted once and a repeat is a
                                                                 // duplicate even when a wider entry also covers it.
    let mut first: std::collections::HashMap<&Target, usize> = std::collections::HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        match first.get(&e.target) {
            Some(&k) => removed[i] = Some((true, k)),
            None => {
                first.insert(&e.target, i);
            }
        }
    }
    // Ranges: sort by start, widest first; a range inside the last kept one is covered.
    let mut ranges: Vec<usize> = (0..n)
        .filter(|&i| removed[i].is_none() && matches!(entries[i].target, Target::Range(_)))
        .collect();
    let r = |i: usize| match &entries[i].target {
        Target::Range(r) => *r,
        Target::Host(_) => unreachable_range(),
    };
    ranges.sort_by(|&a, &b| {
        let (a2, b2) = (r(a), r(b));
        a2.start.cmp(&b2.start).then(b2.end.cmp(&a2.end)).then(a.cmp(&b))
    });
    let mut last: Option<usize> = None;
    for &i in &ranges {
        match last {
            Some(k) if r(k).v4 == r(i).v4 && r(k).end >= r(i).end && r(k).start <= r(i).start => {
                removed[i] = Some((r(k).start == r(i).start && r(k).end == r(i).end, k));
            }
            _ => last = Some(i),
        }
    }
    // Hosts: broader names first; a name with a kept proper suffix is covered.
    let mut hosts: Vec<usize> = (0..n)
        .filter(|&i| removed[i].is_none() && matches!(entries[i].target, Target::Host(_)))
        .collect();
    let h = |i: usize| match &entries[i].target {
        Target::Host(h) => h.as_str(),
        Target::Range(_) => "",
    };
    hosts.sort_by(|&a, &b| h(a).matches('.').count().cmp(&h(b).matches('.').count()).then(a.cmp(&b)));
    let mut kept: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for &i in &hosts {
        let name = h(i);
        if let Some(&k) = kept.get(name) {
            removed[i] = Some((true, k));
            continue;
        }
        let parent = name.match_indices('.').find_map(|(at, _)| kept.get(&name[at + 1..]).copied());
        match parent {
            Some(k) => removed[i] = Some((false, k)),
            None => {
                kept.insert(name, i);
            }
        }
    }
    let mut out = Deduped::default();
    for (i, e) in entries.iter().enumerate() {
        match removed[i] {
            None => out.entries.push(e.clone()),
            Some((dup, k)) => {
                if dup {
                    out.duplicates += 1;
                } else {
                    out.covered += 1;
                }
                if out.examples.len() < MAX_SKIPPED_KEPT {
                    out.examples.push(Removed {
                        entry: e.clone(),
                        by: entries[k].target.clone(),
                    });
                }
            }
        }
    }
    out
}

fn unreachable_range() -> IpRange {
    IpRange {
        start: 0,
        end: 0,
        prefix: 0,
        v4: true,
    }
}

/// Lower-cases a host name and removes the trailing dot; `Err` when it cannot be a name. No resolver is asked.
pub fn normalise_host(raw: &str) -> Result<String, SkipReason> {
    let h = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if h.is_empty() || h.len() > MAX_HOST {
        return Err(SkipReason::NotAnEntry);
    }
    for label in h.split('.') {
        if label.is_empty() || label.len() > 63 || !label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
            return Err(SkipReason::BadHost);
        }
    }
    Ok(h)
}

/// Parses the target part of a line: an address, `address/prefix`, or a host name. `Err(NeverMatched)` for a range whose
/// first address is not public.
pub fn parse_target(text: &str) -> Result<Target, SkipReason> {
    let t = text.trim().trim_start_matches('[').trim_end_matches(']');
    if t.is_empty() {
        return Err(SkipReason::NotAnEntry);
    }
    if let Some((addr, prefix)) = t.split_once('/') {
        let ip: IpAddr = addr.trim().parse().map_err(|_| SkipReason::NotAnEntry)?;
        let prefix: u8 = prefix.trim().parse().map_err(|_| SkipReason::NotAnEntry)?;
        let range = IpRange::cidr(&ip, prefix).ok_or(SkipReason::NotAnEntry)?;
        return never_private(range);
    }
    if let Ok(ip) = t.parse::<IpAddr>() {
        return never_private(IpRange::address(&ip));
    }
    // Looks like a number list ("1.2.3") or has a colon: an address typo, not a host name.
    if t.contains(':') || t.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return Err(SkipReason::NotAnEntry);
    }
    normalise_host(t).map(Target::Host)
}

fn never_private(range: IpRange) -> Result<Target, SkipReason> {
    // A wide range (like 0.0.0.0/0) starts in an unspecified address; judge ranges by what they could match: refuse only a
    // range that lies wholly inside a never-matched class, and let an index skip the non-public addresses at lookup time.
    let wholly_private = classify(&range.first()) != AddressClass::Public && classify(&last_of(&range)) == classify(&range.first());
    if wholly_private && range.prefix >= if range.v4 { 8 } else { 7 } {
        return Err(SkipReason::NeverMatched);
    }
    Ok(Target::Range(range))
}

fn last_of(r: &IpRange) -> IpAddr {
    if r.v4 {
        IpAddr::V4(Ipv4Addr::from(u32::try_from(r.end - V4_BASE).unwrap_or(u32::MAX)))
    } else {
        IpAddr::V6(Ipv6Addr::from(r.end))
    }
}

/// Splits `entry # note`. A `#` starts the note; a line that starts with `#` is a comment.
fn split_note(line: &str) -> (&str, Option<String>) {
    match line.split_once('#') {
        None => (line, None),
        Some((entry, note)) => {
            let note: String = note.trim().chars().filter(|c| !c.is_control()).take(MAX_NOTE).collect();
            (entry, if note.is_empty() { None } else { Some(note) })
        }
    }
}

/// Reads a list's text: at most `max_entries` entries are taken (`truncated` says whether more followed). Never panics; each
/// bad line is counted and the first few are reported with their number.
pub fn parse_list(text: &str, max_entries: usize) -> Parsed {
    let mut out = Parsed::default();
    let mut raw: Vec<Entry> = Vec::new();
    for (i, text_line) in text.lines().enumerate() {
        let line = u32::try_from(i + 1).unwrap_or(u32::MAX);
        let trimmed = text_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let skip = |reason: SkipReason, out: &mut Parsed| {
            out.skipped_count = out.skipped_count.saturating_add(1);
            if out.skipped.len() < MAX_SKIPPED_KEPT {
                out.skipped.push(Skipped { line, reason });
            }
        };
        if text_line.len() > MAX_LINE {
            skip(SkipReason::TooLong, &mut out);
            continue;
        }
        let (entry, note) = split_note(trimmed);
        // The target is the first word; anything after it before the '#' is ignored.
        let mut words = entry.split_whitespace();
        let word = words.next().unwrap_or("");
        let result = if words.next().is_some() {
            Err(SkipReason::NotAnEntry) // "bad host" or "1.2.3.4 5.6.7.8": one entry per line
        } else {
            parse_target(word)
        };
        match result {
            Ok(target) => {
                raw.push(Entry { target, note });
            }
            Err(reason) => skip(reason, &mut out),
        }
    }
    let d = dedupe(raw);
    out.duplicates = d.duplicates;
    out.covered = d.covered;
    out.entries = d.entries;
    if out.entries.len() > max_entries {
        out.entries.truncate(max_entries);
        out.truncated = true;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(s: &str) -> Result<Target, SkipReason> {
        parse_target(s)
    }

    #[test]
    fn addresses_ranges_and_hosts_are_told_apart() {
        assert!(target("203.0.113.9").is_err(), "documentation range is never matched");
        assert!(matches!(target("8.8.8.8"), Ok(Target::Range(_))));
        assert_eq!(target("8.8.8.0/24").unwrap().to_string(), "8.8.8.0/24");
        assert_eq!(target("8.8.8.77/24").unwrap().to_string(), "8.8.8.0/24", "host bits are cleared");
        assert_eq!(target("2606:4700::/32").unwrap().to_string(), "2606:4700::/32");
        assert_eq!(target("[2606:4700::1111]").unwrap().to_string(), "2606:4700::1111");
        assert_eq!(target("Evil.Example.").unwrap(), Target::Host("evil.example".into()));
        assert_eq!(target("8.8.8.0/33"), Err(SkipReason::NotAnEntry));
        assert_eq!(target("1.2.3"), Err(SkipReason::NotAnEntry));
        assert_eq!(target("bad host"), Err(SkipReason::BadHost));
        assert_eq!(target("bücher.example"), Err(SkipReason::BadHost));
        assert_eq!(target("a..b"), Err(SkipReason::BadHost));
    }

    #[test]
    fn private_and_special_addresses_are_refused_but_wide_ranges_are_taken() {
        for s in [
            "10.1.2.3",
            "192.168.0.0/16",
            "127.0.0.1",
            "169.254.1.1",
            "100.64.0.0/10",
            "198.18.0.0/15",
            "fe80::1",
            "fd00::/8",
            "2002::/16",
        ] {
            assert_eq!(target(s), Err(SkipReason::NeverMatched), "{s}");
        }
        assert!(target("0.0.0.0/0").is_ok(), "a /0 names every public address too");
        assert!(target("::/0").is_ok());
    }

    #[test]
    fn a_list_reads_notes_comments_duplicates_and_bad_lines() {
        let text = "# my list\n\n8.8.8.8  # dns test\n8.8.8.8\nevil.example # c2\nnonsense line!\n10.0.0.1\n";
        let p = parse_list(text, 100);
        assert_eq!(p.entries.len(), 2, "the duplicate is dropped");
        assert_eq!(p.entries[0].note.as_deref(), Some("dns test"));
        assert_eq!(p.entries[1].target, Target::Host("evil.example".into()));
        assert_eq!(p.skipped_count, 2);
        assert_eq!((p.duplicates, p.covered), (1, 0));
        assert_eq!(
            p.skipped[0],
            Skipped {
                line: 6,
                reason: SkipReason::NotAnEntry
            }
        );
        assert_eq!(p.skipped[1].reason, SkipReason::NeverMatched);
        assert!(!p.truncated);
    }

    #[test]
    fn the_entry_limit_and_line_limit_hold() {
        let text: String = (1..=50).map(|i| format!("8.8.{i}.1\n")).collect();
        let p = parse_list(&text, 10);
        assert_eq!(p.entries.len(), 10);
        assert!(p.truncated);
        let long = format!("{}.example\n", "a".repeat(MAX_LINE));
        let p = parse_list(&long, 10);
        assert_eq!(p.skipped[0].reason, SkipReason::TooLong);
        let note = format!("8.8.8.8 # {}\n", "n".repeat(300));
        assert_eq!(parse_list(&note, 10).entries[0].note.as_ref().map(String::len), Some(MAX_NOTE));
    }

    #[test]
    fn duplicates_and_covered_entries_are_removed_and_counted() {
        let p = parse_list(
            "8.8.8.8\n8.8.0.0/16 # wide\n8.8.8.0/24\n8.8.0.0/16\nEvil.Example\na.evil.example\n::ffff:8.8.1.1\n::ffff:9.0.0.0/104\n9.0.0.0/8\nother.example\n",
            100,
        );
        let shown: Vec<String> = p.entries.iter().map(|e| e.target.to_string()).collect();
        assert_eq!(
            shown,
            ["8.8.0.0/16", "evil.example", "9.0.0.0/8", "other.example"],
            "original order kept"
        );
        assert_eq!(p.entries[0].note.as_deref(), Some("wide"));
        assert_eq!((p.duplicates, p.covered), (2, 4));
    }

    #[test]
    fn dedupe_reports_what_covers_each_removed_entry() {
        let raw: Vec<Entry> = ["8.8.8.8", "8.8.0.0/16", "x.example", "y.x.example"]
            .iter()
            .map(|t| Entry {
                target: parse_target(t).unwrap(),
                note: None,
            })
            .collect();
        let d = dedupe(raw);
        assert_eq!(d.entries.len(), 2);
        assert_eq!(d.examples.len(), 2);
        assert_eq!(d.examples[0].entry.target.to_string(), "8.8.8.8");
        assert_eq!(d.examples[0].by.to_string(), "8.8.0.0/16");
        assert_eq!(d.examples[1].by.to_string(), "x.example");
    }

    #[test]
    fn an_ipv6_range_never_covers_ipv4_when_deduplicating() {
        let d = dedupe(parse_list("0.0.0.0/0\n::/0", 10).entries);
        assert_eq!(d.entries.len(), 2, "neither family covers the other");
    }

    #[test]
    fn arbitrary_bytes_never_panic() {
        for s in [
            "\u{0}\u{1}",
            "/",
            "//",
            "1.1.1.1/",
            "/8",
            "#",
            "::ffff:1.2.3.4/200",
            "\u{202e}x",
            "a".repeat(5000).as_str(),
        ] {
            let _ = parse_list(s, 5);
        }
    }
}
