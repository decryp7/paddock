//! `ioreg -r -d 1 -w0 -c IOAccelerator` output: the GPU's model and its PerformanceStatistics dictionary.

/// What the accelerator reports.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuStats {
    /// Marketing name, e.g. "Apple M4 Pro".
    pub name: String,
    /// "Device Utilization %", 0–100.
    pub utilization: f32,
    /// Memory the GPU's driver holds, bytes ("In use system memory" on unified memory, "vramUsedBytes" on discrete).
    pub memory_used: u64,
    /// Dedicated video memory, when the card has its own (0 for unified memory).
    pub memory_total: u64,
}

/// The number after `"key"=` inside the output (integers only).
fn number_after(text: &str, key: &str) -> Option<u64> {
    let pat = format!("\"{key}\"=");
    let start = text.find(&pat)? + pat.len();
    let digits: String = text[start..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The quoted string after `"model" = ` (ioreg also writes `<"…">` and `<hex>` forms).
fn model(text: &str) -> Option<String> {
    let start = text.find("\"model\" = ")? + "\"model\" = ".len();
    let rest = text[start..].trim_start_matches('<');
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    let name = rest[..end].trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Parses the accelerator list. The busiest accelerator wins when there are several (integrated + discrete).
/// `None` when no accelerator reports a utilisation.
pub fn parse_gpu(text: &str) -> Option<GpuStats> {
    let mut best: Option<GpuStats> = None;
    // One block per accelerator, each starting at its `+-o Name <class …>` header.
    let blocks: Vec<&str> = if text.contains("+-o ") {
        text.split("+-o ").skip(1).collect()
    } else {
        vec![text]
    };
    for block in blocks {
        let Some(stats_at) = block.find("\"PerformanceStatistics\"") else {
            continue;
        };
        let stats = &block[stats_at..];
        let stats = stats.split('}').next().unwrap_or(stats);
        let stats = stats.replace(" = ", "=");
        let Some(util) = number_after(&stats, "Device Utilization %") else {
            continue;
        };
        let used = number_after(&stats, "vramUsedBytes")
            .or_else(|| number_after(&stats, "In use system memory"))
            .unwrap_or(0);
        let total = number_after(&stats, "vramFreeBytes").map(|free| free + used).unwrap_or(0);
        let gpu = GpuStats {
            name: model(block).or_else(|| model(text)).unwrap_or_else(|| "GPU".into()),
            utilization: util.min(100) as f32,
            memory_used: used,
            memory_total: total,
        };
        if best.as_ref().is_none_or(|b| gpu.utilization > b.utilization) {
            best = Some(gpu);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    const APPLE: &str = r#"+-o AGXAcceleratorG16X  <class AGXAcceleratorG16X, id 0x1000005c3, registered>
    {
      "IOClass" = "AGXAcceleratorG16X"
      "model" = "Apple M4 Pro"
      "gpu-core-count" = 20
      "PerformanceStatistics" = {"In use system memory (driver)"=0,"Alloc system memory"=3835920384,"Tiler Utilization %"=16,"Device Utilization %"=25,"In use system memory"=1453162496}
    }"#;

    #[test]
    fn reads_an_apple_silicon_gpu() {
        let g = parse_gpu(APPLE).unwrap();
        assert_eq!(g.name, "Apple M4 Pro");
        assert_eq!(g.utilization, 25.0);
        assert_eq!(g.memory_used, 1_453_162_496);
        assert_eq!(g.memory_total, 0);
    }

    #[test]
    fn picks_the_busiest_of_two_and_reads_vram() {
        let text = format!(
            "{APPLE}\n+-o AMDRadeonX6000_AMDRadeonAccelerator  <class X>\n{{\n \"IOClass\" = \"AMDRadeonX6000_AMDRadeonAccelerator\"\n \"model\" = <\"AMD Radeon Pro 5500M\">\n \
\"PerformanceStatistics\" = {{\"vramFreeBytes\"=3000,\"vramUsedBytes\"=1000,\"Device Utilization %\"=70}}\n}}"
        );
        let g = parse_gpu(&text).unwrap();
        assert_eq!(g.name, "AMD Radeon Pro 5500M");
        assert_eq!((g.utilization, g.memory_used, g.memory_total), (70.0, 1000, 4000));
    }

    #[test]
    fn garbage_and_empty_are_none() {
        assert!(parse_gpu("").is_none());
        assert!(parse_gpu("\"IOClass\" = \"x\" \"PerformanceStatistics\" = {\"foo\"=1}").is_none());
        assert!(parse_gpu("\u{0}\u{1}\"IOClass\"\"PerformanceStatistics\"").is_none());
    }

    #[test]
    fn utilisation_is_capped() {
        let g = parse_gpu("\"IOClass\" = \"x\" \"PerformanceStatistics\" = {\"Device Utilization %\"=400}").unwrap();
        assert_eq!(g.utilization, 100.0);
        assert_eq!(g.name, "GPU");
    }
}
