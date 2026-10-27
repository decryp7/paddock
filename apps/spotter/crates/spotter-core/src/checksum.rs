//! SHA-256 digests: parsing them from APIs and checksum files, and computing them while downloading.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A SHA-256 digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
    /// From 64 hex digits, optionally prefixed with `sha256:` (as GitHub and OCI write them).
    pub fn parse(text: &str) -> Option<Sha256Digest> {
        let t = text.trim();
        let t = t.strip_prefix("sha256:").or_else(|| t.strip_prefix("SHA256:")).unwrap_or(t);
        if t.len() != 64 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let mut out = [0u8; 32];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(t.get(i * 2..i * 2 + 2)?, 16).ok()?;
        }
        Some(Sha256Digest(out))
    }

    /// Lowercase hex.
    pub fn to_hex(self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// The digest of `bytes`.
    pub fn of(bytes: &[u8]) -> Sha256Digest {
        let mut h = Hasher::new();
        h.update(bytes);
        h.finish()
    }
}

impl TryFrom<String> for Sha256Digest {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        Sha256Digest::parse(&s).ok_or_else(|| format!("\"{s}\" is not a SHA-256 digest"))
    }
}

impl From<Sha256Digest> for String {
    fn from(d: Sha256Digest) -> String {
        d.to_hex()
    }
}

impl std::fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// Computes a digest incrementally (downloads feed it chunk by chunk, resumed downloads first feed the part file).
#[derive(Default, Clone)]
pub struct Hasher(Sha256);

impl Hasher {
    /// A fresh hasher.
    pub fn new() -> Hasher {
        Hasher(Sha256::new())
    }

    /// Adds bytes.
    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    /// The digest.
    pub fn finish(self) -> Sha256Digest {
        Sha256Digest(self.0.finalize().into())
    }
}

/// The digest for `file_name` in a checksum file. Understands `sha256sum` output (`<hex>  name`, `<hex> *name`),
/// BSD style (`SHA256 (name) = <hex>`) and a file holding only the digest. Names are compared without folders.
pub fn find_in_checksum_file(text: &str, file_name: &str) -> Option<Sha256Digest> {
    let base = |n: &str| n.rsplit(['/', '\\']).next().unwrap_or(n).to_string();
    let wanted = base(file_name);
    let mut only: Option<Sha256Digest> = None;
    let mut lines = 0;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        lines += 1;
        if let Some(rest) = line.strip_prefix("SHA256 (").or_else(|| line.strip_prefix("SHA256(")) {
            if let Some((name, hex)) = rest.split_once(")") {
                let hex = hex.trim_start().strip_prefix('=').unwrap_or(hex);
                if base(name) == wanted {
                    return Sha256Digest::parse(hex);
                }
            }
            continue;
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let hex = parts.next().unwrap_or("");
        match parts.next().map(|n| n.trim_start().trim_start_matches('*')) {
            Some(name) if base(name) == wanted => return Sha256Digest::parse(hex),
            Some(_) => {}
            None => only = Sha256Digest::parse(hex),
        }
    }
    if lines == 1 {
        only
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn digests_parse_print_and_compute() {
        let d = Sha256Digest::parse(HEX).unwrap();
        assert_eq!(d.to_hex(), HEX);
        assert_eq!(Sha256Digest::of(b""), d);
        assert_eq!(Sha256Digest::parse(&format!("sha256:{}", HEX.to_uppercase())), Some(d));
        assert_eq!(Sha256Digest::parse(&format!("SHA256:{HEX}")), Some(d));
        for bad in ["", "abc", &HEX[1..], &format!("{}g", &HEX[1..]), &format!("é{}", &HEX[2..])] {
            assert_eq!(Sha256Digest::parse(bad), None, "{bad}");
        }
        assert_eq!(d.to_string(), HEX);
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<Sha256Digest>(&json).unwrap(), d);
        assert!(serde_json::from_str::<Sha256Digest>("\"nope\"").is_err());
        let mut h = Hasher::default();
        h.update(b"a");
        h.update(b"bc");
        assert_eq!(h.finish(), Sha256Digest::of(b"abc"));
    }

    #[test]
    fn checksum_files_in_every_common_format() {
        let other = "0".repeat(64);
        let gnu = format!("{other}  other.zip\n{HEX}  dist/app.zip\n");
        assert_eq!(find_in_checksum_file(&gnu, "app.zip").unwrap().to_hex(), HEX);
        let binary = format!("# comment\n{HEX} *app.zip\n");
        assert_eq!(find_in_checksum_file(&binary, "C:\\x\\app.zip").unwrap().to_hex(), HEX);
        let bsd = format!("SHA256 (other.zip) = {other}\nSHA256 (app.zip) = {HEX}\n");
        assert_eq!(find_in_checksum_file(&bsd, "app.zip").unwrap().to_hex(), HEX);
        assert_eq!(
            find_in_checksum_file(&format!("SHA256(app.zip)= {HEX}"), "app.zip")
                .unwrap()
                .to_hex(),
            HEX
        );
        assert_eq!(find_in_checksum_file(&format!("{HEX}\n"), "anything.zip").unwrap().to_hex(), HEX);
        assert_eq!(find_in_checksum_file(&gnu, "missing.zip"), None);
        assert_eq!(
            find_in_checksum_file(&format!("{HEX}\n{other}\n"), "a"),
            None,
            "two bare digests are ambiguous"
        );
        assert_eq!(find_in_checksum_file("SHA256 (broken", "a"), None);
        assert_eq!(find_in_checksum_file("", "a"), None);
    }
}
