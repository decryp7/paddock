//! A hardware (MAC) address and a SecureOn password: parsed once at the edge into typed values (parse, don't validate).

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

/// A 6-byte hardware address. On the wire and in the database it is the text `aa:bb:cc:dd:ee:ff`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Mac(pub [u8; 6]);

/// Why a text is not a usable hardware address or password.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MacError {
    /// Not six pairs of hex digits.
    #[error("A hardware address is six pairs of hex digits, like 00:11:22:33:44:55.")]
    Shape,
    /// All zeros or all ones: it names no machine.
    #[error("That hardware address (all zeros or all ones) cannot wake a computer.")]
    Unusable,
    /// A group address (the multicast bit is set): it names many machines.
    #[error("That is a group address, not one computer's hardware address.")]
    Group,
    /// A SecureOn password that is not 4 or 6 bytes of hex.
    #[error("A SecureOn password is 4 or 6 bytes of hex digits, like 01:02:03:04:05:06.")]
    Password,
}

/// The longest text parsed as an address (anything longer is not one).
const MAX_TEXT: usize = 64;

/// Splits `text` into hex groups separated by `:`, `-` or `.`-free runs; accepts one or two digits per group (macOS `arp`
/// prints `a:b:c:1:2:3`) or twelve digits with no separator.
fn bytes_of(text: &str, want: &[usize]) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_TEXT || !text.is_ascii() {
        return None;
    }
    let out: Vec<u8> = if text.contains([':', '-']) {
        let mut v = Vec::new();
        for part in text.split([':', '-']) {
            if part.is_empty() || part.len() > 2 {
                return None;
            }
            v.push(u8::from_str_radix(part, 16).ok()?);
        }
        v
    } else {
        if !text.len().is_multiple_of(2) {
            return None;
        }
        let mut v = Vec::new();
        for i in (0..text.len()).step_by(2) {
            v.push(u8::from_str_radix(&text[i..i + 2], 16).ok()?);
        }
        v
    };
    want.contains(&out.len()).then_some(out)
}

impl Mac {
    /// Parses a hardware address a person typed or a tool printed, in `:`, `-` or no-separator form. The all-zero, all-one and
    /// group addresses are refused: none can be woken.
    pub fn parse(text: &str) -> Result<Mac, MacError> {
        let v = bytes_of(text, &[6]).ok_or(MacError::Shape)?;
        let mut b = [0u8; 6];
        b.copy_from_slice(&v);
        if b == [0; 6] || b == [0xff; 6] {
            return Err(MacError::Unusable);
        }
        if b[0] & 1 == 1 {
            return Err(MacError::Group);
        }
        Ok(Mac(b))
    }

    /// The last two bytes, the form logs use (ER-27): `..:00:01`.
    pub fn short(&self) -> String {
        format!("..:{:02x}:{:02x}", self.0[4], self.0[5])
    }
}

impl fmt::Display for Mac {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = &self.0;
        write!(f, "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", b[0], b[1], b[2], b[3], b[4], b[5])
    }
}

impl fmt::Debug for Mac {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Mac({self})")
    }
}

impl FromStr for Mac {
    type Err = MacError;
    fn from_str(s: &str) -> Result<Mac, MacError> {
        Mac::parse(s)
    }
}

impl Serialize for Mac {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Mac {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Mac, D::Error> {
        let text = String::deserialize(d)?;
        Mac::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// A SecureOn password: 4 or 6 bytes appended to the magic packet (AMD "Magic Packet" white paper 20213 and the SecureOn
/// extension). Never printed: `Debug` hides it.
#[derive(Clone, PartialEq, Eq)]
pub struct SecureOn(Vec<u8>);

impl SecureOn {
    /// Parses 4 or 6 bytes of hex (separators `:` or `-` allowed).
    pub fn parse(text: &str) -> Result<SecureOn, MacError> {
        bytes_of(text, &[4, 6]).map(SecureOn).ok_or(MacError::Password)
    }

    /// The bytes appended to the packet.
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    /// The text kept in the credential store.
    pub fn to_text(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
    }
}

impl fmt::Debug for SecureOn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecureOn(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_common_spellings_to_one_address() {
        let want = Mac([2, 0, 0x5e, 0x10, 0, 1]);
        for t in ["02:00:5e:10:00:01", "02-00-5E-10-00-01", "02005e100001", " 2:0:5e:10:0:1 "] {
            assert_eq!(Mac::parse(t).unwrap(), want, "{t}");
        }
        assert_eq!(want.to_string(), "02:00:5e:10:00:01");
        assert_eq!(want.short(), "..:00:01");
    }

    #[test]
    fn refuses_what_cannot_be_woken() {
        assert_eq!(Mac::parse("00:00:00:00:00:00"), Err(MacError::Unusable));
        assert_eq!(Mac::parse("ff:ff:ff:ff:ff:ff"), Err(MacError::Unusable));
        assert_eq!(Mac::parse("01:00:5e:00:00:01"), Err(MacError::Group));
        for bad in [
            "",
            "02:00:5e:10:00",
            "02:00:5e:10:00:01:02",
            "zz:00:5e:10:00:01",
            "02:00:5e:10:00:001",
            "0200",
            "é2:00:5e:10:00:01",
        ] {
            assert_eq!(Mac::parse(bad), Err(MacError::Shape), "{bad}");
        }
        assert_eq!(Mac::parse(&"a:".repeat(100)), Err(MacError::Shape));
    }

    #[test]
    fn serialises_as_text_and_refuses_bad_text() {
        let m = Mac([2, 0, 0x5e, 0x10, 0, 1]);
        assert_eq!(serde_json::to_string(&m).unwrap(), "\"02:00:5e:10:00:01\"");
        assert_eq!(serde_json::from_str::<Mac>("\"02:00:5e:10:00:01\"").unwrap(), m);
        assert!(serde_json::from_str::<Mac>("\"nope\"").is_err());
        let bytes = postcard::to_stdvec(&m).unwrap();
        assert_eq!(postcard::from_bytes::<Mac>(&bytes).unwrap(), m);
        assert_eq!(format!("{m:?}"), "Mac(02:00:5e:10:00:01)");
        assert_eq!("02:00:5e:10:00:01".parse::<Mac>().unwrap(), m);
    }

    #[test]
    fn a_password_is_four_or_six_bytes_and_never_printed() {
        assert_eq!(SecureOn::parse("01:02:03:04").unwrap().bytes(), &[1, 2, 3, 4]);
        let six = SecureOn::parse("0a-0b-0c-0d-0e-0f").unwrap();
        assert_eq!(six.bytes().len(), 6);
        assert_eq!(six.to_text(), "0a:0b:0c:0d:0e:0f");
        assert_eq!(format!("{six:?}"), "SecureOn(..)");
        for bad in ["", "01:02:03", "01:02:03:04:05", "01:02:03:04:05:06:07", "xx:02:03:04"] {
            assert_eq!(SecureOn::parse(bad), Err(MacError::Password), "{bad}");
        }
    }
}
