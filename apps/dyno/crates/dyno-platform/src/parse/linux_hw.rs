//! Parsers for hardware sensors on Linux: hwmon and thermal values, `nvidia-smi` CSV and GPU files.

/// A temperature file's value (millidegrees Celsius) as degrees; `None` outside −50…200 °C (sensors report
/// garbage such as −274 or 255 when disconnected).
pub fn millidegrees(text: &str) -> Option<f32> {
    let c = text.trim().parse::<i64>().ok()? as f32 / 1000.0;
    (c > -50.0 && c < 200.0).then_some(c)
}

/// A fan file's value in rpm.
pub fn rpm(text: &str) -> Option<u32> {
    text.trim().parse().ok()
}

/// The sensor's label: "chip label", else "chip tempN" (or whatever `fallback` says).
pub fn sensor_label(chip: &str, label: Option<&str>, fallback: &str) -> String {
    let label = label.map(str::trim).filter(|l| !l.is_empty());
    match (chip.trim(), label) {
        ("", Some(l)) => l.to_string(),
        ("", None) => fallback.to_string(),
        (c, Some(l)) => format!("{c} {l}"),
        (c, None) => format!("{c} {fallback}"),
    }
}

/// The index `N` of a `tempN_input` / `fanN_input` file name for `kind` (`temp`, `fan`).
pub fn input_index(file: &str, kind: &str) -> Option<u32> {
    file.strip_prefix(kind)?.strip_suffix("_input")?.parse().ok()
}

/// What `nvidia-smi --query-gpu=name,utilization.gpu,memory.used,memory.total --format=csv,noheader,nounits` says
/// about one GPU.
#[derive(Debug, Clone, PartialEq)]
pub struct NvidiaGpu {
    /// Model name.
    pub name: String,
    /// Utilisation 0–100.
    pub utilization: f32,
    /// Memory in use, bytes.
    pub memory_used: u64,
    /// Memory size, bytes.
    pub memory_total: u64,
}

/// The arguments for `nvidia-smi` that [`nvidia_csv`] understands.
pub const NVIDIA_QUERY: [&str; 2] = [
    "--query-gpu=name,utilization.gpu,memory.used,memory.total",
    "--format=csv,noheader,nounits",
];

/// Parses the CSV (memory in MiB). Rows with `[N/A]` or missing numbers keep what is known (0).
pub fn nvidia_csv(text: &str) -> Vec<NvidiaGpu> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').map(str::trim).collect();
            if f.len() < 4 || f[0].is_empty() {
                return None;
            }
            let num = |s: &str| s.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0);
            Some(NvidiaGpu {
                name: f[0].to_string(),
                utilization: num(f[1]).unwrap_or(0.0).min(100.0) as f32,
                memory_used: (num(f[2]).unwrap_or(0.0) * 1_048_576.0) as u64,
                memory_total: (num(f[3]).unwrap_or(0.0) * 1_048_576.0) as u64,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temperatures_and_fans() {
        assert_eq!(millidegrees("45500\n"), Some(45.5));
        assert_eq!(millidegrees("-274000"), None);
        assert_eq!(millidegrees("255000"), None);
        assert_eq!(millidegrees("hot"), None);
        assert_eq!(rpm("1200\n"), Some(1200));
        assert_eq!(rpm("-1"), None);
        assert_eq!(input_index("temp3_input", "temp"), Some(3));
        assert_eq!(input_index("temp3_crit", "temp"), None);
        assert_eq!(input_index("fan1_input", "temp"), None);
        assert_eq!(sensor_label("coretemp", Some("Package id 0\n"), "temp1"), "coretemp Package id 0");
        assert_eq!(sensor_label("nvme", None, "temp1"), "nvme temp1");
        assert_eq!(sensor_label("", Some("x"), "temp1"), "x");
        assert_eq!(sensor_label("", None, "temp1"), "temp1");
        assert_eq!(sensor_label("amdgpu", Some("  "), "temp2"), "amdgpu temp2");
    }

    #[test]
    fn nvidia_rows() {
        let t = "NVIDIA GeForce RTX 4070, 37, 1024, 12282\nNVIDIA T400, [N/A], [N/A], 4096\n, 1, 1, 1\nshort, 1\n";
        let g = nvidia_csv(t);
        assert_eq!(g.len(), 2);
        assert_eq!(
            g[0],
            NvidiaGpu {
                name: "NVIDIA GeForce RTX 4070".into(),
                utilization: 37.0,
                memory_used: 1024 << 20,
                memory_total: 12282 << 20
            }
        );
        assert_eq!((g[1].utilization, g[1].memory_used, g[1].memory_total), (0.0, 0, 4096 << 20));
        assert!(nvidia_csv("").is_empty());
        assert_eq!(nvidia_csv("x, 250, 1, 2")[0].utilization, 100.0);
    }
}
