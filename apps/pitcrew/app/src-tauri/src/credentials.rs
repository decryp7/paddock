//! Secrets in the OS credential store (ADR 0005): Windows Credential Manager, macOS Keychain (Secret Service on
//! Linux). Never in Pitcrew's files, never in logs.
//!
//! - The Gitea **read token** per server (`Pitcrew Gitea API` / `<server URL>`), used by the app for job details and
//!   outcomes.
//! - The **registration token** per server, only when the user ticks "remember" (`Pitcrew registration` /
//!   `<server URL>`), to register further runners without copying it again.
//!
//! The store is `paddock-secrets`' [`Vault`] port (ADR 0077 WP-4): the app passes [`OsVault`], tests a `MemoryVault`, so a
//! test never reaches the real credential store. The entries keep the service and account names Pitcrew 1.0 wrote.

use paddock_secrets::{OsVault, Secret, Vault};

/// The service name of read tokens.
pub const API_SERVICE: &str = "Pitcrew Gitea API";
/// The service name of remembered registration tokens.
pub const REGISTRATION_SERVICE: &str = "Pitcrew registration";
/// Longest token stored.
pub const MAX_TOKEN_LEN: usize = 256;

/// Normalises a server URL as the credential's account name.
pub fn account(server: &str) -> Result<String, String> {
    pitcrew_core::config::check_server_url(server)
}

/// Stores a token in the OS credential store.
pub fn set(service: &str, server: &str, token: &str) -> Result<(), String> {
    set_in(&OsVault, service, server, token)
}

/// Reads a token from the OS credential store (`None` when there is none).
pub fn get(service: &str, server: &str) -> Option<String> {
    get_in(&OsVault, service, server)
}

/// Deletes a token from the OS credential store (deleting a missing one succeeds).
pub fn clear(service: &str, server: &str) -> Result<(), String> {
    clear_in(&OsVault, service, server)
}

/// Stores a token in `vault` under `service` / the normalised server URL (the entries Pitcrew 1.0 wrote).
pub fn set_in(vault: &dyn Vault, service: &str, server: &str, token: &str) -> Result<(), String> {
    let t = token.trim();
    if t.is_empty() || t.len() > MAX_TOKEN_LEN || !t.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(format!("the token must be 1-{MAX_TOKEN_LEN} characters without spaces"));
    }
    let value = Secret::new(t).map_err(|e| e.to_string())?;
    vault
        .set(service, &account(server)?, &value)
        .map_err(|e| format!("the token could not be stored: {e}"))
}

/// Reads a token from `vault` (`None` when there is none or the store cannot be read).
pub fn get_in(vault: &dyn Vault, service: &str, server: &str) -> Option<String> {
    let acc = account(server).ok()?;
    vault.get(service, &acc).ok().flatten().map(|s| s.expose().to_string())
}

/// Deletes a token from `vault` (deleting a missing one succeeds).
pub fn clear_in(vault: &dyn Vault, service: &str, server: &str) -> Result<(), String> {
    vault
        .delete(service, &account(server)?)
        .map_err(|e| format!("the token could not be deleted: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_secrets::MemoryVault;

    #[test]
    fn tokens_and_servers_are_checked_before_the_store_is_touched() {
        let v = MemoryVault::default();
        v.set_unavailable(true);
        assert!(set_in(&v, API_SERVICE, "https://git.example.com", "").is_err());
        assert!(set_in(&v, API_SERVICE, "https://git.example.com", "has space").is_err());
        assert!(set_in(&v, API_SERVICE, "ftp://x", "tok").is_err());
        assert!(clear_in(&v, API_SERVICE, "not a url").is_err());
        assert_eq!(get_in(&v, API_SERVICE, "not a url"), None);
        assert_eq!(account("https://git.example.com/").unwrap(), "https://git.example.com");
    }

    /// ADR 0077 WP-4: the entries stay where Pitcrew 1.0 wrote them, `<service>` / `<server URL>`, so a stored token is read
    /// after the update.
    #[test]
    fn tokens_keep_the_entries_pitcrew_wrote() {
        let v = MemoryVault::default();
        set_in(&v, API_SERVICE, "https://git.example.com/", " tok_1 ").unwrap();
        assert_eq!(
            v.get("Pitcrew Gitea API", "https://git.example.com")
                .unwrap()
                .map(|s| s.expose().to_string()),
            Some("tok_1".to_string()),
            "trimmed, under the normalised URL"
        );
        v.set("Pitcrew registration", "https://git.example.com", &Secret::new("reg_1").unwrap())
            .unwrap();
        assert_eq!(
            get_in(&v, REGISTRATION_SERVICE, "https://git.example.com"),
            Some("reg_1".to_string())
        );
        assert_eq!(get_in(&v, API_SERVICE, "https://other.example.com"), None);
        clear_in(&v, API_SERVICE, "https://git.example.com").unwrap();
        clear_in(&v, API_SERVICE, "https://git.example.com").unwrap();
        assert_eq!(get_in(&v, API_SERVICE, "https://git.example.com"), None);
    }

    #[test]
    fn an_unavailable_store_is_a_message_and_no_token() {
        let v = MemoryVault::default();
        v.set_unavailable(true);
        let e = set_in(&v, API_SERVICE, "https://git.example.com", "zq9").unwrap_err();
        assert!(e.starts_with("the token could not be stored: ") && !e.contains("zq9"), "{e}");
        assert_eq!(get_in(&v, API_SERVICE, "https://git.example.com"), None);
        assert!(clear_in(&v, API_SERVICE, "https://git.example.com").is_err());
    }
}
