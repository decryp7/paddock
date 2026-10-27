//! GPU use per process, where the OS tells: Windows GPU Engine counters (`typeperf`), NVIDIA's `nvidia-smi pmon`
//! (Linux and Windows) and the macOS accelerator's per-client GPU time (`ioreg`, read twice to get a rate). Other
//! combinations (Linux with AMD or Intel graphics, which need vendor interfaces) say so instead of showing zeros.

use super::{clean, clip};
use crate::process::Pid;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Most processes listed.
pub const MAX_PROCESSES: usize = 30;

/// One process's GPU use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuProcess {
    /// Process id.
    pub pid: Pid,
    /// Its name, when the tool gives it (the agent fills it in from the process table otherwise).
    pub name: String,
    /// Percent of the GPU's time it used (0-100).
    pub gpu_percent: f32,
    /// GPU memory in bytes, when the tool says.
    pub memory_bytes: Option<u64>,
}

/// The view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GpuReport {
    /// Processes using the GPU, busiest first.
    pub processes: Vec<GpuProcess>,
    /// Which tool or counter the numbers come from.
    pub source: Option<String>,
    /// Why there are none.
    pub unavailable: Option<String>,
}

fn finish(map: BTreeMap<Pid, GpuProcess>) -> Vec<GpuProcess> {
    let mut v: Vec<GpuProcess> = map
        .into_values()
        .filter(|p| p.gpu_percent > 0.0 || p.memory_bytes.is_some())
        .collect();
    v.sort_by(|a, b| b.gpu_percent.total_cmp(&a.gpu_percent).then(a.pid.cmp(&b.pid)));
    v.truncate(MAX_PROCESSES);
    v
}

