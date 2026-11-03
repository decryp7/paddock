//! The bad-IP check, pure core (ADR 0031 as amended by `0031-amendments.md`): Dyno ships with **no** list. Every list is the
//! user's own (typed, imported from a file, or fetched from a URL the user added). This module turns the user's enabled lists
//! into one index and answers, for a remote address, a [`Verdict`].
//!
//! - **Index.** Every address and CIDR entry is an interval; CIDR ranges are nested or disjoint, so one sweep over them sorted by
//!   start builds disjoint *segments*, each holding, per list, the most specific entry that covers it. A lookup is one binary
//!   search: `O(log n)`, no allocation until a match is reported. Host names are kept in a hash map and matched by label suffix.
//! - **Never matched:** an address that [`crate::ip_lookup::classify`] does not call public (private, loopback, link-local, CGNAT,
//!   multicast, reserved, benchmarking, documentation, 6to4, Teredo, discard-only), after IPv4-mapped and NAT64 addresses are
//!   unwrapped. Not even an address allow or a list entry changes that.
//! - **Order of a verdict:** an address `allow` (an address or a CIDR) wins over everything; then the lists. A *host name* `allow`
//!   cancels only the user's own host name entries: a reverse name is set by whoever controls the address's reverse zone, so a
//!   listed address named `update.example` must stay flagged. Host names are matched only against a reverse name the caller
//!   already holds; this module never resolves anything (no automatic reverse DNS, ADR 0031 D4).
//! - **Bounded:** [`MAX_LISTS`] lists, [`MAX_TOTAL_ENTRIES`] entries in all.

use crate::intel_text::{normalise_host, point, Entry, IpRange, Target};
use crate::ip_lookup::is_public;
use serde::Serialize;
use std::collections::HashMap;
use std::net::IpAddr;

/// Most lists the user may have.
pub const MAX_LISTS: usize = 32;
/// Most entries over all lists (address ranges, addresses and host names).
pub const MAX_TOTAL_ENTRIES: usize = 1_000_000;
/// Most entries on the address allow list.
pub const MAX_ALLOW: usize = 10_000;
/// The label shown wherever a reverse name is the reason for a match.
pub const NAME_LABEL: &str = "name set by the address's owner";

/// A list as the index needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListInput {
    /// A stable id (a slug), kept when the list is renamed.
    pub id: String,
    /// The name the user gave.
    pub name: String,
    /// Off lists are not matched.
    pub enabled: bool,
    /// The entries.
    pub entries: Vec<Entry>,
}

/// The user's allow entries: addresses override everything; host names cancel only host name entries of the lists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AllowInput {
    /// Address allows (an address or a CIDR), each with an optional note.
    pub addresses: Vec<Entry>,
    /// Host name allows, each with an optional note.
    pub hosts: Vec<Entry>,
}

/// Why an index could not be built (the text says what to change).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IntelError {
    /// More than [`MAX_LISTS`] lists.
    #[error("You can have at most {MAX_LISTS} lists. Remove or merge a list first.")]
    TooManyLists,
    /// More than [`MAX_TOTAL_ENTRIES`] entries.
    #[error("The lists hold more than {MAX_TOTAL_ENTRIES} entries in all. Remove a list or shorten one.")]
    TooManyEntries,
    /// More than [`MAX_ALLOW`] allow entries.
    #[error("The allow list can hold at most {MAX_ALLOW} entries.")]
    TooManyAllows,
}

/// How a list entry matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum MatchedBy {
    /// The remote address is inside the entry.
    Address,
    /// The reverse name Dyno already held is the entry or below it. The name is unverified: [`NAME_LABEL`].
    ReverseName {
        /// The name (as cached).
        name: String,
        /// Always [`NAME_LABEL`].
        label: &'static str,
    },
}

/// One list that flagged the connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListMatch {
    /// The list's id.
    pub list_id: String,
    /// The list's name at the time.
    pub list_name: String,
    /// The most specific entry that matched (`203.0.113.0/24`-style text, or the host name).
    pub entry: String,
    /// The user's note on that entry: the reason shown in "why flagged".
    pub note: Option<String>,
    /// Address or reverse name.
    pub by: MatchedBy,
}

/// The verdict for one remote address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Verdict {
    /// On no enabled list (or an address that is never matched).
    Clean,
    /// The user's address allow list overrides every list.
    Allowed {
        /// The allow entry.
        entry: String,
        /// Its note.
        note: Option<String>,
    },
    /// On at least one list.
    Flagged {
        /// Every list that matched, in list order (never empty).
        matches: Vec<ListMatch>,
    },
}

#[derive(Debug, Clone)]
struct Segment {
    start: u128,
    end: u128,
    /// Range into `members`.
    from: u32,
    to: u32,
}

/// A range on a segment that is the most specific one of some lists.
#[derive(Debug, Clone, Copy)]
struct Member {
    /// Index into `ranges`.
    range: u32,
    /// The lists for which this is the most specific range on the segment (a bit per list).
    lists: u32,
}

