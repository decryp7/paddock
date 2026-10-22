//! The caches of the running server (ADR 0019): the response cache and the embeddings cache, each kept in memory or in
//! `cache.sqlite` as the settings say. Policy (keys, expiry, bounds) is the core's; this is the wiring: choosing the
//! store, applying settings, the clean-up pass, and the numbers the UI shows.

use crate::cache_db::SqliteStore;
use pitwall_core::cache::{self, CacheStore, Info, Key, Kind, KindInfo, KindStats, MemoryStore, Meta, Scope, SWEEP_BATCH};
use pitwall_core::chat::ChatOutput;
use pitwall_core::config::{CacheSettings, CacheStorage};
use pitwall_core::time::{Millis, HOUR};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

/// How often the clean-up pass runs.
pub const SWEEP_EVERY: Millis = HOUR;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Both caches.
pub struct Caches {
    memory: MemoryStore,
    disk: Option<SqliteStore>,
    disk_error: Option<String>,
    settings: Mutex<CacheSettings>,
    last_sweep: AtomicU64,
}

impl Caches {
    /// Opens the caches with `settings`. A disk cache that cannot be opened is reported by [`Caches::info`] and the
    /// caches that asked for it use memory instead.
    pub fn open(db: &Path, settings: &CacheSettings, now: Millis) -> Caches {
        let (disk, disk_error) = match SqliteStore::open(db, settings.limits(Kind::Response), settings.limits(Kind::Embedding)) {
            Ok(d) => (Some(d), None),
            Err(e) => (None, Some(e)),
        };
        Caches {
            memory: MemoryStore::new(settings.limits(Kind::Response), settings.limits(Kind::Embedding)),
            disk,
            disk_error,
            settings: Mutex::new(settings.clone()),
            last_sweep: AtomicU64::new(now),
        }
    }

    fn store_for(&self, kind: Kind, s: &CacheSettings) -> &dyn CacheStore {
        match (s.storage(kind), &self.disk) {
            (CacheStorage::Disk, Some(d)) => d,
            _ => &self.memory,
        }
    }

    fn store(&self, kind: Kind) -> &dyn CacheStore {
        let s = lock(&self.settings);
        self.store_for(kind, &s)
    }

    /// Applies changed settings: new bounds at once, and what a cache held in the store it leaves is erased (choosing
    /// "memory only" must not leave answers on disk).
    pub fn apply(&self, new: &CacheSettings) {
        let old = std::mem::replace(&mut *lock(&self.settings), new.clone());
        for kind in Kind::ALL {
            self.memory.configure(kind, new.limits(kind));
            if let Some(d) = &self.disk {
                d.configure(kind, new.limits(kind));
            }
            if old.storage(kind) != new.storage(kind) {
                match old.storage(kind) {
                    CacheStorage::Memory => {
                        self.memory.clear(&Scope::Kind(kind));
                    }
                    CacheStorage::Disk => {
                        if let Some(d) = &self.disk {
                            d.clear(&Scope::Kind(kind));
                        }
                    }
                }
            }
        }
    }

    /// Whether embeddings are cached.
    pub fn embeddings_on(&self) -> bool {
        lock(&self.settings).embeddings
    }

    /// A stored answer.
    pub fn response(&self, key: &Key, now: Millis) -> Option<ChatOutput> {
        cache::decode(&self.store(Kind::Response).get(Kind::Response, key, now)?)
    }

    /// Stores a completed answer.
    pub fn store_response(&self, key: Key, out: &ChatOutput, meta: Meta, now: Millis) {
        if cache::storable(out) {
            self.store(Kind::Response).put(Kind::Response, key, cache::encode(out), meta, now);
        }
    }

    /// A stored embedding.
    pub fn vector(&self, key: &Key, now: Millis) -> Option<Vec<f32>> {
        cache::decode_vector(&self.store(Kind::Embedding).get(Kind::Embedding, key, now)?)
    }

