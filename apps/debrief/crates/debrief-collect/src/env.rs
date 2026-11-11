//! Environment snapshot (`system/environment.json`).

use serde_json::{json, Value};
use sysinfo::{Disks, System};

/// The host name, if the OS reports one.
pub fn host_name() -> Option<String> {
    System::host_name().ok()
}

/// Allow-listed variables (case-insensitive names), sorted by name.
pub fn filter_vars(allow: &[String], vars: impl Iterator<Item = (String, String)>) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = vars.filter(|(k, _)| allow.iter().any(|a| a.eq_ignore_ascii_case(k))).collect();
    v.sort();
    v
}

/// OS, host, CPU, memory, disks and uptime, plus the filtered variables.
pub fn snapshot(allow: &[String], vars: impl Iterator<Item = (String, String)>, debrief_version: &str) -> Value {
    let sys = System::new_with_specifics(
        sysinfo::RefreshKind::nothing()
            .with_memory(sysinfo::MemoryRefreshKind::everything())
            .with_cpu(sysinfo::CpuRefreshKind::everything()),
    )
    .inspect_err(|why| log::warn!("CPU and memory figures unavailable (sysinfo): {why}; the snapshot says null"))
    .ok();
    let cpus = sys.as_ref().map_or(&[][..], |s| s.cpus());
    let disks: Vec<Value> = Disks::new_with_refreshed_list()
        .inspect_err(|why| log::warn!("the volume list is unavailable (sysinfo): {why}; the snapshot lists none"))
        .ok()
        .iter()
        .flat_map(|disks| disks.iter())
        .map(|d| {
            json!({
                "name": d.name().to_string_lossy(),
                "mount": d.mount_point().to_string_lossy(),
                "fileSystem": d.file_system().to_string_lossy(),
                "totalBytes": d.total_space(),
                "availableBytes": d.available_space(),
            })
        })
        .collect();
    let env: serde_json::Map<String, Value> = filter_vars(allow, vars).into_iter().map(|(k, v)| (k, Value::String(v))).collect();
    json!({
        "os": { "name": System::name().ok(), "version": System::os_version().ok(), "longVersion": System::long_os_version().ok(), "kernel": System::kernel_version().ok(), "arch": System::cpu_arch() },
        "host": System::host_name().ok(),
        "cpu": { "brand": cpus.first().map(|c| c.brand().to_string()), "logicalCores": cpus.len() },
        "memory": { "totalBytes": sys.as_ref().map(System::total_memory), "availableBytes": sys.as_ref().map(System::available_memory) },
        "disks": disks,
        "uptimeSecs": System::uptime().ok(),
        "bootTime": System::boot_time().ok(),
        "debriefVersion": debrief_version,
        "collectorVersion": env!("CARGO_PKG_VERSION"),
        "environment": env,
    })
}
