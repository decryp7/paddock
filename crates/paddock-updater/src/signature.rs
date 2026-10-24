//! The minisign signature check of a downloaded update, against the public key built into the app (Tauri's updater key
//! format: the base64 of the `.pub` file `tauri signer generate` writes).
//!
//! - Only pre-hashed signatures (`ED`, what `tauri signer sign` makes) are accepted, so the file is checked as a stream in
//!   bounded memory; a legacy signature is refused.
//! - The signed version is required: the trusted comment (covered by the global signature) must carry `version:<v>` equal
//!   to the version the release information announced. Without it a tampered `latest.json` could pair a newer version
//!   number with an older, validly signed package (GHSA-j38x-g3m3-95fr; Tauri's `requireSignedVersion`).
//! - The check reads the file once, in [`CHUNK`]-byte pieces, reporting progress and looking at the job's cancel token
//!   between pieces. (minisign hashes with BLAKE2b, which the downloader does not compute while writing; this read is the
//!   one exception to "never re-read a just-downloaded file", and it is what the installer is then handed.)

use crate::error::UpdateError;
use base64::Engine;
use minisign_verify::{PublicKey, Signature};
use paddock_jobs::JobContext;
use std::path::Path;

/// Bytes read per step of the check.
pub const CHUNK: usize = 1 << 20;
/// A public key or signature text longer than this is not one.
const MAX_TEXT: usize = 8192;

/// The app's update public key, parsed once.
#[derive(Clone)]
pub struct UpdateKey(PublicKey);

impl std::fmt::Debug for UpdateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UpdateKey(..)")
    }
}

impl UpdateKey {
    /// Parses Tauri's key format: base64 of `untrusted comment: ...\n<base64 key>\n`.
    pub fn parse(text: &str) -> Result<UpdateKey, String> {
        let file = decode_text(text, "the update public key")?;
        PublicKey::decode(&file)
            .map(UpdateKey)
            .map_err(|e| format!("the update public key is not a minisign public key ({e})"))
    }

    /// The build's key from the first non-empty of `candidates` (the build-time one, then the configured one). `None` when
    /// there is none or it is not a valid key: then updates are not available in this build.
    pub fn resolve<'a>(candidates: impl IntoIterator<Item = Option<&'a str>>) -> Option<UpdateKey> {
        let text = candidates
            .into_iter()
            .flatten()
            .map(str::trim)
            .find(|k| !k.is_empty())?;
        UpdateKey::parse(text).ok()
    }
}

/// Checks `file` against `signature` (base64 of a minisign signature file) and requires it to be signed for `announced`.
/// With `ctx`, the bytes read are added to its current phase and a cancel stops the check (`Err(Busy)` is never returned
/// for a cancel: the caller looks at the token).
pub fn verify_file(
    key: &UpdateKey,
    file: &Path,
    signature: &str,
    announced: &semver::Version,
    ctx: Option<&JobContext>,
) -> Result<(), VerifyError> {
    let invalid = |detail: String| VerifyError::Invalid(UpdateError::SignatureInvalid { detail });
    let text = decode_text(signature, "the signature").map_err(invalid)?;
    let sig = Signature::decode(&text)
        .map_err(|e| invalid(format!("the signature cannot be read ({e})")))?;
    // `verify_stream` refuses only another key and a legacy signature.
    let mut verifier = key.0.verify_stream(&sig).map_err(|e| {
        invalid(match e {
            minisign_verify::Error::UnexpectedKeyId => {
                "the update was signed with another key than this app's".to_string()
            }
            _ => "legacy (not pre-hashed) signatures are not accepted".to_string(),
        })
    })?;
    let mut feed = Feed {
        verifier: &mut verifier,
        ctx,
    };
    let copied = std::fs::File::open(file)
        .and_then(|f| std::io::copy(&mut std::io::BufReader::with_capacity(CHUNK, f), &mut feed))
        .and_then(|_| std::io::Write::flush(&mut feed));
    if let Err(e) = copied {
        let cancelled = ctx.is_some_and(|c| c.check().is_err());
        return Err(if cancelled {
            VerifyError::Cancelled
        } else {
            VerifyError::Io(e.to_string())
        });
    }
    verifier
        .finalize()
        .map_err(|_| invalid("the file does not match its signature".into()))?;
    // Only now is the trusted comment trustworthy: the global signature checked above covers it.
    check_signed_version(sig.trusted_comment(), announced).map_err(invalid)
}

/// Hands the file's bytes to the verifier, counting them in the job's phase; a cancel ends the copy.
struct Feed<'a, 'k> {
    verifier: &'a mut minisign_verify::StreamVerifier<'k>,
    ctx: Option<&'a JobContext>,
}

impl std::io::Write for Feed<'_, '_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Some(ctx) = self.ctx {
            ctx.check()
                .map_err(|_| std::io::Error::other("cancelled"))?;
            ctx.add(buf.len() as u64);
        }
        self.verifier.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Why [`verify_file`] did not pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    /// The signature is not valid for this file, key or version ([`UpdateError::SignatureInvalid`]).
    Invalid(UpdateError),
    /// The file could not be read.
    Io(String),
    /// The job was cancelled.
    Cancelled,
}

impl VerifyError {
    /// The update error to show (a cancel is not one: callers handle it first, and it maps to `Busy`).
    pub fn into_update_error(self) -> UpdateError {
        match self {
            VerifyError::Invalid(e) => e,
            VerifyError::Io(detail) => UpdateError::Storage { detail },
            VerifyError::Cancelled => UpdateError::Busy {
                detail: "The signature check was cancelled.".into(),
            },
        }
    }
}

/// The `version:` field of a Tauri trusted comment (`timestamp:1\tfile:app.tar.gz\tversion:1.2.3`) must name `announced`
/// (compared as semantic versions, so `v1.2.3` matches).
pub fn check_signed_version(
    trusted_comment: &str,
    announced: &semver::Version,
) -> Result<(), String> {
    let signed = trusted_comment
        .split('\t')
        .find_map(|field| field.strip_prefix("version:"))
        .ok_or("the signature does not name the version it was made for")?;
    match semver::Version::parse(signed.trim().trim_start_matches('v')) {
        Ok(v) if v == *announced => Ok(()),
        _ => Err(format!(
            "the signature was made for version {}, but the release information announces {announced}",
            signed.chars().take(40).collect::<String>()
        )),
    }
}

fn decode_text(b64: &str, what: &str) -> Result<String, String> {
    let b64 = b64.trim();
    if b64.is_empty() || b64.len() > MAX_TEXT {
        return Err(format!("{what} is empty or too long"));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|_| format!("{what} is not base64"))?;
    String::from_utf8(bytes).map_err(|_| format!("{what} is not text"))
}
