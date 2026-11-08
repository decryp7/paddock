//! The magic packet (AMD "Magic Packet Technology" white paper 20213): six `0xFF` bytes, then the hardware address sixteen
//! times, then an optional SecureOn password of 4 or 6 bytes. Sent as the payload of a UDP datagram (port 9, discard).

use super::mac::{Mac, SecureOn};

/// The port Dyno sends to (the discard service; the NIC looks at the payload, not the port).
pub const WOL_PORT: u16 = 9;

/// The payload for `mac`, with `secure_on` appended when there is one.
pub fn magic_packet(mac: &Mac, secure_on: Option<&SecureOn>) -> Vec<u8> {
    let mut p = Vec::with_capacity(6 + 16 * 6 + 6);
    p.extend_from_slice(&[0xff; 6]);
    for _ in 0..16 {
        p.extend_from_slice(&mac.0);
    }
    if let Some(s) = secure_on {
        p.extend_from_slice(s.bytes());
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC: Mac = Mac([0x02, 0x00, 0x5e, 0x10, 0x00, 0x01]);

    #[test]
    fn the_packet_is_sync_then_sixteen_copies() {
        let p = magic_packet(&MAC, None);
        assert_eq!(p.len(), 102);
        assert_eq!(&p[..6], &[0xff; 6]);
        for i in 0..16 {
            assert_eq!(&p[6 + i * 6..12 + i * 6], &MAC.0);
        }
    }

    #[test]
    fn a_password_is_appended_after_the_copies() {
        let four = SecureOn::parse("01:02:03:04").unwrap();
        let p = magic_packet(&MAC, Some(&four));
        assert_eq!(p.len(), 106);
        assert_eq!(&p[102..], &[1, 2, 3, 4]);
        let six = SecureOn::parse("0a:0b:0c:0d:0e:0f").unwrap();
        assert_eq!(magic_packet(&MAC, Some(&six)).len(), 108);
        assert_eq!(WOL_PORT, 9);
    }
}
