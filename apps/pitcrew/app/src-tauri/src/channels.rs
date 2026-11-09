//! Notification channels (webhook, ntfy, email) around the shareable `notify-channels` module (ADR 0017): the settings
//! file, the OS keychain as the secret store, and the commands. Off by default; secrets only in the keychain.
//!
//! The app sends: it holds the keychain access (the service never sees a secret) and is running whenever the tray is.
//! A runner box with no app running gets OS notifications only; that limit is in the ADR.

use notify_channels::{Dispatcher, Message, SecretStore, SendError, Settings, Severity};
use paddock_secrets::{OsVault, Secret, Vault};
use pitcrew_core::notify::{Notification, Severity as PcSeverity};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The keychain service name of channel secrets.
pub const SECRET_SERVICE: &str = "Pitcrew notification channel";
/// The settings file.
pub const FILE: &str = "notification-channels.json";
/// Largest settings file read.
const MAX_BYTES: u64 = 256 * 1024;
/// Longest secret stored.
pub const MAX_SECRET_LEN: usize = 512;

#[derive(Serialize, Deserialize)]
struct Envelope {
    version: u32,
    settings: Settings,
}

/// Loads the settings: a missing, damaged, oversized or newer file gives the defaults (no channels).
pub fn load(dir: &Path) -> Settings {
    let path = dir.join(FILE);
    let Ok(meta) = std::fs::metadata(&path) else {
        return Settings::default();
    };
    if meta.len() > MAX_BYTES {
        return Settings::default();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<Envelope>(&t).ok())
        .filter(|e| e.version == 1)
        .map(|e| e.settings)
        .unwrap_or_default()
}

/// Validates and saves the settings atomically.
pub fn save(dir: &Path, settings: &Settings) -> Result<(), String> {
    settings.validate()?;
    std::fs::create_dir_all(dir).map_err(|e| format!("the settings folder could not be made: {e}"))?;
    let text = serde_json::to_string_pretty(&Envelope {
        version: 1,
        settings: settings.clone(),
    })
    .map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{FILE}.tmp"));
    std::fs::write(&tmp, text).map_err(|e| format!("the settings could not be saved: {e}"))?;
    std::fs::rename(&tmp, dir.join(FILE)).map_err(|e| format!("the settings could not be saved: {e}"))
}

/// Channel secrets in a credential store (`Pitcrew notification channel` / `<key>`, the entries Pitcrew 1.0 wrote): the OS
/// one in the app ([`Keychain`]), a `MemoryVault` in tests (ADR 0077 WP-4).
pub struct ChannelSecrets<'a>(pub &'a dyn Vault);

impl SecretStore for ChannelSecrets<'_> {
    fn get(&self, key: &str) -> Option<String> {
        check_key(key).ok()?;
        self.0.get(SECRET_SERVICE, key).ok().flatten().map(|s| s.expose().to_string())
    }
}

impl ChannelSecrets<'_> {
    /// Stores a secret.
    pub fn set(&self, key: &str, value: &str) -> Result<(), String> {
        check_key(key)?;
        if value.is_empty() || value.len() > MAX_SECRET_LEN || value.contains(['\r', '\n']) {
            return Err(format!("the secret must be 1-{MAX_SECRET_LEN} characters on one line"));
        }
        let value = Secret::new(value).map_err(|e| e.to_string())?;
        self.0
            .set(SECRET_SERVICE, key, &value)
            .map_err(|e| format!("the secret could not be stored: {e}"))
    }

    /// Deletes a secret (deleting a missing one succeeds).
    pub fn clear(&self, key: &str) -> Result<(), String> {
        check_key(key)?;
        self.0
            .delete(SECRET_SERVICE, key)
            .map_err(|e| format!("the secret could not be deleted: {e}"))
    }

    /// Whether a secret is stored (never reveals it).
    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
}

/// The OS credential store as a [`SecretStore`].
pub struct Keychain;

impl SecretStore for Keychain {
    fn get(&self, key: &str) -> Option<String> {
        ChannelSecrets(&OsVault).get(key)
    }
}

/// Stores a secret in the OS credential store.
pub fn set_secret(key: &str, value: &str) -> Result<(), String> {
    ChannelSecrets(&OsVault).set(key, value)
}

/// Deletes a secret from the OS credential store (deleting a missing one succeeds).
pub fn clear_secret(key: &str) -> Result<(), String> {
    ChannelSecrets(&OsVault).clear(key)
}

/// Whether a secret is stored in the OS credential store (never reveals it).
pub fn has_secret(key: &str) -> bool {
    ChannelSecrets(&OsVault).has(key)
}

fn check_key(key: &str) -> Result<(), String> {
    if key.is_empty()
        || key.len() > 64
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
    {
        return Err("the secret's name must be 1-64 letters, digits, dashes, dots or underscores".into());
    }
    Ok(())
}

/// The channel message for a Pitcrew notification: `kind` is the notification kind's id as the UI knows it.
pub fn message_for(n: &Notification) -> Message {
    let kind = serde_json::to_value(n.kind)
        .ok()
        .and_then(|v| v.as_str().map(ToString::to_string))
        .unwrap_or_default();
    Message {
        title: n.title.clone(),
        body: n.body.clone(),
        severity: match n.severity {
            PcSeverity::Info => Severity::Info,
            PcSeverity::Warning => Severity::Warning,
            PcSeverity::Error => Severity::Error,
        },
        kind,
        source: n.runner.to_string(),
    }
}