/// A range stored **once**, whatever number of lists contain it.
#[derive(Debug, Clone)]
struct Stored {
    range: IpRange,
    /// The text to show when it differs from the range's own (a part cut out of a wider IPv6 range).
    label: Option<Box<str>>,
    /// Which lists contain it (a bit per list).
    mask: u32,
    /// Each owning list with its own note on this entry.
    owners: Vec<(u8, Option<String>)>,
}

/// Stores `range` for `list`, sharing the slot of an identical range already stored.
fn store(ranges: &mut Vec<Stored>, at: &mut HashMap<(u128, u128), u32>, range: IpRange, list: u8, note: Option<String>) {
    let parts = range.own_family_parts();
    let cut = parts.len() != 1 || parts[0] != range;
    for part in parts {
        let label = cut.then(|| range.to_string().into_boxed_str());
        store_one(ranges, at, part, label, list, note.clone());
    }
}

fn store_one(
    ranges: &mut Vec<Stored>,
    at: &mut HashMap<(u128, u128), u32>,
    range: IpRange,
    label: Option<Box<str>>,
    list: u8,
    note: Option<String>,
) {
    let slot = *at.entry((range.start, range.end)).or_insert_with(|| {
        ranges.push(Stored {
            range,
            label,
            mask: 0,
            owners: Vec::new(),
        });
        (ranges.len() - 1) as u32
    });
    let s = &mut ranges[slot as usize];
    if s.mask & (1 << list) == 0 {
        s.mask |= 1 << list;
        s.owners.push((list, note));
    }
}

/// The matcher built from the user's enabled lists and allow entries.
#[derive(Debug, Clone, Default)]
pub struct IntelIndex {
    lists: Vec<(String, String)>,
    ranges: Vec<Stored>,
    range_at: HashMap<(u128, u128), u32>,
    segments: Vec<Segment>,
    members: Vec<Member>,
    /// Normalised host name -> (list, note).
    hosts: HashMap<String, Vec<(u8, Option<String>)>>,
    allow: Vec<Stored>,
    allow_segments: Vec<Segment>,
    allow_members: Vec<Member>,
    allow_hosts: HashMap<String, Option<String>>,
}

impl Stored {
    fn shown(&self) -> String {
        self.label.as_deref().map_or_else(|| self.range.to_string(), str::to_string)
    }
}

type Sweep = (Vec<Segment>, Vec<Member>);

/// Builds disjoint segments from nested-or-disjoint ranges: each segment names, per list, its most specific covering range.
fn sweep(stored: &[Stored]) -> Sweep {
    let mut order: Vec<u32> = (0..stored.len() as u32).collect();
    // Start ascending; for equal starts the widest first, so a parent sits below its children on the stack.
    order.sort_by(|&a, &b| {
        let (a, b) = (&stored[a as usize].range, &stored[b as usize].range);
        a.start.cmp(&b.start).then(b.end.cmp(&a.end))
    });
    let mut segments = Vec::new();
    let mut members = Vec::new();
    let mut stack: Vec<u32> = Vec::new();
    // `pos` is the first address not yet covered by an emitted segment; `None` once the whole space is behind us.
    let mut pos: Option<u128> = Some(0);

    let emit = |segments: &mut Vec<Segment>, members: &mut Vec<Member>, stack: &[u32], from: u128, to: u128| {
        if stack.is_empty() || from > to {
            return;
        }
        let first = members.len() as u32;
        let mut seen: u32 = 0;
        for &ri in stack.iter().rev() {
            let fresh = stored[ri as usize].mask & !seen;
            if fresh != 0 {
                seen |= fresh;
                members.push(Member { range: ri, lists: fresh });
            }
        }
        segments.push(Segment {
            start: from,
            end: to,
            from: first,
            to: members.len() as u32,
        });
    };

    let pop_until =
        |stack: &mut Vec<u32>, segments: &mut Vec<Segment>, members: &mut Vec<Member>, pos: &mut Option<u128>, limit: Option<u128>| {
            // Pops every range that ends before `limit` (all of them when `limit` is None).
            while let Some(&top) = stack.last() {
                let end = stored[top as usize].range.end;
                if limit.is_some_and(|l| end >= l) {
                    break;
                }
                if let Some(p) = *pos {
                    emit(segments, members, stack, p, end);
                    *pos = end.checked_add(1);
                }
                stack.pop();
            }
        };

    for &i in &order {
        let r = stored[i as usize].range;
        pop_until(&mut stack, &mut segments, &mut members, &mut pos, Some(r.start));
        if let Some(p) = pos {
            if p < r.start {
                emit(&mut segments, &mut members, &stack, p, r.start - 1);
            }
        }
        pos = Some(r.start);
        stack.push(i);
    }
    pop_until(&mut stack, &mut segments, &mut members, &mut pos, None);
    (segments, members)
}

fn find(segments: &[Segment], p: u128) -> Option<&Segment> {
    let i = segments.partition_point(|s| s.start <= p);
    let s = segments.get(i.checked_sub(1)?)?;
    (p <= s.end).then_some(s)
}

