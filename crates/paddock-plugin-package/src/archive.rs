//! Reading, verifying, building and extracting `.paddockplugin` files.
//!
//! A package is a zip archive with exactly these entries (directories `bin/` and `bin/<target>/` may also appear):
//!
//! ```text
//! manifest.toml                         the manifest (crate::manifest)
//! bin/<target>/<binary>[.exe]           one program per target the manifest lists
//! SHA256SUMS                            "<sha256>  <path>" for manifest.toml and every program, sorted by path
//! signature                             optional: key, public key and Ed25519 signature of the digest
//! ```
//!
//! Reading is bounded and never trusts the archive ([`Limits`]): the file size, the number of entries, each entry's size
//! (counted while reading, not taken from the header) and the total are capped; names must be exactly the listed ones (no
//! traversal, links, encrypted or duplicate entries), and each entry is hashed as it streams. Every hash must equal the
//! one in `SHA256SUMS`, which must list exactly the manifest and the programs. The **digest** is the SHA-256 of a canonical
//! text with the hash of the manifest and the hash of the `SHA256SUMS` bytes ([`Package::digest`]); the signature covers
//! that digest, so changing any byte of the manifest, the sums or a program breaks it. After installing,
//! [`verify_installed`] detects a program that changed on disk.

use crate::PackageError;
use crate::keys::{PublicKey, SecretKey, from_hex, to_hex};
use crate::manifest::{MAX_MANIFEST_BYTES, Manifest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Seek, Write};
use std::path::Path;

/// Largest package file (256 MiB).
pub const MAX_PACKAGE_BYTES: u64 = 256 << 20;
/// Largest program in a package (128 MiB).
pub const MAX_BINARY_BYTES: u64 = 128 << 20;
/// Largest total of all entries, uncompressed (512 MiB).
pub const MAX_TOTAL_BYTES: u64 = 512 << 20;
/// Largest `signature` file.
pub const MAX_SIGNATURE_BYTES: u64 = 4096;
/// Largest `SHA256SUMS` file.
pub const MAX_SUMS_BYTES: u64 = 16 * 1024;
/// Most entries (files and directories).
pub const MAX_ENTRIES: usize = 64;
/// The manifest's entry name.
pub const MANIFEST: &str = "manifest.toml";
/// The checksum list's entry name.
pub const SUMS: &str = "SHA256SUMS";
/// The signature's entry name.
pub const SIGNATURE: &str = "signature";
/// First line of the canonical digest text; its version changes with the digest's definition.
const DIGEST_HEADER: &str = "paddock-plugin-package-v1\n";

/// The size limits applied while reading a package ([`Limits::default`] is what the constants say).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest package file.
    pub package: u64,
    /// Largest program.
    pub binary: u64,
    /// Largest total of all entries, uncompressed.
    pub total: u64,
    /// Most entries.
    pub entries: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            package: MAX_PACKAGE_BYTES,
            binary: MAX_BINARY_BYTES,
            total: MAX_TOTAL_BYTES,
            entries: MAX_ENTRIES,
        }
    }
}

/// The `signature` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignatureFile {
    /// Format of this file (1).
    pub format: u32,
    /// The signing key's id (must equal the manifest's `plugin.key`).
    pub key: String,
    /// The signing public key, text form ([`PublicKey::to_text`]).
    pub public_key: String,
    /// The Ed25519 signature of the digest, hex.
    pub signature: String,
}

/// One checksummed file of a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its name in the archive.
    pub path: String,
    /// Its size in bytes.
    pub size: u64,
    /// Its SHA-256.
    pub sha256: [u8; 32],
}

impl Entry {
    /// The SHA-256 as hexadecimal.
    pub fn sha256_hex(&self) -> String {
        to_hex(&self.sha256)
    }
}

/// Whether and by whom a package is signed (its signature is valid; a broken one is [`PackageError::Tampered`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signature {
    /// No signature.
    Unsigned,
    /// A valid signature by `key`.
    Signed {
        /// The signing key.
        key: PublicKey,
    },
}

/// A read and verified package (the programs stay in the file; [`Package::extract`] copies one out).
#[derive(Debug, Clone)]
pub struct Package {
    /// The manifest.
    pub manifest: Manifest,
    /// The checksummed files, by name (the manifest and each program).
    pub entries: BTreeMap<String, Entry>,
    /// The signature, verified against [`Package::digest`].
    pub signature: Signature,
    digest: [u8; 32],
}

fn not_a_package(e: impl std::fmt::Display) -> PackageError {
    PackageError::NotAPackage(e.to_string())
}

fn tampered(m: &str) -> PackageError {
    PackageError::Tampered(m.to_string())
}

/// Reads `r` to its end through SHA-256, refusing more than `limit` bytes; `sink` receives the bytes.
fn hash_bounded(
    mut r: impl Read,
    limit: u64,
    what: &str,
    mut sink: impl FnMut(&[u8]) -> std::io::Result<()>,
) -> Result<(u64, [u8; 32]), PackageError> {
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = r
            .read(&mut buf)
            .map_err(|e| not_a_package(format!("{what}: {e}")))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > limit {
            return Err(PackageError::TooLarge {
                what: what.into(),
                limit,
            });
        }
        hasher.update(&buf[..n]);
        sink(&buf[..n])?;
    }
    Ok((total, hasher.finalize().into()))
}

/// The `SHA256SUMS` text for `entries`: `<sha256>  <path>` lines sorted by path.
pub fn sums_text<'a>(entries: impl IntoIterator<Item = &'a Entry>) -> String {
    let mut sorted: Vec<&Entry> = entries.into_iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    sorted
        .iter()
        .map(|e| format!("{}  {}\n", e.sha256_hex(), e.path))
        .collect()
}

