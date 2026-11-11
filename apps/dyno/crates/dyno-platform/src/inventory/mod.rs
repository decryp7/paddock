//! Reading the memory layout: pure parsers for what each OS reports ([`windows`] CIM rows as JSON, [`macos`]
//! `system_profiler` JSON, [`linux`] SMBIOS tables), and [`read_memory_layout`], the thin reader that runs the
//! right one. Called at agent start and on demand, never from the sampling loop.
#![cfg_attr(
    not(test),
    deny(clippy::let_underscore_must_use, clippy::let_underscore_future, clippy::let_underscore_untyped)
)]

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
    let _: Duration = TOOL_TIMEOUT;
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
    match crate::system_powershell::windows_system_directory() {
        Some(dir) => read_windows_memory(&crate::runner::SystemRunner, &dir, total),
        None => MemoryLayout::TotalOnly {
            total,
            reason: Unavailable::failed("The memory details could not be read: the system folder is unknown."),
        },
    }
}

/// The Windows memory read through `runner`: the two CIM queries run by the system PowerShell, which is the one in `system_dir` (the
/// OS's system folder) with its module path pinned (see [`crate::system_powershell`]). The reader runs as SYSTEM in the helper.
#[cfg(any(windows, test))]
pub(crate) fn read_windows_memory(runner: &dyn crate::runner::ToolRunner, system_dir: &str, total: u64) -> MemoryLayout {
    let run = |script: &str| {
        let tool = crate::system_powershell::system_powershell(system_dir, script);
        runner.run_tool(&tool, TOOL_TIMEOUT).and_then(|out| {
            if out.ok {
                Ok(out.stdout)
            } else {
                Err(dyno_core::ports::PlatformError::Failed(format!(
                    "powershell.exe failed: {}",
                    if out.error.is_empty() { "no message" } else { out.error.as_str() }
                )))
            }
        })
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

#[cfg(test)]
mod windows_read_tests {
    use super::*;
    use crate::runner::{Tool, ToolOutput, ToolRunner};
    use dyno_core::ports::PlatformError;
    use std::sync::{Arc, Mutex};

    struct Records(Arc<Mutex<Vec<Tool>>>);
    impl ToolRunner for Records {
        fn run_tool(&self, t: &Tool, _: Duration) -> Result<ToolOutput, PlatformError> {
            self.0.lock().unwrap().push(t.clone());
            Ok(ToolOutput {
                ok: true,
                stdout: "[]".into(),
                error: String::new(),
            })
        }
    }

    /// Plan WP-6 (3), review P1-1: the memory read runs as SYSTEM too, so it uses the system folder's PowerShell and the pinned module path.
    #[test]
    fn the_powershell_run_as_system_pins_its_module_path_to_pshome() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let _ = read_windows_memory(&Records(seen.clone()), r"D:\Win\System32", 8 << 30);
        let tools = seen.lock().unwrap();
        assert_eq!(tools.len(), 2, "the module query and the array query");
        for t in tools.iter() {
            assert_eq!(
                t.program, r"D:\Win\System32\WindowsPowerShell\v1.0\powershell.exe",
                "never a bare name, never C:\\Windows"
            );
            assert_eq!(
                t.env,
                [(
                    "PSModulePath".to_string(),
                    r"D:\Win\System32\WindowsPowerShell\v1.0\Modules".to_string()
                )]
            );
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn read_impl(total: u64) -> MemoryLayout {
    let _: Duration = TOOL_TIMEOUT;
    MemoryLayout::TotalOnly {
        total,
        reason: Unavailable::failed("Memory details are not available on this operating system."),
    }
}