    /// Stores an embedding.
    pub fn store_vector(&self, key: Key, v: &[f32], meta: Meta, now: Millis) {
        self.store(Kind::Embedding).put(Kind::Embedding, key, cache::encode_vector(v), meta, now);
    }

    /// Removes the entries in scope from both stores; returns how many went.
    pub fn clear(&self, scope: &Scope) -> u64 {
        self.memory.clear(scope) + self.disk.as_ref().map_or(0, |d| d.clear(scope))
    }

    /// One clean-up pass: removes expired entries, at most [`SWEEP_BATCH`] per cache and store.
    pub fn sweep(&self, now: Millis) -> u64 {
        self.last_sweep.store(now, Ordering::SeqCst);
        Kind::ALL
            .iter()
            .map(|k| self.memory.sweep(*k, now, SWEEP_BATCH) + self.disk.as_ref().map_or(0, |d| d.sweep(*k, now, SWEEP_BATCH)))
            .sum()
    }

    /// Whether the hourly pass is due.
    pub fn sweep_due(&self, now: Millis) -> bool {
        now.saturating_sub(self.last_sweep.load(Ordering::SeqCst)) >= SWEEP_EVERY
    }

    /// When the next pass runs.
    pub fn next_sweep_at(&self) -> Millis {
        self.last_sweep.load(Ordering::SeqCst).saturating_add(SWEEP_EVERY)
    }

    /// Waits for queued disk writes.
    pub fn flush(&self) {
        if let Some(d) = &self.disk {
            d.flush();
        }
    }

