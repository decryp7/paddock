//! SHA-256 checksums of runner downloads: parsing the published checksum files and verifying a download.
//!
//! Gitea publishes `<asset>.sha256` (one line, `<hex>  <file name>`) next to every runner binary and a
//! `checksums.txt` with one such line per asset (see `docs/adr/0002-runner-auto-update.md`).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::io::Read;

/// Largest checksum file Pitcrew reads (a `checksums.txt` of a release is about 2-3 KB).
pub const MAX_CHECKSUM_FILE: usize = 64 * 1024;

/// A SHA-256 digest.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Sha256Digest([u8; 32]);

/// Problems with checksums.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChecksumError {
    /// Not 64 hexadecimal digits.
    #[error("{0:?} is not a SHA-256 checksum (64 hexadecimal digits)")]
    BadDigest(String),
    /// The checksum file has no line for the file.
    #[error("the checksum file has no entry for {0}")]
    NoEntry(String),
    /// The checksum file lists the file twice with different checksums.
    #[error("the checksum file lists {0} twice with different checksums")]
    Ambiguous(String),
    /// The checksum file is larger than [`MAX_CHECKSUM_FILE`].
    #[error("the checksum file is larger than {MAX_CHECKSUM_FILE} bytes")]
    TooLarge,
    /// Two independent sources publish different checksums for the same file.
    #[error("the checksum sources disagree about {file}: {first} vs {second}")]
    SourcesDisagree {
        /// The file.
        file: String,
        /// The first source's checksum.
        first: Sha256Digest,
        /// The second source's checksum.
        second: Sha256Digest,
    },
    /// The download does not match its published checksum.
    #[error("the download does not match its published checksum (expected {expected}, got {actual})")]
    Mismatch {
        /// The published checksum.
        expected: Sha256Digest,
        /// The checksum of the downloaded bytes.
        actual: Sha256Digest,
    },
    /// Reading the download failed.
    #[error("could not read the download: {0}")]
    Read(String),
}

impl Sha256Digest {
    /// Parses 64 hexadecimal digits (either case).
    pub fn parse_hex(s: &str) -> Result<Self, ChecksumError> {
        let t = s.trim();
        let bytes = t.as_bytes();
        if bytes.len() != 64 {
            return Err(ChecksumError::BadDigest(t.chars().take(80).collect()));
        }
        let mut out = [0u8; 32];
        let (pairs, _) = bytes.as_chunks::<2>();
        for (i, pair) in pairs.iter().enumerate() {
            match (hex_value(pair[0]), hex_value(pair[1])) {
                (Some(h), Some(l)) => out[i] = (h << 4) | l,
                _ => return Err(ChecksumError::BadDigest(t.to_string())),
            }
        }
        Ok(Sha256Digest(out))
    }

    /// The digest of `bytes`.
    pub fn of_bytes(bytes: &[u8]) -> Self {
        let out = Sha256::digest(bytes);
        let mut a = [0u8; 32];
        a.copy_from_slice(&out);
        Sha256Digest(a)
    }

    /// The digest of everything `reader` yields (streamed; for downloads of any size).
    pub fn of_reader(mut reader: impl Read) -> Result<Self, ChecksumError> {
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(ChecksumError::Read(e.to_string())),
            };
            hasher.update(&buf[..n]);
        }
        let out = hasher.finalize();
        let mut a = [0u8; 32];
        a.copy_from_slice(&out);
        Ok(Sha256Digest(a))
    }
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sha256Digest({self})")
    }
}

impl TryFrom<String> for Sha256Digest {
    type Error = ChecksumError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Sha256Digest::parse_hex(&s)
    }
}

impl From<Sha256Digest> for String {
    fn from(d: Sha256Digest) -> Self {
        d.to_string()
    }
}

