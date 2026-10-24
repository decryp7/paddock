//! The 256-bit per-launch secret a host gives its child (or writes into its discovery file), checked in constant time.

use std::fmt;
use std::io;
use subtle::ConstantTimeEq;

/// Bytes of a [`Token`].
pub const TOKEN_BYTES: usize = 32;

/// 32 random bytes. `Debug` never shows them.
#[derive(Clone)]
pub struct Token([u8; TOKEN_BYTES]);

impl Token {
    /// A fresh random token.
    pub fn random() -> io::Result<Token> {
        let mut bytes = [0u8; TOKEN_BYTES];
        getrandom::fill(&mut bytes)
            .map_err(|e| io::Error::other(format!("no random numbers available: {e}")))?;
        Ok(Token(bytes))
    }

    /// The token made of `bytes`.
    pub fn from_bytes(bytes: [u8; TOKEN_BYTES]) -> Token {
        Token(bytes)
    }

    /// Its bytes.
    pub fn as_bytes(&self) -> &[u8; TOKEN_BYTES] {
        &self.0
    }

    /// Lowercase hex, for a bootstrap line or a discovery file.
    pub fn to_hex(&self) -> String {
        hex(&self.0)
    }

    /// Parses 64 hex digits.
    pub fn from_hex(text: &str) -> Option<Token> {
        let digits = text.as_bytes();
        if digits.len() != TOKEN_BYTES * 2 {
            return None;
        }
        let mut bytes = [0u8; TOKEN_BYTES];
        for (b, pair) in bytes.iter_mut().zip(digits.as_chunks::<2>().0) {
            let s = std::str::from_utf8(pair.as_slice()).ok()?;
            *b = u8::from_str_radix(s, 16).ok()?;
        }
        Some(Token(bytes))
    }

    /// Whether `other` is this token, compared in constant time.
    pub fn matches(&self, other: &[u8; TOKEN_BYTES]) -> bool {
        self.0.ct_eq(other).into()
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(..)")
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `n` random bytes as lowercase hex (endpoint names).
pub(crate) fn random_hex(n: usize) -> io::Result<String> {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b)
        .map_err(|e| io::Error::other(format!("no random numbers available: {e}")))?;
    Ok(hex(&b))
}
