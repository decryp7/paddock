//! One byte budget for what the app keeps in memory for the user's open captures and documents (profiles, heap
//! snapshots, dumps), across all windows (MEM-2 of the Debrief 1.2 plan).
//!
//! Every structure is *admitted* with its size before it is kept. When the total would pass the limit, the least recently
//! used structures that can be loaded again from their file are *released* (their owner drops the body and reads the file
//! again on the next use). A structure that cannot be released (it has no file) stays. When even that is not enough,
//! the admission fails with an [`OverBudget`] that names the document and what to do. Sizes are estimates, set by the
//! owner; the limit is a Setting (Auto: a quarter of the memory, between 512 MB and 8 GB).
//!
//! Pure and bounded: no I/O, no clock (use order is a counter), one lock held only for bookkeeping; the owners' release
//! callbacks run with no lock held.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

/// The smallest limit Auto chooses, in bytes.
pub const AUTO_MIN: u64 = 512 << 20;
/// The largest limit Auto chooses, in bytes.
pub const AUTO_MAX: u64 = 8 << 30;

/// The limit Auto gives a machine with `ram_bytes` of memory: a quarter of it, between [`AUTO_MIN`] and [`AUTO_MAX`].
pub fn auto_limit(ram_bytes: u64) -> u64 {
    (ram_bytes / 4).clamp(AUTO_MIN, AUTO_MAX)
}

/// What the owner of a structure does when the budget needs its memory: drop the body (it is read again from its file
/// on the next use).
pub trait Release: Send + Sync {
    /// Drops the body; the next use reads it again from its file.
    fn release(&self);
}

/// One structure in the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    /// What it is: `profile`, `heap snapshot`, `dump`, `UMDH log`.
    pub kind: &'static str,
    /// The document's name, as the user knows it.
    pub name: String,
    /// Estimated bytes in memory (0 when released).
    pub bytes: u64,
    /// Whether the body is in memory now.
    pub resident: bool,
    /// Whether it can be released (it has a file to reload from).
    pub releasable: bool,
}

/// An admission that did not fit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverBudget {
    /// The structure that was being admitted.
    pub wanted: Held,
    /// The needed bytes of the structure being admitted (what it would add).
    pub needs: u64,
    /// The limit in force, in bytes.
    pub limit: u64,
    /// Resident structures that cannot be released, largest first.
    pub pinned: Vec<Held>,
}

fn mb(bytes: u64) -> String {
    format!("{} MB", bytes.div_ceil(1 << 20))
}

impl fmt::Display for OverBudget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let w = &self.wanted;
        if self.pinned.is_empty() || self.needs > self.limit {
            write!(
                f,
                "{} \"{}\" needs about {}, more than the {} Debrief may keep for open captures and documents. Raise Settings > Performance > Memory for open documents, or open a smaller file.",
                w.kind,
                w.name,
                mb(self.needs),
                mb(self.limit)
            )
        } else {
            let names: Vec<String> = self
                .pinned
                .iter()
                .take(3)
                .map(|h| format!("\"{}\" ({})", h.name, mb(h.bytes)))
                .collect();
            write!(
                f,
                "Opening {} \"{}\" (about {}) would pass the {} Debrief may keep for open captures and documents; these cannot be released to make room: {}. Close one of them, or raise Settings > Performance > Memory for open documents.",
                w.kind,
                w.name,
                mb(self.needs),
                mb(self.limit),
                names.join(", ")
            )
        }
    }
}

impl std::error::Error for OverBudget {}

struct Item {
    kind: &'static str,
    name: String,
    bytes: u64,
    resident: bool,
    last_used: u64,
    release: Option<Weak<dyn Release>>,
}

impl Item {
    fn held(&self) -> Held {
        Held {
            kind: self.kind,
            name: self.name.clone(),
            bytes: if self.resident { self.bytes } else { 0 },
            resident: self.resident,
            releasable: self.release.is_some(),
        }
    }
}

