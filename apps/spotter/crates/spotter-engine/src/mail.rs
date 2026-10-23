//! E-mail channel delivery over SMTP (lettre, TLS through rustls and the platform's certificate verifier). The server
//! comes from the settings, the password from the credential store (scope `app`, key `smtp-password`). Every step has a
//! timeout, and every failure is a [`MailError`] whose message says what to check next.
use lettre::message::header::ContentType;
use lettre::message::{Mailbox, Message};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use spotter_core::notify::{is_email_address, SmtpSecurity, SmtpSettings};
use std::time::Duration;

/// The credential store scope of the mail password.
pub const PASSWORD_SCOPE: &str = "app";
/// The credential store key of the mail password.
pub const PASSWORD_KEY: &str = "smtp-password";
/// How long one step of the conversation with the server (connect, each command) may take.
pub const STEP_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a whole delivery may take.
pub const TOTAL_TIMEOUT: Duration = Duration::from_secs(45);
/// Longest subject sent, in characters.
const MAX_SUBJECT_CHARS: usize = 200;
/// Longest body sent, in bytes.
const MAX_BODY_BYTES: usize = 64 << 10;

/// Why a message was not delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailError {
    /// The settings or the recipient are not usable.
    Invalid(String),
    /// The account has no password in the credential store.
    NoPassword,
    /// The server could not be reached.
    Connect(String),
    /// The connection could not be secured.
    Tls(String),
    /// The server did not accept the account.
    Auth(String),
    /// The server refused the sender or the recipient.
    Rejected(String),
    /// The server did not answer in time.
    Timeout(String),
    /// Anything else the server or the library reported.
    Failed(String),
}

impl MailError {
    /// What happened and what to do next, for the user.
    pub fn message(&self) -> String {
        match self {
            MailError::Invalid(why) => why.clone(),
            MailError::NoPassword => {
                "The mail account has no password stored. Enter it in Settings, or clear the account name if the server needs none.".into()
            }
            MailError::Connect(host) => format!(
                "{host} could not be reached. Check the server name and port, and that this computer is online (a firewall may block mail ports)."
            ),
            MailError::Tls(host) => format!(
                "A secure connection to {host} could not be set up. Check the security setting: port 587 usually needs STARTTLS and port 465 needs TLS. \
                 Choose \"None\" only for a relay on a trusted network."
            ),
            MailError::Auth(host) => format!(
                "{host} did not accept the account. Check the account name and password; some providers need an app password."
            ),
            MailError::Rejected(why) => format!("The mail server refused the message ({why}). Check the sender and recipient addresses."),
            MailError::Timeout(host) => format!("{host} did not answer in time. Check the server and port, then try again."),
            MailError::Failed(why) => format!("The message could not be sent ({why}). Check the mail settings and try again."),
        }
    }
}

impl std::fmt::Display for MailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for MailError {}

/// A message to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailMessage {
    /// The recipient's address.
    pub to: String,
    /// The subject (control characters are dropped, long ones cut).
    pub subject: String,
    /// The plain-text body.
    pub body: String,
}

/// The mail server with the password to sign in with.
#[derive(Clone)]
pub struct MailAccount {
    /// The server and sender.
    pub settings: SmtpSettings,
    /// The password from the credential store; `None` when the settings name no account.
    pub password: Option<String>,
}

impl std::fmt::Debug for MailAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MailAccount")
            .field("settings", &self.settings)
            .field("password", &self.password.as_ref().map(|_| "***"))
            .finish()
    }
}

fn single_line(text: &str, max_chars: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_string()
}

fn build_message(account: &MailAccount, m: &MailMessage) -> Result<Message, MailError> {
    if !is_email_address(&m.to) {
        return Err(MailError::Invalid(
            "The recipient is not an e-mail address like name@example.org.".into(),
        ));
    }
    let mailbox = |address: &str, role: &str| {
        address
            .trim()
            .parse::<Mailbox>()
            .map_err(|_| MailError::Invalid(format!("The {role} address is not valid.")))
    };
    let mut body = m.body.clone();
    if body.len() > MAX_BODY_BYTES {
        let mut cut = MAX_BODY_BYTES;
        while !body.is_char_boundary(cut) {
            cut -= 1;
        }
        body.truncate(cut);
    }
    Message::builder()
        .from(mailbox(&account.settings.from, "sender")?)
        .to(mailbox(&m.to, "recipient")?)
        .subject(single_line(&m.subject, MAX_SUBJECT_CHARS))
        .header(ContentType::TEXT_PLAIN)
        .body(body)
        .map_err(|e| MailError::Failed(e.to_string()))
}

fn transport(account: &MailAccount, step: Duration) -> Result<AsyncSmtpTransport<Tokio1Executor>, MailError> {
    let s = &account.settings;
    s.check().map_err(MailError::Invalid)?;
    let host = s.host.trim();
    let builder = match s.security {
        SmtpSecurity::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(host),
        SmtpSecurity::StartTls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host),
        SmtpSecurity::None => Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)),
    }
    .map_err(|_| MailError::Invalid("Enter the mail server's name, like smtp.example.org.".into()))?;
    let mut builder = builder.port(s.port).timeout(Some(step));
    if let Some(user) = s.account() {
        let password = account.password.as_deref().filter(|p| !p.is_empty()).ok_or(MailError::NoPassword)?;
        builder = builder.credentials(Credentials::new(user.to_string(), password.to_string()));
    }
    Ok(builder.build())
}