fn parse_sums(text: &str) -> Option<BTreeMap<String, [u8; 32]>> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let (hash, path) = line.split_once("  ")?;
        if hash.len() != 64 || hash.bytes().any(|b| b.is_ascii_uppercase()) {
            return None;
        }
        if out
            .insert(path.to_string(), from_hex::<32>(hash)?)
            .is_some()
        {
            return None;
        }
    }
    Some(out)
}

/// The canonical digest the signature covers: the hash of the manifest bytes and of the `SHA256SUMS` bytes.
pub fn digest_of(manifest: &[u8], sums: &[u8]) -> [u8; 32] {
    let text = format!(
        "{DIGEST_HEADER}manifest {}\nsums {}\n",
        to_hex(&Sha256::digest(manifest)),
        to_hex(&Sha256::digest(sums))
    );
    Sha256::digest(text.as_bytes()).into()
}

/// Whether `name` is a directory entry the layout allows (`bin/`, `bin/<target>/`).
fn allowed_dir(name: &str, m: &Manifest) -> bool {
    name == "bin/" || m.targets.iter().any(|t| name == format!("bin/{t}/"))
}

impl Package {
    /// Reads and verifies the package at `path` with the default [`Limits`]: layout, limits, manifest, checksums and
    /// signature (a signature or checksum that does not match is [`PackageError::Tampered`]). Whether the signer is trusted
    /// is [`crate::trust`]'s question.
    pub fn open(path: &Path) -> Result<Package, PackageError> {
        Package::open_with(path, &Limits::default())
    }

    /// [`Package::open`] with other limits.
    pub fn open_with(path: &Path, limits: &Limits) -> Result<Package, PackageError> {
        let file = std::fs::File::open(path)
            .map_err(|e| PackageError::Io(format!("cannot open {}: {e}", path.display())))?;
        let len = file.metadata()?.len();
        if len > limits.package {
            return Err(PackageError::TooLarge {
                what: "the package".into(),
                limit: limits.package,
            });
        }
        Package::read_with(std::io::BufReader::new(file), limits)
    }

    /// [`Package::open`] for a package in memory or any seekable reader.
    pub fn read(r: impl Read + Seek) -> Result<Package, PackageError> {
        Package::read_with(r, &Limits::default())
    }

    /// [`Package::read`] with other limits.
    pub fn read_with(r: impl Read + Seek, limits: &Limits) -> Result<Package, PackageError> {
        let mut zip = zip::ZipArchive::new(r).map_err(not_a_package)?;
        if zip.len() > limits.entries {
            return Err(PackageError::TooLarge {
                what: format!("the package's list of {} entries", zip.len()),
                limit: limits.entries as u64,
            });
        }
        // The manifest first: it says which other entries may exist.
        let manifest_bytes = {
            let mut e = zip.by_name(MANIFEST).map_err(|e| match e {
                zip::result::ZipError::FileNotFound => PackageError::MissingFile(MANIFEST.into()),
                other => not_a_package(other),
            })?;
            let mut bytes = Vec::new();
            hash_bounded(&mut e, MAX_MANIFEST_BYTES, MANIFEST, |b| {
                bytes.extend_from_slice(b);
                Ok(())
            })?;
            bytes
        };
        let manifest = Manifest::parse(
            std::str::from_utf8(&manifest_bytes)
                .map_err(|_| PackageError::InvalidManifest("not UTF-8 text".into()))?,
        )?;
        let programs: Vec<String> = manifest
            .targets
            .iter()
            .map(|t| manifest.binary_path(t))
            .collect();
        let mut entries: BTreeMap<String, Entry> = BTreeMap::new();
        let (mut sums_bytes, mut signature_bytes) = (None, None);
        let mut total = 0u64;
        for i in 0..zip.len() {
            let mut e = zip.by_index(i).map_err(not_a_package)?;
            let name = e.name().map_err(not_a_package)?.into_owned();
            if e.is_dir() {
                if allowed_dir(&name, &manifest) {
                    continue;
                }
                return Err(PackageError::UnlistedFile(name));
            }
            if e.encrypted() || e.is_symlink() {
                return Err(PackageError::UnsafeEntry(name));
            }
            let limit = if name == MANIFEST {
                MAX_MANIFEST_BYTES
            } else if name == SIGNATURE {
                MAX_SIGNATURE_BYTES
            } else if name == SUMS {
                MAX_SUMS_BYTES
            } else if programs.contains(&name) {
                limits.binary
            } else {
                return Err(
                    if name.contains("..")
                        || name.starts_with('/')
                        || name.contains('\\')
                        || name.contains(':')
                    {
                        PackageError::UnsafeEntry(name)
                    } else {
                        PackageError::UnlistedFile(name)
                    },
                );
            };
            let mut kept = Vec::new();
            let keep = name == SIGNATURE || name == SUMS;
            let (size, sha256) = hash_bounded(
                &mut e,
                limit.min(limits.total.saturating_sub(total)),
                &name,
                |b| {
                    if keep {
                        kept.extend_from_slice(b);
                    }
                    Ok(())
                },
            )?;
            total += size;
            match name.as_str() {
                SIGNATURE => signature_bytes = Some(kept),
                SUMS => sums_bytes = Some(kept),
                _ => {
                    entries.insert(
                        name.clone(),
                        Entry {
                            path: name,
                            size,
                            sha256,
                        },
                    );
                }
            }
        }
        for p in &programs {
            if !entries.contains_key(p) {
                return Err(PackageError::MissingFile(p.clone()));
            }
        }
        let sums_bytes = sums_bytes.ok_or_else(|| PackageError::MissingFile(SUMS.into()))?;
        check_sums(&entries, &sums_bytes)?;
        let digest = digest_of(&manifest_bytes, &sums_bytes);
        let signature_text = signature_bytes
            .map(|b| String::from_utf8(b).map_err(|_| tampered("the signature is not text")))
            .transpose()?;
        let signature = verify(&manifest, signature_text.as_deref(), &digest)?;
        Ok(Package {
            manifest,
            entries,
            signature,
            digest,
        })
    }

