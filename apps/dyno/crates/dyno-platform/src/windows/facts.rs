//! The real Windows readers behind [`crate::win_facts_source::WinSystem`] (ADR 0033, WP-3): the registry, the power API, the
//! OS version call and WMI, all with the caller's own rights (Dyno changes nothing here). Which fact comes from where, and how a
//! failure reads, is in `win_facts_source`; the mapping is in `parse::win_facts`. This file only calls the OS and says what failed.

use crate::parse::win_facts::{classify_wmi_hresult, format_guid, DiskReadError, OsValues, Overlay, RawDisk};
use crate::win_facts_source::{PowerRead, RestartKey, WinFacts, WinSystem};
use std::path::PathBuf;
use windows::core::{s, w, GUID, PCWSTR};
use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, HLOCAL, WIN32_ERROR};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
use windows::Win32::System::Power::{PowerGetActiveScheme, PowerReadFriendlyName};
use windows::Win32::System::Registry::{
    RegCloseKey, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_WOW64_64KEY,
    REG_ROUTINE_FLAGS, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY,
};
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;

/// The facts of this Windows machine.
pub fn os_facts() -> WinFacts<OsWinSystem> {
    WinFacts::new(OsWinSystem)
}

/// The OS calls, made with the caller's rights. It holds no handle between calls.
pub struct OsWinSystem;

const CURRENT_VERSION: PCWSTR = w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");

/// A registry value name for a log line (the names are our own literals).
#[allow(unsafe_code)]
fn name_of(name: PCWSTR) -> String {
    // SAFETY: only called with the NUL-terminated literals of `os_values`.
    unsafe { name.to_string() }.unwrap_or_default()
}

fn code(e: WIN32_ERROR) -> String {
    format!("registry error {}", e.0)
}

