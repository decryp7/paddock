//! Secrets in the OS credential store (ADR 0005): Windows Credential Manager, macOS Keychain (Secret Service on
//! Linux). Never in Pitcrew's files, never in logs.
//!
//! - The Gitea **read token** per server (`Pitcrew Gitea API` / `<server URL>`), used by the app for job details and
//!   outcomes.
//! - The **registration token** per server, only when the user ticks "remember" (`Pitcrew registration` /
//!   `<server URL>`), to register further runners without copying it again.

/// The service name of read tokens.
pub const API_SERVICE: &str = "Pitcrew Gitea API";
/// The service name of remembered registration tokens.
pub const REGISTRATION_SERVICE: &str = "Pitcrew registration";
/// Longest token stored.
pub const MAX_TOKEN_LEN: usize = 256;

fn entry(service: &str, server: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(service, server).map_err(|e| format!("the credential store is not available: {e}"))
}

/// Normalises a server URL as the credential's account name.
pub fn account(server: &str) -> Result<String, String> {
    pitcrew_core::config::check_server_url(server)
}

/// Stores a token.
pub fn set(service: &str, server: &str, token: &str) -> Result<(), String> {
    let t = token.trim();
    if t.is_empty() || t.len() > MAX_TOKEN_LEN || !t.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(format!("the token must be 1-{MAX_TOKEN_LEN} characters without spaces"));
    }
    entry(service, &account(server)?)?
        .set_password(t)
        .map_err(|e| format!("the token could not be stored: {e}"))
}

/// Reads a token (`None` when there is none).
pub fn get(service: &str, server: &str) -> Option<String> {
    let acc = account(server).ok()?;
    entry(service, &acc).ok()?.get_password().ok()
}

/// Deletes a token (deleting a missing one succeeds).
pub fn clear(service: &str, server: &str) -> Result<(), String> {
    match entry(service, &account(server)?)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("the token could not be deleted: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_and_servers_are_checked_before_the_store_is_touched() {
        assert!(set(API_SERVICE, "https://git.example.com", "").is_err());
        assert!(set(API_SERVICE, "https://git.example.com", "has space").is_err());
        assert!(set(API_SERVICE, "ftp://x", "tok").is_err());
        assert!(clear(API_SERVICE, "not a url").is_err());
        assert_eq!(get(API_SERVICE, "not a url"), None);
        assert_eq!(account("https://git.example.com/").unwrap(), "https://git.example.com");
    }
}
