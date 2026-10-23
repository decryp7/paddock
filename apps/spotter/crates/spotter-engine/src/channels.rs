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
