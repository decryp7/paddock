//! The 256-bit secret a host gives its child, checked in constant time in the handshake.

use std::fmt;
use std::io;
use subtle::ConstantTimeEq;

/// 32 random bytes. `Debug` never shows them.
#[derive(Clone)]
pub struct Token([u8; 32]);

impl Token {
    /// A fresh random token.
    pub fn random() -> io::Result<Token> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|e| io::Error::other(format!("no random numbers available: {e}")))?;
        Ok(Token(bytes))
    }

    /// The token made of `bytes`.
    pub fn from_bytes(bytes: [u8; 32]) -> Token {
        Token(bytes)
    }

    /// Its bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase hex, for the child's bootstrap line.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Parses 64 hex digits.
    pub fn from_hex(hex: &str) -> Option<Token> {
        let digits = hex.as_bytes();
        if digits.len() != 64 {
            return None;
        }
        let mut bytes = [0u8; 32];
        for (b, pair) in bytes.iter_mut().zip(digits.as_chunks::<2>().0) {
            let s = std::str::from_utf8(pair.as_slice()).ok()?;
            *b = u8::from_str_radix(s, 16).ok()?;
        }
        Some(Token(bytes))
    }

    /// Whether `other` is this token, compared in constant time.
    pub fn matches(&self, other: &[u8; 32]) -> bool {
        self.0.ct_eq(other).into()
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(..)")
    }
}

/// `bytes` random bytes as lowercase hex (endpoint names).
pub(crate) fn random_hex(bytes: usize) -> io::Result<String> {
    let mut b = vec![0u8; bytes];
    getrandom::fill(&mut b).map_err(|e| io::Error::other(format!("no random numbers available: {e}")))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_roundtrip_through_hex_and_compare_exactly() {
        let t = Token::random().unwrap();
        assert_ne!(t.as_bytes(), Token::random().unwrap().as_bytes());
        let hex = t.to_hex();
        assert_eq!(hex.len(), 64);
        assert!(Token::from_hex(&hex).unwrap().matches(t.as_bytes()));
        let mut other = *t.as_bytes();
        other[31] ^= 1;
        assert!(!t.matches(&other));
        assert!(Token::from_hex("zz").is_none());
        assert!(Token::from_hex(&"g".repeat(64)).is_none());
        assert!(Token::from_hex(&"é".repeat(32)).is_none());
        assert_eq!(format!("{t:?}"), "Token(..)");
        assert!(Token::from_bytes([1; 32]).matches(&[1; 32]));
        assert_eq!(random_hex(4).unwrap().len(), 8);
    }
}