    /// The canonical digest the signature covers, hex (also what the user approved: install compares it again).
    pub fn digest(&self) -> String {
        to_hex(&self.digest)
    }

    /// The program for `target`, when the package has one.
    pub fn program(&self, target: &str) -> Option<&Entry> {
        self.entries.get(&self.manifest.binary_path(target))
    }

    /// Copies the program for `target` out of the package at `path` into `dest`, checking that it is still the program
    /// [`Package::open`] verified (the file may have changed since). Returns the program's SHA-256, which the host records to
    /// check the installed file later with [`verify_installed`].
    pub fn extract(
        &self,
        path: &Path,
        target: &str,
        dest: &Path,
    ) -> Result<[u8; 32], PackageError> {
        let Some(expected) = self.program(target).cloned() else {
            return Err(PackageError::WrongTarget {
                target: target.into(),
                supported: self.manifest.targets.join(", "),
            });
        };
        let file = std::fs::File::open(path)
            .map_err(|e| PackageError::Io(format!("cannot open {}: {e}", path.display())))?;
        let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(not_a_package)?;
        let entry = zip
            .by_name(&expected.path)
            .map_err(|_| PackageError::MissingFile(expected.path.clone()))?;
        let mut out = std::fs::File::create(dest)
            .map_err(|e| PackageError::Io(format!("cannot write {}: {e}", dest.display())))?;
        let result = hash_bounded(entry, expected.size.max(1), &expected.path, |b| {
            out.write_all(b)
        });
        let finished = result.and_then(|r| out.sync_all().map(|()| r).map_err(PackageError::from));
        drop(out);
        match finished {
            Ok((size, sha256)) if size == expected.size && sha256 == expected.sha256 => {
                // The program runs: on Unix it needs its execute bit (only for the user; Windows has none).
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o700))?;
                }
                Ok(sha256)
            }
            other => {
                let _ = std::fs::remove_file(dest);
                match other {
                    Err(e) => Err(e),
                    Ok(_) => Err(PackageError::Tampered(format!(
                        "{} changed while it was being installed",
                        expected.path
                    ))),
                }
            }
        }
    }
}

/// Whether the installed program at `installed` is still the one that was verified and installed: its SHA-256 must equal
/// `expected` (what [`Package::extract`] returned, or [`Entry::sha256`]). A changed or replaced binary is
/// [`PackageError::Tampered`], which the host treats as a quarantine-grade fault; a deleted one is
/// [`PackageError::MissingFile`]. Bounded: a file larger than [`MAX_BINARY_BYTES`] is refused without reading it all.
pub fn verify_installed(installed: &Path, expected: &[u8; 32]) -> Result<(), PackageError> {
    verify_installed_with(installed, expected, MAX_BINARY_BYTES)
}

fn verify_installed_with(
    installed: &Path,
    expected: &[u8; 32],
    max: u64,
) -> Result<(), PackageError> {
    let file = match std::fs::File::open(installed) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(PackageError::MissingFile(installed.display().to_string()));
        }
        Err(e) => {
            return Err(PackageError::Io(format!(
                "cannot open {}: {e}",
                installed.display()
            )));
        }
    };
    let (_, sha256) = hash_bounded(file, max, &installed.display().to_string(), |_| Ok(()))
        .map_err(|e| match e {
            PackageError::TooLarge { .. } => tampered(&format!(
                "{} grew beyond the size of any plugin program",
                installed.display()
            )),
            other => other,
        })?;
    if &sha256 == expected {
        Ok(())
    } else {
        Err(PackageError::Tampered(format!(
            "{} is not the program that was installed: it changed after installation",
            installed.display()
        )))
    }
}

/// `SHA256SUMS` must list exactly the checksummed files, with the hashes just computed.
fn check_sums(entries: &BTreeMap<String, Entry>, sums: &[u8]) -> Result<(), PackageError> {
    let listed = std::str::from_utf8(sums)
        .ok()
        .and_then(parse_sums)
        .ok_or_else(|| tampered("SHA256SUMS is not a list of \"<sha256>  <path>\" lines"))?;
    if listed.len() != entries.len()
        || entries
            .iter()
            .any(|(p, e)| listed.get(p) != Some(&e.sha256))
    {
        return Err(PackageError::Tampered(
            "a file does not match SHA256SUMS (the manifest or a program was changed)".into(),
        ));
    }
    Ok(())
}

/// Checks the signature file against the manifest and the digest.
fn verify(
    manifest: &Manifest,
    signature: Option<&str>,
    digest: &[u8; 32],
) -> Result<Signature, PackageError> {
    let (text, key_id) = match (signature, &manifest.key) {
        (None, None) => return Ok(Signature::Unsigned),
        (None, Some(_)) => {
            return Err(tampered(
                "the manifest names a signing key but the package has no signature",
            ));
        }
        (Some(_), None) => {
            return Err(tampered(
                "the package has a signature but its manifest names no signing key",
            ));
        }
        (Some(t), Some(k)) => (t, k),
    };
    let file: SignatureFile =
        toml::from_str(text).map_err(|_| tampered("the signature file is not valid"))?;
    if file.format != 1 {
        return Err(tampered("the signature file has an unknown format"));
    }
    let key = PublicKey::parse(&file.public_key)
        .map_err(|_| tampered("the signature's public key is not valid"))?;
    if key.id() != file.key || &file.key != key_id {
        return Err(tampered(
            "the signature's key is not the key the manifest names",
        ));
    }
    let sig =
        from_hex::<64>(&file.signature).ok_or_else(|| tampered("the signature is not valid"))?;
    if !key.verifies(digest, &sig) {
        return Err(tampered("the signature does not match the contents"));
    }
    Ok(Signature::Signed { key })
}

