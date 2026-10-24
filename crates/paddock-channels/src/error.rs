//! Why a send failed.

use std::fmt;
use std::time::Duration;

/// Why a delivery failed. Every message says what happened and what to check, and none contains a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// The channel needs a token or password that the credential store does not hold.
    MissingSecret {
        /// The channel's name (or id).
        channel: String,
    },
    /// The credential store could not be read.
    SecretStore(String),
    /// The configuration or the message cannot be sent (bad address, bad URL, unusable header).
    Invalid(String),
    /// The server could not be reached (the host).
    Connect(String),
    /// The secure connection could not be set up (the host).
    Tls(String),
    /// The proxy failed.
    Proxy(String),
    /// The mail server did not accept the account (the host).
    Auth(String),
    /// The server did not answer in time (the host).
    Timeout(String),
    /// The HTTP server answered with a status other than 2xx (redirects count: they are never followed).
    Rejected {
        /// The status.
        status: u16,
        /// How long the server asked to wait (`Retry-After`), capped by the dispatcher.
        retry_after: Option<Duration>,
    },
    /// The mail server refused the sender, a recipient or the message for good (5xx).
    MailRefused(String),
    /// The mail server answered with a temporary refusal (4xx); another attempt may work.
    MailBusy(String),
    /// Anything else the library reported.
    Failed(String),
    /// Stopped on request.
    Cancelled,
    /// The channel code panicked; the app keeps running.
    Panicked,
}

impl SendError {
    /// Whether another attempt can change the outcome: the host was not reachable or did not answer, the HTTP server said 408,
    /// 425, 429 or 5xx, or the mail server answered temporarily. Bad configuration, a missing secret, an unverifiable
    /// certificate, a refused login or a 4xx rejection never succeed on a retry.
    pub fn retryable(&self) -> bool {
        match self {
            SendError::Connect(_) | SendError::Timeout(_) | SendError::MailBusy(_) => true,
            SendError::Rejected { status, .. } => {
                matches!(*status, 408 | 425 | 429) || (500..600).contains(status)
            }
            _ => false,
        }
    }

    /// The wait the server asked for, if any.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            SendError::Rejected { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SendError::MissingSecret { channel } => write!(
                f,
                "{channel} needs a token or password that is not stored. Enter it in the channel's settings."
            ),
            SendError::SecretStore(why) => write!(
                f,
                "The credential store could not be read ({why}). Unlock it and try again."
            ),
            SendError::Invalid(why) => f.write_str(why),
            SendError::Connect(host) => write!(
                f,
                "{host} could not be reached. Check the address and port, and that this computer is online (a firewall may block it)."
            ),
            SendError::Tls(host) => write!(
                f,
                "A secure connection to {host} could not be set up. Check the security setting (port 587 usually needs STARTTLS, 465 needs TLS) and the computer's clock."
            ),
            SendError::Proxy(why) => {
                write!(f, "The proxy failed ({why}). Check the proxy settings.")
            }
            SendError::Auth(host) => write!(
                f,
                "{host} did not accept the account. Check the account name and password; some providers need an app password."
            ),
            SendError::Timeout(host) => write!(
                f,
                "{host} did not answer in time. Check the server and port, then try again."
            ),
            SendError::Rejected { status, .. } => write!(
                f,
                "The server rejected the message (HTTP {status}). Check the address and the token."
            ),
            SendError::MailRefused(why) => write!(
                f,
                "The mail server refused the message ({why}). Check the sender and recipient addresses."
            ),
            SendError::MailBusy(why) => write!(
                f,
                "The mail server could not take the message now ({why}). It will be tried again."
            ),
            SendError::Failed(why) => {
                write!(
                    f,
                    "The message could not be sent ({why}). Check the settings and try again."
                )
            }
            SendError::Cancelled => f.write_str("Stopped."),
            SendError::Panicked => {
                f.write_str("The channel failed unexpectedly; the app keeps running.")
            }
        }
    }
}

impl std::error::Error for SendError {}
