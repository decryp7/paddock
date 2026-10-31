//! The port to the mail server: one message to its recipients. [`LettreSender`] (feature `smtp`) is the real one.

use crate::config::Security;
use paddock_secrets::Secret;
use std::fmt;
use std::time::Duration;

/// The account to sign in with. `Debug` hides the password.
#[derive(Clone)]
pub struct Login {
    /// The account name.
    pub username: String,
    /// The password from the credential store.
    pub password: Secret,
}

impl fmt::Debug for Login {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Login({}, ***)", self.username)
    }
}

/// One e-mail to send.
#[derive(Debug, Clone)]
pub struct MailRequest {
    /// SMTP host.
    pub host: String,
    /// SMTP port.
    pub port: u16,
    /// Connection security.
    pub security: Security,
    /// The account, if the relay needs one.
    pub login: Option<Login>,
    /// Sender address.
    pub from: String,
    /// Recipient addresses (at least one).
    pub to: Vec<String>,
    /// One line, already bounded.
    pub subject: String,
    /// Plain text, already bounded.
    pub body: String,
    /// Longest one step of the conversation (connect, each command) may take.
    pub step_timeout: Duration,
    /// The channel is on the user's home network (`ChannelConfig::allow_lan`): the server may be a private address. When false,
    /// a private address must be refused.
    pub lan: bool,
}

/// The mail port. Contract: deliver to every recipient or fail; map failures to [`SendError`](crate::SendError) with
/// `Connect`, `Tls`, `Auth`, `Timeout`, `MailRefused` (5xx), `MailBusy` (4xx) or `Failed`; never put the password in an error.
/// Called from several threads at once.
pub trait MailSender: Send + Sync {
    /// Sends the mail.
    fn send(&self, mail: &MailRequest) -> Result<(), crate::SendError>;
}

#[cfg(feature = "smtp")]
mod real {
    use super::{MailRequest, MailSender};
    use crate::{SendError, config::Security};
    const SERVER_NAME: &str = "Enter the mail server's name, like smtp.example.org.";
    use lettre::message::Mailbox;
    use lettre::message::header::ContentType;
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::transport::smtp::client::{Tls, TlsParameters};
    use lettre::{Message, SmtpTransport, Transport};
    use paddock_net::{Egress, LanAccess, NetError};
    use std::error::Error;
    use std::net::IpAddr;
    use std::sync::Arc;

    /// [`MailSender`] over SMTP with lettre; TLS through rustls and the platform's certificate verifier. SMTP does not go
    /// through an HTTP proxy. It obeys the app's outbound policy like the HTTP channels: before connecting it asks the
    /// [`Egress`] (offline mode refuses with no name resolved and no socket opened; the host must be a public address, or a
    /// private one only for a channel with `allow_lan`, within the policy's reach), and it connects to the **checked address**,
    /// not to the name again, so a name cannot answer differently the second time. The name is still what the certificate is
    /// verified against.
    #[derive(Clone)]
    pub struct LettreSender {
        egress: Arc<Egress>,
    }

    impl std::fmt::Debug for LettreSender {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("LettreSender")
        }
    }

    impl LettreSender {
        /// A sender that asks `egress` (build it with [`channel_policy`](crate::channel_policy)) before every connection.
        pub fn new(egress: Arc<Egress>) -> LettreSender {
            LettreSender { egress }
        }

        /// The addresses the policy allows for the mail's server.
        fn allowed(&self, mail: &MailRequest, host: &str) -> Result<Vec<IpAddr>, SendError> {
            let lan = if mail.lan {
                LanAccess::Policy
            } else {
                LanAccess::Deny
            };
            self.egress
                .check_host(host, mail.port, lan)
                .map_err(|e| match e {
                    NetError::Connect(h) => SendError::Connect(h),
                    other => SendError::Invalid(other.to_string()),
                })
        }
    }

    fn mailbox(address: &str, role: &str) -> Result<Mailbox, SendError> {
        address.trim().parse::<Mailbox>().map_err(|_| {
            SendError::Invalid(format!("The {role} address '{address}' is not valid."))
        })
    }

    impl MailSender for LettreSender {
        fn send(&self, mail: &MailRequest) -> Result<(), SendError> {
            let host = mail.host.trim();
            if host.is_empty() || host.contains(|c: char| c.is_whitespace() || c == '/' || c == '@')
            {
                return Err(SendError::Invalid(
                    "Enter the mail server's name, like smtp.example.org.".into(),
                ));
            }
            if mail.to.is_empty() {
                return Err(SendError::Invalid("There is no recipient.".into()));
            }
            let mut builder = Message::builder()
                .from(mailbox(&mail.from, "sender")?)
                .subject(mail.subject.as_str());
            for to in &mail.to {
                builder = builder.to(mailbox(to, "recipient")?);
            }
            let message = builder
                .header(ContentType::TEXT_PLAIN)
                .body(mail.body.clone())
                .map_err(|e| SendError::Failed(e.to_string()))?;
            let tls = match mail.security {
                Security::None => Tls::None,
                s => {
                    let params = TlsParameters::new(host.to_string())
                        .map_err(|_| SendError::Invalid(SERVER_NAME.into()))?;
                    if s == Security::Implicit {
                        Tls::Wrapper(params)
                    } else {
                        Tls::Required(params)
                    }
                }
            };
            let addrs = self.allowed(mail, host)?;
            let mut last = SendError::Connect(host.to_string());
            for addr in addrs {
                // The connection goes to the checked address; `tls` still names the host for the certificate.
                let mut transport = SmtpTransport::builder_dangerous(addr.to_string())
                    .port(mail.port)
                    .tls(tls.clone())
                    .timeout(Some(mail.step_timeout));
                if let Some(login) = &mail.login {
                    transport = transport.credentials(Credentials::new(
                        login.username.clone(),
                        login.password.expose().to_string(),
                    ));
                }
                match transport.build().send(&message) {
                    Ok(_) => return Ok(()),
                    // Another address of the same name may answer; anything else is the server's answer.
                    Err(e) => match classify(&e, host) {
                        c @ SendError::Connect(_) => last = c,
                        other => return Err(other),
                    },
                }
            }
            Err(last)
        }
    }

    /// Maps a library error to what the user should know.
    fn classify(e: &lettre::transport::smtp::Error, host: &str) -> SendError {
        let mut chain = e.to_string().to_ascii_lowercase();
        let mut source = e.source();
        while let Some(s) = source {
            chain.push(' ');
            chain.push_str(&s.to_string().to_ascii_lowercase());
            source = s.source();
        }
        if e.is_timeout()
            || chain.contains("timed out")
            || chain.contains("temporarily unavailable")
        {
            return SendError::Timeout(host.to_string());
        }
        if e.is_tls() || chain.contains("tls") || chain.contains("certificate") {
            return SendError::Tls(host.to_string());
        }
        if let Some(code) = e.status().map(|c| c.to_string()) {
            return match code.as_str() {
                "530" | "534" | "535" | "454" => SendError::Auth(host.to_string()),
                c if c.starts_with('5') => SendError::MailRefused(format!("code {c}")),
                c => SendError::MailBusy(format!("code {c}")),
            };
        }
        if chain.contains("authentication") || chain.contains("credentials") {
            return SendError::Auth(host.to_string());
        }
        if e.is_client() || e.is_response() || e.is_transport_shutdown() {
            return SendError::Failed("the server's answer was not understood".into());
        }
        SendError::Connect(host.to_string())
    }
}

#[cfg(feature = "smtp")]
pub use real::LettreSender;
