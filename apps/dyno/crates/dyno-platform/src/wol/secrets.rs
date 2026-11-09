//! Where a Wake-on-LAN computer's SecureOn password is kept (ADR 0032 D6, amended 2026-11-05).
//!
//! On macOS an owner-only file in the agent's data folder, never the Keychain: the Keychain binds an item to the build that made it,
//! so after an update every read waits behind a password dialog with no time limit (measured: still blocked after 15 minutes in a
//! background agent, so a scheduled wake could wait for nobody). On Windows and Linux the system's store has no such binding and
//! stays. The Keychain adapter is not even compiled on macOS (`dyno-platform`'s `Cargo.toml` enables `paddock-secrets`'s `keyring`
//! feature only for other targets), so no macOS code path can construct it.

#[cfg(target_os = "macos")]
use paddock_secrets::FileSecrets;
use paddock_secrets::Secrets;
use std::path::Path;
use std::sync::Arc;

/// The password store of the real machine: the file store in `dir` on macOS.
#[cfg(target_os = "macos")]
pub fn os_store(dir: &Path) -> Arc<dyn Secrets> {
    // Owner-only files (0600 on Unix, written through a temporary file and renamed), one per password; a file others can read is
    // refused on read. The folder is created on the first write.
    Arc::new(FileSecrets::new(dir))
}

/// The password store of the real machine: the system's credential store (`dir` is not used).
#[cfg(not(target_os = "macos"))]
pub fn os_store(dir: &Path) -> Arc<dyn Secrets> {
    let _ = dir;
    Arc::new(paddock_secrets::KeyringSecrets::os("Dyno"))
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use super::*;
    use paddock_secrets::{FileSecrets, Name, Secret, Secrets};

    #[test]
    fn the_password_store_is_an_owner_only_file_in_the_folder_it_is_given() {
        let dir = tempfile::tempdir().unwrap();
        // The agent's data folder is the user's own; a temp folder on a CI runner may let others create files (run 7821).
        paddock_secrets::make_folder_private(dir.path()).unwrap();
        let store = FileSecrets::new(dir.path().join("secrets"));
        let name = Name::new("wol", "secure-on-7").unwrap();
        store.set(&name, &Secret::new("0a:0b:0c:0d:0e:0f").unwrap()).unwrap();
        assert_eq!(store.get(&name).unwrap().unwrap().expose(), "0a:0b:0c:0d:0e:0f");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let files: Vec<_> = std::fs::read_dir(dir.path().join("secrets"))
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            assert_eq!(files.len(), 1);
            assert_eq!(std::fs::metadata(&files[0]).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn on_macos_the_os_choice_is_the_file_store_never_the_keychain() {
        let dir = tempfile::tempdir().unwrap();
        let store = os_store(&dir.path().join("secrets"));
        let name = Name::new("wol", "secure-on-8").unwrap();
        store.set(&name, &Secret::new("0a:0b:0c:0d:0e:0f").unwrap()).unwrap();
        assert!(
            std::fs::read_dir(dir.path().join("secrets")).unwrap().next().is_some(),
            "the value went to a file"
        );
    }
}
