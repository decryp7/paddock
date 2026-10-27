//! "Will this fit?": how much memory a model needs at a context length, and whether this machine has it (before a
//! download, from the file size; after, from the GGUF header). The numbers are estimates, so verdicts leave margin.

use crate::gguf::GgufInfo;
use crate::memory::Need;
use crate::quant::Quant;
use crate::{GIB, MIB};
use serde::{Deserialize, Serialize};

/// Compute buffers llama.cpp allocates on top of weights and KV cache.
pub const COMPUTE_OVERHEAD: u64 = 512 * MIB;
/// What the process needs beyond what it offloads to the GPU (runtime, tokenizer, host buffers).
pub const HOST_OVERHEAD: u64 = 384 * MIB;
/// GPUs with less free memory than this are not worth offloading to.
pub const MIN_USEFUL_VRAM: u64 = 2 * GIB;

/// What we know about a model's size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSize {
    /// Bytes of the weights (all parts of a split file).
    pub weights: u64,
    /// Bytes of its vision projector, if loaded with it.
    pub projector: u64,
    /// KV cache bytes per token of context.
    pub kv_per_token: u64,
}

impl ModelSize {
    /// From a GGUF header.
    pub fn from_header(info: &GgufInfo, projector: u64) -> ModelSize {
        ModelSize {
            weights: info.file_size,
            projector,
            kv_per_token: info.kv_bytes_per_token(),
        }
    }

    /// Before download: from the file size and quantisation alone (KV cache scaled from the parameter count, 128 KiB
    /// per token for an 8B model with grouped-query attention).
    pub fn from_file(weights: u64, quant: Option<&str>) -> ModelSize {
        let q = quant.and_then(Quant::parse).unwrap_or(Quant::Q4);
        let params_b = (weights as f64 / 1e9) / q.gb_per_b();
        let kv = (params_b / 8.0 * 128.0 * 1024.0).clamp(16.0 * 1024.0, 512.0 * 1024.0) as u64;
        ModelSize {
            weights,
            projector: 0,
            kv_per_token: kv,
        }
    }

    /// Total bytes at a context length.
    pub fn total(&self, context: u32) -> u64 {
        self.total_tokens(u64::from(context))
    }

    /// Total bytes with a KV cache of `tokens` positions (all slots together; saturating).
    pub fn total_tokens(&self, tokens: u64) -> u64 {
        self.weights
            .saturating_add(self.projector)
            .saturating_add(self.kv_per_token.saturating_mul(tokens))
            .saturating_add(COMPUTE_OVERHEAD)
    }
}

/// How much of the model goes to the GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Offload {
    /// Everything.
    All,
    /// Some layers.
    Partial,
    /// CPU only.
    None,
}

/// The memory available for a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Room {
    /// Physical memory.
    pub ram_total: u64,
    /// Available physical memory now.
    pub ram_available: u64,
    /// Memory hot models hold that idle release could free.
    pub ram_releasable: u64,
    /// Free memory of the best discrete GPU (`None`: no usable GPU or unified memory).
    pub vram_free: Option<u64>,
    /// The GPU shares system memory (Apple Silicon): VRAM is RAM.
    pub unified: bool,
}

/// The split of a model's need between GPU and system memory at a context length.
pub fn need(size: &ModelSize, context: u32, room: &Room) -> (Need, Offload) {
    need_tokens(size, u64::from(context), room)
}

/// [`need`] for a KV cache of `tokens` positions in total (the context times the parallel slots, see `slots`).
pub fn need_tokens(size: &ModelSize, tokens: u64, room: &Room) -> (Need, Offload) {
    let total = size.total_tokens(tokens);
    match room.vram_free.filter(|v| *v >= MIN_USEFUL_VRAM && !room.unified) {
        Some(v) => {
            let usable = v.saturating_sub(COMPUTE_OVERHEAD);
            if total <= usable + COMPUTE_OVERHEAD {
                (
                    Need {
                        ram: HOST_OVERHEAD,
                        vram: total,
                    },
                    Offload::All,
                )
            } else {
                let on_gpu = usable.min(total);
                (
                    Need {
                        ram: total.saturating_sub(on_gpu).saturating_add(HOST_OVERHEAD),
                        vram: total,
                    },
                    Offload::Partial,
                )
            }
        }
        None => (
            Need {
                ram: total.saturating_add(HOST_OVERHEAD),
                vram: 0,
            },
            Offload::None,
        ),
    }
}

/// The verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    /// Fits with room to spare.
    Fits,
    /// Fits only by releasing other models, or with little margin.
    Tight,
    /// Doesn't fit this machine.
    WontFit,
    /// Nothing known about its size.
    Unknown,
}

/// The fit check the UI shows (`Fit` in `api.types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fit {
    /// The verdict.
    pub verdict: Verdict,
    /// System memory it needs.
    pub need_ram: u64,
    /// Video memory for a full offload.
    pub need_vram: u64,
    /// System memory available (with what idle release frees).
    pub have_ram: u64,
    /// Free video memory.
    pub have_vram: u64,
    /// Where it runs.
    pub offload: Offload,
    /// One sentence for the user.
    pub message: String,
}

pub(crate) fn gib(b: u64) -> String {
    format!("{:.1} GB", b as f64 / GIB as f64)
}

/// Whether a model fits at a context length.
pub fn assess(size: Option<&ModelSize>, context: u32, room: &Room) -> Fit {
    assess_tokens(size, u64::from(context), room)
}

