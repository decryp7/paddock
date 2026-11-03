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

use crate::intel_text::{normalise_host, Entry, IpRange, Target};
use crate::ip_lookup::{is_public, unwrapped};
use serde::Serialize;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

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
    /// Stopped by the user while building.
    #[error("Stopped.")]
    Cancelled,
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

/// A key of one family's address space: `u32` for IPv4, `u128` for IPv6.
trait Key: Copy + Ord + std::hash::Hash {
    fn succ(self) -> Option<Self>;
}
impl Key for u32 {
    fn succ(self) -> Option<u32> {
        self.checked_add(1)
    }
}
impl Key for u128 {
    fn succ(self) -> Option<u128> {
        self.checked_add(1)
    }
}

/// A range stored **once**, whatever number of lists contain it.
#[derive(Debug, Clone, Copy)]
struct Slot<K> {
    start: K,
    end: K,
    prefix: u8,
    /// Which lists contain it (a bit per list).
    mask: u32,
}

/// A range on a segment that is the most specific one of some lists.
#[derive(Debug, Clone, Copy)]
struct Member {
    /// Index into the family's slots.
    slot: u32,
    /// The lists for which this is the most specific range on the segment (a bit per list).
    lists: u32,
}

/// The matcher of one address family, compact: segment starts, the offset of each segment's members, and the members. A gap
/// between ranges is a segment with no members, so no segment stores an end.
#[derive(Debug, Clone)]
struct Family<K> {
    slots: Vec<Slot<K>>,
    starts: Vec<K>,
    /// `from[i]..from[i + 1]` are the members of segment `i`; one more entry than `starts`.
    from: Vec<u32>,
    members: Vec<Member>,
}

impl<K> Default for Family<K> {
    fn default() -> Self {
        Family {
            slots: Vec::new(),
            starts: Vec::new(),
            from: vec![0],
            members: Vec::new(),
        }
    }
}

impl<K: Key> Family<K> {
    /// Sorts the slots into disjoint segments. `tick` is told how many slots were swept (and may cancel by returning false).
    fn build(slots: Vec<Slot<K>>, tick: &mut dyn FnMut(u64) -> bool) -> Result<Family<K>, IntelError> {
        let mut order: Vec<u32> = (0..slots.len() as u32).collect();
        // Start ascending; for equal starts the widest first, so a parent sits below its children on the stack.
        order.sort_unstable_by(|&a, &b| {
            let (a, b) = (&slots[a as usize], &slots[b as usize]);
            a.start.cmp(&b.start).then(b.end.cmp(&a.end))
        });
        let mut f = Family {
            slots,
            ..Family::default()
        };
        let mut stack: Vec<u32> = Vec::new();
        for (n, &i) in order.iter().enumerate() {
            if (n as u64).is_multiple_of(16_384) && !tick(n as u64) {
                return Err(IntelError::Cancelled);
            }
            let start = f.slots[i as usize].start;
            f.close_before(&mut stack, Some(start));
            stack.push(i);
            f.emit(&stack, start);
        }
        f.close_before(&mut stack, None);
        Ok(f)
    }

    /// Pops every range that ends before `limit` (all of them when `None`); behind each one a new segment begins, holding what
    /// is left on the stack (empty: a gap).
    fn close_before(&mut self, stack: &mut Vec<u32>, limit: Option<K>) {
        while let Some(&top) = stack.last() {
            let end = self.slots[top as usize].end;
            if limit.is_some_and(|l| end >= l) {
                break;
            }
            stack.pop();
            if let Some(next) = end.succ() {
                self.emit(stack, next);
            }
        }
    }

    /// Starts a segment at `at` holding the most specific range of each list on `stack`; a segment already started at `at` is
    /// replaced.
    fn emit(&mut self, stack: &[u32], at: K) {
        if self.starts.last() == Some(&at) {
            self.starts.pop();
            self.from.pop();
            self.members.truncate(self.from.last().copied().unwrap_or(0) as usize);
        }
        self.starts.push(at);
        let mut seen: u32 = 0;
        for &si in stack.iter().rev() {
            let fresh = self.slots[si as usize].mask & !seen;
            if fresh != 0 {
                seen |= fresh;
                self.members.push(Member { slot: si, lists: fresh });
            }
        }
        self.from.push(self.members.len() as u32);
    }

    fn find(&self, p: K) -> Option<&[Member]> {
        let i = self.starts.partition_point(|s| *s <= p).checked_sub(1)?;
        let (a, b) = (*self.from.get(i)? as usize, *self.from.get(i + 1)? as usize);
        let m = self.members.get(a..b)?;
        (!m.is_empty()).then_some(m)
    }

    fn matched_segments(&self) -> usize {
        self.from.windows(2).filter(|w| w[1] > w[0]).count()
    }

