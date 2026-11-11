//! The UDP sender and the card list. A magic packet is a UDP broadcast, sent from a socket bound to each card's own address so
//! it leaves through that card. On macOS a UDP broadcast is a Local Network operation (the agent needs that privilege, ADR 0032).

use dyno_core::wol::wake::UdpBroadcast;
use dyno_core::wol::{Interfaces, NetIf};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};

/// Sends from a real socket.
pub struct OsUdp;

impl UdpBroadcast for OsUdp {
    fn send(&self, source: Ipv4Addr, dest: Ipv4Addr, port: u16, payload: &[u8]) -> Result<(), String> {
        let socket =
            UdpSocket::bind(SocketAddr::V4(SocketAddrV4::new(source, 0))).map_err(|e| format!("could not use this network card: {e}"))?;
        socket
            .set_broadcast(true)
            .map_err(|e| format!("this network card does not allow broadcasts: {e}"))?;
        socket
            .send_to(payload, SocketAddr::V4(SocketAddrV4::new(dest, port)))
            .map(|_| ())
            .map_err(|e| send_error_words(&e, cfg!(target_os = "macos")))
    }
}

/// The words for a refused send. On macOS "No route to host" (65) or "Operation not permitted" (1) from a broadcast is how a denied
/// Local Network privilege shows (Apple TN3179), so the message says where to look.
fn send_error_words(e: &std::io::Error, macos: bool) -> String {
    let base = format!("the network refused the packet: {e}");
    match (macos, e.raw_os_error()) {
        (true, Some(1 | 65)) => {
            format!("{base}. macOS may be blocking Dyno from the local network: check System Settings, Privacy & Security, Local Network")
        }
        _ => base,
    }
}

/// The cards, from the OS's list of addresses.
pub struct OsInterfaces;

impl Interfaces for OsInterfaces {
    fn ipv4(&self) -> Vec<NetIf> {
        let Ok(networks) = sysinfo::Networks::new_with_refreshed_list()
            .inspect_err(|why| log::warn!("the network interface list is unavailable (sysinfo): {why}; no cards are offered"))
        else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (name, data) in &networks {
            for n in data.ip_networks() {
                if let std::net::IpAddr::V4(addr) = n.addr {
                    out.push(NetIf {
                        name: name.to_string_lossy().into_owned(),
                        addr,
                        prefix: n.prefix,
                    });
                }
            }
        }
        out.sort_by(|a, b| (&a.name, a.addr).cmp(&(&b.name, b.addr)));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Loopback only: a datagram to a loopback port this test listens on (nothing leaves the machine, ER-14).
    #[test]
    fn the_sender_delivers_a_datagram_over_loopback() {
        let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
        rx.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let port = rx.local_addr().unwrap().port();
        OsUdp.send(Ipv4Addr::LOCALHOST, Ipv4Addr::LOCALHOST, port, &[1, 2, 3]).unwrap();
        let mut buf = [0u8; 16];
        let (n, _) = rx.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..n], &[1, 2, 3]);
    }

    #[test]
    fn an_address_that_is_not_ours_is_an_error_with_words_not_a_panic() {
        let e = OsUdp.send(Ipv4Addr::new(192, 0, 2, 123), Ipv4Addr::LOCALHOST, 9, &[0]).unwrap_err();
        assert!(e.starts_with("could not use this network card"), "{e}");
    }

    #[test]
    fn a_denied_local_network_on_macos_says_where_to_look() {
        let denied = std::io::Error::from_raw_os_error(65);
        assert!(send_error_words(&denied, true).contains("Local Network"));
        assert!(!send_error_words(&denied, false).contains("Local Network"));
        assert!(!send_error_words(&std::io::Error::from_raw_os_error(101), true).contains("Local Network"));
    }

    #[test]
    fn the_card_list_has_only_ipv4_addresses_with_valid_prefixes() {
        for i in OsInterfaces.ipv4() {
            assert!(i.prefix <= 32);
        }
    }
}
