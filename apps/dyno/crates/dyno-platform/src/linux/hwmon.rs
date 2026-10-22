//! Temperatures and fans from `/sys/class/hwmon` (and `/sys/class/thermal` when hwmon has no temperatures).

use super::paths::read_trim;
use crate::parse::linux_hw::{input_index, millidegrees, rpm, sensor_label};
use dyno_core::metrics::{Fan, Temperature};
use std::path::Path;

/// Most chips, sensors and zones read; guards against a sysfs with thousands of entries.
const MAX_ENTRIES: usize = 64;

/// Everything the hardware monitors report.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Readings {
    /// Temperatures.
    pub temperatures: Vec<Temperature>,
    /// Fans.
    pub fans: Vec<Fan>,
}

fn sorted_entries(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .map(|d| d.flatten().take(MAX_ENTRIES * 4).map(|e| e.path()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// Reads hwmon chips under `sys`/class/hwmon, falling back to thermal zones for temperatures.
pub fn read(sys: &Path) -> Readings {
    let mut out = Readings::default();
    for chip in sorted_entries(&sys.join("class/hwmon")).into_iter().take(MAX_ENTRIES) {
        // On older kernels the sensors sit one level down, in `device/`.
        let dir = if chip.join("name").exists() || !chip.join("device").is_dir() {
            chip.clone()
        } else {
            chip.join("device")
        };
        let name = read_trim(&dir.join("name")).unwrap_or_default();
        for file in sorted_entries(&dir) {
            let Some(fname) = file.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
                continue;
            };
            if let Some(i) = input_index(&fname, "temp") {
                let Some(celsius) = read_trim(&file).as_deref().and_then(millidegrees) else {
                    continue;
                };
                let label = read_trim(&dir.join(format!("temp{i}_label")));
                let critical = read_trim(&dir.join(format!("temp{i}_crit")))
                    .as_deref()
                    .and_then(millidegrees)
                    .filter(|c| *c > 0.0);
                out.temperatures.push(Temperature {
                    label: sensor_label(&name, label.as_deref(), &format!("temp{i}")),
                    celsius,
                    critical,
                });
            } else if let Some(i) = input_index(&fname, "fan") {
                let Some(rpm) = read_trim(&file).as_deref().and_then(rpm) else {
                    continue;
                };
                let label = read_trim(&dir.join(format!("fan{i}_label")));
                out.fans.push(Fan {
                    label: sensor_label(&name, label.as_deref(), &format!("fan{i}")),
                    rpm,
                });
            }
        }
    }
    if out.temperatures.is_empty() {
        for zone in sorted_entries(&sys.join("class/thermal")).into_iter().take(MAX_ENTRIES) {
            if !zone
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("thermal_zone"))
            {
                continue;
            }
            let Some(celsius) = read_trim(&zone.join("temp")).as_deref().and_then(millidegrees) else {
                continue;
            };
            let kind = read_trim(&zone.join("type"));
            let critical = sorted_entries(&zone)
                .iter()
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("trip_point_") && n.ends_with("_type"))
                })
                .find(|p| read_trim(p).as_deref() == Some("critical"))
                .and_then(|p| {
                    let n = p.file_name()?.to_str()?.replace("_type", "_temp");
                    read_trim(&zone.join(n)).as_deref().and_then(millidegrees)
                });
            out.temperatures.push(Temperature {
                label: kind.unwrap_or_else(|| "thermal zone".to_string()),
                celsius,
                critical,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn reads_hwmon_chips_with_labels_critical_values_and_fans() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        w(r, "class/hwmon/hwmon0/name", "coretemp\n");
        w(r, "class/hwmon/hwmon0/temp1_input", "52000\n");
        w(r, "class/hwmon/hwmon0/temp1_label", "Package id 0\n");
        w(r, "class/hwmon/hwmon0/temp1_crit", "100000\n");
        w(r, "class/hwmon/hwmon0/temp2_input", "-274000\n");
        w(r, "class/hwmon/hwmon1/name", "nct6798\n");
        w(r, "class/hwmon/hwmon1/temp3_input", "38500\n");
        w(r, "class/hwmon/hwmon1/fan2_input", "1180\n");
        w(r, "class/hwmon/hwmon1/fan2_label", "CPU fan\n");
        w(r, "class/hwmon/hwmon1/fan4_input", "0\n");
        w(r, "class/hwmon/hwmon1/fan5_input", "junk\n");
        // Older layout: sensors under device/.
        w(r, "class/hwmon/hwmon2/device/name", "k10temp\n");
        w(r, "class/hwmon/hwmon2/device/temp1_input", "61000\n");
        // Thermal zones are ignored when hwmon has temperatures.
        w(r, "class/thermal/thermal_zone0/temp", "40000\n");
        let got = read(r);
        let temps: Vec<(&str, f32, Option<f32>)> = got.temperatures.iter().map(|t| (t.label.as_str(), t.celsius, t.critical)).collect();
        assert_eq!(
            temps,
            [
                ("coretemp Package id 0", 52.0, Some(100.0)),
                ("nct6798 temp3", 38.5, None),
                ("k10temp temp1", 61.0, None)
            ]
        );
        let fans: Vec<(&str, u32)> = got.fans.iter().map(|f| (f.label.as_str(), f.rpm)).collect();
        assert_eq!(fans, [("nct6798 CPU fan", 1180), ("nct6798 fan4", 0)]);
    }

    #[test]
    fn falls_back_to_thermal_zones_and_survives_nothing() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        assert_eq!(read(r), Readings::default());
        w(r, "class/thermal/thermal_zone0/temp", "47000\n");
        w(r, "class/thermal/thermal_zone0/type", "x86_pkg_temp\n");
        w(r, "class/thermal/thermal_zone0/trip_point_0_type", "passive\n");
        w(r, "class/thermal/thermal_zone0/trip_point_0_temp", "90000\n");
        w(r, "class/thermal/thermal_zone0/trip_point_1_type", "critical\n");
        w(r, "class/thermal/thermal_zone0/trip_point_1_temp", "105000\n");
        w(r, "class/thermal/cooling_device0/temp", "30000\n");
        w(r, "class/thermal/thermal_zone1/temp", "bad\n");
        let t = read(r).temperatures;
        assert_eq!(t.len(), 1);
        assert_eq!(
            (t[0].label.as_str(), t[0].celsius, t[0].critical),
            ("x86_pkg_temp", 47.0, Some(105.0))
        );
    }
}
