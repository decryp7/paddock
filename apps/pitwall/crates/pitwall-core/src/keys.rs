//! Per-app API keys (ADR 0007). A key is `pw_` plus 32 random bytes (base64url); Pitwall stores only its SHA-256
//! and shows the first characters to tell keys apart. Keys are high-entropy, so a plain hash is enough (no password
//! stretching), and comparison is constant-time. Randomness is passed in, so this module stays pure.

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Every Pitwall key starts with this.
pub const KEY_PREFIX: &str = "pw_";
/// Characters of a key shown in the UI.
pub const SHOWN_CHARS: usize = 10;
/// Longest credential header Pitwall reads.
pub const MAX_CREDENTIAL_LEN: usize = 256;

/// A new key's secret (shown once) and what Pitwall keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewKey {
    /// The secret, for the app's credential store.
    pub secret: String,
    /// What Pitwall stores.
    pub stored: StoredKey,
}

/// What Pitwall stores of a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredKey {
    /// The key's id (its first characters after the prefix).
    pub id: String,
    /// `pw_3f9a1c…`, for the UI.
    pub shown: String,
    /// SHA-256 of the secret, lowercase hex.
    pub sha256: String,
    /// When it was created.
    pub created_at: u64,
    /// When it was last used.
    #[serde(default)]
    pub last_used_at: Option<u64>,
}

/// Makes a key from 32 random bytes.
pub fn generate(random: [u8; 32], now: u64) -> NewKey {
    let secret = format!("{KEY_PREFIX}{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random));
    let stored = StoredKey {
        id: secret[KEY_PREFIX.len()..KEY_PREFIX.len() + 8].to_string(),
        shown: format!("{}…", &secret[..SHOWN_CHARS]),
        sha256: hash(&secret),
        created_at: now,
        last_used_at: None,
    };
    NewKey { secret, stored }
}

/// SHA-256 of a secret, lowercase hex.
pub fn hash(secret: &str) -> String {
    let d = Sha256::digest(secret.as_bytes());
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// Constant-time equality of two strings of the same length (different lengths are unequal at once; hashes always
/// have the same length).
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Whether a presented secret matches a stored key.
pub fn matches(secret: &str, key: &StoredKey) -> bool {
    constant_time_eq(&hash(secret), &key.sha256)
}

/// The credential of a request: `Authorization: Bearer <key>` (OpenAI, Ollama clients with a key) or
/// `x-api-key: <key>` (Anthropic). `None` when absent; over-long or malformed values are treated as absent.
pub fn credential<'a>(authorization: Option<&'a str>, x_api_key: Option<&'a str>) -> Option<&'a str> {
    let bearer = authorization.and_then(|a| {
        let a = a.trim();
        a.strip_prefix("Bearer ").or_else(|| a.strip_prefix("bearer ")).map(str::trim)
    });
    bearer
        .or(x_api_key.map(str::trim))
        .filter(|k| !k.is_empty() && k.len() <= MAX_CREDENTIAL_LEN && k.bytes().all(|b| b.is_ascii_graphic()))
}

/// A six-digit pairing code both apps show, from a random number.
pub fn pairing_code(random: u32) -> String {
    format!("{:06}", random % 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_hashed_and_shown_partially() {
        let k = generate([7; 32], 5);
        assert!(k.secret.starts_with("pw_") && k.secret.len() == 3 + 43);
        assert_eq!(k.stored.id.len(), 8);
        assert!(k.stored.shown.ends_with('…') && k.secret.starts_with(k.stored.shown.trim_end_matches('…')));
        assert!(!k.stored.sha256.contains(&k.secret[3..]));
        assert_eq!(k.stored.sha256.len(), 64);
        assert!(matches(&k.secret, &k.stored));
        assert!(!matches("pw_wrong", &k.stored));
        assert_ne!(generate([8; 32], 5).secret, k.secret);
        let json = serde_json::to_string(&k.stored).unwrap();
        assert!(!json.contains(&k.secret));
        let back: StoredKey = serde_json::from_str(&json).unwrap();
        assert_eq!(back, k.stored);
    }

    #[test]
    fn constant_time() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "ab"));
    }

    #[test]
    fn credentials_from_headers() {
        assert_eq!(credential(Some("Bearer pw_x"), None), Some("pw_x"));
        assert_eq!(credential(Some(" bearer  pw_x "), None), Some("pw_x"));
        assert_eq!(credential(None, Some("pw_y")), Some("pw_y"));
        assert_eq!(credential(Some("Basic abc"), Some("pw_y")), Some("pw_y"));
        assert_eq!(credential(Some("Bearer "), None), None);
        assert_eq!(credential(Some(&format!("Bearer {}", "a".repeat(300))), None), None);
        assert_eq!(credential(Some("Bearer a b"), None), None);
        assert_eq!(credential(None, None), None);
    }

    #[test]
    fn pairing_codes() {
        assert_eq!(pairing_code(42), "000042");
        assert_eq!(pairing_code(u32::MAX), "967295");
    }
}