#[derive(Default)]
struct Inner {
    next: u64,
    tick: u64,
    items: BTreeMap<u64, Item>,
}

impl Inner {
    fn resident_total(&self) -> u64 {
        self.items.values().filter(|i| i.resident).map(|i| i.bytes).sum()
    }

    /// One line per structure for the log, with no names (they are the user's): kind, size and whether it is in memory,
    /// most recently used first.
    fn log_line(&self) -> String {
        let mut rows: Vec<&Item> = self.items.values().collect();
        rows.sort_by_key(|i| std::cmp::Reverse(i.last_used));
        let parts: Vec<String> = rows
            .iter()
            .map(|i| {
                format!(
                    "{} {} MB{}{}",
                    i.kind,
                    i.bytes >> 20,
                    if i.resident { "" } else { " (released)" },
                    if i.release.is_some() { "" } else { " (no file)" }
                )
            })
            .collect();
        format!(
            "{} structures, {} MB in memory: {}",
            rows.len(),
            self.resident_total() >> 20,
            parts.join("; ")
        )
    }
}

/// The budget; one per app, shared by every window's stores.
pub struct MemoryBudget {
    limit: AtomicU64,
    inner: Mutex<Inner>,
}

impl MemoryBudget {
    /// A budget of `limit_bytes`, shared through the returned `Arc`.
    pub fn new(limit_bytes: u64) -> Arc<MemoryBudget> {
        Arc::new(MemoryBudget {
            limit: AtomicU64::new(limit_bytes),
            inner: Mutex::new(Inner::default()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The limit in force, in bytes.
    pub fn limit(&self) -> u64 {
        self.limit.load(Ordering::Relaxed)
    }

    /// Changes the limit (a Setting). Nothing is released now; the next admission makes the room.
    pub fn set_limit(&self, bytes: u64) {
        self.limit.store(bytes, Ordering::Relaxed);
    }

    /// Bytes of the structures in memory now.
    pub fn used(&self) -> u64 {
        self.lock().resident_total()
    }

    /// Every structure, most recently used first: for the diagnostic bundle and the log (names are the user's: callers
    /// log counts and sizes, not names).
    pub fn report(&self) -> Vec<Held> {
        let g = self.lock();
        let mut rows: Vec<(&u64, &Item)> = g.items.iter().collect();
        rows.sort_by_key(|(_, i)| std::cmp::Reverse(i.last_used));
        rows.into_iter().map(|(_, i)| i.held()).collect()
    }

    /// Admits a structure of `bytes` (an estimate), releasing least recently used ones that can be reloaded if the total
    /// would pass the limit. The ticket removes it from the budget when dropped.
    pub fn admit(
        self: &Arc<Self>,
        kind: &'static str,
        name: &str,
        bytes: u64,
        release: Option<Weak<dyn Release>>,
    ) -> Result<Ticket, OverBudget> {
        let id = {
            let mut g = self.lock();
            let id = g.next;
            g.next += 1;
            g.tick += 1;
            let last_used = g.tick;
            g.items.insert(
                id,
                Item {
                    kind,
                    name: name.to_string(),
                    bytes,
                    resident: false,
                    last_used,
                    release,
                },
            );
            id
        };
        let ticket = Ticket { budget: self.clone(), id };
        ticket.readmit(bytes)?;
        Ok(ticket)
    }
}

/// A structure's place in the budget.
pub struct Ticket {
    budget: Arc<MemoryBudget>,
    id: u64,
}

impl Ticket {
    /// Marks the structure as just used (it is released last).
    pub fn touch(&self) {
        let mut g = self.budget.lock();
        g.tick += 1;
        let t = g.tick;
        if let Some(i) = g.items.get_mut(&self.id) {
            i.last_used = t;
        }
    }

    /// Whether the body is in memory according to the budget.
    pub fn resident(&self) -> bool {
        self.budget.lock().items.get(&self.id).is_some_and(|i| i.resident)
    }

    /// The structure (again) takes `bytes` in memory: after a release, a reload. Releases others as needed.
    pub fn readmit(&self, bytes: u64) -> Result<(), OverBudget> {
        let limit = self.budget.limit();
        let to_release: Vec<Arc<dyn Release>> = {
            let mut g = self.budget.lock();
            g.tick += 1;
            let t = g.tick;
            let others_resident: u64 = g
                .items
                .iter()
                .filter(|(k, i)| **k != self.id && i.resident)
                .map(|(_, i)| i.bytes)
                .sum();
            let mut candidates: Vec<(u64, u64, u64)> = g
                .items
                .iter()
                .filter(|(k, i)| **k != self.id && i.resident && i.release.is_some())
                .map(|(k, i)| (i.last_used, *k, i.bytes))
                .collect();
            candidates.sort();
            let releasable: u64 = candidates.iter().map(|c| c.2).sum();
            let pinned_total = others_resident - releasable;
            if pinned_total + bytes > limit {
                let wanted = g.items.get(&self.id).map(Item::held).unwrap_or(Held {
                    kind: "document",
                    name: String::new(),
                    bytes: 0,
                    resident: false,
                    releasable: false,
                });
                let mut pinned: Vec<Held> = g
                    .items
                    .iter()
                    .filter(|(k, i)| **k != self.id && i.resident && i.release.is_none())
                    .map(|(_, i)| i.held())
                    .collect();
                pinned.sort_by_key(|h| std::cmp::Reverse(h.bytes));
                log::warn!(
                    "memory budget: a {} of about {} MB does not fit the {} MB limit ({} MB cannot be released); now {}",
                    wanted.kind,
                    bytes >> 20,
                    limit >> 20,
                    pinned_total >> 20,
                    g.log_line()
                );
                return Err(OverBudget {
                    wanted,
                    needs: bytes,
                    limit,
                    pinned,
                });
            }
            let mut total = others_resident + bytes;
            let mut out = Vec::new();
            for (_, key, size) in candidates {
                if total <= limit {
                    break;
                }
                if let Some(item) = g.items.get_mut(&key) {
                    item.resident = false;
                    if let Some(r) = item.release.as_ref().and_then(Weak::upgrade) {
                        out.push(r);
                    }
                    total -= size;
                    log::info!("memory budget: released a {} of about {} MB to make room", item.kind, size >> 20);
                }
            }
            if let Some(item) = g.items.get_mut(&self.id) {
                item.bytes = bytes;
                item.resident = true;
                item.last_used = t;
            }
            if !out.is_empty() {
                log::info!("memory budget after releasing {}: {}", out.len(), g.log_line());
            }
            out
        };
        // Outside the lock: an owner's release takes its own lock.
        for r in to_release {
            r.release();
        }
        Ok(())
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.budget.lock().items.remove(&self.id);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test code: a failed unwrap is the failed test")]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    const MB: u64 = 1 << 20;

    #[derive(Default)]
    struct Body(AtomicBool);
    impl Release for Body {
        fn release(&self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    fn weak(b: &Arc<Body>) -> Option<Weak<dyn Release>> {
        let w: Weak<dyn Release> = Arc::downgrade(&(b.clone() as Arc<dyn Release>));
        Some(w)
    }

    #[test]
    fn the_least_recently_used_structure_is_released_first() {
        let budget = MemoryBudget::new(100 * MB);
        let (a, b, c) = (Arc::new(Body::default()), Arc::new(Body::default()), Arc::new(Body::default()));
        let ta = budget.admit("profile", "a.folded", 40 * MB, weak(&a)).unwrap();
        let _tb = budget.admit("profile", "b.folded", 40 * MB, weak(&b)).unwrap();
        ta.touch();
        let _tc = budget.admit("heap snapshot", "c.heapsnapshot", 40 * MB, weak(&c)).unwrap();
        assert!(!a.0.load(Ordering::SeqCst), "a was used last of the two");
        assert!(b.0.load(Ordering::SeqCst), "b was the least recently used");
        assert_eq!(budget.used(), 80 * MB);
        assert!(ta.resident());
        let rows = budget.report();
        assert_eq!(rows.iter().filter(|r| !r.resident).count(), 1);
    }

    #[test]
    fn a_release_reloads_through_readmit_and_releases_another() {
        let budget = MemoryBudget::new(100 * MB);
        let (a, b) = (Arc::new(Body::default()), Arc::new(Body::default()));
        let ta = budget.admit("profile", "a", 60 * MB, weak(&a)).unwrap();
        let tb = budget.admit("profile", "b", 60 * MB, weak(&b)).unwrap();
        assert!(a.0.load(Ordering::SeqCst) && !ta.resident());
        ta.readmit(60 * MB).unwrap();
        assert!(b.0.load(Ordering::SeqCst) && !tb.resident());
        assert_eq!(budget.used(), 60 * MB);
    }

    #[test]
    fn structures_without_a_file_stay_and_the_error_names_the_document_and_what_to_do() {
        let budget = MemoryBudget::new(100 * MB);
        let _pinned = budget.admit("dump", "crash.dmp", 70 * MB, None).unwrap();
        let big = Arc::new(Body::default());
        let err = budget
            .admit("heap snapshot", "huge.heapsnapshot", 50 * MB, weak(&big))
            .err()
            .unwrap();
        let text = err.to_string();
        assert!(text.contains("huge.heapsnapshot") && text.contains("crash.dmp"), "{text}");
        assert!(
            text.contains("Close one of them") && text.contains("Memory for open documents"),
            "{text}"
        );
        assert_eq!(budget.used(), 70 * MB, "the failed admission left nothing behind");
        assert!(!big.0.load(Ordering::SeqCst));
        let alone = budget.admit("profile", "x", 500 * MB, None).err().unwrap().to_string();
        assert!(
            alone.contains("more than the 100 MB") && alone.contains("open a smaller file"),
            "{alone}"
        );
    }

    #[test]
    fn nothing_is_released_when_it_would_not_be_enough() {
        let budget = MemoryBudget::new(100 * MB);
        let a = Arc::new(Body::default());
        let _t = budget.admit("profile", "a", 30 * MB, weak(&a)).unwrap();
        let _p = budget.admit("dump", "pinned", 50 * MB, None).unwrap();
        assert!(budget.admit("profile", "big", 60 * MB, None).is_err());
        assert!(!a.0.load(Ordering::SeqCst), "a stays: releasing it would not have made the room");
        assert_eq!(budget.used(), 80 * MB);
    }

    #[test]
    fn dropping_a_ticket_frees_its_bytes_and_the_limit_can_change() {
        let budget = MemoryBudget::new(100 * MB);
        let t = budget.admit("profile", "a", 90 * MB, None).unwrap();
        assert!(budget.admit("profile", "b", 20 * MB, None).is_err());
        drop(t);
        assert_eq!(budget.used(), 0);
        budget.set_limit(10 * MB);
        assert!(budget.admit("profile", "c", 20 * MB, None).is_err());
        assert_eq!(budget.limit(), 10 * MB);
    }

    #[test]
    fn the_log_line_has_kinds_and_sizes_but_no_names() {
        let budget = MemoryBudget::new(100 * MB);
        let _a = budget.admit("profile", "secret-customer.folded", 30 * MB, None).unwrap();
        let line = budget.lock().log_line();
        assert!(line.contains("profile 30 MB (no file)") && !line.contains("secret"), "{line}");
    }

    #[test]
    fn auto_is_a_quarter_of_the_memory_between_the_bounds() {
        assert_eq!(auto_limit(1 << 30), AUTO_MIN);
        assert_eq!(auto_limit(16 << 30), 4 << 30);
        assert_eq!(auto_limit(256 << 30), AUTO_MAX);
    }
}