    fn bytes(&self) -> usize {
        use std::mem::size_of;
        self.slots.capacity() * size_of::<Slot<K>>()
            + self.starts.capacity() * size_of::<K>()
            + self.from.capacity() * 4
            + self.members.capacity() * size_of::<Member>()
    }
}
impl<K> Slot<K> {
    fn new(start: K, end: K, prefix: u8) -> Slot<K> {
        Slot {
            start,
            end,
            prefix,
            mask: 0,
        }
    }
}

/// Collects ranges for one family, sharing the slot of an identical range already seen (one entry in several lists is stored
/// once).
struct Collect<K> {
    slots: Vec<Slot<K>>,
    at: HashMap<(K, u8), u32>,
}

impl<K: Key> Collect<K> {
    fn new() -> Self {
        Collect {
            slots: Vec::new(),
            at: HashMap::new(),
        }
    }

    fn add(&mut self, start: K, end: K, prefix: u8, list: u8) {
        let slot = *self.at.entry((start, prefix)).or_insert_with(|| {
            self.slots.push(Slot::new(start, end, prefix));
            (self.slots.len() - 1) as u32
        });
        self.slots[slot as usize].mask |= 1 << list;
    }
}

/// What the build is doing, for a job's progress (ER-18): the phase and how far in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildPhase {
    /// Taking the entries of the lists (counted in entries).
    Collect,
    /// Sorting and sweeping the IPv4 ranges (counted in ranges).
    SweepV4,
    /// Sorting and sweeping the IPv6 ranges (counted in ranges).
    SweepV6,
}

/// A host name entry: its list and note.
type HostEntry = (u8, Option<Box<str>>);

/// One address family's matcher plus the addresses' notes.
#[derive(Debug, Clone, Default)]
struct Side {
    v4: Family<u32>,
    v6: Family<u128>,
}

/// The matcher built from the user's enabled lists and allow entries. Holds only compact ranges: the entries themselves (and
/// their text) stay with the lists, not here; only notes, which are few, are kept.
#[derive(Debug, Clone, Default)]
pub struct IntelIndex {
    lists: Vec<(String, String)>,
    lists_side: Side,
    /// Normalised host name -> (list, note).
    hosts: HashMap<String, Vec<HostEntry>>,
    allow_side: Side,
    allow_hosts: HashMap<String, Option<Box<str>>>,
    /// Notes of address entries: `(list, start, prefix)` for lists, `(255, start, prefix)` for allows.
    notes: HashMap<(u8, u128, u8), Box<str>>,
}

const ALLOW: u8 = 255;

fn bx(n: &Option<String>) -> Option<Box<str>> {
    n.as_deref().map(Box::from)
}

fn show(addr: IpAddr, prefix: u8) -> String {
    let max = if addr.is_ipv4() { 32 } else { 128 };
    if prefix == max {
        addr.to_string()
    } else {
        format!("{addr}/{prefix}")
    }
}

fn v4_addr(s: u32) -> IpAddr {
    IpAddr::V4(Ipv4Addr::from(s))
}

/// Label suffixes of a normalised name, longest first: `a.b.c` gives `a.b.c`, `b.c`, `c`.
fn suffixes(name: &str) -> impl Iterator<Item = &str> {
    std::iter::once(name).chain(name.match_indices('.').map(move |(i, _)| &name[i + 1..]))
}

impl IntelIndex {
    /// Builds the index from the enabled lists and the allow entries.
    pub fn build(lists: &[ListInput], allow: &AllowInput) -> Result<IntelIndex, IntelError> {
        IntelIndex::build_with_progress(lists, allow, &mut |_, _| true)
    }