/// The checksum of `file_name` in a checksum file: `<hex>  <name>` lines (`sha256sum` format, `*name` for binary
/// mode), or a file that holds only the digest. Blank lines and `#` comments are ignored.
pub fn checksum_for(text: &str, file_name: &str) -> Result<Sha256Digest, ChecksumError> {
    if text.len() > MAX_CHECKSUM_FILE {
        return Err(ChecksumError::TooLarge);
    }
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    if let [only] = lines.as_slice() {
        if only.split_whitespace().count() == 1 {
            return Sha256Digest::parse_hex(only);
        }
    }
    let mut found: Option<Sha256Digest> = None;
    for line in lines {
        let mut parts = line.split_whitespace();
        let (Some(hex), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        let name = name.strip_prefix('*').unwrap_or(name);
        if name != file_name {
            continue;
        }
        let digest = Sha256Digest::parse_hex(hex)?;
        match found {
            Some(prev) if prev != digest => return Err(ChecksumError::Ambiguous(file_name.to_string())),
            _ => found = Some(digest),
        }
    }
    found.ok_or_else(|| ChecksumError::NoEntry(file_name.to_string()))
}

/// The one checksum two independent sources agree on (e.g. `dl.gitea.com` and the gitea.com release): a download
/// is only as trustworthy as the place its checksum comes from, so Pitcrew asks two hosts when it can.
pub fn agreed(file_name: &str, first: Sha256Digest, second: Option<Sha256Digest>) -> Result<Sha256Digest, ChecksumError> {
    match second {
        Some(s) if s != first => Err(ChecksumError::SourcesDisagree {
            file: file_name.to_string(),
            first,
            second: s,
        }),
        _ => Ok(first),
    }
}

/// Checks a download's digest against the published checksum.
pub fn verify(expected: Sha256Digest, actual: Sha256Digest) -> Result<(), ChecksumError> {
    if expected == actual {
        Ok(())
    } else {
        Err(ChecksumError::Mismatch { expected, actual })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // SHA-256 of "abc" (FIPS 180-2 test vector).
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn digests_match_the_standard_test_vector() {
        assert_eq!(Sha256Digest::of_bytes(b"abc").to_string(), ABC);
        assert_eq!(Sha256Digest::of_reader(&b"abc"[..]).unwrap().to_string(), ABC);
        assert_eq!(Sha256Digest::parse_hex(&ABC.to_uppercase()).unwrap().to_string(), ABC);
        assert_eq!(format!("{:?}", Sha256Digest::of_bytes(b"abc")), format!("Sha256Digest({ABC})"));
    }

    #[test]
    fn refuses_malformed_digests() {
        let short = &ABC[..63];
        let not_hex = format!("{}zz", &ABC[..62]);
        let long = format!("{ABC}00");
        for bad in ["", "abc", short, not_hex.as_str(), long.as_str()] {
            assert!(matches!(Sha256Digest::parse_hex(bad), Err(ChecksumError::BadDigest(_))), "{bad}");
        }
        assert!(serde_json::from_str::<Sha256Digest>("\"nope\"").is_err());
        let round: Sha256Digest = serde_json::from_str(&format!("\"{ABC}\"")).unwrap();
        assert_eq!(serde_json::to_string(&round).unwrap(), format!("\"{ABC}\""));
    }

    #[test]
    fn reads_single_and_multi_entry_checksum_files() {
        let name = "gitea-runner-5.2.0-windows-amd64.exe";
        assert_eq!(checksum_for(&format!("{ABC}  {name}\n"), name).unwrap().to_string(), ABC);
        assert_eq!(checksum_for(&format!("{ABC}\n"), name).unwrap().to_string(), ABC);
        let other = "0".repeat(64);
        let list = format!("# release\n{other}  gitea-runner-5.2.0-linux-amd64\n\n{ABC} *{name}\nmalformed\n");
        assert_eq!(checksum_for(&list, name).unwrap().to_string(), ABC);
        assert_eq!(checksum_for(&list, "missing"), Err(ChecksumError::NoEntry("missing".into())));
        let twice = format!("{ABC}  {name}\n{other}  {name}\n");
        assert_eq!(checksum_for(&twice, name), Err(ChecksumError::Ambiguous(name.into())));
        let same_twice = format!("{ABC}  {name}\n{ABC}  {name}\n");
        assert!(checksum_for(&same_twice, name).is_ok());
        assert_eq!(checksum_for(&" ".repeat(MAX_CHECKSUM_FILE + 1), name), Err(ChecksumError::TooLarge));
        assert!(matches!(
            checksum_for(&format!("xyz  {name}\n"), name),
            Err(ChecksumError::BadDigest(_))
        ));
    }

    #[test]
    fn verifies_downloads_and_sources() {
        let good = Sha256Digest::of_bytes(b"abc");
        let bad = Sha256Digest::of_bytes(b"abd");
        assert!(verify(good, good).is_ok());
        let err = verify(good, bad).unwrap_err();
        assert!(err.to_string().contains(ABC), "{err}");
        assert_eq!(agreed("f", good, None), Ok(good));
        assert_eq!(agreed("f", good, Some(good)), Ok(good));
        assert!(matches!(agreed("f", good, Some(bad)), Err(ChecksumError::SourcesDisagree { .. })));
    }

    #[test]
    fn reports_read_errors() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("disk gone"))
            }
        }
        assert!(matches!(Sha256Digest::of_reader(Broken), Err(ChecksumError::Read(_))));
    }
}