/// Sends `n` to the channels that want it, on a background thread. Failures are logged without any content.
pub fn dispatch_in_background(dir: std::path::PathBuf, n: &Notification) {
    let settings = load(&dir);
    if settings.channels.iter().all(|c| !c.enabled) {
        return;
    }
    let msg = message_for(n);
    std::thread::spawn(move || {
        for (id, result) in Dispatcher::new(settings).dispatch(&msg, &Keychain) {
            if let Err(e) = result {
                crate::applog::write(&format!("notification channel {id}: {}", describe(&e)));
            }
        }
    });
}

/// A failure in words (never includes secrets or message text).
pub fn describe(e: &SendError) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify_channels::{ChannelConfig, Transport};
    use pitcrew_core::names::RunnerName;
    use pitcrew_core::notify::NotificationKind;
    use pitcrew_core::time::Timestamp;

    #[allow(
        clippy::disallowed_methods,
        reason = "a test builds its scratch path from the OS temp folder and removes it"
    )]
    fn temp(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("pitcrew-it-channels-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    fn webhook() -> ChannelConfig {
        ChannelConfig {
            id: "hook1".into(),
            name: "Team hook".into(),
            enabled: true,
            events: vec!["diskLow".into()],
            min_severity: Severity::Info,
            transport: Transport::Webhook {
                url: "https://example.org/hook".into(),
                secret_key: None,
            },
        }
    }

    #[test]
    fn settings_round_trip_and_damage_gives_the_defaults() {
        let d = temp("rt");
        assert!(load(&d).channels.is_empty(), "off by default");
        let s = Settings { channels: vec![webhook()] };
        save(&d, &s).unwrap();
        assert_eq!(load(&d), s);
        std::fs::write(d.join(FILE), "{broken").unwrap();
        assert!(load(&d).channels.is_empty());
        std::fs::write(d.join(FILE), r#"{"version":9,"settings":{"channels":[]}}"#).unwrap();
        assert!(load(&d).channels.is_empty());
        std::fs::write(d.join(FILE), vec![b' '; (MAX_BYTES + 1) as usize]).unwrap();
        assert!(load(&d).channels.is_empty());
        let mut bad = webhook();
        bad.transport = Transport::Webhook {
            url: "ftp://x".into(),
            secret_key: None,
        };
        assert!(
            save(&d, &Settings { channels: vec![bad] }).is_err(),
            "invalid settings are not saved"
        );
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn notifications_become_messages_with_the_kind_id_the_ui_uses() {
        let n = Notification {
            kind: NotificationKind::DiskLow,
            severity: PcSeverity::Warning,
            runner: RunnerName::parse("mac").unwrap(),
            title: "t".into(),
            body: "b".into(),
            at: Timestamp(1),
        };
        let m = message_for(&n);
        assert_eq!(
            (m.kind.as_str(), m.severity, m.source.as_str()),
            ("diskLow", Severity::Warning, "mac")
        );
        let held = Notification {
            kind: NotificationKind::Held,
            ..n
        };
        assert_eq!(message_for(&held).kind, "held");
    }

    #[test]
    fn secret_names_and_values_are_checked_before_the_keychain_is_touched() {
        let v = paddock_secrets::MemoryVault::default();
        v.set_unavailable(true);
        let c = ChannelSecrets(&v);
        assert!(c.set("", "x").is_err());
        assert!(c.set("bad name", "x").is_err());
        assert!(c.set("ok-name", "").is_err());
        assert!(c.set("ok-name", "two\nlines").is_err());
        assert!(c.set("ok-name", &"x".repeat(MAX_SECRET_LEN + 1)).is_err());
        assert!(c.clear("../x").is_err());
        assert!(!c.has("bad name"));
    }

    /// ADR 0077 WP-4: channel secrets stay where Pitcrew 1.0 wrote them, `Pitcrew notification channel` / `<key>`, so a
    /// stored one is read after the update; a store that cannot be read gives no secret and a message, never the value.
    #[test]
    fn channel_secrets_keep_the_entries_pitcrew_wrote() {
        let v = paddock_secrets::MemoryVault::default();
        let c = ChannelSecrets(&v);
        c.set("hook1.secret", "s3cr").unwrap();
        assert_eq!(
            v.get("Pitcrew notification channel", "hook1.secret")
                .unwrap()
                .map(|s| s.expose().to_string()),
            Some("s3cr".to_string())
        );
        assert!(c.has("hook1.secret"));
        assert_eq!(SecretStore::get(&c, "hook1.secret").as_deref(), Some("s3cr"));
        c.clear("hook1.secret").unwrap();
        c.clear("hook1.secret").unwrap();
        assert!(!c.has("hook1.secret"));
        v.set_unavailable(true);
        let e = c.set("hook1.secret", "s3cr").unwrap_err();
        assert!(e.starts_with("the secret could not be stored: ") && !e.contains("s3cr"), "{e}");
        assert!(!c.has("hook1.secret"));
        assert!(c.clear("hook1.secret").is_err());
    }
}
