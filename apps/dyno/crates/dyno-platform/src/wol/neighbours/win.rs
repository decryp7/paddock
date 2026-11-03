//! The Windows neighbour table through the IP Helper API (`GetIpNetTable2`, IPv4 and IPv6 together). The table is copied into
//! Dyno's own types and freed at once; the API call and the raw reads are the only unsafe code, each with its reason.

use super::state_of_nl;
use dyno_core::ports::PlatformError;
use dyno_core::wol::{Mac, Neighbour};
use std::net::{Ipv4Addr, Ipv6Addr};
use windows::Win32::Foundation::NO_ERROR;
use windows::Win32::NetworkManagement::IpHelper::{
    ConvertInterfaceIndexToLuid, ConvertInterfaceLuidToAlias, FreeMibTable, GetIpNetTable2, MIB_IPNET_TABLE2,
};
use windows::Win32::NetworkManagement::Ndis::NET_LUID_LH;
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC};

/// How many UTF-16 units an interface alias may have (`NDIS_IF_MAX_STRING_SIZE` + 1).
const ALIAS_UNITS: usize = 257;

fn alias(index: u32) -> String {
    let mut luid = NET_LUID_LH::default();
    // SAFETY: `luid` is a valid out-pointer for the call; the API only writes it.
    if unsafe { ConvertInterfaceIndexToLuid(index, &mut luid) } != NO_ERROR {
        return format!("if{index}");
    }
    let mut buf = [0u16; ALIAS_UNITS];
    // SAFETY: `luid` was filled by the call above and `buf` is a writable slice of the length the API documents.
    if unsafe { ConvertInterfaceLuidToAlias(&luid, &mut buf) } != NO_ERROR {
        return format!("if{index}");
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Every table entry with a usable hardware address and state.
pub(super) fn table() -> Result<Vec<Neighbour>, PlatformError> {
    let mut raw: *mut MIB_IPNET_TABLE2 = std::ptr::null_mut();
    // SAFETY: `raw` is a valid out-pointer; on success the API allocates the table, which is freed below.
    let status = unsafe { GetIpNetTable2(AF_UNSPEC, &mut raw) };
    if status != NO_ERROR || raw.is_null() {
        return Err(PlatformError::Failed(format!("GetIpNetTable2 failed with code {}", status.0)));
    }
    let mut out = Vec::new();
    let mut names: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    // SAFETY: on success `raw` points to a table of `NumEntries` rows; the slice is read and dropped before the table is freed.
    let rows = unsafe { std::slice::from_raw_parts((*raw).Table.as_ptr(), (*raw).NumEntries as usize) };
    for row in rows {
        if out.len() >= dyno_core::wol::neighbours::MAX_NEIGHBOURS || row.PhysicalAddressLength != 6 {
            continue;
        }
        let Some(state) = state_of_nl(row.State.0) else { continue };
        let text = row.PhysicalAddress[..6]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":");
        let Ok(mac) = Mac::parse(&text) else { continue };
        // SAFETY: the union member read is chosen by the family the row itself states.
        let ip = unsafe {
            let family = row.Address.si_family;
            if family == AF_INET {
                Ipv4Addr::from(u32::from_be(row.Address.Ipv4.sin_addr.S_un.S_addr)).to_string()
            } else if family == AF_INET6 {
                Ipv6Addr::from(row.Address.Ipv6.sin6_addr.u.Byte).to_string()
            } else {
                continue;
            }
        };
        let interface = names.entry(row.InterfaceIndex).or_insert_with(|| alias(row.InterfaceIndex)).clone();
        out.push(Neighbour {
            name: None,
            mac,
            ip,
            interface,
            state,
        });
    }
    // SAFETY: `raw` came from `GetIpNetTable2` and is freed exactly once, after the last read.
    unsafe { FreeMibTable(raw as *const _) };
    Ok(out)
}
