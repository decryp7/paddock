//! Caching policy (ADR 0019), pure: which requests may be answered from a cache, the keys, replaying a stored answer
//! as a stream, and an in-memory store. The disk store (`cache.sqlite`) is an adapter in `pitwall-server`; both
//! implement [`CacheStore`].
//!
//! Two caches live here. The **response cache** answers identical deterministic chat requests (temperature 0 or a fixed
//! seed, no tools) from a stored answer; an app or a preset must opt in. The **embeddings cache** stores one vector per
//! input text; embeddings are deterministic, so it is on by default. Prompt-prefix reuse is llama-server's
//! (`cache_prompt`, slot affinity in [`crate::affinity`]).
//!
//! A key is the SHA-256 of everything that can change the answer: the model file's identity, the engine build, every
//! generation parameter and the full messages. The API key and the app never enter it, so apps that opted in share
//! answers.

use crate::chat::{ChatEvent, ChatOutput, ChatRequest, Finish};
use crate::time::Millis;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

/// A cache key: SHA-256 of the fingerprint and the canonical request.
pub type Key = [u8; 32];

/// What the response cache did for a request (`X-Pitwall-Cache`, `CacheOutcome` in `api.types.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// Answered from the cache.
    Hit,
    /// Eligible, not found; the answer is stored when it completes.
    Miss,
    /// The caller asked not to use the cache (`Cache-Control: no-cache`).
    Bypass,
}

impl Outcome {
    /// The `X-Pitwall-Cache` header value.
    pub fn header(self) -> &'static str {
        match self {
            Outcome::Hit => "hit",
            Outcome::Miss => "miss",
            Outcome::Bypass => "bypass",
        }
    }
}

/// Whether a `Cache-Control` request header asks to skip the cache (`no-cache` or `no-store`).
pub fn bypass(cache_control: Option<&str>) -> bool {
    cache_control.is_some_and(|v| {
        v.split(',')
            .map(|d| d.trim().to_ascii_lowercase())
            .any(|d| d == "no-cache" || d == "no-store")
    })
}

/// Whether a chat request may be cached: deterministic and with no tools to run.
pub fn cacheable(req: &ChatRequest) -> bool {
    (req.temperature == Some(0.0) || req.seed.is_some()) && req.tools.is_empty()
}

/// Whether a finished answer may be stored: it completed (not cancelled) and called no tools.
pub fn storable(out: &ChatOutput) -> bool {
    matches!(out.finish, Some(Finish::Stop | Finish::Length)) && out.tool_calls.is_empty()
}

/// The key of a chat request. `fingerprint` names the model file, the engine and its build (see
/// [`fingerprint`]). `stream` and `priority` do not change the answer, so they are not part of the key.
pub fn key(fingerprint: &str, req: &ChatRequest) -> Key {
    let canonical = ChatRequest {
        stream: false,
        priority: None,
        ..req.clone()
    };
    let mut h = Sha256::new();
    h.update(b"chat\0");
    h.update(fingerprint.as_bytes());
    h.update([0]);
    h.update(serde_json::to_vec(&canonical).unwrap_or_default());
    h.finalize().into()
}

/// The key of one embedding input.
pub fn embed_key(fingerprint: &str, input: &str) -> Key {
    let mut h = Sha256::new();
    h.update(b"embed\0");
    h.update(fingerprint.as_bytes());
    h.update([0]);
    h.update(input.as_bytes());
    h.finalize().into()
}

/// What identifies the thing that computes the answer: the model id, the model file (size and modified time, or a
/// hash when one is known), the engine and its build. A changed file, engine or build gives other keys, so stale
/// answers can never be served.
pub fn fingerprint(model: &str, file_version: &str, engine: &str, build: &str) -> String {
    format!("{model}\u{1f}{file_version}\u{1f}{engine}\u{1f}{build}")
}

