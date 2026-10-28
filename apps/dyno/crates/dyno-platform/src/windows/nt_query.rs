//! The call behind [`crate::nt`]: `NtQuerySystemInformation` from `ntdll.dll`, loaded at run time as Microsoft documents
//! for functions that are not in an import library, and Dyno's own counters from the documented per-process APIs for the
//! start-up self-check.

use crate::nt::{
    read_with_growth, ApiCounters, NtError, NtProcess, QueryOutcome, INITIAL_BUFFER, MAX_BUFFER, STATUS_INFO_LENGTH_MISMATCH,
    SYSTEM_PROCESS_INFORMATION,
};
use crate::processes::NtSource;
use std::ffi::c_void;
use windows::core::{s, w};
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessIoCounters, GetProcessTimes, IO_COUNTERS};

type NtQuerySystemInformation = unsafe extern "system" fn(u32, *mut c_void, u32, *mut u32) -> i32;

/// The loaded function, when `ntdll.dll` has it.
pub struct NtQuery {
    query: Option<NtQuerySystemInformation>,
}

impl NtQuery {
    /// Looks the function up in the `ntdll.dll` that is always loaded in a process.
    #[allow(unsafe_code)]
    pub fn load() -> Self {
        // SAFETY: both calls take NUL-terminated literals; the module stays loaded for the life of the process.
        let query = unsafe {
            GetModuleHandleW(w!("ntdll.dll"))
                .ok()
                .and_then(|m| GetProcAddress(m, s!("NtQuerySystemInformation")))
                // SAFETY: NtQuerySystemInformation has exactly this documented signature on every supported Windows.
                .map(|f| std::mem::transmute::<unsafe extern "system" fn() -> isize, NtQuerySystemInformation>(f))
        };
        NtQuery { query }
    }
}

fn filetime(f: FILETIME) -> u64 {
    (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)
}

impl NtSource for NtQuery {
    #[allow(unsafe_code)]
    fn snapshot(&mut self) -> Result<Vec<NtProcess>, NtError> {
        let query = self.query.ok_or(NtError::Unsupported)?;
        let buf = read_with_growth(
            |buf| {
                let mut needed = 0u32;
                let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
                // SAFETY: `buf` is valid for `len` bytes of writes and `needed` for one u32; the call writes only inside them.
                let status = unsafe { query(SYSTEM_PROCESS_INFORMATION, buf.as_mut_ptr().cast(), len, &mut needed) };
                if status >= 0 {
                    QueryOutcome::Done(if needed == 0 { buf.len() } else { (needed as usize).min(buf.len()) })
                } else if status == STATUS_INFO_LENGTH_MISMATCH {
                    QueryOutcome::TooSmall(needed as usize)
                } else {
                    QueryOutcome::Failed(status)
                }
            },
            INITIAL_BUFFER,
            MAX_BUFFER,
        )?;
        crate::nt::parse_processes(&buf)
    }

    #[allow(unsafe_code)]
    fn own_counters(&mut self) -> Option<ApiCounters> {
        // SAFETY: the pseudo handle needs no closing; every out parameter is a valid local of the size the call expects.
        unsafe {
            let me = GetCurrentProcess();
            let (mut created, mut exited, mut kernel, mut user) =
                (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
            GetProcessTimes(me, &mut created, &mut exited, &mut kernel, &mut user).ok()?;
            let mut mem = PROCESS_MEMORY_COUNTERS::default();
            GetProcessMemoryInfo(me, &mut mem, std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32).ok()?;
            let mut io = IO_COUNTERS::default();
            GetProcessIoCounters(me, &mut io).ok()?;
            Some(ApiCounters {
                cpu_time: filetime(kernel).saturating_add(filetime(user)),
                working_set: mem.WorkingSetSize as u64,
                read_bytes: io.ReadTransferCount,
                write_bytes: io.WriteTransferCount,
            })
        }
    }
}