/// `nvidia-smi pmon -c 1 -s um`: `# gpu pid type sm mem enc dec fb command`, `-` for no value. The shown percentage is the
/// larger of the SM and memory-bandwidth columns.
pub fn parse_nvidia_pmon(text: &str) -> Vec<GpuProcess> {
    let mut cols: Vec<String> = Vec::new();
    let mut map: BTreeMap<Pid, GpuProcess> = BTreeMap::new();
    for line in text.lines().map(clean) {
        if let Some(h) = line.strip_prefix('#') {
            let names: Vec<String> = h.split_whitespace().map(str::to_string).collect();
            if names.iter().any(|n| n == "pid") {
                cols = names;
            }
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        let at = |name: &str| cols.iter().position(|c| c == name).and_then(|i| f.get(i).copied());
        let Some(pid) = at("pid").and_then(|p| p.parse::<Pid>().ok()) else {
            continue;
        };
        let num = |name: &str| at(name).and_then(|v| v.parse::<f32>().ok());
        let percent = num("sm").unwrap_or(0.0).max(num("mem").unwrap_or(0.0)).clamp(0.0, 100.0);
        let name = at("command").map(|c| clip(c, 100)).unwrap_or_default();
        let memory_bytes = num("fb").map(|mib| (mib * 1024.0 * 1024.0) as u64);
        let e = map.entry(pid).or_insert(GpuProcess {
            pid,
            name,
            gpu_percent: 0.0,
            memory_bytes: None,
        });
        e.gpu_percent = e.gpu_percent.max(percent);
        e.memory_bytes = match (e.memory_bytes, memory_bytes) {
            (Some(a), Some(b)) => Some(a + b),
            (a, b) => a.or(b),
        };
    }
    finish(map)
}

/// `typeperf "\GPU Engine(*)\Utilization Percentage" -sc 1`: a CSV with one column per engine instance
/// `\\HOST\GPU Engine(pid_1234_luid_0x0_0x1_phys_0_eng_0_engtype_3D)\Utilization Percentage` and one row of values.
/// Task Manager shows, per process, the busiest engine type, so engines of one type are summed and the types compared.
pub fn parse_typeperf_gpu(text: &str) -> Vec<GpuProcess> {
    let mut lines = text.lines().map(clean).filter(|l| !l.is_empty());
    let Some(header) = lines.find(|l| l.starts_with("\"(PDH-CSV")) else {
        return Vec::new();
    };
    let Some(values) = lines.find(|l| l.starts_with('"') && !l.contains("PDH")) else {
        return Vec::new();
    };
    let head = csv_fields(header);
    let vals = csv_fields(values);
    let mut per: BTreeMap<(Pid, String), f32> = BTreeMap::new();
    for (h, v) in head.iter().zip(vals.iter()).skip(1) {
        let Some(inst) = h.split_once("GPU Engine(").and_then(|(_, r)| r.split_once(')')).map(|(i, _)| i) else {
            continue;
        };
        let Some(pid) = inst
            .strip_prefix("pid_")
            .and_then(|r| r.split('_').next())
            .and_then(|p| p.parse::<Pid>().ok())
        else {
            continue;
        };
        let kind = inst.rsplit_once("engtype_").map_or("", |(_, k)| k).to_string();
        if let Ok(x) = v.trim().parse::<f32>() {
            *per.entry((pid, kind)).or_default() += x;
        }
    }
    let mut map: BTreeMap<Pid, GpuProcess> = BTreeMap::new();
    for ((pid, _), x) in per {
        let e = map.entry(pid).or_insert(GpuProcess {
            pid,
            name: String::new(),
            gpu_percent: 0.0,
            memory_bytes: None,
        });
        e.gpu_percent = e.gpu_percent.max(x.clamp(0.0, 100.0));
    }
    finish(map)
}

fn csv_fields(line: &str) -> Vec<String> {
    line.trim().trim_matches('"').split("\",\"").map(str::to_string).collect()
}

/// The accelerator's cumulative GPU time per process in nanoseconds, from `ioreg -r -c AGXAccelerator -l -w0`: every
/// client has `"IOUserClientCreator" = "pid 433, WindowServer"` followed by an `AppUsage` list whose entries carry
/// `accumulatedGPUTime`.
pub fn parse_ioreg_gpu_time(text: &str) -> Vec<(Pid, String, u64)> {
    let mut map: BTreeMap<Pid, (String, u64)> = BTreeMap::new();
    let mut current: Option<Pid> = None;
    for line in text.lines() {
        if let Some(rest) = line.split("\"IOUserClientCreator\" = \"pid ").nth(1) {
            let (pid, name) = rest.trim_end_matches('"').split_once(", ").unwrap_or((rest, ""));
            current = pid.trim().parse().ok();
            if let Some(p) = current {
                map.entry(p).or_insert_with(|| (clip(name, 100), 0));
            }
        } else if line.contains("\"AppUsage\" = (") {
            let Some(p) = current else { continue };
            let total: u64 = line
                .split("\"accumulatedGPUTime\"=")
                .skip(1)
                .filter_map(|r| r.split(|c: char| !c.is_ascii_digit()).next()?.parse::<u64>().ok())
                .fold(0u64, u64::saturating_add);
            if let Some(e) = map.get_mut(&p) {
                e.1 = e.1.saturating_add(total);
            }
        }
    }
    map.into_iter().map(|(p, (n, t))| (p, n, t)).collect()
}

/// A rate from two `ioreg` reads `elapsed_ms` apart: percent of the elapsed wall time each process kept the GPU busy.
pub fn macos_rates(first: &[(Pid, String, u64)], second: &[(Pid, String, u64)], elapsed_ms: u64) -> Vec<GpuProcess> {
    if elapsed_ms == 0 {
        return Vec::new();
    }
    let before: BTreeMap<Pid, u64> = first.iter().map(|(p, _, t)| (*p, *t)).collect();
    let wall_ns = elapsed_ms as f64 * 1_000_000.0;
    let mut map = BTreeMap::new();
    for (pid, name, t) in second {
        let Some(prev) = before.get(pid) else { continue };
        let used = t.saturating_sub(*prev) as f64;
        map.insert(
            *pid,
            GpuProcess {
                pid: *pid,
                name: name.clone(),
                gpu_percent: ((used / wall_ns) * 100.0).clamp(0.0, 100.0) as f32,
                memory_bytes: None,
            },
        );
    }
    finish(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_pmon() {
        let t = "# gpu        pid  type    sm   mem   enc   dec    fb  command
# Idx          #   C/G     %     %     %     %    MB  name
    0       4242     G    35    12     -     -   310  firefox
    0       4242     C     5     1     -     -    90  firefox
    0        900     G     -     -     -     -    20  Xorg
";
        let p = parse_nvidia_pmon(t);
        assert_eq!(p[0].pid, 4242);
        assert_eq!(p[0].gpu_percent, 35.0);
        assert_eq!(p[0].memory_bytes, Some(400 * 1024 * 1024));
        assert_eq!(p[0].name, "firefox");
        assert_eq!(p[1].pid, 900);
        assert!(parse_nvidia_pmon("").is_empty());
    }

    #[test]
    fn typeperf_engines_take_the_busiest_type() {
        let t = "\"(PDH-CSV 4.0)\",\"\\\\PC\\GPU Engine(pid_100_luid_0x0_0x1_phys_0_eng_0_engtype_3D)\\Utilization Percentage\",\"\\\\PC\\GPU Engine(pid_100_luid_0x0_0x1_phys_0_eng_1_engtype_3D)\\Utilization Percentage\",\"\\\\PC\\GPU Engine(pid_100_luid_0x0_0x1_phys_0_eng_2_engtype_Copy)\\Utilization Percentage\",\"\\\\PC\\GPU Engine(pid_200_luid_0x0_0x1_phys_0_eng_0_engtype_VideoDecode)\\Utilization Percentage\"
\"10/23/2026 10:00:00.123\",\"20.5\",\"10.5\",\"50\",\"0\"
The command completed successfully.
";
        let p = parse_typeperf_gpu(t);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].pid, 100);
        assert_eq!(p[0].gpu_percent, 50.0);
        assert!(parse_typeperf_gpu("The data is not valid.").is_empty());
    }

    const IOREG: &str = "  |   +-o AGXAccelerator
  |     \"IOUserClientCreator\" = \"pid 438, runningboardd\"
  |     \"IOUserClientCreator\" = \"pid 433, WindowServer\"
  |     \"AppUsage\" = ({\"API\"=\"Metal\",\"lastSubmittedTime\"=1,\"accumulatedGPUTime\"=2000000000},{\"API\"=\"Metal\",\"accumulatedGPUTime\"=500000000})
  |     \"IOUserClientCreator\" = \"pid 672, com.apple.dock.e\"
  |     \"AppUsage\" = ()
";

    #[test]
    fn ioreg_gpu_time_per_client() {
        let t = parse_ioreg_gpu_time(IOREG);
        assert_eq!(t.len(), 3);
        let ws = t.iter().find(|(p, ..)| *p == 433).unwrap();
        assert_eq!(ws.1, "WindowServer");
        assert_eq!(ws.2, 2_500_000_000);
        assert_eq!(t.iter().find(|(p, ..)| *p == 672).unwrap().2, 0);
        assert!(parse_ioreg_gpu_time("").is_empty());
    }

    #[test]
    fn rates_come_from_two_reads() {
        let a = vec![(433, "WindowServer".to_string(), 1_000_000_000), (9, "gone".to_string(), 5)];
        let b = vec![(433, "WindowServer".to_string(), 1_250_000_000), (10, "new".to_string(), 9)];
        let r = macos_rates(&a, &b, 1000);
        assert_eq!(r.len(), 1);
        assert!((r[0].gpu_percent - 25.0).abs() < 0.01);
        assert!(macos_rates(&a, &b, 0).is_empty());
        // A counter that went backwards (a restarted client) is not negative.
        let c = vec![(433, "WindowServer".to_string(), 5)];
        assert!(macos_rates(&a, &c, 1000).is_empty());
    }
}