/// A stored answer as the events a stream would have carried (answer text in pieces, so streams look like streams).
pub fn replay(out: &ChatOutput) -> Vec<ChatEvent> {
    const PIECE: usize = 96;
    let mut events = vec![];
    let mut pieces = |text: &str, make: fn(String) -> ChatEvent| {
        let mut rest = text;
        while !rest.is_empty() {
            let mut end = rest.len().min(PIECE);
            while !rest.is_char_boundary(end) {
                end += 1;
            }
            let (a, b) = rest.split_at(end);
            events.push(make(a.to_string()));
            rest = b;
        }
    };
    pieces(&out.reasoning, ChatEvent::Reasoning);
    pieces(&out.text, ChatEvent::Delta);
    events.push(ChatEvent::Done {
        finish: out.finish.unwrap_or(Finish::Stop),
        usage: out.usage,
        timings: None,
    });
    events
}

/// A stored answer as bytes.
pub fn encode(out: &ChatOutput) -> Vec<u8> {
    serde_json::to_vec(out).unwrap_or_default()
}

/// A stored answer from bytes (`None` when the bytes are damaged).
pub fn decode(bytes: &[u8]) -> Option<ChatOutput> {
    serde_json::from_slice(bytes).ok().filter(storable)
}

/// A vector as little-endian bytes.
pub fn encode_vector(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// A vector from bytes (`None` when the length is not a whole number of floats).
pub fn decode_vector(b: &[u8]) -> Option<Vec<f32>> {
    (b.len() % 4 == 0).then(|| b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

/// Which cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// Chat answers.
    Response,
    /// Embedding vectors.
    Embedding,
}

impl Kind {
    /// Both kinds.
    pub const ALL: [Kind; 2] = [Kind::Response, Kind::Embedding];

    /// A stable name (the disk store's column).
    pub fn name(self) -> &'static str {
        match self {
            Kind::Response => "response",
            Kind::Embedding => "embedding",
        }
    }
}

/// What an entry remembers besides its value, for scoped clearing and the listing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Meta {
    /// The model id.
    pub model: String,
    /// The app that created the entry (its id, never its key).
    pub app: String,
}

/// What to clear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Everything.
    All,
    /// One cache.
    Kind(Kind),
    /// Entries one app created.
    App(String),
    /// Entries of one model.
    Model(String),
}

impl Scope {
    /// Whether an entry of `kind` with `meta` is in scope.
    pub fn covers(&self, kind: Kind, meta: &Meta) -> bool {
        match self {
            Scope::All => true,
            Scope::Kind(k) => *k == kind,
            Scope::App(a) => &meta.app == a,
            Scope::Model(m) => &meta.model == m,
        }
    }
}

/// How long an entry is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expiry {
    /// Milliseconds; `None`: never (only the size cap removes entries).
    pub ttl: Option<Millis>,
    /// Counted from the last use (sliding) instead of from when the entry was stored.
    pub sliding: bool,
}

impl Expiry {
    /// Whether an entry stored at `stored` and last used at `used` is expired at `now`. An expired entry is never served,
    /// whether or not a sweep removed it yet.
    pub fn expired(self, stored: Millis, used: Millis, now: Millis) -> bool {
        let from = if self.sliding { used } else { stored };
        self.ttl.is_some_and(|t| now.saturating_sub(from) >= t)
    }

    /// The instant an entry stored at `stored` and last used at `used` expires (`None`: never).
    pub fn at(self, stored: Millis, used: Millis) -> Option<Millis> {
        self.ttl.map(|t| (if self.sliding { used } else { stored }).saturating_add(t))
    }
}