/// What [`build`] packs.
#[derive(Debug, Clone)]
pub struct Build<'a> {
    /// The manifest; its `key` is set from `sign` (or cleared without it).
    pub manifest: Manifest,
    /// The program for each target the manifest lists.
    pub programs: Vec<(String, &'a Path)>,
    /// Sign with this key.
    pub sign: Option<&'a SecretKey>,
}

/// Writes a package to `out`: the manifest, one program per target, `SHA256SUMS` and, with a key, the signature. The archive
/// is deterministic (fixed times, sorted entries). Returns the package's digest (hex).
pub fn build(b: Build, out: &Path) -> Result<String, PackageError> {
    build_with(b, out, MAX_BINARY_BYTES)
}

fn build_with(b: Build, out: &Path, max_binary: u64) -> Result<String, PackageError> {
    let mut manifest = b.manifest;
    manifest.key = b.sign.map(|k| k.public().id());
    let text = manifest.to_toml();
    // The result must read back: validate what is written.
    let manifest = Manifest::parse(&text)?;
    let manifest_bytes = text.into_bytes();
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for t in &manifest.targets {
        let Some((_, path)) = b.programs.iter().find(|(target, _)| target == t) else {
            return Err(PackageError::MissingFile(format!("the program for {t}")));
        };
        let len = std::fs::metadata(path)
            .map_err(|e| PackageError::Io(format!("{}: {e}", path.display())))?
            .len();
        if len > max_binary {
            return Err(PackageError::TooLarge {
                what: path.display().to_string(),
                limit: max_binary,
            });
        }
        let bytes = std::fs::read(path)
            .map_err(|e| PackageError::Io(format!("{}: {e}", path.display())))?;
        files.insert(manifest.binary_path(t), bytes);
    }
    if let Some((t, _)) = b
        .programs
        .iter()
        .find(|(t, _)| !manifest.targets.contains(t))
    {
        return Err(PackageError::UnlistedFile(format!(
            "a program for {t}, which the manifest does not list"
        )));
    }
    files.insert(MANIFEST.into(), manifest_bytes.clone());
    let entries: Vec<Entry> = files
        .iter()
        .map(|(path, bytes)| Entry {
            path: path.clone(),
            size: bytes.len() as u64,
            sha256: Sha256::digest(bytes).into(),
        })
        .collect();
    let sums = sums_text(&entries).into_bytes();
    let digest = digest_of(&manifest_bytes, &sums);
    files.insert(SUMS.into(), sums);
    if let Some(key) = b.sign {
        let sig = SignatureFile {
            format: 1,
            key: key.public().id(),
            public_key: key.public().to_text(),
            signature: to_hex(&key.sign(&digest)),
        };
        files.insert(
            SIGNATURE.into(),
            toml::to_string(&sig).unwrap_or_default().into_bytes(),
        );
    }
    let tmp = out.with_extension("paddockplugin.part");
    let written = (|| -> Result<(), PackageError> {
        let file = std::fs::File::create(&tmp)
            .map_err(|e| PackageError::Io(format!("cannot write {}: {e}", tmp.display())))?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default())
            .unix_permissions(0o755);
        for (name, bytes) in &files {
            zip.start_file(name.as_str(), opts)
                .map_err(|e| PackageError::Io(e.to_string()))?;
            zip.write_all(bytes)?;
        }
        zip.finish()
            .map_err(|e| PackageError::Io(e.to_string()))?
            .sync_all()?;
        Ok(())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, out)
        .map_err(|e| PackageError::Io(format!("cannot write {}: {e}", out.display())))?;
    Ok(to_hex(&digest))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::manifest::tests::SAMPLE;

    pub(crate) const WIN: &str = "x86_64-pc-windows-msvc";
    pub(crate) const LINUX: &str = "x86_64-unknown-linux-gnu";

    /// A package of the sample manifest with tiny programs, written to a temp folder.
    pub(crate) fn sample(dir: &Path, key: Option<&SecretKey>) -> std::path::PathBuf {
        let win = dir.join("kv-lines.exe");
        let lin = dir.join("kv-lines");
        std::fs::write(&win, b"MZ windows program").unwrap();
        std::fs::write(&lin, b"\x7fELF linux program").unwrap();
        let out = dir.join("kv.paddockplugin");
        build(
            Build {
                manifest: Manifest::parse(SAMPLE).unwrap(),
                programs: vec![(WIN.into(), &win), (LINUX.into(), &lin)],
                sign: key,
            },
            &out,
        )
        .unwrap();
        out
    }

    /// Rewrites the zip at `path` with `edit` applied to its (name, bytes) list.
    pub(crate) fn rewrite(path: &Path, edit: impl FnOnce(&mut Vec<(String, Vec<u8>)>)) {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
        let mut files = Vec::new();
        for i in 0..zip.len() {
            let mut e = zip.by_index(i).unwrap();
            let mut b = Vec::new();
            e.read_to_end(&mut b).unwrap();
            files.push((e.name().unwrap().into_owned(), b));
        }
        drop(zip);
        edit(&mut files);
        let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (n, b) in files {
            w.start_file(n, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(&b).unwrap();
        }
        w.finish().unwrap();
    }

    type Files = Vec<(String, Vec<u8>)>;

    fn file<'a>(f: &'a mut [(String, Vec<u8>)], name: &str) -> &'a mut Vec<u8> {
        &mut f.iter_mut().find(|(n, _)| n == name).unwrap().1
    }

    #[test]
    fn a_signed_package_builds_reads_back_and_verifies() {
        let t = tempfile::tempdir().unwrap();
        let key = SecretKey::generate().unwrap();
        let p = sample(t.path(), Some(&key));
        let pkg = Package::open(&p).unwrap();
        assert_eq!(pkg.signature, Signature::Signed { key: key.public() });
        assert_eq!(
            pkg.manifest.key.as_deref(),
            Some(key.public().id().as_str())
        );
        assert_eq!(pkg.entries.len(), 3, "{:?}", pkg.entries.keys());
        assert_eq!(pkg.digest().len(), 64);
        let dest = t.path().join("out.exe");
        let sha = pkg.extract(&p, WIN, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"MZ windows program");
        assert_eq!(sha, pkg.program(WIN).unwrap().sha256);
        assert_eq!(pkg.program(WIN).unwrap().sha256_hex().len(), 64);
        // Regression (Linux CI): installed programs had no execute bit and failed with "Permission denied".
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        assert_eq!(
            pkg.extract(&p, "aarch64-apple-darwin", &dest)
                .unwrap_err()
                .code(),
            "wrongTarget"
        );
        // Deterministic: the same inputs give the same bytes.
        let first = std::fs::read(&p).unwrap();
        let again = sample(t.path(), Some(&key));
        assert_eq!(std::fs::read(again).unwrap(), first);
        // The sums file is sha256sum style and lists the manifest and both programs.
        let z = std::fs::read(&p).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(z)).unwrap();
        let mut sums = String::new();
        zip.by_name(SUMS)
            .unwrap()
            .read_to_string(&mut sums)
            .unwrap();
        assert_eq!(sums.lines().count(), 3);
        assert!(
            sums.contains("  manifest.toml\n")
                && sums.contains(&format!("  bin/{LINUX}/kv-lines\n")),
            "{sums}"
        );
    }

    #[test]
    fn an_unsigned_package_is_unsigned() {
        let t = tempfile::tempdir().unwrap();
        let pkg = Package::open(&sample(t.path(), None)).unwrap();
        assert_eq!(pkg.signature, Signature::Unsigned);
        assert_eq!(pkg.manifest.key, None);
    }

    #[test]
    fn a_tampered_binary_fails_verification() {
        let t = tempfile::tempdir().unwrap();
        let key = SecretKey::generate().unwrap();
        let p = sample(t.path(), Some(&key));
        rewrite(&p, |f| file(f, &format!("bin/{WIN}/kv-lines.exe")).push(0));
        let e = Package::open(&p).unwrap_err();
        assert_eq!(e.code(), "tampered", "{e}");
        assert!(e.to_string().contains("SHA256SUMS"), "{e}");
        // The attacker also fixes the sums: the signature over them no longer matches.
        let p = sample(t.path(), Some(&key));
        rewrite(&p, |f| {
            file(f, &format!("bin/{LINUX}/kv-lines")).push(0);
            let entries: Vec<Entry> = f
                .iter()
                .filter(|(n, _)| n != SUMS && n != SIGNATURE)
                .map(|(n, b)| Entry {
                    path: n.clone(),
                    size: b.len() as u64,
                    sha256: Sha256::digest(b).into(),
                })
                .collect();
            *file(f, SUMS) = sums_text(&entries).into_bytes();
        });
        let e = Package::open(&p).unwrap_err();
        assert_eq!(e.code(), "tampered", "{e}");
        assert!(e.to_string().contains("signature does not match"), "{e}");
    }

    #[test]
    fn tampering_with_the_manifest_the_sums_or_the_signature_is_detected() {
        let t = tempfile::tempdir().unwrap();
        let key = SecretKey::generate().unwrap();
        let tamper = |edit: &dyn Fn(&mut Files), says: &str| {
            let p = sample(t.path(), Some(&key));
            rewrite(&p, edit);
            let e = Package::open(&p).unwrap_err();
            assert_eq!(e.code(), "tampered", "{e}");
            assert!(e.to_string().contains(says), "{e}");
        };
        tamper(
            &|f| {
                let m = file(f, MANIFEST);
                *m = String::from_utf8(m.clone())
                    .unwrap()
                    .replace("1.2.0", "9.9.9")
                    .into_bytes();
            },
            "SHA256SUMS",
        );
        // Removing the signature but keeping the manifest's key.
        tamper(&|f| f.retain(|(n, _)| n != SIGNATURE), "no signature");
        // A signature by another key, with the manifest pointing at it: still the wrong signature.
        tamper(
            &|f| {
                let s = file(f, SIGNATURE);
                let mut sf: SignatureFile =
                    toml::from_str(std::str::from_utf8(s).unwrap()).unwrap();
                sf.public_key = SecretKey::generate().unwrap().public().to_text();
                *s = toml::to_string(&sf).unwrap().into_bytes();
            },
            "not the key the manifest names",
        );
        // The key names match but the signature bytes are another key's.
        tamper(
            &|f| {
                let s = file(f, SIGNATURE);
                let mut sf: SignatureFile =
                    toml::from_str(std::str::from_utf8(s).unwrap()).unwrap();
                sf.signature = to_hex(&SecretKey::generate().unwrap().sign(b"x"));
                *s = toml::to_string(&sf).unwrap().into_bytes();
            },
            "signature does not match",
        );
        let edit_sig = |edit: fn(&mut SignatureFile)| {
            move |f: &mut Vec<(String, Vec<u8>)>| {
                let s = file(f, SIGNATURE);
                let mut sf: SignatureFile =
                    toml::from_str(std::str::from_utf8(s).unwrap()).unwrap();
                edit(&mut sf);
                *s = toml::to_string(&sf).unwrap().into_bytes();
            }
        };
        tamper(&edit_sig(|s| s.format = 2), "unknown format");
        tamper(
            &edit_sig(|s| s.public_key = "junk".into()),
            "public key is not valid",
        );
        tamper(
            &edit_sig(|s| s.signature = "00".into()),
            "signature is not valid",
        );
        tamper(
            &|f| *file(f, SIGNATURE) = b"not = [toml".to_vec(),
            "signature file is not valid",
        );
        tamper(&|f| *file(f, SIGNATURE) = vec![0xff, 0xfe], "not text");
        // A signature with an unsigned manifest.
        let p = sample(t.path(), None);
        let signed = sample(&t.path().join("..").canonicalize().unwrap(), Some(&key));
        let mut sig = Vec::new();
        zip::ZipArchive::new(std::fs::File::open(&signed).unwrap())
            .unwrap()
            .by_name(SIGNATURE)
            .unwrap()
            .read_to_end(&mut sig)
            .unwrap();
        rewrite(&p, |f| f.push((SIGNATURE.into(), sig)));
        assert!(
            Package::open(&p)
                .unwrap_err()
                .to_string()
                .contains("names no signing key")
        );
    }

    #[test]
    fn bad_sums_files_are_refused() {
        let t = tempfile::tempdir().unwrap();
        for (edit, says) in [
            (
                Box::new(|s: &str| s.replace("  ", " ")) as Box<dyn Fn(&str) -> String>,
                "not a list",
            ),
            (Box::new(|s: &str| format!("{s}{s}")), "not a list"),
            (Box::new(|s: &str| s.to_uppercase()), "not a list"),
            (
                Box::new(|s: &str| s.lines().take(1).collect::<Vec<_>>().join("\n") + "\n"),
                "does not match",
            ),
            (
                Box::new(|s: &str| {
                    format!("{s}{} extra.txt\n", "0".repeat(64)).replace(" extra", "  extra")
                }),
                "does not match",
            ),
        ] {
            let p = sample(t.path(), None);
            rewrite(&p, |f| {
                let s = file(f, SUMS);
                *s = edit(std::str::from_utf8(s).unwrap()).into_bytes();
            });
            let e = Package::open(&p).unwrap_err();
            assert_eq!(e.code(), "tampered", "{e}");
            assert!(e.to_string().contains(says), "{says}: {e}");
        }
        let p = sample(t.path(), None);
        rewrite(&p, |f| *file(f, SUMS) = vec![0xff]);
        assert_eq!(Package::open(&p).unwrap_err().code(), "tampered");
        let p = sample(t.path(), None);
        rewrite(&p, |f| f.retain(|(n, _)| n != SUMS));
        assert_eq!(Package::open(&p).unwrap_err().code(), "missingFile");
    }

    #[test]
    fn layouts_that_are_not_allowed_are_refused() {
        let t = tempfile::tempdir().unwrap();
        type Edit = Box<dyn Fn(&mut Vec<(String, Vec<u8>)>)>;
        let cases: Vec<(Edit, &str)> = vec![
            (
                Box::new(|f| f.push(("extra.txt".into(), b"x".to_vec()))),
                "unlistedFile",
            ),
            // A traversal entry is refused, however it is spelled.
            (
                Box::new(|f| f.push(("../evil.exe".into(), b"x".to_vec()))),
                "unsafeEntry",
            ),
            (
                Box::new(|f| f.push(("bin/../../evil.exe".into(), b"x".to_vec()))),
                "unsafeEntry",
            ),
            (
                Box::new(|f| f.push(("/etc/evil".into(), b"x".to_vec()))),
                "unsafeEntry",
            ),
            (
                Box::new(|f| f.push(("bin\\evil.exe".into(), b"x".to_vec()))),
                "unsafeEntry",
            ),
            (
                Box::new(|f| f.push(("C:evil.exe".into(), b"x".to_vec()))),
                "unsafeEntry",
            ),
            (
                Box::new(|f| f.push((format!("bin/{WIN}/other.exe"), b"x".to_vec()))),
                "unlistedFile",
            ),
            (
                Box::new(|f| f.retain(|(n, _)| !n.ends_with("kv-lines"))),
                "missingFile",
            ),
            (
                Box::new(|f| f.retain(|(n, _)| n != MANIFEST)),
                "missingFile",
            ),
            (
                Box::new(|f| *file(f, MANIFEST) = vec![0xff, 0xfe]),
                "invalidManifest",
            ),
        ];
        for (edit, code) in cases {
            let p = sample(t.path(), None);
            rewrite(&p, edit);
            let e = Package::open(&p).unwrap_err();
            assert_eq!(e.code(), code, "{e}");
        }
        std::fs::write(t.path().join("no.paddockplugin"), b"not a zip").unwrap();
        assert_eq!(
            Package::open(&t.path().join("no.paddockplugin"))
                .unwrap_err()
                .code(),
            "notAPackage"
        );
        assert_eq!(
            Package::open(&t.path().join("missing.paddockplugin"))
                .unwrap_err()
                .code(),
            "io"
        );
    }

    /// A zip built by hand, with entries `Package::read` must refuse.
    fn raw_zip(build: impl FnOnce(&mut zip::ZipWriter<std::io::Cursor<Vec<u8>>>)) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        build(&mut w);
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn links_directories_and_encrypted_entries_are_refused() {
        let opts = zip::write::SimpleFileOptions::default();
        let manifest = Manifest::parse(SAMPLE).unwrap().to_toml();
        let with_manifest = |w: &mut zip::ZipWriter<std::io::Cursor<Vec<u8>>>| {
            w.start_file(MANIFEST, opts).unwrap();
            w.write_all(manifest.as_bytes()).unwrap();
        };
        // A symlink in place of a program.
        let bytes = raw_zip(|w| {
            with_manifest(w);
            w.add_symlink(format!("bin/{LINUX}/kv-lines"), "/etc/passwd", opts)
                .unwrap();
        });
        assert_eq!(
            Package::read(std::io::Cursor::new(bytes))
                .unwrap_err()
                .code(),
            "unsafeEntry"
        );
        // A directory the layout does not allow, and the ones it does.
        let bytes = raw_zip(|w| {
            with_manifest(w);
            w.add_directory("etc/", opts).unwrap();
        });
        assert_eq!(
            Package::read(std::io::Cursor::new(bytes))
                .unwrap_err()
                .code(),
            "unlistedFile"
        );
        let bytes = raw_zip(|w| {
            with_manifest(w);
            w.add_directory("bin/", opts).unwrap();
            w.add_directory(format!("bin/{WIN}/"), opts).unwrap();
        });
        assert_eq!(
            Package::read(std::io::Cursor::new(bytes))
                .unwrap_err()
                .code(),
            "missingFile",
            "directories are allowed, the programs are missing"
        );
        // A stored entry flagged encrypted (general purpose bit 0): refused before it is read.
        let mut bytes = raw_zip(with_manifest);
        let flag = bytes.windows(4).position(|w| w == b"PK\x03\x04").unwrap() + 6;
        bytes[flag] |= 1;
        let central = bytes.windows(4).position(|w| w == b"PK\x01\x02").unwrap() + 8;
        bytes[central] |= 1;
        let e = Package::read(std::io::Cursor::new(bytes)).unwrap_err();
        assert_eq!(e.code(), "notAPackage", "{e}");
        assert!(
            e.to_string().contains("Password"),
            "an encrypted entry is refused: {e}"
        );
    }

    #[test]
    fn a_duplicate_entry_name_cannot_hide_a_second_file() {
        let opts = zip::write::SimpleFileOptions::default();
        let mut bytes = raw_zip(|w| {
            w.start_file(MANIFEST, opts).unwrap();
            w.write_all(Manifest::parse(SAMPLE).unwrap().to_toml().as_bytes())
                .unwrap();
            w.start_file("signaturf", opts).unwrap();
            w.write_all(b"a").unwrap();
            w.start_file("signature", opts).unwrap();
            w.write_all(b"b").unwrap();
        });
        // The writer refuses two entries of one name, so make the second name equal the first in the headers.
        let mut at = 0;
        while let Some(i) = bytes[at..].windows(9).position(|w| w == b"signaturf") {
            bytes[at + i + 8] = b'e';
            at += i + 9;
        }
        // The zip reader keeps one entry per name, so reading by index and extracting by name see the same file (if a future
        // version listed both, `Package::read` would hash the one it visits last and extraction would re-verify it).
        let z = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
        assert_eq!(z.len(), 2, "{:?}", z.file_names().collect::<Vec<_>>());
        assert!(Package::read(std::io::Cursor::new(bytes)).is_err());
    }

    #[test]
    fn limits_bound_the_package_its_entries_and_its_programs() {
        let t = tempfile::tempdir().unwrap();
        let p = sample(t.path(), None);
        assert_eq!(
            Package::open_with(&p, &Limits::default())
                .unwrap()
                .entries
                .len(),
            3
        );
        let size = std::fs::metadata(&p).unwrap().len();
        let tight = |f: &dyn Fn(&mut Limits)| {
            let mut l = Limits::default();
            f(&mut l);
            Package::open_with(&p, &l).unwrap_err()
        };
        assert_eq!(tight(&|l| l.package = size - 1).code(), "tooLarge");
        assert_eq!(tight(&|l| l.entries = 3).code(), "tooLarge");
        let e = tight(&|l| l.binary = 4);
        assert!(
            matches!(&e, PackageError::TooLarge { what, limit: 4 } if what.starts_with("bin/")),
            "{e}"
        );
        assert_eq!(
            tight(&|l| l.total = 10).code(),
            "tooLarge",
            "the total of all entries is bounded"
        );
        // The size is counted while reading: a header that lies about the size does not help (the reader stops at the limit).
        let big = t.path().join("big.exe");
        std::fs::write(&big, vec![0u8; 200_000]).unwrap();
        let out = t.path().join("big.paddockplugin");
        let m = Manifest::parse(SAMPLE).unwrap();
        build(
            Build {
                manifest: m,
                programs: vec![(WIN.into(), &big), (LINUX.into(), &big)],
                sign: None,
            },
            &out,
        )
        .unwrap();
        assert!(
            std::fs::metadata(&out).unwrap().len() < 20_000,
            "zeros compress"
        );
        let e = Package::open_with(
            &out,
            &Limits {
                binary: 100_000,
                ..Limits::default()
            },
        )
        .unwrap_err();
        assert_eq!(e.code(), "tooLarge", "{e}");
    }

    #[test]
    fn an_oversized_manifest_is_refused_before_it_is_parsed() {
        let t = tempfile::tempdir().unwrap();
        let p = sample(t.path(), None);
        rewrite(&p, |f| {
            let m = file(f, MANIFEST);
            m.extend(std::iter::repeat_n(b'#', MAX_MANIFEST_BYTES as usize));
        });
        let e = Package::open(&p).unwrap_err();
        assert_eq!(e.code(), "tooLarge", "{e}");
        assert!(e.to_string().contains("manifest.toml"));
    }

    #[test]
    fn building_needs_every_listed_program_and_nothing_else() {
        let t = tempfile::tempdir().unwrap();
        let prog = t.path().join("p");
        std::fs::write(&prog, b"x").unwrap();
        let m = Manifest::parse(SAMPLE).unwrap();
        let attempt = |programs: Vec<(String, &Path)>, name: &str| {
            build(
                Build {
                    manifest: m.clone(),
                    programs,
                    sign: None,
                },
                &t.path().join(name),
            )
        };
        assert_eq!(
            attempt(vec![(WIN.into(), &prog)], "a.paddockplugin")
                .unwrap_err()
                .code(),
            "missingFile"
        );
        let extra = attempt(
            vec![
                (WIN.into(), &prog),
                (LINUX.into(), &prog),
                ("aarch64-apple-darwin".into(), &prog),
            ],
            "b.paddockplugin",
        );
        assert_eq!(extra.unwrap_err().code(), "unlistedFile");
        let gone = t.path().join("gone");
        assert_eq!(
            attempt(
                vec![(WIN.into(), &gone), (LINUX.into(), &prog)],
                "c.paddockplugin"
            )
            .unwrap_err()
            .code(),
            "io"
        );
        // Unwritable output: a typed error and no half-written file.
        let into_file = t.path().join("p").join("x.paddockplugin");
        assert_eq!(
            attempt(
                vec![(WIN.into(), &prog), (LINUX.into(), &prog)],
                into_file.to_str().unwrap()
            )
            .unwrap_err()
            .code(),
            "io"
        );
        // A program above the size limit is refused before it is read.
        let big = build_with(
            Build {
                manifest: m.clone(),
                programs: vec![(WIN.into(), &prog), (LINUX.into(), &prog)],
                sign: None,
            },
            &t.path().join("e.paddockplugin"),
            0,
        );
        assert_eq!(big.unwrap_err().code(), "tooLarge");
        // An invalid manifest never reaches the archive.
        let mut bad = m.clone();
        bad.binary = "../x".into();
        let e = build(
            Build {
                manifest: bad,
                programs: vec![],
                sign: None,
            },
            &t.path().join("d.paddockplugin"),
        );
        assert_eq!(e.unwrap_err().code(), "invalidManifest");
    }

    #[test]
    fn a_changed_binary_after_install_is_detected() {
        let t = tempfile::tempdir().unwrap();
        let p = sample(t.path(), Some(&SecretKey::generate().unwrap()));
        let pkg = Package::open(&p).unwrap();
        let dest = t.path().join("installed.exe");
        let sha = pkg.extract(&p, WIN, &dest).unwrap();
        verify_installed(&dest, &sha).unwrap();
        // One byte appended, one byte flipped, the file replaced: each is tampering.
        std::fs::OpenOptions::new()
            .append(true)
            .open(&dest)
            .unwrap()
            .write_all(b"!")
            .unwrap();
        let e = verify_installed(&dest, &sha).unwrap_err();
        assert_eq!(e.code(), "tampered", "{e}");
        assert!(e.to_string().contains("changed after installation"), "{e}");
        pkg.extract(&p, WIN, &dest).unwrap();
        let mut bytes = std::fs::read(&dest).unwrap();
        bytes[0] ^= 1;
        std::fs::write(&dest, bytes).unwrap();
        assert_eq!(
            verify_installed(&dest, &sha).unwrap_err().code(),
            "tampered"
        );
        std::fs::remove_file(&dest).unwrap();
        assert_eq!(
            verify_installed(&dest, &sha).unwrap_err().code(),
            "missingFile"
        );
        // A folder where the program was: a typed error.
        std::fs::create_dir(&dest).unwrap();
        assert!(verify_installed(&dest, &sha).is_err());
        std::fs::remove_dir(&dest).unwrap();
        // A file larger than any program is tampering, and is not read to its end.
        std::fs::write(&dest, vec![0u8; 100]).unwrap();
        let e = verify_installed_with(&dest, &sha, 50).unwrap_err();
        assert_eq!(e.code(), "tampered", "{e}");
        assert!(e.to_string().contains("grew beyond"), "{e}");
        // A program that cannot be opened (no read permission) is an I/O error, not a verdict.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o000)).unwrap();
            // (running as root can still read it)
            if std::fs::File::open(&dest).is_err() {
                assert_eq!(verify_installed(&dest, &sha).unwrap_err().code(), "io");
            }
        }
    }

    #[test]
    fn extract_notices_a_package_that_changed_since_it_was_opened() {
        let t = tempfile::tempdir().unwrap();
        let exe = format!("bin/{WIN}/kv-lines.exe");
        let dest = t.path().join("out.exe");
        let changed = |content: &[u8]| {
            let p = sample(t.path(), None);
            let pkg = Package::open(&p).unwrap();
            rewrite(&p, |f| *file(f, &exe) = content.to_vec());
            (p, pkg)
        };
        // Shorter, longer and same-size-other-bytes programs: never installed, nothing left behind.
        for (content, code) in [
            (&b"MZ evil"[..], "tampered"),
            (&b"MZ windows program but much longer"[..], "tooLarge"),
            (&b"MZ windows prograM"[..], "tampered"),
        ] {
            let (p, pkg) = changed(content);
            let e = pkg.extract(&p, WIN, &dest).unwrap_err();
            assert_eq!(e.code(), code, "{e}");
            assert!(!dest.exists(), "nothing is left behind");
        }
        // The entry was removed, the package vanished, or the destination cannot be written.
        let (p, pkg) = changed(b"x");
        rewrite(&p, |f| f.retain(|(n, _)| n != &exe));
        assert_eq!(
            pkg.extract(&p, WIN, &dest).unwrap_err().code(),
            "missingFile"
        );
        assert_eq!(
            pkg.extract(&t.path().join("none"), WIN, &dest)
                .unwrap_err()
                .code(),
            "io"
        );
        let p = sample(t.path(), None);
        let pkg = Package::open(&p).unwrap();
        assert_eq!(
            pkg.extract(&p, WIN, &t.path().join("no").join("dir"))
                .unwrap_err()
                .code(),
            "io"
        );
    }
}
