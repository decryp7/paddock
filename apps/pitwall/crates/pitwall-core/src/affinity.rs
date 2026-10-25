//! Prompt-prefix reuse (ADR 0019). llama-server keeps each slot's KV cache, so a follow-up request of the same
//! conversation is fast only when it lands on the slot that served the earlier turns. [`prefix_key`] identifies a
//! conversation by its app and its leading messages (the system prompts and the first user message); [`SlotTable`]
//! remembers which slot last served which key and hands out free slots, preferring the one that already holds the
//! prefix. Everything is pure; the server asks for `id_slot` only when a slot is claimed and otherwise leaves the
//! choice to llama-server.

use crate::chat::{ChatRequest, Part, Role};
use crate::{GIB, MIB};
use sha2::{Digest, Sha256};

/// Identifies a conversation's prefix.
pub type PrefixKey = [u8; 32];

/// The key of a request: the app plus the system messages and the first message of anyone else.
pub fn prefix_key(app: &str, req: &ChatRequest) -> PrefixKey {
    let mut h = Sha256::new();
    h.update(app.as_bytes());
    h.update([0]);
    let mut seen_other = false;
    for m in &req.messages {
        if m.role != Role::System {
            if seen_other {
                break;
            }
            seen_other = true;
        }
        h.update([m.role as u8]);
        for p in &m.parts {
            match p {
                Part::Text(t) => h.update(t.as_bytes()),
                Part::Image(_) => h.update(b"[image]"),
            }
            h.update([0]);
        }
    }
    h.finalize().into()
}

/// A slot handed out for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Claim {
    /// The slot (`id_slot`).
    pub slot: u32,
    /// The slot last served the same prefix, so its cache should hit.
    pub warm: bool,
}

#[derive(Debug, Clone, Default)]
struct Slot {
    busy: bool,
    key: Option<PrefixKey>,
    used: u64,
}

/// Which slot holds which conversation.
#[derive(Debug, Clone)]
pub struct SlotTable {
    slots: Vec<Slot>,
    tick: u64,
}

impl SlotTable {
    /// A table for `n` slots, all free and empty.
    pub fn new(n: usize) -> SlotTable {
        SlotTable {
            slots: vec![Slot::default(); n],
            tick: 0,
        }
    }

    /// Takes a slot for `key`: a free slot that last served it; otherwise the free slot unused the longest (an empty one
    /// first), so other conversations' prefixes are kept as long as possible. `None` when every slot is busy.
    pub fn claim(&mut self, key: PrefixKey) -> Option<Claim> {
        let warm = self.slots.iter().position(|s| !s.busy && s.key == Some(key));
        let (i, warm) = match warm {
            Some(i) => (i, true),
            None => (
                self.slots
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| !s.busy)
                    .min_by_key(|(_, s)| (s.key.is_some(), s.used))
                    .map(|(i, _)| i)?,
                false,
            ),
        };
        self.tick += 1;
        self.slots[i] = Slot {
            busy: true,
            key: Some(key),
            used: self.tick,
        };
        Some(Claim { slot: i as u32, warm })
    }

    /// Frees a slot; it keeps its prefix.
    pub fn release(&mut self, slot: u32) {
        if let Some(s) = self.slots.get_mut(slot as usize) {
            s.busy = false;
        }
    }

    /// Forgets every prefix (the context was cleared); busy slots stay busy.
    pub fn forget(&mut self) {
        for s in &mut self.slots {
            s.key = None;
        }
    }
}

/// The host-memory prompt cache (`--cache-ram`) per model: a sixteenth of this machine's memory, between 256 MiB and
/// 4 GiB, unless the user chose a size. 0 switches it off.
pub fn prompt_cache_mib(total_ram: u64, chosen: Option<u32>) -> u32 {
    chosen.unwrap_or_else(|| (total_ram / 16).clamp(256 * MIB, 4 * GIB).div_ceil(MIB) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::Message;

    fn req(msgs: &[(Role, &str)]) -> ChatRequest {
        ChatRequest {
            messages: msgs.iter().map(|(r, t)| Message::text(*r, *t)).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn a_conversation_keeps_its_key_as_it_grows() {
        let a = prefix_key("opencode", &req(&[(Role::System, "rules"), (Role::User, "fix it")]));
        let later = req(&[
            (Role::System, "rules"),
            (Role::User, "fix it"),
            (Role::Assistant, "done"),
            (Role::User, "and now?"),
        ]);
        assert_eq!(a, prefix_key("opencode", &later));
        assert_ne!(a, prefix_key("other", &later));
        assert_ne!(
            a,
            prefix_key("opencode", &req(&[(Role::System, "other rules"), (Role::User, "fix it")]))
        );
        assert_ne!(
            a,
            prefix_key("opencode", &req(&[(Role::System, "rules"), (Role::User, "different")]))
        );
        let image = ChatRequest {
            messages: vec![Message {
                parts: vec![Part::Image(crate::chat::Image {
                    mime: crate::chat::ImageMime::Png,
                    data: "AAAA".into(),
                })],
                ..Message::text(Role::User, "")
            }],
            ..Default::default()
        };
        assert_ne!(a, prefix_key("opencode", &image));
        let _ = prefix_key("x", &ChatRequest::default());
    }

    #[test]
    fn the_slot_with_the_prefix_is_preferred_and_others_fall_back() {
        let mut t = SlotTable::new(3);
        let (a, b, c, d) = ([1; 32], [2; 32], [3; 32], [4; 32]);
        let ca = t.claim(a).unwrap();
        let cb = t.claim(b).unwrap();
        assert_eq!((ca, cb), (Claim { slot: 0, warm: false }, Claim { slot: 1, warm: false }));
        t.release(0);
        t.release(1);
        // a's follow-up goes back to slot 0, b's to slot 1, even though slot 2 is the emptiest.
        assert_eq!(t.claim(a).unwrap(), Claim { slot: 0, warm: true });
        assert_eq!(t.claim(b).unwrap(), Claim { slot: 1, warm: true });
        // a new conversation takes the empty slot; with none empty, the one idle longest.
        assert_eq!(t.claim(c).unwrap(), Claim { slot: 2, warm: false });
        assert!(t.claim(d).is_none(), "all busy: the engine chooses");
        t.release(1);
        t.release(0);
        assert_eq!(t.claim(d).unwrap().slot, 0, "slot 0 has been idle the longest");
    }

    #[test]
    fn a_busy_slot_is_not_shared_and_forgetting_cools_everything() {
        let mut t = SlotTable::new(2);
        let k = [7; 32];
        assert_eq!(t.claim(k).unwrap().slot, 0);
        assert_eq!(t.claim(k).unwrap(), Claim { slot: 1, warm: false });
        t.release(0);
        t.release(1);
        t.release(99);
        assert!(t.claim(k).unwrap().warm);
        t.release(0);
        t.forget();
        assert!(!t.claim(k).unwrap().warm);
        assert!(SlotTable::new(0).claim(k).is_none());
    }

    #[test]
    fn the_prompt_cache_scales_with_memory() {
        assert_eq!(prompt_cache_mib(2 * GIB, None), 256);
        assert_eq!(prompt_cache_mib(16 * GIB, None), 1024);
        assert_eq!(prompt_cache_mib(1024 * GIB, None), 4096);
        assert_eq!(prompt_cache_mib(16 * GIB, Some(0)), 0);
        assert_eq!(prompt_cache_mib(16 * GIB, Some(300)), 300);
    }
}
