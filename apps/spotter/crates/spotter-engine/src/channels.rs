//! The engine's operations for the e-mail channel: the mail password in the credential store, its status and the
//! test message. Sending itself is [`crate::mail`]; the settings (server, port, sender) live in the configuration.
use crate::engine::Engine;
use crate::error::{EngineError, Result};
use crate::mail::{self, MailAccount, MailMessage, PASSWORD_KEY, PASSWORD_SCOPE};
use spotter_core::notify::is_email_address;

/// Whether e-mail can be sent, for the settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MailStatus {
    /// A mail server is set up.
    pub configured: bool,
    /// The credential store holds a password for the account.
    pub password_set: bool,
}

impl Engine {
    /// Whether a mail server and a password are set up. The password itself is never returned.
    pub fn mail_status(&self) -> MailStatus {
        MailStatus {
            configured: self.settings().smtp.is_some(),
            password_set: self.inner.svc.secrets.get(PASSWORD_SCOPE, PASSWORD_KEY).is_some(),
        }
    }

    /// Stores the mail password in the credential store (`None` or an empty text removes it).
    pub fn set_mail_password(&self, password: Option<&str>) -> Result<()> {
        let store = &self.inner.svc.secrets;
        let outcome = match password.filter(|p| !p.is_empty()) {
            Some(p) => store.set(PASSWORD_SCOPE, PASSWORD_KEY, p),
            None => store.delete(PASSWORD_SCOPE, PASSWORD_KEY),
        };
        outcome.map_err(|e| {
            EngineError::Unavailable(format!(
                "The mail password could not be saved: {e}. Check the system's credential store."
            ))
        })
    }

    /// The configured mail server with its password, when there is a server.
    pub(crate) fn mail_account(&self) -> Option<MailAccount> {
        let settings = self.settings().smtp?;
        let password = settings
            .account()
            .and_then(|_| self.inner.svc.secrets.get(PASSWORD_SCOPE, PASSWORD_KEY));
        Some(MailAccount { settings, password })
    }

    /// Sends a test message to `to` through the saved mail settings; the error says what to check.
    pub async fn send_test_email(&self, to: &str) -> Result<()> {
        if !is_email_address(to) {
            return Err(EngineError::BadRequest("Enter an e-mail address like name@example.org.".into()));
        }
        let Some(account) = self.mail_account() else {
            return Err(EngineError::BadRequest(
                "Set up the mail server first (server, port and sender), then send the test.".into(),
            ));
        };
        let message = MailMessage {
            to: to.trim().to_string(),
            subject: "Spotter test message".into(),
            body: "This is a test message from Spotter. If you can read it, e-mail notifications work.\n\n-- \nSent by Spotter".into(),
        };
        mail::send(&account, &message)
            .await
            .map_err(|e| EngineError::Unavailable(e.message()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{EngineConfig, Services};
    use crate::secrets::MemorySecrets;
    use spotter_core::notify::{SmtpSecurity, SmtpSettings};
    use std::sync::Arc;

    fn engine(dir: &std::path::Path) -> Engine {
        Engine::open(EngineConfig::new(dir, "test"), Services::new(Arc::new(MemorySecrets::new()))).unwrap()
    }

    #[tokio::test]
    async fn the_test_message_needs_an_address_a_server_and_a_reachable_one() {
        let dir = tempfile::tempdir().unwrap();
        let engine = engine(dir.path());
        assert_eq!(
            engine.mail_status(),
            MailStatus {
                configured: false,
                password_set: false
            }
        );
        assert_eq!(engine.send_test_email("nobody").await.unwrap_err().code(), "badRequest");
        assert_eq!(engine.send_test_email("me@example.test").await.unwrap_err().code(), "badRequest");

        let mut settings = engine.settings();
        settings.smtp = Some(SmtpSettings {
            host: "127.0.0.1".into(),
            port: 1,
            security: SmtpSecurity::None,
            username: None,
            from: "spotter@example.test".into(),
        });
        engine.save_settings(settings).unwrap();
        assert!(engine.mail_status().configured);
        let e = engine.send_test_email("me@example.test").await.unwrap_err();
        assert_eq!(e.code(), "unavailable");
        assert!(e.to_string().contains("could not be reached"), "{e}");

        engine.set_mail_password(Some("pw")).unwrap();
        assert!(engine.mail_status().password_set);
        assert!(
            engine.mail_account().unwrap().password.is_none(),
            "no account name: the password is not used"
        );
        engine.set_mail_password(None).unwrap();
    }
}