    /// [`build`](Self::build) for a job: `progress` is told the phase and the count so far and returns `false` to cancel
    /// ([`IntelError::Cancelled`]); nothing partial is kept. The caller swaps the finished index in whole.
    pub fn build_with_progress(
        lists: &[ListInput],
        allow: &AllowInput,
        progress: &mut dyn FnMut(BuildPhase, u64) -> bool,
    ) -> Result<IntelIndex, IntelError> {
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
        let (mut l4, mut l6) = (Collect::<u32>::new(), Collect::<u128>::new());
        let mut seen = 0u64;
        for l in lists {
            let li = ix.lists.len() as u8;
            ix.lists.push((l.id.clone(), l.name.clone()));
            if !l.enabled {
                continue;
            }
            for e in &l.entries {
                seen += 1;
                if seen.is_multiple_of(16_384) && !progress(BuildPhase::Collect, seen) {
                    return Err(IntelError::Cancelled);
                }
                match &e.target {
                    Target::Range(r) if r.v4 => {
                        let (s, en) = (r.start - IpRange::V4_BLOCK.0, r.end - IpRange::V4_BLOCK.0);
                        l4.add(s as u32, en as u32, r.prefix, li);
                        if let Some(n) = &e.note {
                            ix.notes.insert((li, s, r.prefix), Box::from(n.as_str()));
                        }
                    }
                    Target::Range(r) => {
                        l6.add(r.start, r.end, r.prefix, li);
                        if let Some(n) = &e.note {
                            ix.notes.insert((li, r.start, r.prefix), Box::from(n.as_str()));
                        }
                    }
                    Target::Host(h) => ix.hosts.entry(h.clone()).or_default().push((li, bx(&e.note))),
                }
            }
        }
        let (mut a4, mut a6) = (Collect::<u32>::new(), Collect::<u128>::new());
        for e in &allow.addresses {
            if let Target::Range(r) = &e.target {
                if r.v4 {
                    let (s, en) = (r.start - IpRange::V4_BLOCK.0, r.end - IpRange::V4_BLOCK.0);
                    a4.add(s as u32, en as u32, r.prefix, 0);
                    if let Some(n) = &e.note {
                        ix.notes.insert((ALLOW, s, r.prefix), Box::from(n.as_str()));
                    }
                } else {
                    a6.add(r.start, r.end, r.prefix, 0);
                    if let Some(n) = &e.note {
                        ix.notes.insert((ALLOW, r.start, r.prefix), Box::from(n.as_str()));
                    }
                }
            }
        }
        for e in &allow.hosts {
            if let Target::Host(h) = &e.target {
                ix.allow_hosts.insert(h.clone(), bx(&e.note));
            }
        }
        ix.lists_side.v4 = Family::build(l4.slots, &mut |n| progress(BuildPhase::SweepV4, n))?;
        ix.lists_side.v6 = Family::build(l6.slots, &mut |n| progress(BuildPhase::SweepV6, n))?;
        ix.allow_side.v4 = Family::build(a4.slots, &mut |_| true)?;
        ix.allow_side.v6 = Family::build(a6.slots, &mut |_| true)?;
        Ok(ix)
    }

    /// Number of disjoint address segments that hold a match (for the status line and tests).
    pub fn segment_count(&self) -> usize {
        self.lists_side.v4.matched_segments() + self.lists_side.v6.matched_segments()
    }

    /// Number of distinct ranges stored (an identical range in several lists is stored once).
    pub fn stored_ranges(&self) -> usize {
        self.lists_side.v4.slots.len() + self.lists_side.v6.slots.len()
    }

    /// The memory the index holds, in bytes (a count from the element sizes, not an allocator reading).
    pub fn approx_bytes(&self) -> usize {
        let fam = |s: &Side| s.v4.bytes() + s.v6.bytes();
        fam(&self.lists_side)
            + fam(&self.allow_side)
            + self.hosts.iter().map(|(k, v)| k.len() + 64 + v.len() * 24).sum::<usize>()
            + self.notes.values().map(|v| v.len() + 48).sum::<usize>()
    }

    fn matches_for(&self, ip: &IpAddr, side: &Side, allow: bool) -> Vec<(u8, String, Option<String>)> {
        // (list, entry text, note)
        let mut out = Vec::new();
        let note = |list: u8, start: u128, prefix: u8| self.notes.get(&(list, start, prefix)).map(|n| n.to_string());
        match unwrapped(ip) {
            IpAddr::V4(a) => {
                if let Some(ms) = side.v4.find(u32::from(a)) {
                    for m in ms {
                        let s = &side.v4.slots[m.slot as usize];
                        for l in (0..32u8).filter(|l| m.lists & (1 << l) != 0) {
                            let key = if allow { ALLOW } else { l };
                            out.push((l, show(v4_addr(s.start), s.prefix), note(key, u128::from(s.start), s.prefix)));
                        }
                    }
                }
            }
            IpAddr::V6(a) => {
                if let Some(ms) = side.v6.find(u128::from(a)) {
                    for m in ms {
                        let s = &side.v6.slots[m.slot as usize];
                        for l in (0..32u8).filter(|l| m.lists & (1 << l) != 0) {
                            let key = if allow { ALLOW } else { l };
                            out.push((l, show(IpAddr::V6(Ipv6Addr::from(s.start)), s.prefix), note(key, s.start, s.prefix)));
                        }
                    }
                }
            }
        }
        out
    }

    /// The verdict for `ip`. `reverse_name` is a reverse name Dyno **already holds** (the user asked for names on the page); it
    /// is never looked up here.
    pub fn check(&self, ip: &IpAddr, reverse_name: Option<&str>) -> Verdict {
        if !is_public(ip) {
            return Verdict::Clean;
        }
        // The narrowest allow entry that covers the address overrides every list.
        if let Some((_, entry, note)) = self.matches_for(ip, &self.allow_side, true).into_iter().next() {
            return Verdict::Allowed { entry, note };
        }
        let mut found: Vec<ListMatch> = self
            .matches_for(ip, &self.lists_side, false)
            .into_iter()
            .map(|(l, entry, note)| {
                let (id, name) = &self.lists[l as usize];
                ListMatch {
                    list_id: id.clone(),
                    list_name: name.clone(),
                    entry,
                    note,
                    by: MatchedBy::Address,
                }
            })
            .collect();
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
                                note: note.as_deref().map(str::to_string),
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