    /// What each cache holds, for Settings; the keep-warm numbers are the caller's.
    pub fn info(&self, keep_warm_files: u64, keep_warm_bytes: u64) -> Info {
        let s = lock(&self.settings).clone();
        let one = |kind: Kind| {
            let l = s.limits(kind);
            let st: KindStats = self.store_for(kind, &s).stats(kind);
            let disk_wanted = s.storage(kind) == CacheStorage::Disk;
            KindInfo {
                storage: if disk_wanted && self.disk.is_none() { CacheStorage::Memory } else { s.storage(kind) },
                entries: st.entries,
                bytes: st.bytes,
                max_bytes: l.max_bytes,
                hits: st.hits,
                oldest_at: st.oldest,
                expires_after_secs: l.expiry.ttl.map(|t| t / 1000),
                sliding: l.expiry.sliding,
                disk_error: if disk_wanted {
                    self.disk_error
                        .clone()
                        .or_else(|| self.disk.as_ref().and_then(SqliteStore::last_error))
                } else {
                    None
                },
            }
        };
        Info {
            responses: one(Kind::Response),
            embeddings: one(Kind::Embedding),
            keep_warm_files,
            keep_warm_bytes,
            next_sweep_at: self.next_sweep_at(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitwall_core::chat::{Finish, Usage};

    fn out(t: &str) -> ChatOutput {
        ChatOutput {
            text: t.into(),
            finish: Some(Finish::Stop),
            usage: Usage::default(),
            ..Default::default()
        }
    }

    fn meta() -> Meta {
        Meta {
            model: "m".into(),
            app: "a".into(),
            preset: String::new(),
        }
    }

    fn disk_settings() -> CacheSettings {
        CacheSettings::default()
    }

    #[test]
    fn answers_and_vectors_round_trip_on_disk_and_survive_a_restart() {
        let d = tempfile::tempdir().unwrap();
        let db = d.path().join("cache.sqlite");
        {
            let c = Caches::open(&db, &disk_settings(), 0);
            c.store_response([1; 32], &out("hi"), meta(), 10);
            c.store_response([2; 32], &ChatOutput { finish: None, ..out("cut") }, meta(), 10);
            c.store_vector([3; 32], &[0.5, 1.5], meta(), 10);
            c.flush();
            assert_eq!(c.response(&[1; 32], 11).unwrap().text, "hi");
            assert!(c.response(&[2; 32], 11).is_none(), "unfinished answers are not stored");
            assert_eq!(c.vector(&[3; 32], 11).unwrap(), vec![0.5, 1.5]);
            let info = c.info(1, 2);
            assert_eq!((info.responses.entries, info.embeddings.entries), (1, 1));
            assert_eq!((info.keep_warm_files, info.keep_warm_bytes), (1, 2));
            assert_eq!(info.responses.expires_after_secs, Some(7 * 24 * 3600));
            assert_eq!(info.embeddings.expires_after_secs, Some(90 * 24 * 3600));
            assert!(info.embeddings.sliding && !info.responses.sliding);
            assert_eq!(info.responses.storage, CacheStorage::Disk);
            assert!(info.responses.disk_error.is_none());
        }
        let c = Caches::open(&db, &disk_settings(), 0);
        assert_eq!(c.response(&[1; 32], 20).unwrap().text, "hi");
        assert!(c.embeddings_on());
    }

    #[test]
    fn memory_only_keeps_nothing_on_disk_and_choosing_it_erases_what_was_there() {
        let d = tempfile::tempdir().unwrap();
        let db = d.path().join("cache.sqlite");
        let c = Caches::open(&db, &disk_settings(), 0);
        c.store_vector([1; 32], &[1.0], meta(), 1);
        c.flush();
        assert_eq!(c.info(0, 0).embeddings.entries, 1);
        let mem = CacheSettings {
            embeddings_storage: CacheStorage::Memory,
            ..disk_settings()
        };
        c.apply(&mem);
        assert_eq!(c.info(0, 0).embeddings.entries, 0, "the disk copy was erased");
        c.store_vector([2; 32], &[1.0], meta(), 2);
        assert_eq!(c.info(0, 0).embeddings.storage, CacheStorage::Memory);
        drop(c);
        let again = Caches::open(&db, &disk_settings(), 0);
        assert!(again.vector(&[2; 32], 3).is_none(), "nothing reached the disk");
        // And back: what memory held goes.
        let c = Caches::open(&db, &mem, 0);
        c.store_vector([4; 32], &[1.0], meta(), 1);
        c.apply(&disk_settings());
        assert!(c.vector(&[4; 32], 2).is_none());
    }

    #[test]
    fn an_unopenable_disk_cache_falls_back_to_memory_and_says_why() {
        let d = tempfile::tempdir().unwrap();
        let c = Caches::open(&d.path().join("no").join("cache.sqlite"), &disk_settings(), 0);
        c.store_vector([1; 32], &[1.0], meta(), 1);
        assert!(c.vector(&[1; 32], 2).is_some(), "memory serves");
        let info = c.info(0, 0);
        assert_eq!(info.embeddings.storage, CacheStorage::Memory);
        assert!(info.embeddings.disk_error.as_deref().unwrap().contains("cache database"));
        c.flush();
    }

    #[test]
    fn the_clean_up_pass_is_hourly_and_bounded_and_clearing_covers_both_stores() {
        let d = tempfile::tempdir().unwrap();
        let s = CacheSettings {
            responses_ttl_secs: 60,
            responses_storage: CacheStorage::Memory,
            ..disk_settings()
        };
        let c = Caches::open(&d.path().join("c.sqlite"), &s, 1000);
        assert!(!c.sweep_due(1000 + HOUR - 1) && c.sweep_due(1000 + HOUR));
        assert_eq!(c.next_sweep_at(), 1000 + HOUR);
        c.store_response([1; 32], &out("a"), meta(), 0);
        c.store_vector([2; 32], &[1.0], meta(), 0);
        assert_eq!(c.sweep(70_000), 1, "the answer expired (60 s); the embedding did not");
        assert!(!c.sweep_due(70_000 + 1) && c.next_sweep_at() == 70_000 + HOUR);
        assert_eq!(c.clear(&Scope::All), 1);
        let never = CacheSettings {
            responses_ttl_secs: 0,
            ..s
        };
        c.apply(&never);
        assert_eq!(c.info(0, 0).responses.expires_after_secs, None);
    }
}