/// [`assess`] for a KV cache of `tokens` positions in total.
pub fn assess_tokens(size: Option<&ModelSize>, tokens: u64, room: &Room) -> Fit {
    let have_vram = room.vram_free.unwrap_or(0);
    let Some(size) = size.filter(|s| s.weights > 0) else {
        return Fit {
            verdict: Verdict::Unknown,
            need_ram: 0,
            need_vram: 0,
            have_ram: room.ram_available,
            have_vram,
            offload: Offload::None,
            message: "The size of this model is not known.".into(),
        };
    };
    let (n, offload) = need_tokens(size, tokens, room);
    let now = room.ram_available;
    let releasable = now.saturating_add(room.ram_releasable);
    let where_ = match offload {
        Offload::All => "entirely on the GPU",
        Offload::Partial => "partly on the GPU, partly on the CPU (slower)",
        Offload::None if room.unified => "in shared GPU memory",
        Offload::None => "on the CPU",
    };
    let (verdict, message) = if n.ram as f64 <= now as f64 * 0.8 {
        (
            Verdict::Fits,
            format!("Fits: needs about {} and runs {where_}.", gib(n.ram.max(n.vram))),
        )
    } else if n.ram <= releasable && n.ram as f64 <= room.ram_total as f64 * 0.85 {
        (
            Verdict::Tight,
            format!(
                "Tight: needs about {}; Pitwall releases idle models to make room. Runs {where_}.",
                gib(n.ram)
            ),
        )
    } else {
        (
            Verdict::WontFit,
            format!(
                "Won't fit: needs about {} but this machine can free about {}. Choose a smaller quantisation or a shorter context.",
                gib(n.ram),
                gib(releasable)
            ),
        )
    };
    Fit {
        verdict,
        need_ram: n.ram,
        need_vram: n.vram,
        have_ram: releasable,
        have_vram,
        offload,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(ram_avail_gib: u64, vram_gib: Option<u64>) -> Room {
        Room {
            ram_total: 32 * GIB,
            ram_available: ram_avail_gib * GIB,
            ram_releasable: 0,
            vram_free: vram_gib.map(|v| v * GIB),
            unified: false,
        }
    }

    #[test]
    fn sizes() {
        let s = ModelSize::from_file(5 * GIB, Some("Q4_K_M"));
        assert!(s.kv_per_token > 64 * 1024 && s.kv_per_token < 256 * 1024, "{}", s.kv_per_token);
        assert_eq!(ModelSize::from_file(1, None).kv_per_token, 16 * 1024, "clamped");
        assert_eq!(s.total(0), 5 * GIB + COMPUTE_OVERHEAD);
        let b = crate::testing::gguf::qwen_like();
        let info = crate::gguf::parse(std::io::Cursor::new(b.clone()), b.len() as u64).unwrap();
        let h = ModelSize::from_header(&info, 10);
        assert_eq!((h.projector, h.kv_per_token), (10, info.kv_bytes_per_token()));
    }

    #[test]
    fn offload_split() {
        let s = ModelSize {
            weights: 4 * GIB,
            projector: 0,
            kv_per_token: 0,
        };
        assert_eq!(
            need(&s, 0, &room(20, Some(12))),
            (
                Need {
                    ram: HOST_OVERHEAD,
                    vram: 4 * GIB + COMPUTE_OVERHEAD
                },
                Offload::All
            )
        );
        let (n, o) = need(&s, 0, &room(20, Some(3)));
        assert_eq!(o, Offload::Partial);
        assert_eq!(n.ram, 4 * GIB + COMPUTE_OVERHEAD - (3 * GIB - COMPUTE_OVERHEAD) + HOST_OVERHEAD);
        assert_eq!(need(&s, 0, &room(20, Some(1))).1, Offload::None, "too little VRAM to bother");
        assert_eq!(
            need(
                &s,
                0,
                &Room {
                    unified: true,
                    ..room(20, Some(12))
                }
            )
            .1,
            Offload::None
        );
    }

    #[test]
    fn verdicts() {
        let s = ModelSize {
            weights: 4 * GIB,
            projector: 0,
            kv_per_token: 128 * 1024,
        };
        let f = assess(Some(&s), 8192, &room(20, None));
        assert_eq!(f.verdict, Verdict::Fits);
        assert!(f.message.contains("on the CPU"), "{}", f.message);
        let f = assess(Some(&s), 8192, &room(20, Some(12)));
        assert!(f.message.contains("entirely on the GPU"));
        assert_eq!(f.offload, Offload::All);
        let f = assess(Some(&s), 8192, &room(20, Some(3)));
        assert!(f.message.contains("partly"));
        let f = assess(
            Some(&s),
            8192,
            &Room {
                unified: true,
                ..room(20, None)
            },
        );
        assert!(f.message.contains("shared GPU memory"));
        let tight = Room {
            ram_releasable: 8 * GIB,
            ..room(4, None)
        };
        let f = assess(Some(&s), 8192, &tight);
        assert_eq!(f.verdict, Verdict::Tight, "{}", f.message);
        assert_eq!(f.have_ram, 12 * GIB);
        let huge = ModelSize { weights: 60 * GIB, ..s };
        let f = assess(Some(&huge), 8192, &room(20, Some(12)));
        assert_eq!(f.verdict, Verdict::WontFit);
        assert!(f.message.contains("smaller quantisation"));
        let u = assess(None, 8192, &room(20, Some(12)));
        assert_eq!((u.verdict, u.have_vram), (Verdict::Unknown, 12 * GIB));
        assert_eq!(
            assess(Some(&ModelSize { weights: 0, ..s }), 1, &room(1, None)).verdict,
            Verdict::Unknown
        );
        assert_eq!(serde_json::to_value(Verdict::WontFit).unwrap(), "wontFit");
    }
}