/// Sends `message` through the server in `account`.
pub async fn send(account: &MailAccount, message: &MailMessage) -> Result<(), MailError> {
    send_within(account, message, STEP_TIMEOUT, TOTAL_TIMEOUT).await
}

/// [`send`] with other limits: `step` for each step of the conversation, `total` for the whole delivery.
pub async fn send_within(account: &MailAccount, message: &MailMessage, step: Duration, total: Duration) -> Result<(), MailError> {
    let host = account.settings.host.trim().to_string();
    let built = build_message(account, message)?;
    let transport = transport(account, step)?;
    match tokio::time::timeout(total, transport.send(built)).await {
        Err(_) => Err(MailError::Timeout(host)),
        Ok(Err(e)) => Err(classify(&e, &host)),
        Ok(Ok(_)) => Ok(()),
    }
}

/// Maps a library error to what the user should know.
fn classify(e: &lettre::transport::smtp::Error, host: &str) -> MailError {
    use std::error::Error;
    let mut chain = e.to_string().to_ascii_lowercase();
    let mut source = e.source();
    while let Some(s) = source {
        chain.push(' ');
        chain.push_str(&s.to_string().to_ascii_lowercase());
        source = s.source();
    }
    if e.is_timeout() || chain.contains("timed out") {
        return MailError::Timeout(host.to_string());
    }
    if e.is_tls() || chain.contains("tls") || chain.contains("certificate") {
        return MailError::Tls(host.to_string());
    }
    if let Some(code) = e.status().map(|c| c.to_string()) {
        return match code.as_str() {
            "530" | "534" | "535" | "454" => MailError::Auth(host.to_string()),
            c if c.starts_with('5') => MailError::Rejected(format!("code {c}")),
            c => MailError::Failed(format!("code {c}")),
        };
    }
    if chain.contains("authentication") || chain.contains("credentials") {
        return MailError::Auth(host.to_string());
    }
    if e.is_client() || e.is_response() || e.is_transport_shutdown() {
        return MailError::Failed("the server's answer was not understood".into());
    }
    MailError::Connect(host.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(port: u16, security: SmtpSecurity) -> MailAccount {
        MailAccount {
            settings: SmtpSettings {
                host: "127.0.0.1".into(),
                port,
                security,
                username: None,
                from: "spotter@example.test".into(),
            },
            password: None,
        }
    }

    fn message() -> MailMessage {
        MailMessage {
            to: "me@example.test".into(),
            subject: "Subject".into(),
            body: "€".repeat(30_000),
        }
    }

    #[test]
    fn subjects_lose_control_characters_and_long_bodies_are_cut_on_a_character() {
        assert_eq!(single_line("a\r\nb\tc", 10), "abc");
        assert_eq!(single_line("  padded  ", 10), "padded");
        assert_eq!(single_line(&"x".repeat(500), 200).len(), 200);
        let built = build_message(&account(25, SmtpSecurity::None), &message()).unwrap();
        let text = String::from_utf8_lossy(&built.formatted()).into_owned();
        assert!(text.contains("Subject: Subject"), "{text}");
        let bad = MailMessage {
            to: "x".into(),
            ..message()
        };
        assert!(matches!(
            build_message(&account(25, SmtpSecurity::None), &bad),
            Err(MailError::Invalid(_))
        ));
    }

    #[test]
    fn errors_read_as_advice_and_never_show_the_password() {
        let a = MailAccount {
            password: Some("hunter2".into()),
            ..account(25, SmtpSecurity::None)
        };
        assert!(!format!("{a:?}").contains("hunter2"));
        for e in [
            MailError::NoPassword,
            MailError::Tls("h".into()),
            MailError::Auth("h".into()),
            MailError::Timeout("h".into()),
        ] {
            assert_eq!(e.to_string(), e.message());
            assert!(e.message().len() > 20);
        }
    }

    #[tokio::test]
    async fn an_unreachable_server_and_bad_settings_fail_before_anything_is_sent() {
        let e = send(&account(1, SmtpSecurity::None), &message()).await.unwrap_err();
        assert!(matches!(e, MailError::Connect(_)), "{e:?}");
        let mut a = account(1, SmtpSecurity::Tls);
        a.settings.port = 0;
        assert!(matches!(send(&a, &message()).await.unwrap_err(), MailError::Invalid(_)));
        let mut a = account(1, SmtpSecurity::StartTls);
        a.settings.username = Some("u".into());
        assert_eq!(send(&a, &message()).await.unwrap_err(), MailError::NoPassword);
    }

    #[tokio::test]
    async fn a_silent_server_hits_the_total_limit() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let _keep = tokio::spawn(async move {
            let _held = listener.accept().await;
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let e = send_within(
            &account(port, SmtpSecurity::None),
            &message(),
            Duration::from_secs(5),
            Duration::from_millis(300),
        )
        .await
        .unwrap_err();
        assert!(matches!(e, MailError::Timeout(_)), "{e:?}");
    }
}