/// Label suffixes of a normalised name, longest first: `a.b.c` gives `a.b.c`, `b.c`, `c`.
fn suffixes(name: &str) -> impl Iterator<Item = &str> {
    std::iter::once(name).chain(name.match_indices('.').map(move |(i, _)| &name[i + 1..]))
}

impl IntelIndex {
    /// Builds the index from the enabled lists and the allow entries.
    pub fn build(lists: &[ListInput], allow: &AllowInput) -> Result<IntelIndex, IntelError> {
        if lists.len() > MAX_LISTS {
            return Err(IntelError::TooManyLists);
        }
        let total: usize = lists.iter().map(|l| l.entries.len()).sum();
        if total > MAX_TOTAL_ENTRIES {
            return Err(IntelError::TooManyEntries);
        }
        if allow.addresses.len() + allow.hosts.len() > MAX_ALLOW {
            return Err(IntelError::TooManyAllows);
        }
        let mut ix = IntelIndex::default();
        let mut allow_at = HashMap::new();
        for l in lists {
            let li = ix.lists.len() as u8;
            ix.lists.push((l.id.clone(), l.name.clone()));
            if !l.enabled {
                continue;
            }
            for e in &l.entries {
                match &e.target {
                    Target::Range(r) => store(&mut ix.ranges, &mut ix.range_at, *r, li, e.note.clone()),
                    Target::Host(h) => ix.hosts.entry(h.clone()).or_default().push((li, e.note.clone())),
                }
            }
        }
        for e in &allow.addresses {
            if let Target::Range(r) = &e.target {
                store(&mut ix.allow, &mut allow_at, *r, 0, e.note.clone());
            }
        }
        for e in &allow.hosts {
            if let Target::Host(h) = &e.target {
                ix.allow_hosts.insert(h.clone(), e.note.clone());
            }
        }
        (ix.segments, ix.members) = sweep(&ix.ranges);
        ix.range_at = HashMap::new(); // only needed while building
        (ix.allow_segments, ix.allow_members) = sweep(&ix.allow);
        Ok(ix)
    }

    /// Number of disjoint address segments (for the benchmark and the status line).
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Number of distinct ranges stored (an identical range in several lists is stored once).
    pub fn stored_ranges(&self) -> usize {
        self.ranges.len()
    }

    /// The memory the address index holds, in bytes (a count from the element sizes, not an allocator reading).
    pub fn approx_bytes(&self) -> usize {
        use std::mem::size_of;
        self.segments.capacity() * size_of::<Segment>()
            + self.members.capacity() * size_of::<Member>()
            + self.ranges.capacity() * size_of::<Stored>()
            + self
                .ranges
                .iter()
                .map(|s| s.owners.capacity() * size_of::<(u8, Option<String>)>())
                .sum::<usize>()
            + self.hosts.len() * 64
    }

    /// The verdict for `ip`. `reverse_name` is a reverse name Dyno **already holds** (the user asked for names on the page); it
    /// is never looked up here.
    pub fn check(&self, ip: &IpAddr, reverse_name: Option<&str>) -> Verdict {
        if !is_public(ip) {
            return Verdict::Clean;
        }
        let p = point(ip);
        if let Some(seg) = find(&self.allow_segments, p) {
            // The narrowest allow entry that covers the address.
            if let Some(m) = self.allow_members[seg.from as usize..seg.to as usize].first() {
                let a = &self.allow[m.range as usize];
                return Verdict::Allowed {
                    entry: a.shown(),
                    note: a.owners.first().and_then(|(_, n)| n.clone()),
                };
            }
        }
        let mut found: Vec<ListMatch> = Vec::new();
        if let Some(seg) = find(&self.segments, p) {
            for m in &self.members[seg.from as usize..seg.to as usize] {
                let s = &self.ranges[m.range as usize];
                for (li, note) in s.owners.iter().filter(|(l, _)| m.lists & (1 << l) != 0) {
                    let (id, name) = &self.lists[*li as usize];
                    found.push(ListMatch {
                        list_id: id.clone(),
                        list_name: name.clone(),
                        entry: s.shown(),
                        note: note.clone(),
                        by: MatchedBy::Address,
                    });
                }
            }
        }
        if let Some(raw) = reverse_name {
            if let Ok(name) = normalise_host(raw) {
                // A host name allow cancels host name entries only: the address matches above are untouched.
                let allowed = suffixes(&name).any(|s| self.allow_hosts.contains_key(s));
                if !allowed {
                    for suffix in suffixes(&name) {
                        for (li, note) in self.hosts.get(suffix).into_iter().flatten() {
                            let (id, lname) = &self.lists[*li as usize];
                            found.push(ListMatch {
                                list_id: id.clone(),
                                list_name: lname.clone(),
                                entry: suffix.to_string(),
                                note: note.clone(),
                                by: MatchedBy::ReverseName {
                                    name: name.clone(),
                                    label: NAME_LABEL,
                                },
                            });
                        }
                    }
                }
            }
        }
        if found.is_empty() {
            return Verdict::Clean;
        }
        found.sort_by(|a, b| a.list_id.cmp(&b.list_id));
        Verdict::Flagged { matches: found }
    }
}

#[cfg(test)]
mod tests;
