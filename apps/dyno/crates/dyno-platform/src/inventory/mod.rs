//! Reading the memory layout: pure parsers for what each OS reports ([`windows`] CIM rows as JSON, [`macos`]
//! `system_profiler` JSON, [`linux`] SMBIOS tables), and [`read_memory_layout`], the thin reader that runs the
//! right one. Called at agent start and on demand, never from the sampling loop.

pub mod linux;
pub mod macos;
pub mod windows;

use dyno_core::inventory::{MemoryLayout, Unavailable};
use std::time::Duration;

/// The longest any inventory tool may run.
const TOOL_TIMEOUT: Duration = Duration::from_secs(8);

/// Reads the memory layout of this machine. `total` is the installed memory in bytes as the OS counts it, used when
/// the detail cannot be read. Never panics; failures come back as [`MemoryLayout::TotalOnly`] with the reason.
pub fn read_memory_layout(total: u64) -> MemoryLayout {
    read_impl(total)
}

#[cfg(target_os = "macos")]
fn read_impl(total: u64) -> MemoryLayout {
    let mut cmd = crate::process::command("/usr/sbin/system_profiler");
    cmd.args(["SPMemoryDataType", "-json"]);
    match crate::process::output_text(cmd, TOOL_TIMEOUT) {
        Ok(text) => macos::parse(&text, total),
        Err(e) => MemoryLayout::TotalOnly {
            total,
            reason: Unavailable::failed(format!("The memory details could not be read: {e}")),
        },
    }
}

#[cfg(target_os = "linux")]
fn read_impl(total: u64) -> MemoryLayout {
    use std::io::Read;
    let _ = TOOL_TIMEOUT;
    let file = match std::fs::File::open("/sys/firmware/dmi/tables/DMI") {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return MemoryLayout::TotalOnly {
                total,
                reason: Unavailable::linux_dmi_root_only(),
            }
        }
        Err(_) => {
            return MemoryLayout::TotalOnly {
                total,
                reason: Unavailable::failed("This computer does not publish its memory slot table (common in virtual machines)."),
            }
        }
    };
    let mut bytes = Vec::new();
    if let Err(e) = file.take(linux::MAX_TABLE_BYTES as u64).read_to_end(&mut bytes) {
        return MemoryLayout::TotalOnly {
            total,
            reason: Unavailable::failed(format!("The memory slot table could not be read: {e}")),
        };
    }
    linux::parse(&bytes, total)
}

#[cfg(windows)]
fn read_impl(total: u64) -> MemoryLayout {
    let run = |script: &str| {
        let mut cmd = crate::process::command("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]);
        crate::process::output_text(cmd, TOOL_TIMEOUT)
    };
    let modules = run("Get-CimInstance Win32_PhysicalMemory | Select-Object BankLabel,DeviceLocator,Capacity,Speed,ConfiguredClockSpeed,SMBIOSMemoryType,Manufacturer,PartNumber,FormFactor | ConvertTo-Json -Compress");
    let array =
        run("Get-CimInstance Win32_PhysicalMemoryArray | Select-Object MemoryDevices,MaxCapacity,MaxCapacityEx | ConvertTo-Json -Compress");
    match modules {
        Ok(m) => self::windows::parse(&m, array.as_deref().unwrap_or(""), total),
        Err(e) => MemoryLayout::TotalOnly {
            total,
            reason: Unavailable::failed(format!("The memory details could not be read: {e}")),
        },
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn read_impl(total: u64) -> MemoryLayout {
    let _ = TOOL_TIMEOUT;
    MemoryLayout::TotalOnly {
        total,
        reason: Unavailable::failed("Memory details are not available on this operating system."),
    }
}

/// Trimmed text, `None` when empty or a placeholder firmware leaves behind.
pub(crate) fn clean(s: &str) -> Option<String> {
    let t: String = s.trim().chars().take(64).collect();
    let l = t.to_ascii_lowercase();
    let junk = [
        "",
        "unknown",
        "not specified",
        "to be filled by o.e.m.",
        "[empty]",
        "none",
        "n/a",
        "0",
        "undefined",
        "empty",
    ];
    (!junk.contains(&l.as_str()) && !l.chars().all(|c| c == '0' || c == 'f' || c == ' ')).then_some(t)
}
