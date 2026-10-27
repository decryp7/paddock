//! Email channel over SMTP (STARTTLS or implicit TLS), built on `lettre`.

use crate::dispatch::Channel;
use crate::model::{ChannelConfig, Message, SecretStore, SendError, Tls, Transport};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls as LettreTls, TlsParameters};
use lettre::{Message as Mail, SmtpTransport, Transport as _};
use std::time::Duration;

/// Sends the message as a plain-text email.
pub(crate) struct EmailChannel {
    cfg: ChannelConfig,
    timeout: Duration,
    /// Test hook: connect without TLS to a local fake server.
    plaintext: bool,
}

impl EmailChannel {
    pub(crate) fn new(cfg: &ChannelConfig, timeout: Duration) -> Self {
        Self {
            cfg: cfg.clone(),
            timeout,
            plaintext: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn plaintext(mut self) -> Self {
        self.plaintext = true;
        self
    }
}

fn smtp(e: impl std::fmt::Display) -> SendError {
    SendError::Smtp(e.to_string())
}

impl Channel for EmailChannel {
    fn id(&self) -> &str {
        &self.cfg.id
    }

    fn send(&self, msg: &Message, secrets: &dyn SecretStore) -> Result<(), SendError> {
        let Transport::Email {
            host,
            port,
            tls,
            username,
            secret_key,
            from,
            to,
        } = &self.cfg.transport
        else {
            return Err(SendError::Invalid("not an email channel".to_string()));
        };
        let password = secrets
            .get(secret_key)
            .ok_or_else(|| SendError::MissingSecret(secret_key.clone()))?;
        let mut builder = Mail::builder()
            .from(
                from.parse()
                    .map_err(|_| SendError::Invalid(format!("sender '{from}' is not a valid address")))?,
            )
            .subject(msg.title.replace(['\r', '\n'], " "));
        for addr in to {
            builder = builder.to(addr
                .parse()
                .map_err(|_| SendError::Invalid(format!("recipient '{addr}' is not a valid address")))?);
        }
        let mail = builder.body(msg.body.clone()).map_err(smtp)?;

        let tls_mode = if self.plaintext {
            LettreTls::None
        } else {
            let params = TlsParameters::new(host.clone()).map_err(smtp)?;
            match tls {
                Tls::StartTls => LettreTls::Required(params),
                Tls::Implicit => LettreTls::Wrapper(params),
            }
        };
        let transport = SmtpTransport::builder_dangerous(host.as_str())
            .port(*port)
            .tls(tls_mode)
            .timeout(Some(self.timeout))
            .credentials(Credentials::new(username.clone(), password))
            .build();
        transport.send(&mail).map(|_| ()).map_err(|e| {
            let text = e.to_string();
            if text.to_lowercase().contains("timed out") {
                SendError::Timeout
            } else if e.is_transient() || e.is_permanent() {
                smtp(text)
            } else {
                SendError::Connect(text)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::model::{MemorySecrets, Severity};
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    /// A scripted SMTP server accepting one mail; returns the client's lines.
    fn fake_smtp(reject_rcpt: bool) -> (u16, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let mut w = s.try_clone().unwrap();
            let mut r = BufReader::new(s);
            let mut seen = Vec::new();
            let mut in_data = false;
            let _ = w.write_all(b"220 fake ESMTP\r\n");
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let line = line.trim_end().to_string();
                seen.push(line.clone());
                if in_data {
                    if line == "." {
                        in_data = false;
                        let _ = w.write_all(b"250 queued\r\n");
                    }
                    continue;
                }
                let up = line.to_uppercase();
                let reply: &[u8] = if up.starts_with("EHLO") {
                    b"250-fake\r\n250-AUTH PLAIN LOGIN\r\n250 8BITMIME\r\n"
                } else if up.starts_with("AUTH") {
                    b"235 ok\r\n"
                } else if up.starts_with("RCPT") && reject_rcpt {
                    b"550 no such user\r\n"
                } else if up.starts_with("DATA") {
                    in_data = true;
                    b"354 go\r\n"
                } else if up.starts_with("QUIT") {
                    let _ = w.write_all(b"221 bye\r\n");
                    break;
                } else {
                    b"250 ok\r\n"
                };
                let _ = w.write_all(reply);
            }
            seen
        });
        (port, h)
    }

    fn channel(port: u16) -> EmailChannel {
        let cfg = ChannelConfig {
            id: "m".into(),
            name: String::new(),
            enabled: true,
            events: vec![],
            min_severity: Severity::Info,
            transport: Transport::Email {
                host: "127.0.0.1".into(),
                port,
                tls: Tls::StartTls,
                username: "bot".into(),
                secret_key: "pw".into(),
                from: "bot@example.com".into(),
                to: vec!["a@example.org".into(), "b@example.org".into()],
            },
        };
        EmailChannel::new(&cfg, Duration::from_secs(5)).plaintext()
    }

    fn message() -> Message {
        Message {
            title: "Disk low".into(),
            body: "C: has 2 GB left".into(),
            severity: Severity::Error,
            kind: "diskLow".into(),
            source: "r".into(),
        }
    }

    #[test]
    fn smtp_conversation_authenticates_and_delivers() {
        let (port, server) = fake_smtp(false);
        let ch = channel(port);
        assert_eq!(ch.id(), "m");
        assert_eq!(ch.send(&message(), &MemorySecrets::new().with("pw", "pw-value")), Ok(()));
        let seen = server.join().unwrap();
        let all = seen.join("\n");
        assert!(seen[0].to_uppercase().starts_with("EHLO"), "{all}");
        assert!(seen.iter().any(|l| l.to_uppercase().starts_with("AUTH PLAIN")), "{all}");
        assert!(all.contains("MAIL FROM:<bot@example.com>"), "{all}");
        assert!(
            all.contains("RCPT TO:<a@example.org>") && all.contains("RCPT TO:<b@example.org>"),
            "{all}"
        );
        assert!(all.contains("Subject: Disk low"), "{all}");
        assert!(all.contains("C: has 2 GB left"), "{all}");
        assert!(seen.iter().any(|l| l.to_uppercase().starts_with("QUIT")), "{all}");
    }

    #[test]
    fn smtp_rejection_is_reported() {
        let (port, server) = fake_smtp(true);
        let r = channel(port).send(&message(), &MemorySecrets::new().with("pw", "x"));
        assert!(
            matches!(r, Err(SendError::Smtp(ref m)) if m.contains("550") || m.to_lowercase().contains("no such")),
            "{r:?}"
        );
        let _ = server.join();
    }

    #[test]
    fn non_email_transport_is_invalid() {
        let mut ch = channel(1);
        ch.cfg.transport = Transport::Webhook {
            url: "https://a.com".into(),
            secret_key: None,
        };
        assert!(matches!(ch.send(&message(), &MemorySecrets::new()), Err(SendError::Invalid(_))));
    }

    #[test]
    fn tls_modes_fail_cleanly_against_a_non_tls_server() {
        for tls in [Tls::StartTls, Tls::Implicit] {
            let (port, server) = fake_smtp(false);
            let mut ch = channel(port);
            ch.plaintext = false;
            if let Transport::Email { tls: t, .. } = &mut ch.cfg.transport {
                *t = tls;
            }
            ch.timeout = Duration::from_secs(2);
            assert!(ch.send(&message(), &MemorySecrets::new().with("pw", "x")).is_err());
            drop(server);
        }
    }
}