/// Reads one string value (`None` when the key or value is missing).
#[allow(unsafe_code)]
fn reg_string(hive: HKEY, subkey: PCWSTR, name: PCWSTR, flags: REG_ROUTINE_FLAGS) -> Result<Option<String>, String> {
    let mut bytes = 0u32;
    // SAFETY: the key and value names are NUL-terminated; with no data pointer the call only reports the size in `bytes`.
    let rc = unsafe { RegGetValueW(hive, subkey, name, RRF_RT_REG_SZ | flags, None, None, Some(&mut bytes)) };
    if rc == ERROR_FILE_NOT_FOUND || rc == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    if rc != ERROR_SUCCESS && rc != ERROR_MORE_DATA {
        return Err(code(rc));
    }
    // A value is at most a few hundred characters; a bigger one is not a name Dyno would show.
    if bytes == 0 || bytes > 64 * 1024 {
        return Ok(None);
    }
    let mut buf = vec![0u16; (bytes as usize).div_ceil(2)];
    let mut size = bytes;
    // SAFETY: `buf` is valid for `size` bytes of writes, and `size` says so.
    let rc = unsafe {
        RegGetValueW(
            hive,
            subkey,
            name,
            RRF_RT_REG_SZ | flags,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if rc == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if rc != ERROR_SUCCESS {
        return Err(code(rc));
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Ok(Some(String::from_utf16_lossy(&buf[..len])))
}

/// Reads one DWORD value (`None` when the key or value is missing).
#[allow(unsafe_code)]
fn reg_dword(hive: HKEY, subkey: PCWSTR, name: PCWSTR, flags: REG_ROUTINE_FLAGS) -> Result<Option<u32>, String> {
    let mut value = 0u32;
    let mut size = 4u32;
    // SAFETY: `value` is valid for the 4 bytes `size` announces.
    let rc = unsafe {
        RegGetValueW(
            hive,
            subkey,
            name,
            RRF_RT_REG_DWORD | flags,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    };
    match rc {
        ERROR_SUCCESS => Ok(Some(value)),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(None),
        other => Err(code(other)),
    }
}

/// Whether a key under HKLM (64-bit view) exists.
#[allow(unsafe_code)]
fn hklm_key_exists(subkey: PCWSTR) -> Result<bool, String> {
    let mut key = HKEY::default();
    // SAFETY: the sub-key name is NUL-terminated and `key` is a valid out pointer; the key is closed below on success.
    let rc = unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey, None, KEY_QUERY_VALUE | KEY_WOW64_64KEY, &mut key) };
    match rc {
        ERROR_SUCCESS => {
            // SAFETY: `key` was just opened and is closed once.
            let _closed = unsafe { RegCloseKey(key) };
            Ok(true)
        }
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
        other => Err(code(other)),
    }
}

/// A GUID the power API allocated, as text; the allocation is freed here.
#[allow(unsafe_code)]
fn take_guid(p: *mut GUID) -> Option<String> {
    if p.is_null() {
        return None;
    }
    // SAFETY: `p` is the GUID `PowerGetActiveScheme` allocated for us, read once before it is freed.
    let g = unsafe { *p };
    // SAFETY: the buffer came from LocalAlloc inside the API, and the docs say the caller frees it with LocalFree.
    let _freed = unsafe { windows::Win32::Foundation::LocalFree(Some(HLOCAL(p.cast()))) };
    Some(format_guid(g.data1, g.data2, g.data3, g.data4))
}

#[allow(unsafe_code)]
fn friendly_name(scheme: &GUID) -> Option<String> {
    let mut bytes = 0u32;
    // SAFETY: `scheme` is valid; with no buffer the call reports the size needed in `bytes`.
    let rc = unsafe { PowerReadFriendlyName(None, Some(scheme), None, None, None, &mut bytes) };
    if (rc != ERROR_SUCCESS && rc != ERROR_MORE_DATA) || !(2..=64 * 1024).contains(&bytes) {
        return None;
    }
    let mut buf = vec![0u16; (bytes as usize).div_ceil(2)];
    // SAFETY: `buf` is valid for `bytes` bytes (rounded up to whole characters), as announced.
    let rc = unsafe { PowerReadFriendlyName(None, Some(scheme), None, None, Some(buf.as_mut_ptr().cast::<u8>()), &mut bytes) };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let wide: Vec<u16> = buf.into_iter().take_while(|&c| c != 0).collect();
    Some(String::from_utf16_lossy(&wide))
}

/// Asks `powrprof.dll` for the effective overlay (the Windows 11 power mode).
///
/// `PowerGetEffectiveOverlayScheme` is not in the Windows SDK metadata that the `windows` crate is generated from (checked in
/// windows 0.62.2), and no Microsoft Learn page for it exists (a fetch of its expected URL answered 404 on 2026-11-12). The
/// signature `DWORD PowerGetEffectiveOverlayScheme(GUID *EffectiveOverlayGuid)` and "0 on success" are therefore the widely
/// published declaration, not a Microsoft contract, and the one measurement is a single observed rc=0 on Windows 11 build 26300
/// (ADR 0033, WP-0 item 8). So the function is looked up at run time, and any non-zero code is reported, never read as "no overlay".
#[allow(unsafe_code)]
fn effective_overlay() -> Overlay {
    type OverlayFn = unsafe extern "system" fn(*mut GUID) -> u32;
    // SAFETY: a NUL-terminated literal; only the System32 copy of the library is loaded.
    let Ok(module) = (unsafe { LoadLibraryExW(w!("powrprof.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }) else {
        log::debug!(target: "dyno::facts", "power_plan: powrprof.dll did not load; no overlay is read");
        return Overlay::NotPresent;
    };
    // SAFETY: a NUL-terminated literal name.
    let Some(f) = (unsafe { GetProcAddress(module, s!("PowerGetEffectiveOverlayScheme")) }) else {
        log::debug!(target: "dyno::facts", "power_plan: this Windows has no PowerGetEffectiveOverlayScheme; no overlay is read");
        return Overlay::NotPresent;
    };
    // SAFETY: the signature is the one described above (a GUID out pointer, 0 on success); an unmeasured assumption, see the doc.
    let f = unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, OverlayFn>(f) };
    let mut guid = GUID::zeroed();
    // SAFETY: `guid` is a valid out pointer.
    let rc = unsafe { f(&mut guid) };
    if rc == 0 {
        Overlay::Read(format_guid(guid.data1, guid.data2, guid.data3, guid.data4))
    } else {
        log::warn!(target: "dyno::facts", "power_plan: PowerGetEffectiveOverlayScheme returned {rc}; the power mode is not reported");
        Overlay::Failed(rc)
    }
}

impl WinSystem for OsWinSystem {
    fn os_values(&mut self) -> Result<OsValues, String> {
        let hive = HKEY_LOCAL_MACHINE;
        let flags = RRF_SUBKEY_WOW6464KEY;
        let string = |name: PCWSTR| reg_string(hive, CURRENT_VERSION, name, flags);
        // A value that cannot be read leaves the name shorter, and the failure is logged with its code.
        let optional = |name: PCWSTR| {
            string(name).unwrap_or_else(|why| {
                log::warn!(target: "dyno::facts", "fact os: value {} unreadable ({why})", name_of(name));
                None
            })
        };
        let product_name = string(w!("ProductName"))?;
        Ok(OsValues {
            product_name,
            edition_id: optional(w!("EditionID")),
            display_version: optional(w!("DisplayVersion")),
            release_id: optional(w!("ReleaseId")),
            installation_type: optional(w!("InstallationType")),
            build: optional(w!("CurrentBuildNumber")).and_then(|b| b.trim().parse().ok()),
            ubr: reg_dword(hive, CURRENT_VERSION, w!("UBR"), flags).unwrap_or_else(|why| {
                log::warn!(target: "dyno::facts", "fact os: value UBR unreadable ({why})");
                None
            }),
        })
    }

    #[allow(unsafe_code)]
    fn api_build(&mut self) -> Option<u32> {
        let mut info = OSVERSIONINFOW {
            dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
            ..Default::default()
        };
        // SAFETY: `info` is valid and its size field is set, as the call requires.
        let status = unsafe { RtlGetVersion(&mut info) };
        (status.0 >= 0).then_some(info.dwBuildNumber)
    }

    fn restart_key_exists(&mut self, key: RestartKey) -> Result<bool, String> {
        hklm_key_exists(match key {
            RestartKey::UpdateRebootRequired => w!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired"),
            RestartKey::ServicingRebootPending => w!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending"),
        })
    }

    fn visual_fx_setting(&mut self) -> Result<Option<u32>, String> {
        reg_dword(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\VisualEffects"),
            w!("VisualFXSetting"),
            REG_ROUTINE_FLAGS(0),
        )
    }

    #[allow(unsafe_code)]
    fn power(&mut self) -> Result<PowerRead, String> {
        let mut scheme: *mut GUID = std::ptr::null_mut();
        // SAFETY: a valid out pointer; the API allocates the GUID, which `take_guid` frees.
        let rc = unsafe { PowerGetActiveScheme(None, &mut scheme) };
        if rc != ERROR_SUCCESS {
            return Err(format!("PowerGetActiveScheme error {}", rc.0));
        }
        let name = if scheme.is_null() {
            None
        } else {
            // SAFETY: non-null and allocated by the call above, not yet freed.
            friendly_name(unsafe { &*scheme })
        };
        Ok(PowerRead {
            scheme: take_guid(scheme),
            friendly_name: name,
            overlay: effective_overlay(),
        })
    }

    fn physical_disks(&mut self) -> Result<Vec<RawDisk>, DiskReadError> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Row {
            friendly_name: Option<String>,
            health_status: Option<u16>,
        }
        let fail = |e: wmi::WMIError| match e {
            wmi::WMIError::HResultError { hres } => classify_wmi_hresult(hres),
            other => DiskReadError::Failed(other.to_string().chars().take(80).collect()),
        };
        let wmi = wmi::WMIConnection::with_namespace_path(r"ROOT\Microsoft\Windows\Storage").map_err(fail)?;
        let rows: Vec<Row> = wmi
            .raw_query("SELECT FriendlyName, HealthStatus FROM MSFT_PhysicalDisk")
            .map_err(fail)?;
        Ok(rows
            .into_iter()
            .map(|r| RawDisk {
                friendly_name: r.friendly_name,
                health_status: r.health_status,
            })
            .collect())
    }

    fn home(&self) -> Option<PathBuf> {
        std::env::var_os("USERPROFILE").filter(|h| !h.is_empty()).map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::ports::FactsSource;

    // These run on the Windows runner only. They read this machine's own state, read-only, and write nothing.

    #[test]
    fn the_registry_names_this_windows() {
        let v = OsWinSystem.os_values().expect("the CurrentVersion key is readable by every user");
        assert!(v.product_name.as_deref().is_some_and(|p| p.starts_with("Windows")), "{v:?}");
        assert!(v.build.is_some_and(|b| b >= 10_000), "{v:?}");
    }

    #[test]
    fn rtl_get_version_agrees_with_the_registry_build() {
        let api = OsWinSystem.api_build().expect("RtlGetVersion exists on every supported Windows");
        let reg = OsWinSystem.os_values().unwrap().build.unwrap();
        assert_eq!(api, reg);
    }

    #[test]
    fn a_key_that_cannot_exist_is_absent_not_an_error() {
        assert_eq!(
            hklm_key_exists(w!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Dyno-test-no-such-key")),
            Ok(false)
        );
        assert_eq!(
            hklm_key_exists(w!(r"SOFTWARE\Microsoft\Windows\CurrentVersion")),
            Ok(true),
            "a control that can see a key"
        );
    }

    #[test]
    fn the_power_scheme_is_a_guid_with_a_name() {
        let p = OsWinSystem.power().expect("every user may read the active scheme");
        let guid = p.scheme.expect("a scheme GUID");
        assert_eq!(guid.len(), 36, "{guid}");
        assert!(p.friendly_name.is_some_and(|n| !n.is_empty()));
    }

    #[test]
    fn the_real_facts_name_the_os_and_are_never_the_fake() {
        // The profile folder is a temporary one: the test never walks the real %USERPROFILE% (ER-14).
        struct InTemp(OsWinSystem, PathBuf);
        impl WinSystem for InTemp {
            fn os_values(&mut self) -> Result<OsValues, String> {
                self.0.os_values()
            }
            fn api_build(&mut self) -> Option<u32> {
                self.0.api_build()
            }
            fn restart_key_exists(&mut self, key: RestartKey) -> Result<bool, String> {
                self.0.restart_key_exists(key)
            }
            fn visual_fx_setting(&mut self) -> Result<Option<u32>, String> {
                self.0.visual_fx_setting()
            }
            fn power(&mut self) -> Result<PowerRead, String> {
                self.0.power()
            }
            fn physical_disks(&mut self) -> Result<Vec<RawDisk>, DiskReadError> {
                self.0.physical_disks()
            }
            fn home(&self) -> Option<PathBuf> {
                Some(self.1.clone())
            }
        }
        let home = tempfile::tempdir().unwrap();
        let f = WinFacts::new(InTemp(OsWinSystem, home.path().to_path_buf())).facts();
        assert!(f.os.starts_with("Windows"), "{}", f.os);
        assert_ne!(f.os, "Fake OS 1.0");
        assert!(f.restart_pending.value().is_some(), "{:?}", f.restart_pending);
        assert!(f.power_plan.value().is_some(), "{:?}", f.power_plan);
        // Disk health is the administrator's, or a standard user's "needs administrator readings": never "Not checked yet".
        assert_ne!(f.disks.unavailable_reason(), Some("Not checked yet"));
        assert!(f.not_read_yet.is_empty());
    }
}
