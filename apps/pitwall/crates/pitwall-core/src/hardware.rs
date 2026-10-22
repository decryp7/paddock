//! What the machine has: RAM, cores and GPUs, as values the server's probes gather (ported and adapted from IMT's
//! `imt_ai::hardware`). Parsing of tool output is here; running the tools is the server's job.

use crate::MIB;
use serde::{Deserialize, Serialize};

/// A GPU.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gpu {
    /// Its name.
    pub name: String,
    /// Dedicated video memory, when known.
    pub vram_total: Option<u64>,
    /// Video memory in use, when known.
    pub vram_used: Option<u64>,
    /// Utilisation 0–100, when the driver reports it.
    pub utilisation: Option<f32>,
    /// Memory shared with the CPU (Apple Silicon).
    pub unified: bool,
}

impl Gpu {
    /// Free video memory, when known.
    pub fn vram_free(&self) -> Option<u64> {
        Some(self.vram_total?.saturating_sub(self.vram_used.unwrap_or(0)))
    }
}

/// The machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Hardware {
    /// `windows`, `macos`, `linux`.
    pub os: String,
    /// `x86_64`, `aarch64`.
    pub arch: String,
    /// Logical cores.
    pub cpu_cores: usize,
    /// Physical memory.
    pub total_ram: u64,
    /// GPUs.
    pub gpus: Vec<Gpu>,
}

impl Hardware {
    /// The largest free video memory of one GPU, when any GPU reports it.
    pub fn vram_free(&self) -> Option<u64> {
        self.gpus.iter().filter(|g| !g.unified).filter_map(Gpu::vram_free).max()
    }

    /// Whether the GPU shares system memory (then video memory is not a separate budget).
    pub fn unified(&self) -> bool {
        self.gpus.iter().any(|g| g.unified)
    }
}

/// The `nvidia-smi` query Pitwall runs (`--format=csv,noheader,nounits`).
pub const NVIDIA_SMI_QUERY: &str = "--query-gpu=name,memory.total,memory.used,utilization.gpu";

/// Parses `nvidia-smi --query-gpu=name,memory.total,memory.used,utilization.gpu --format=csv,noheader,nounits`
/// (MiB and percent). Lines that don't parse are skipped; `[N/A]` fields become `None`.
pub fn parse_nvidia_smi(out: &str) -> Vec<Gpu> {
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').map(str::trim).collect();
            let [name, total, used, util] = f.as_slice() else { return None };
            if name.is_empty() {
                return None;
            }
            let mib = |s: &str| {
                s.parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .map(|v| (v * MIB as f64) as u64)
            };
            Some(Gpu {
                name: (*name).to_string(),
                vram_total: Some(mib(total)?),
                vram_used: mib(used),
                utilisation: util.parse::<f32>().ok().filter(|v| (0.0..=100.0).contains(v)),
                unified: false,
            })
        })
        .collect()
}

/// Apple Silicon: the GPU shares all of the RAM.
pub fn apple_silicon_gpu(os: &str, arch: &str, total_ram: u64) -> Option<Gpu> {
    (os == "macos" && arch == "aarch64").then(|| Gpu {
        name: "Apple Silicon GPU".into(),
        vram_total: Some(total_ram),
        vram_used: None,
        utilisation: None,
        unified: true,
    })
}

/// Whether a GPU name looks like an NVIDIA card.
pub fn is_nvidia(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["nvidia", "geforce", "quadro", "tesla", "rtx "].iter().any(|k| n.contains(k))
}

/// Whether a display adapter is a real GPU (virtual, remote and basic display adapters are not).
pub fn is_real_gpu(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ![
        "basic display",
        "basic render",
        "remote display",
        "virtio",
        "vmware",
        "virtualbox",
        "hyper-v",
        "llvmpipe",
        "qxl",
        "cirrus",
    ]
    .iter()
    .any(|k| n.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_smi_output() {
        let g =
            parse_nvidia_smi("NVIDIA GeForce RTX 4070, 12282, 1024, 37\nbroken line\n, 1, 1, 1\nX, [N/A], 0, 0\nY, 2048, [N/A], [N/A]\n");
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].name, "NVIDIA GeForce RTX 4070");
        assert_eq!(g[0].vram_total, Some(12282 * MIB));
        assert_eq!(g[0].vram_free(), Some((12282 - 1024) * MIB));
        assert_eq!(g[0].utilisation, Some(37.0));
        assert_eq!((g[1].vram_used, g[1].utilisation), (None, None));
        assert!(parse_nvidia_smi("").is_empty());
    }

    #[test]
    fn machines() {
        let mac = apple_silicon_gpu("macos", "aarch64", 16 * MIB).unwrap();
        assert!(mac.unified);
        assert!(apple_silicon_gpu("windows", "x86_64", 1).is_none());
        let hw = Hardware {
            gpus: vec![mac.clone()],
            ..Hardware::default()
        };
        assert!(hw.unified());
        assert_eq!(hw.vram_free(), None, "unified memory is not a separate budget");
        let hw = Hardware {
            gpus: parse_nvidia_smi("A, 100, 10, 1\nB, 200, 10, 1"),
            ..Hardware::default()
        };
        assert_eq!(hw.vram_free(), Some(190 * MIB));
        assert!(!hw.unified());
        let no = Gpu {
            name: "x".into(),
            vram_total: None,
            vram_used: None,
            utilisation: None,
            unified: false,
        };
        assert_eq!(no.vram_free(), None);
    }

    #[test]
    fn gpu_names() {
        assert!(is_nvidia("NVIDIA GeForce RTX 4070"));
        assert!(!is_nvidia("AMD Radeon"));
        assert!(is_real_gpu("AMD Radeon RX 7800 XT"));
        assert!(!is_real_gpu("Microsoft Basic Display Adapter"));
        assert!(!is_real_gpu("llvmpipe (LLVM 15)"));
    }
}
