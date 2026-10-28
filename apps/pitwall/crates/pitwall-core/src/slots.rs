//! Parallel slots per model and what they cost (ADR 0015).
//!
//! llama-server serves up to `--parallel N` requests at once on one loaded model. Each running request owns a *slot*
//! with its own part of the KV cache. The real semantics of the engine's options are:
//!
//! * `--ctx-size C` is the **total** number of KV positions, shared by all slots; each slot gets `C / N` tokens.
//! * The KV cache is allocated up front, so the memory is `C x kv bytes per token` whatever the slots are doing.
//!
//! Pitwall therefore treats the configured context as the **context per slot** (what one request may use, so choosing
//! more slots never silently shrinks what a request can send) and launches with `--ctx-size = per slot x slots`.
//! The memory of a model is `weights + projector + overhead + slots x per-slot KV`, where the per-slot KV is
//! `context x layers x kv heads x head size x 2 (keys and values) x bytes per element` read from the GGUF header
//! (`GgufInfo::kv_bytes_per_token`), or, when the header lacks the layout, a documented estimate for a typical 7-9B
//! model (128 KiB per token; before a download, scaled from the file size).
//!
//! Everything here is pure: the solver gets the model's size and the machine's [`Room`] as values.

use crate::fit::{self, assess_tokens, gib, ModelSize, Room, Verdict};
use serde::{Deserialize, Deserializer, Serialize};

/// Smallest context a slot may be given.
pub const MIN_CONTEXT: u32 = 512;
/// Largest context a slot may be given (matches the settings range).
pub const MAX_CONTEXT: u32 = 1 << 20;
/// The solver answers in multiples of this many tokens.
pub const CONTEXT_STEP: u32 = 256;
/// The slot counts the fit table always shows.
pub const TABLE_SLOTS: [u32; 4] = [1, 2, 4, 8];

/// Why a slot count was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("parallel slots must be between 1 and 16, not {0}")]
pub struct SlotsError(pub u32);

/// How many requests one loaded model serves at once: 1 to 16.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Slots(u8);

impl Slots {
    /// The fewest.
    pub const MIN: u32 = 1;
    /// The most.
    pub const MAX: u32 = 16;
    /// The default when neither the model nor the settings say otherwise.
    pub const DEFAULT: Slots = Slots(2);
    /// One slot.
    pub const ONE: Slots = Slots(1);

    /// Parses a slot count (parse, don't validate).
    pub fn new(n: u32) -> Result<Slots, SlotsError> {
        if (Self::MIN..=Self::MAX).contains(&n) {
            Ok(Slots(n as u8))
        } else {
            Err(SlotsError(n))
        }
    }

    /// The count.
    pub fn get(self) -> u32 {
        u32::from(self.0)
    }

    /// The count as a size.
    pub fn count(self) -> usize {
        usize::from(self.0)
    }

    /// The `--ctx-size` for `per_slot` tokens in each slot (llama-server splits it evenly).
    pub fn launch_context(self, per_slot: u32) -> u32 {
        per_slot.saturating_mul(self.get())
    }
}

impl Default for Slots {
    fn default() -> Self {
        Slots::DEFAULT
    }
}

impl<'de> Deserialize<'de> for Slots {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Slots, D::Error> {
        let n = u32::deserialize(d)?;
        Slots::new(n).map_err(serde::de::Error::custom)
    }
}

/// Reads an optional slot preference from a settings file, treating anything unusable (out of range, wrong type) as
/// "not set" so one damaged value never costs the user their other settings.
pub fn lenient<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Slots>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Num(i64),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Raw::deserialize(d) {
        Ok(Raw::Num(n)) => u32::try_from(n).ok().and_then(|n| Slots::new(n).ok()),
        _ => None,
    })
}