/// One cache as the UI shows it (`CacheKindInfo` in `api.types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KindInfo {
    /// Where it is kept.
    pub storage: crate::config::CacheStorage,
    /// Entries held.
    pub entries: u64,
    /// Bytes of values held.
    pub bytes: u64,
    /// The size cap.
    pub max_bytes: u64,
    /// Hits served by the entries held.
    pub hits: u64,
    /// When the oldest entry was stored.
    pub oldest_at: Option<Millis>,
    /// Seconds an entry is kept (`None`: never).
    pub expires_after_secs: Option<u64>,
    /// The time counts from the last use (otherwise from when the entry was stored).
    pub sliding: bool,
    /// Why the disk cache is not in use (it then behaves as memory only), if so.
    pub disk_error: Option<String>,
}

/// Both caches (`CacheInfo` in `api.types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    /// Chat answers.
    pub responses: KindInfo,
    /// Embedding vectors.
    pub embeddings: KindInfo,
    /// Saved prompt prefixes (keep warm): files and bytes.
    pub keep_warm_files: u64,
    /// Bytes of the saved prefixes.
    pub keep_warm_bytes: u64,
    /// When the next clean-up pass runs.
    pub next_sweep_at: Millis,
}

/// What the UI asks to clear (`CacheClearScope` in `api.types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ClearRequest {
    /// Everything.
    All,
    /// The response cache.
    Responses,
    /// The embeddings cache.
    Embeddings,
    /// What one app caused.
    #[serde(rename_all = "camelCase")]
    App {
        /// The app id.
        app_id: String,
    },
    /// What one model computed.
    #[serde(rename_all = "camelCase")]
    Model {
        /// The model id.
        model_id: String,
    },
}

impl ClearRequest {
    /// The scope to clear.
    pub fn scope(&self) -> Scope {
        match self {
            ClearRequest::All => Scope::All,
            ClearRequest::Responses => Scope::Kind(Kind::Response),
            ClearRequest::Embeddings => Scope::Kind(Kind::Embedding),
            ClearRequest::App { app_id } => Scope::App(app_id.clone()),
            ClearRequest::Model { model_id } => Scope::Model(model_id.clone()),
        }
    }
}

/// Bounds of one cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Most bytes of values held.
    pub max_bytes: u64,
    /// How long an entry is kept.
    pub expiry: Expiry,
}

/// Most entries one sweep pass removes, so a pass stays short.
pub const SWEEP_BATCH: u64 = 5000;

/// What a cache holds now (`CacheKindInfo` in `api.types.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KindStats {
    /// Entries.
    pub entries: u64,
    /// Bytes of values.
    pub bytes: u64,
    /// Hits served by the entries held.
    pub hits: u64,
    /// When the oldest entry was stored.
    pub oldest: Option<Millis>,
}

/// A cache backend (a port). Implementations never panic and never fail a request: a store that cannot read or write
/// behaves as an empty one.
pub trait CacheStore: Send + Sync {
    /// The value under `key`, if it is held and not expired; counts a hit.
    fn get(&self, kind: Kind, key: &Key, now: Millis) -> Option<Vec<u8>>;
    /// Stores a value (replacing one), evicting least recently used entries to stay within the limits. A value larger
    /// than the whole cache is not stored.
    fn put(&self, kind: Kind, key: Key, value: Vec<u8>, meta: Meta, now: Millis);
    /// Removes the entries in scope; returns how many.
    fn clear(&self, scope: &Scope) -> u64;
    /// What `kind` holds.
    fn stats(&self, kind: Kind) -> KindStats;
    /// Sets the bounds of `kind`; entries over them go at once.
    fn configure(&self, kind: Kind, limits: Limits);
    /// Removes expired entries of `kind`, at most `max` (the next pass continues); returns how many went.
    fn sweep(&self, kind: Kind, now: Millis, max: u64) -> u64;
    /// Waits until queued writes reached the backend.
    fn flush(&self) {}
}

struct Entry {
    value: Vec<u8>,
    meta: Meta,
    stored: Millis,
    used: Millis,
    hits: u64,
}

struct Part {
    limits: Limits,
    bytes: u64,
    entries: HashMap<Key, Entry>,
}

