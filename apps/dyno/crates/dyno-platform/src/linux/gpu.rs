//! The GPU: AMD through the `amdgpu` driver's sysfs files, NVIDIA through `nvidia-smi` (run with a timeout, only when
//! the program exists). Intel exposes no utilisation without root, so it is not read.

use super::paths::read_trim;
use super::tools::Tools;
use crate::parse::linux_hw::{nvidia_csv, NVIDIA_QUERY};
use dyno_core::metrics::{GpuSample, Reading};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// How long `nvidia-smi` may take (it can stall for seconds while the driver wakes the GPU).
const NVIDIA_TIMEOUT: Duration = Duration::from_secs(3);
/// The reason shown when no supported GPU exists.
pub const NO_GPU: &str = "No supported GPU found";

fn amd_cards(sys: &Path) -> Vec<GpuSample> {
    let Ok(dir) = std::fs::read_dir(sys.join("class/drm")) else {
        return Vec::new();
    };
    let mut names: Vec<String> = dir
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| {
            n.strip_prefix("card")
                .is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_digit()))
        })
        .collect();
    names.sort();
    names
        .into_iter()
        .take(16)
        .filter_map(|card| {
            let dev = sys.join("class/drm").join(&card).join("device");
            let busy: f32 = read_trim(&dev.join("gpu_busy_percent"))?.parse().ok()?;
            let num = |f: &str| read_trim(&dev.join(f)).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            Some(GpuSample {
                name: read_trim(&dev.join("product_name")).unwrap_or_else(|| format!("AMD GPU ({card})")),
                utilization: busy.clamp(0.0, 100.0),
                memory_used: num("mem_info_vram_used"),
                memory_total: num("mem_info_vram_total"),
            })
        })
        .collect()
}

/// Reads the GPU, asking `nvidia-smi` only every `nvidia_every` calls.
pub struct GpuReader {
    tools: Arc<dyn Tools>,
    nvidia_every: u64,
    calls: u64,
    nvidia: Result<Vec<GpuSample>, String>,
}

impl GpuReader {
    /// A reader; `nvidia_every` is clamped to at least 1.
    pub fn new(tools: Arc<dyn Tools>, nvidia_every: u64) -> Self {
        GpuReader {
            tools,
            nvidia_every: nvidia_every.max(1),
            calls: 0,
            nvidia: Ok(Vec::new()),
        }
    }

    fn refresh_nvidia(&mut self) {
        if !self.tools.exists("nvidia-smi") {
            self.nvidia = Ok(Vec::new());
            return;
        }
        self.nvidia = match self.tools.run("nvidia-smi", &NVIDIA_QUERY, NVIDIA_TIMEOUT) {
            Ok(out) if out.ok => Ok(nvidia_csv(&out.stdout)
                .into_iter()
                .map(|g| GpuSample {
                    name: g.name,
                    utilization: g.utilization,
                    memory_used: g.memory_used,
                    memory_total: g.memory_total,
                })
                .collect()),
            Ok(out) => Err(format!("nvidia-smi could not read the GPU: {}", out.first_error_line())),
            Err(e) => Err(e.to_string()),
        };
    }

    /// The busiest GPU found, or why there is none.
    pub fn read(&mut self, sys: &Path) -> Reading<GpuSample> {
        if self.calls.is_multiple_of(self.nvidia_every) {
            self.refresh_nvidia();
        }
        self.calls = self.calls.wrapping_add(1);
        let mut gpus = amd_cards(sys);
        let nvidia_error = match &self.nvidia {
            Ok(g) => {
                gpus.extend(g.iter().cloned());
                None
            }
            Err(e) => Some(e.clone()),
        };
        match gpus.into_iter().max_by(|a, b| a.utilization.total_cmp(&b.utilization)) {
            Some(g) => Reading::Available(g),
            None => Reading::unavailable(nvidia_error.unwrap_or_else(|| NO_GPU.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tools::fake::FakeTools;
    use super::super::tools::ToolOutput;
    use super::*;
    use dyno_core::ports::PlatformError;

    fn w(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn no_gpu_says_so() {
        let d = tempfile::tempdir().unwrap();
        let mut r = GpuReader::new(Arc::new(FakeTools::default()), 5);
        assert_eq!(r.read(d.path()), Reading::unavailable(NO_GPU));
    }

    #[test]
    fn reads_amd_sysfs() {
        let d = tempfile::tempdir().unwrap();
        w(d.path(), "class/drm/card1/device/gpu_busy_percent", "63\n");
        w(d.path(), "class/drm/card1/device/mem_info_vram_used", "1073741824\n");
        w(d.path(), "class/drm/card1/device/mem_info_vram_total", "8589934592\n");
        w(d.path(), "class/drm/card1/device/product_name", "Radeon RX 7600\n");
        w(d.path(), "class/drm/card0/device/gpu_busy_percent", "5\n");
        w(d.path(), "class/drm/card1-DP-1/device/gpu_busy_percent", "99\n");
        w(d.path(), "class/drm/card2/device/vendor", "0x8086\n");
        let g = GpuReader::new(Arc::new(FakeTools::default()), 5).read(d.path());
        let g = g.value().unwrap();
        assert_eq!(
            (g.name.as_str(), g.utilization, g.memory_used, g.memory_total),
            ("Radeon RX 7600", 63.0, 1 << 30, 8 << 30)
        );
    }

    #[test]
    fn nvidia_smi_runs_only_every_few_ticks_and_only_when_present() {
        let d = tempfile::tempdir().unwrap();
        let tools = Arc::new(FakeTools::with("nvidia-smi", "NVIDIA RTX A2000, 12, 800, 6138\n"));
        let mut r = GpuReader::new(tools.clone(), 3);
        for _ in 0..6 {
            let g = r.read(d.path());
            assert_eq!(g.value().map(|g| g.name.as_str()), Some("NVIDIA RTX A2000"));
        }
        let calls = tools.calls();
        assert_eq!(calls.len(), 2);
        assert!(calls[0].starts_with("nvidia-smi --query-gpu=name,utilization.gpu"));
        let absent = Arc::new(FakeTools::default());
        GpuReader::new(absent.clone(), 1).read(d.path());
        assert!(absent.calls().is_empty());
    }

    #[test]
    fn nvidia_failures_become_reasons() {
        let d = tempfile::tempdir().unwrap();
        let tools = Arc::new(FakeTools::default());
        tools.answer(
            "nvidia-smi",
            Err(PlatformError::Timeout("nvidia-smi did not finish within 3 s.".into())),
        );
        let mut r = GpuReader::new(tools.clone(), 1);
        assert_eq!(r.read(d.path()), Reading::unavailable("nvidia-smi did not finish within 3 s."));
        tools.answer(
            "nvidia-smi",
            Ok(ToolOutput {
                ok: false,
                stdout: String::new(),
                stderr: "NVIDIA-SMI has failed\n".into(),
            }),
        );
        assert!(matches!(r.read(d.path()), Reading::Unavailable(m) if m.contains("NVIDIA-SMI has failed")));
        // The failure of one vendor does not hide the other.
        w(d.path(), "class/drm/card0/device/gpu_busy_percent", "1\n");
        assert!(r.read(d.path()).value().is_some());
    }
}