/// The context each slot gets when a model is loaded: the explicit request, else the model's preference, else the
/// default, capped at the model's trained maximum (when known) and never below [`MIN_CONTEXT`]. The one rule shared by
/// the load path and what `/x/v1` reports as `contextPerSlot`.
pub fn context_per_slot(requested: Option<u32>, preferred: Option<u32>, default: u32, trained_max: Option<u64>) -> u32 {
    let cap = trained_max.unwrap_or(u64::MAX).min(u64::from(u32::MAX)) as u32;
    requested.or(preferred).unwrap_or(default).min(cap).max(MIN_CONTEXT)
}

/// KV cache bytes one slot holds at `per_slot_context` tokens.
pub fn kv_per_slot(size: &ModelSize, per_slot_context: u32) -> u64 {
    size.kv_per_token.saturating_mul(u64::from(per_slot_context))
}

/// KV positions in total for `slots` slots of `per_slot_context` tokens.
fn tokens(per_slot_context: u32, slots: Slots) -> u64 {
    u64::from(per_slot_context) * u64::from(slots.get())
}

/// The verdict for `slots` slots of `per_slot_context` tokens.
pub fn assess_slots(size: Option<&ModelSize>, per_slot_context: u32, slots: Slots, room: &Room) -> fit::Fit {
    assess_tokens(size, tokens(per_slot_context, slots), room)
}

fn usable(v: Verdict, comfortable: bool) -> bool {
    match v {
        Verdict::Fits => true,
        Verdict::Tight => !comfortable,
        Verdict::WontFit | Verdict::Unknown => false,
    }
}

/// The largest context per slot (in multiples of [`CONTEXT_STEP`], at most `cap`) that still fits with `slots` slots;
/// `comfortable` demands a plain "fits" rather than "tight". 0 when even [`MIN_CONTEXT`] does not fit.
pub fn max_context(size: &ModelSize, slots: Slots, room: &Room, cap: u32, comfortable: bool) -> u32 {
    let cap = cap.clamp(MIN_CONTEXT, MAX_CONTEXT);
    let ok = |c: u32| usable(assess_slots(Some(size), c, slots, room).verdict, comfortable);
    if !ok(MIN_CONTEXT) {
        return 0;
    }
    // The memory need only grows with the context, so the answer is a boundary: binary search the steps.
    // `cap` is at least MIN_CONTEXT, so `hi >= lo` always holds.
    let (mut lo, mut hi) = (MIN_CONTEXT.div_ceil(CONTEXT_STEP), cap / CONTEXT_STEP);
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if ok(mid * CONTEXT_STEP) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let best = lo * CONTEXT_STEP;
    if best == cap / CONTEXT_STEP * CONTEXT_STEP && cap > best && ok(cap) {
        cap
    } else {
        best
    }
}

/// The most slots (up to [`Slots::MAX`]) that fit at `per_slot_context` tokens each; 0 when not even one does.
pub fn max_slots(size: &ModelSize, per_slot_context: u32, room: &Room, comfortable: bool) -> u32 {
    (Slots::MIN..=Slots::MAX)
        .take_while(|n| Slots::new(*n).is_ok_and(|s| usable(assess_slots(Some(size), per_slot_context, s, room).verdict, comfortable)))
        .last()
        .unwrap_or(0)
}

/// One row of the fit table (`SlotChoice` in `api.types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotChoice {
    /// The number of slots.
    pub slots: u32,
    /// Verdict at the asked context per slot.
    pub verdict: Verdict,
    /// Memory the model needs with this many slots (the larger of system and video memory).
    pub need_bytes: u64,
    /// What this adds over a single slot.
    pub extra_bytes: u64,
    /// Largest context per slot that fits ("tight" counts).
    pub max_context: u32,
    /// Largest context per slot that fits with room to spare.
    pub comfortable_context: u32,
}