impl Part {
    fn remove(&mut self, k: &Key) {
        if let Some(e) = self.entries.remove(k) {
            self.bytes -= e.value.len() as u64;
        }
    }

    fn evict_to(&mut self, budget: u64) {
        while self.bytes > budget {
            let Some(oldest) = self.entries.iter().min_by_key(|(_, e)| e.used).map(|(k, _)| *k) else {
                break;
            };
            self.remove(&oldest);
        }
    }
}

/// The in-memory store.
pub struct MemoryStore {
    parts: Mutex<HashMap<Kind, Part>>,
}

impl MemoryStore {
    /// An empty store with the given bounds per kind.
    pub fn new(response: Limits, embedding: Limits) -> MemoryStore {
        let part = |limits| Part {
            limits,
            bytes: 0,
            entries: HashMap::new(),
        };
        MemoryStore {
            parts: Mutex::new(HashMap::from([(Kind::Response, part(response)), (Kind::Embedding, part(embedding))])),
        }
    }

    fn parts(&self) -> MutexGuard<'_, HashMap<Kind, Part>> {
        self.parts.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl CacheStore for MemoryStore {
    fn get(&self, kind: Kind, key: &Key, now: Millis) -> Option<Vec<u8>> {
        let mut parts = self.parts();
        let p = parts.get_mut(&kind)?;
        let exp = p.limits.expiry;
        if p.entries.get(key).is_some_and(|e| exp.expired(e.stored, e.used, now)) {
            p.remove(key);
            return None;
        }
        let e = p.entries.get_mut(key)?;
        e.used = now;
        e.hits += 1;
        Some(e.value.clone())
    }

    fn put(&self, kind: Kind, key: Key, value: Vec<u8>, meta: Meta, now: Millis) {
        let mut parts = self.parts();
        let Some(p) = parts.get_mut(&kind) else { return };
        let size = value.len() as u64;
        if size > p.limits.max_bytes {
            return;
        }
        p.remove(&key);
        p.evict_to(p.limits.max_bytes - size);
        p.bytes += size;
        p.entries.insert(
            key,
            Entry {
                value,
                meta,
                stored: now,
                used: now,
                hits: 0,
            },
        );
    }

    fn clear(&self, scope: &Scope) -> u64 {
        let mut n = 0;
        for (kind, p) in self.parts().iter_mut() {
            let doomed: Vec<Key> = p.entries.iter().filter(|(_, e)| scope.covers(*kind, &e.meta)).map(|(k, _)| *k).collect();
            n += doomed.len() as u64;
            for k in doomed {
                p.remove(&k);
            }
        }
        n
    }

    fn stats(&self, kind: Kind) -> KindStats {
        self.parts().get(&kind).map_or_else(KindStats::default, |p| KindStats {
            entries: p.entries.len() as u64,
            bytes: p.bytes,
            hits: p.entries.values().map(|e| e.hits).sum(),
            oldest: p.entries.values().map(|e| e.stored).min(),
        })
    }

    fn configure(&self, kind: Kind, limits: Limits) {
        if let Some(p) = self.parts().get_mut(&kind) {
            p.limits = limits;
            p.evict_to(limits.max_bytes);
        }
    }

    fn sweep(&self, kind: Kind, now: Millis, max: u64) -> u64 {
        let mut parts = self.parts();
        let Some(p) = parts.get_mut(&kind) else { return 0 };
        let exp = p.limits.expiry;
        let doomed: Vec<Key> = p
            .entries
            .iter()
            .filter(|(_, e)| exp.expired(e.stored, e.used, now))
            .map(|(k, _)| *k)
            .take(usize::try_from(max).unwrap_or(usize::MAX))
            .collect();
        for k in &doomed {
            p.remove(k);
        }
        doomed.len() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{Message, Role, Tool, ToolCall, Usage};

    fn req(t: Option<f32>) -> ChatRequest {
        ChatRequest {
            messages: vec![Message::text(Role::User, "hi")],
            temperature: t,
            ..Default::default()
        }
    }

    fn out(text: &str) -> ChatOutput {
        ChatOutput {
            text: text.into(),
            finish: Some(Finish::Stop),
            usage: Usage {
                prompt_tokens: 3,
                completion_tokens: 2,
            },
            ..Default::default()
        }
    }

    const FP: &str = "m\u{1f}v1\u{1f}llama.cpp\u{1f}b1";

    fn lim(max: u64, ttl: Millis) -> Limits {
        Limits {
            max_bytes: max,
            expiry: Expiry {
                ttl: Some(ttl),
                sliding: false,
            },
        }
    }

    fn mem(max: u64, ttl: Millis) -> MemoryStore {
        MemoryStore::new(lim(max, ttl), lim(max, ttl))
    }

    fn put(s: &MemoryStore, k: Key, v: &[u8], now: Millis) {
        s.put(Kind::Response, k, v.to_vec(), Meta::default(), now);
    }

    #[test]
    fn what_is_cacheable() {
        assert!(cacheable(&req(Some(0.0))));
        assert!(!cacheable(&req(Some(0.7))));
        assert!(!cacheable(&req(None)));
        assert!(cacheable(&ChatRequest {
            seed: Some(1),
            ..req(None)
        }));
        let tools = ChatRequest {
            tools: vec![Tool {
                name: "t".into(),
                description: String::new(),
                parameters: serde_json::Value::Null,
            }],
            ..req(Some(0.0))
        };
        assert!(!cacheable(&tools));
    }

    #[test]
    fn what_is_storable() {
        assert!(storable(&out("a")));
        assert!(storable(&ChatOutput {
            finish: Some(Finish::Length),
            ..out("a")
        }));
        assert!(!storable(&ChatOutput {
            finish: Some(Finish::Cancelled),
            ..out("a")
        }));
        assert!(!storable(&ChatOutput { finish: None, ..out("a") }));
        assert!(!storable(&ChatOutput {
            tool_calls: vec![ToolCall {
                id: "1".into(),
                name: "t".into(),
                arguments: "{}".into()
            }],
            ..out("a")
        }));
    }

    #[test]
    fn keys_cover_everything_that_changes_the_answer() {
        let base = key(FP, &req(Some(0.0)));
        // Streaming and priority don't change the answer.
        let streamed = ChatRequest {
            stream: true,
            priority: Some(crate::scheduler::Priority::Batch),
            ..req(Some(0.0))
        };
        assert_eq!(base, key(FP, &streamed));
        // Model file, engine, build, parameters and messages do.
        for other in [
            fingerprint("m", "v2", "llama.cpp", "b1"),
            fingerprint("m", "v1", "other", "b1"),
            fingerprint("m", "v1", "llama.cpp", "b2"),
            fingerprint("n", "v1", "llama.cpp", "b1"),
        ] {
            assert_ne!(base, key(&other, &req(Some(0.0))));
        }
        assert_eq!(FP, fingerprint("m", "v1", "llama.cpp", "b1"));
        for changed in [
            ChatRequest {
                top_p: Some(0.5),
                ..req(Some(0.0))
            },
            ChatRequest {
                seed: Some(4),
                ..req(Some(0.0))
            },
            ChatRequest {
                max_tokens: Some(5),
                ..req(Some(0.0))
            },
            ChatRequest {
                stop: vec!["x".into()],
                ..req(Some(0.0))
            },
            ChatRequest {
                messages: vec![Message::text(Role::User, "hi!")],
                ..req(Some(0.0))
            },
        ] {
            assert_ne!(base, key(FP, &changed));
        }
        assert_ne!(embed_key(FP, "a"), embed_key(FP, "b"));
        assert_ne!(embed_key(FP, "a"), embed_key("other", "a"));
    }

    #[test]
    fn cache_control_bypass() {
        assert!(bypass(Some("no-cache")));
        assert!(bypass(Some("max-age=0, No-Store")));
        assert!(!bypass(Some("max-age=60")));
        assert!(!bypass(None));
        assert_eq!(
            [Outcome::Hit, Outcome::Miss, Outcome::Bypass].map(Outcome::header),
            ["hit", "miss", "bypass"]
        );
    }

    #[test]
    fn replay_is_the_stream_the_answer_was() {
        let o = ChatOutput {
            reasoning: "think".into(),
            text: "é".repeat(100),
            ..out("")
        };
        let events = replay(&o);
        let mut back = ChatOutput::default();
        for e in &events {
            back.push(e);
        }
        assert_eq!(back.text, o.text);
        assert_eq!(back.reasoning, "think");
        assert_eq!(back.finish, Some(Finish::Stop));
        assert_eq!(back.usage, o.usage);
        assert!(events.len() > 3, "long text is streamed in pieces");
        assert_eq!(replay(&ChatOutput::default()).len(), 1);
    }

    #[test]
    fn answers_and_vectors_round_trip_and_damage_is_a_miss() {
        let o = out("hello");
        assert_eq!(decode(&encode(&o)).unwrap().text, "hello");
        assert!(decode(b"not json").is_none());
        assert!(decode(&encode(&ChatOutput { finish: None, ..o })).is_none());
        assert_eq!(decode_vector(&encode_vector(&[0.5, -1.0])).unwrap(), vec![0.5, -1.0]);
        assert!(decode_vector(&[1, 2, 3]).is_none());
    }

    #[test]
    fn lru_ttl_and_bounds() {
        let s = mem(10, 1000);
        let (a, b, d) = ([1; 32], [2; 32], [3; 32]);
        put(&s, a, b"AAAA", 0);
        put(&s, b, b"BBBB", 1);
        assert_eq!(s.get(Kind::Response, &a, 2).unwrap(), b"AAAA");
        put(&s, d, b"DDDD", 3);
        assert_eq!(s.stats(Kind::Response).entries, 2);
        assert!(s.get(Kind::Response, &b, 4).is_none(), "least recently used went");
        assert!(s.get(Kind::Response, &a, 2000).is_none(), "expired");
        put(&s, d, b"D2", 5);
        assert_eq!(s.get(Kind::Response, &d, 6).unwrap(), b"D2");
        assert_eq!(s.stats(Kind::Response).bytes, 2);
        put(&s, a, &[0; 20], 7);
        assert!(s.get(Kind::Response, &a, 8).is_none(), "larger than the cache");
        assert_eq!(s.stats(Kind::Response).hits, 1);
        assert_eq!(s.stats(Kind::Embedding), KindStats::default());
    }

    #[test]
    fn clear_requests_name_scopes() {
        let r: ClearRequest = serde_json::from_str(r#"{"kind":"app","appId":"debrief"}"#).unwrap();
        assert_eq!(r.scope(), Scope::App("debrief".into()));
        let m: ClearRequest = serde_json::from_str(r#"{"kind":"model","modelId":"o/r:q"}"#).unwrap();
        assert_eq!(m.scope(), Scope::Model("o/r:q".into()));
        assert_eq!(ClearRequest::All.scope(), Scope::All);
        assert_eq!(ClearRequest::Responses.scope(), Scope::Kind(Kind::Response));
        assert_eq!(ClearRequest::Embeddings.scope(), Scope::Kind(Kind::Embedding));
    }

    #[test]
    fn clearing_by_scope_and_reconfiguring() {
        let s = mem(1000, 1000);
        let put_as = |k: u8, kind, app: &str, model: &str| {
            s.put(
                kind,
                [k; 32],
                vec![0; 10],
                Meta {
                    model: model.into(),
                    app: app.into(),
                },
                0,
            );
        };
        put_as(1, Kind::Response, "debrief", "m1");
        put_as(2, Kind::Response, "pitcrew", "m1");
        put_as(3, Kind::Embedding, "debrief", "m2");
        put_as(4, Kind::Embedding, "pitcrew", "m2");
        assert_eq!(s.clear(&Scope::App("debrief".into())), 2);
        assert_eq!(s.clear(&Scope::Model("m2".into())), 1);
        assert_eq!(s.clear(&Scope::Kind(Kind::Embedding)), 0);
        assert_eq!(s.clear(&Scope::All), 1);
        put_as(5, Kind::Response, "x", "m");
        put_as(6, Kind::Response, "x", "m");
        s.configure(Kind::Response, lim(10, 5));
        assert_eq!(s.stats(Kind::Response).entries, 1);
        assert!(s.get(Kind::Response, &[5; 32], 100).is_none() && s.get(Kind::Response, &[6; 32], 100).is_none());
        assert_eq!(Kind::ALL.map(Kind::name), ["response", "embedding"]);
    }

    #[test]
    fn expiry_is_absolute_or_sliding_and_never_serves_expired() {
        let abs = Expiry {
            ttl: Some(100),
            sliding: false,
        };
        let slide = Expiry { sliding: true, ..abs };
        assert!(!abs.expired(0, 90, 99) && abs.expired(0, 90, 100));
        assert!(!slide.expired(0, 90, 150) && slide.expired(0, 90, 190));
        assert_eq!((abs.at(10, 50), slide.at(10, 50)), (Some(110), Some(150)));
        let never = Expiry { ttl: None, sliding: true };
        assert!(!never.expired(0, 0, u64::MAX) && never.at(0, 0).is_none());
        // Sliding in a store: using an entry keeps it; an idle one expires, even before a sweep ran.
        let sliding = Limits {
            max_bytes: 100,
            expiry: slide,
        };
        let s = MemoryStore::new(sliding, sliding);
        s.put(Kind::Embedding, [1; 32], vec![1], Meta::default(), 0);
        s.put(Kind::Embedding, [2; 32], vec![2], Meta::default(), 0);
        assert!(s.get(Kind::Embedding, &[1; 32], 90).is_some());
        assert!(s.get(Kind::Embedding, &[1; 32], 180).is_some(), "used at 90, so kept to 190");
        assert!(s.get(Kind::Embedding, &[2; 32], 180).is_none(), "idle since 0, expired though never swept");
    }

    #[test]
    fn a_sweep_removes_expired_entries_a_batch_at_a_time() {
        let s = mem(1000, 10);
        for i in 0..5u8 {
            put(&s, [i; 32], b"x", 0);
        }
        put(&s, [9; 32], b"fresh", 100);
        assert_eq!(s.stats(Kind::Response).oldest, Some(0));
        assert_eq!(s.sweep(Kind::Response, 105, 2), 2, "bounded per pass");
        assert_eq!(s.sweep(Kind::Response, 105, 100), 3);
        assert_eq!(s.sweep(Kind::Response, 105, 100), 0);
        assert_eq!(s.stats(Kind::Response).entries, 1);
        assert_eq!(s.stats(Kind::Response).oldest, Some(100));
        assert_eq!(s.sweep(Kind::Embedding, 105, 100), 0);
        assert_eq!(MemoryStore::new(lim(1, 1), lim(1, 1)).stats(Kind::Response).oldest, None);
    }

    #[test]
    fn a_poisoned_store_keeps_working() {
        let s = std::sync::Arc::new(mem(100, 100));
        let s2 = s.clone();
        let _ = std::thread::spawn(move || {
            let _g = s2.parts.lock().unwrap();
            panic!("poison");
        })
        .join();
        put(&s, [9; 32], b"x", 0);
        assert_eq!(s.get(Kind::Response, &[9; 32], 1).unwrap(), b"x");
    }
}
