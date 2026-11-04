//! The macOS check of the helper that is about to be copied into `/Library/PrivilegedHelperTools` as root.
//!
//! The helper sits in Dyno's bundle, which its user owns (a drag install), so no ownership rule can vouch for it. The release
//! job signs the final bytes of the bundled helper with the family update key and ships the signature as
//! [`SIGNATURE_FILE`] in the bundle; the elevated install reads the helper once, verifies those bytes with the key compiled
//! into it and writes exactly those bytes. A missing or bad signature, or a build without the key, refuses the install with a
//! next step. What this cannot stop (a replaced helper and a replaced signature plus key before the password prompt) is the
//! accepted risk of ADR 0027's code-signing amendment.

use paddock_service::privileged::SourceVerifier;
use paddock_updater::signature::{UpdateKey, VerifyError, verify_bytes};
use std::path::{Path, PathBuf};

/// The signature file the release job places in the bundle (next to the helper, or in the bundle's `Resources`).
pub const SIGNATURE_FILE: &str = "dyno-helper.minisig";

/// Where the signature of the helper at `source` may be: next to it, then in `../Resources` of an app bundle's `MacOS` folder.
pub fn signature_locations(source: &Path) -> Vec<PathBuf> {
    let mut places = vec![source.with_file_name(SIGNATURE_FILE)];
    let bundle_contents = source
        .parent()
        .filter(|d| d.file_name().is_some_and(|n| n == "MacOS"))
        .and_then(Path::parent);
    if let Some(contents) = bundle_contents {
        places.push(contents.join("Resources").join(SIGNATURE_FILE));
    }
    places
}

/// Checks a helper against the family update key (Tauri's key format, [`UpdateKey`]).
pub struct FamilyKey(Result<UpdateKey, String>);

impl FamilyKey {
    /// The key this build was made with (`TAURI_UPDATER_PUBKEY` at build time, as the app's updater has it). A build without
    /// one cannot tell a genuine helper from a replaced one and refuses every install.
    pub fn compiled_in() -> Self {
        Self::from_text(option_env!("TAURI_UPDATER_PUBKEY"))
    }

    /// A key from its text (tests use a key of their own).
    pub fn from_text(text: Option<&str>) -> Self {
        Self(match UpdateKey::resolve([text]) {
            Some(key) => Ok(key),
            None => Err(
                "this build of Dyno carries no update key, so its helper cannot be checked; install Dyno from the official download".into(),
            ),
        })
    }
}

impl SourceVerifier for FamilyKey {
    fn verify(&self, program: &[u8], signature: &[u8]) -> Result<String, String> {
        let key = self.0.as_ref().map_err(Clone::clone)?;
        let text = std::str::from_utf8(signature)
            .map_err(|_| "the helper's signature file is not text; reinstall Dyno from the official download".to_string())?;
        verify_bytes(key, program, text)
            .map(|comment| format!("valid ({})", comment.chars().take(80).collect::<String>()))
            .map_err(|e| {
                let detail = match e {
                    VerifyError::Invalid(why) => why.to_string(),
                    VerifyError::Io(why) => why,
                    VerifyError::Cancelled => "cancelled".into(),
                };
                format!(
                    "the helper does not match its signature ({detail}), so it may have been replaced; reinstall Dyno from the official download and turn the helper on again"
                )
            })
    }
}