/// The answer to "how much context or concurrency fits" (`SlotFit` in `api.types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotFit {
    /// The context per slot asked about.
    pub context: u32,
    /// The slots chosen (preference or default).
    pub slots: u32,
    /// Verdict at the chosen slots and context.
    pub verdict: Verdict,
    /// Memory independent of the slots: weights, projector and compute buffers.
    pub base_bytes: u64,
    /// What each slot adds at this context.
    pub per_slot_bytes: u64,
    /// The KV size comes from the model's header (false: a typical-model estimate).
    pub kv_exact: bool,
    /// The most slots that fit at this context (0: not even one).
    pub max_slots: u32,
    /// The most slots that fit with room to spare.
    pub comfortable_slots: u32,
    /// Rows for 1, 2, 4, 8 slots and the chosen count.
    pub choices: Vec<SlotChoice>,
    /// The answer in a sentence or two.
    pub message: String,
}

fn tokens_text(n: u32) -> String {
    // 8192 -> "8,192"
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The fit table for a model: how much context fits at each slot count and how many slots fit at `context`.
/// `trained_max` caps the context per slot; `kv_exact` says whether the KV size came from the header.
pub fn solve(size: Option<&ModelSize>, context: u32, chosen: Slots, trained_max: Option<u32>, kv_exact: bool, room: &Room) -> SlotFit {
    let Some(size) = size.filter(|s| s.weights > 0) else {
        return SlotFit {
            context,
            slots: chosen.get(),
            verdict: Verdict::Unknown,
            base_bytes: 0,
            per_slot_bytes: 0,
            kv_exact: false,
            max_slots: 0,
            comfortable_slots: 0,
            choices: vec![],
            message: "The size of this model is not known, so Pitwall cannot say how many slots fit.".into(),
        };
    };
    let cap = trained_max.map_or(MAX_CONTEXT, |m| m.clamp(MIN_CONTEXT, MAX_CONTEXT));
    let base = size.total_tokens(0);
    let per_slot = kv_per_slot(size, context);
    let mut counts: Vec<u32> = TABLE_SLOTS.to_vec();
    counts.push(chosen.get());
    counts.sort_unstable();
    counts.dedup();
    let choices: Vec<SlotChoice> = counts
        .iter()
        .filter_map(|n| Slots::new(*n).ok())
        .map(|s| {
            let f = assess_slots(Some(size), context, s, room);
            SlotChoice {
                slots: s.get(),
                verdict: f.verdict,
                need_bytes: f.need_ram.max(f.need_vram),
                extra_bytes: per_slot.saturating_mul(u64::from(s.get() - 1)),
                max_context: max_context(size, s, room, cap, false),
                comfortable_context: max_context(size, s, room, cap, true),
            }
        })
        .collect();
    let max = max_slots(size, context, room, false);
    let comfortable = max_slots(size, context, room, true);
    let now = assess_slots(Some(size), context, chosen, room);
    let estimate = if kv_exact {
        ""
    } else {
        " (estimated: the model file does not describe its layout)"
    };
    let message = if max == 0 {
        format!(
            "Not even one slot of {} tokens fits on this machine. Choose a shorter context or a smaller quantisation.",
            tokens_text(context)
        )
    } else {
        let one = choices.iter().find(|c| c.slots == chosen.get()).map_or(0, |c| c.max_context);
        format!(
            "Each slot adds about {}{estimate}. At {} tokens per slot up to {} {} fit ({} with room to spare); with {} {} each can have up to {} tokens.",
            gib(per_slot),
            tokens_text(context),
            max,
            if max == 1 { "slot" } else { "slots" },
            comfortable,
            chosen.get(),
            if chosen.get() == 1 { "slot" } else { "slots" },
            tokens_text(one),
        )
    };
    SlotFit {
        context,
        slots: chosen.get(),
        verdict: now.verdict,
        base_bytes: base,
        per_slot_bytes: per_slot,
        kv_exact,
        max_slots: max,
        comfortable_slots: comfortable,
        choices,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GIB;

    #[test]
    fn context_per_slot_prefers_request_then_preference_then_default_within_limits() {
        assert_eq!(context_per_slot(Some(4096), Some(8192), 16384, None), 4096);
        assert_eq!(context_per_slot(None, Some(8192), 16384, None), 8192);
        assert_eq!(context_per_slot(None, None, 16384, None), 16384);
        // capped by the trained maximum, floored at the minimum
        assert_eq!(context_per_slot(None, None, 16384, Some(8192)), 8192);
        assert_eq!(context_per_slot(None, None, 16384, Some(100)), MIN_CONTEXT);
        assert_eq!(context_per_slot(Some(10), None, 16384, None), MIN_CONTEXT);
        assert_eq!(context_per_slot(None, None, u32::MAX, Some(u64::MAX)), u32::MAX);
    }

    fn room(avail_gib: u64) -> Room {
        Room {
            ram_total: 64 * GIB,
            ram_available: avail_gib * GIB,
            ram_releasable: 0,
            vram_free: None,
            unified: false,
        }
    }

    fn size() -> ModelSize {
        ModelSize {
            weights: 4 * GIB,
            projector: 0,
            kv_per_token: 128 * 1024,
        }
    }

    #[test]
    fn slots_are_bounded_and_serialise_as_numbers() {
        assert_eq!(Slots::new(0), Err(SlotsError(0)));
        assert_eq!(Slots::new(17), Err(SlotsError(17)));
        assert_eq!(Slots::new(16).unwrap().get(), 16);
        assert_eq!(Slots::new(3).unwrap().count(), 3);
        assert_eq!(Slots::default(), Slots::DEFAULT);
        assert!(SlotsError(99).to_string().contains("between 1 and 16"));
        assert_eq!(serde_json::to_value(Slots::ONE).unwrap(), 1);
        assert_eq!(serde_json::from_value::<Slots>(serde_json::json!(4)).unwrap().get(), 4);
        assert!(serde_json::from_value::<Slots>(serde_json::json!(40)).is_err());
        assert_eq!(Slots::new(4).unwrap().launch_context(8192), 32_768);
        assert_eq!(Slots::new(16).unwrap().launch_context(u32::MAX), u32::MAX, "saturates");
    }

    #[test]
    fn lenient_reads_damage_as_unset() {
        #[derive(Deserialize)]
        struct T {
            #[serde(default, deserialize_with = "lenient")]
            s: Option<Slots>,
        }
        for (toml, want) in [
            ("s = 4", Some(4)),
            ("s = 0", None),
            ("s = 99", None),
            ("s = -3", None),
            ("s = \"x\"", None),
            ("", None),
        ] {
            let t: T = toml::from_str(toml).unwrap();
            assert_eq!(t.s.map(Slots::get), want, "{toml}");
        }
    }

    #[test]
    fn memory_grows_by_one_kv_per_added_slot() {
        let r = room(48);
        let f = solve(Some(&size()), 8192, Slots::new(2).unwrap(), None, true, &r);
        assert_eq!(f.per_slot_bytes, 128 * 1024 * 8192);
        assert_eq!(f.base_bytes, 4 * GIB + fit::COMPUTE_OVERHEAD);
        let by = |n: u32| f.choices.iter().find(|c| c.slots == n).unwrap();
        assert_eq!(by(1).extra_bytes, 0);
        assert_eq!(by(4).extra_bytes, 3 * f.per_slot_bytes);
        assert_eq!(by(4).need_bytes - by(2).need_bytes, 2 * f.per_slot_bytes);
        assert_eq!(f.choices.iter().map(|c| c.slots).collect::<Vec<_>>(), vec![1, 2, 4, 8]);
        assert_eq!(f.verdict, Verdict::Fits);
        assert!(f.message.contains("Each slot adds about 1 GB"), "{}", f.message);
    }

    #[test]
    fn more_slots_leave_less_context_each() {
        let r = room(12);
        let c = |n: u32| max_context(&size(), Slots::new(n).unwrap(), &r, MAX_CONTEXT, false);
        assert!(
            c(1) > c(2) && c(2) > c(4) && c(4) > c(8) && c(8) > 0,
            "{} {} {} {}",
            c(1),
            c(2),
            c(4),
            c(8)
        );
        assert_eq!(c(1) % CONTEXT_STEP, 0);
        // The answer is the boundary: one step more no longer fits.
        let s = Slots::new(2).unwrap();
        assert!(assess_slots(Some(&size()), c(2), s, &r).verdict != Verdict::WontFit);
        assert_eq!(assess_slots(Some(&size()), c(2) + CONTEXT_STEP, s, &r).verdict, Verdict::WontFit);
        // Comfortable is never more than the maximum.
        assert!(max_context(&size(), s, &r, MAX_CONTEXT, true) <= c(2));
    }

    #[test]
    fn slots_at_a_context() {
        let r = room(12);
        let n = max_slots(&size(), 8192, &r, false);
        assert!((2..16).contains(&n), "{n}");
        assert!(max_slots(&size(), 8192, &r, true) <= n);
        assert_eq!(max_slots(&size(), 8192, &room(48), false), 16, "capped at the maximum");
        assert_eq!(max_slots(&size(), 8192, &room(2), false), 0);
    }

    #[test]
    fn edge_cases() {
        // No free memory.
        let f = solve(Some(&size()), 8192, Slots::ONE, None, true, &room(0));
        assert_eq!((f.max_slots, f.comfortable_slots, f.verdict), (0, 0, Verdict::WontFit));
        assert!(f.choices.iter().all(|c| c.max_context == 0 && c.comfortable_context == 0));
        assert!(f.message.contains("Not even one slot"), "{}", f.message);
        // Unknown size.
        let u = solve(None, 8192, Slots::ONE, None, false, &room(8));
        assert_eq!((u.verdict, u.max_slots, u.choices.len()), (Verdict::Unknown, 0, 0));
        let zero = ModelSize { weights: 0, ..size() };
        assert_eq!(solve(Some(&zero), 1, Slots::ONE, None, false, &room(8)).verdict, Verdict::Unknown);
        // Metadata missing: the estimate is flagged.
        let e = solve(Some(&size()), 4096, Slots::ONE, None, false, &room(48));
        assert!(!e.kv_exact && e.message.contains("estimated"), "{}", e.message);
        // A huge context asked for does not overflow or panic and does not fit.
        let h = solve(Some(&size()), u32::MAX, Slots::new(16).unwrap(), None, true, &room(48));
        assert_eq!((h.verdict, h.max_slots), (Verdict::WontFit, 0));
        // The trained context caps the answer; a tiny cap still answers the minimum.
        let c = max_context(&size(), Slots::ONE, &room(48), 4096, false);
        assert_eq!(c, 4096);
        assert_eq!(max_context(&size(), Slots::ONE, &room(48), 100, false), MIN_CONTEXT);
        assert_eq!(
            max_context(&size(), Slots::ONE, &room(48), 4100, false),
            4100,
            "a cap off the step is reached exactly"
        );
        let capped = solve(Some(&size()), 2048, Slots::ONE, Some(2048), true, &room(48));
        assert!(capped.choices.iter().all(|c| c.max_context == 2048));
        // A chosen count outside the table gets its own row.
        let f = solve(Some(&size()), 2048, Slots::new(3).unwrap(), None, true, &room(48));
        assert_eq!(f.choices.iter().map(|c| c.slots).collect::<Vec<_>>(), vec![1, 2, 3, 4, 8]);
        // A weight-only machine with a huge model: saturating arithmetic.
        let giant = ModelSize {
            weights: u64::MAX,
            projector: u64::MAX,
            kv_per_token: u64::MAX,
        };
        assert_eq!(solve(Some(&giant), 8192, Slots::ONE, None, true, &room(48)).max_slots, 0);
    }

    #[test]
    fn a_gpu_split_changes_the_answer() {
        let r = Room {
            vram_free: Some(8 * GIB),
            ..room(12)
        };
        let f = solve(Some(&size()), 8192, Slots::new(4).unwrap(), None, true, &r);
        assert!(f.max_slots >= 4, "{}", f.message);
        assert_eq!(tokens_text(1_234_567), "1,234,567");
        assert_eq!(tokens_text(512), "512");
    }
}
