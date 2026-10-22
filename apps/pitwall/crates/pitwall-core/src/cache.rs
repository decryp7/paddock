//! The response cache (ADR 0005): identical deterministic requests (temperature 0 or a fixed seed, no tools to run)
//! to the same model are answered from memory. Bounded by entries and bytes, with a time to live, least recently
//! used evicted first. Off for apps that ask (`Cache-Control: no-store`) and never persisted to disk.
//! Prompt-prefix caching is llama-server's (`cache_prompt`, `--cache-reuse`).

use crate::chat::{ChatOutput, ChatRequest};
use crate::time::Millis;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// A cache key: SHA-256 of the model and the canonical request.
pub type Key = [u8; 32];

/// Whether a request may be cached: deterministic and not streamed through tools.
pub fn cacheable(req: &ChatRequest) -> bool {
    (req.temperature == Some(0.0) || req.seed.is_some()) && req.tools.is_empty()
}

/// The key of a request for a model (`stream` doesn't change the answer, so it is not part of the key).
pub fn key(model: &str, req: &ChatRequest) -> Key {
    let canonical = ChatRequest { stream: false, priority: None, ..req.clone() };
    let mut h = Sha256::new();
    h.update(model.as_bytes());
    h.update([0]);
    h.update(serde_json::to_vec(&canonical).unwrap_or_default());
    h.finalize().into()
}

struct Entry {
    out: ChatOutput,
    bytes: usize,
    stored: Millis,
    used: Millis,
}

/// The cache.
pub struct Cache {
    max_entries: usize,
    max_bytes: usize,
    ttl: Millis,
    bytes: usize,
    entries: HashMap<Key, Entry>,
}

impl Cache {
    /// An empty cache.
    pub fn new(max_entries: usize, max_bytes: usize, ttl: Millis) -> Cache {
        Cache { max_entries, max_bytes, ttl, bytes: 0, entries: HashMap::new() }
    }

    /// A cached answer.
    pub fn get(&mut self, k: &Key, now: Millis) -> Option<ChatOutput> {
        let expired = self.entries.get(k).is_some_and(|e| now.saturating_sub(e.stored) >= self.ttl);
        if expired {
            self.remove(k);
            return None;
        }
        let e = self.entries.get_mut(k)?;
        e.used = now;
        Some(e.out.clone())
    }

    fn remove(&mut self, k: &Key) {
        if let Some(e) = self.entries.remove(k) {
            self.bytes -= e.bytes;
        }
    }

    /// Stores an answer, evicting least recently used entries as needed. Answers larger than the whole cache are not
    /// stored.
    pub fn put(&mut self, k: Key, out: ChatOutput, now: Millis) {
        let bytes = out.text.len() + out.reasoning.len() + out.tool_calls.iter().map(|c| c.arguments.len() + c.name.len()).sum::<usize>() + 64;
        if bytes > self.max_bytes || self.max_entries == 0 {
            return;
        }
        self.remove(&k);
        while self.entries.len() >= self.max_entries || self.bytes + bytes > self.max_bytes {
            let Some(oldest) = self.entries.iter().min_by_key(|(_, e)| e.used).map(|(k, _)| *k) else { break };
            self.remove(&oldest);
        }
        self.bytes += bytes;
        self.entries.insert(k, Entry { out, bytes, stored: now, used: now });
    }

    /// Entries held.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drops everything (memory release, model unload).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{Message, Role, Tool};

    fn req(t: Option<f32>) -> ChatRequest {
        ChatRequest { messages: vec![Message::text(Role::User, "hi")], temperature: t, ..Default::default() }
    }

    fn out(text: &str) -> ChatOutput {
        ChatOutput { text: text.into(), ..Default::default() }
    }

    #[test]
    fn what_is_cacheable() {
        assert!(cacheable(&req(Some(0.0))));
        assert!(!cacheable(&req(Some(0.7))));
        assert!(cacheable(&ChatRequest { seed: Some(1), ..req(None) }));
        let tools = ChatRequest { tools: vec![Tool { name: "t".into(), description: String::new(), parameters: serde_json::Value::Null }], ..req(Some(0.0)) };
        assert!(!cacheable(&tools));
        assert_eq!(key("m", &req(Some(0.0))), key("m", &ChatRequest { stream: true, ..req(Some(0.0)) }));
        assert_ne!(key("m", &req(Some(0.0))), key("n", &req(Some(0.0))));
    }

    #[test]
    fn lru_ttl_and_bounds() {
        let mut c = Cache::new(2, 10_000, 1000);
        assert!(c.is_empty());
        let (a, b, d) = (key("a", &req(None)), key("b", &req(None)), key("d", &req(None)));
        c.put(a, out("A"), 0);
        c.put(b, out("B"), 1);
        assert_eq!(c.get(&a, 2).unwrap().text, "A");
        c.put(d, out("D"), 3);
        assert_eq!(c.len(), 2);
        assert!(c.get(&b, 4).is_none(), "least recently used went");
        assert!(c.get(&a, 2000).is_none(), "expired");
        c.put(d, out("D2"), 5);
        assert_eq!(c.get(&d, 6).unwrap().text, "D2");
        c.put(a, out(&"x".repeat(20_000)), 7);
        assert!(c.get(&a, 8).is_none(), "too large for the cache");
        let mut small = Cache::new(10, 200, 1000);
        small.put(a, out(&"x".repeat(100)), 0);
        small.put(b, out(&"y".repeat(100)), 1);
        assert_eq!(small.len(), 1, "byte bound");
        small.clear();
        assert!(small.is_empty());
        let mut none = Cache::new(0, 100, 1);
        none.put(a, out("x"), 0);
        assert!(none.is_empty());
    }
}
