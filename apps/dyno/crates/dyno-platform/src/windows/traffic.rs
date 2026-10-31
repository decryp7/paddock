//! Per-program traffic on Windows through the per-connection TCP statistics (`SetPerTcpConnectionEStats` /
//! `GetPerTcpConnectionEStats`). Switching collection on needs administrator rights, and Dyno never runs elevated, so
//! without them this says so (the helper will do it later). IPv4 connections only; UDP has no counters.

use super::net::{rows, tcp};
use crate::parse::win_estats::{counters_by_pid, enable_failure, rod_bytes, ERROR_ACCESS_DENIED, ERROR_NOT_FOUND};
use dyno_core::network_traffic::TrafficCounter;
use dyno_core::ports::{PlatformError, ProcessTraffic};
use std::collections::HashMap;
use windows::Win32::NetworkManagement::IpHelper::{
    GetPerTcpConnectionEStats, SetPerTcpConnectionEStats, TCP_ESTATS_DATA_ROD_v0, TCP_ESTATS_DATA_RW_v0, TcpConnectionEstatsData,
    MIB_TCPROW_LH, MIB_TCPROW_LH_0, MIB_TCPROW_OWNER_PID,
};
use windows::Win32::Networking::WinSock::AF_INET;

/// `MIB_TCP_STATE_ESTAB`.
const ESTABLISHED: u32 = 5;
/// Most connections asked about per read.
const MAX_CONNECTIONS: usize = 8_192;

/// Reads the byte counters of established IPv4 connections.
#[derive(Debug, Default, Clone, Copy)]
pub struct WinTraffic;

/// Switches collection on for one connection and reads its bytes `(in, out)`. `Err(code)` is the Win32 error.
fn connection_bytes(row: &MIB_TCPROW_OWNER_PID) -> Result<Option<(u64, u64)>, u32> {
    let key = MIB_TCPROW_LH {
        Anonymous: MIB_TCPROW_LH_0 { dwState: row.dwState },
        dwLocalAddr: row.dwLocalAddr,
        dwLocalPort: row.dwLocalPort,
        dwRemoteAddr: row.dwRemoteAddr,
        dwRemotePort: row.dwRemotePort,
    };
    let rw = TCP_ESTATS_DATA_RW_v0 { EnableCollection: true };
    // SAFETY: `rw` is a plain struct viewed as bytes for exactly its size; `key` is a valid row. Both outlive the call.
    let set = unsafe {
        let bytes = std::slice::from_raw_parts(std::ptr::addr_of!(rw).cast::<u8>(), size_of::<TCP_ESTATS_DATA_RW_v0>());
        SetPerTcpConnectionEStats(&key, TcpConnectionEstatsData, bytes, 0, 0)
    };
    match set {
        0 => {}
        ERROR_NOT_FOUND => return Ok(None),
        code => return Err(code),
    }
    let mut rod = vec![0u8; size_of::<TCP_ESTATS_DATA_ROD_v0>()];
    // SAFETY: `rod` is writable for its length, which is the size of the version-0 struct requested.
    let got = unsafe { GetPerTcpConnectionEStats(&key, TcpConnectionEstatsData, None, 0, None, 0, Some(&mut rod), 0) };
    match got {
        0 => Ok(rod_bytes(&rod)),
        // A fresh connection has nothing yet, or ended meanwhile: not an error for the whole read.
        ERROR_NOT_FOUND => Ok(None),
        code => Err(code),
    }
}

impl ProcessTraffic for WinTraffic {
    fn counters(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
        let table =
            tcp(u32::from(AF_INET.0)).map_err(|e| PlatformError::Failed(crate::parse::win_tcp::table_error("TCP connections", e)))?;
        let mut conns = Vec::new();
        for row in rows::<MIB_TCPROW_OWNER_PID>(&table)
            .iter()
            .filter(|r| r.dwState == ESTABLISHED)
            .take(MAX_CONNECTIONS)
        {
            match connection_bytes(row) {
                Ok(Some((rx, tx))) => conns.push((row.dwOwningPid, rx, tx)),
                Ok(None) => {}
                // Not elevated: stop at the first refusal instead of asking for every connection.
                Err(code) if code == ERROR_ACCESS_DENIED => return Err(enable_failure(code)),
                Err(_) => {}
            }
        }
        let names = program_names(&conns.iter().map(|c| c.0).collect::<Vec<_>>());
        Ok(counters_by_pid(&conns, &names))
    }
}

fn program_names(pids: &[u32]) -> HashMap<u32, String> {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let list: Vec<Pid> = pids.iter().map(|p| Pid::from_u32(*p)).collect();
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&list), true);
    pids.iter()
        .filter_map(|p| {
            sys.process(Pid::from_u32(*p))
                .map(|pr| (*p, pr.name().to_string_lossy().into_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_administrator_rights_the_reply_says_so_or_lists_programs() {
        // Collection can be switched on only when elevated; either answer is honest, a panic is not.
        match WinTraffic.counters() {
            Ok(_) => {}
            Err(PlatformError::Unsupported(m)) => assert!(m.contains("administrator")),
            Err(e) => panic!("unexpected: {e:?}"),
        }
    }
}
